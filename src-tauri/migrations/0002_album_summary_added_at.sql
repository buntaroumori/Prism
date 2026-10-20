-- 0002 — `v_album_summary.added_at`
--
-- The Home view asks for the most recently added albums (`TrackSort::Added`),
-- and the query orders the album read model by `added_at`. The column only
-- existed on `tracks`, so every fresh launch failed with
-- `no such column: added_at` while the rest of the shell rendered around the
-- error. An album's "added" timestamp is the newest of its tracks: that is the
-- moment it became playable.
--
-- Views are not covered by `ALTER TABLE`, so the read model is recreated. The
-- migration is additive — every previous column keeps its name, type and order,
-- and `added_at` is appended so `SELECT *` consumers stay compatible.

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
    MAX(t.added_at)                                          AS added_at
FROM albums AS al
LEFT JOIN tracks      AS t  ON t.album_id = al.id AND t.missing = 0 AND t.source = 'file'
LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id
GROUP BY al.id;

-- Sorting the album grid by recency is a first-class view of the library, not
-- an occasional query.
CREATE INDEX IF NOT EXISTS ix_tracks_album_added
    ON tracks (album_id, added_at DESC)
    WHERE missing = 0 AND source = 'file';
