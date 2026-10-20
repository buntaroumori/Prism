# FASE 7 — Open-source release hardening

**Status:** delivered, awaiting approval
**Scope:** the repository as a public artefact — MIT licensing with a real audit behind it, a
defensible `.gitignore`, a README written for someone who has never heard of this project, a
changelog with a structure instead of a scroll, and a release pipeline that is validated before a
tag is ever pushed.

The phase has one rule: **a public repository is a promise**. A licence claim, a changelog entry and
a release artefact are all statements someone will rely on, so each one here is generated, checked
or executable rather than asserted.

---

## 1. What was delivered

| Deliverable      | Artefact                                         | How it is kept honest                                                                                      |
| :--------------- | :----------------------------------------------- | :--------------------------------------------------------------------------------------------------------- |
| Licence          | `LICENSE` (MIT, 2026)                            | `scripts/validate_release.py` checks the text, the copyright line and that all three manifests declare MIT |
| Licence audit    | `scripts/license-audit.mjs` → `docs/LICENSES.md` | 671 packages classified from the lockfiles; CI fails on a refusal or a stale document                      |
| Ignore policy    | `.gitignore`                                     | secrets, runtime state and build output are ignored; `Cargo.lock` and `pnpm-lock.yaml` are not             |
| README           | `README.md`                                      | every number in it is a measurement from `docs/PERFORMANCE.md` or `docs/PHASE-6.md`                        |
| Changelog        | `CHANGELOG.md`                                   | Keep a Changelog sections; release notes are generated from it                                             |
| Release pipeline | `.github/workflows/release.yml`                  | 30 static checks, plus a documented negative test                                                          |
| Release handbook | `docs/RELEASE.md`                                | the procedure a maintainer follows, including rollback                                                     |

---

## 2. The licence audit

### 2.1 Why a script and not a list

`docs/LICENSES.md` could have been written by hand once and forgotten. It would then have been wrong
the first time a dependency changed — and wrong in the direction that matters, because the file
exists to support a legal claim. It is generated instead, from the two lockfiles, and CI fails when
it drifts:

```bash
pnpm licenses        # regenerate docs/LICENSES.md
pnpm licenses:check  # CI: fail if the inventory or the policy moved
```

### 2.2 Three sets, because "dependency" is three things

| Set            | Definition                                                                                                | Enforced |
| :------------- | :-------------------------------------------------------------------------------------------------------- | :------- |
| **Shipped**    | reachable from the crate over normal edges — what ends up inside `prism.exe` and the AppImage             | yes      |
| **Build-only** | reachable only over `[build-dependencies]` (`bindgen`, `cc`, `pkg-config`, …) — runs on the build machine | reported |
| **npm**        | `pnpm licenses list --json --prod` — what Vite bundles into the WebView                                   | yes      |

Computing the closure instead of listing `Cargo.toml` entries matters: the real graph is **691
packages**, of which 651 are linked into the binary and 39 are build-only. Reading the manifest would
have seen eleven dependencies and called it an audit.

### 2.3 Policy

`OR` picks the least demanding branch — that is what dual licensing means: we choose. `AND` requires
every branch to clear the policy. The legacy `MIT/Apache-2.0` form is normalised before parsing,
because crates.io metadata is not consistent about it.

| Status                 | Licences                                                                                                                                      |
| :--------------------- | :-------------------------------------------------------------------------------------------------------------------------------------------- |
| Permissive             | MIT, MIT-0, 0BSD, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, BSL-1.0, Unlicense, CC0-1.0, Unicode-3.0 |
| Permissive with notice | MPL-2.0, OFL-1.1, CDLA-Permissive-2.0                                                                                                         |
| Refused                | GPL, AGPL, SSPL, EPL, CDDL, EUPL, CPAL, Commons Clause                                                                                        |

Unknown or unparseable licences fail the audit. There is an escape hatch — `MANUAL_APPROVALS` — but
it is empty, and an entry there needs a reason a reviewer can check.

