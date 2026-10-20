# Third-party licences

Prism is released under the MIT licence. This document is the inventory of everything that
licence has to coexist with. It is generated from `Cargo.lock` and `pnpm-lock.yaml` by
`node scripts/license-audit.mjs`.

> Generated file — do not edit. CI fails if it drifts from the lockfiles.

## Summary

| Set | Packages | Enforced |
| :-- | -------: | :------- |
| Rust, linked into the binary | 651 | by this audit |
| Rust, build-only | 39 | runs on the build machine, never linked |
| npm, bundled into the frontend | 20 | by this audit |

Of the 671 shipped packages, **652** are permissive, **19** carry a notice obligation and **0** require a decision. No GPL, AGPL, SSPL or CDDL package appears anywhere in the graph.

### Licences in use

| Licence | Packages | Status |
| :------ | -------: | :----- |
| MIT | 500 | Permissive |
| Apache-2.0 | 76 | Permissive |
| Zlib | 20 | Permissive |
| Unicode-3.0 | 18 | Permissive |
| MPL-2.0 | 17 | Notice required |
| Unlicense | 12 | Permissive |
| BSD-3-Clause | 8 | Permissive |
| Apache-2.0 WITH LLVM-exception | 5 | Permissive |
| ISC | 4 | Permissive |
| BSD-2-Clause | 3 | Permissive |
| 0BSD | 2 | Permissive |
| Apache-2.0 AND ISC | 1 | Permissive |
| Apache-2.0 AND MIT | 1 | Permissive |
| BSD-3-Clause AND MIT | 1 | Permissive |
| CC0-1.0 | 1 | Permissive |
| CDLA-Permissive-2.0 | 1 | Notice required |
| OFL-1.1 | 1 | Notice required |

## Policy

Permissive (MIT, MIT-0, 0BSD, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause,
BSD-3-Clause, ISC, Zlib, BSL-1.0, Unlicense, CC0-1.0, Unicode-3.0) is accepted outright: it
imposes nothing beyond attribution, which this document provides.

Notice-required licences are accepted with an explicit obligation, listed below. GPL, AGPL,
SSPL, EPL, CDDL, EUPL, CPAL and the Commons Clause are refused, because they would constrain
how Prism itself is licensed.

Where a crate offers a choice — `MIT OR Apache-2.0 OR LGPL-2.1-or-later`, for instance — the
audit picks the least demanding branch, which is the entire point of dual licensing. Only the
default feature set is counted: `profiling` (pprof, inferno) is a maintainer tool that never
enters a release artefact, so it is not part of this inventory.

## Notices we must honour

### MPL-2.0 — 17 packages, all unmodified

This is the file-level copyleft that covers the audio decoders and two of Tauri's CSS
crates. Shipping them inside an MIT application is explicitly permitted as long as the
MPL text travels with the distribution and any modified file is published under MPL-2.0.
Prism does not patch any of them: they are consumed from crates.io exactly as pinned in
`Cargo.lock`.

### Apache-2.0 — 78 packages

Apache-2.0 requires the licence text, and any `NOTICE` file a crate ships, to be retained
in distribution. Release artefacts publish this document next to the binaries for exactly
that reason; the full texts remain available in the corresponding crates.io entries and
npm packages.

### OFL-1.1 — the interface typeface

Inter (`@fontsource-variable/inter@5.3.0`) is embedded under
the SIL Open Font Licence: it may be bundled and redistributed, it may not be sold on its
own, and its licence text ships inside the npm package.

### Linux runtime libraries

On Linux the application links dynamically against WebKitGTK and GTK, which are
LGPL-2.1-or-later and are supplied by the operating system. Dynamic linking against them
does not extend the LGPL to Prism's own code, and the `.deb` declares them as dependencies
instead of bundling private copies.

## Inventory — Rust, linked into the binary

