# Prism — Architecture

> Status: persistence, IPC, logging, shell (FASE 1) and the complete audio engine (FASE 2) are
> implemented. Catalogue workers (FASE 3), the full interface (FASE 4) and the plugin runtime
> (FASE 5) are specified here as the binding target architecture — every later module lands against
> these contracts rather than improvising around them.

## 1. Process and thread model

A single native process hosts everything; there is no sidecar binary to keep alive or supervise.

| Thread              | Ownership                                              | Rules                                                                                     |
| :------------------ | :----------------------------------------------------- | :---------------------------------------------------------------------------------------- |
| **UI / event loop** | Tauri, WebView, window management                      | Never touches audio buffers; commands are `async` and return quickly                      |
| **Command workers** | tokio multi-thread runtime                             | All SQL, filesystem and HTTP work; bounded by the pool                                    |
| **Scanner**         | dedicated rayon pool                                   | `walkdir` producer → parallel metadata readers → single SQL writer                        |
| **Supervisor**      | one thread, `prism` names it                           | Owns the decoder, the DSP chain, the playhead and the device handle; the only writer of the ring |
| **Prefetch worker** | one thread (`prism-prefetch`)                          | Prepares track _N+1_ while track _N_ plays (gapless)                                      |
| **Audio callback**  | OS real-time thread owned by cpal/WASAPI/ASIO/PipeWire | **No allocation, no locks, no syscalls, no logging.** Only ring-buffer reads and format conversion |
| **Discord RPC**     | tokio task                                             | Never blocks playback; failures are retried with exponential backoff                      |
| **Plugin host**     | sandboxed Lua / wasmtime instance                      | Event-driven, CPU- and memory-budgeted, cannot touch the filesystem directly              |

The audio callback communicates with the rest of the system exclusively through a pre-allocated
`ringbuf` (power-of-two, lock-free SPSC) and atomics. Any error inside the callback sets a flag and
the supervisor tears down/rebuilds the device on a normal thread — the process never aborts because
a DAC disappeared.

## 2. Audio output layer

### Backends

| Platform | Backend | Mode | Implementation | Exclusive |
| :------- | :------ | :--- | :------------- | :-------- |
| Windows | `WasapiExclusive` | Exclusive | `windows` 0.58 COM: `IMMDeviceEnumerator` → `IAudioClient::Initialize` with `AUDCLNT_SHAREMODE_EXCLUSIVE`, event-driven (`hnsPeriodicity = 0`) | yes |
| Windows | `WasapiShared` | Shared | cpal WASAPI host; the graph format is the device format | no |
| Windows | `Asio` | Native | cpal ASIO host behind the opt-in `asio` feature (needs the Steinberg SDK) | yes |
| Windows | `CpalShared` | Shared | System default host, last-resort fallback | no |
| Linux | `AlsaHw` | Raw `hw:` | `alsa` 0.9 `snd_pcm_writei` loop on a dedicated thread; dmix and PulseAudio are bypassed | yes |
| Linux | `PipeWire` | Native graph node | `pw_stream` with `RT_PROCESS`, format negotiation through the SPA pod | no |
| Linux | `CpalShared` | Shared | cpal ALSA host on `default` | no |

`OutputMode` (`Auto`, `Exclusive`, `Shared`, `Asio`) is a *preference*, not a promise. `open_sink`
walks the candidate backends in order and returns the first one that negotiates; `Auto` therefore
degrades from exclusive to mixed output on its own, and a configuration error never leaves the user
without sound. Selecting an endpoint pins its driver stack: an `alsa-hw:` id never silently opens
PipeWire.

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
and player at the new native rate. Mixed endpoints are locked to the graph rate instead, and the
DSP chain resamples — which is why `bitPerfect` is false there by construction.

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
- **Volume** is *not* part of the chain: the pump applies a per-block linear ramp so a slider drag
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
   │ walkdir (streaming)
   ▼
path filter (extensions, .cue detection, symlink policy)
   │ rayon: parse tags + technical properties (lofty + symphonia probe)
   ▼
identity resolution (normalise path, SHA-free)
   │ batch of N=512 rows
   ▼
single writer transaction: upsert artists → albums → tracks (+FTS triggers)
   │
   ▼
