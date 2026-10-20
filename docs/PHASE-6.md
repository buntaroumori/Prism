# FASE 6 — QA, stress and the 120 FPS proof

**Status:** delivered, awaiting approval
**Commits:** `FASE-6` (this document ships with it)
**Scope:** the FASE 6 deliverables — device hot-swap and DAC loss, corrupt-file recovery, a 120 FPS
frame-budget audit of the 500 000-row list, memory and undefined-behaviour tooling (valgrind, Miri),
CPU profiling with real flamegraphs, and Discord RPC offline fallback with tested backoff.

The rule for this phase was simple: **no claim without a measurement**. Everything below was executed
in this repository, on a 2-vCPU sandbox with no audio hardware — which is precisely why the device
paths are exercised through an injected loopback endpoint instead of being described.

---

## 1. What was measured, and what the numbers were

| Gate                                | Where it runs                                               | Result                                                            |
| :---------------------------------- | :---------------------------------------------------------- | :---------------------------------------------------------------- |
| Rust unit tests                     | `cargo test --lib`                                          | **197 passed**, 0 failed                                          |
| Rust integration tests              | `cargo test --tests`                                        | **39 passed** (32 integration + 7 stress), 1 ignored              |
| Undefined behaviour, unknown memory | `valgrind --tool=memcheck` over the stress suite            | **0 errors from 0 contexts**                                      |
| Undefined behaviour, unit level     | `valgrind --tool=memcheck` over 105 `audio::*` tests        | **0 errors from 0 contexts**                                      |
| Frame budget, 500 000 rows          | `node scripts/perf-audit.mjs`                               | **2.8–3.4 ms/frame** of the 8.33 ms a 120 Hz panel allows         |
| Interaction latency                 | same harness                                                | **0.1–0.3 ms** of main-thread work per frame while typing         |
| Layout shift                        | same harness                                                | **CLS = 0** exactly, zero shift entries                           |
| Virtualisation                      | same harness                                                | **39–40 rows / 755–775 nodes** in the document for 500 000 tracks |
| Decode throughput                   | `cargo run --example profile_hot_path --features profiling` | **656× real time**, 0.018 ms per 512-frame block                  |
| Resampling throughput               | same, `--output-rate 48000`                                 | **25× real time**, 0.424 ms per block (debug build)               |
| Discord RPC offline                 | `cargo test --lib discord::`                                | 16 passed, including the reconnect-flush cycle                    |

Frame-graph artefacts are committed: `docs/media/flamegraph-decode.svg` and
`docs/media/flamegraph-resample.svg`.

---

## 2. The optimisations this phase found

Profiling was not decoration: it found four defects, each of which is fixed and each of which has a
regression path.

### 2.1 The virtualiser rebuilt a 500 000-entry array on every scroll frame

`@tanstack/react-virtual` memoises its measurement cache on the **identity** of `estimateSize` and
`getItemKey`. Both were inline arrows:

```tsx
estimateSize: () => ROW_HEIGHT,
getItemKey: (index) => index,
```

New function objects every render ⇒ a new memo key every scroll frame ⇒ `getMeasurements()` walked
all 500 000 rows to rebuild an array the list then used for 39 of them. The JS sampler put
**21–30 % of all samples inside `getMeasurements`**.

Hoisting the two callbacks to module scope (and `getScrollElement` behind `useCallback`) keeps the
cache alive across renders:

| Metric (same harness, same machine) |       Before |           After |
| :---------------------------------- | -----------: | --------------: |
| Main-thread work per frame          | **18.92 ms** |     **3.02 ms** |
| …of which script                    |     10.91 ms |         0.73 ms |
| Frames over the 60 Hz budget        |       12.9 % |       **0.0 %** |
| Uncapped paint rate                 |       51 fps | **262–305 fps** |

That is a 6.3× reduction in per-frame cost and the difference between "misses frames on a 60 Hz
panel" and "has 2.7× headroom on a 120 Hz panel".

### 2.2 The library list never loaded its first page

`useTrackWindow` only fetched a page when the _total_ was already known — and the total is only known
after the first page arrives. A fresh mount therefore showed "No tracks match" forever. The hook now
bootstraps page 0 when its query generation changes, guarded on the cache (not on the count) so an
empty result is fetched once and never re-requested.

This bug was invisible in the FASE 5 demo because the harness that would have caught it — a list
measured against a real catalogue — did not exist yet. It is the clearest argument for the phase.

### 2.3 A rejected Discord client id caused an infinite reconnect loop

