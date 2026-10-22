# Changelog

Notable user-visible changes are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4] — 2026-10-22

### Added

- User playlists with explicit ordering, duplicate protection and track assignment from context
  menus.
- Genre shelves on Home and a persistent genre filter in Library.
- Application context menus for tracks, albums and playlists, including an album details dialog.
- Borderless fullscreen through the window control and F11.
- A Discord preference for retaining or clearing activity while playback is paused.

### Changed

- Appearance controls now cover colour bleed, effects, corners, density, cover size, rail arrows,
  Home counters and spectrum visibility.
- Library sorting uses reversible Artist, Album and Year controls and persists across restarts.
- Home collections and their See all pages use the same catalogue predicates.
- Track rows use single-click selection and double-click playback.
- Discord activity presents song, artist and album without duplicated album text.
- Volume uses a perceptual taper and immediate mute feedback.
- Appearance, browsing and open layout controls persist between sessions.
- Browser context menus, browser shortcuts and Tab navigation are suppressed in the desktop shell.

### Fixed

- Files that share a MusicBrainz recording id no longer abort a library scan.
- Multi-disc album ordering, scrolling and artwork resolution.
- Album text escaping and artist filtering in shared collection queries.
- The sidebar icon and remaining outdated technical copy.

## [0.1.3] — 2026-10-22

### Added

- Source-rate and fixed-rate output policies.
- Automatic reduction of expensive visual effects on slower compositors.

### Fixed

- Preserved complete decoded blocks during resampling and prevented discontinuities at ring fills.
- Published track duration after decoder initialization and improved playhead interpolation.
- Restored cover and artist clicks on draggable Home shelves.
- Kept volume changes stable while waiting for engine acknowledgement.
- Isolated scanner failures and made cancellation release the scanner cleanly.
- Applied saved output settings during application startup.

## [0.1.2] — 2026-10-21

### Added

- Album and artist pages with disc-aware playback and deterministic shuffle.
- Global search across tracks, albums and artists.
- Home collection shelves and an album-grid Library with persistent cover sizing.
- Expanded appearance controls and backend-driven artwork resolution.

### Changed

- Shared output became the default playback path.
- Output selection moved to Settings and reports the negotiated endpoint format.
- The Now Playing layout was simplified around artwork, lyrics, spectrum and queue modules.
- Volume is presented as a percentage with a separate mute control.
- Albums and artist names link to their corresponding catalogue pages.

### Fixed

- Corrected format negotiation and shared-mode conversion on Windows.
- Corrected WASAPI exclusive buffer writes and end-of-track resampler flushing.
- Removed the scanner's fixed 1024-file processing ceiling.
- Library-root removal now removes associated catalogue rows.
- Discord artwork resolution and playback-position scaling.
- Stop, shelf dragging and album artist links.
- Album artist identifiers are written during scanning and exposed by the album read model.

## [0.1.1] — 2026-10-20

### Fixed

- Prevented a startup failure that could leave the desktop window blank.
- Track-window initialization now loads the first result page.
- Unreadable tracks are skipped without stopping the remaining queue.
- Corrected Discord handshake event handling and reconnect reporting.
- Added missing album play statistics and aggregate columns through database migrations.
- Release staging now reads artefacts from the Cargo workspace target directory.

## [0.1.0] — 2026-10-20

### Added

- Rust audio engine with gapless playback, CUE support, RAM Play, DSD over PCM and device recovery.
- WASAPI, optional ASIO, PipeWire and ALSA output backends.
- Resampling, ReplayGain, parametric EQ and crossfeed.
- SQLite catalogue with migrations, FTS5 search, aggregate read models and deterministic identities.
- Incremental metadata scanning, artwork caching and Discord Rich Presence.
- Tauri desktop shell with typed IPC and a React and TypeScript interface.
- Album and track browsing, virtualised lists, lyrics, spectrum analysis and modular Now Playing
  layouts.
- Optional Lua and WebAssembly plugin runtimes with capability checks and resource limits.
- Windows and Linux packaging configuration, dependency licence inventory and release checksums.

[Unreleased]: https://github.com/buntaroumori/Prism/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.4
[0.1.3]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.3
[0.1.2]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.2
[0.1.1]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.1
[0.1.0]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.0
