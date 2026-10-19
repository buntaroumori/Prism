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

[0.1.0]: https://github.com/prism-audio/prism/releases/tag/v0.1.0
