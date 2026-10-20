# Contributing to Prism

Thank you for helping build a player that respects both the listener and the audio.

## Ground rules

1. **No placeholder code.** No empty functions, mocked data paths, `// TODO` implementations or
   "coming soon" buttons. If a feature is not ready, it is not merged.
2. **Every error is handled.** Rust: `thiserror`/`anyhow`, no `unwrap()` in production paths.
   TypeScript: no floating promises, no unvalidated IPC payloads.
3. **Measure performance.** Changes touching audio callbacks, the scanner, the virtual list or the
   ambient canvas need before/after numbers (see `docs/PERFORMANCE.md`).
4. **Respect the layering.** The WebView never reads files or opens devices; Rust never renders.

## Development setup

```bash
corepack enable
pnpm install
pnpm tauri dev
```

Linux dependencies are listed in the README. Rust 1.88 is pinned by `rust-toolchain.toml`.

## Quality gate (what CI runs)

```bash
node scripts/check-version.mjs
pnpm check                 # tsc --noEmit, ESLint strictTypeChecked, Prettier
pnpm build
pnpm schema:validate       # real SQLite execution of migrations
pnpm licenses:check        # licence policy + docs/LICENSES.md freshness
python3 scripts/validate_release.py --skip-licence
cargo fmt --all -- --check
cargo clippy --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf
cargo test --all-targets
```

A pull request is mergeable only when all of the above pass on Linux **and** Windows. CI adds three
more gates that are slow but non-negotiable: the `qa` job (Miri on the pure-logic modules, valgrind
over the stress suite), the `performance` job (the 500 000-row frame budget) and the licence audit.

## Dependencies

A new crate or npm package is a licensing decision as much as a technical one:

1. it must clear the policy in `scripts/license-audit.mjs` (MIT/BSD/Apache-2.0/ISC/Zlib/Unicode/
   MPL-2.0 and friends — never GPL, AGPL, SSPL, EPL, CDDL or the Commons Clause);
2. `pnpm licenses` must be re-run and `docs/LICENSES.md` committed with the change;
3. a licence with an obligation (MPL-2.0, OFL, CDLA) needs a sentence in the "Notices we must
   honour" section of that document, saying what we do to honour it.

Speculative dependencies are refused in review: a crate added "for later" is a maintenance and
supply-chain cost with no feature to show for it.

## Commit convention

Conventional Commits, enforced socially and used by the release changelog:

```text
feat(library): add FTS5 phrase search
fix(audio): keep WASAPI exclusive stream alive across sample-rate changes
perf(scanner): batch 512 upserts per transaction
docs(readme): document Linux baseline
```

Scopes in use: `audio`, `dsp`, `library`, `db`, `ui`, `ipc`, `discord`, `plugins`, `ci`, `deps`.

## Testing expectations

- New SQL behaviour → extend `scripts/validate_schema.py` and/or `src-tauri/tests/database.rs`.
- New IPC command → add a Zod schema, use it in the client, and cover the failure codes.
- DSP math → unit tests with golden vectors; no golden files larger than 1 MB.
- UI → keyboard navigation, focus visibility, reduced-motion parity.

## Code style

- Rust: edition 2024, `cargo fmt`, 100-column width, `// SAFETY:` on every `unsafe` block.
- TypeScript: strict mode with `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`; prefer
  pure functions in `src/lib`; components take primitives, not stores.
- CSS: Tailwind v4 tokens only; no inline hex colours outside `globals.css`.
- Icons: Lucide SVGs only — no emoji anywhere in the interface or docs.

## Pull requests

Include: intent, screenshots or traces where relevant, and the exact commands you ran. Keep changes
scoped; large architectural shifts should start as an issue with a design note in `docs/`.
