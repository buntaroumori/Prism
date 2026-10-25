# Performance and validation

Prism uses automated checks for regressions and targeted profiling for hot paths. Measurements are
only comparable when the commit, build profile, workload and hardware are recorded. A shared CI
runner is useful for detecting structural regressions, but it is not a reference audio or display
system.

## Validation layers

| Layer                 | Purpose                                                      | Main command or workflow              |
| :-------------------- | :----------------------------------------------------------- | :------------------------------------ |
| Frontend contracts    | Type safety, linting and formatting                          | `pnpm check`                          |
| Production bundle     | Verify the shipped frontend can be built                     | `pnpm build`                          |
| Startup smoke         | Mount the production bundle under its desktop CSP            | `node scripts/smoke-ui.mjs`           |
| Interaction behaviour | Drive playback, filters, lyrics, navigation and window flows | `node scripts/ui-behaviour.mjs`       |
| Database              | Apply migrations and check constraints, triggers and views   | `pnpm schema:validate`                |
| Rust tests            | Exercise audio, catalogue, Discord and integration behaviour | `cargo test --all-targets`            |
| Frame audit           | Check virtualisation, layout stability and renderer cost     | `pnpm perf`                           |
| Memory checks         | Inspect native execution and selected pure-Rust modules      | Valgrind and Miri                     |
| Release validation    | Check versioning, licences, workflow and artefact coverage   | `python3 scripts/validate_release.py` |

## Frontend frame audit

The performance harness is separate from the application bundle. It mounts the production virtual
track list with a synthetic catalogue and measures browser behaviour during controlled scrolling
and filtering.

```bash
pnpm perf
```

This command builds the benchmark and writes `perf-report.json`. The audit reports:

- renderer main-thread time per painted frame;
- frame cadence and over-budget frames;
- long tasks and input latency;
- layout shifts and their sources;
- DOM size, virtualised row count and JavaScript heap usage;
- unexpected browser console errors.

For the CI-oriented verdict set:

```bash
node scripts/perf-audit.mjs --ci
```

CI treats deterministic properties such as bounded DOM usage, zero layout shift and clean consoles
as gates. Timing-derived results are also recorded, but absolute frame timing on shared runners
should be interpreted with the runner specifications and current load in mind.

A CPU profile of the browser workload can be collected with:

```bash
node scripts/perf-audit.mjs --profile
```

### Rendering practices

The main list and playback views rely on the following constraints:

- stable virtualiser callbacks and item keys;
- fixed row geometry for loading and loaded states;
- paged data with a bounded cache and generation-scoped responses;
- narrow store selectors for high-frequency playback updates;
- canvas spectrum updates outside React state;
- reduced visual effects when the measured compositor cannot sustain them.

Changes to these paths should include before-and-after measurements from the same machine and
workload.

## Audio and device testing

The Rust suites include decoder, ring buffer, sample conversion, DSP, resampling, CUE, device
negotiation and supervisor tests. A loopback output seam allows device loss, output replacement,
corrupt tracks and queue recovery to be tested without physical audio hardware.

```bash
cargo test --all-targets
```

These tests validate control flow and sample handling. They do not replace listening tests or
electrical measurements on a real endpoint.

Hardware-dependent checks should record:

- operating system, output backend and device;
- source and negotiated formats;
- buffer size and sample-rate policy;
- active DSP, ReplayGain and volume settings;
- cable, driver and firmware versions where relevant.

Bit-perfect output claims require an exclusive endpoint and a path that satisfies the conditions in
[Architecture](ARCHITECTURE.md#bit-perfect-status). Shared-output results must not be presented as
bit-perfect verification.

## CPU profiling

The `profile_hot_path` example uses `pprof` behind the opt-in `profiling` feature:

```bash
cargo run --example profile_hot_path --features profiling -- --seconds 10 --tracks 8
cargo run --example profile_hot_path --features profiling -- --output-rate 48000
```

The first command profiles decoding and processing without a sample-rate conversion. The second
exercises the resampling path. Generated flamegraphs can be compared with:

- [Decode and DSP flamegraph](media/flamegraph-decode.svg)
- [Resampling flamegraph](media/flamegraph-resample.svg)

For line-level debug information with release-style optimisation, use the profiling Cargo profile:

```bash
cargo run --profile profiling --example profile_hot_path --features profiling
```

## Memory and undefined-behaviour checks

### Valgrind

The CI memory job builds the library test binary and runs the audio unit tests under Valgrind. The
engine supervisor tests are excluded from this job because their wall-clock assertions are not
compatible with the slowdown introduced by instrumentation.

The equivalent command sequence is:

```bash
cargo test --lib --no-run --message-format=json \
  | jq -r 'select(.profile.test == true) | .executable' \
  | grep -v '^$' | tail -1 > /tmp/prism-test-bin

valgrind --tool=memcheck \
  --error-exitcode=1 \
  --leak-check=no \
  --errors-for-leak-kinds=definite \
  --track-origins=yes \
  "$(cat /tmp/prism-test-bin)" --test-threads=1 audio:: --skip audio::engine
```

### Miri

Miri runs weekly and on demand through `.github/workflows/miri.yml`. Its targets are the
pure-logic `audio::ring`, `audio::format` and `lyrics::` modules. Running it outside CI requires a
nightly Rust toolchain with the Miri component.

## Scanner and database analysis

Scanner investigations should distinguish metadata parsing from SQLite writes. Useful measurements
include files processed per second, skipped unchanged files, per-file errors, batch latency and the
number of commits.

For query work, retain the SQL, bound parameters, row count and `EXPLAIN QUERY PLAN` output. Test
both an empty catalogue and a catalogue large enough to exercise pagination and aggregate views.

## Reporting results

A useful performance report includes:

1. commit and build profile;
2. hardware and operating system;
3. exact command and workload;
4. warm-up procedure and number of runs;
5. median and range, rather than a single best result;
6. unexpected background load or environmental limitations.

Generated reports and local profiling output should not replace the reproducible scripts used to
produce them.
