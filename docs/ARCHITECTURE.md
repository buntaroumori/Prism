# Prism — Architecture

> Status: persistence, IPC, logging, shell (FASE 1) and the complete audio engine (FASE 2) are
> implemented. Catalogue workers (FASE 3), the full interface (FASE 4) and the plugin runtime
> (FASE 5) are specified here as the binding target architecture — every later module lands against
> these contracts rather than improvising around them.

## 1. Process and thread model

A single native process hosts everything; there is no sidecar binary to keep alive or supervise.

| Thread              | Ownership                                              | Rules                                                                                              |
| :------------------ | :----------------------------------------------------- | :------------------------------------------------------------------------------------------------- |
| **UI / event loop** | Tauri, WebView, window management                      | Never touches audio buffers; commands are `async` and return quickly                               |
| **Command workers** | tokio multi-thread runtime                             | All SQL, filesystem and HTTP work; bounded by the pool                                             |
| **Scanner**         | dedicated rayon pool                                   | `walkdir` producer → parallel metadata readers → single SQL writer                                 |
| **Supervisor**      | one thread, `prism` names it                           | Owns the decoder, the DSP chain, the playhead and the device handle; the only writer of the ring   |
| **Prefetch worker** | one thread (`prism-prefetch`)                          | Prepares track _N+1_ while track _N_ plays (gapless)                                               |
| **Audio callback**  | OS real-time thread owned by cpal/WASAPI/ASIO/PipeWire | **No allocation, no locks, no syscalls, no logging.** Only ring-buffer reads and format conversion |
| **Discord RPC**     | tokio task                                             | Never blocks playback; failures are retried with exponential backoff                               |
| **Plugin host**     | sandboxed Lua / wasmtime instance                      | Event-driven, CPU- and memory-budgeted, cannot touch the filesystem directly                       |

The audio callback communicates with the rest of the system exclusively through a pre-allocated
`ringbuf` (power-of-two, lock-free SPSC) and atomics. Any error inside the callback sets a flag and
the supervisor tears down/rebuilds the device on a normal thread — the process never aborts because
a DAC disappeared.

## 2. Audio output layer

### Backends

| Platform | Backend           | Mode              | Implementation                                                                                                                                 | Exclusive |
| :------- | :---------------- | :---------------- | :--------------------------------------------------------------------------------------------------------------------------------------------- | :-------- |
| Windows  | `WasapiExclusive` | Exclusive         | `windows` 0.58 COM: `IMMDeviceEnumerator` → `IAudioClient::Initialize` with `AUDCLNT_SHAREMODE_EXCLUSIVE`, event-driven (`hnsPeriodicity = 0`) | yes       |
| Windows  | `WasapiShared`    | Shared            | cpal WASAPI host; the graph format is the device format                                                                                        | no        |
| Windows  | `Asio`            | Native            | cpal ASIO host behind the opt-in `asio` feature (needs the Steinberg SDK)                                                                      | yes       |
| Windows  | `CpalShared`      | Shared            | System default host, last-resort fallback                                                                                                      | no        |
| Linux    | `AlsaHw`          | Raw `hw:`         | `alsa` 0.9 `snd_pcm_writei` loop on a dedicated thread; dmix and PulseAudio are bypassed                                                       | yes       |
| Linux    | `PipeWire`        | Native graph node | `pw_stream` with `RT_PROCESS`, format negotiation through the SPA pod                                                                          | no        |
| Linux    | `CpalShared`      | Shared            | cpal ALSA host on `default`                                                                                                                    | no        |

`OutputMode` (`Auto`, `Exclusive`, `Shared`, `Asio`) is a _preference_, not a promise. `open_sink`
walks the candidate backends in order and returns the first one that negotiates; `Auto` therefore
degrades from exclusive to mixed output on its own, and a configuration error never leaves the user
without sound. Selecting an endpoint pins its driver stack: an `alsa-hw:` id never silently opens
PipeWire.

