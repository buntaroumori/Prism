-- =============================================================================
--  Prism — 0001_initial_schema.sql
--  Target: SQLite >= 3.45 (bundled by `libsqlite3-sys` through sqlx 0.8)
--  Journal: WAL (set at connection level by the database layer)
--
--  DESIGN NOTES
--  ------------
--  * All tables are STRICT: SQLite only accepts INTEGER / REAL / TEXT / BLOB /
--    ANY there, which eliminates per-row type drift in a 500k-row library.
--  * Timestamps are ISO-8601 UTC strings with millisecond precision
--    (`2026-10-19T12:34:56.789Z`). They sort lexicographically and decode
--    losslessly into `time::OffsetDateTime` with the `time` sqlx feature.
--  * File paths are stored verbatim (absolute, `\\?\` verbatim Windows paths
--    preserved). The identity index uses an expression that folds the two
--    path separators *and* lower-cases, so `C:\Music\a.flac` and
--    `c:/music/A.FLAC` can never be indexed twice on case-insensitive
--    filesystems while still allowing case-sensitive Linux trees to coexist.
--  * Denormalised columns (`tracks.artist_name`, `tracks.album_title`, ...)
--    exist because the library grid renders them directly for 500k rows
--    without a join. Triggers below keep them coherent when an artist or an
--    album is renamed, so denormalisation never becomes stale.
--  * The FTS5 index is `content='tracks'` (external content: zero duplication
--    of the base data) and is maintained by triggers, including the rename
--    cascades above. `tracks_fts` exposes ready-to-use `title`, `artist`,
--    `album`, `album_artist`, `genre`, `comment` and `composer` columns.
--  * Everything the player needs to boot offline is local: cover art cache
--    rows carry either a remote URL (Discord RPC requirement) or a local
--    extracted file.
-- =============================================================================

-- -----------------------------------------------------------------------------
--  1. ARTISTS
-- -----------------------------------------------------------------------------
CREATE TABLE artists (
    id                    TEXT    PRIMARY KEY,
    name                  TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 512),
    sort_name             TEXT    NOT NULL DEFAULT '',
    type                  TEXT    CHECK (type IS NULL OR type IN ('person','group','other')),
    country               TEXT,
    disambiguation        TEXT,
    musicbrainz_artist_id TEXT,
    added_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at            TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT artists_name_unique UNIQUE (name COLLATE NOCASE)
) STRICT;

CREATE UNIQUE INDEX ux_artists_mbid
    ON artists (musicbrainz_artist_id)
    WHERE musicbrainz_artist_id IS NOT NULL;

CREATE INDEX ix_artists_sort_name ON artists (sort_name COLLATE NOCASE);

-- -----------------------------------------------------------------------------
--  2. COVER ART CACHE
--  Primary key is the SHA-256 of `AlbumArtist + Album` (lower-cased, trimmed).
--  This doubles as the Discord RPC artwork pipeline key (FASE 2 / D).
-- -----------------------------------------------------------------------------
CREATE TABLE cover_cache (
    id           TEXT    PRIMARY KEY,
    artist_key   TEXT    NOT NULL,
    album_key    TEXT    NOT NULL,
    hash         TEXT    NOT NULL CHECK (length(hash) = 64),
    provider     TEXT    NOT NULL CHECK (provider IN ('embedded','folder','itunes','musicbrainz','coverartarchive','unknown')),
    remote_url   TEXT,
    local_path   TEXT,
    width        INTEGER CHECK (width  IS NULL OR width  > 0),
    height       INTEGER CHECK (height IS NULL OR height > 0),
    attempts     INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error   TEXT,
    fetched_at   TEXT,
    expires_at   TEXT,
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT cover_cache_hash_unique UNIQUE (hash),
    CONSTRAINT cover_cache_has_asset CHECK (remote_url IS NOT NULL OR local_path IS NOT NULL)
) STRICT;

CREATE INDEX ix_cover_cache_identity ON cover_cache (artist_key, album_key);
CREATE INDEX ix_cover_cache_expiry   ON cover_cache (expires_at)
    WHERE expires_at IS NOT NULL;

-- -----------------------------------------------------------------------------
--  3. ALBUMS
--  `cover_id` points at cover_cache; NULL until artwork is resolved.
-- -----------------------------------------------------------------------------
CREATE TABLE albums (
    id                   TEXT    PRIMARY KEY,
    title                TEXT    NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 1024),
    sort_title           TEXT    NOT NULL DEFAULT '',
    artist_id            TEXT    REFERENCES artists (id) ON DELETE SET NULL,
    album_artist_id      TEXT    REFERENCES artists (id) ON DELETE SET NULL,
    album_artist_name    TEXT    NOT NULL DEFAULT '',
    year                 INTEGER CHECK (year IS NULL OR year BETWEEN 1000 AND 2999),
    original_year        INTEGER CHECK (original_year IS NULL OR original_year BETWEEN 1000 AND 2999),
    release_date         TEXT,
    release_type         TEXT    CHECK (release_type IS NULL OR release_type IN ('album','ep','single','live','compilation','soundtrack','dj-mix','other')),
    is_compilation       INTEGER NOT NULL DEFAULT 0 CHECK (is_compilation IN (0,1)),
    disc_count           INTEGER NOT NULL DEFAULT 1 CHECK (disc_count BETWEEN 1 AND 128),
    track_count          INTEGER NOT NULL DEFAULT 0 CHECK (track_count >= 0),
    duration_ms          INTEGER NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    label                TEXT,
    catalog_number       TEXT,
    barcode              TEXT,
    musicbrainz_release_id       TEXT,
    musicbrainz_release_group_id TEXT,
    cover_id             TEXT    REFERENCES cover_cache (id) ON DELETE SET NULL,
    added_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT albums_identity UNIQUE (title COLLATE NOCASE, album_artist_name COLLATE NOCASE)
) STRICT;

CREATE INDEX ix_albums_artist        ON albums (artist_id);
CREATE INDEX ix_albums_album_artist  ON albums (album_artist_id);
CREATE INDEX ix_albums_year          ON albums (year);
CREATE INDEX ix_albums_sort_title    ON albums (sort_title COLLATE NOCASE);
CREATE INDEX ix_albums_cover         ON albums (cover_id)
    WHERE cover_id IS NOT NULL;
CREATE UNIQUE INDEX ux_albums_mbid   ON albums (musicbrainz_release_id)
    WHERE musicbrainz_release_id IS NOT NULL;

-- -----------------------------------------------------------------------------
--  4. CUE SHEETS
--  One row per physical .cue file. `content` keeps the original text so the
--  file can be re-parsed after a parser upgrade; `sha256` detects edits.
--  Album image files > 1 GiB are the norm: nothing here loads audio, only text.
-- -----------------------------------------------------------------------------
CREATE TABLE cue_sheets (
    id            TEXT    PRIMARY KEY,
    album_id      TEXT    REFERENCES albums (id) ON DELETE SET NULL,
    path          TEXT    NOT NULL CHECK (length(path) > 0),
    album_image   TEXT    NOT NULL CHECK (length(album_image) > 0),
    sha256        TEXT    NOT NULL CHECK (length(sha256) = 64),
    catalog       TEXT,
    performer     TEXT,
    title         TEXT,
    songwriter    TEXT,
    genre         TEXT,
    date          TEXT,
    disc_id       TEXT,
    comment       TEXT,
    track_count   INTEGER NOT NULL DEFAULT 0 CHECK (track_count >= 0),
    total_ms      INTEGER NOT NULL DEFAULT 0 CHECK (total_ms >= 0),
    content       TEXT    NOT NULL,
    added_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT cue_sheets_path_unique UNIQUE (path)
) STRICT;

CREATE INDEX ix_cue_sheets_album ON cue_sheets (album_id);
CREATE INDEX ix_cue_sheets_image ON cue_sheets (album_image);

-- -----------------------------------------------------------------------------
--  5. TRACKS
--  The hot table. It is intentionally wide but every column is displayed
--  somewhere (library grid, inspector, player bar quality badge, DSP chain).
--  `source` distinguishes physical files from CUE-sheet virtual tracks so the
--  scanner can rebuild one album image's virtual tracks without touching
--  regular files.
-- -----------------------------------------------------------------------------
CREATE TABLE tracks (
    id                       TEXT    PRIMARY KEY,
    path                     TEXT    NOT NULL CHECK (length(path) > 0),
    folder                   TEXT    NOT NULL DEFAULT '',
    filename                 TEXT    NOT NULL DEFAULT '',
    source                   TEXT    NOT NULL DEFAULT 'file' CHECK (source IN ('file','cue')),

    -- Core tags (denormalised on purpose, see header) ---------------------
    title                    TEXT    NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 2048),
    title_sort               TEXT    NOT NULL DEFAULT '',
    artist_id                TEXT    REFERENCES artists (id) ON DELETE SET NULL,
    artist_name              TEXT    NOT NULL DEFAULT '',
    album_id                 TEXT    REFERENCES albums (id) ON DELETE SET NULL,
    album_title              TEXT    NOT NULL DEFAULT '',
    album_artist_id          TEXT    REFERENCES artists (id) ON DELETE SET NULL,
    album_artist_name        TEXT    NOT NULL DEFAULT '',
    composer                 TEXT    NOT NULL DEFAULT '',
    genre                    TEXT    NOT NULL DEFAULT '',
    comment                  TEXT    NOT NULL DEFAULT '',
    year                     INTEGER CHECK (year IS NULL OR year BETWEEN 1000 AND 2999),
    track_no                 INTEGER CHECK (track_no  IS NULL OR track_no  BETWEEN 0 AND 9999),
    disc_no                  INTEGER CHECK (disc_no   IS NULL OR disc_no   BETWEEN 0 AND 999),
    track_total              INTEGER CHECK (track_total IS NULL OR track_total BETWEEN 0 AND 9999),
    disc_total               INTEGER CHECK (disc_total  IS NULL OR disc_total  BETWEEN 0 AND 999),

    -- Technical properties -------------------------------------------------
    format                   TEXT    NOT NULL CHECK (length(format) BETWEEN 1 AND 32),
    container                TEXT,
    codec_profile            TEXT,
    bit_depth                INTEGER CHECK (bit_depth IS NULL OR bit_depth BETWEEN 1 AND 64),
    sample_rate              INTEGER CHECK (sample_rate IS NULL OR sample_rate BETWEEN 8000 AND 1536000),
    channels                 INTEGER CHECK (channels IS NULL OR channels BETWEEN 1 AND 16),
    bitrate_kbps             INTEGER CHECK (bitrate_kbps IS NULL OR bitrate_kbps >= 0),
    dsd_rate                 INTEGER CHECK (dsd_rate IS NULL OR dsd_rate IN (2822400, 5644800, 11289600, 22579200)),
    duration_ms              INTEGER NOT NULL CHECK (duration_ms >= 0),
    file_size                INTEGER NOT NULL DEFAULT 0 CHECK (file_size >= 0),

    -- Integrated loudness / ReplayGain v2 (EBU R128) -----------------------
    replaygain_track_gain_db REAL    CHECK (replaygain_track_gain_db IS NULL OR replaygain_track_gain_db BETWEEN -60.0 AND 30.0),
    replaygain_track_peak    REAL    CHECK (replaygain_track_peak    IS NULL OR replaygain_track_peak    BETWEEN 0.0 AND 1.0),
    replaygain_album_gain_db REAL    CHECK (replaygain_album_gain_db IS NULL OR replaygain_album_gain_db BETWEEN -60.0 AND 30.0),
    replaygain_album_peak    REAL    CHECK (replaygain_album_peak    IS NULL OR replaygain_album_peak    BETWEEN 0.0 AND 1.0),

    -- External identifiers --------------------------------------------------
    musicbrainz_track_id          TEXT,
    musicbrainz_release_track_id  TEXT,
    isrc                          TEXT,

    -- Playback statistics ---------------------------------------------------
    rating                   INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
    play_count               INTEGER NOT NULL DEFAULT 0 CHECK (play_count >= 0),
    skip_count               INTEGER NOT NULL DEFAULT 0 CHECK (skip_count >= 0),
    last_played_at           TEXT,
    last_skipped_at          TEXT,
    added_at                 TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    indexed_at               TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),

    -- Integrity / lifecycle ---------------------------------------------------
    file_mtime               INTEGER NOT NULL DEFAULT 0,
    cue_sheet_id             TEXT    REFERENCES cue_sheets (id) ON DELETE CASCADE,
    cue_start_ms             INTEGER CHECK (cue_start_ms IS NULL OR cue_start_ms >= 0),
    cue_end_ms               INTEGER CHECK (cue_end_ms IS NULL OR cue_end_ms >= 0),
    integrity                TEXT    NOT NULL DEFAULT 'ok' CHECK (integrity IN ('ok','suspect','corrupt')),
    missing                  INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0,1)),
    has_embedded_art         INTEGER NOT NULL DEFAULT 0 CHECK (has_embedded_art IN (0,1)),
    extra_tags               TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(extra_tags)),

    CONSTRAINT tracks_cue_range CHECK (
        cue_start_ms IS NULL OR cue_end_ms IS NULL OR cue_end_ms > cue_start_ms
    ),
    CONSTRAINT tracks_cue_consistency CHECK (
        (source = 'file' AND cue_sheet_id IS NULL) OR
        (source = 'cue'  AND cue_sheet_id IS NOT NULL)
    )
) STRICT;

-- Identity: case- and separator-insensitive unique path. CUE virtual tracks
-- deliberately share the album-image path, so the offset disambiguates them.
CREATE UNIQUE INDEX ux_tracks_identity ON tracks (
    replace(replace(path, '\', '/'), '//', '/') COLLATE NOCASE,
    source,
    COALESCE(cue_start_ms, -1)
);

CREATE INDEX ix_tracks_album          ON tracks (album_id, disc_no, track_no, cue_start_ms);
CREATE INDEX ix_tracks_artist         ON tracks (artist_id);
CREATE INDEX ix_tracks_album_artist   ON tracks (album_artist_id);
CREATE INDEX ix_tracks_genre          ON tracks (genre COLLATE NOCASE);
CREATE INDEX ix_tracks_year           ON tracks (year);
CREATE INDEX ix_tracks_format         ON tracks (format COLLATE NOCASE);
CREATE INDEX ix_tracks_quality        ON tracks (sample_rate, bit_depth);
CREATE INDEX ix_tracks_bitrate        ON tracks (bitrate_kbps);
CREATE INDEX ix_tracks_added          ON tracks (added_at);
CREATE INDEX ix_tracks_last_played    ON tracks (last_played_at)
    WHERE last_played_at IS NOT NULL;
CREATE INDEX ix_tracks_rating         ON tracks (rating)
    WHERE rating > 0;
CREATE INDEX ix_tracks_missing        ON tracks (missing)
    WHERE missing = 1;
CREATE INDEX ix_tracks_cue            ON tracks (cue_sheet_id)
    WHERE cue_sheet_id IS NOT NULL;
CREATE INDEX ix_tracks_title_sort     ON tracks (title_sort COLLATE NOCASE);
CREATE INDEX ix_tracks_duration       ON tracks (duration_ms);
CREATE UNIQUE INDEX ux_tracks_mbid    ON tracks (musicbrainz_track_id)
    WHERE musicbrainz_track_id IS NOT NULL;

-- -----------------------------------------------------------------------------
--  6. PLAYLISTS
-- -----------------------------------------------------------------------------
CREATE TABLE playlists (
    id          TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 512),
    description TEXT,
    kind        TEXT    NOT NULL DEFAULT 'static' CHECK (kind IN ('static','smart')),
    cover_id    TEXT    REFERENCES cover_cache (id) ON DELETE SET NULL,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT playlists_name_unique UNIQUE (name COLLATE NOCASE)
) STRICT;

CREATE TABLE playlist_items (
    id          TEXT    PRIMARY KEY,
    playlist_id TEXT    NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    track_id    TEXT    NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    position    REAL    NOT NULL CHECK (position >= 0),
    added_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX ix_playlist_items_order ON playlist_items (playlist_id, position, id);
CREATE INDEX ix_playlist_items_track ON playlist_items (track_id);

CREATE TABLE smart_playlists (
    playlist_id  TEXT    PRIMARY KEY REFERENCES playlists (id) ON DELETE CASCADE,
    query        TEXT    NOT NULL CHECK (length(trim(query)) > 0),
    sort_clause  TEXT    NOT NULL DEFAULT 'title COLLATE NOCASE ASC',
    limit_count  INTEGER CHECK (limit_count IS NULL OR limit_count > 0),
    updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- -----------------------------------------------------------------------------
--  7. LYRICS CACHE
--  `synced_lrc` stores the raw LRC text (with `[mm:ss.xx]` stamps) so the
--  frontend can re-parse karaoke timing without another IPC round-trip.
-- -----------------------------------------------------------------------------
CREATE TABLE lyrics_cache (
    id          TEXT    PRIMARY KEY,
    track_id    TEXT    NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    provider    TEXT    NOT NULL CHECK (provider IN ('embedded','sidecar','lrclib','manual','plugin')),
    source_url  TEXT,
    language    TEXT,
    plain_text  TEXT,
    synced_lrc  TEXT,
    has_synced  INTEGER NOT NULL DEFAULT 0 CHECK (has_synced IN (0,1)),
    fetched_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT lyrics_cache_track_unique UNIQUE (track_id),
    CONSTRAINT lyrics_cache_has_body CHECK (plain_text IS NOT NULL OR synced_lrc IS NOT NULL)
) STRICT;

CREATE INDEX ix_lyrics_cache_provider ON lyrics_cache (provider);

-- -----------------------------------------------------------------------------
--  8. PLAY HISTORY
--  Append-only; the retention job prunes rows older than the user setting.
-- -----------------------------------------------------------------------------
CREATE TABLE play_history (
    id            TEXT    PRIMARY KEY,
    track_id      TEXT    NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    played_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ms_played     INTEGER NOT NULL DEFAULT 0 CHECK (ms_played >= 0),
    completed     INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0,1)),
    skipped       INTEGER NOT NULL DEFAULT 0 CHECK (skipped IN (0,1)),
    source        TEXT    NOT NULL DEFAULT 'queue' CHECK (source IN ('queue','album','playlist','smart','search','radio')),
    output_device TEXT,
    sample_rate   INTEGER,
    bit_depth     INTEGER
) STRICT;

CREATE INDEX ix_play_history_track  ON play_history (track_id, played_at DESC);
CREATE INDEX ix_play_history_recent ON play_history (played_at DESC);

-- -----------------------------------------------------------------------------
--  9. KEY/VALUE SETTINGS (user preferences, DSP chain state, RPC client id...)
-- -----------------------------------------------------------------------------
CREATE TABLE settings (
    key        TEXT    PRIMARY KEY CHECK (length(trim(key)) BETWEEN 1 AND 128),
    value      TEXT    NOT NULL CHECK (json_valid(value)),
    updated_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- -----------------------------------------------------------------------------
-- 10. LIBRARY ROOTS & SCAN BOOKKEEPING
-- -----------------------------------------------------------------------------
CREATE TABLE library_roots (
    id            TEXT    PRIMARY KEY,
    path          TEXT    NOT NULL CHECK (length(path) > 0),
    name          TEXT    NOT NULL DEFAULT '',
    enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    recursive     INTEGER NOT NULL DEFAULT 1 CHECK (recursive IN (0,1)),
    watch         INTEGER NOT NULL DEFAULT 1 CHECK (watch IN (0,1)),
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT library_roots_path_unique UNIQUE (path)
) STRICT;

CREATE TABLE scan_runs (
    id             TEXT    PRIMARY KEY,
    root_id        TEXT    REFERENCES library_roots (id) ON DELETE SET NULL,
    status         TEXT    NOT NULL DEFAULT 'running' CHECK (status IN ('running','completed','failed','cancelled')),
    trigger        TEXT    NOT NULL DEFAULT 'manual' CHECK (trigger IN ('startup','manual','watch','scheduled','cli')),
    files_seen     INTEGER NOT NULL DEFAULT 0 CHECK (files_seen >= 0),
    files_indexed  INTEGER NOT NULL DEFAULT 0 CHECK (files_indexed >= 0),
    files_skipped  INTEGER NOT NULL DEFAULT 0 CHECK (files_skipped >= 0),
    files_failed   INTEGER NOT NULL DEFAULT 0 CHECK (files_failed >= 0),
    tracks_removed INTEGER NOT NULL DEFAULT 0 CHECK (tracks_removed >= 0),
    bytes_read     INTEGER NOT NULL DEFAULT 0 CHECK (bytes_read >= 0),
    started_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at    TEXT,
    duration_ms    INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    error_message  TEXT
) STRICT;

CREATE INDEX ix_scan_runs_started ON scan_runs (started_at DESC);
CREATE INDEX ix_scan_runs_status  ON scan_runs (status)
    WHERE status = 'running';

CREATE TABLE scan_errors (
    id          TEXT    PRIMARY KEY,
    run_id      TEXT    NOT NULL REFERENCES scan_runs (id) ON DELETE CASCADE,
    path        TEXT    NOT NULL,
    error_code  TEXT    NOT NULL,
    message     TEXT    NOT NULL,
    occurred_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX ix_scan_errors_run  ON scan_errors (run_id, occurred_at);
CREATE INDEX ix_scan_errors_code ON scan_errors (error_code);

-- -----------------------------------------------------------------------------
-- 11. DSP PRESETS & SAVED LAYOUTS
-- -----------------------------------------------------------------------------
CREATE TABLE eq_presets (
    id           TEXT    PRIMARY KEY,
    name         TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 128),
    kind         TEXT    NOT NULL DEFAULT 'parametric' CHECK (kind IN ('parametric','graphic')),
    bands        TEXT    NOT NULL CHECK (json_valid(bands)),
    preamp_db    REAL    NOT NULL DEFAULT 0.0 CHECK (preamp_db BETWEEN -24.0 AND 24.0),
    is_builtin   INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0,1)),
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT eq_presets_name_unique UNIQUE (name COLLATE NOCASE)
) STRICT;

CREATE TABLE layouts (
    id         TEXT    PRIMARY KEY,
    name       TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 128),
    config     TEXT    NOT NULL CHECK (json_valid(config)),
    is_active  INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0,1)),
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT layouts_name_unique UNIQUE (name COLLATE NOCASE)
) STRICT;

CREATE UNIQUE INDEX ux_layouts_active ON layouts (is_active)
    WHERE is_active = 1;

-- -----------------------------------------------------------------------------
-- 12. FTS5 — instant search
--  `unicode61` tokeniser with diacritic folding so "Beyonce" finds "Beyoncé".
--  External content table: no data duplication; rowids are `tracks.id` mapped
--  through `tracks.rowid` (TEXT primary keys still get an implicit rowid).
-- -----------------------------------------------------------------------------
CREATE VIRTUAL TABLE tracks_fts USING fts5(
    title,
    artist_name,
    album_title,
    album_artist_name,
    genre,
    comment,
    composer,
    content = 'tracks',
    content_rowid = 'rowid',
    tokenize = "unicode61 remove_diacritics 2"
);

CREATE TRIGGER trg_tracks_fts_insert AFTER INSERT ON tracks BEGIN
    INSERT INTO tracks_fts (
        rowid, title, artist_name, album_title, album_artist_name, genre, comment, composer
    ) VALUES (
        new.rowid, new.title, new.artist_name, new.album_title,
        new.album_artist_name, new.genre, new.comment, new.composer
    );
END;

CREATE TRIGGER trg_tracks_fts_delete AFTER DELETE ON tracks BEGIN
    INSERT INTO tracks_fts (
        tracks_fts, rowid, title, artist_name, album_title,
        album_artist_name, genre, comment, composer
    ) VALUES (
        'delete', old.rowid, old.title, old.artist_name, old.album_title,
        old.album_artist_name, old.genre, old.comment, old.composer
    );
END;

CREATE TRIGGER trg_tracks_fts_update AFTER UPDATE OF
    title, artist_name, album_title, album_artist_name, genre, comment, composer
ON tracks BEGIN
    INSERT INTO tracks_fts (
        tracks_fts, rowid, title, artist_name, album_title,
        album_artist_name, genre, comment, composer
    ) VALUES (
        'delete', old.rowid, old.title, old.artist_name, old.album_title,
        old.album_artist_name, old.genre, old.comment, old.composer
    );
    INSERT INTO tracks_fts (
        rowid, title, artist_name, album_title, album_artist_name, genre, comment, composer
    ) VALUES (
        new.rowid, new.title, new.artist_name, new.album_title,
        new.album_artist_name, new.genre, new.comment, new.composer
    );
END;

-- Artist rename cascade: keep denormalised track rows (and therefore FTS) hot.
CREATE TRIGGER trg_artists_rename
AFTER UPDATE OF name ON artists
WHEN new.name IS NOT old.name
BEGIN
    UPDATE tracks SET artist_name = new.name WHERE artist_id = old.id;
    UPDATE tracks SET album_artist_name = new.name WHERE album_artist_id = old.id;
    UPDATE albums SET album_artist_name = new.name WHERE album_artist_id = old.id;
END;

-- Album rename cascade.
CREATE TRIGGER trg_albums_rename
AFTER UPDATE OF title ON albums
WHEN new.title IS NOT old.title
BEGIN
    UPDATE tracks SET album_title = new.title WHERE album_id = old.id;
END;

-- -----------------------------------------------------------------------------
-- 13. VIEWS — read models used by the frontend over IPC.
-- -----------------------------------------------------------------------------
CREATE VIEW v_track_details AS
SELECT
    t.id,
    t.path,
    t.folder,
    t.filename,
    t.source,
    t.title,
    t.title_sort,
    t.artist_id,
    t.artist_name,
    t.album_id,
    t.album_title,
    t.album_artist_id,
    t.album_artist_name,
    t.composer,
    t.genre,
    t.comment,
    t.year,
    t.track_no,
    t.disc_no,
    t.track_total,
    t.disc_total,
    t.format,
    t.container,
    t.codec_profile,
    t.bit_depth,
    t.sample_rate,
    t.channels,
    t.bitrate_kbps,
    t.dsd_rate,
    t.duration_ms,
    t.file_size,
    t.replaygain_track_gain_db,
    t.replaygain_track_peak,
    t.replaygain_album_gain_db,
    t.replaygain_album_peak,
    t.rating,
    t.play_count,
    t.skip_count,
    t.last_played_at,
    t.added_at,
    t.modified_at,
    t.file_mtime,
    t.cue_sheet_id,
    t.cue_start_ms,
    t.cue_end_ms,
    t.integrity,
    t.missing,
    t.has_embedded_art,
    t.extra_tags,
    al.year            AS album_year,
    al.cover_id        AS album_cover_id,
    al.release_type    AS album_release_type,
    al.disc_count      AS album_disc_count,
    al.track_count     AS album_track_count,
    cc.remote_url      AS cover_remote_url,
    cc.local_path      AS cover_local_path,
    lc.has_synced      AS has_synced_lyrics
FROM tracks AS t
LEFT JOIN albums      AS al ON al.id = t.album_id
LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id
LEFT JOIN lyrics_cache AS lc ON lc.track_id = t.id;

CREATE VIEW v_album_summary AS
SELECT
    al.id,
    al.title,
    al.sort_title,
    al.year,
    al.album_artist_name,
    al.is_compilation,
    al.disc_count,
    al.track_count,
    al.duration_ms,
    al.release_type,
    al.cover_id,
    cc.remote_url AS cover_remote_url,
    cc.local_path AS cover_local_path,
    COUNT(t.id)                                              AS available_tracks,
    COALESCE(SUM(t.duration_ms), 0)                          AS available_duration_ms,
    MIN(t.sample_rate)                                       AS min_sample_rate,
    MAX(t.sample_rate)                                       AS max_sample_rate,
    MIN(t.bit_depth)                                         AS min_bit_depth,
    MAX(t.bit_depth)                                         AS max_bit_depth,
    MIN(t.format)                                            AS format
FROM albums AS al
LEFT JOIN tracks      AS t  ON t.album_id = al.id AND t.missing = 0 AND t.source = 'file'
LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id
GROUP BY al.id;

CREATE VIEW v_artist_summary AS
SELECT
    ar.id,
    ar.name,
    ar.sort_name,
    ar.musicbrainz_artist_id,
    (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id)              AS album_count,
    (SELECT COUNT(*) FROM tracks t WHERE t.artist_id = ar.id AND t.missing = 0) AS track_count,
    (SELECT COALESCE(SUM(t.duration_ms), 0) FROM tracks t
      WHERE t.artist_id = ar.id AND t.missing = 0)                            AS total_duration_ms
FROM artists AS ar;

CREATE VIEW v_playlist_summary AS
SELECT
    p.id,
    p.name,
    p.description,
    p.kind,
    p.created_at,
    p.updated_at,
    COUNT(pi.id)                   AS track_count,
    COALESCE(SUM(t.duration_ms), 0) AS duration_ms,
    p.cover_id,
    cc.remote_url AS cover_remote_url,
    cc.local_path AS cover_local_path
FROM playlists AS p
LEFT JOIN playlist_items AS pi ON pi.playlist_id = p.id
LEFT JOIN tracks AS t          ON t.id = pi.track_id AND t.missing = 0
LEFT JOIN cover_cache AS cc    ON cc.id = p.cover_id
GROUP BY p.id;

CREATE VIEW v_library_stats AS
SELECT
    COUNT(*)                                          AS track_count,
    COALESCE(SUM(duration_ms), 0)                     AS total_duration_ms,
    COALESCE(SUM(file_size), 0)                       AS total_bytes,
    COUNT(DISTINCT album_id)                          AS album_count,
    COUNT(DISTINCT artist_id)                         AS artist_count,
    COALESCE(SUM(CASE WHEN missing = 1 THEN 1 ELSE 0 END), 0)       AS missing_count,
    COALESCE(SUM(CASE WHEN integrity <> 'ok' THEN 1 ELSE 0 END), 0) AS unhealthy_count,
    COALESCE(SUM(CASE WHEN source = 'cue' THEN 1 ELSE 0 END), 0)    AS cue_track_count,
    MAX(sample_rate)                                  AS max_sample_rate,
    MAX(bit_depth)                                    AS max_bit_depth
FROM tracks;
