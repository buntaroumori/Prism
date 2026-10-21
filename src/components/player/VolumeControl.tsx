import type { JSX } from "react";
import { Volume, Volume1, Volume2, VolumeX } from "lucide-react";

import { Slider } from "@/components/ui/Slider";
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
 * Volume and mute.
 *
 * The engine still works in decibels — that is the only unit in which "unity
 * gain" is a statement rather than an approximation — but the *interface* shows
 * a percentage: a listener adjusts loudness, and a number between 0 and 100 is
 * the shape of that gesture. The dB value stays in the tooltip for anyone who
 * needs it, and the icon changes with the level so the bar answers "how loud"
 * at a glance.
 */
export function VolumeControl({
  volumeDb,
  muted,
  onPreview,
  onCommit,
  onToggleMute,
}: VolumeControlProps): JSX.Element {
  const value = muted ? FLOOR_DB : Math.min(TOP_DB, Math.max(FLOOR_DB, volumeDb));
  const percent = Math.round(((value - FLOOR_DB) / (TOP_DB - FLOOR_DB)) * 100);
  const Icon =
    muted || percent === 0 ? VolumeX : percent > 75 ? Volume2 : percent > 35 ? Volume1 : Volume;

  return (
    <div className="no-drag flex w-[150px] shrink-0 items-center gap-2">
      <button
        type="button"
        onClick={onToggleMute}
        aria-label={muted ? "Unmute" : "Mute"}
        aria-pressed={muted}
        title={muted ? "Unmute" : `Mute · ${String(Math.round(value))} dB`}
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
        onChange={(next) => {
          if (muted) {
            onToggleMute();
          }
          onPreview(next);
        }}
        onCommit={onCommit}
      />
    </div>
  );
}