The application itself requests `Shared`. Coexisting with every other sound source is a product
requirement, not a preference, so there is no engine-mode switch in the interface; the exclusive
backends remain compiled and reachable for diagnostics and future endpoints.

Shared output asks a second question, and since 0.1.3 it is the user's: **what rate should the
endpoint run at** (`RatePolicy`, Settings → Output).

- `MatchSource` (the default) hands the backend the _record's_ format. An endpoint free to follow —
  ALSA `hw:`, PipeWire, an exclusive stream — is then opened at the file's own rate and nothing
  converts it. A boundary between two rates reopens the device, but only after the endpoint has
  proved once that it grants what it is asked for (`follows_source_rate`).
- `Fixed` asks the driver for its live configuration and converts every track into it.

Whatever is requested, the answer is what matters. `negotiator` records the format the driver
actually granted, and when that is not the source rate the DSP chain resamples into it. Windows
shared mode is pinned to the system mix format and will refuse a 44.1 kHz request on a 24/192
endpoint; the engine sees the refusal, keeps converting, and does not reopen the device at every
track. Requesting the source format _without_ that check is what once made a 44.1 kHz file play
through the mixer as if it were broken while the badge blamed the endpoint.

### Negotiation and commit

Opening a device is split in two so the DAC never starts on an empty buffer:

1. **`negotiate(device, desired_format, buffer_frames)`** answers with a `Negotiation`: the concrete
   endpoint, the exact format the clock will run at, the period size, the ring capacity and the
   driver latency. Exclusive backends honour the requested format or fail; mixed backends are
   allowed to answer with their graph format.
2. The supervisor creates the lock-free ring at `Negotiation::format.channels`, builds the
   `GaplessPlayer` and the DSP chain for that exact output format, and primes several periods.
3. **`start(negotiation, consumer, refill, stats)`** commits: it spawns the real-time thread and
   returns an `OutputSink`. Only then does the supervisor call `OutputSink::start`.

### Synchronisation

The device thread pulls from a `ringbuf` SPSC ring of interleaved `f32` frames at the device rate.
It never locks, allocates, syscalls or logs. When it consumes a buffer it calls
`RefillSignal::request()`, which sets a flag and unparks the pump thread — `Thread::unpark`
allocates nothing and never blocks, so it is safe inside a real-time callback. Requests coalesce,
so a burst of wake-ups costs one pump iteration.

The same object carries liveness: a driver that dies (WASAPI `AUDCLNT_E_DEVICE_INVALIDATED`,
ALSA `Disconnected`, PipeWire `StreamState::Error`) calls `RefillSignal::close()`. The supervisor
sees `!is_open()` on its next pump, tears the endpoint down and rebuilds it — the process never
aborts because a DAC disappeared, and the queue is never cleared.

### Format renegotiation

An exclusive endpoint owns the clock, so a mixed-rate queue is played bit-perfectly by reopening
the device at every rate change. The supervisor detects the boundary inside a decoded block, keeps
the outgoing track's tail, drops at most one period of the incoming track, and rebuilds the device
and player at the new native rate.

A shared endpoint follows the same path when — and only when — it granted the source rate on the
previous open, and only for a _rate_ change: channel and container changes are converted instead,
because reopening the graph for them buys nothing audible and costs a gap. Endpoints locked to the
graph rate never reopen mid-queue and the DSP chain resamples, which is why `bitPerfect` is false
there by construction.

The blocks the pump writes must also fit: a decoded chunk larger than the ring's free space used to
be truncated, which is a hole in the waveform at every fill. `fill_once` now carries the remainder
(`carry`), reads the input proportional to the frames still missing
(`DspChain::input_frames_for`) and converts in 256-frame chunks, so a 44.1 → 192 kHz stream has no
discontinuity at any block boundary. The resampler's own continuity is pinned by
`a_tone_survives_any_chunking_without_a_step`; the end-to-end frame count by
`a_resampled_track_reaches_the_device_whole`.

## 3. Bit-perfect path

`file bytes → decoder → DSP chain (bypassable) → ring → endpoint`

`EngineStatus::bit_perfect` is true only when **all** of the following hold, checked live:

