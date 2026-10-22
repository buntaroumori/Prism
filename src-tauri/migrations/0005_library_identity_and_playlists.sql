-- A MusicBrainz recording id identifies the recording, not the physical file.
-- The same recording legitimately occurs in two pressings, in a backup root,
-- or in two files of a multi-disc edition. File identity is already enforced
-- by ux_tracks_identity (path + source + CUE offset); making the external id
-- unique as well aborted the whole write batch with SQLite error 2067.
DROP INDEX IF EXISTS ux_tracks_mbid;
CREATE INDEX IF NOT EXISTS ix_tracks_mbid ON tracks (musicbrainz_track_id)
    WHERE musicbrainz_track_id IS NOT NULL;

-- Adding a song twice must not duplicate it or fail the whole insert. Be
-- conservative with databases written by early development builds: keep the
-- first position of any pre-existing duplicate before creating the index.
DELETE FROM playlist_items
WHERE rowid NOT IN (
    SELECT MIN(rowid) FROM playlist_items GROUP BY playlist_id, track_id
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_playlist_items_track
    ON playlist_items (playlist_id, track_id);