| Package | Version | Licence (chosen) | Status |
| :------ | :------ | :--------------- | :----- |
| `adler2` | 2.0.1 | 0BSD | Permissive |
| `aho-corasick` | 1.1.5 | Unlicense | Permissive |
| `alloc-no-stdlib` | 2.0.4 | BSD-3-Clause | Permissive |
| `alloc-stdlib` | 0.2.4 | BSD-3-Clause | Permissive |
| `allocator-api2` | 0.2.21 | MIT | Permissive |
| `alsa` | 0.9.1 | Apache-2.0 | Permissive |
| `alsa-sys` | 0.3.1 | MIT | Permissive |
| `android_system_properties` | 0.1.6 | MIT | Permissive |
| `anyhow` | 1.0.104 | MIT | Permissive |
| `approx` | 0.5.1 | Apache-2.0 | Permissive |
| `arrayvec` | 0.7.8 | MIT | Permissive |
| `async-broadcast` | 0.7.2 | MIT | Permissive |
| `async-channel` | 2.5.0 | Apache-2.0 | Permissive |
| `async-compression` | 0.4.51 | MIT | Permissive |
| `async-executor` | 1.14.0 | Apache-2.0 | Permissive |
| `async-io` | 2.6.0 | Apache-2.0 | Permissive |
| `async-lock` | 3.4.2 | Apache-2.0 | Permissive |
| `async-process` | 2.5.0 | Apache-2.0 | Permissive |
| `async-recursion` | 1.2.0 | MIT | Permissive |
| `async-signal` | 0.2.14 | Apache-2.0 | Permissive |
| `async-task` | 4.7.1 | Apache-2.0 | Permissive |
| `async-trait` | 0.1.92 | MIT | Permissive |
| `atk` | 0.18.2 | MIT | Permissive |
| `atk-sys` | 0.18.2 | MIT | Permissive |
| `atoi` | 2.0.0 | MIT | Permissive |
| `atomic-waker` | 1.1.2 | Apache-2.0 | Permissive |
| `base64` | 0.23.1 | MIT | Permissive |
| `base64` | 0.22.1 | MIT | Permissive |
| `base64` | 0.21.7 | MIT | Permissive |
| `base64ct` | 1.8.3 | Apache-2.0 | Permissive |
| `bit-set` | 0.8.0 | Apache-2.0 | Permissive |
| `bit-vec` | 0.8.0 | Apache-2.0 | Permissive |
| `bitflags` | 2.13.2 | MIT | Permissive |
| `bitflags` | 1.3.2 | MIT | Permissive |
| `block-buffer` | 0.10.4 | MIT | Permissive |
| `block2` | 0.6.2 | MIT | Permissive |
| `blocking` | 1.7.0 | Apache-2.0 | Permissive |
| `brotli` | 8.0.4 | BSD-3-Clause AND MIT | Permissive |
| `brotli-decompressor` | 5.0.3 | BSD-3-Clause | Permissive |
| `bs58` | 0.5.1 | MIT | Permissive |
| `bumpalo` | 3.20.3 | MIT | Permissive |
| `by_address` | 1.2.1 | MIT | Permissive |
| `bytemuck` | 1.25.2 | Zlib | Permissive |
| `byteorder` | 1.5.0 | Unlicense | Permissive |
| `byteorder-lite` | 0.1.0 | Unlicense | Permissive |
| `bytes` | 1.12.1 | MIT | Permissive |
| `cairo-rs` | 0.18.5 | MIT | Permissive |
| `cairo-sys-rs` | 0.18.2 | MIT | Permissive |
| `camino` | 1.2.6 | MIT | Permissive |
| `cargo_metadata` | 0.19.2 | MIT | Permissive |
| `cargo-platform` | 0.1.9 | MIT | Permissive |
| `cesu8` | 1.1.0 | Apache-2.0 | Permissive |
| `cfb` | 0.7.3 | MIT | Permissive |
| `cfg-if` | 1.0.5 | MIT | Permissive |
| `chacha20` | 0.10.2 | MIT | Permissive |
| `chrono` | 0.4.45 | MIT | Permissive |
| `color-thief` | 0.2.2 | MIT | Permissive |
| `combine` | 4.6.9 | MIT | Permissive |
| `compression-codecs` | 0.4.46 | MIT | Permissive |
| `compression-core` | 0.4.34 | MIT | Permissive |
| `concurrent-queue` | 2.5.0 | Apache-2.0 | Permissive |
| `const-oid` | 0.9.6 | Apache-2.0 | Permissive |
| `convert_case` | 0.6.0 | MIT | Permissive |
| `cookie` | 0.18.2 | MIT | Permissive |
| `cookie-factory` | 0.3.3 | MIT | Permissive |
| `core_detect` | 1.0.0 | MIT | Permissive |
| `core-foundation` | 0.10.1 | MIT | Permissive |
| `core-foundation-sys` | 0.8.7 | MIT | Permissive |
| `core-graphics` | 0.25.0 | MIT | Permissive |
| `core-graphics-types` | 0.2.0 | MIT | Permissive |
| `coreaudio-rs` | 0.11.3 | MIT | Permissive |
| `coreaudio-sys` | 0.2.18 | MIT | Permissive |
| `cpal` | 0.15.3 | Apache-2.0 | Permissive |
| `cpufeatures` | 0.2.17 | MIT | Permissive |
| `cpufeatures` | 0.3.1 | MIT | Permissive |
| `crc` | 3.4.0 | MIT | Permissive |
| `crc-catalog` | 2.5.0 | MIT | Permissive |
| `crc32fast` | 1.5.2 | MIT | Permissive |
| `crossbeam-channel` | 0.5.17 | MIT | Permissive |
| `crossbeam-deque` | 0.8.8 | MIT | Permissive |
| `crossbeam-epoch` | 0.9.21 | MIT | Permissive |
| `crossbeam-queue` | 0.3.14 | MIT | Permissive |
| `crossbeam-utils` | 0.8.23 | MIT | Permissive |
| `crypto-common` | 0.1.7 | MIT | Permissive |
| `cssparser` | 0.36.0 | MPL-2.0 | Notice required |
| `cssparser-macros` | 0.6.1 | MPL-2.0 | Notice required |
| `ctor` | 0.8.0 | Apache-2.0 | Permissive |
| `ctor-proc-macro` | 0.0.7 | Apache-2.0 | Permissive |
| `darling` | 0.24.1 | MIT | Permissive |
| `darling_core` | 0.24.1 | MIT | Permissive |
| `darling_macro` | 0.24.1 | MIT | Permissive |
| `dasp_sample` | 0.11.0 | MIT | Permissive |
| `data-encoding` | 2.11.1 | MIT | Permissive |
| `dbus` | 0.9.12 | Apache-2.0 | Permissive |
| `defmt` | 1.2.0 | MIT | Permissive |
| `defmt-macros` | 1.2.0 | MIT | Permissive |
| `defmt-parser` | 1.1.0 | MIT | Permissive |
| `der` | 0.7.10 | Apache-2.0 | Permissive |
| `deranged` | 0.5.9 | MIT | Permissive |
| `derive_more` | 2.1.1 | MIT | Permissive |
| `derive_more-impl` | 2.1.1 | MIT | Permissive |
| `digest` | 0.10.7 | MIT | Permissive |
| `dirs` | 6.0.0 | MIT | Permissive |
| `dirs-sys` | 0.5.0 | MIT | Permissive |
| `dispatch2` | 0.3.1 | Zlib | Permissive |
| `displaydoc` | 0.2.7 | MIT | Permissive |
| `dlopen2` | 0.8.2 | MIT | Permissive |
| `dlopen2_derive` | 0.4.3 | MIT | Permissive |
| `dom_query` | 0.27.0 | MIT | Permissive |
| `dotenvy` | 0.15.7 | MIT | Permissive |
| `dpi` | 0.1.2 | Apache-2.0 AND MIT | Permissive |
| `dtoa` | 1.0.11 | MIT | Permissive |
| `dtoa-short` | 0.3.5 | MPL-2.0 | Notice required |
| `dtor` | 0.3.0 | Apache-2.0 | Permissive |
| `dtor-proc-macro` | 0.0.6 | Apache-2.0 | Permissive |
| `dunce` | 1.0.5 | CC0-1.0 | Permissive |
| `dyn-clone` | 1.0.20 | MIT | Permissive |
| `either` | 1.19.0 | MIT | Permissive |
| `embed_plist` | 1.2.2 | MIT | Permissive |
| `encoding_rs` | 0.8.42 | Apache-2.0 | Permissive |
| `endi` | 1.1.1 | MIT | Permissive |
| `enumflags2` | 0.7.12 | MIT | Permissive |
| `enumflags2_derive` | 0.7.12 | MIT | Permissive |
| `equivalent` | 1.0.3 | Apache-2.0 | Permissive |
| `erased-serde` | 0.4.10 | MIT | Permissive |
| `errno` | 0.3.14 | MIT | Permissive |
| `etcetera` | 0.8.0 | MIT | Permissive |
| `event-listener` | 5.4.2 | Apache-2.0 | Permissive |
| `event-listener-strategy` | 0.5.4 | Apache-2.0 | Permissive |
| `extended` | 0.1.0 | MIT | Permissive |
| `fastrand` | 2.5.0 | Apache-2.0 | Permissive |
| `fdeflate` | 0.3.7 | MIT | Permissive |
| `field-offset` | 0.3.6 | MIT | Permissive |
| `flate2` | 1.1.10 | MIT | Permissive |
| `flume` | 0.11.1 | Apache-2.0 | Permissive |
| `fnv` | 1.0.7 | Apache-2.0 | Permissive |
| `foldhash` | 0.2.0 | Zlib | Permissive |
| `foldhash` | 0.1.5 | Zlib | Permissive |
| `foreign-types` | 0.5.0 | MIT | Permissive |
| `foreign-types-macros` | 0.2.4 | MIT | Permissive |
| `foreign-types-shared` | 0.3.1 | MIT | Permissive |
| `form_urlencoded` | 1.2.2 | MIT | Permissive |
| `futures` | 0.3.34 | MIT | Permissive |
| `futures-channel` | 0.3.34 | MIT | Permissive |
| `futures-core` | 0.3.34 | MIT | Permissive |
| `futures-executor` | 0.3.34 | MIT | Permissive |
| `futures-intrusive` | 0.5.0 | MIT | Permissive |
| `futures-io` | 0.3.34 | MIT | Permissive |
| `futures-lite` | 2.6.1 | Apache-2.0 | Permissive |
| `futures-macro` | 0.3.34 | MIT | Permissive |
| `futures-sink` | 0.3.34 | MIT | Permissive |
| `futures-task` | 0.3.34 | MIT | Permissive |
| `futures-util` | 0.3.34 | MIT | Permissive |
| `gdk` | 0.18.2 | MIT | Permissive |
| `gdk-pixbuf` | 0.18.5 | MIT | Permissive |
| `gdk-pixbuf-sys` | 0.18.0 | MIT | Permissive |
| `gdk-sys` | 0.18.2 | MIT | Permissive |
| `gdkwayland-sys` | 0.18.2 | MIT | Permissive |
| `gdkx11` | 0.18.2 | MIT | Permissive |
| `gdkx11-sys` | 0.18.2 | MIT | Permissive |
| `generic-array` | 0.14.7 | MIT | Permissive |
| `getrandom` | 0.4.3 | MIT | Permissive |
| `getrandom` | 0.3.4 | MIT | Permissive |
| `getrandom` | 0.2.17 | MIT | Permissive |
| `gio` | 0.18.4 | MIT | Permissive |
| `gio-sys` | 0.18.1 | MIT | Permissive |
| `glib` | 0.18.5 | MIT | Permissive |
| `glib-macros` | 0.18.5 | MIT | Permissive |
| `glib-sys` | 0.18.1 | MIT | Permissive |
| `glob` | 0.3.4 | MIT | Permissive |
| `gobject-sys` | 0.18.0 | MIT | Permissive |
| `gtk` | 0.18.2 | MIT | Permissive |
| `gtk-sys` | 0.18.2 | MIT | Permissive |
| `gtk3-macros` | 0.18.2 | MIT | Permissive |
| `hashbrown` | 0.17.1 | MIT | Permissive |
| `hashbrown` | 0.12.3 | MIT | Permissive |
| `hashbrown` | 0.15.5 | MIT | Permissive |
| `hashlink` | 0.10.0 | MIT | Permissive |
| `heck` | 0.5.0 | MIT | Permissive |
| `heck` | 0.4.1 | MIT | Permissive |
| `hermit-abi` | 0.5.3 | MIT | Permissive |
| `hex` | 0.4.3 | MIT | Permissive |
| `hkdf` | 0.12.4 | MIT | Permissive |
| `hmac` | 0.12.1 | MIT | Permissive |
| `home` | 0.5.12 | MIT | Permissive |
| `html5ever` | 0.38.0 | MIT | Permissive |
| `http` | 1.5.0 | MIT | Permissive |
| `http-body` | 1.1.0 | MIT | Permissive |
| `http-body-util` | 0.1.5 | MIT | Permissive |
| `http-range` | 0.1.6 | MIT | Permissive |
| `httparse` | 1.10.1 | MIT | Permissive |
| `hyper` | 1.12.0 | MIT | Permissive |
| `hyper-rustls` | 0.27.10 | Apache-2.0 | Permissive |
| `hyper-util` | 0.1.21 | MIT | Permissive |
| `iana-time-zone` | 0.1.65 | MIT | Permissive |
| `iana-time-zone-haiku` | 0.1.2 | MIT | Permissive |
| `ico` | 0.5.0 | MIT | Permissive |
| `icu_collections` | 2.3.0 | Unicode-3.0 | Permissive |
| `icu_locale_core` | 2.3.0 | Unicode-3.0 | Permissive |
| `icu_normalizer` | 2.3.0 | Unicode-3.0 | Permissive |
| `icu_normalizer_data` | 2.3.0 | Unicode-3.0 | Permissive |
| `icu_properties` | 2.3.0 | Unicode-3.0 | Permissive |
| `icu_properties_data` | 2.3.0 | Unicode-3.0 | Permissive |
| `icu_provider` | 2.3.1 | Unicode-3.0 | Permissive |
| `ident_case` | 1.0.1 | MIT | Permissive |
| `idna` | 1.1.0 | MIT | Permissive |
| `idna_adapter` | 1.2.2 | Apache-2.0 | Permissive |
| `image` | 0.25.10 | MIT | Permissive |
| `image-webp` | 0.2.4 | MIT | Permissive |
| `indexmap` | 2.14.2 | Apache-2.0 | Permissive |
| `indexmap` | 1.9.3 | Apache-2.0 | Permissive |
| `infer` | 0.19.0 | MIT | Permissive |
| `ipnet` | 2.12.2 | MIT | Permissive |
| `is-docker` | 0.2.0 | MIT | Permissive |
| `is-wsl` | 0.4.0 | MIT | Permissive |
| `itoa` | 1.0.18 | MIT | Permissive |
| `javascriptcore-rs` | 1.1.2 | MIT | Permissive |
| `javascriptcore-rs-sys` | 1.1.1 | MIT | Permissive |
| `jiff` | 0.2.38 | Unlicense | Permissive |
| `jiff-core` | 0.1.1 | Unlicense | Permissive |
| `jiff-static` | 0.2.38 | Unlicense | Permissive |
| `jiff-tzdb` | 0.1.9 | Unlicense | Permissive |
| `jiff-tzdb-platform` | 0.1.3 | Unlicense | Permissive |
| `jni` | 0.21.1 | MIT | Permissive |
| `jni-sys` | 0.3.1 | MIT | Permissive |
| `jni-sys` | 0.4.1 | MIT | Permissive |
| `jni-sys-macros` | 0.4.1 | MIT | Permissive |
| `js-sys` | 0.3.106 | MIT | Permissive |
| `json-patch` | 3.0.1 | MIT | Permissive |
| `jsonptr` | 0.6.3 | MIT | Permissive |
| `keyboard-types` | 0.7.0 | MIT | Permissive |
| `lazy_static` | 1.5.1 | MIT | Permissive |
| `libappindicator` | 0.9.0 | Apache-2.0 | Permissive |
| `libappindicator-sys` | 0.9.0 | Apache-2.0 | Permissive |
| `libc` | 0.2.191 | MIT | Permissive |
| `libdbus-sys` | 0.2.7 | Apache-2.0 | Permissive |
| `libloading` | 0.7.4 | ISC | Permissive |
| `libm` | 0.2.16 | MIT | Permissive |
| `libredox` | 0.1.25 | MIT | Permissive |
| `libspa` | 0.8.0 | MIT | Permissive |
| `libspa-sys` | 0.8.0 | MIT | Permissive |
| `libsqlite3-sys` | 0.30.1 | MIT | Permissive |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception | Permissive |
| `litemap` | 0.8.3 | Unicode-3.0 | Permissive |
| `lock_api` | 0.4.14 | MIT | Permissive |
| `lofty` | 0.22.4 | MIT | Permissive |
| `lofty_attr` | 0.11.1 | MIT | Permissive |
| `log` | 0.4.34 | MIT | Permissive |
| `lru-slab` | 0.1.3 | MIT | Permissive |
| `mach2` | 0.4.3 | BSD-2-Clause | Permissive |
| `markup5ever` | 0.38.0 | MIT | Permissive |
| `matchers` | 0.2.0 | MIT | Permissive |
| `md-5` | 0.10.6 | MIT | Permissive |
| `memchr` | 2.8.3 | Unlicense | Permissive |
| `memoffset` | 0.9.1 | MIT | Permissive |
| `mime` | 0.3.17 | MIT | Permissive |
| `minimal-lexical` | 0.2.1 | MIT | Permissive |
| `miniz_oxide` | 0.8.9 | MIT | Permissive |
| `miniz_oxide` | 0.9.1 | MIT | Permissive |
| `mio` | 1.2.4 | MIT | Permissive |
| `moxcms` | 0.8.1 | BSD-3-Clause | Permissive |
| `muda` | 0.19.3 | Apache-2.0 | Permissive |
| `multiversion_no_op` | 1.0.0 | Apache-2.0 | Permissive |
| `ndk` | 0.9.0 | MIT | Permissive |
| `ndk` | 0.8.0 | MIT | Permissive |
| `ndk-context` | 0.1.1 | MIT | Permissive |
| `ndk-sys` | 0.6.0+11769913 | MIT | Permissive |
| `ndk-sys` | 0.5.0+25.2.9519653 | MIT | Permissive |
| `new_debug_unreachable` | 1.0.6 | MIT | Permissive |
| `nix` | 0.27.1 | MIT | Permissive |
| `nom` | 7.1.3 | MIT | Permissive |
| `nu-ansi-term` | 0.50.3 | MIT | Permissive |
| `num_enum` | 0.7.6 | BSD-3-Clause | Permissive |
| `num_enum_derive` | 0.7.6 | BSD-3-Clause | Permissive |
| `num-bigint-dig` | 0.8.6 | MIT | Permissive |
| `num-complex` | 0.4.6 | MIT | Permissive |
| `num-conv` | 0.2.2 | MIT | Permissive |
| `num-derive` | 0.4.2 | MIT | Permissive |
| `num-integer` | 0.1.47 | MIT | Permissive |
| `num-iter` | 0.1.46 | MIT | Permissive |
| `num-traits` | 0.2.19 | MIT | Permissive |
| `objc2` | 0.6.5 | MIT | Permissive |
| `objc2-app-kit` | 0.3.2 | Zlib | Permissive |
| `objc2-cloud-kit` | 0.3.2 | Zlib | Permissive |
| `objc2-core-data` | 0.3.2 | Zlib | Permissive |
| `objc2-core-foundation` | 0.3.2 | Zlib | Permissive |
| `objc2-core-graphics` | 0.3.2 | Zlib | Permissive |
| `objc2-core-image` | 0.3.2 | Zlib | Permissive |
| `objc2-core-location` | 0.3.2 | Zlib | Permissive |
| `objc2-core-text` | 0.3.2 | Zlib | Permissive |
| `objc2-encode` | 4.1.0 | MIT | Permissive |
| `objc2-exception-helper` | 0.1.1 | Zlib | Permissive |
| `objc2-foundation` | 0.3.2 | MIT | Permissive |
| `objc2-io-surface` | 0.3.2 | Zlib | Permissive |
| `objc2-quartz-core` | 0.3.2 | Zlib | Permissive |
| `objc2-ui-kit` | 0.3.2 | Zlib | Permissive |
| `objc2-user-notifications` | 0.3.2 | Zlib | Permissive |
| `objc2-web-kit` | 0.3.2 | Zlib | Permissive |
| `oboe` | 0.6.1 | Apache-2.0 | Permissive |
| `oboe-sys` | 0.6.1 | Apache-2.0 | Permissive |
| `ogg_pager` | 0.7.1 | MIT | Permissive |
| `once_cell` | 1.21.4 | MIT | Permissive |
| `open` | 5.4.4 | MIT | Permissive |
| `option-ext` | 0.2.0 | MPL-2.0 | Notice required |
| `ordered-stream` | 0.2.0 | MIT | Permissive |
| `palette` | 0.7.7 | MIT | Permissive |
| `palette_derive` | 0.7.7 | MIT | Permissive |
| `palette_math` | 0.7.7 | MIT | Permissive |
| `pango` | 0.18.3 | MIT | Permissive |
| `pango-sys` | 0.18.0 | MIT | Permissive |
| `parking` | 2.2.1 | Apache-2.0 | Permissive |
| `parking_lot` | 0.12.5 | MIT | Permissive |
| `parking_lot_core` | 0.9.12 | MIT | Permissive |
| `paste` | 1.0.15 | MIT | Permissive |
| `pem-rfc7468` | 0.7.0 | Apache-2.0 | Permissive |
| `percent-encoding` | 2.3.2 | MIT | Permissive |
| `phf` | 0.13.1 | MIT | Permissive |
| `phf_generator` | 0.13.1 | MIT | Permissive |
| `phf_macros` | 0.13.1 | MIT | Permissive |
| `phf_shared` | 0.13.1 | MIT | Permissive |
| `pin-project-lite` | 0.2.17 | Apache-2.0 | Permissive |
| `piper` | 0.2.5 | MIT | Permissive |
| `pipewire` | 0.8.0 | MIT | Permissive |
| `pipewire-sys` | 0.8.0 | MIT | Permissive |
| `pkcs1` | 0.7.5 | Apache-2.0 | Permissive |
| `pkcs8` | 0.10.2 | Apache-2.0 | Permissive |
| `plain` | 0.2.3 | MIT | Permissive |
| `plist` | 1.10.1 | MIT | Permissive |
| `png` | 0.18.1 | MIT | Permissive |
| `png` | 0.17.16 | MIT | Permissive |
| `polling` | 3.11.0 | Apache-2.0 | Permissive |
| `portable-atomic` | 1.15.0 | Apache-2.0 | Permissive |
| `portable-atomic-util` | 0.2.8 | Apache-2.0 | Permissive |
| `potential_utf` | 0.1.6 | Unicode-3.0 | Permissive |
| `powerfmt` | 0.2.1 | MIT | Permissive |
| `ppv-lite86` | 0.2.21 | MIT | Permissive |
| `precomputed-hash` | 0.1.1 | MIT | Permissive |
| `primal-check` | 0.3.4 | MIT | Permissive |
| `proc-macro-crate` | 3.5.0 | MIT | Permissive |
| `proc-macro-crate` | 2.0.0 | MIT | Permissive |
| `proc-macro-crate` | 1.3.1 | MIT | Permissive |
| `proc-macro-error` | 1.0.4 | MIT | Permissive |
| `proc-macro-error-attr` | 1.0.4 | MIT | Permissive |
| `proc-macro2` | 1.0.107 | MIT | Permissive |
| `pxfm` | 0.1.30 | BSD-3-Clause | Permissive |
| `quick-error` | 2.0.1 | MIT | Permissive |
| `quick-xml` | 0.42.0 | MIT | Permissive |
| `quinn` | 0.11.12 | MIT | Permissive |
| `quinn-proto` | 0.11.19 | MIT | Permissive |
| `quinn-udp` | 0.5.16 | MIT | Permissive |
| `quote` | 1.0.47 | MIT | Permissive |
| `r-efi` | 6.0.0 | MIT | Permissive |
| `r-efi` | 5.3.0 | MIT | Permissive |
| `rand` | 0.8.8 | MIT | Permissive |
| `rand` | 0.10.3 | MIT | Permissive |
| `rand_chacha` | 0.3.1 | MIT | Permissive |
| `rand_core` | 0.10.1 | MIT | Permissive |
| `rand_core` | 0.6.4 | MIT | Permissive |
| `rand_pcg` | 0.10.2 | MIT | Permissive |
| `raw-window-handle` | 0.6.2 | MIT | Permissive |
| `rayon` | 1.12.0 | MIT | Permissive |
| `rayon-core` | 1.13.0 | MIT | Permissive |
| `realfft` | 3.5.0 | MIT | Permissive |
| `redox_syscall` | 0.5.18 | MIT | Permissive |
| `redox_syscall` | 0.9.5 | MIT | Permissive |
| `redox_users` | 0.5.3 | MIT | Permissive |
| `ref-cast` | 1.0.27 | MIT | Permissive |
| `ref-cast-impl` | 1.0.27 | MIT | Permissive |
| `regex` | 1.13.1 | MIT | Permissive |
| `regex-automata` | 0.4.18 | MIT | Permissive |
| `regex-syntax` | 0.8.11 | MIT | Permissive |
| `reqwest` | 0.12.28 | MIT | Permissive |
| `reqwest` | 0.13.5 | MIT | Permissive |
| `rfd` | 0.16.0 | MIT | Permissive |
| `rgb` | 0.8.53 | MIT | Permissive |
| `ring` | 0.17.14 | Apache-2.0 AND ISC | Permissive |
| `ringbuf` | 0.4.8 | MIT | Permissive |
| `rsa` | 0.9.10 | MIT | Permissive |
| `rubato` | 0.16.2 | MIT | Permissive |
| `rustc-hash` | 2.1.3 | Apache-2.0 | Permissive |
| `rustfft` | 6.4.1 | MIT | Permissive |
| `rustix` | 1.1.5 | Apache-2.0 WITH LLVM-exception | Permissive |
| `rustls` | 0.23.46 | Apache-2.0 | Permissive |
| `rustls-pki-types` | 1.15.1 | MIT | Permissive |
| `rustls-webpki` | 0.103.15 | ISC | Permissive |
| `rustversion` | 1.0.23 | MIT | Permissive |
| `ryu` | 1.0.23 | Apache-2.0 | Permissive |
| `same-file` | 1.0.6 | Unlicense | Permissive |
| `schemars` | 0.8.22 | MIT | Permissive |
| `schemars` | 0.9.0 | MIT | Permissive |
| `schemars` | 1.2.3 | MIT | Permissive |
| `schemars_derive` | 0.8.22 | MIT | Permissive |
| `scopeguard` | 1.2.0 | MIT | Permissive |
| `selectors` | 0.36.1 | MPL-2.0 | Notice required |
| `semver` | 1.0.28 | MIT | Permissive |
| `serde` | 1.0.229 | MIT | Permissive |
| `serde_core` | 1.0.229 | MIT | Permissive |
| `serde_derive` | 1.0.229 | MIT | Permissive |
| `serde_derive_internals` | 0.29.1 | MIT | Permissive |
| `serde_json` | 1.0.154 | MIT | Permissive |
| `serde_repr` | 0.1.21 | MIT | Permissive |
| `serde_spanned` | 0.6.9 | MIT | Permissive |
| `serde_spanned` | 1.1.2 | MIT | Permissive |
| `serde_urlencoded` | 0.7.1 | MIT | Permissive |
| `serde_with` | 3.24.0 | MIT | Permissive |
| `serde_with_macros` | 3.24.0 | MIT | Permissive |
| `serde-untagged` | 0.1.9 | MIT | Permissive |
| `serialize-to-javascript` | 0.1.2 | MIT | Permissive |
| `serialize-to-javascript-impl` | 0.1.2 | MIT | Permissive |
| `servo_arc` | 0.4.3 | MIT | Permissive |
| `sha1` | 0.10.7 | MIT | Permissive |
| `sha2` | 0.10.9 | MIT | Permissive |
| `sharded-slab` | 0.1.7 | MIT | Permissive |
| `signal-hook-registry` | 1.4.8 | MIT | Permissive |
| `signature` | 2.2.0 | Apache-2.0 | Permissive |
| `simd-adler32` | 0.3.10 | MIT | Permissive |
| `simdutf8` | 0.1.5 | MIT | Permissive |
| `siphasher` | 1.0.4 | MIT | Permissive |
| `slab` | 0.4.12 | MIT | Permissive |
| `smallvec` | 1.16.3 | MIT | Permissive |
| `socket2` | 0.6.5 | MIT | Permissive |
| `softbuffer` | 0.4.8 | MIT | Permissive |
| `soup3` | 0.5.0 | MIT | Permissive |
| `soup3-sys` | 0.5.0 | MIT | Permissive |
| `spin` | 0.9.9 | MIT | Permissive |
| `spki` | 0.7.3 | Apache-2.0 | Permissive |
| `sqlx` | 0.8.6 | MIT | Permissive |
| `sqlx-core` | 0.8.6 | MIT | Permissive |
| `sqlx-macros` | 0.8.6 | MIT | Permissive |
| `sqlx-macros-core` | 0.8.6 | MIT | Permissive |
| `sqlx-mysql` | 0.8.6 | MIT | Permissive |
| `sqlx-postgres` | 0.8.6 | MIT | Permissive |
| `sqlx-sqlite` | 0.8.6 | MIT | Permissive |
| `stable_deref_trait` | 1.2.1 | MIT | Permissive |
| `strength_reduce` | 0.2.4 | MIT | Permissive |
| `string_cache` | 0.9.0 | MIT | Permissive |
| `stringprep` | 0.1.5 | MIT | Permissive |
| `strsim` | 0.11.1 | MIT | Permissive |
| `subtle` | 2.6.1 | BSD-3-Clause | Permissive |
| `swift-rs` | 1.0.9 | MIT | Permissive |
| `symlink` | 0.1.0 | MIT | Permissive |
| `symphonia` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-bundle-flac` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-bundle-mp3` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-codec-alac` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-codec-pcm` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-codec-vorbis` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-core` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-format-mkv` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-format-ogg` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-format-riff` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-metadata` | 0.5.5 | MPL-2.0 | Notice required |
| `symphonia-utils-xiph` | 0.5.5 | MPL-2.0 | Notice required |
| `syn` | 2.0.119 | MIT | Permissive |
| `syn` | 3.0.9 | MIT | Permissive |
| `syn` | 1.0.109 | MIT | Permissive |
| `sync_wrapper` | 1.0.2 | Apache-2.0 | Permissive |
| `synstructure` | 0.14.0 | MIT | Permissive |
| `tao` | 0.35.3 | Apache-2.0 | Permissive |
| `tao-macros` | 0.1.4 | MIT | Permissive |
| `tauri` | 2.11.6 | Apache-2.0 | Permissive |
| `tauri-codegen` | 2.6.3 | Apache-2.0 | Permissive |
| `tauri-macros` | 2.6.3 | Apache-2.0 | Permissive |
| `tauri-plugin-dialog` | 2.7.3 | Apache-2.0 | Permissive |
| `tauri-plugin-fs` | 2.5.2 | Apache-2.0 | Permissive |
| `tauri-plugin-opener` | 2.5.5 | Apache-2.0 | Permissive |
| `tauri-plugin-single-instance` | 2.4.5 | Apache-2.0 | Permissive |
| `tauri-runtime` | 2.11.3 | Apache-2.0 | Permissive |
| `tauri-runtime-wry` | 2.11.4 | Apache-2.0 | Permissive |
| `tauri-utils` | 2.9.3 | Apache-2.0 | Permissive |
| `tempfile` | 3.27.0 | MIT | Permissive |
| `tendril` | 0.5.1 | MIT | Permissive |
| `thiserror` | 2.0.21 | MIT | Permissive |
| `thiserror` | 1.0.69 | MIT | Permissive |
| `thiserror-impl` | 2.0.21 | MIT | Permissive |
| `thiserror-impl` | 1.0.69 | MIT | Permissive |
| `thread_local` | 1.1.10 | MIT | Permissive |
| `time` | 0.3.55 | MIT | Permissive |
| `time-core` | 0.1.9 | MIT | Permissive |
| `time-macros` | 0.2.32 | MIT | Permissive |
| `tinystr` | 0.8.4 | Unicode-3.0 | Permissive |
| `tinyvec` | 1.13.3 | Zlib | Permissive |
| `tokio` | 1.53.3 | MIT | Permissive |
| `tokio-macros` | 2.7.2 | MIT | Permissive |
| `tokio-rustls` | 0.26.6 | MIT | Permissive |
| `tokio-stream` | 0.1.19 | MIT | Permissive |
| `tokio-util` | 0.7.20 | MIT | Permissive |
| `toml` | 0.8.23 | MIT | Permissive |
| `toml` | 1.1.8+spec-1.1.0 | MIT | Permissive |
| `toml_datetime` | 0.6.11 | MIT | Permissive |
| `toml_datetime` | 1.1.2+spec-1.1.0 | MIT | Permissive |
| `toml_edit` | 0.22.27 | MIT | Permissive |
| `toml_edit` | 0.25.17+spec-1.1.0 | MIT | Permissive |
| `toml_edit` | 0.20.7 | MIT | Permissive |
| `toml_edit` | 0.19.15 | MIT | Permissive |
| `toml_parser` | 1.1.5+spec-1.1.0 | MIT | Permissive |
| `toml_write` | 0.1.2 | MIT | Permissive |
| `toml_writer` | 1.1.3+spec-1.1.0 | MIT | Permissive |
| `tower` | 0.5.3 | MIT | Permissive |
| `tower-http` | 0.6.11 | MIT | Permissive |
| `tower-layer` | 0.3.3 | MIT | Permissive |
| `tower-service` | 0.3.3 | MIT | Permissive |
| `tracing` | 0.1.44 | MIT | Permissive |
| `tracing-appender` | 0.2.5 | MIT | Permissive |
| `tracing-attributes` | 0.1.31 | MIT | Permissive |
| `tracing-core` | 0.1.36 | MIT | Permissive |
| `tracing-log` | 0.2.0 | MIT | Permissive |
| `tracing-serde` | 0.2.0 | MIT | Permissive |
| `tracing-subscriber` | 0.3.23 | MIT | Permissive |
| `transpose` | 0.2.3 | MIT | Permissive |
| `tray-icon` | 0.24.2 | MIT | Permissive |
| `try-lock` | 0.2.5 | MIT | Permissive |
| `typeid` | 1.0.3 | MIT | Permissive |
| `typenum` | 1.20.1 | MIT | Permissive |
| `uds_windows` | 1.2.1 | MIT | Permissive |
| `unic-char-property` | 0.9.0 | MIT | Permissive |
| `unic-char-range` | 0.9.0 | MIT | Permissive |
| `unic-common` | 0.9.0 | MIT | Permissive |
| `unic-ucd-ident` | 0.9.0 | MIT | Permissive |
| `unic-ucd-version` | 0.9.0 | MIT | Permissive |
| `unicode-bidi` | 0.3.18 | MIT | Permissive |
| `unicode-ident` | 1.0.27 | MIT | Permissive |
| `unicode-normalization` | 0.1.26 | MIT | Permissive |
| `unicode-properties` | 0.1.4 | MIT | Permissive |
| `unicode-segmentation` | 1.13.3 | MIT | Permissive |
| `untrusted` | 0.9.0 | ISC | Permissive |
| `url` | 2.5.8 | MIT | Permissive |
| `urlpattern` | 0.3.0 | MIT | Permissive |
| `utf8_iter` | 1.0.4 | Apache-2.0 | Permissive |
| `uuid` | 1.26.1 | Apache-2.0 | Permissive |
| `valuable` | 0.1.1 | MIT | Permissive |
| `walkdir` | 2.5.0 | Unlicense | Permissive |
| `want` | 0.3.2 | MIT | Permissive |
| `wasi` | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception | Permissive |
| `wasip2` | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception | Permissive |
| `wasite` | 0.1.0 | Apache-2.0 | Permissive |
| `wasm-bindgen` | 0.2.129 | MIT | Permissive |
| `wasm-bindgen-futures` | 0.4.79 | MIT | Permissive |
| `wasm-bindgen-macro` | 0.2.129 | MIT | Permissive |
| `wasm-bindgen-macro-support` | 0.2.129 | MIT | Permissive |
| `wasm-bindgen-shared` | 0.2.129 | MIT | Permissive |
| `wasm-streams` | 0.5.0 | MIT | Permissive |
| `web_atoms` | 0.2.6 | MIT | Permissive |
| `web-sys` | 0.3.106 | MIT | Permissive |
| `web-time` | 1.1.0 | MIT | Permissive |
| `webkit2gtk` | 2.0.2 | MIT | Permissive |
| `webkit2gtk-sys` | 2.0.2 | MIT | Permissive |
| `webpki-roots` | 1.0.9 | CDLA-Permissive-2.0 | Notice required |
| `webview2-com` | 0.38.2 | MIT | Permissive |
| `webview2-com-macros` | 0.8.1 | MIT | Permissive |
| `webview2-com-sys` | 0.38.2 | MIT | Permissive |
| `whoami` | 1.6.1 | Apache-2.0 | Permissive |
| `winapi` | 0.3.9 | MIT | Permissive |
| `winapi-i686-pc-windows-gnu` | 0.4.0 | MIT | Permissive |
| `winapi-util` | 0.1.11 | Unlicense | Permissive |
| `winapi-x86_64-pc-windows-gnu` | 0.4.0 | MIT | Permissive |
| `window-vibrancy` | 0.6.0 | Apache-2.0 | Permissive |
| `windows` | 0.58.0 | MIT | Permissive |
| `windows` | 0.61.3 | MIT | Permissive |
| `windows` | 0.54.0 | MIT | Permissive |
| `windows_aarch64_gnullvm` | 0.52.6 | MIT | Permissive |
| `windows_aarch64_gnullvm` | 0.53.1 | MIT | Permissive |
| `windows_aarch64_gnullvm` | 0.42.2 | MIT | Permissive |
| `windows_aarch64_gnullvm` | 0.48.5 | MIT | Permissive |
| `windows_aarch64_msvc` | 0.52.6 | MIT | Permissive |
| `windows_aarch64_msvc` | 0.53.1 | MIT | Permissive |
| `windows_aarch64_msvc` | 0.42.2 | MIT | Permissive |
| `windows_aarch64_msvc` | 0.48.5 | MIT | Permissive |
| `windows_i686_gnu` | 0.52.6 | MIT | Permissive |
| `windows_i686_gnu` | 0.53.1 | MIT | Permissive |
| `windows_i686_gnu` | 0.42.2 | MIT | Permissive |
| `windows_i686_gnu` | 0.48.5 | MIT | Permissive |
| `windows_i686_gnullvm` | 0.52.6 | MIT | Permissive |
| `windows_i686_gnullvm` | 0.53.1 | MIT | Permissive |
| `windows_i686_msvc` | 0.52.6 | MIT | Permissive |
| `windows_i686_msvc` | 0.53.1 | MIT | Permissive |
| `windows_i686_msvc` | 0.42.2 | MIT | Permissive |
| `windows_i686_msvc` | 0.48.5 | MIT | Permissive |
| `windows_x86_64_gnu` | 0.52.6 | MIT | Permissive |
| `windows_x86_64_gnu` | 0.53.1 | MIT | Permissive |
| `windows_x86_64_gnu` | 0.42.2 | MIT | Permissive |
| `windows_x86_64_gnu` | 0.48.5 | MIT | Permissive |
| `windows_x86_64_gnullvm` | 0.52.6 | MIT | Permissive |
| `windows_x86_64_gnullvm` | 0.53.1 | MIT | Permissive |
| `windows_x86_64_gnullvm` | 0.42.2 | MIT | Permissive |
| `windows_x86_64_gnullvm` | 0.48.5 | MIT | Permissive |
| `windows_x86_64_msvc` | 0.52.6 | MIT | Permissive |
| `windows_x86_64_msvc` | 0.53.1 | MIT | Permissive |
| `windows_x86_64_msvc` | 0.42.2 | MIT | Permissive |
| `windows_x86_64_msvc` | 0.48.5 | MIT | Permissive |
| `windows-collections` | 0.2.0 | MIT | Permissive |
| `windows-core` | 0.58.0 | MIT | Permissive |
| `windows-core` | 0.61.2 | MIT | Permissive |
| `windows-core` | 0.62.2 | MIT | Permissive |
| `windows-core` | 0.54.0 | MIT | Permissive |
| `windows-future` | 0.2.1 | MIT | Permissive |
| `windows-implement` | 0.58.0 | MIT | Permissive |
| `windows-implement` | 0.60.2 | MIT | Permissive |
| `windows-interface` | 0.58.0 | MIT | Permissive |
| `windows-interface` | 0.59.3 | MIT | Permissive |
| `windows-link` | 0.2.1 | MIT | Permissive |
| `windows-link` | 0.1.3 | MIT | Permissive |
| `windows-numerics` | 0.2.0 | MIT | Permissive |
| `windows-result` | 0.2.0 | MIT | Permissive |
| `windows-result` | 0.3.4 | MIT | Permissive |
| `windows-result` | 0.4.1 | MIT | Permissive |
| `windows-result` | 0.1.2 | MIT | Permissive |
| `windows-strings` | 0.1.0 | MIT | Permissive |
| `windows-strings` | 0.4.2 | MIT | Permissive |
| `windows-strings` | 0.5.1 | MIT | Permissive |
| `windows-sys` | 0.61.2 | MIT | Permissive |
| `windows-sys` | 0.60.2 | MIT | Permissive |
| `windows-sys` | 0.59.0 | MIT | Permissive |
| `windows-sys` | 0.45.0 | MIT | Permissive |
| `windows-sys` | 0.48.0 | MIT | Permissive |
| `windows-sys` | 0.52.0 | MIT | Permissive |
| `windows-targets` | 0.52.6 | MIT | Permissive |
| `windows-targets` | 0.53.5 | MIT | Permissive |
| `windows-targets` | 0.42.2 | MIT | Permissive |
| `windows-targets` | 0.48.5 | MIT | Permissive |
| `windows-threading` | 0.1.0 | MIT | Permissive |
| `windows-version` | 0.1.7 | MIT | Permissive |
| `winnow` | 0.7.15 | MIT | Permissive |
| `winnow` | 1.0.4 | MIT | Permissive |
| `winnow` | 0.5.40 | MIT | Permissive |
| `wit-bindgen` | 0.57.1 | Apache-2.0 WITH LLVM-exception | Permissive |
| `writeable` | 0.6.4 | Unicode-3.0 | Permissive |
| `wry` | 0.55.1 | Apache-2.0 | Permissive |
| `x11` | 2.21.0 | MIT | Permissive |
| `x11-dl` | 2.21.0 | MIT | Permissive |
| `yoke` | 0.8.3 | Unicode-3.0 | Permissive |
| `yoke-derive` | 0.8.4 | Unicode-3.0 | Permissive |
| `zbus` | 5.19.0 | MIT | Permissive |
| `zbus_macros` | 5.19.0 | MIT | Permissive |
| `zbus_names` | 4.3.5 | MIT | Permissive |
| `zcheapstr` | 1.1.0 | MIT | Permissive |
| `zerocopy` | 0.8.63 | BSD-2-Clause | Permissive |
| `zerocopy-derive` | 0.8.63 | BSD-2-Clause | Permissive |
| `zerofrom` | 0.1.8 | Unicode-3.0 | Permissive |
| `zerofrom-derive` | 0.1.8 | Unicode-3.0 | Permissive |
| `zeroize` | 1.9.1 | Apache-2.0 | Permissive |
| `zerotrie` | 0.2.5 | Unicode-3.0 | Permissive |
| `zerovec` | 0.11.8 | Unicode-3.0 | Permissive |
| `zerovec-derive` | 0.11.6 | Unicode-3.0 | Permissive |
| `zlib-rs` | 0.6.8 | Zlib | Permissive |
| `zmij` | 1.0.23 | MIT | Permissive |
| `zune-core` | 0.5.3 | MIT | Permissive |
| `zune-jpeg` | 0.5.15 | MIT | Permissive |
| `zvariant` | 5.15.1 | MIT | Permissive |
| `zvariant_derive` | 5.15.1 | MIT | Permissive |
| `zvariant_utils` | 4.2.0 | MIT | Permissive |

