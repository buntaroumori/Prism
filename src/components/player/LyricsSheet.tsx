import { motion, useReducedMotion } from "framer-motion";
import { Mic2, X } from "lucide-react";
import type { JSX } from "react";
import { createPortal } from "react-dom";

import { LyricsPanel } from "@/components/player/LyricsPanel";
import { useLyrics } from "@/hooks/useLyrics";

interface LyricsSheetProps {
  trackId: string | null;
  onClose: () => void;
}

/** Centre-bottom lyric sheet revealed from behind the persistent player bar. */
export function LyricsSheet({ trackId, onClose }: LyricsSheetProps): JSX.Element {
  const reduce = useReducedMotion();
  const lyrics = useLyrics(trackId);

  return createPortal(
    <div className="pointer-events-none fixed bottom-[78px] left-1/2 z-[25] h-[min(62vh,620px)] w-[66vw] max-w-[980px] min-w-[520px] -translate-x-1/2">
      <motion.section
        aria-label="Synchronized lyrics"
        initial={reduce === true ? { opacity: 0 } : { opacity: 0, y: 120 }}
        animate={{ opacity: 1, y: 0 }}
        exit={reduce === true ? { opacity: 0 } : { opacity: 0, y: 120 }}
        transition={{ duration: 0.19, ease: [0.22, 1, 0.36, 1] }}
        className="pointer-events-auto flex h-full flex-col"
      >
        <div className="glass-bar flex h-11 shrink-0 items-center justify-between rounded-t-3xl border border-b-0 border-white/[0.08] px-4">
          <div className="flex items-center gap-2">
            <Mic2 className="h-4 w-4 text-accent" aria-hidden="true" />
            <h2 className="text-[12px] font-medium text-white/75">Synchronized lyrics</h2>
          </div>
          <button
            type="button"
            aria-label="Hide synchronized lyrics"
            onClick={onClose}
            className="grid h-7 w-7 place-items-center rounded-full text-white/35 transition-colors hover:bg-white/[0.07] hover:text-white"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
        <div className="min-h-0 flex-1">
          <LyricsPanel lyrics={lyrics} className="rounded-t-none border-white/[0.08]" />
        </div>
      </motion.section>
    </div>,
    document.body,
  );
}
