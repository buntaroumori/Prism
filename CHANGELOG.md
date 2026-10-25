# Changelog

Notable user-visible changes are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] — 2026-10-25

### Added

- Dynamic Library decade filtering with ordered year buttons, decade navigation and album-count
  labels for genre and decade facets.
- Local-first lyric resolution with LRCLIB lookup, cached plain or synchronized lyrics and real-time
  line highlighting. The integration does not infer word timing that the source does not provide.
- A centered synchronized-lyrics sheet opened from the Player Bar microphone control.
- Windows System Media Transport Controls integration with music metadata, cached artwork,
  playback position, seeking and native transport or media-key commands.
- Full playlist editing from the in-page pencil, including an optional description and genre.

### Changed

- Now Playing uses a bounded, non-scrolling two-column deck with metadata above unframed artwork,
  complete playback controls below it and equally sized lyrics and Spectrum panels. The global
  Player Bar is hidden only on this route.
- Spectrum remains optional alongside lyrics and becomes the sole right-hand panel when lyrics are
  unavailable. The sidebar can be hidden from the transport and revealed from the left edge.
- Non-interactive Top Bar space drags the desktop window. Genre, output and other remaining native
  selectors use Prism-styled components.
- Album genre links open the corresponding Library filter. Detail Back controls and the Settings
  toggle restore the actual previous route and its scroll position.
- Artwork views share thumbnail requests and upgrade missing legacy thumbnails in the background;
  output-device enumeration no longer blocks entry to Settings.
- Album Details follows Pin Album in context menus. Playlist context rename changes only the title,
  while playlist rows are numbered independently from stored track metadata.
- Text selection is limited to album names, artist names and editable content; Ctrl+A remains
  available inside editable fields.

### Fixed

- Removed avoidable cover-loading and scrolling stalls on Home and Library, and the repeatable delay
  when opening Settings.
- Custom playlist-cover imports now use Windows-safe managed filenames instead of playlist ids that
  contain invalid path characters.
- Returning from Now Playing restores the global Player Bar immediately without carrying its fixed
  layout or scrolling constraints into other views.

## [1.0.0] — 2026-10-23

### Added

- Full queue management with album groups, track and block dragging, context removal, double-click
  jumping and queue-aware Skip.
- Play next for albums and tracks, including immediate playback when the queue is idle.
- Single-click Play actions beside track numbers in album, playlist and queue lists.
- Album playback stops at the album boundary by default; optional continuation uses the originating
  Library filters and complete-date ordering.
- A newest-first, reorderable Dock for pinned albums and playlists in expanded and collapsed
  sidebars.
- Custom playlist covers selected through the native file picker and stored as managed artwork.
- Local Discord artwork overrides for albums without public artwork, with validated HTTPS URLs and
  no image upload.
- Integrated minimize, maximize and close controls for the borderless desktop window.

### Changed

- Now Playing uses one layout with artwork and lyrics; Spectrum expands into the lyrics area when
  no lyrics are available.
- Source labels distinguish green CD and purple Hi-Res FLAC from neutral non-FLAC formats in the
  player, album pages and Album Details.
- Album pages show complete release dates and genres, while tracklists omit redundant per-track
  genre and quality fields. Album Details follows a consistent metadata order.
- Settings opens from the top bar and includes System information. Obsolete sidebar status and
  totals were removed.
- The player bar places Queue beside volume and no longer duplicates output-device and format text.
- Album Shuffle generates a fresh order on every press and shares the player shuffle state.
- Artwork loading reuses master and thumbnail caches and coalesces identical requests across Home,
  Library and Now Playing.

### Fixed

- Disabling shuffle restores the exact previous queue order while retaining the current song.
- Volume changes reach the running audio engine while the slider is being dragged.
- Carried audio no longer receives a repeated gain ramp, avoiding attenuation artifacts without
  changing unity-gain playback.
- Same-year albums sort by their complete release dates, and undated albums remain last in both
  Year directions.
- Live queue edits preserve the current source, DSP state and playhead; stale edits fail without
  discarding playback.

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

[Unreleased]: https://github.com/buntaroumori/Prism/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/buntaroumori/Prism/releases/tag/v1.1.0
[1.0.0]: https://github.com/buntaroumori/Prism/releases/tag/v1.0.0
[0.1.4]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.4
[0.1.3]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.3
[0.1.2]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.2
[0.1.1]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.1
[0.1.0]: https://github.com/buntaroumori/Prism/releases/tag/v0.1.0
