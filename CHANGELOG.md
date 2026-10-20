# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html), and every entry is
traceable to the delivery document of the phase that introduced it (`docs/PHASE-*.md`).

## [Unreleased]

_No unreleased changes._

## [0.1.1] — 2026-10-20

A correctness and pipeline release. `0.1.0` built and installed, but the window
stayed black on Windows and Linux: the interface bundle threw before React
mounted. This release fixes that, one database read model that broke the Home
view on a fresh install, and the release pipeline that published without its
installers.

### Fixed

- **The library list actually loads**: `useTrackWindow` bootstraps page 0 when its query generation
  changes — previously the total was only known after the first page, which was never requested, so
  a fresh mount showed an empty list forever.
- **A corrupt first track no longer takes the album down**: `start_queue` probed the head of the
  queue and propagated the error, so an album whose first rip was truncated would not play at all.
  The supervisor now walks forward, emits `TrackSkipped` per dead file and starts on the first
  playable track; an all-dead queue stops cleanly instead of spinning.
- **Discord handshake read `ev`, Discord sends `evt`**: a rejected client id was therefore parsed as
  "unknown event, the pipe is alive, connected", the socket then dropped and the settings pane showed
  a spinner that never resolved. The event name is now read as `evt` with `ev` as a fallback, and the
  state reports `Unavailable` with Discord's reason.
- **CLS = 0 with zero shift entries**: the audit reported one 0.0000037 shift, from the harness chrome
  swapping the text "loading" for "ready" inside a fixed-width pill — a label whose text changes is a
  layout-shift source. The state is now a coloured dot, the audit names the element and its rects if a
  shift ever returns, and CLS is exactly zero.
- **Black window on launch, Windows and Linux alike**: `app.security.freezePrototype` froze
  `Object.prototype` before the bundle ran, and the validation layer assigns a shadowing `toString`
  at module scope. Under the frozen prototype that assignment throws a `TypeError` in strict mode,
  the entry module never finishes evaluating, and the window stays black with the process perfectly
  healthy. Reproduced in the real binary under Xvfb, isolated by A/B against the same bundle with and
  without the freeze, and fixed by disabling the option — the CSP, the capability set, the IPC
  allowlist and `Object.freeze` on the app's own objects are unaffected.
- **`Home` failed on every fresh install with `no such column: added_at`**: the album grid sorts by
  the newest track it contains, but `v_album_summary` never exposed `added_at`. Migration `0002`
  recreates the read model with `MAX(t.added_at)` and adds a covering index; the schema harness now
  asserts both the column and the ordering.
- **Release staging read the wrong `target/` directory**: this is a Cargo workspace, so artefacts
  land in `./target/release`, not `./src-tauri/target/release`. Every bundle job staged from the
  second path, which exists but is empty, and the release published without installers. Staging now
  reads the workspace target, fails loudly when an artefact is missing, and the release validator
  refuses the old path.

### Testing and CI

- **Boot smoke test** (`scripts/smoke-ui.mjs`, `pnpm smoke`): loads the production bundle in a
  Tauri-shaped runtime — the real CSP from `tauri.conf.json` with inline-script hashes injected the
  way `tauri-codegen` injects them, `freezePrototype` applied as the app applies it, and a stubbed
  IPC that fails every command like a first launch does — then asserts that React committed a tree
  and that no unexpected error was reported. Verified in both directions: it fails on the exact
  configuration that shipped the black window.
- **Boot watchdog** (`index.html`): an inline, dependency-free script that renders a readable failure
  card — message, file, line — if the bundle has not mounted after a few seconds, instead of an
  empty black window. It runs before any module, so it survives a bundle that throws while being
  evaluated, and Tauri hashes it into the CSP so the policy stays strict.
- **Miri moved out of the pull-request path** into `miri.yml` (weekly and on demand): interpreting a
  crate that links WebKitGTK does not fit a CI timeout, and a gate that always times out is not a
  gate. Valgrind remains the memory gate in `qa`, pointed at the **audio kernel** — the ring, the
  sample conversions, the decoder, the CUE span arithmetic — rather than at the engine supervisor
  tests, which assert on wall-clock deadlines and cannot survive a 20× interpreter slowdown.