## Inventory — npm, bundled into the frontend

| Package | Version | Licence (chosen) | Status |
| :------ | :------ | :--------------- | :----- |
| `@fontsource-variable/inter` | 5.3.0 | OFL-1.1 | Notice required |
| `@tanstack/react-virtual` | 3.14.21 | MIT | Permissive |
| `@tanstack/virtual-core` | 3.19.6 | MIT | Permissive |
| `@tauri-apps/api` | 2.11.1 | Apache-2.0 | Permissive |
| `@tauri-apps/plugin-dialog` | 2.7.3 | MIT | Permissive |
| `@tauri-apps/plugin-opener` | 2.5.5 | MIT | Permissive |
| `@types/react` | 19.3.0 | MIT | Permissive |
| `clsx` | 2.1.1 | MIT | Permissive |
| `csstype` | 3.2.3 | MIT | Permissive |
| `framer-motion` | 12.43.0 | MIT | Permissive |
| `lucide-react` | 0.468.0 | ISC | Permissive |
| `motion-dom` | 12.43.0 | MIT | Permissive |
| `motion-utils` | 12.39.0 | MIT | Permissive |
| `react` | 19.3.1 | MIT | Permissive |
| `react-dom` | 19.3.1 | MIT | Permissive |
| `scheduler` | 0.28.0 | MIT | Permissive |
| `tailwind-merge` | 3.7.0 | MIT | Permissive |
| `tslib` | 2.8.1 | 0BSD | Permissive |
| `zod` | 3.25.76 | MIT | Permissive |
| `zustand` | 5.0.15 | MIT | Permissive |

## Build-only dependencies

Reachable only through `[build-dependencies]`: they execute on the build machine and are not
linked into the artefact. Listed for completeness, not enforced.

- **Apache-2.0** (8): annotate-snippets, autocfg, cargo_toml, cexpr, clang-sys, rustc-hash, tauri-build, tauri-plugin
- **Apache-2.0 WITH LLVM-exception** (1): target-lexicon
- **BSD-3-Clause** (2): bindgen, bindgen
- **ISC** (1): libloading
- **MIT** (27): cc, cfg-expr, cfg_aliases, embed-resource, find-msvc-tools, itertools, itertools, jobserver, lazycell, phf_codegen, pkg-config, rustc_version, shlex, shlex, string_cache_codegen, system-deps, tauri-winres, toml, toml_datetime, unicode-width, vcpkg, version-compare, version_check, vswhom, vswhom-sys, winreg, yansi-term

## Reproducing this document

```bash
node scripts/license-audit.mjs          # regenerate
node scripts/license-audit.mjs --check  # verify (CI)
```

`cargo metadata --locked` resolves the Rust graph and `pnpm licenses list --json --prod`
resolves the frontend graph. Neither reaches the network when the caches are warm, which is
what lets the audit run in the same offline container as the rest of CI.

