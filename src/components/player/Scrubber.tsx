import { useEffect, useRef, useState, type JSX } from "react";

import { Slider } from "@/components/ui/Slider";
import { useSmoothPlayhead } from "@/hooks/useSmoothPlayhead";
import { formatPosition } from "@/lib/format";
import type { PlayerStatus } from "@/types/ipc";

interface ScrubberProps {
  status: PlayerStatus;
  positionMs: number;
  durationMs: number | null;
  onSeek: (ms: number) => void;
}

/** How long the preview outranks the engine after a seek, in ms. */
const SEEK_HOLD_MS = 700;

/**
 * Seek bar with a live preview and an honest duration.
 *
 * Three things make a seek feel broken, and all three are handled here:
 *
 *  1. During the drag the labels follow the pointer and the engine is left
 *     alone; the seek is one call on release, not forty.
 *  2. After the release the bar keeps the target for a short window instead of
 *     snapping back to the last tick while the device flushes its buffer —
 *     the "it jumps back when I let go" symptom.
 *  3. A track whose header declares no duration disables the bar rather than
 *     pretending it is infinitely long.
 */
export function Scrubber({ status, positionMs, durationMs, onSeek }: ScrubberProps): JSX.Element {
  const [preview, setPreview] = useState<number | null>(null);
  const holdUntil = useRef(0);
  const [held, setHeld] = useState<number | null>(null);

  const known = durationMs !== null && durationMs > 0;
  const playing = status.state === "playing" || status.state === "buffering";
  // The engine's last word is an anchor; between anchors the clock keeps the
  // bar moving. Without this the position sits still whenever a tick is late,
  // which is exactly the "it does not advance" report.
  const live = useSmoothPlayhead(positionMs, playing, known ? durationMs : null);
  const target = preview ?? held ?? live;
  // Never clamp an unknown length to zero: that pinned both the label and the
  // fill at the start of the track for the whole session. With no length the
  // bar simply shows how far it has got.
  const value = known ? Math.min(target, durationMs) : target;
  const waiting = status.state === "buffering";

  // Drop the held target as soon as the engine has caught up (or the hold
  // expires), so the bar follows the playhead again.
  useEffect(() => {
    if (held === null) {
      return;
    }
    const remaining = holdUntil.current - performance.now();
    if (remaining <= 0 || Math.abs(positionMs - held) < 1_200) {
      setHeld(null);
      return;
    }
    const timer = window.setTimeout(() => {
      setHeld(null);
    }, remaining);
    return () => {
      window.clearTimeout(timer);
    };
  }, [held, positionMs]);

  const commit = (next: number): void => {
    setPreview(null);
    if (!known) {
      return;
    }
    holdUntil.current = performance.now() + SEEK_HOLD_MS;
    setHeld(next);
    onSeek(next);
  };

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
          onCommit={commit}
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