- **The 1.5 GB CUE stress test no longer assumes a filesystem**: it is `#[cfg(unix)]`, because the
  sparse-file precondition it checks (`blocks() * 512`) has no Windows counterpart — on NTFS
  `set_len` allocates the range and the test would have failed on `windows-latest` on every run.
  Where the filesystem does not sparsify at all (overlayfs, some network mounts), it now reports
  the fact and relies on the elapsed-time assertion, which is the real proof: a code path that
  read the image end to end would take minutes. CUE expansion is covered on both platforms by
  `tests/library.rs`.
- **Frame-budget verdicts split by determinism**: the `performance` job runs the audit with `--ci`,
  where the deterministic properties (bounded DOM, CLS 0, input off the paint path, clean console)
  fail the build and the timing-derived ones (headroom, frame cadence, stalls) are reported as
  advisory, because a shared runner is not a reference machine. `pnpm perf` locally still enforces
  all seven.

## [0.1.0] — 2026-10-20

First public release: a bit-perfect audio engine, a catalogue that survives 500 000 tracks,
and a keyboard-first interface measured against a 120 Hz frame budget.

### Added

#### Foundation (FASE 1)

- Rust workspace (edition 2024, MSRV 1.88) with release profile tuned for throughput
  (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`).
- Complete SQLite schema: 16 STRICT tables covering tracks, albums, artists, CUE sheets, playlists,
  smart playlists, lyrics, cover cache, play history, settings, library roots, scan ledger, DSP
  presets and layouts.
- FTS5 external-content search with diacritic folding, insert/update/delete triggers and
  artist/album rename cascades.
- CUE-aware track identity (shared image path disambiguated by sample offset) and path identity
  index folding separators and case.
- Aggregate read models (`v_track_details`, `v_album_summary`, `v_artist_summary`,
  `v_playlist_summary`, `v_library_stats`).
- WAL-mode sqlx pool with tuned pragmas, embedded migrations and a WAL enforcement check at boot.
- Serializable error taxonomy (`PrismError` → `{ code, message }`) and typed Tauri commands.
- Non-blocking dual-sink logging (stderr + daily rotating files, 14-day retention).
- Tauri v2 shell: hidden-until-ready window, OLED background, single-instance focus handoff,
  minimal capabilities, strict CSP with artwork/metadata allowlist.
- React 19 + TypeScript strict + Tailwind v4 diagnostics interface with validated IPC, persisted
  shell preferences and reduced-motion support.
- Windows NSIS/MSI and Linux DEB/AppImage bundling configuration with WebView2 bootstrapper and
  Ubuntu 24.04 `t64` dependency alternatives.
- CI/CD: cross-platform quality gate, native release matrix, structured changelog generation,
  SHA-256 checksums and version consistency audit.
- Test suites: real-SQLite schema harness (FTS5 integrity, cascades, constraints, views) and Rust
  integration tests for WAL, foreign keys, FTS cascade coherence and aggregate views.

#### Audio engine (FASE 2)

- Sample-format layer: S16/S24/S24-in-32/S32/F32/F64, allocation-free interleaved conversion,
  channel remapping and saturation-safe writers; DoP frame encoder with alternating `0x05`/`0xFA`
  markers.
- Bit-perfect signalling: `bitPerfect` is only true with an exclusive endpoint, no active DSP stage,
  0 dB volume, no mute and an exact source/device format match.
- Gapless pipeline: `GaplessPlayer` with a dedicated prefetch thread, sample-exact track boundaries,
  per-track DSP chain compilation and a bounded delivery counter so a queue ends silently.
- Decoder: Symphonia backend for FLAC/ALAC/MP3/AAC/WAV/AIFF/Ogg/Vorbis/Opus/WavPack, CUE-span
  addressing, packet carry-over (no frame ever stalls on a short read), corrupt-frame skip and a
  RAM Play source with a size-aware streaming fallback.
- DSD: DSF (block-interleaved) and DFF (bit-reversed) readers, DoP packaging at 176.4/352.8/705.6/
  1411.2 kHz and a forced-bypass path that keeps markers intact.
- DSP: parametric EQ (31 biquad sections, RBJ coefficients), Bauer/Meier/custom crossfeed, rubato
  sinc resampler (256-tap Blackman-Harris) and ReplayGain 2 metering that reproduces the BS.1770-4
  reference filters exactly at 48 kHz.
- Output backends: WASAPI exclusive (direct `IAudioClient` COM, event-driven), WASAPI shared and
  ASIO through cpal, raw ALSA `hw:` writer thread, native PipeWire stream node, and a cpal fallback.
- Device supervisor: negotiation/commit split so the DAC never starts on an empty ring, runtime
  format renegotiation for mixed-rate queues, per-block volume ramping, xrun recovery, device-loss
  detection through the refill channel and seamless endpoint rebuilds.
- IPC: `player_*` commands (play, pause, resume, stop, seek, volume, mute, DSP, output, RAM Play,
  device enumeration) plus a `player://event` push channel (state, track, position, format, device,
  underrun, error).