- the sink is an exclusive backend (`WasapiExclusive`, `Asio`, `AlsaHw`);
- no DSP stage is active: no EQ band, no ReplayGain mode, no crossfeed;
- no resampling is engaged (`source.rate == device.rate`);
- the volume is exactly 0 dB and the output is not muted (volume is applied by the pump, so any
  attenuation is by definition a bit change);
- `source.sample_format == device.sample_format` and the channel counts match;
- for DoP, the carrier rate equals the device rate and the marker bytes reach the DAC untouched.

The engine never claims bit-perfect playback it cannot prove. When the device refuses the exact
format, the status reports the negotiated format and the UI shows the mismatch.

DSD is transmitted as DoP: 16-bit groups packed into 24-bit PCM words with the `0x05`/`0xFA`
markers alternating every frame, carried at 176.4 kHz (DSD64), 352.8 kHz (DSD128), 705.6 kHz
(DSD256) and 1411.2 kHz (DSD512). DSD always takes the bypass path — any gain or filter would
corrupt the markers and the DAC would mute.

## 4. Gapless playback and RAM Play

`GaplessPlayer` owns a bounded prefetch channel and a worker thread that prepares track _N+1_ while
track _N_ is audible:

- `prepare_track` opens the file, builds the DSP chain for the target format and pre-rolls the first
  `PRIME_FRAMES` so the boundary lands on real samples, not on a decoder warm-up;
- `next_chunk(frames)` returns interleaved `f32` plus the annotations the supervisor needs:
  `ended`, `started` and `boundary_offset_frames` — the sample-exact offset inside the block where
  the new track begins;
- a `delivered`/`planned` pair of counters stops the player from waiting for a track that does not
  exist, which is what makes the tail of a queue end silently instead of blocking for 30 s.

Track boundaries are sample-accurate: the sum of the decoded frames of two consecutive tracks equals
the frames the player produced, with no silence inserted anywhere. CUE virtual tracks share a single
decode context, so cue boundaries are gap-free too.

**RAM Play** decodes a whole track into one contiguous allocation before playback starts. Files
larger than `ram_limit_bytes` are streamed instead, and the fallback is logged rather than silently
degrading the gapless guarantee.

## 5. DSP graph

`decode → [ReplayGain 2] → [rubato resampler] → [parametric EQ] → [crossfeed] → ring → [gain ramp]`

- **Chain compilation**: `DspChain::build` is called once per (settings, source, output) triple and
  compiles out inactive stages entirely — a bypassed chain returns the input buffer unchanged.
- **Volume** is _not_ part of the chain: the pump applies a per-block linear ramp so a slider drag
  never rebuilds a filter graph and never clicks. `DspSettings::volume_db` therefore stays at 0 dB
  for chain consumers, and the engine owns the value.
- **ReplayGain 2 / EBU R128**: track or album gain with a peak-aware guard; measured by
  `LoudnessMeter`, whose K-weighting reproduces the BS.1770-4 reference filters exactly at 48 kHz
  (coefficients from De Man's analogue reconstruction, unnormalised RLB high-pass numerator
  included) and reads a 0 dBFS 997 Hz mono sine as −3.01 LUFS.
- **rubato**: 256-tap sinc interpolation with a Blackman-Harris window, linear phase, anti-aliasing
  on downsampling; the chain reports its latency in output frames so the playhead stays honest.
- **EQ**: up to 31 biquad sections per channel (peaking, low/high shelf, HP/LP), coefficients
  recomputed off the audio thread. Shelves use the RBJ cookbook; the loudness meter deliberately
  does not, see above.
- **Crossfeed**: Bauer (default), Meier and custom delay/gain. The direct path is untouched and only
  the opposite channel is delayed and low-passed, which is what keeps the image stable.

## 6. Library pipeline