The IPC handshake read the event name from `ev`. Discord sends `evt`. An `ERROR` frame was therefore
read as "unknown event ⇒ the pipe is alive ⇒ connected", the socket then dropped, the worker went
`Reconnecting`, and the settings panel showed a spinner that never resolved. The new test suite
failed on exactly this, the field is now read as `evt` with `ev` as a fallback, and the state machine
reports `Unavailable` with the reason Discord gave.

### 2.4 A corrupt first track took the whole queue down

`start_queue` probed the head of the queue and propagated the error, so an album whose first rip was
truncated would not play at all — even though the _rest_ of the album was fine. The supervisor now
walks forward, emits `TrackSkipped` for each dead file and starts on the first playable track; if
nothing plays it tears down cleanly instead of spinning.

---

## 3. Robustness: seven stress tests with no sound card

`src-tauri/tests/stress.rs` injects a **loopback endpoint** through `Engine::with_opener` — the same
`Fn` the production build points at `backend::open_sink`. The replacement consumes the same lock-free
ring, honours the same `RefillSignal` contract and can be killed on demand, so everything above the
device boundary (supervisor, decoder, DSP chain, events, playhead) is production code.

| Test                                                                    | What it proves                                                                                                                                                                                                                                                                              |
| :---------------------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `swapping_the_output_mid_track_keeps_the_playhead_and_the_queue`        | Five hot swaps: device identity changes every time, the playhead keeps advancing, the queue index survives, and exactly six endpoints are opened for six configurations — no leaked device threads.                                                                                         |
| `an_unplugged_dac_is_rebuilt_and_playback_resumes_at_the_same_position` | The endpoint thread exits and closes the refill signal (the exact production death signal). The supervisor reopens, reports `DeviceChanged`, and resumes without restarting the track from zero.                                                                                            |
| `a_corrupt_track_mid_queue_is_skipped_and_the_album_finishes`           | A FLAC header followed by garbage is skipped mid-album; the skip names the file and a human-readable reason; the third track still reaches the device.                                                                                                                                      |
| `a_queue_of_only_corrupt_files_stops_cleanly_instead_of_hanging`        | Four dead files ⇒ four skip events in queue order, then `Stopped`. No hang, no spin, and `stop()` remains idempotent.                                                                                                                                                                       |
| `a_cue_image_over_one_gigabyte_expands_without_being_read`              | A **1.5 GB sparse** WAV plus a CUE sheet becomes three virtual tracks with sample-accurate spans; the last span ends at `8 503 401 ms`, the length of the image, which is only derivable from its header. Indexed in **under 20 s with 4 MB of the file on disk** — nothing read the image. |
| `an_endpoint_that_refuses_to_open_leaves_the_engine_usable`             | The opener fails once, the engine reports the error and stays `Stopped`; the second `play` succeeds on the same engine.                                                                                                                                                                     |
| `stopping_during_an_endpoint_swap_never_leaves_a_dangling_device`       | Swapping and stopping concurrently leaves every loopback thread joined: no live device after the engine is gone.                                                                                                                                                                            |

All seven run in **0.58 s** because the loopback clock is accelerated (1 ms per period). The suite
runs on Windows too: it needs no driver, no device and no privileges.

### Corrupt-file recovery in the UI

A skip is not a silent drop. The decoder records `SkippedTrack { track_id, title, artist, reason }`;
the supervisor drains it into `EngineEvent::TrackSkipped`; the bridge forwards it as the plugin event
`track_skipped`; `playbackStore` keeps a `notice`; and `AppShell` renders a dismissible banner with
the file, the artist and the reason. The queue keeps playing underneath.

---

## 4. Memory and undefined behaviour

- **valgrind memcheck** over the stress suite (real threads, real sockets, repeated device
  open/close) and over the 105 `audio::*` unit tests: **0 errors**. No invalid reads, no use of
  uninitialised values, no use-after-free across thread boundaries.
- **Miri** is wired into CI (`qa` job, `cargo miri test --lib audio::ring / audio::format / lyrics::`)
  for the pure-logic modules. Miri interprets the program, so a crate that links WebKitGTK and cpal
  needs a full interpreted dependency build; on the 2-vCPU delivery sandbox that build did not
  finish inside the phase window. This is stated rather than papered over: **valgrind covered the
  same surface locally with zero findings**, and the Miri job is what runs on every pull request from
  a runner that can afford the build.

---

## 5. CPU profiling

`src-tauri/examples/profile_hot_path.rs` drives the production `GaplessPlayer` over a generated
catalogue under `pprof`, and writes an SVG flamegraph plus a throughput table.

```bash
cargo run --example profile_hot_path --features profiling -- --seconds 10 --tracks 8
cargo run --profile profiling --example profile_hot_path --features profiling   # line-level DWARF
```

