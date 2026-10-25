import type { JSX } from "react";
import {
  PanelLeftClose,
  PanelLeftOpen,
  Pause,
  Play,
  Shuffle,
  SkipBack,
  SkipForward,
  Square,
} from "lucide-react";

import { cn } from "@/lib/utils";
import type { PlaybackState } from "@/types/ipc";

interface TransportControlsProps {
  state: PlaybackState;
  canSkip: boolean;
  busy: boolean;
  shuffled: boolean;
  sidebarHidden: boolean;
  onToggleSidebar: () => void;
  onToggleShuffle: () => void;
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
  shuffled,
  sidebarHidden,
  onToggleSidebar,
  onToggleShuffle,
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
        onClick={onToggleSidebar}
        aria-label={sidebarHidden ? "Show sidebar" : "Hide sidebar"}
        aria-pressed={sidebarHidden}
        title={sidebarHidden ? "Show sidebar" : "Hide sidebar"}
        className="grid h-9 w-9 place-items-center rounded-full text-white/40 transition-colors hover:bg-white/[0.07] hover:text-white focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
      >
        {sidebarHidden ? (
          <PanelLeftOpen className="h-[17px] w-[17px]" aria-hidden="true" />
        ) : (
          <PanelLeftClose className="h-[17px] w-[17px]" aria-hidden="true" />
        )}
      </button>

      <button
        type="button"
        onClick={onToggleShuffle}
        disabled={idle}
        aria-label={shuffled ? "Disable shuffle" : "Shuffle queue"}
        aria-pressed={shuffled}
        title={shuffled ? "Restore the original queue order" : "Shuffle the upcoming queue"}
        className={cn(
          "mr-1 grid h-9 w-9 place-items-center rounded-full transition-colors",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
          "disabled:opacity-30 disabled:hover:bg-transparent",
          shuffled
            ? "bg-accent/[0.14] text-accent hover:bg-accent/[0.22]"
            : "text-white/45 hover:bg-white/[0.07] hover:text-white",
        )}
      >
        <Shuffle className="h-[17px] w-[17px]" aria-hidden="true" />
      </button>

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
