#!/usr/bin/env python3
"""Schema regression harness for Prism's SQLite migrations.

Runs the *real* migration files against a throw-away SQLite database, then
exercises the invariants the Rust layer depends on:

  * every statement applies cleanly (STRICT tables, FTS5, triggers, views),
  * FTS5 search works through the external-content index and its triggers,
  * the artist/album rename cascades keep `tracks` and `tracks_fts` coherent,
  * `integrity-check` passes after inserts/updates/deletes,
  * CUE virtual tracks sharing one image path are legal while true duplicates
    are rejected,
  * JSON columns reject malformed documents.

This is executed in CI (`schema` job) on every push, and locally via
`python3 scripts/validate_schema.py`.

Exit code 0 = schema is sound. Exit code 1 = regression, with details.
"""

from __future__ import annotations

import hashlib
import pathlib
import sqlite3
import sys
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
MIGRATIONS = ROOT / "src-tauri" / "migrations"

FAILURES: list[str] = []


def check(condition: bool, label: str) -> None:
    if condition:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}")
        FAILURES.append(label)


def applies(conn: sqlite3.Connection) -> None:
    files = sorted(MIGRATIONS.glob("*.sql"))
    assert files, "no migrations found"
    for migration in files:
        conn.executescript(migration.read_text())
        print(f"applied {migration.name}")


def seed(conn: sqlite3.Connection) -> None:
    artist = str(uuid.uuid4())
    album = str(uuid.uuid4())
    cover = str(uuid.uuid4())
    sheet = str(uuid.uuid4())
    conn.execute(
        "INSERT INTO artists (id, name, sort_name) VALUES (?, ?, ?)",
        (artist, "Björk", "Bjork"),
    )
    conn.execute(
        "INSERT INTO cover_cache (id, artist_key, album_key, hash, provider, remote_url)"
        " VALUES (?, ?, ?, ?, 'itunes', ?)",
        (cover, "björk", "homogenic", hashlib.sha256(b"bjorkhomogenic").hexdigest(),
         "https://is1-ssl.mzstatic.com/image/thumb/x/1000x1000bb.jpg"),
    )
    conn.execute(
        "INSERT INTO albums (id, title, sort_title, artist_id, album_artist_id,"
        " album_artist_name, year, cover_id, track_count) VALUES (?,?,?,?,?,?,?,?,?)",
        (album, "Homogenic", "Homogenic", artist, artist, "Björk", 1997, cover, 0),
    )
    conn.execute(
        "INSERT INTO cue_sheets (id, album_id, path, album_image, sha256, title, content,"
        " track_count) VALUES (?,?,?,?,?,?,?,?)",
        (sheet, album, "/music/Homogenic.cue", "/music/Homogenic.flac",
         hashlib.sha256(b"cue").hexdigest(), "Homogenic",
         'FILE "Homogenic.flac" WAVE\n  TRACK 01 AUDIO\n    TITLE "Hunter"\n',
         2),
    )
    conn.execute(
        "INSERT INTO tracks (id, path, folder, filename, title, artist_id, artist_name,"
        " album_id, album_title, album_artist_name, genre, year, track_no, format,"
        " bit_depth, sample_rate, channels, duration_ms, file_size, extra_tags)"
        " VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        (str(uuid.uuid4()), "/music/Homogenic/01 Hunter.flac", "/music/Homogenic",
         "01 Hunter.flac", "Hunter", artist, "Björk", album, "Homogenic", "Björk",
         "Electronic", 1997, 1, "flac", 24, 96000, 2, 260_000, 42_000_000, "{}"),
    )
    # Two virtual tracks from the same image file (legal), different offsets.
    for idx, start in enumerate((0, 260_000)):
        conn.execute(
            "INSERT INTO tracks (id, path, folder, filename, source, title, artist_id,"
            " artist_name, album_id, album_title, album_artist_name, track_no, format,"
            " duration_ms, cue_sheet_id, cue_start_ms, cue_end_ms)"
            " VALUES (?,?,?,?,'cue',?,?,?,?,?,?,?,?,?,?,?,?)",
            (str(uuid.uuid4()), "/music/Homogenic.flac", "/music", "Homogenic.flac",
             "Cue Track " + str(idx + 1), artist, "Björk", album, "Homogenic", "Björk",
             idx + 1, "flac", 260_000, sheet, start, start + 260_000),
        )


