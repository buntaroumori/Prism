-- 0004 — `v_album_summary.artist_id`
--
-- Every artist name in the interface is a link. On the album mosaic and the
-- album page the name comes from the read model, which until now carried the
-- album artist's *name* but not its identifier — so the link had nowhere to go
-- and the UI would have had to search by name, which breaks on duplicates
-- ("Nirvana" the band and "Nirvana" the tribute act).
--
-- The view is recreated with `al.album_artist_id` appended at the end: every
-- existing column keeps its name, type and position.

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
    COALESCE(SUM(t.play_count), 0)                           AS play_count,
    al.album_artist_id                                       AS artist_id
FROM albums AS al
LEFT JOIN tracks      AS t  ON t.album_id = al.id AND t.missing = 0 AND t.source = 'file'
LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id
GROUP BY al.id;
