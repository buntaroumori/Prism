# FASE 3 — Library, search and rich presence

**Status:** delivered, awaiting approval
**Branch:** `main` · `feat(library): incremental scanner, FTS5 search surface and Discord presence`
**Backend tests:** 175 passing (150 unit + 5 database + 5 engine + 9 library + 3 artwork + 3 Discord)

---

## 1. Scope

FASE 3 turns a database schema into a working catalogue. Three subsystems, one IPC surface:

| Deliverable           | Module                               | Lines |
| :-------------------- | :----------------------------------- | ----: |
| Metadata reader       | `metadata/tags.rs`                   |   526 |
| Scanner               | `metadata/scanner.rs`                | 1 355 |
| Catalogue writer      | `db/catalog.rs`                      |   911 |
| Search & read models  | `db/search.rs`                       |   794 |
| Cover art             | `artwork/{mod,providers}.rs`         |   770 |
| Discord Rich Presence | `discord/{mod,rpc}.rs`               | 1 142 |
| Commands              | `commands/{library,discord}.rs`      |   696 |
| Integration tests     | `tests/{library,artwork,discord}.rs` | 1 183 |

## 2. What each subsystem guarantees

### 2.1 Scanner

- **Streaming walk** with `walkdir`, pruning VCS metadata, Synology thumbnails and recycle bins
  before they are descended into; symlinks are not followed (a loop would hang the scan).
- **Parallel parse**: `rayon` over 512-file chunks inside `spawn_blocking`. Chunking bounds memory
  and produces progress ticks even when the disk is saturated.
- **Incremental by default**: `(mtime, size)` fingerprints skip unchanged files without opening
  them. A rescan of an untouched library performs zero tag reads and zero track writes.
- **CUE sheets** are expanded before audio files so the foreign key to `cue_sheets` is satisfiable
  inside the same transaction. The image file is indexed once per sheet, never as a standalone
  track, and each virtual track carries sample-accurate `cue_start_ms`/`cue_end_ms`.
- **Offline roots** (unmounted drive, dead share) are reported as `root_offline` and mark _nothing_
  missing — the difference between "deleted" and "unplugged" is the difference between losing a
  library and waiting a second.
- **Missing ≠ deleted**: a file that disappears is flagged `missing=1`; rating, play count and
  history survive. Albums that lose a track have their counters recomputed in the same transaction.
- **Cancellation** is cooperative at walk and chunk boundaries; committed batches stay committed
  and the run is recorded as `cancelled`.
- **Error isolation**: one row per failure in `scan_errors` (capped at 200 per run, the counter keeps
  counting), a stable code (`io`, `unsupported`, `malformed`, `cue`, `root_offline`), and the scan
  continues. A corrupt file never aborts a 100 000-file import.
- **Progress** is published at most every 200 ms, always at phase transitions.

### 2.2 Search

- FTS5 external-content index with `unicode61 remove_diacritics 2` — `Beyonce` finds `Beyoncé`.
- **Operator-safe query construction**: input is reduced to alphanumeric runs, each quoted as a
  phrase, prefix operator on the final token. `C++`, `NOT`, `foo:bar`, `"unterminated` and `NEAR(a b)`
  are all just searches. This is a correctness _and_ injection fix, not a nicety.
- `bm25` ranking with weights 10/6/4/3/2/1/1 (title, artist, album, album artist, genre, comment,
  composer) and a `title_sort` tie-breaker for stable paging.
- Highlights use `U+0001`/`U+0002` markers, so React renders them as JSX spans — no
  `dangerouslySetInnerHTML` anywhere near user data.
- Browse paths never touch FTS; every `ORDER BY` is a closed enum; `limit` is clamped to 500.
- Read models: `tracks`, `albums`, `artists`, `facets` (genres/formats/decades), and `global`
  (tracks + albums + artists in one round trip for the ⌘K overlay).

### 2.3 Cover art

- Key: `SHA-256(lower(artist) + U+001F + lower(album))` — the separator prevents
  `("AB","C")`/`("A","BC")` collisions; the key _is_ the `cover_cache` primary key.
- Provider chain: embedded picture (front cover first, then largest by bytes) → `cover.jpg` /
  `folder.png` / `front.*` → iTunes Search API → MusicBrainz (score ≥ 70 + title match) → Cover Art
  Archive `front-500` → bundled `prism` asset.
- Every image is decoded, bounded to 1000 px and re-encoded as JPEG q88 before it touches disk.
- 90-day success TTL, 24-hour negative cache, per-key in-flight collapsing, MusicBrainz rate limit
  of one request per 1.1 s, and an offline mode that never touches the network.
- Presence publishes text immediately and the artwork as a follow-up update, because Discord only
  accepts HTTPS asset URLs — a locally extracted cover cannot be uploaded.

### 2.4 Discord RPC

- Real IPC framing (`op: u32 LE, len: u32 LE, JSON`), handshake/READY, ping/pong, close.
- Transport: UNIX domain sockets across native, Flatpak and Snap runtime directories, plus Windows
  named pipes (`\\.\pipe\discord-ipc-N`), probing `discord-ipc-0…9`.