#### Library, search and rich presence (FASE 3)

- Metadata reader (`metadata/tags.rs`): one normalised `TrackTags` per file across FLAC, Vorbis, Opus,
  MP3/ID3v2, MP4 ilst, APEv2, RIFF INFO and AIFF, plus the technical properties the UI badges
  (bit depth, sample rate, channels, DSD family) and ReplayGain 2 tags with unit-aware parsing.
  Unknown tags are preserved as bounded JSON in `tracks.extra_tags`.
- Scanner (`metadata/scanner.rs`): streaming walkdir with VCS/cache pruning, rayon parsing in
  512-file chunks, 256-row transactions with a document-order column whitelist, deterministic
  SHA-256 identities, incremental `(mtime, size)` fingerprinting, CUE-sheet expansion into virtual
  tracks, album counter aggregation, missing-file sweep that never deletes rows, offline-root
  protection, cooperative cancellation and ≤ 5 Hz progress events.
- Catalogue writer (`db/catalog.rs`): `INSERT … ON CONFLICT(id) DO UPDATE` upserts, artist/album
  resolution with case-insensitive identity, album metadata merging across discs, temp-table based
  missing diff, full-scan reconciliation and opt-in orphan pruning.
- Search and read models (`db/search.rs`): sanitised FTS5 `MATCH` construction (no operator can reach
  the grammar), `bm25` ranking with per-column weights, control-character highlight markers,
  whitelisted sort orders, offset pagination with a stable tie-breaker, and album/artist/facet
  browsers plus a single-round-trip global search.
- Cover art (`artwork/`): SHA-256(artist ⋈ album) cache key, provider chain embedded → folder →
  iTunes → MusicBrainz + Cover Art Archive, 1000 px JPEG normalisation, 90-day success TTL, 24-hour
  negative caching, per-key in-flight collapsing, MusicBrainz rate limiting and an offline mode.
- Discord Rich Presence (`discord/`): full IPC framing and handshake over UNIX sockets and Windows
  named pipes, ping/pong, 1.5× exponential backoff with jitter, 2.5 s update coalescing, activity
  rendering with timestamps and quality badge, and a presence bridge driven by the audio engine
  rather than the UI.
- IPC: 17 new commands (`library_scan_*`, `library_roots`, `library_add_root`, `library_search`,
  `library_albums`, `library_artists`, `library_facets`, `library_global_search`,
  `library_album_cover`, `library_track_cover`, `discord_*`) plus `library://scan-progress` and
  `library://scan-complete` events, mirrored by zod schemas and typed wrappers.
- Tests: 150 unit tests and 14 new integration tests covering scanner idempotency, CUE offsets,
  missing sweeps, FTS ranking, diacritic folding, hostile query input, artwork normalisation,
  embedded-art precedence, negative caching and a real Discord IPC round trip.

#### Desktop interface (FASE 4)

- Artwork palette engine (`palette/`): deterministic median cut over a 128 px downsample, swatch
  population recovered by re-counting sampled pixels, accent scoring by saturation × mid-lightness
  bell × prominence, tone mapping to 0.66–0.88 value, and a WCAG 2.1 loop that lightens the accent
  until it clears 4.5:1 against the generated background. Greyscale art, corrupt files and missing
  covers fall back to a hue derived from the cover key (FNV-1a), so the same album is always the same
  colour. 512-entry in-memory cache, key validated as 64 hex characters before any disk access.
- `library_cover_palette` command plus `player_tick_channel`, and a `player://tick` stream carrying
  `{ positionMs, durationMs, playing }` at 8 Hz — dispatched by Rust so the webview never polls.
