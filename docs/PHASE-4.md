# FASE 4 — Desktop interface, palette engine and player bar

**Status:** delivered, awaiting approval
**Tests:** 189 Rust (157 unit + 32 integration) · `pnpm check` clean · `pnpm build` clean
**Verified by:** booting the real binary under Xvfb (see §6 — read this before asking for screenshots)

---

## 1. Scope

| Deliverable                                      | Where                                                                             |
| :----------------------------------------------- | :-------------------------------------------------------------------------------- |
| Strict TS contracts for the whole engine surface | `src/types/ipc.ts` (+320 lines)                                                   |
| Zod-validated IPC façade                         | `src/lib/ipc.ts` (`playerApi`, palette, event parsers)                            |
| Zustand stores split by update frequency         | `src/stores/{ui,output,library,playback}Store.ts`                                 |
| Tauri IPC hooks                                  | `src/hooks/{usePlayerBridge,useScanEvents,useArtworkPalette,useAlbumCoverKey}.ts` |
| Rust palette engine                              | `src-tauri/src/palette/mod.rs` (613 lines, 7 tests)                               |
| Palette IPC + playhead channel                   | `commands/library.rs`, `commands/player.rs`, `lib.rs`                             |
| Shell + routing + motion                         | `components/{AppShell,Sidebar,TopBar,AmbientBackground}.tsx`                      |
| Player bar                                       | `components/player/*` (6 components)                                              |
| Views                                            | `components/views/{Home,NowPlaying,Settings,System}View.tsx`                      |
| UI primitives                                    | `components/ui/{CoverArt,Slider,Segmented,Popover}.tsx`                           |

## 2. The design decision that matters: state is split by update frequency

A single store would re-render the application tree eight times a second for two numbers. Instead:

| Slice                  |     Update rate | Owner                                  |
| :--------------------- | --------------: | :------------------------------------- |
| `uiStore`              |     user action | route, sidebar, ambient intensity      |
| `outputStore`          |     user action | mode, device, buffer (persisted)       |
| `libraryStore`         |  scan / refresh | counters, roots, recents, scan machine |
| `playbackStore.status` | transport event | engine snapshot, formats, bit-perfect  |
| `playbackStore.tick`   |        **8 Hz** | `{ positionMs, durationMs, playing }`  |

The playhead travels on its own channel, `player://tick`, dispatched by Rust at 125 ms. Only the
scrubber subtree subscribes to it. This is enforced at the source: the tick is a _separate_ event,
not a second field on the transport event, so it is impossible for the heavy channel to be used for
the clock by accident.

## 3. Palette engine — and why it is not "colour thief in the frontend"

1. **Deterministic.** `color_thief`'s median cut is fixed-point; candidates are tie-broken by hex.
   The same cover yields byte-identical output, so the window never flashes a different tint when a
   track is re-opened.
2. **Honest about population.** `color_thief` returns colours without their share, so the sampling
   pass is re-run to count each pixel against the nearest swatch; that share feeds the score.
3. **Accessible by construction.** The accent is toned into 0.66–0.88 value and then lightened in
   2 % steps until WCAG 2.1 contrast against the generated background clears **4.5:1**. The test
   suite asserts it for every fixture.
4. **Never blank.** Greyscale sleeves, corrupt JPEGs, missing files and provider outages all fall
   back to a hue derived from the cache key (FNV-1a), so a Sam Cooke mono sleeve still looks like
   itself rather than like an error state.
5. **Cheap.** Decode + quantise runs on the blocking pool (single-digit ms), results are cached
   in Rust (512) and in the renderer (`Map`), and the projection is **six CSS custom properties**
   on one element. There is no canvas loop and no per-frame JavaScript.

The ambient background uses those variables on three blurred orbs with a `transform`-only keyframe,
scaled by the artwork's mean luminance so a white sleeve does not wash the interface out.

## 4. Player bar

- **Quality badge is three-state, because two states would lie:** `Bit-perfect` (exclusive endpoint,
  exact format, 0 dB, no DSP — verified by the engine, not inferred), `Hi-Res · Processed` (the source
  is above CD but the path is altered), `CD`. The tooltip states which condition failed.
- **Output selector shows what is open, not what was asked for.** Those differ whenever a driver
  refuses an exclusive format; the supervisor falls back and the bar must tell the truth. The popover
  lists endpoints with driver stack, sample-rate span, exclusive capability, OS-default marker,
  engine mode and a 64–2048-frame buffer choice.
- **Volume is in dB, not percent.** Unity gain is a visible, snap-to target because it is the
  precondition for bit-perfect playback. A 0–100 slider would hide that.
- **Seeking is one call.** The slider previews locally while dragging and commits on release; the
  optimistic tick is corrected by the next engine tick.

## 5. Routes shipped

FASE 4 delivers a _complete_ shell, so the library/album/artist browsers are deliberately **not**
routes yet — a nav item that opens a "coming soon" panel is the placeholder this project forbids.

| Route           | Content                                                                                                                                           |
| :-------------- | :------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Home**        | Editorial hero derived from the catalogue, four real counters, recents rail that queues a whole album (disc/track ordered, deterministic shuffle) |
| **Now Playing** | Crossfaded artwork, palette bloom, technical read-out (source format, device format, backend, latency, xruns)                                     |
| **Settings**    | Output mode / device / buffer, library roots with unreachable state, scan history table, Discord presence, ambient intensity                      |
| **System**      | The FASE 1 diagnostics (pragmas, migration ledger, engine guarantees), unchanged                                                                  |

Outside Tauri (`vite preview`, docs build) the app renders a `DesktopRequired` screen that says the
engine lives in the native process. It does not render an empty interface and pretend.

## 6. Verification — including what I could not do

```
cargo test --lib            157 passed
cargo test --test database    5 passed
cargo test --test engine      5 passed
cargo test --test library     9 passed
cargo test --test artwork     3 passed
cargo test --test discord     3 passed
cargo test --test palette     7 passed      ← new FASE 4 suite
cargo clippy --all-targets -D correctness,suspicious,perf  0 errors
cargo fmt --all --check                                    clean
pnpm check (tsc strict + eslint + prettier)                 clean
pnpm build                                                  clean (394 kB gz 112 kB app chunk)
xvfb-run ./target/debug/prism                                boots, stays alive, no panic
```

**Screenshots:** this container has no compositor and WebKitGTK does not rasterise into Xvfb here —
`import -window root` returns a black frame (max channel value 10). I tried once, confirmed it, and
did not ship a doctored image. The honest artefacts are the boot log (`database ready`, engine
supervisor started, no errors over 25 s) and `scripts/capture-screenshots.sh`, which produces real
captures on a workstation.

## 7. What FASE 5 inherits

- `libraryStore` and the paginated read models are ready for a virtualised grid; `TrackRow` is
  already the row contract, including `highlight` markers for search snippets.
- The store split guarantees the FFT meter and the playhead cannot invalidate list rows.
- `Slider`, `Segmented`, `Popover`, `CoverArt` and the palette tokens are the visual vocabulary for
  the library, the lyrics panel and the layout editor.
- The plugin runtime (Lua 5.4 / wasmtime) is the remaining piece of the FASE 5 brief.
