import { libraryApi } from "@/lib/ipc";
import type { CoverAsset } from "@/types/ipc";

interface CachedCover {
  asset: CoverAsset;
  /** Null means a successful row remains valid for this application run. */
  expiresAt: number | null;
}

const MISSING_TTL_MS = 5 * 60 * 1000;
const BY_TRACK = new Map<string, CachedCover>();
const BY_ALBUM = new Map<string, CachedCover>();
const INFLIGHT = new Map<string, Promise<CoverAsset>>();

function albumKey(artist: string | undefined, album: string | undefined): string | null {
  if (album === undefined || album.trim().length === 0) return null;
  return `${artist?.trim().toLowerCase() ?? ""}\u001f${album.trim().toLowerCase()}`;
}

function read(cache: Map<string, CachedCover>, key: string): CoverAsset | null {
  const entry = cache.get(key);
  if (entry === undefined) return null;
  if (entry.expiresAt !== null && entry.expiresAt <= Date.now()) {
    cache.delete(key);
    return null;
  }
  return entry.asset;
}

/** Synchronous cache read for view initialization without an unnecessary IPC round-trip. */
export function cachedTrackCover(
  trackId: string,
  artist?: string,
  album?: string,
): CoverAsset | null {
  const direct = read(BY_TRACK, trackId);
  if (direct !== null) return direct;
  const identity = albumKey(artist, album);
  return identity === null ? null : read(BY_ALBUM, identity);
}

/**
 * Resolve one track's album cover, sharing both completed and in-flight work.
 * Successful assets live for the run; misses expire so newly added artwork can
 * be discovered without making every surface repeat the same failed request.
 */
export function trackCover(trackId: string, artist?: string, album?: string): Promise<CoverAsset> {
  const cached = cachedTrackCover(trackId, artist, album);
  if (cached !== null) return Promise.resolve(cached);

  const identity = albumKey(artist, album);
  const requestKey = identity ?? `track:${trackId}`;
  const pending = INFLIGHT.get(requestKey);
  if (pending !== undefined) return pending;

  const request = libraryApi
    .trackCover(trackId)
    .then((asset) => {
      const entry: CachedCover = {
        asset,
        expiresAt: asset.provider === "unknown" ? Date.now() + MISSING_TTL_MS : null,
      };
      BY_TRACK.set(trackId, entry);
      if (identity !== null) BY_ALBUM.set(identity, entry);
      return asset;
    })
    .finally(() => {
      INFLIGHT.delete(requestKey);
    });
  INFLIGHT.set(requestKey, request);
  return request;
}
