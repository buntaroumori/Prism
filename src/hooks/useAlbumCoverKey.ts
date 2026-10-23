import { useEffect, useState } from "react";

import { cachedTrackCover, trackCover } from "@/lib/trackCover";

/**
 * Cover-cache key for the album loaded in the engine. Artist and album identity
 * prevents equal titles from different artists sharing a palette, while the
 * shared cover cache avoids another lookup for each song on the same record.
 */
export function useAlbumCoverKey(
  album: string | null,
  trackId: string | null,
  artist: string | null,
): string | null {
  const cached =
    album !== null && trackId !== null
      ? cachedTrackCover(trackId, artist ?? undefined, album)
      : null;
  const [key, setKey] = useState<string | null>(() =>
    cached === null || cached.provider === "unknown" ? null : cached.key,
  );

  useEffect(() => {
    if (album === null || trackId === null) {
      setKey(null);
      return;
    }
    const cachedAsset = cachedTrackCover(trackId, artist ?? undefined, album);
    if (cachedAsset !== null) {
      setKey(cachedAsset.provider === "unknown" ? null : cachedAsset.key);
      return;
    }

    let active = true;
    setKey(null);
    void trackCover(trackId, artist ?? undefined, album)
      .then((asset) => {
        if (active) setKey(asset.provider === "unknown" ? null : asset.key);
      })
      .catch(() => {
        if (active) setKey(null);
      });
    return () => {
      active = false;
    };
  }, [album, trackId, artist]);

  return key;
}
