-- Prism 1.1: editable playlist genre metadata.
-- Playlist titles and descriptions already existed; genre is optional and has
-- no relationship to album genres, so a playlist can never enter Library.

ALTER TABLE playlists ADD COLUMN genre TEXT;

DROP VIEW IF EXISTS v_playlist_summary;

CREATE VIEW v_playlist_summary AS
SELECT
    p.id,
    p.name,
    p.description,
    p.genre,
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