```text
library_roots
   │ walkdir (single-threaded, filter_entry prunes VCS/cache dirs)
   ▼
extension filter ─┬─ *.cue ──▶ CueSheet::parse ──▶ virtual tracks + sheet row
                  └─ audio  ──▶ fingerprint check (mtime+size) ──▶ skip
                                     │ miss
                                     ▼
                     bounded per-file lofty read + isolated deadline
                                     │ TrackTags → TrackRecord
                                     ▼
                     batch of 256 rows, one transaction:
                     INSERT OR IGNORE artists → albums → tracks upsert (+FTS triggers)
                                     │
                                     ▼
                     album counters refreshed → missing sweep → CUE↔album link
                                     │
                                     ▼
                     scan_runs ledger, scan_errors rows, progress events (≤ 5 Hz)
```

- **Deterministic ids.** Every row id is `prefix + 96 bits of SHA-256` over a canonical identity:
  `ar:lower(name)`, `al:lower(title)|lower(album_artist)`,
  `tr:lower(normalised_path)|source|cue_start_ms`, `cs:lower(sheet_path)`. Re-scanning, renaming a
  library root or importing a friend's catalogue converges on the same rows, so playlists, ratings
  and play history survive. The birthday bound at 500k tracks is below 1e-17. A MusicBrainz
  recording id is deliberately non-unique metadata: migration `0005` keeps a lookup index but
  leaves physical-file uniqueness to `ux_tracks_identity`.
- **Incremental by default.** A scan reads `(path, mtime, size)` for every indexed file under the
  roots and parses only what changed. `.cue` sheets are compared by SHA-256 of their bytes. A second
  scan of an unchanged 100k library performs no tag reads and no track writes.
- **Parallel parse, serial write.** `lofty` decoding is CPU-bound and each file has its own bounded
  task and deadline, so one malformed or blocking input cannot withhold a whole chunk. `SQLite`
  serialises writers, so the database side stays on the async thread with one transaction per 256
  rows.
- **Album aggregation** is recomputed for touched albums after every batch (`COUNT`, `SUM`,
  `MAX(disc_no)` over the album index), and globally at the end of a full scan. The missing sweep
  re-runs the recomputation for albums that lost a track, so the grid never advertises a track that
  is no longer on disk.
- **Missing ≠ deleted.** A file that disappears is flagged `missing=1`; the row, its rating and its
  play history remain. An unreachable root (unplugged drive, unmounted share) is reported as
  `root_offline` and marks _nothing_ missing. Pruning albums/artists without tracks is an explicit
  opt-in on a full scan.
- **CUE.** Sheets are expanded before audio files so every virtual track's `cue_sheet_id` foreign key
  is satisfiable inside the same batch. The referenced image is indexed once as a sheet and N times
  as virtual tracks (`source='cue'`, `cue_start_ms`/`cue_end_ms`, sample-accurate offsets) and is
  excluded from the standalone-file index. Technical properties (rate, depth, DSD family) are read
  from the image; per-track ReplayGain is deliberately left null because an album-wide gain does not
  apply to a single movement.
- **Error isolation.** A corrupt file produces one `scan_errors` row with a stable code (`io`,
  `unsupported`, `malformed`, `cue`, `root_offline`) and the walk continues. The ledger is capped at
  200 stored errors per run while the counter keeps counting.
- **Cancellation** is cooperative: the flag is checked at every walk entry and every parse chunk.
  Committed batches stay committed; the run is recorded as `cancelled`.
- **Targets**: 100k FLAC metadata reads in < 10 s on warm NVMe. Parsing is I/O + CPU bound; the write
  side is ~1.5k rows/s per commit on the reference laptop.

## 7. Persistence and query surface

- WAL, `synchronous=NORMAL`, `mmap_size=256 MiB`, 32 MiB page cache, 5 s busy timeout, 6 connections.
- Migrations are embedded (`sqlx::migrate!`), applied at boot inside a transaction; the schema version
  is part of the health payload.
- `tracks` is deliberately wide and denormalised: the library grid renders artist/album/quality
  directly, and triggers keep the copies coherent when an artist or album is renamed.
- **FTS5** is external content (`content='tracks'`) with insert/delete/update triggers, tokenised by
  `unicode61 remove_diacritics 2` so "Beyonce" finds "Beyoncé".