### 2.4 Result

| Metric                          |                       Value |
| :------------------------------ | --------------------------: |
| Shipped packages                | **671** (651 Rust + 20 npm) |
| Permissive                      |                     **652** |
| With a notice obligation        |                      **19** |
| Requiring a decision            |                       **0** |
| GPL/AGPL/SSPL/CDDL in the graph |                    **none** |
| Build-only packages             |                          39 |

The 19 notice-required packages are 17 MPL-2.0 crates (Symphonia's decoders plus `cssparser` /
`selectors` via Tauri), the Inter typeface under OFL-1.1, and one CDLA-Permissive-2.0 data crate.
`docs/LICENSES.md` states the obligation each one carries and what Prism does about it — including
the one thing that decides MPL compliance, which is that **no MPL-2.0 file is patched**; if one ever
is, that file alone has to be published under MPL-2.0.

### 2.5 What is deliberately out of scope

`pprof` and `inferno` (the `profiling` feature) do not appear in the inventory, and that is not an
oversight: the audit resolves the **default** feature set, which is what `tauri build` compiles for a
release. `inferno` is CDDL-1.0 — a licence this project refuses — and it is acceptable here
precisely because it can never enter an artefact. The same logic covers the `asio` feature, which
needs the Steinberg SDK and is never enabled in CI.

---

## 3. README

The README now answers, in order, the questions a stranger asks: what it is, what works today, how it
looks, how it is built, how fast it is, how to install it, how the code is laid out, and under what
licence.

Changes worth naming:

- **The phase table is status, not aspiration.** Phase 7 moved to Complete; the table carries no
  emoji, matching the rule that the repository uses no emoji anywhere.
- **A measured performance section.** 2.8–3.4 ms per frame on a 500 000-row list, 656× real-time
  decoding, 0 valgrind errors — each traced to a command the reader can run.
- **A licence section that states the audit.** Not "we use MIT-compatible dependencies" but 671
  packages, 652 permissive, 19 with notices, none copyleft, and the command that regenerates the
  claim.
- **A repository layout that matches the tree**, including `docs/RELEASE.md`, `docs/LICENSES.md`,
  `src/perf/` and the four audit scripts.
- **A CI/CD section that describes the pipeline that exists**, including the licence and validation
  jobs, instead of the pipeline that was planned in FASE 1.

---

## 4. Changelog and release notes

`CHANGELOG.md` follows Keep a Changelog properly now: an `[Unreleased]` section, one `## [0.1.0]`
section, and the canonical categories. The phase material is preserved as `####` subsections under
`### Added`, so nothing was lost in the move — 61 bullets went in, 64 came out (three fixed-defect
entries were promoted out of the FASE 6 block where they had been hiding as features).

The release notes are **not** a commit dump. `scripts/generate-changelog.mjs` reads the `## [x.y.z]`
section out of the changelog, demotes its headings one level for the release page, and appends the
Conventional Commit list inside a collapsed `<details>` block. If a version has no changelog section,
the notes say so and fall back to commits — a release never publishes an empty body.

Verified locally:

```bash
$ GITHUB_REF_NAME=v0.1.0 node scripts/generate-changelog.mjs | head -6
# Prism v0.1.0

First public release: a bit-perfect audio engine, a catalogue that survives 500 000 tracks,
and a keyboard-first interface measured against a 120 Hz frame budget.

## Added
```

---

## 5. Validating a release pipeline without releasing

A release workflow cannot run on a laptop: it needs two operating systems, a tag that does not exist
yet, and code-signing material. What can be checked is every assumption it makes, and those are what
actually break a release.

`scripts/validate_release.py` performs **30 static checks** in five groups:

