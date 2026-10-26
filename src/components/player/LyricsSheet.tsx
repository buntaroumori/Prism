import { motion } from "framer-motion";
import { Captions, X } from "lucide-react";
import type { JSX } from "react";

import { LyricsPanel } from "@/components/player/LyricsPanel";
import { usePlaybackStore } from "@/stores/playbackStore";

interface LyricsSheetProps {
  onClose: () => void;
}

const COLUMN_WIDTH = "clamp(304px, 29vw, 432px)";

/**
 * Structured lyrics column owned by the application shell.
 *
 * It participates in layout instead of floating over the current route: its
 * top is below the Top Bar, its bottom is above the Player Bar, and the main
 * content receives the remaining width. The inner fixed-width sheet prevents
 * text from reflowing during the width transition.
 */
export function LyricsSheet({ onClose }: LyricsSheetProps): JSX.Element | null {
  const track = usePlaybackStore((state) => state.status?.track ?? null);

  if (track === null) return null;

  return (
    <motion.aside
      data-lyrics-dock
      aria-label="Lyrics column"
      initial={{ x: 24, opacity: 0 }}
      animate={{ x: 0, opacity: 1 }}
      exit={{ x: 24, opacity: 0 }}
      transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
      style={{ width: COLUMN_WIDTH }}
      className="relative z-10 h-full min-h-0 shrink-0 overflow-hidden border-l border-white/[0.07] bg-oled-950/88 backdrop-blur-2xl"
    >
      <div
        className="flex h-full min-h-0 flex-col"
        style={{ width: COLUMN_WIDTH }}
        data-interactive
      >
        <header className="flex h-[62px] shrink-0 items-center justify-between border-b border-white/[0.055] px-5">
          <div className="min-w-0">
            <p className="text-[9.5px] font-semibold tracking-[0.18em] text-white/25 uppercase">
              Now playing
            </p>
            <h2 className="mt-0.5 truncate text-[13px] font-medium text-white/85">{track.title}</h2>
          </div>
          <button
            type="button"
            aria-label="Close lyrics"
            title="Close lyrics"
            onClick={onClose}
            className="grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/35 transition-colors hover:bg-white/[0.06] hover:text-white focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </header>

        <div className="flex min-h-0 flex-1 items-center gap-2 border-b border-white/[0.045] px-5 py-2.5 text-white/35">
          <Captions className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <p className="truncate text-[10.5px]">{track.artist || "Unknown artist"}</p>
        </div>

        <div className="min-h-0 flex-1 px-5 py-4">
          <LyricsPanel className="rounded-none border-0 bg-transparent backdrop-blur-none" />
        </div>
      </div>
    </motion.aside>
  );
}
