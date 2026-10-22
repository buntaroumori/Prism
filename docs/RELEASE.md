# Release process

Prism releases are produced from annotated version tags. The release workflow checks the source,
builds native packages and publishes the resulting artefacts with checksums and licence
information.

## Versioning

Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). While the major version
is zero, a minor release may include breaking changes to the plugin API, IPC contract or database
schema. Patch releases should remain compatible with existing 0.1.x data.

Database migrations are forward-only. A release must be able to open databases written by supported
earlier releases in the same compatibility line.

## Version carriers

The following files define the release version:

| File                        | Field                               |
| :-------------------------- | :---------------------------------- |
| `package.json`              | `version`                           |
| `Cargo.toml`                | `[workspace.package] version`       |
| `src-tauri/Cargo.toml`      | inherits `version.workspace = true` |
| `src-tauri/tauri.conf.json` | `version`                           |

Check them before tagging:

```bash
node scripts/check-version.mjs
node scripts/check-version.mjs v0.2.0
```

The second command also checks the proposed tag.

## Changelog

`CHANGELOG.md` follows the Keep a Changelog structure. User-visible changes are collected under
`[Unreleased]` and moved into a dated version section during release preparation.

The release workflow uses the matching version section to generate the published notes. Missing or
mismatched version metadata should be resolved before a tag is created.

## Preparing a release

1. Review the changes and update `CHANGELOG.md`.
2. Update every version carrier.
3. Regenerate the licence inventory if dependencies changed:

   ```bash
   pnpm licenses
   ```

4. Run the local quality gate:

   ```bash
   node scripts/check-version.mjs v0.2.0
   pnpm check
   pnpm build
   pnpm schema:validate
   pnpm licenses:check
   python3 scripts/validate_release.py v0.2.0
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf
   cargo test --all-targets
   ```

5. Run the frontend smoke and interaction suites for user-facing changes.
6. Merge the release preparation change into the main branch.

## Tagging

Create an annotated tag from the reviewed commit:

```bash
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

The tag must match the manifest versions. The release workflow is triggered by tags matching `v*`.

## Workflow

`.github/workflows/release.yml` contains three jobs:

| Job       | Responsibility                                                                         |
| :-------- | :------------------------------------------------------------------------------------- |
| `quality` | Check versions, licences, frontend, schema, Rust code and release workflow assumptions |
| `bundle`  | Build native artefacts on Windows and Linux                                            |
| `publish` | Assemble checksums, licence files, release notes and the GitHub Release                |

Bundling depends on the quality job, and publication depends on successful bundles for both
platforms. Release concurrency is scoped by tag and does not cancel an active release.

## Artefacts

The configured package families are:

| Platform     | Artefacts                                             |
| :----------- | :---------------------------------------------------- |
| Windows      | NSIS installer, MSI installer and portable executable |
| Linux        | AppImage and DEB package                              |
| All releases | `SHA256SUMS.txt` and `THIRD-PARTY-LICENSES.md`        |

Build outputs are read from the Cargo workspace `target/release/` directory. Staging checks that
every required artefact exists before publication.

Release builds use default features unless a separate package is explicitly defined. Optional ASIO,
plugin and profiling features are not silently enabled.

## Release validation

`scripts/validate_release.py` performs static checks that do not require publishing a release:

- manifest and tag agreement;
- workflow structure and job dependencies;
- action pinning and referenced script paths;
- package staging and publication coverage;
- checksum and licence inventory generation;
- changelog coverage and licence metadata.

The validator does not replace native builds or platform-specific testing. Both are still required
for a release.

## Verifying published packages

Download the release and verify its checksums:

```bash
gh release download v0.2.0 --repo buntaroumori/Prism --dir /tmp/prism-0.2.0
cd /tmp/prism-0.2.0
sha256sum -c SHA256SUMS.txt
```

Installation and startup should then be checked on each supported platform. Audio checks should
include the actual output backend and source formats affected by the release.

## Failed or defective releases

- If validation or bundling fails before publication, fix the source and move the unpublished tag to
  the corrected commit.
- If a published artefact is defective, mark the release appropriately and publish a corrected
  patch release. Avoid silently replacing packages that may already have been downloaded.
- Incorrect release notes can be edited without replacing the binaries.

## Signing and updates

Packages are currently distributed without a project code-signing signature. Automatic updater
artefacts are not part of the release pipeline. Any future signing or update mechanism must make
signature verification explicit and fail closed when required signing material is unavailable.