- Strict TypeScript contracts for the whole engine surface: `PlayerStatus`, `NowPlaying`,
  `AudioFormat`, `OutputDevice`, `TrackRequest`, the internally tagged `EngineEvent` union, the tick
  payload and `CoverPalette`, all zod-validated at the IPC edge.
- Zustand stores split by update frequency (`ui`, `output`, `library`, `playback.status`,
  `playback.tick`) with exhaustive-selector subscriptions; shell and output preferences persisted and
  versioned with a migration from the FASE 1 layout.
- IPC hooks: `usePlayerBridge` (status + events + ticks, generation-guarded), `useScanEvents`,
  `useArtworkPalette` and `useAlbumCoverKey` (album-scoped so skipping inside a record does not
  re-tint the window).
- Shell: navigable rail with live counters, top bar with scan state, routed content column with a
  180 ms cross-fade, and a `DesktopRequired` screen for the browser build instead of fake data.
- Player bar: artwork thumbnail, Hi-Res / Bit-perfect / Processed badge with the exact reason in the
  tooltip, scrubber with drag preview and single-shot seek, transport cluster, output selector
  (device, driver stack, exclusive capability, buffer size) and a dB-labelled volume control that
  snaps to unity gain.
- Now Playing: crossfaded artwork, palette bloom and a technical read-out of source format, device
  format, backend, latency and xrun counters.
- Home: editorial hero derived from the catalogue, four counters, and a recents rail that queues a
  whole album (disc/track ordered, deterministic shuffle) through the real engine.
- Settings: output mode/device/buffer, library roots with availability state, scan history table,
  Discord presence configuration and ambient intensity.
- Tests: 189 Rust tests (157 unit + 32 integration across database, engine, library, artwork,
  Discord and palette).

#### Virtualised library, spectrum, lyrics and plugins (FASE 5)

- Lyrics (`lyrics/`): embedded `USLT` → sidecar `.lrc` → empty resolution, a dialect-tolerant LRC
  parser (1–3 fraction digits, `mm:ss:ff`, multi-timestamp lines, metadata tags, BOM/CRLF, unsorted
  input, `[offset:]` in both signs), sort + de-duplication, 512 KiB body limit, and a
  `lyrics_cache` policy where a miss is never stored and a sidecar row is re-validated against its
  path on every read.
- Spectrum analysis (`audio/analysis.rs`): 2 048-point Hann-windowed FFT on the audio supervisor's
  producer thread, 31 log-spaced bands (20 Hz → 0.98 × Nyquist), −80 dBFS floor, peak-per-band
  scaling, attack/release smoothing, 4 096-sample ring with sample-rate invalidation, and a
  reference-counted `visualiser://frame` stream at 60 Hz (20 Hz while silent).
- IPC: `lyrics_get`, `lyrics_clear`, `visualiser_set_enabled`, `visualiser_frame`, `plugins_list`,
  `plugins_reload`, `plugins_set_enabled`, `plugins_set_config`, `plugins_open_folder`, plus the
  two new event channels.
- Plugin runtime (`plugins/`): directory discovery, strict `plugin.toml` validation (reverse-DNS ids,
  semver, API revision, traversal-proof entry paths, self-validating defaults), capability-gated host
  bridge (`log`, `notify`, `player:read`, `player:control`), Lua 5.4 sandbox (`mlua` vendored,
  restricted stdlib, 16 MiB memory limit, 20 M-instruction budget enforced by a VM hook) and a
  WebAssembly sandbox (`wasmtime`, fuel-metered, 64 MiB linear memory, import-list isolation, textual
  `prism` ABI), session-scoped failure handling, persisted enable/disable and per-plugin settings.
- Interface: `LibraryView` with debounced FTS search and sort, `VirtualTrackList` (52 px rows,
  12-row overscan, `contain: strict`, keyboard selection), `useTrackWindow` (500-row pages, 12-page
  bounded cache, generation-safe responses), `SpectrumVisualiser` (canvas, rAF interpolation, DPR
  aware, reduced-motion static mode), `LyricsPanel` (karaoke cursor, auto-follow with manual-scroll
  suspension), `useLayoutStore` + `NowPlayingDeck` (five modules, three presets, persisted order), and
  a `PluginsSection` in Settings with typed controls and a live activity stream.
- `prism.setting(key)` for Lua plugins: typed setting access as native Lua values, because the sandbox
  deliberately has no JSON decoder.