- **Query building.** User text is tokenised to runs of alphanumerics, each quoted as a phrase, with a
  prefix operator on the final token (`beeth` → `"beeth"*`). FTS5 operators (`-`, `:`, `*`, `^`,
  `NEAR`, `AND`/`OR`/`NOT`) can therefore never reach the grammar — `C++` is a search, not a syntax
  error. Ranking is `bm25` with weights 10/6/4/3/2/1/1 over title, artist, album, album artist,
  genre, comment, composer. Snippets use `U+0001`/`U+0002` as highlight markers so React can render
  them through JSX without `dangerouslySetInnerHTML`.
- **Browse** never touches the FTS index: it walks the covering indexes (`ix_tracks_title_sort`,
  `ix_tracks_album`, …), and every `ORDER BY` comes from a closed enum with a primary-key tie-breaker
  for stable offset pagination.
- **Read models**: `library_search` (filters + sort + paging), `library_albums`, `library_artists`,
  `library_facets` (genres/formats/decades) and `library_global_search` (tracks + albums + artists in
  one round trip for the ⌘K overlay).
- VectorClock-style conflict resolution is unnecessary (single user, single process); concurrent
  edits are serialised by SQLite.
- Backup/export: a consistent copy is produced with `VACUUM INTO` under a read transaction; the same
  file format is the import format.

## 8. Cover art and Discord Rich Presence

**Cover cache.** The key is `SHA-256(lower(artist) + U+001F + lower(album))`, which is exactly the
`cover_cache` primary key. The separator prevents `("AB","C")` and `("A","BC")` from colliding.

1. Cache hit with a live TTL and an existing file → returned without touching a provider.
2. Miss → provider chain: embedded picture of the first track (front cover preferred, largest by byte
   size) → `cover.jpg`/`folder.png`/`front.*` in the album folder → iTunes Search API (fuzzy title and
   artist match, 100 px URL upscaled to 600) → MusicBrainz release search (score ≥ 70, title match)
   plus Cover Art Archive `front-500`.
3. Every image is decoded, bounded to 1000 px on the longest edge and re-encoded as JPEG q88 before it
   is written to `<cache>/artwork/<key>.jpg`. A 12 MP gatefold scan becomes a ~90 KB tile.
4. Results live for 90 days; failures are cached as `provider='unknown'` for 24 h so a library of
   bootlegs does not hammer iTunes on every scroll. Concurrent lookups for the same album are
   collapsed by a per-key async mutex.
5. MusicBrainz is rate-limited to one request per second with a descriptive `User-Agent`, as its terms
   require. Offline mode skips network providers entirely.

**Rich presence.** `DiscordService` subscribes to the engine broadcast channel (not to React), so
minimising to the tray keeps Discord accurate. It publishes the textual activity immediately and the
artwork as a second update once the cache answers, because Discord only accepts HTTPS URLs for
external assets — a locally extracted cover therefore still leaves `large_image` on the bundled
`prism` asset. The RPC client speaks the real IPC framing (`op:u32 LE, len:u32 LE, JSON`), probes
`discord-ipc-0…9` across native, Flatpak and Snap runtime directories (named pipes on Windows),
answers pings, and reconnects with 1.5× exponential backoff capped at 60 s plus jitter. Updates are
coalesced to at most one every 2.5 s. The application id comes from `PRISM_DISCORD_CLIENT_ID` or from
Settings; with no id the feature stays off rather than pretending to work.

## 9. Plugin runtime

Two opt-in sandboxes behind Cargo features (`lua-plugins`, `wasm-plugins`); the default build ships
neither, and Settings says so instead of failing at load time.

- **Lua 5.4** (`mlua`, vendored — no system Lua) and **WebAssembly** (`wasmtime` 26, Cranelift, no
  WASI). One interpreter/store per plugin, never shared.
- **Capabilities are declared, not implied.** `log`, `notify`, `player:read` and `player:control` are
  listed in `plugin.toml`; a host call without its capability fails closed with an error that names
  the missing one. There is no filesystem, process or network handle in either sandbox.
