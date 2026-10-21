-- 0003 — `v_album_summary.last_played_at` and `play_count`
--
-- The Home screen gained two rails that are stated in terms of listening, not
-- of filing: "Recently played" and "Most played". Both are album-level
-- questions ("when did I last hear this record?", "which record do I return
-- to?"), so the read model needs the aggregate. `play_history` already records
-- every start, and `tracks.play_count` / `tracks.last_played_at` are the
-- denormalised counters it maintains.
--
-- An album is as recent as its most recent track, and as played as the sum of
-- its tracks. Views cannot be altered in place, so the read model is recreated;
-- every previous column keeps its name, type and position, and the two new
-- columns are appended so positional consumers stay compatible.

DROP VIEW IF EXISTS v_album_summary;

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
    MIN(t.format)                                            AS format,
    MAX(t.added_at)                                          AS added_at,
    MAX(t.last_played_at)                                    AS last_played_at,
    COALESCE(SUM(t.play_count), 0)                           AS play_count
FROM albums AS al
LEFT JOIN tracks      AS t  ON t.album_id = al.id AND t.missing = 0 AND t.source = 'file'
LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id
GROUP BY al.id;

-- The two rails sort on these aggregates; without an index SQLite would scan
-- `tracks` for every album it orders.
CREATE INDEX IF NOT EXISTS ix_tracks_album_played
    ON tracks (album_id, last_played_at DESC)
    WHERE missing = 0 AND source = 'file';