| Group          | Checks                                                                                                                                                                                                                                  |
| :------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Versions       | the three carriers agree; the tag matches `package.json`                                                                                                                                                                                |
| Workflow shape | YAML parses; `push.tags: v*`; `contents: write`; jobs chain `quality → bundle → publish`; both platforms in the matrix; concurrency never cancels                                                                                       |
| Hygiene        | every `uses:` pinned to a version tag or commit (one documented exception: `dtolnay/rust-toolchain@master`, where the branch _is_ the selector); every `scripts/…` a step calls exists; no `secrets.*` in `if:`; no `continue-on-error` |
| Artefacts      | every `bundle.targets` entry has a staging step; every staged file is matched by a published glob; checksums and the licence inventory are published; notes come from the changelog                                                     |
| Licensing      | `LICENSE` is MIT with a copyright line; `package.json`, `Cargo.toml` and the bundle metadata say MIT; the audit passes and the document is current                                                                                      |

Result:

```
[ok  ] version carriers agree              package.json=0.1.0, Cargo.toml=0.1.0, tauri.conf.json=0.1.0
[ok  ] jobs present                        quality, bundle, publish
[ok  ] both operating systems bundled      platforms = ['linux', 'windows']
[ok  ] stages deb                          copies from bundle/deb
[ok  ] publishes every staged artefact     ok
[ok  ] licence audit passes                license-audit: 671 shipped packages — 652 permissive, 19 with notice
...
30/30 checks passed
```

### The negative test

A validator that has never failed is a validator nobody has tested. Removing the `.deb` staging step
from `release.yml` — a mistake that would silently publish a Linux release without a Debian package —
is caught immediately:

```
[FAIL] stages deb                        copies from bundle/deb
28/29 checks passed
```

Restoring the line returns `29/29`. CI runs the validator on every pull request (`licenses` job) and
again inside the release `quality` job.

### What cannot be validated locally, and how it is covered

| Cannot be checked here                      | Covered by                                                                |
| :------------------------------------------ | :------------------------------------------------------------------------ |
| MSI/NSIS production, signing, Windows paths | the `bundle` job on `windows-latest`, gated on `quality`                  |
| WebKitGTK availability on 24.04             | `bundle` on `ubuntu-24.04`, with the same dependency list CI installs     |
| Updater signature                           | not implemented, and `docs/RELEASE.md` says why: no unsigned auto-updates |

The release also gained a compliance step: `docs/LICENSES.md` is copied to
`THIRD-PARTY-LICENSES.md`, checksummed with everything else, and published next to the binaries.

---

## 6. `.gitignore`

Tightened into a policy with a stated rule: **anything regenerable is ignored; anything required for a
reproducible build is committed.** Five groups:

| Group             | Examples                                                    | Why                                               |
| :---------------- | :---------------------------------------------------------- | :------------------------------------------------ |
| Rust build output | `target/`, `*.profraw`, `perf.data`, `tracy-*.trace`        | regenerable, and large                            |
| Node build output | `node_modules/`, `dist/`, `.perf-dist/`, `perf-report.json` | the audit harness is not a shipped artefact       |
| Tauri             | `src-tauri/gen/schemas/`, `*.msi`, `*.AppImage`, `*.deb`    | bundler output                                    |
| Runtime state     | `*.db`, `*.db-wal`, `logs/`, `data/`                        | a user's library must never reach a commit        |
| Secrets           | `.env*`, `*.pem`, `*.key`, signing material                 | the one category where a mistake is unrecoverable |

`Cargo.lock`, `pnpm-lock.yaml` and the icons stay tracked: a release build that resolves different
crates than CI tested is not reproducible. A test media exception (`!assets/demo/*.flac`) keeps the
fixtures that the CUE and decoder tests need.

---

## 7. Verification