- **Budgets:** Lua gets 16 MiB of memory and 20 M VM instructions per call (checked every 10 000 by a
  VM hook); WebAssembly gets 64 MiB of linear memory and 200 M fuel per call. Exceeding either raises
  a normal error, so a runaway plugin cannot hang the dispatcher — measured by tests that abort a
  `while true do end` plugin and a `(loop (br 0))` module.
- **Failure policy:** a plugin that errors is disabled for the session, and the reason is stored on
  its descriptor for the settings panel. Playback never depends on a plugin.
- **Dispatch:** the same engine broadcast that feeds Discord presence also feeds the plugin manager,
  so plugins see transport truth even when the window is hidden.
- **Storage:** `settings` rows `plugins.disabled` and `plugin.config.<id>`; configuration is validated
  against the manifest schema on write and reconciled on load (a plugin update can change its own
  schema without stranding the user on an invalid value).

DSP-block processing and `library.read`/`network` capabilities are planned for the plugin API v2; v1
covers what is implemented above, and nothing in the UI claims otherwise.

## 10. Frontend rendering

### 10.1 State topology

Five stores, split by update frequency — the split _is_ the performance design:

| Store                  |      Updated at | Contents                                            |
| :--------------------- | --------------: | :-------------------------------------------------- |
| `uiStore`              |     user action | route, sidebar width, ambient intensity (persisted) |
| `outputStore`          |     user action | engine mode, device id, buffer size (persisted)     |
| `libraryStore`         |  scan / refresh | counters, roots, recent rail, scan state machine    |
| `playbackStore.status` | transport event | engine snapshot: state, track, formats, bit-perfect |
| `playbackStore.tick`   |        **8 Hz** | `{ positionMs, durationMs, playing }`               |

The playhead lives in its own slice and its own event channel (`player://tick`,
dispatched by Rust at 125 ms). Only the scrubber subtree subscribes to it; the
cover, the quality badge, the device selector and the whole library never see a
playhead update. A single store would re-render the tree eight times a second
for two numbers.

### 10.2 Palette pipeline

1. `artwork` resolves the cover and writes `<cache>/artwork/<sha256>.jpg`.
2. `palette::PaletteService` decodes it on the blocking pool, downsamples to
   128 px, runs deterministic median cut (`color_thief`), re-counts the sampled
   pixels to recover each swatch's true share, and scores candidates by
   `saturation^1.15 × mid-lightness bell × prominence^0.5`.
3. The accent is tone-mapped (saturation 0.46–0.92, value 0.66–0.88) and then
   lightened in 2 % steps until WCAG 2.1 contrast against the generated page
   background clears **4.5:1**. Background and surface are the same hue at
   7.5 % and 12.5 % value, so the chrome stays OLED-dark whatever the artwork.
4. Greyscale art, a corrupt file or a missing cover falls back to a palette
   _derived from the cache key_ (FNV-1a → hue), so the same album is always the
   same colour across restarts, and an outage never changes the interface.
5. The answer is memoised (512 entries in Rust, `Map` in the renderer) and
   applied as **six CSS custom properties on the shell root**. Components read
   `var(--prism-accent)`; a track change writes six strings and the compositor
   does the rest. No per-frame JS, no canvas loop.

The ambient canvas itself is three blurred orbs animated with a
`transform`-only keyframe; intensity is scaled by the artwork's mean luminance so
a white sleeve does not wash out the window. `prefers-reduced-motion` and the
persisted "Off" setting both collapse it to flat black.

### 10.3 Shell and motion

- The player bar is a flex sibling of the scroller, not an overlay: content can
  never be hidden behind it, and the reserved height means the bar's entrance
  cannot shift the page (CLS = 0).
- Framer Motion is used for four things only: page cross-fade (180 ms,
  opacity + 6 px), the player-bar slide-in, the segmented-control indicator
  (`layoutId`) and the album cross-fade in Now Playing. Everything continuous is
  CSS/compositor-only.
