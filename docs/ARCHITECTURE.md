# Architecture

Prism is a single desktop application with three main boundaries: a native Rust core, a typed IPC
layer and a React interface. The WebView does not open audio devices or scan the filesystem, and the
Rust core does not render interface state.

## System overview

```text
React interface
      │ validated commands and events
      ▼
Tauri IPC ────────────────┐
      │                   │
      ▼                   ▼
Application services   Playback engine
      │                   │
      ├── SQLite          ├── Decoder and prefetch
      ├── Scanner         ├── DSP and resampling
      ├── Artwork         ├── Output negotiation
      ├── Discord         └── Device backends
      └── Plugins
```

The main source areas are:

| Path                      | Responsibility                                                    |
| :------------------------ | :---------------------------------------------------------------- |
| `src-tauri/src/audio/`    | Decoding, playback supervision, DSP, analysis and output backends |
| `src-tauri/src/metadata/` | Tag reading, CUE expansion and library scanning                   |
| `src-tauri/src/db/`       | Connections, migrations, catalogue writes and read models         |
| `src-tauri/src/artwork/`  | Artwork resolution, normalisation and caching                     |
| `src-tauri/src/discord/`  | Discord IPC transport and presence policy                         |
| `src-tauri/src/plugins/`  | Plugin discovery, manifests, configuration and sandboxes          |
| `src-tauri/src/commands/` | Tauri command surface                                             |
| `src/`                    | React views, stores, IPC contracts and interface components       |

## Process and thread model

Prism runs as one native process. Work is separated according to its timing requirements.

| Execution context         | Responsibility                                            |
| :------------------------ | :-------------------------------------------------------- |
| Tauri event loop          | Window management and desktop events                      |
| Tokio runtime             | Database, filesystem, HTTP and application commands       |
| Tokio blocking pool       | Bounded metadata parsing during scans                     |
| Playback supervisor       | Queue state, decoding, DSP, playhead and device lifecycle |
| Prefetch worker           | Preparation of the next track for gapless transitions     |
| Device callback or writer | Transfer of buffered audio to the selected endpoint       |
| Discord task              | Presence updates and reconnect handling                   |
| Plugin runtimes           | Isolated plugin execution with per-call budgets           |

The playback producer and device consumer share a preallocated single-producer/single-consumer
ring. Filesystem access, SQL, HTTP and logging stay outside the audio callback. Device failures are
reported to the supervisor, which can rebuild the endpoint without discarding the queue.

## Audio engine

### Output backends

| Platform          | Backend              | Access                                         |
| :---------------- | :------------------- | :--------------------------------------------- |
| Windows           | WASAPI shared        | System mix graph                               |
| Windows           | WASAPI exclusive     | Direct endpoint, when selected by the engine   |
| Windows           | ASIO                 | Optional `asio` Cargo feature and external SDK |
| Linux             | ALSA `hw:`           | Direct hardware device                         |
| Linux             | PipeWire             | Native graph stream                            |
| Windows and Linux | cpal shared fallback | Platform default output                        |

Shared output is the standard application path. Endpoint selection and sample-rate policy are
configured in Settings. Opening an output has two stages:

1. **Negotiation** resolves the endpoint, channel layout, sample format, sample rate and period.
2. **Commit** creates the ring and processing chain for the negotiated format, primes audio and
   starts the endpoint.

The negotiated format, rather than the requested format, is used for playback and status reporting.
If the source and endpoint rates differ, the processing chain performs resampling.

### Sample-rate policy

- **Follow the record** requests the source rate and renegotiates at a rate boundary when the
  endpoint has demonstrated that it can accept that rate.
- **Device rate** keeps the endpoint at its configured rate and converts source audio as needed.

An endpoint that refuses source-rate negotiation remains on its accepted format; the engine does
not repeatedly reopen it for each track.

### Bit-perfect status

The interface reports bit-perfect playback only when the active path satisfies all required
conditions:

- an exclusive endpoint is in use;
- source and output sample formats, channel counts and rates match;
- no resampling or optional DSP stage is active;
- volume is at unity and the output is not muted;
- for DSD over PCM, the DoP carrier is preserved.

Otherwise, the UI reports a processed path and exposes the negotiated format.

### Decoding, gapless playback and RAM Play

The decoder supports local files and CUE spans. A prefetch worker prepares the next track while the
current track is playing, including its processing chain and initial audio. Boundary metadata tells
the supervisor where a new track starts inside a delivered block.

RAM Play decodes a track into memory before playback. Tracks above the configured memory limit use
the streaming source instead. Unreadable tracks are reported and skipped without discarding the
rest of the queue.

### Processing chain

```text
decode → ReplayGain → resampling → EQ → crossfeed → output ring → volume ramp
```

Stages that are not required are omitted when the chain is built. Volume is applied as a per-block
ramp outside the filter graph so routine volume changes do not rebuild the chain.

The processing modules include:

- sinc resampling with continuous streaming state and end-of-stream flushing;
- track or album ReplayGain with peak protection;
- up to 31 parametric EQ bands;
- configurable crossfeed;
- DSD over PCM packing with a bypass path for DoP data.

