import { useState, type JSX } from "react";

import { Slider } from "@/components/ui/Slider";
import { formatPosition } from "@/lib/format";
import type { PlayerStatus } from "@/types/ipc";

interface ScrubberProps {
  status: PlayerStatus;
  positionMs: number;
  durationMs: number | null;
  onSeek: (ms: number) => void;
}

/**
 * Seek bar with a live preview and an honest duration.
 *
 * While dragging, the time labels follow the pointer and the engine is left
 * alone; `onSeek` fires once on release. When the container declares no
 * duration (a malformed header), the bar is disabled rather than pretending the
 * track is infinitely long.
 */
export function Scrubber({ status, positionMs, durationMs, onSeek }: ScrubberProps): JSX.Element {
  const [preview, setPreview] = useState<number | null>(null);
  const known = durationMs !== null && durationMs > 0;
  const value = Math.min(preview ?? positionMs, known ? durationMs : 0);
  const waiting = status.state === "buffering";

  return (
    <div className="flex w-full items-center gap-3">
      <span className="tabular w-11 shrink-0 text-right text-[11px] text-white/45">
        {formatPosition(value)}
      </span>

      <div className="relative flex-1">
        <Slider
          value={value}
          min={0}
          max={known ? durationMs : 1}
          step={1_000}
          label="Seek"
          disabled={!known}
          onChange={(next) => {
            if (known) {
              setPreview(next);
            }
          }}
          onCommit={(next) => {
            setPreview(null);
            if (known) {
              onSeek(next);
            }
          }}
        />
        {waiting ? (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 -top-1.5 flex justify-center"
          >
            <span className="accent-bg h-0.5 w-16 animate-pulse rounded-full opacity-70" />
          </span>
        ) : null}
      </div>

      <span className="tabular w-11 shrink-0 text-[11px] text-white/45">
        {known ? formatPosition(durationMs) : "—:—"}
      </span>
    </div>
  );
}