- The shell renders one of two things, never a half-state: the desktop shell
  when `__TAURI_INTERNALS__` exists, or a `DesktopRequired` screen explaining
  that playback and scanning live in the Rust process. There is no browser mode
  with fake data.
- **The interface measures its own compositor.** `useFrameBudget` samples 45
  frames once, 1.5 s after the first paint; a median above 24 ms means the
  expensive layers are paid for on the CPU, so the shell sets
  `data-effects="reduced"` and paints flat surfaces instead of backdrop filters,
  drifting orbs and grain. The preference (`auto` / `full` / `reduced`) lives in
  Appearance. This is what separates "fluid" from "torpid" on the same build
  when one machine has a GL driver and the other composites in software.

### 10.4 Contracts

Every Rust → TS boundary is a zod schema (`src/types/ipc.ts`), including the
engine event union (internally tagged, discriminated on `type`) and the
`player://tick` payload. A malformed event is dropped and logged, never written
into a store; a malformed command result surfaces as a `contract` failure.

### 10.5 The 500 000-row list

The list is **paged, never materialised**. `useTrackWindow` owns a cache object per query
(`pages`, `inflight`, `center`), requests 500-row pages — the backend's maximum — for the indices the
virtualiser is about to paint, and keeps twelve pages (6 000 rows, ~3 MB), evicting the ones farthest
from the viewport first. A new query swaps the cache object during render, so a response that arrives
late lands in a cache nobody reads instead of overwriting the current results.

`@tanstack/react-virtual` maps scroll position to indices; rows are a fixed 52 px, positioned with a
single `translateY`, overscanned by 12 and contained with `contain: strict`. The DOM holds 30–60 rows
whatever the library size. Skeleton rows use the same height, so a page arriving never moves the list
(CLS = 0). "Play from here" queues the page that contains the row, because a 500 000-entry queue
would pin the whole catalogue in the engine's memory and in IPC.

Two properties are load-bearing and both were found by measurement, not by reading the docs:

1. **The virtualiser's option callbacks are module-level.** `estimateSize`, `getItemKey` and
   `getScrollElement` are compared by identity inside the library's memoised measurement cache. Inline
   arrows are new objects on every render, so the cache was rebuilt over 500 000 rows on every scroll
   frame — 21–30 % of all JS samples inside `getMeasurements`, 18.9 ms of main-thread work per frame.
   Stable callbacks cost 3.0 ms.
2. **The first page is fetched by the hook, not by the list.** A virtualiser with `count: 0` renders
   nothing, so a hook that only fetches once it knows the total never fetches at all. `useTrackWindow`
   bootstraps page 0 per query generation, guarded on the page cache so an empty result is requested
   once.

Rows are `memo` components that subscribe to `playbackStore` through boolean selectors, so a track
change re-renders two rows rather than the list, and scrolling re-renders none.

### 10.6 Spectrum path

```
decoder → DSP → apply_gain → [Analyzer::push] → ring → device callback
```

The FFT runs in Rust on the supervisor's **producer** thread, on the block that was just decoded —
never a second read of the file, never in the device callback. 2 048 points, Hann window, 31
log-spaced bands from 20 Hz to 0.98 × Nyquist, peak per band, −80 dBFS floor, attack 0.55 / release
0.14. A sample-rate change invalidates the window rather than reinterpreting old samples at a new
clock.

The webview receives `visualiser://frame` at 60 Hz (~450 bytes) and paints a canvas on
`requestAnimationFrame`, interpolating between frames: on-screen motion runs at the display rate
(120 Hz included) while the IPC rate stays at 60. The tap is reference-counted in the frontend, so an
unmounted visualiser costs the audio thread nothing. Frames never touch React state.

### 10.7 Lyrics

Embedded `USLT` → sidecar `.lrc` → empty, with a miss never cached (adding a `.lrc` and pressing
refresh always shows it) and sidecar rows re-validated against their path. The parser accepts the
dialects that exist in the wild: 1–3 fraction digits, `mm:ss:ff`, several timestamps per line,
metadata tags, BOM/CRLF, unsorted input and `[offset:]` in both signs (positive delays). The karaoke
cursor is a binary search over timestamps on every playhead tick; manual scrolling suspends
auto-follow for four seconds.

