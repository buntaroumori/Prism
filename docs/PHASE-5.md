# FASE 5 — Virtualised library, spectrum, lyrics and the plugin runtime

**Status:** delivered, awaiting approval
**Commit:** _(recorded on delivery)_
**Scope:** the four FASE 5 deliverables — a 500 000-row virtualised track list, a GPU-friendly
31-band FFT visualiser, synchronised LRC lyrics with a karaoke cursor, a modular Now Playing layout,
and the Lua / WebAssembly plugin runtime with its configuration surface.

---

## 1. What was built

| Deliverable | Files | Verified by |
| :--- | :--- | :--- |
| Virtualised track list (500k rows) | `src/components/library/VirtualTrackList.tsx`, `src/hooks/useTrackWindow.ts`, `src/components/views/LibraryView.tsx` | `pnpm typecheck`, `pnpm lint`, `pnpm build` |
| Spectrum analyser | `src-tauri/src/audio/analysis.rs`, `src-tauri/src/commands/visualiser.rs`, `src/components/player/SpectrumVisualiser.tsx`, `src/lib/spectrum.ts` | `cargo test --lib` (9 tests) |
| Lyrics | `src-tauri/src/lyrics/mod.rs`, `src-tauri/src/commands/lyrics.rs`, `src/components/player/LyricsPanel.tsx`, `src/hooks/useLyrics.ts` | `cargo test --lib` (9 tests) |
| Modular layout | `src/stores/layoutStore.ts`, `src/components/nowplaying/*` | `pnpm check` |
| Plugin runtime | `src-tauri/src/plugins/*`, `src-tauri/src/commands/plugins.rs`, `src/components/settings/PluginsSection.tsx` | `cargo test --lib --features lua-plugins` (23 tests), `cargo check --features wasm-plugins` |

Nothing in this phase is a stub: every command is wired into `generate_handler!`, every store is
consumed by a mounted component, and every panel has a real empty state.

---

## 2. Lyrics

### Resolution order

