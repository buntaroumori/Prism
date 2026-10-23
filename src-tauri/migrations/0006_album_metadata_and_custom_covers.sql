-- 0006 — full album dates, grid thumbnails and custom playlist artwork
--
-- `release_date` was already indexed with each album. Exposing it in the read
-- model avoids deriving a second date from track metadata. A lightweight JPEG
-- derivative lets grids reuse the decoded artwork without decoding the 1000 px
-- master for every tile. Custom playlist artwork stays local and is deliberately
-- separate from the album cover used by Discord Rich Presence.

ALTER TABLE cover_cache ADD COLUMN thumbnail_path TEXT;
ALTER TABLE playlists ADD COLUMN custom_cover_path TEXT;

DROP VIEW IF EXISTS v_album_summary;

CREATE VIEW v_album_summary AS
SELECT
    al.id,
    al.title,
    al.sort_title,
    al.year,
    al.release_date,
    al.album_artist_name,
    al.is_compilation,
    al.disc_count,
    al.track_count,
    al.duration_ms,
    al.release_type,
    al.cover_id,
    cc.remote_url AS cover_remote_url,
    cc.local_path AS cover_local_path,
    cc.thumbnail_path AS cover_thumbnail_path,
    COUNT(t.id)                                              AS available_tracks,
    COALESCE(SUM(t.duration_ms), 0)                          AS available_duration_ms,
    MIN(t.sample_rate)                                       AS min_sample_rate,
    MAX(t.sample_rate)                                       AS max_sample_rate,
    MIN(t.bit_depth)                                         AS min_bit_depth,
    MAX(t.bit_depth)                                         AS max_bit_depth,
    MIN(t.format)                                            AS format,
    MIN(NULLIF(TRIM(t.genre), ''))                            AS genre,
    MAX(t.added_at)                                          AS added_at,
    MAX(t.last_played_at)                                    AS last_played_at,
    COALESCE(SUM(t.play_count), 0)                           AS play_count,
    al.album_artist_id                                       AS artist_id
FROM albums AS al
LEFT JOIN tracks      AS t  ON t.album_id = al.id AND t.missing = 0 AND t.source = 'file'
LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id
GROUP BY al.id;

DROP VIEW IF EXISTS v_playlist_summary;

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
    CASE WHEN p.custom_cover_path IS NULL THEN cc.remote_url ELSE NULL END
        AS cover_remote_url,
    COALESCE(p.custom_cover_path, cc.local_path) AS cover_local_path
FROM playlists AS p
LEFT JOIN playlist_items AS pi ON pi.playlist_id = p.id
LEFT JOIN tracks AS t          ON t.id = pi.track_id AND t.missing = 0
LEFT JOIN cover_cache AS cc    ON cc.id = p.cover_id
GROUP BY p.id;