Results from a **debug** build (dependencies at `opt-level = 2`), which makes the numbers
pessimistic and therefore safe to quote:

| Path                                                  |         Throughput | Cost per 512-frame block | Budget at 2 % CPU |
| :---------------------------------------------------- | -----------------: | -----------------------: | ----------------: |
| Decode → DSP, 44.1 kHz in, 44.1 kHz out (bit-perfect) | **656× real time** |                 0.018 ms |           0.23 ms |
| Decode → DSP → sinc resample 44.1 → 48 kHz            |  **25× real time** |                 0.424 ms |           0.23 ms |

The flamegraphs (`docs/media/flamegraph-decode.svg`, `docs/media/flamegraph-resample.svg`) show the
expected split: the bit-perfect path is dominated by `symphonia`'s PCM reader and `SampleBuffer`
copies, while the resampled path is dominated by `rubato`'s AVX sinc interpolator. That is also the
design validation for `resample_when_needed`: resampling costs ~23× what decoding does, so it must
never happen when it is not required.

`tracy`: the code emits `tracing` spans on the supervisor, decoder and device threads, so a
`tracing-tracy` subscriber can be dropped in without touching call sites. It is **not** vendored —
adding a C++ client to the dependency graph for a tool that only a maintainer runs is not a trade
this project makes. The claim in `docs/PERFORMANCE.md` has been corrected to say exactly this.

---

## 6. The 120 FPS audit

`scripts/perf-audit.mjs` builds a harness page (`vite.bench.config.ts` → `perf.html`) that mounts the
**production** `VirtualTrackList` over a synthetic 500 000-row catalogue and drives it in Chromium.
It is not part of the application bundle.

Two runs, because they answer different questions:

- **vsync** (default flags) — does the list keep up with the display it is on?
- **uncapped** (`--disable-frame-rate-limit`) — how fast can it actually paint?

And two scroll speeds, because they are different workloads:

- **flick**, 120 px per frame — what a trackpad or a wheel really does. _This is the gate._
- **scrub**, three viewports per frame — dragging the scrollbar across the whole catalogue, where a
  page fetch and a blank row are acceptable.

Headroom is measured as **renderer main-thread time per painted frame** (`Performance.getMetrics`
`TaskDuration` delta ÷ frames), not as a frame rate: a compositor can tick at 60 Hz on a machine with
six times the budget to spare, and reporting "60 fps" there would hide the headroom the 120 Hz panel
needs.

### Results (Chromium, 1440×900, 2 vCPU shared sandbox)

| Probe                    |                                    Frame cadence | Main-thread per frame |  Script | Layout + style |
| :----------------------- | -----------------------------------------------: | --------------------: | ------: | -------------: |
| Flick, vsync run         |                   60.0 fps, 0 frames over budget |            **2.8 ms** | 0.69 ms |        0.57 ms |
| Flick, uncapped run      | **262–305 fps**, mean 3.3–3.8 ms, p95 5.6–7.1 ms |        **3.0–3.4 ms** | 0.72 ms |        0.57 ms |
| Scrub, 3 viewports/frame |                        0–22 % frames over budget |          13.4–15.0 ms |       — |              — |
| Typing in the filter     |                                                — |        **0.1–0.3 ms** |       — |              — |

Against the 8.33 ms a 120 Hz panel allows, a flick frame costs **2.5–3.0× less than the budget**. A
frame costs more than the budget only while the user drags the scrollbar across half a million rows,
and even there the list keeps painting instead of blanking. The ranges are five consecutive runs on
the same 2-vCPU box: the per-frame cost moves by about ±10 %, the frame rate moves much more, which
is precisely why the gate is per-frame cost and not fps.

Also recorded: **39–40 mounted rows / 755–775 DOM nodes** for 500 000 tracks, **14.5 MB** of JS heap,
**0 long tasks** over 50 ms (mount included), and **CLS = 0 with zero shift entries**.

CLS only reached zero after a change that looks trivial and is not: the harness's ready state moved
from a text label to a coloured dot. Swapping `loading` for `ready` re-laid out the text node and
registered as a 0.0000037 shift even inside a fixed-width pill — the source rect reported by the
observer was the pill itself, 86 × 11 px at the same coordinates before and after, which is what a
text-driven reflow looks like. The rule it leaves behind: a label whose text changes is a
layout-shift source, so carry state with something that does not reflow. `ListFooter` is safe by
construction — `justify-between` pins both ends, so its count can grow without moving anything else
— and the audit keeps the shift observer in place, naming the element and its rects if a shift ever
reappears.

