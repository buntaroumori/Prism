# Security policy

## Supported versions

| Version         | Supported |
| :-------------- | :-------- |
| 0.1.x (current) | Yes       |
| < 0.1           | No        |

## Reporting a vulnerability

Email **security@prism-audio.dev** with a description, affected version, reproduction steps and any
proof-of-concept. Do not open a public issue before a fix is available.

- Acknowledgement within 72 hours.
- Coordinated disclosure: 90 days or a released fix, whichever comes first.
- Reporters are credited in the release notes unless they ask to remain anonymous.

## Threat model

Prism plays untrusted local media and talks to a small set of public metadata APIs. The relevant
assets are the user's files, credentials-free local state, and the integrity of the audio path.

### Controls in place

- **WebView isolation**: strict CSP, no remote code, `freezePrototype`, asset protocol restricted to
  `$APPDATA` and `$APPCACHE`, capabilities scoped to one window.
- **IPC**: every command validates its arguments; responses are schema-checked before rendering.
- **Rust memory safety**: `unsafe` is denied outside explicitly documented audio backend boundaries;
  those blocks carry `// SAFETY:` justifications and are fuzzed.
- **Parsers**: symphonia, lofty and the CUE parser are fuzzed with `cargo-fuzz` before release;
  malformed files fail closed with a `scan_errors` row.
- **Network**: TLS only (rustls), timeouts, no credentials, responses size-capped.
- **Plugins** (FASE 5): capability-based permissions, instruction budgets, no raw filesystem or
  device handles, wasmtime sandbox for untrusted UI extensions.

### Out of scope

- Denial of service through enormous local libraries on underpowered hardware.
- Unsigned builds downloaded from third parties (verify `SHA256SUMS.txt` from the official release).
- Vulnerabilities in WebView2/WebKitGTK themselves — report upstream, but tell us so we can pin.

## Hardening roadmap

- Code signing for Windows installers.
- Reproducible builds with `cargo auditable` metadata.
- minisign signatures for release checksums.
- `cargo deny` in CI with an explicit licence allowlist.