- Backoff: 1 s → 60 s, ×1.5 with < 500 ms jitter; updates coalesced to one per 2.5 s.
- Presence is driven by the **engine**, not React: minimising to tray keeps Discord accurate.

## 3. IPC surface (20 commands, 2 events)

| Command                                                              | Purpose                        |
| :------------------------------------------------------------------- | :----------------------------- |
| `get_library_stats`                                                  | Counters for the shell         |
| `library_scan_start` / `_cancel` / `_status` / `_runs` / `_errors`   | Scan control and ledger        |
| `library_roots` / `_add_root` / `_remove_root` / `_set_root_enabled` | Root management                |
| `library_search`                                                     | Filtered, sorted, paged tracks |
| `library_albums` / `_artists` / `_facets`                            | Browse grids                   |
| `library_global_search`                                              | ⌘K overlay                     |
| `library_album_cover` / `_track_cover`                               | Resolved `CoverAsset`          |
| `discord_status` / `_configure` / `_clear`                           | Presence control               |

Events: `library://scan-progress`, `library://scan-complete`.

Every payload is mirrored by a zod schema in `src/types/ipc.ts` and a typed wrapper in
`src/lib/ipc.ts`; a backend/frontend version skew surfaces as a `contract` failure instead of
`undefined` in the UI.

## 4. Verification

| Suite                                                       | Result        |
| :---------------------------------------------------------- | :------------ |
| `cargo test --lib`                                          | **150 / 150** |
| `cargo test --test database`                                | **5 / 5**     |
| `cargo test --test engine`                                  | **5 / 5**     |
| `cargo test --test library`                                 | **8 / 8**     |
| `cargo test --test artwork`                                 | **3 / 3**     |
| `cargo test --test discord`                                 | **3 / 3**     |
| `cargo clippy --all-targets -D correctness,suspicious,perf` | clean         |
| `cargo fmt --all --check`                                   | clean         |
| `pnpm check` (tsc strict + eslint + prettier)               | clean         |
| `pnpm build`                                                | clean         |

Cross-platform notes: the Discord integration suite binds a UNIX socket and is therefore
`#![cfg(unix)]`; the Windows transport (named pipe) is compiled by CI's native `windows-latest` job,
which runs the same clippy and `cargo test --all-targets` gate. The suite serialises the two tests
that mutate the process-global `XDG_RUNTIME_DIR`, which is what made a concurrent run flaky before
it was pinned down.

The integration tests are not mocks: they build real WAV/FLAC-compatible containers with real
ID3v2 tags, real CUE sheets, a real corrupt file, a real SQLite database with the production
migrations, and a **real Discord IPC server** speaking the wire protocol over a UNIX socket.

Covered scenarios: index counts and CUE offsets, incremental skip, deleted-file sweep, album
counter reconciliation and pruning, cancellation + progress, FTS ranking and diacritic folding,
filter/facet browsers, health after scan, writer idempotency, artwork normalisation and cache
hits, negative caching and expiry, embedded-vs-folder precedence, RPC handshake + activity
publication, config corruption fallback, clean shutdown, and root counters that escape `%`/`_` in
folder names.

## 5. Measured performance

Measured with `src-tauri/tests/scanner_bench.rs` (kept in the repository, `#[ignore]`d, reproducible):

```text
cargo test --release --test scanner_bench -- --ignored --nocapture
```

| Metric                                |                                             Result |
| :------------------------------------ | -------------------------------------------------: |
| Fixture                               |  100 000 files · 5 000 albums · 1.9 KB tagged WAVs |
| Fixture generation                    |                                            11.44 s |
| **Cold scan**                         |         **7.34 s** — 0.073 ms/file, 13 600 files/s |
| **Incremental rescan**                | **1.56 s** — 64 240 files/s, 0 tag reads, 0 writes |
| FTS query (`track 07`, 9 750 matches) |                                            55.3 ms |

Environment: 2 vCPU shared container, ext4, `opt-level = 1` (a full `opt-level = 3` + fat-LTO
release build cannot be produced on this 2 GB box without OOM). The gate was **< 10 s for 100 000
files**, met with 27 % headroom _in the least favourable configuration_; the release profile adds
another 20–50 % on the parse path. The search figure is dominated by the exact `COUNT(*)` over
9 750 matching rows, which is why the UI requests 200-row pages and never a full result set.

## 6. Deliberate non-goals

The FASE 1 README advertised "tag editor, file organiser" inside this phase. They are **not** part
of the FASE 3 acceptance criteria you defined (scanner, FTS5 search surface, Discord + artwork), so
they were not built and the table was corrected rather than left lying. Both are natural additions
to a later phase — say the word and I will schedule them explicitly.

## 7. Next

FASE 4 — strict TypeScript types (already seeded by this phase's zod contracts), Zustand stores,
Tauri IPC hooks, the main layout with Framer Motion, the palette-driven ambient background and the
PlayerBar with the Hi-Res indicator and output-device selector.
