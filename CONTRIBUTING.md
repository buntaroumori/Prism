# Contributing to Prism

Contributions should be scoped, reproducible and consistent with the current architecture. Bug
reports benefit from clear reproduction steps; code changes should include the checks needed to
verify the affected behaviour.

## Development setup

Prism requires Rust 1.88, Node.js 20.19 or newer, pnpm through Corepack and the native Tauri
dependencies for the platform. The full package list for Debian and Ubuntu is in the
[README](README.md#build-from-source).

```bash
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

## Before submitting a change

Run the checks relevant to the change. For a full local validation pass:

```bash
node scripts/check-version.mjs
pnpm check
pnpm build
pnpm schema:validate
pnpm licenses:check
python3 scripts/validate_release.py --skip-licence
cargo fmt --all -- --check
cargo clippy --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf
cargo test --all-targets
```

For interface changes, also run the browser-level checks:

```bash
pnpm exec playwright install chromium
node scripts/smoke-ui.mjs
node scripts/ui-behaviour.mjs
```

Performance-sensitive changes should include measurements collected according to
[docs/PERFORMANCE.md](docs/PERFORMANCE.md).

## Testing expectations

- **Database:** extend `scripts/validate_schema.py` or the Rust database and library tests when
  constraints, migrations or read models change.
- **IPC:** update the Rust payload and its Zod schema together. Include invalid-input and error
  cases where appropriate.
- **Audio and DSP:** add deterministic unit tests for calculations and integration tests for queue,
  format or device behaviour.
- **Scanner:** use real fixture files and verify idempotency, failure isolation and cancellation.
- **Interface:** exercise the production bundle for navigation, persistence and pointer or keyboard
  behaviour affected by the change.
- **Documentation:** update the relevant English-language document when commands, configuration or
  observable behaviour change.

## Implementation conventions

### Rust

- Use edition 2024 and `cargo fmt`.
- Keep production error handling explicit with the existing error types.
- Do not perform filesystem, SQL, HTTP or logging work in the audio callback.
- Preserve the playback supervisor's ownership of device and queue state.
- Document the reason for any new unsafe boundary and its safety requirements.

### TypeScript and CSS

- Keep strict TypeScript checks enabled, including indexed-access and optional-property checks.
- Validate IPC payloads before writing them to stores.
- Avoid unhandled promises and broad store subscriptions in frequently updated components.
- Reuse the existing design tokens and Lucide icons.
- Preserve reduced-motion behaviour and stable layout dimensions.

### Dependencies

New dependencies must be necessary for the change and compatible with the repository licence
policy. After changing either dependency graph, regenerate and commit the inventory:

```bash
pnpm licenses
pnpm licenses:check
```

`docs/LICENSES.md` is generated and should not be edited manually.

## Commits and pull requests

Conventional Commit prefixes are preferred:

```text
feat(library): add genre filtering
fix(audio): preserve queued audio during output recovery
perf(scanner): reduce unchanged-file work
docs(release): clarify package verification
```

Common scopes include `audio`, `dsp`, `library`, `db`, `ui`, `ipc`, `discord`, `plugins`, `docs`,
`ci` and `deps`.

A pull request should contain:

- a concise description of the behaviour being changed;
- the reason for the change and any relevant issue link;
- screenshots or measurements for visible or performance-sensitive changes;
- the exact validation commands run and their results;
- any platform-specific behaviour that remains unverified.

Keep unrelated refactoring, dependency updates and documentation rewrites in separate changes where
practical.

## Continuous integration

Pull requests run frontend, schema, licence, Rust, memory and performance jobs. Rust formatting,
Clippy and tests run on Linux and Windows. Miri runs separately on a weekly schedule and on demand.
Release workflows are described in [docs/RELEASE.md](docs/RELEASE.md).
