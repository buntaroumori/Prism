import { useCallback, useEffect, useRef, useState } from "react";

import { toUiError, type UiError } from "@/lib/errors";
import { lyricsApi } from "@/lib/ipc";
import type { LyricsDocument } from "@/types/ipc";

export interface LyricsState {
  document: LyricsDocument | null;
  loading: boolean;
  error: UiError | null;
  /** Re-read the file/cache; used by the panel's refresh button. */
  refresh: () => void;
}

/**
 * Lyrics for one track.
 *
 * The document is fetched once per track id and kept while the panel is open. A
 * missing document is a normal answer (`provider === "none"`), not an error: the
 * panel renders its own empty state for it. Refreshing bypasses the backend
 * cache so a `.lrc` added while the track plays shows up immediately.
 */
export function useLyrics(trackId: string | null): LyricsState {
  const [document, setDocument] = useState<LyricsDocument | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [nonce, setNonce] = useState(0);
  const requestRef = useRef(0);

  const refresh = useCallback(() => {
    setNonce((value) => value + 1);
  }, []);

  useEffect(() => {
    if (trackId === null) {
      setDocument(null);
      setError(null);
      setLoading(false);
      return;
    }

    requestRef.current += 1;
    const request = requestRef.current;
    let active = true;
    setLoading(true);

    void lyricsApi
      .get(trackId, nonce > 0)
      .then((next) => {
        if (!active || request !== requestRef.current) {
          return;
        }
        setDocument(next);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (!active || request !== requestRef.current) {
          return;
        }
        // A read failure (permissions, a corrupt tag) must not look like
        // "this track has no lyrics": keep the previous document out and show
        // the reason in the panel.
        setDocument(null);
        setError(toUiError(cause));
      })
      .finally(() => {
        if (active && request === requestRef.current) {
          setLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [trackId, nonce]);

  return { document, loading, error, refresh };
}

/**
 * Index of the line that is playing at `positionMs`.
 *
 * Binary search over the sorted timestamps: a 300-line lyric sheet changes line a
 * few times a minute, but the lookup runs on every playhead tick, so it must not
 * scan the array.
 *
 * Returns `-1` before the first timestamp (an intro), which the panel renders as
 * "no active line" instead of highlighting line 0 early.
 */
export function activeLineIndex(lines: readonly { timeMs: number }[], positionMs: number): number {
  const first = lines[0];
  if (first === undefined || positionMs < first.timeMs) {
    return -1;
  }
  let low = 0;
  let high = lines.length - 1;
  let answer = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    const line = lines[middle];
    if (line !== undefined && line.timeMs <= positionMs) {
      answer = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return answer;
}