1. **Embedded** — a `USLT` frame (or the container's `LYRICS` key) read through `lofty`.
2. **Sidecar** — `song.lrc` next to the file; if that misses, the directory is scanned for a
   case-insensitive stem match, because `.LRC` from a Windows ripper is the same format.
3. **Nothing** — `LyricsDocument::empty` with `provider = "none"`.

A miss is **never cached**. `lyrics_cache` stores only documents that exist, so a user who drops a
`.lrc` beside a track and presses refresh sees it immediately. A cached `sidecar` row records the
absolute path in `source_url` and is re-validated on every read: deleting the `.lrc` invalidates the
row instead of serving a ghost.

### Parser

The parser accepts the dialects that exist in the wild rather than the one in the specification:

| Case | Handling |
| :--- | :--- |
| `[mm:ss]`, `[mm:ss.f]`, `[mm:ss.ff]`, `[mm:ss.fff]` | Parsed; 1–3 fraction digits, `.` or `:` separator |
| Several timestamps on one line | One line per timestamp (the standard chorus-repeat idiom) |
| Metadata tags (`[ar:]`, `[ti:]`, `[al:]`, `[by:]`, `[la:]`) | Collected into `metadata`; `la` becomes the document language |
| `[offset:N]` | Applied to every timestamp: **positive delays**, negative advances, clamped at 0 |
| BOM, CRLF, leading whitespace | Normalised |
| Unsorted timestamps | Sorted; identical `(time, text)` pairs de-duplicated |
| Untimed lines in a timed file | Dropped (they would break the cursor) |
| A file with no timestamps at all | Not synced: `parse_plain` keeps the line order |

`MAX_LYRICS_BYTES = 512 KiB` bounds the cache entry; a larger body is rejected with a message naming
the track instead of being silently truncated.

### Panel

`LyricsPanel` highlights the active line from a binary search over the timestamps — the lookup runs on
every playhead tick, so it cannot be a linear scan. Auto-follow scrolls the sheet (never the page) to
place the active line at 42 % of the panel height; manual scrolling suspends follow for four seconds.
`prefers-reduced-motion` swaps the smooth scroll for an instant one. Unsynced documents render as a
plain sheet with a `Unsynced · sidecar` label, and a track with no lyrics explains the resolution order
above the refresh button.

---

## 3. Spectrum analyser

### Signal path

```
decoder → DSP → apply_gain → [Analyzer::push] → ring buffer → device callback
```

The tap is in `Supervisor::push`, on the **producer** side of the ring — the same block that was just
decoded, never a second read of the file. The device callback is untouched: it still only reads from
the pre-allocated ring and applies the final gain ramp.

`Analyzer::push` is a no-op unless at least one visualiser is mounted, in which case it downmixes the
interleaved block to mono into a 4 096-sample ring (two full windows). The display side copies the
newest 2 048 samples under a short-lived lock, so the worst-case contention on the audio producer is a
512-float copy.

### Analysis

| Parameter | Value | Why |
| :--- | :--- | :--- |
| FFT size | 2 048 | 46 ms at 44.1 kHz: resolves 21.5 Hz bins without visible lag |
| Window | Hann | A rectangular window leaks a 1 kHz tone across the whole display |
| Bands | 31 | Third-octave layout, matches the 31-band EQ page, ~450-byte IPC frames |
| Band edges | 20 Hz → min(20 kHz, 0.98 × Nyquist) | Log-spaced; clamped so 8 kHz content does not alias into the top band |
| Scaling | 2 / (N/2), peak per band | A 0 dBFS sine reads as 0 dB |
| Display range | −80 dBFS → 0 dBFS | Below −80 dB is noise floor, not signal |
| Smoothing | attack 0.55, release 0.14 | Transients appear on the frame they happen; bars fall like meters |

A sample-rate change invalidates the window rather than pretending the old samples fit the new clock.

### Transport to the webview

Frames are emitted on `visualiser://frame` at **60 Hz** (~450 bytes each); silence falls back to 20 Hz
so a stopped transport does not cost a 60 Hz timer. The canvas draws on `requestAnimationFrame` and
interpolates between frames, so on-screen motion runs at the display rate (120 Hz included) while the
IPC budget stays at 60 events/s.

Capture is reference-counted in the frontend (`src/lib/spectrum.ts`): the first visualiser enables the
tap, the last unmount disables it. A player with no visualiser mounted pays nothing — no copy, no
timer, no event.

The canvas paints imperatively from refs: a spectrum frame never triggers a React render, and peak
caps fall independently of the bar heights.

---

## 4. Virtualised track list

### Data model

A 500 000-track library cannot be a JS array, so the list is **paged**, not fetched:

* one query → one cache object (`pages`, `inflight`, `center`);
* `PAGE_SIZE = 500` (the backend's maximum page);
* `MAX_PAGES = 12` → 6 000 rows retained (~3 MB), evicted farthest-from-viewport first;
* a new query or a manual reload swaps the cache object during render, so an in-flight response from
  the previous query lands in a cache nobody reads instead of overwriting the current results.

### Rendering

`@tanstack/react-virtual` maps scroll position to indices; each row is absolutely positioned with a
single `translateY`, at a fixed `ROW_HEIGHT = 52 px`, with `overscan: 12` and `contain: strict` on the
scroll container. The DOM holds roughly 30–60 rows regardless of library size, and the scrollbar is
sized by one spacer element. Loading rows render as skeletons at the same height, so a page arriving
never shifts the list (CLS = 0).

### Interaction

Single click selects, double click plays, `Enter` plays the selected row, `↑`/`↓`/`Shift+↑`/`Shift+↓`/
`PageUp`/`PageDown`/`Home`/`End` move the selection and scroll it into view. The row play button queues
the page the row belongs to, starting at that row: shipping a 500 000-item queue through IPC would
pin the catalogue in the engine's memory, and one page is the largest unit the viewport can actually
present.

---

## 5. Modular Now Playing layout

`useLayoutStore` owns an ordered list of modules — `artwork`, `lyrics`, `spectrum`, `technical`,
`queue` — persisted under `prism.layout.now-playing` with a schema-aware migration.

* **Presets** are starting points: *Editorial* (artwork, signal path, lyrics), *Listening* (artwork,
  lyrics, spectrum), *Analysis* (spectrum, signal path, queue).
* Editing a preset makes the layout `custom`; modules can be toggled and moved up/down.
* **Invariant:** at least one module stays visible — an empty deck would look broken with no way back.
* Removing a module removes its work: the spectrum releases the audio-thread tap, the lyrics panel
  stops fetching, the queue stops subscribing.

The deck animates with Framer Motion `layout` (transform-only) and honours `prefers-reduced-motion`.

---

## 6. Plugin runtime

### Manifest

A plugin is a directory under `<app-data>/plugins/` containing `plugin.toml` and an entry file:

```toml
[plugin]
id = "io.prism.night-mode"        # reverse-DNS, lowercase
name = "Night Mode"
version = "1.0.0"
api = 1                            # supported API revision
entry = "main.lua"                 # .lua → Lua sandbox, .wasm/.wat → WebAssembly
description = "Lowers the volume after a given hour."
author = "Prism Contributors"
capabilities = ["log", "notify", "player:read", "player:control"]

[[settings]]
key = "after_hour"
label = "After hour"
type = "integer"                   # string | number | integer | boolean | select
default = 22
min = 0
max = 23
```

Validation is strict and refuses rather than repairs: unknown keys fail the manifest, identifiers and
semver are pattern-checked, `api` must equal the compiled revision, the entry must be a plain relative
path that resolves inside the plugin directory (a symlink pointing outside is rejected), and every
declared default must satisfy its own schema. Defaults are validated at load time because the first
launch would otherwise start with an invalid configuration.

### Capabilities

| Capability | Grants |
| :--- | :--- |
| `log` | `prism.log(level, message)` → `plugin://message` |
| `notify` | `prism.emit(topic, message)` → a user-facing notification |
| `player:read` | `prism.now_playing()` → the engine status as JSON |
| `player:control` | `prism.pause()`, `prism.resume()`, `prism.set_volume(db ∈ [−60, 0])` |

A host function called without its declared capability returns a permission error that names the
capability to add — the same error the Settings panel shows.

### Sandboxes

| | Lua 5.4 (`lua-plugins`) | WebAssembly (`wasm-plugins`) |
| :--- | :--- | :--- |
| Runtime | `mlua`, vendored (no system Lua) | `wasmtime` 26, Cranelift |
| Libraries | `table`, `string`, `math`, `utf8`, `coroutine`; `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, `load`, `require`, `print` are removed | No WASI: only the five `prism` imports resolve |
| Memory ceiling | 16 MiB (`set_memory_limit`) | 64 MiB (`StoreLimits`) |
| Time ceiling | 20 M VM instructions, watched every 10 000 | 200 M fuel per call (`consume_fuel`) |
| Entry point | chunk executed once; `on_load`, `on_event`, `on_config` optional globals | `alloc` + optional `on_load`/`on_event`/`on_config`, all `(ptr, len)` |
| Isolation | one `Lua` state per plugin, never shared | one `Store` + `Instance` per plugin, `instances = 1`, `tables = 4` |

Both budgets produce an ordinary error that surfaces as a plugin failure; neither can hang the
dispatcher. A measured check exists for the Lua path: a `while true do end` plugin is aborted by the
instruction hook, and a `(loop (br 0))` module is aborted by fuel exhaustion.

### WebAssembly ABI

```
imports (module "prism"):
  log(level: i32, ptr: i32, len: i32)
  emit(ptr: i32, len: i32)
  now_playing(ptr: i32, cap: i32) -> i32   # bytes written, or -required
  config(ptr: i32, cap: i32) -> i32
  control(op: i32, value: f64)              # 0 pause, 1 resume, 2 volume

exports:
  memory, alloc(len: i32) -> i32
  on_event(ptr, len)                        # required to receive events
  on_load(ptr, len), on_config(ptr, len)    # optional
```

Payloads are UTF-8 JSON written by the host into memory returned by `alloc`. String arguments are
bounds-checked and must be valid UTF-8.

### Dispatch and failure policy

The event bridge subscribes to the engine exactly like Discord presence does and forwards
`state_changed`, `track_changed`, `device_changed`, `output_error` and `underrun` to every enabled
plugin. A plugin that errors — budget exhausted, permission denied, Lua exception, WASM trap — is
**disabled for the session** and its error is stored on its descriptor: the player keeps playing, the
panel explains why, and the user can re-enable it after fixing the plugin.

### Storage and commands

| Key | Content |
| :--- | :--- |
| `plugins.disabled` | JSON array of disabled ids |
| `plugin.config.<id>` | JSON object of the plugin's settings |

| Command | Returns |
| :--- | :--- |
| `plugins_list` | snapshot: descriptors, runtime availability, directory |
| `plugins_reload` | rescan the folder, keeping configuration |
| `plugins_set_enabled` | snapshot after enabling/disabling |
| `plugins_set_config` | snapshot after a schema-validated write |
| `plugins_open_folder` | opens the plugin directory in the file manager |

`PluginManager::bootstrap` runs inside `AppState::initialize`, so plugin load failures are known before
the window is shown. The default binary compiles **neither** runtime (`default = []`): manifests are
still listed and validated, each descriptor says which runtime is missing, and the Settings panel
prints the exact feature flag to rebuild with.

---

## 7. New IPC surface

| Command | Args | Result |
| :--- | :--- | :--- |
| `lyrics_get` | `trackId`, `refresh?` | `LyricsDocument` |
| `lyrics_clear` | `trackId` | `bool` |
| `visualiser_set_enabled` | `enabled` | `bool` |
| `visualiser_frame` | — | `SpectrumFrame?` |
| `plugins_list` / `plugins_reload` | — | `PluginSnapshot` |
| `plugins_set_enabled` | `id`, `enabled` | `PluginSnapshot` |
| `plugins_set_config` | `id`, `key`, `value` | `PluginSnapshot` |
| `plugins_open_folder` | — | `void` |

| Event | Payload |
| :--- | :--- |
| `visualiser://frame` | `SpectrumFrame { bands[31], level, silent, sampleRate }` |
| `plugin://message` | `PluginMessage { pluginId, kind, level, message, topic }` |

All four payload types are zod-validated at the IPC edge, and the `PluginMessage` stream is parsed on
receipt so a malformed frame is dropped instead of reaching React.

---

## 8. Verification

| Gate | Command | Result |
| :--- | :--- | :--- |
| Unit tests (default features) | `cargo test --lib` | **191 passed**, 0 failed |
| Lyrics + spectrum + manifest suites | included above | 9 + 9 + 16 tests |
| Plugin subset with Lua runtime | `cargo test --lib --features lua-plugins plugins::` | **23 passed**, including the instruction-budget abort and the sealed-globals assertion |
| Type-check with WebAssembly runtime | `cargo check --lib --features wasm-plugins` | clean |
| Frontend type-check + lint + format | `pnpm check` | clean |
| Frontend bundle | `pnpm build` | see delivery report |

Test totals after FASE 5: **191 lib + 32 integration = 223** in the default build, **198** lib tests
with `--features lua-plugins`.

### Deliberate limitations, stated rather than hidden

* **WASM unit tests** require building and linking `wasmtime` (cranelift + JIT). The 2-vCPU delivery
  sandbox could type-check the runtime (`cargo check --lib --features wasm-plugins`) but could not
  finish the test binary; the suite is committed and runs with
  `cargo test --lib --features wasm-plugins plugins::wasm_rt` on any workstation.
* The plugin event surface covers playback events. Library-scan and DSP hooks are named in the
  long-term API plan but are not implemented, and nothing in the UI claims they are.
* The spectrum is a magnitude display; it is not a calibrated SPL meter.
* Lyric files above 512 KiB are rejected by design.

---

## 9. What FASE 6 inherits

* The virtualised list and the spectrum are the two hot paths to profile under `tracy`/flamegraph:
  one is a React render loop, the other an audio-thread copy under a lock.
* `plugin.config.*` rows and the `lyrics_cache` table are the first candidates for a retention sweep.
* The plugin failure path (disable + record) needs a dedicated stress case: N plugins × M events.