| Gate             | Command                                                      | Result                                     |
| :--------------- | :----------------------------------------------------------- | :----------------------------------------- |
| Versions         | `node scripts/check-version.mjs v0.1.0`                      | consistent across three carriers           |
| Licence policy   | `node scripts/license-audit.mjs --check`                     | 671 packages, 0 refusals, document current |
| Release pipeline | `python3 scripts/validate_release.py`                        | **30/30**                                  |
| Release notes    | `GITHUB_REF_NAME=v0.1.0 node scripts/generate-changelog.mjs` | curated section + 10 commits               |
| Frontend         | `pnpm check`                                                 | typecheck, ESLint and Prettier clean       |
| Bundle           | `pnpm build`                                                 | production build succeeds                  |
| Workflows        | PyYAML parse of `ci.yml` and `release.yml`                   | both valid                                 |

---

## 8. What post-1.0 inherits

- **Signing is the next release blocker**, not this one: Authenticode for Windows and a minisign key
  for Tauri's updater. The pipeline is written to fail closed when the key arrives rather than to
  publish unsigned artefacts silently.
- **The audit is the gate for every new dependency.** Adding a crate now means running
  `pnpm licenses` and committing the regenerated inventory, which is a reviewable diff.
- **`docs/RELEASE.md` is the procedure.** If the pipeline and the document disagree, the validator
  fails and one of them is wrong — which is the point.

---

## 10. Post-delivery round — 0.1.1

The first public build installed, launched and showed **a black window** on Windows and Linux. That
is a defect of the delivery, not of the feature set: every gate was green because no gate reproduced
the runtime the WebView provides. This section is the account of what was wrong, how it was found,
and what now prevents it.

### 10.1 Root cause: `freezePrototype` versus a module-scope assignment

`tauri.conf.json` shipped `"freezePrototype": true`. Tauri executes
`Object.freeze(Object.prototype)` in every frame **before** the page's scripts run. The validation
layer assigns a shadowing property on a plain object at module scope:

```js
errorUtil.errToObj = (message) => …;
errorUtil.toString = (message) => …;   // ← TypeError once Object.prototype is frozen
```

In a strict-mode ES module that assignment throws, evaluation of the entry module aborts, React never
mounts, and the shell shows an empty window with a healthy process. Nothing is logged where a user
would look, which is why the symptom is "black window" rather than an error.

It is platform-independent, which is why both Windows and Linux were affected.

**Evidence.** Reproduced in the real artefact: a `--features tauri/custom-protocol` build with the
frontend embedded, run under Xvfb. Then isolated A/B on the _same_ `dist/`, with and without the
freeze:

| Runtime                                        | Mounted                                                   |
| :--------------------------------------------- | :-------------------------------------------------------- |
| `Object.freeze(Object.prototype)` (as shipped) | **no** — `Cannot assign to read only property 'toString'` |
| without the freeze                             | **yes**                                                   |

**Fix.** `freezePrototype: false`. The option is a hardening measure against prototype pollution in
the page; the CSP (`script-src 'self'`, no inline, no `eval`), the asset allowlist, the minimal
capability set and the plugin capability bridge are unchanged, and the application still freezes its
own objects where it matters.

### 10.2 A read model that broke the Home view on every fresh install

With the window rendering, the next failure was immediate and reproducible: `Request failed ·
database — no such column: added_at`. `library_albums({ sort: "added" })` orders
`v_album_summary` by `added_at`, and that view never exposed the column.

**Fix.** Migration `0002` recreates the view with `MAX(t.added_at)` — an album counts as added when
its newest track arrived — and adds a covering index for the sort. `scripts/validate_schema.py` now
asserts both the column and the ordering, so the harness fails if the view regresses.

### 10.3 The release pipeline staged from the wrong directory

`src-tauri/target/release` was assumed to hold the bundles. This is a Cargo **workspace**, so
artefacts land in `./target/release`; the `src-tauri/target` path exists but stays empty. The result
was a published release with no installers, and `fail_on_unmatched_files` turning that into a red
publish job.

**Fix.** Staging reads the workspace target, `find`s each artefact, and exits with a GitHub error
annotation plus a directory listing when one is missing. `scripts/validate_release.py` gained two
checks (staging path, and that every staging step fails loudly), which is what makes the mistake
unrepeatable rather than merely fixed.

