import type { JSX } from "react";
import { Pause, Play, SkipBack, SkipForward, Square } from "lucide-react";

import { cn } from "@/lib/utils";
import type { PlaybackState } from "@/types/ipc";

interface TransportControlsProps {
  state: PlaybackState;
  canSkip: boolean;
  busy: boolean;
  onToggle: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onStop: () => void;
}

/**
 * Transport cluster.
 *
 * The play button changes glyph by state and never resizes: `Play` and `Pause`
 * are both Lucide 24 px glyphs inside the same 44 px circle, so starting a
 * track does not shift the two skip buttons by a pixel.
 */
export function TransportControls({
  state,
  canSkip,
  busy,
  onToggle,
  onNext,
  onPrevious,
  onStop,
}: TransportControlsProps): JSX.Element {
  const playing = state === "playing" || state === "buffering";
  const idle = state === "stopped";
  const label = playing ? "Pause" : state === "paused" ? "Resume" : "Play";

  return (
    <div className="flex items-center gap-1.5">
      <button
        type="button"
        onClick={onPrevious}
        disabled={idle}
        aria-label="Previous track"
        className={cn(
          "grid h-9 w-9 place-items-center rounded-full text-white/60 transition-colors",
          "hover:bg-white/[0.07] hover:text-white disabled:opacity-30 disabled:hover:bg-transparent",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
        )}
      >
        <SkipBack className="h-[18px] w-[18px]" aria-hidden="true" fill="currentColor" />
      </button>

      <button
        type="button"
        onClick={onToggle}
        disabled={idle || busy}
        aria-label={label}
        aria-pressed={playing}
        className={cn(
          "grid h-11 w-11 place-items-center rounded-full bg-white text-oled-950 transition-transform duration-150",
          "hover:scale-[1.04] active:scale-[0.97] disabled:pointer-events-none disabled:opacity-40",
          "focus-visible:ring-2 focus-visible:ring-white/80 focus-visible:ring-offset-2 focus-visible:ring-offset-oled-950 focus-visible:outline-none",
        )}
      >
        {playing ? (
          <Pause className="h-[19px] w-[19px]" aria-hidden="true" fill="currentColor" />
        ) : (
          <Play
            className="ml-0.5 h-[19px] w-[19px]"
            aria-hidden="true"
            fill="currentColor"
            strokeWidth={1}
          />
        )}
      </button>

      <button
        type="button"
        onClick={onNext}
        disabled={!canSkip}
        aria-label="Next track"
        className={cn(
          "grid h-9 w-9 place-items-center rounded-full text-white/60 transition-colors",
          "hover:bg-white/[0.07] hover:text-white disabled:opacity-30 disabled:hover:bg-transparent",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
        )}
      >
        <SkipForward className="h-[18px] w-[18px]" aria-hidden="true" fill="currentColor" />
      </button>

      <button
        type="button"
        onClick={onStop}
        disabled={idle}
        aria-label="Stop"
        className={cn(
          "grid h-9 w-9 place-items-center rounded-full text-white/35 transition-colors",
          "hover:bg-white/[0.07] hover:text-white/80 disabled:pointer-events-none disabled:opacity-0",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
        )}
      >
        <Square className="h-3.5 w-3.5" aria-hidden="true" fill="currentColor" />
      </button>
    </div>
  );
}
