import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useState, type JSX } from "react";

import { CoverArt } from "@/components/ui/CoverArt";
import { trackCover } from "@/lib/trackCover";
import type { CoverAsset } from "@/types/ipc";

interface ArtworkModuleProps {
  trackId: string | null;
  album: string;
  artist: string;
  title: string;
}

/**
 * Album artwork, the loudest element of the surface.
 *
 * The cover is crossfaded on its content hash, so navigating between two tracks
 * of the same record does not flash the same image, and switching records blurs
 * one into the other in 450 ms — the only long transition in the app.
 */
export function ArtworkModule({ trackId, album, artist, title }: ArtworkModuleProps): JSX.Element {
  const [asset, setAsset] = useState<CoverAsset | null>(null);

  useEffect(() => {
    if (trackId === null) {
      setAsset(null);
      return;
    }
    let active = true;
    void trackCover(trackId, artist, album)
      .then((next) => {
        if (active) {
          setAsset(next);
        }
      })
      .catch(() => {
        if (active) {
          setAsset(null);
        }
      });
    return () => {
      active = false;
    };
  }, [trackId, artist, album]);

  return (
    <div
      data-now-playing-artwork
      className="relative mx-auto aspect-square h-full max-h-full w-auto max-w-full self-center"
    >
      <div
        aria-hidden="true"
        className="absolute inset-6 rounded-full opacity-50 blur-[90px]"
        style={{ background: "var(--prism-accent)" }}
      />
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={asset?.key ?? "no-cover"}
          initial={{ opacity: 0, scale: 0.985, filter: "blur(6px)" }}
          animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
          exit={{ opacity: 0, scale: 1.015, filter: "blur(8px)" }}
          transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
          className="relative h-full w-full"
        >
          <CoverArt
            path={asset?.localPath ?? null}
            alt={album.length > 0 ? album : title}
            className="h-full w-full rounded-[26px] shadow-[0_50px_120px_-60px_rgb(0_0_0/0.95)]"
            rounded="rounded-[26px]"
            fallbackClassName="h-16 w-16"
          />
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