### 10.4 Two gates that made CI red for reasons unrelated to the code

| Job              | Why it failed                                                                                                                                                                                                                                                                                    | What changed                                                                                                                                                                                      |
| :--------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `qa` (Miri)      | Interpreting a crate that links WebKitGTK, tokio and rustls does not fit a CI timeout; the job died on the clock, every time.                                                                                                                                                                    | Miri moved to `miri.yml`, weekly and on demand. Valgrind stays in `qa` as the pull-request memory gate.                                                                                           |
| `performance`    | The frame _rate_ on a shared runner is noise: a rented vCPU and a software compositor produced intermittent verdicts.                                                                                                                                                                            | `--ci` enforces the deterministic properties (bounded DOM, CLS 0, input off the paint path, clean console) and reports the timing ones as advisory. `pnpm perf` locally still enforces all seven. |
| `rust` (Windows) | `stress::a_cue_image_over_one_gigabyte_expands_without_being_read` asserted that the 1.5 GB fixture occupied under 4 MB **on disk** — true of a sparse file on ext4, false on NTFS, where `set_len` allocates the range and `metadata().len()` reports it. A test that could only pass on Linux. | The sparse-file assertion is `#[cfg(unix)]`; cross-platform CUE coverage lives in `tests/library.rs`, and the reason is written next to the attribute.                                            |
| `qa`             | Valgrind ran the integration stress suite, whose supervisor tests assert on wall-clock deadlines. At 20× slowdown a correct implementation misses them.                                                                                                                                          | Valgrind runs the **audio kernel** (`audio::` lib tests, `--skip audio::engine`): the ring, the conversions, the decoder, the CUE spans — the code that actually owns raw memory.                 |

### 10.5 The gate that would have caught it

`scripts/smoke-ui.mjs` (`pnpm smoke`) loads the **built** bundle in a Tauri-shaped runtime:

- the production CSP, with inline-script hashes injected the way `tauri-codegen` injects them;
- `freezePrototype`, read from `tauri.conf.json` and applied as the shell applies it;
- a stubbed `__TAURI_INTERNALS__` that fails every command, which is exactly a first launch;
- the window sized as the configuration says.

It asserts that React committed a tree, that the boot watchdog did not fire, and that no unexpected
error was reported. Verified in both directions — with the option restored to `true` it reproduces
the black window and names the error; with the fix it passes.

The `frontend` job runs it on every pull request, after building the same `dist/` the release ships.

### 10.6 A failure that is no longer silent

`index.html` now carries a small inline watchdog: if the root has no children a few seconds after
load, it renders a card with the first error, its file and line, and a reload button. It runs before
any module, so it survives a bundle that throws during evaluation, and its hash is added to the CSP
by Tauri — the policy stays strict. A startup failure is now a readable message instead of a black
rectangle.

### 10.7 Verification of this round

| Gate                                      | Result                                                       |
| :---------------------------------------- | :----------------------------------------------------------- |
| Native binary, embedded assets, Xvfb      | **renders the shell** (screenshot captured before and after) |
| `pnpm smoke`                              | rendered, watchdog quiet, no unexpected errors               |
| `pnpm smoke` with `freezePrototype: true` | **fails** with the exact `TypeError`                         |
| `cargo test --all-targets`                | 197 + 39 passed, 1 ignored                                   |
| `cargo clippy --all-targets`              | exit 0                                                       |
| `cargo fmt --check`                       | clean                                                        |
| `pnpm check`                              | typecheck, ESLint, Prettier clean                            |
| `python3 scripts/validate_schema.py`      | schema OK, including the new `added_at` assertions           |
| `python3 scripts/validate_release.py`     | 33/33                                                        |
| `node scripts/license-audit.mjs --check`  | 671 packages, document current                               |
