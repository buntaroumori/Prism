import type { JSX } from "react";
import { Volume, Volume1, Volume2, VolumeX } from "lucide-react";

import { Slider } from "@/components/ui/Slider";
import { formatDecibels } from "@/lib/format";
import { cn } from "@/lib/utils";

interface VolumeControlProps {
  /** Current attenuation in dB (`0` = unity, `-60` = silence). */
  volumeDb: number;
  muted: boolean;
  onPreview: (db: number) => void;
  onCommit: (db: number) => void;
  onToggleMute: () => void;
}

/** Attenuation floor exposed to the user; below this the engine mute flag is used. */
const FLOOR_DB = -60;
const TOP_DB = 0;

/**
 * Volume in decibels, not in a fake percentage.
 *
 * An audiophile player has to be able to say "unity gain" out loud: the slider is
 * labelled in dB, the notch snaps to exactly 0 dB, and only at 0 dB (unmuted, no
 * DSP) can the engine claim a bit-perfect path. A linear 0–100 % slider would
 * make that promise impossible to see.
 */
export function VolumeControl({
  volumeDb,
  muted,
  onPreview,
  onCommit,
  onToggleMute,
}: VolumeControlProps): JSX.Element {
  const value = muted ? FLOOR_DB : Math.min(TOP_DB, Math.max(FLOOR_DB, volumeDb));
  const Icon = muted ? VolumeX : volumeDb >= -3 ? Volume2 : volumeDb >= -18 ? Volume1 : Volume;

  return (
    <div className="no-drag flex w-[168px] items-center gap-2.5">
      <button
        type="button"
        onClick={onToggleMute}
        aria-label={muted ? "Unmute" : "Mute"}
        aria-pressed={muted}
        className={cn(
          "grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/55 transition-colors",
          "hover:bg-white/[0.07] hover:text-white",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
        )}
      >
        <Icon className="h-4 w-4" aria-hidden="true" />
      </button>

      <Slider
        value={value}
        min={FLOOR_DB}
        max={TOP_DB}
        step={0.5}
        label="Volume"
        variant="volume"
        onChange={onPreview}
        onCommit={onCommit}
      />

      <span
        className={cn(
          "tabular w-14 shrink-0 text-right text-[10.5px]",
          value === TOP_DB && !muted ? "accent-soft-text" : "text-white/40",
        )}
        title={value === TOP_DB && !muted ? "Unity gain — the bit-perfect requirement" : undefined}
      >
        {muted ? "Muted" : formatDecibels(value)}
      </span>
    </div>
  );
}
