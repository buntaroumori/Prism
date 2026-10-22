<div align="center">

<img src="assets/icon.png" alt="Prism icon" width="104" />

# Prism

**A local music player for Windows and Linux.**

Prism combines a Rust playback engine, a SQLite catalogue and a Tauri desktop interface. It is
designed for local music libraries, precise format handling and a configurable listening workspace.

</div>

## Features

### Playback

- Gapless playback with prefetching and sample-aware track boundaries.
- Native output through WASAPI, optional ASIO, PipeWire and ALSA, with a shared-output fallback.
- Source-rate and fixed-rate output policies, with format negotiation and resampling when required.
- FLAC, ALAC, WAV, AIFF, Ogg Vorbis and MP3 decoding, plus DSF/DFF input and DSD over PCM.
- CUE sheets, including individual tracks within a single audio image.
- ReplayGain, parametric EQ, crossfeed, volume ramping and optional in-memory playback.
- Live source and output format information. The bit-perfect indicator is shown only when the
  negotiated output and processing path satisfy its requirements.

### Library

- Incremental scanning with per-file error isolation and cooperative cancellation.
- Album, artist, genre and multi-disc browsing with persistent sorting and filters.
- Full-text search with diacritic folding and grouped track, album and artist results.
- User playlists with explicit ordering and duplicate protection.
- Embedded and folder artwork, with optional online artwork providers and a local cache.
- Home collections for recent additions, listening history, releases, genres and rediscovery.

### Interface and integration

- Home, Library, Playlists, album, artist and Now Playing views.
- A modular Now Playing deck with artwork, lyrics, spectrum and queue panels.
- Persistent appearance, layout and browsing preferences.
- Application context menus, borderless fullscreen and desktop-oriented track interactions.
- Optional Discord Rich Presence with configurable paused activity.
- Optional Lua and WebAssembly plugin runtimes with declared capabilities and resource limits.

## Screenshots

|                                    Home                                    |                         Library                         |
| :------------------------------------------------------------------------: | :-----------------------------------------------------: |
|           ![Prism Home with genre shelves](docs/media/home.png)            |   ![Prism Library album grid](docs/media/library.png)   |
|                              **Now Playing**                               |                     **Appearance**                      |
| ![Prism Now Playing with artwork and spectrum](docs/media/now-playing.png) | ![Prism appearance settings](docs/media/appearance.png) |

## Download

Windows and Linux packages are published on [GitHub Releases](https://github.com/buntaroumori/Prism/releases/latest).
Each release includes a `SHA256SUMS.txt` file for verifying the downloaded artefacts.

## Build from source

### Prerequisites

- Rust **1.88.0**, selected automatically by `rust-toolchain.toml`.
- Node.js **20.19.0 or newer** and pnpm **10.34.7** through Corepack.
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for the target platform.

On Debian or Ubuntu, the native build dependencies can be installed with:

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libasound2-dev libpipewire-0.3-dev libxdo-dev \
  libayatana-appindicator3-dev libclang-dev
```

Windows builds require the MSVC Rust target, Microsoft C++ Build Tools and WebView2.

### Development

```bash
git clone https://github.com/buntaroumori/Prism.git
cd Prism
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

The Tauri process starts the Vite development server and opens the desktop window. Playback,
library scanning and filesystem access require the desktop application; they are not emulated in a
standalone browser.

### Production packages

```bash
pnpm tauri build
```

Tauri builds the frontend and the Rust release binary before creating the packages enabled in
`src-tauri/tauri.conf.json`. Artefacts are written below `target/release/` and
`target/release/bundle/`.

The default build does not include the optional plugin runtimes or ASIO support. See
[Plugin development](docs/PLUGINS.md) for the corresponding Cargo features.

## Verification

The repository includes frontend, database, audio, scanner, integration and release-pipeline
checks. The main local quality gate is:

```bash
node scripts/check-version.mjs
pnpm check
pnpm build
pnpm schema:validate
pnpm licenses:check
cargo fmt --all -- --check
cargo clippy --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf
cargo test --all-targets
```

Browser-level checks use Playwright:

```bash
pnpm exec playwright install chromium
node scripts/smoke-ui.mjs
node scripts/ui-behaviour.mjs
```

Additional performance and memory-checking procedures are documented in
[Performance and validation](docs/PERFORMANCE.md).

## Project layout

```text
prism/
├── src/                    React and TypeScript interface
├── src-tauri/              Rust application, audio engine and catalogue
│   ├── migrations/         Embedded SQLite migrations
│   ├── src/                Application implementation
│   └── tests/              Rust integration and stress tests
├── assets/                 Application icon
├── docs/                   Technical and maintenance documentation
├── examples/plugins/       Example plugins
├── scripts/                Validation, auditing and benchmark tools
└── .github/workflows/      Continuous integration and release workflows
```

## Documentation

- [Documentation index](docs/README.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Performance and validation](docs/PERFORMANCE.md)
- [Plugin development](docs/PLUGINS.md)
- [Release process](docs/RELEASE.md)
- [Third-party licences](docs/LICENSES.md)

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) describes the development workflow, quality checks and pull
request conventions. Security reports are covered by [SECURITY.md](SECURITY.md).

## Licence

Prism is available under the [MIT Licence](LICENSE). The
[third-party licence inventory](docs/LICENSES.md) is generated from the Rust and npm lockfiles.