- Example plugin `examples/plugins/volume-guard` (manifest + Lua + README), loaded by the test suite so
  the documented host API stays executable.
- Tests: 34 new unit tests (9 lyrics, 9 analysis, 16 plugin manifest/runtime), 191 lib tests in the
  default build and 199 with `--features lua-plugins`.

#### Quality engineering (FASE 6)

- **Device stress** (`tests/stress.rs`): a loopback endpoint injected through `Engine::with_opener`
  — the same seam production points at `backend::open_sink` — drives seven tests: five mid-track hot
  swaps (playhead, queue index and one device open per swap), an unplugged DAC reopened and resumed
  at the same position, a corrupt track skipped mid-album and reported to the UI, a queue of only
  corrupt files stopping cleanly with four ordered skip events, a 1.5 GB sparse CUE image expanded
  into three sample-accurate virtual tracks in under 20 s, an endpoint that refuses once and then
  succeeds, and swap/stop churn leaving no live device thread.
- **Corrupt-file recovery end to end**: `SkippedTrack` in the decoder, `EngineEvent::TrackSkipped`
  through the supervisor, a `track_skipped` plugin event, a `notice` slice in `playbackStore` and a
  dismissible `AppShell` banner naming the file and the reason.

- **120 FPS audit** (`scripts/perf-audit.mjs`, `vite.bench.config.ts`, `src/perf/*`): the production
  `VirtualTrackList` over 500 000 synthetic rows in Chromium, two scroll workloads, main-thread
  milliseconds per painted frame via `Performance.getMetrics`, CLS with its sources, long tasks, DOM
  count and heap. Seven pass/fail verdicts; `pnpm perf` fails the build on regression; CI runs it on
  every pull request and uploads the report.
- **Discord RPC offline fallback**: reconnect schedule as a value (`BackoffPolicy`, 1 s start,
  ×1.5 growth, 60 s ceiling, additive jitter) and six tests against a real UNIX socket — unavailable
  state with a reason, rate-limited retries, rejection by client id, pending activity flushed on
  reconnect, the growth curve, and a missing socket that is an error rather than a panic.

- **Memory and UB tooling**: `valgrind --tool=memcheck` over the stress suite and 105 `audio::*`
  tests reports **0 errors from 0 contexts**; a nightly Miri job covers `audio::ring`,
  `audio::format` and `lyrics::` in CI.
- **CPU profiling**: `pprof` behind the `profiling` feature and `examples/profile_hot_path.rs`, with
  two flamegraphs committed under `docs/media/`. Measured in a debug build: **656× real time**
  bit-perfect (0.018 ms per 512-frame block) and **25× real time** resampling 44.1 → 48 kHz.

- **CI**: new `qa` job (Miri + valgrind) and `performance` job (frame budget with artefact upload);
  `docs/PERFORMANCE.md` now carries the measured numbers, the profiling recipes and a withdrawn
  `loom` plan instead of a promise.
- Tests: **197 lib + 39 integration** (32 + 7 stress) = 236 in the default build; 205 lib with
  `--features lua-plugins`.

### Performance

- **Virtualiser fix**: `estimateSize`/`getItemKey`/`getScrollElement` hoisted out of render. Inline
  arrows invalidated `@tanstack/react-virtual`'s measurement memo, rebuilding a 500 000-entry array
  on every scroll frame (21–30 % of JS samples). **18.92 ms → 3.02 ms** of main-thread work per
  frame; 51 → 262–305 fps unpinned.
- **Rows**: `TrackRowItem` is a `memo` component subscribing to `playbackStore` through boolean
  selectors, so a track change re-renders two rows instead of the list and scrolling re-renders none.

### Security

- **A webview that cannot leave its sandbox**: strict CSP with an explicit artwork and metadata
  allowlist, a minimal Tauri capability set, frozen prototypes, no `unsafe-eval` outside the dev
  CSP, and a plugin runtime that gates every host call behind a declared capability with memory
  and instruction budgets (FASE 1, FASE 5).

[0.1.0]: https://github.com/prism-audio/prism/releases/tag/v0.1.0
[0.1.1]: https://github.com/prism-audio/prism/releases/tag/v0.1.1
[Unreleased]: https://github.com/prism-audio/prism/compare/v0.1.0...HEAD