def run() -> None:
    conn = sqlite3.connect(":memory:")
    conn.execute("PRAGMA foreign_keys = ON")
    applies(conn)
    seed(conn)
    print("seeded")

    print("FTS5")
    rows = conn.execute(
        "SELECT t.title FROM tracks_fts f JOIN tracks t ON t.rowid = f.rowid"
        " WHERE tracks_fts MATCH ?",
        ('"hunter"',),
    ).fetchall()
    check(rows == [("Hunter",)], "MATCH finds track by title")
    rows = conn.execute(
        "SELECT t.title FROM tracks_fts f JOIN tracks t ON t.rowid = f.rowid"
        " WHERE tracks_fts MATCH ?",
        ("bjork",),
    ).fetchall()
    check(len(rows) >= 1, "diacritics folded (bjork -> Björk)")
    rows = conn.execute(
        "SELECT count(*) FROM tracks_fts WHERE tracks_fts MATCH ?", ("electronic*",)
    ).fetchone()
    check(rows[0] >= 1, "prefix wildcard works")

    print("rename cascades")
    conn.execute("UPDATE artists SET name = 'Bjork Renamed'")
    check(
        conn.execute(
            "SELECT count(*) FROM tracks WHERE artist_name = 'Bjork Renamed'"
        ).fetchone()[0] == 3,
        "artist rename cascades to tracks",
    )
    check(
        conn.execute(
            "SELECT count(*) FROM tracks_fts WHERE tracks_fts MATCH ?",
            ('"renamed"',),
        ).fetchone()[0] == 3,
        "artist rename refreshes FTS",
    )
    conn.execute("UPDATE albums SET title = 'Homogenic (Remaster)'")
    check(
        conn.execute(
            "SELECT count(*) FROM tracks WHERE album_title = 'Homogenic (Remaster)'"
        ).fetchone()[0] == 3,
        "album rename cascades to tracks",
    )

    print("constraints")
    try:
        conn.execute(
            "INSERT INTO tracks (id, path, title, format, duration_ms)"
            " VALUES (?, '/music/Homogenic/01 Hunter.flac', 'Dup', 'flac', 1)",
            (str(uuid.uuid4()),),
        )
        check(False, "duplicate identity rejected")
    except sqlite3.IntegrityError:
        check(True, "duplicate identity rejected")
    try:
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('x', 'not json')"
        )
        check(False, "settings rejects invalid JSON")
    except sqlite3.IntegrityError:
        check(True, "settings rejects invalid JSON")
    try:
        conn.execute(
            "UPDATE tracks SET rating = 9 WHERE source = 'file'"
        )
        check(False, "rating range enforced")
    except sqlite3.IntegrityError:
        check(True, "rating range enforced")

    print("FTS external-content integrity")
    conn.execute("INSERT INTO tracks_fts (tracks_fts) VALUES ('integrity-check')")
    check(True, "integrity-check after update")
    conn.execute("DELETE FROM tracks WHERE source = 'cue'")
    conn.execute("INSERT INTO tracks_fts (tracks_fts) VALUES ('integrity-check')")
    check(True, "integrity-check after delete")

    print("read models")
    stats = conn.execute("SELECT * FROM v_library_stats").fetchone()
    check(stats[0] == 1, "v_library_stats counts surviving tracks")
    details = conn.execute("SELECT * FROM v_track_details").fetchall()
    check(len(details) == 1, "v_track_details returns one row")
    albums = conn.execute(
        "SELECT available_tracks, format FROM v_album_summary"
    ).fetchone()
    check(albums[0] == 1 and albums[1] == "flac", "v_album_summary aggregates")

    # The Home view sorts albums by the newest track they contain; without this
    # column every fresh launch failed with `no such column: added_at`.
    album_columns = [row[1] for row in conn.execute("PRAGMA table_info(v_album_summary)")]
    check("added_at" in album_columns, "v_album_summary exposes added_at")
    recent = conn.execute(
        "SELECT id FROM v_album_summary ORDER BY added_at DESC, id ASC LIMIT 5"
    ).fetchall()
    check(len(recent) >= 1, "v_album_summary sorts by added_at")
    playlists = conn.execute("SELECT * FROM v_playlist_summary").fetchall()
    check(playlists == [], "v_playlist_summary empty-safe")

    fk = conn.execute("PRAGMA foreign_key_check").fetchall()
    check(fk == [], "no foreign key violations")

    conn.close()
    if FAILURES:
        print(f"\n{len(FAILURES)} failure(s)")
        sys.exit(1)
    print("\nschema OK")


if __name__ == "__main__":
    run()