## Catalogue and scanning

### Scan pipeline

```text
Library roots
     │
     ▼
Filesystem walk and extension filtering
     │
     ├── CUE sheet ──► sheet parser ──► virtual track records
     │
     └── Audio file ─► mtime/size fingerprint ─► metadata reader
                              │
                              ▼
                    Bounded blocking-pool tasks
                              │
                              ▼
                    Transactional catalogue writer
                              │
                              ▼
              Aggregates, missing-file state and scan ledger
```

Unchanged files are skipped using their filesystem fingerprint; CUE sheets use a content digest.
Metadata parsing runs in bounded waves on the blocking pool. Parser failures and timeouts are
recorded against individual files, and committed batches remain valid if a scan is cancelled.

Catalogue writes are transactional. Album and artist aggregates are refreshed after writes, and
scan progress is published separately from database commits.

### Identity and missing files

Track identity is based on normalised path, source and CUE offset. Artist and album identities use
normalised metadata. This makes repeated scans idempotent while preserving references from
playlists and listening history.

A MusicBrainz recording id is metadata, not a file identity. Multiple files may legitimately share
one recording id, so the database keeps a non-unique lookup index for it.

A missing file is marked rather than immediately deleted. Its metadata and user-owned references
can therefore survive a temporary disconnect. An offline library root is handled separately and
does not mark its entire contents missing.

### Playlists

Static playlists and their entries are stored in SQLite. Entry order is explicit, and a unique
`(playlist_id, track_id)` index prevents duplicate songs. Playlist reads join the same track read
model used by the rest of the application.

### Search and read models

SQLite runs in WAL mode with embedded, forward-only migrations. The main read models cover tracks,
albums, artists, playlists and library statistics.

Full-text search uses an external-content FTS5 table with diacritic folding. User text is converted
into bounded quoted terms before it reaches `MATCH`; sort fragments are selected from closed enums.
Pagination includes a stable identity tie-breaker.

Home shelves and their corresponding See all pages share collection predicates. This keeps counts,
shelf membership and full results consistent.

## Artwork and Discord

Artwork resolution follows a cached provider chain:

1. embedded track artwork;
2. artwork files in the album folder;
3. enabled online providers;
4. a local fallback when no artwork is available.

Images are decoded, resized within a fixed bound and stored in the application cache. Successful
and failed provider lookups have different cache lifetimes, and concurrent requests for the same
album are coalesced. Multi-disc albums use the first available album artwork rather than requiring
a separate image in every disc folder.

Discord Rich Presence subscribes to engine events rather than interface state. The transport uses
Discord IPC over Unix sockets or Windows named pipes, with bounded reconnect backoff. Activity
updates include track metadata and, when available, an externally accessible artwork URL. A
persisted preference controls whether paused activity remains visible or is cleared until resume.

## Interface and IPC

### Contracts

Rust command results and event payloads have corresponding Zod schemas in `src/types/ipc.ts`.
Malformed payloads are rejected at the IPC boundary instead of being written to application stores.
Command errors use a stable code and message rather than requiring the UI to parse Rust error text.

### State and rendering

Zustand stores are separated by responsibility and update frequency. High-frequency playhead and
spectrum updates do not invalidate the entire view tree. Browsing, appearance, layout, output and
window preferences use versioned persistence where appropriate.

Long track results are paged and virtualised. Page requests are generation-scoped so a late response
from a previous query cannot replace current results. Album grids use backend pagination and stable
ordering.

### Now Playing

The Now Playing deck contains independent artwork, lyrics, spectrum and queue modules. Layout and
module visibility are persisted, and at least one module remains available.

The spectrum analyser runs on decoded audio in the Rust producer path. It sends bounded band data to
the interface, where a canvas interpolates frames without putting every update into React state.

Lyrics are resolved from embedded USLT metadata or a sidecar LRC file. The parser handles common
timestamp variants, offsets and unsorted input.

## Plugins

Lua and WebAssembly runtimes are optional Cargo features and are disabled in the default build.
Plugins declare their capabilities in a strict manifest and run with memory and execution budgets.
Playback does not depend on plugin success.

The manifest, host API, ABI and configuration model are specified in
[Plugin development](PLUGINS.md).

## Errors, logging and security boundaries

- `PrismError` and audio-specific error types preserve actionable error codes.
- IPC arguments and responses are validated at their respective boundaries.
- SQL values are bound, and dynamic ordering uses fixed fragments.
- The WebView uses a restrictive content security policy and a minimal Tauri capability set.
- Asset protocol access is limited to application data and cache locations.
- Online metadata access uses HTTPS and does not require application credentials.
- Logs are written through a non-blocking worker to standard error and rotating files. `PRISM_LOG`
  controls filtering with standard `EnvFilter` syntax.

## Packaging

Tauri produces NSIS, MSI, AppImage and DEB artefacts on their respective platforms. The release
workflow builds on Windows and Linux, verifies version agreement and publishes checksums and the
third-party licence inventory with the packages.

The release procedure and artefact layout are documented in [Release process](RELEASE.md).