scan_runs / scan_errors ledger, integrity flags, missing-file sweep
```

- **Resumability**: every scan is a `scan_runs` row with counters; a crash leaves a `running` row
  that the next boot marks `failed` and resumes from dirty roots.
- **Error isolation**: a corrupt FLAC header produces a `scan_errors` row and is skipped; the queue
  and the rest of the scan proceed. Files are never deleted from the catalogue by a failed read —
  only a successful enumeration marks `missing=0`.
- **Identity**: the unique expression index folds both path separators and case. The scanner still
  normalises before insert so that the same file cannot appear twice through differently-cased roots.
- **CUE**: `.cue` files are parsed first; the referenced image is inserted once per physical file and
  once per virtual track (with `source='cue'`, offsets, and the shared image path), all linked by
  `cue_sheet_id`. Re-scanning a sheet replaces its virtual tracks transactionally.
- **Targets**: 100k FLAC metadata reads in < 10 s on warm NVMe; SQLite writes are batched to keep the
  WAL under its 64 MiB size limit.

## 7. Persistence

- WAL, `synchronous=NORMAL`, `mmap_size=256 MiB`, 32 MiB page cache, 5 s busy timeout, 6 connections.
- Migrations are embedded (`sqlx::migrate!`), applied at boot inside a transaction; the schema version
  is part of the health payload.
- `tracks` is deliberately wide and denormalised: the library grid renders artist/album/quality
  directly, and triggers keep the copies coherent when an artist or album is renamed.
- FTS5 uses external content (`content='tracks'`) with triggers; `integrity-check` runs in tests and
  on demand in diagnostics.
- VectorClock-style conflict resolution is unnecessary (single user, single process); concurrent
  edits are serialised by SQLite.
- Backup/export: a consistent copy is produced with `VACUUM INTO` under a read transaction; the same
  file format is the import format.

## 8. Discord Rich Presence

1. `SHA-256(trim(lower(artist)) + '\n' + trim(lower(album)))` is the cache key.
2. `cover_cache` is consulted; hits carry the remote 512/1000 px URL used by Discord.
3. On a miss, a worker queries iTunes Search API, then MusicBrainz + Cover Art Archive; results are
   persisted with TTL and attempt counters.
4. The RPC payload contains title, artist, album, elapsed/remaining timestamps, a Hi-Res badge and
   `large_image` = cached URL.
5. Offline or unmatched → the Developer Portal asset key is used; retries use 2ⁿ backoff capped at
   five minutes and never run on the playback thread.

## 9. Plugin runtime

- **Lua 5.4** (`mlua`, vendored) for DSP processors, scrobblers and fetchers; **wasmtime** for
  untrusted visualisers.
- Events are delivered from a dedicated thread; each plugin gets an instruction/epoch budget per
  callback. Panics/traps are isolated and reported to the plugin manager.
- The host API is capability-based: a plugin declares `dsp`, `library.read`, `network` or `ui.panel`;
  undeclared capabilities fail closed. No plugin receives raw filesystem or device handles.
- DSP plugins operate on interleaved f32 blocks with a maximum block size; the host validates sample
  bounds before and after the call.

## 10. Frontend rendering

- Zustand stores are split by domain (shell, queue, playback, library, DSP); components subscribe with
  atomic selectors. The 60 Hz playhead and 60 Hz FFT meter do not live in the same store slice as the
  library list.
- `@tanstack/react-virtual` renders only visible rows; row height is fixed per density mode, keys are
  track ids, and no cell performs an Intl call per frame (formatters are cached).
- The ambient canvas is a GPU layer: palette extraction happens in Rust; the WebView only animates
  pre-composited blobs with `transform`/`opacity`.
- Framer Motion is limited to entrances, layout transitions and the player bar; continuous animation
  uses CSS/compositor-only properties.
- CLS budget: 0 — the shell geometry is identical between skeleton and loaded states.

## 11. Error taxonomy and observability

- `PrismError` serialises to `{ code, message }`: `database`, `migration`, `io`, `runtime`,
  `install`, `invalid_input`, `internal`. The frontend branches on `code`, never on message text.
- Zod validates every IPC payload; a mismatch surfaces as `contract` instead of corrupting state.
- Logs: stderr + daily files (14-day retention, non-blocking worker). `PRISM_LOG` controls verbosity.
- A diagnostics export bundles app info, pragma-derived database health, recent scan errors and the
  log tail, with paths redacted on request.

## 12. Packaging and delivery

- Tauri v2 bundles: NSIS + WiX (MSI) + portable EXE on Windows; DEB + AppImage on Linux.
- WebView2 uses the download bootstrapper (silent); Linux DEB dependencies use the Ubuntu 24.04
  `t64` names with alternatives for older releases.
- Release CI builds natively on both operating systems, generates `SHA256SUMS.txt`, and publishes a
  changelog derived from Conventional Commits.
- The updater is deliberately deferred until signing keys exist; unsigned auto-updates are a
  supply-chain risk.
