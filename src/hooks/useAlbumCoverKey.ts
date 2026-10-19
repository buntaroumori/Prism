import { useEffect, useState } from "react";

import { libraryApi } from "@/lib/ipc";

/** Album identity → cache key, so returning to an album never re-asks Rust. */
const KEY_BY_ALBUM = new Map<string, string | null>();

/**
 * Cover-cache key for the album currently loaded in the engine.
 *
 * Keyed by album name rather than track id on purpose: a queue of tracks from
 * one record resolves the cover once, and skipping within the album does not
 * invalidate the ambient palette. Albums with no resolvable artwork remember the
 * miss so the same lookup is not repeated on every skip.
 */
export function useAlbumCoverKey(album: string | null, trackId: string | null): string | null {
  const [key, setKey] = useState<string | null>(() =>
    album === null ? null : (KEY_BY_ALBUM.get(album) ?? null),
  );

  useEffect(() => {
    if (album === null || trackId === null) {
      return;
    }
    const cached = KEY_BY_ALBUM.get(album);
    if (cached !== undefined) {
      setKey(cached);
      return;
    }

    let active = true;
    void libraryApi
      .trackCover(trackId)
      .then((asset) => {
        const resolved = asset.provider === "unknown" ? null : asset.key;
        KEY_BY_ALBUM.set(album, resolved);
        if (active) {
          setKey(resolved);
        }
      })
      .catch(() => {
        if (active) {
          setKey(null);
        }
      });

    return () => {
      active = false;
    };
  }, [album, trackId]);

  return key;
}
