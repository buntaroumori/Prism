# Prism — Architecture

> Status: the persistence, IPC, logging and shell layers are implemented (FASE 1).
> The audio engine (FASE 2), catalogue workers (FASE 3), full interface (FASE 4) and plugin runtime
> (FASE 5) are specified here as the binding target architecture — every later module lands against
> these contracts rather than improvising around them.

## 1. Process and thread model

A single native process hosts everything; there is no sidecar binary to keep alive or supervise.

| Thread              | Ownership                                              | Rules                                                                                     |
| :------------------ | :----------------------------------------------------- | :---------------------------------------------------------------------------------------- |
| **UI / event loop** | Tauri, WebView, window management                      | Never touches audio buffers; commands are `async` and return quickly                      |
| **Command workers** | tokio multi-thread runtime                             | All SQL, filesystem and HTTP work; bounded by the pool                                    |
| **Scanner**         | dedicated rayon pool                                   | `walkdir` producer → parallel metadata readers → single SQL writer                        |
| **Decode A/B**      | two cooperating threads                                | Pre-roll the next track while the current one plays (gapless)                             |
| **Audio callback**  | OS real-time thread owned by cpal/WASAPI/ASIO/PipeWire | **No allocation, no locks, no syscalls, no logging.** Only ring-buffer reads and DSP math |
| **DSP worker**      | high-priority non-RT thread                            | Resampling and EQ run ahead of the callback, feeding a lock-free ring buffer              |
| **Discord RPC**     | tokio task                                             | Never blocks playback; failures are retried with exponential backoff                      |
| **Plugin host**     | sandboxed Lua / wasmtime instance                      | Event-driven, CPU- and memory-budgeted, cannot touch the filesystem directly              |

The audio callback communicates with the rest of the system exclusively through a pre-allocated
`ringbuf` (power-of-two, lock-free SPSC) and atomics. Any error inside the callback sets a flag and
the supervisor tears down/rebuilds the device on a normal thread — the process never aborts because
a DAC disappeared.

## 2. Audio output layer

| Platform | Backend  | Mode          | Notes                                                                                |
| :------- | :------- | :------------ | :----------------------------------------------------------------------------------- |
| Windows  | WASAPI   | Exclusive     | `AUDCLNT_SHAREMODE_EXCLUSIVE`, event-driven, device clock; bypasses the system mixer |
| Windows  | WASAPI   | Shared        | Fallback with format negotiation; mixer bypass impossible by definition              |
| Windows  | ASIO     | Native        | Direct driver access for pro interfaces; per-driver buffer size control              |
| Linux    | PipeWire | Native client | Low-latency graph node, explicit sample format negotiation                           |
| Linux    | ALSA     | Direct `hw:`  | Bit-perfect passthrough, bypasses PulseAudio/PipeWire entirely                       |

Device discovery is dynamic. A hot-swap (USB DAC unplugged mid-track) follows this sequence:

1. callback returns silence and raises `DeviceError` via an atomic flag;
2. the supervisor stops the stream and marks the engine `Suspended`;
3. a watcher waits for device topology changes (WASAPI notifications / PipeWire registry events /
   ALSA card enumeration);
4. when the previous device or its default replacement returns, the clock is rebuilt, the decoder
   seeks to the exact sample position and playback resumes with a short fade-in.

The UI receives `device_lost` / `device_restored` events; the queue is never cleared.

## 3. Bit-perfect path

`file bytes → decoder (integer samples where possible) → DSP graph (bypassable) → ring buffer → OS
audio endpoint`.

- Float conversion only occurs inside DSP modules that require it; if EQ, crossfeed, ReplayGain and
  resampling are all disabled, PCM samples reach the endpoint untouched.
- The engine refuses to open an output stream that would force a hidden resample; instead it exposes
  the limitation as a UI badge ("device does not accept 96 kHz") and offers an explicit resampler.
- DSD is transmitted either as native DSD (ASIO DSD, PipeWire DSD) or as DoP: 1-bit groups packed
  into 24-bit PCM frames with the `0x05/0xFA` marker, at 176.4 kHz (DSD64), 352.8 kHz (DSD128) and
  705.6 kHz (DSD256). A DoP stream is never passed through EQ or volume.

## 4. Gapless playback and RAM Play

Two decode contexts alternate. While track _N_ is audible, track _N+1_ is fully decoded into a
bounded buffer. The GPU/UI never sees a gap: the transition is a pointer swap at a sample boundary.

- **Strict gapless** requires matching sample rate/format. When they differ, Prism either reconfigures
  the device (exclusive backends) or inserts the configured resampler; the transition point is still
  sample-accurate.
- **RAM Play** preloads a track or a whole album into a single contiguous allocation. Files are
  memory-mapped when larger than the configured threshold; the scanner records `file_size` and the
  decoder estimates decoded PCM size before committing.
- CUE virtual tracks share one decode context: seeking is arithmetic on the album image plus the
  track offset, so cue boundaries are also gap-free.

## 5. DSP graph

`Decoder → [ReplayGain 2] → [Resampler] → [Parametric EQ] → [Crossfeed] → [Volume] → Ring buffer`

- **ReplayGain 2 / EBU R128**: track or album gain, pre-amp, peak-aware limiting; applied in the
  float domain only when enabled, with a true-peak guard.
- **rubato**: sinc interpolation (256-tap, Blackman-Harris window) for irrational ratios; linear
  phase; anti-aliasing enforced on downsample.
- **EQ**: up to 31 biquad bands, peaking/low-shelf/high-shelf/HP/LP, per channel, coefficients
  recomputed off the audio thread and atomically published. Presets are stored as JSON with schema
  validation in SQLite.
- **Crossfeed**: Bauer (default), Meier and custom delay/gain; mild low-pass on the opposite channel
  to reduce headphone fatigue without collapsing the image.
- Graphs are compiled once per configuration change; bypassed nodes cost zero instructions (the
  compiler removes them from the process closure).

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
