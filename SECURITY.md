# Security policy

## Supported versions

| Version          | Status      |
| :--------------- | :---------- |
| 0.1.x            | Supported   |
| Earlier versions | Unsupported |

Security fixes are applied to the current development line and included in a subsequent release.

## Reporting a vulnerability

Use a [private GitHub security advisory](https://github.com/buntaroumori/Prism/security/advisories/new)
for suspected vulnerabilities. Do not include exploitable details in a public issue.

A useful report contains:

- the affected version and operating system;
- the relevant feature or input format;
- reproduction steps and expected security impact;
- a minimal proof of concept where appropriate;
- any known mitigations.

Disclosure timing should be coordinated according to severity, exploitability and the availability
of a fix. Reporter credit can be included in release notes when requested.

## Security boundaries

Prism processes local media and metadata that may be malformed or untrusted. The principal assets
are local files, application state, playback availability and the integrity of the desktop process.

### Desktop and IPC

- The WebView uses a restrictive content security policy and a minimal Tauri capability set.
- Asset protocol access is limited to application data and cache directories.
- IPC commands expose typed inputs and results; frontend payloads are validated with Zod.
- Browser navigation, development shortcuts and unrelated browser menus are disabled in the
  desktop interface.

### Catalogue and parsers

- SQL values are bound, and sort or filter fragments are selected from fixed definitions.
- Metadata parser failures are isolated per file and recorded without aborting the entire scan.
- Filesystem identity checks and path handling are centralised in the catalogue and scanner.
- Online artwork and metadata providers use HTTPS and do not require application credentials.

### Plugins

Plugin runtimes are optional and disabled in default builds. Enabled plugins run with:

- capabilities explicitly declared in their manifests;
- no direct filesystem, process or network API from the host;
- memory and execution budgets;
- separate runtime instances and session-scoped failure handling.

A plugin should still be treated as code installed by the local user. Capability declarations limit
the host surface; they are not a substitute for reviewing an untrusted plugin.

## Release verification

Release packages are accompanied by `SHA256SUMS.txt` and a generated third-party licence inventory.
Downloaded artefacts should be obtained from the project repository and checked against the
published checksums before installation.

Packages are not currently accompanied by a project code-signing signature. Reports about release
workflow integrity, dependency substitution or checksum publication are in scope.

## Third-party components

Security issues in Rust, npm, WebView2, WebKitGTK or operating-system components may also need to be
reported upstream. Dependency updates and mitigations are evaluated through the normal repository
review process.
