# Changelog

All notable changes to Prism are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-10-19

### Added

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

### Audio engine (FASE 2)

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

[0.1.0]: https://github.com/prism-audio/prism/releases/tag/v0.1.0
