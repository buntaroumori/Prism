import { useEffect, useRef, useState } from "react";

import { libraryApi } from "@/lib/ipc";
import type { CoverPalette } from "@/types/ipc";

/** In-process palette memo: navigating back to an album never re-asks Rust. */
const MEMO = new Map<string, CoverPalette>();

/**
 * Palette for a cover key.
 *
 * The previous palette stays on screen while the next one loads, so switching
 * tracks blends tint instead of flashing the neutral shell. Requests are
 * generation-guarded: a fast skip through five tracks commits only the last
 * answer, never a stale one.
 */
export function useArtworkPalette(key: string | null): CoverPalette | null {
  const [palette, setPalette] = useState<CoverPalette | null>(() =>
    key !== null ? (MEMO.get(key) ?? null) : null,
  );
  const generation = useRef(0);

  useEffect(() => {
    if (key === null) {
      return;
    }
    const memoised = MEMO.get(key);
    if (memoised !== undefined) {
      setPalette(memoised);
      return;
    }

    const token = generation.current + 1;
    generation.current = token;

    void libraryApi
      .coverPalette(key)
      .then((next) => {
        MEMO.set(key, next);
        if (generation.current === token) {
          setPalette(next);
        }
      })
      .catch((cause: unknown) => {
        // A palette is decorative: a failure must never surface as an error
        // state on top of working playback.
        console.warn("Cover palette unavailable", cause);
      });
  }, [key]);

  return palette;
}