### 10.8 Modular Now Playing

`useLayoutStore` holds an ordered module list (`artwork`, `lyrics`, `spectrum`, `technical`, `queue`)
with three presets, persisted and migrated. Editing a preset makes the layout `custom`; at least one
module always remains. Modules are real panels with their own data source, so removing one removes its
work — a hidden spectrum releases the audio tap, a hidden lyrics panel stops fetching. Reordering
animates with Framer Motion `layout` (transform-only) and honours `prefers-reduced-motion`.

## 11. Failure injection and measurement

Everything in §1–§10 is only as good as the tests that break it on purpose.

- **Device seam.** `Engine::with_opener` takes the same `Fn(OutputMode, Option<&str>, &AudioFormat,
u32) -> Result<OpenedSink>` that the production build points at `backend::open_sink`. The stress
  suite replaces it with a loopback that consumes the same ring, honours the same `RefillSignal` and
  can be unplugged on demand, so hot-swap, device loss and recovery are exercised end to end on a
  machine with no sound card.
- **Skipped tracks.** A file that cannot be opened is a `SkippedTrack`, not an error: the decoder
  records it, the supervisor drains it into `EngineEvent::TrackSkipped`, the plugin bridge reports
  `track_skipped`, and the UI shows the file and the reason while the queue keeps playing. A queue
  whose _head_ is unreadable walks forward before opening a device; a queue with nothing playable
  tears down cleanly.
- **Frame budget.** `scripts/perf-audit.mjs` mounts the production list over 500 000 synthetic rows
  and measures renderer main-thread time per painted frame — not frame rate, because a compositor can
  tick at 60 Hz on a machine with several times the budget to spare. Two workloads (a 120 px/frame
  flick and a three-viewport scrub) are reported separately, and seven verdicts gate CI.
- **Memory.** `valgrind --tool=memcheck` over the stress suite and the `audio::*` units; Miri over the
  pure-logic modules in CI. Both are described in `docs/PERFORMANCE.md`, with the numbers.
- **Backoff as a value.** `BackoffPolicy` (start, ceiling, growth, jitter) is data, so the Discord
  reconnect loop is tested in milliseconds against a real UNIX socket instead of being described in a
  comment.

## 12. Error taxonomy and observability

- `PrismError` serialises to `{ code, message }`: `database`, `migration`, `io`, `runtime`,
  `install`, `invalid_input`, `internal`. The frontend branches on `code`, never on message text.
- Zod validates every IPC payload; a mismatch surfaces as `contract` instead of corrupting state.
- Logs: stderr + daily files (14-day retention, non-blocking worker). `PRISM_LOG` controls verbosity.
- A diagnostics export bundles app info, pragma-derived database health, recent scan errors and the
  log tail, with paths redacted on request.

## 13. Packaging and delivery

- Tauri v2 bundles: NSIS + WiX (MSI) + portable EXE on Windows; DEB + AppImage on Linux.
- WebView2 uses the download bootstrapper (silent); Linux DEB dependencies use the Ubuntu 24.04
  `t64` names with alternatives for older releases.
- Release CI builds natively on both operating systems, generates `SHA256SUMS.txt`, and publishes
  release notes taken from the curated changelog section with the commit list appended. The full
  procedure, including rollback, lives in `docs/RELEASE.md`.
- The pipeline validates itself before it runs: `scripts/validate_release.py` checks tag/manifest
  agreement, staging coverage against `bundle.targets`, action pinning and the artefacts the release
  publishes. A broken workflow fails on a pull request instead of during a tag.
- Distribution carries its licence obligations: `THIRD-PARTY-LICENSES.md`, generated from the
  lockfiles by `scripts/license-audit.mjs`, is attached to every release alongside the binaries.
- The updater is deliberately deferred until signing keys exist; unsigned auto-updates are a
  supply-chain risk.
