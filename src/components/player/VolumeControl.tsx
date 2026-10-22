import type { JSX } from "react";
import { useEffect, useRef } from "react";
import { Volume, Volume1, Volume2, VolumeX } from "lucide-react";

import { Slider } from "@/components/ui/Slider";
import { cn } from "@/lib/utils";
import { volumeDbFromPercent, volumePercentFromDb } from "@/lib/volume";

interface VolumeControlProps {
  /** Current attenuation in dB (`0` = unity, `-60` = silence). */
  volumeDb: number;
  muted: boolean;
  onPreview: (db: number) => void;
  onCommit: (db: number) => void;
  onToggleMute: () => void;
  /** Explicit, idempotent unmute for the start of a slider gesture. */
  onUnmute: () => void;
}

/**
 * Volume and mute.
 *
 * The rail is a perceptual 0–100 control; only the IPC boundary uses decibels.
 * A drag that starts muted unmutes once, never once per pointer-move event.
 */
export function VolumeControl({
  volumeDb,
  muted,
  onPreview,
  onCommit,
  onToggleMute,
  onUnmute,
}: VolumeControlProps): JSX.Element {
  const value = Math.min(0, Math.max(-60, volumeDb));
  const percent = muted ? 0 : volumePercentFromDb(value);
  const unmuteRequested = useRef(false);

  useEffect(() => {
    if (!muted) {
      unmuteRequested.current = false;
    }
  }, [muted]);

  const prepareChange = (): void => {
    if (muted && !unmuteRequested.current) {
      unmuteRequested.current = true;
      onUnmute();
    }
  };

  const Icon =
    muted || percent === 0 ? VolumeX : percent > 75 ? Volume2 : percent > 35 ? Volume1 : Volume;

  return (
    <div className="no-drag flex w-[150px] shrink-0 items-center gap-2">
      <button
        type="button"
        onClick={onToggleMute}
        aria-label={muted ? "Unmute" : "Mute"}
        aria-pressed={muted}
        title={muted ? "Unmute" : `Mute · ${value.toFixed(1)} dB`}
        className={cn(
          "grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/55 transition-colors",
          "hover:bg-white/[0.07] hover:text-white",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
        )}
      >
        <Icon className="h-4 w-4" aria-hidden="true" />
      </button>

      <Slider
        value={percent}
        min={0}
        max={100}
        step={1}
        label="Volume"
        variant="volume"
        onChange={(next) => {
          prepareChange();
          onPreview(volumeDbFromPercent(next));
        }}
        onCommit={(next) => {
          onCommit(volumeDbFromPercent(next));
        }}
      />
      <span className="tabular w-7 text-right text-[10px] text-white/30" aria-hidden="true">
        {percent}
      </span>
    </div>
  );
}