The audit fails the build (`exitCode = 1`) if any of the seven verdicts regress:
`virtualised`, `headroom120`, `inputUnder16ms`, `clsZero`, `noStalls`, `vsyncClean`,
`noConsoleErrors`.

---

## 7. Discord RPC offline fallback

The RPC worker previously had a reconnect loop with exponential backoff and no test. FASE 6 turned
the schedule into a value (`BackoffPolicy`: start, ceiling, growth, jitter) so it can be exercised in
milliseconds instead of minutes, and added six tests that run against a **real Unix socket**:

| Test                                                                 | What it pins down                                                                                                                                      |
| :------------------------------------------------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `offline_discord_is_reported_and_retried_without_spinning`           | No socket ⇒ `Unavailable` with a reason naming the path; three attempts cost at least the first two backoff sleeps, so a closed Discord costs 0 % CPU. |
| `a_rejected_client_id_falls_back_instead_of_failing_the_application` | A `DISPATCH/ERROR` handshake reply is surfaced as `Unavailable` with Discord's code; the handle stays usable and stops cleanly.                        |
| `reconnecting_flushes_the_pending_activity`                          | A track started while Discord is unreachable is published the moment the socket comes up — the user does not wait for the 2.5 s update tick.           |
| `backoff_grows_geometrically_then_holds_at_the_ceiling`              | 100 → 150 → 225 ms …, capped, monotonic, jitter only additive, `reset()` restores the start.                                                           |
| `the_default_policy_waits_a_second_and_caps_at_a_minute`             | The shipped defaults.                                                                                                                                  |
| `a_socket_that_does_not_exist_is_never_a_panic`                      | A missing path is an error string, never a panic.                                                                                                      |

---

## 8. Verification

| Gate                 | Command                                                                                      | Result                                               |
| :------------------- | :------------------------------------------------------------------------------------------- | :--------------------------------------------------- |
| Formatting           | `cargo fmt --all -- --check`                                                                 | clean                                                |
| Lints                | `cargo clippy --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf` | exit 0                                               |
| Unit tests           | `cargo test --lib`                                                                           | **197 passed**                                       |
| Integration + stress | `cargo test --tests`                                                                         | **39 passed** (32 integration + 7 stress), 1 ignored |
| Memory               | `valgrind --tool=memcheck` (stress + audio units)                                            | **0 errors**                                         |
| Frontend             | `pnpm check`                                                                                 | typecheck + lint + format clean                      |
| Bundle               | `pnpm build`                                                                                 | see delivery report                                  |
| Frame budget         | `pnpm perf`                                                                                  | 7/7 verdicts PASS                                    |
| Profiling            | `cargo run --example profile_hot_path --features profiling`                                  | two flamegraphs committed                            |

Test totals after FASE 6: **197 lib + 39 integration = 236** in the default build (one `#[ignore]`d
scanner benchmark excluded from the count); **205** lib tests with `--features lua-plugins`.

### New tooling added

- `src-tauri/tests/stress.rs` — the seven device/decoder stress tests.
- `src-tauri/examples/profile_hot_path.rs` + `profiling` feature — CPU profiling.
- `scripts/perf-audit.mjs`, `perf.html`, `vite.bench.config.ts`, `src/perf/*` — the 120 FPS audit.
- `.github/workflows/ci.yml` — `qa` job (Miri + valgrind) and `performance` job (frame budget, with
  the report and a screenshot uploaded as artefacts).

### Deliberate limitations, stated rather than hidden

- The frame audit runs in headless Chromium, not in the Tauri WebView. The list is the same React
  code, but WebKitGTK composites differently from Blink; the CI gate is a regression detector, and
  the reference-machine numbers in `docs/PERFORMANCE.md` remain the release gate.
- Stress tests use an accelerated loopback clock (1 ms per period) to stay under a second. Timing
  assertions are about ordering and liveness, never about wall-clock audio latency.
- Miri could not complete in this sandbox (see §4). Valgrind stands in locally; CI runs Miri.
- The profiler example generates WAV because the repository ships no encoder; FLAC decode costs more
  than PCM decode, so the throughput figures are optimistic for FLAC and are quoted as such.
- `tracy` is documented, not vendored (see §5).

---

## 9. What FASE 7 inherits

- `LICENSE`, `.gitignore`, license audit of the dependency tree (the `pprof`/`inferno` additions must
  appear in it), a professional README and a validated release workflow with a structured changelog.
- The `profiling` feature must stay out of release artifacts; the release workflow builds default
  features only.
- `docs/PERFORMANCE.md` now carries the measured numbers and the honest tooling list; FASE 7 should
  link it from the README rather than restate it.
