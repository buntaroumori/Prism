import type { JSX } from "react";
import { Gem, TriangleAlert } from "lucide-react";

import { displayBits } from "@/lib/audio";
import { cn } from "@/lib/utils";
import type { PlayerStatus } from "@/types/ipc";

interface QualityBadgeProps {
  status: PlayerStatus;
  className?: string;
}

/**
 * The honesty badge.
 *
 * `HI-RES` is claimed only when the *source* is above CD quality (bit depth or
 * sample rate), and `BIT-PERFECT` only when the engine confirms an untouched
 * path to an exclusive endpoint. When the engine reports a bit-perfect-capable
 * source but the path is being altered, the badge says `PROCESSED` instead of
 * quietly implying purity — that distinction is the whole point of the product.
 */
export function QualityBadge({ status, className }: QualityBadgeProps): JSX.Element | null {
  const source = status.sourceFormat;
  const track = status.track;

  if (track === null) {
    return null;
  }

  const bits = source === null ? null : displayBits(source.sampleFormat);
  const rate = source?.sampleRate ?? null;
  const hiRes = status.dop || (bits !== null && bits > 16) || (rate !== null && rate > 48_000);

  if (status.bitPerfect) {
    return (
      <span
        className={cn(
          "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] uppercase",
          "border-positive/35 bg-positive/[0.10] text-positive",
          className,
        )}
        title={
          status.dop
            ? `DoP ${String(rate ?? 0)} Hz — DSD markers reach the DAC untouched`
            : "Exclusive endpoint, exact format match, no DSP, 0 dB"
        }
      >
        <Gem className="h-3 w-3" aria-hidden="true" strokeWidth={2.2} />
        {status.dop ? "DSD · Bit-perfect" : "Bit-perfect"}
      </span>
    );
  }

  if (hiRes) {
    return (
      <span
        className={cn(
          "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] uppercase",
          "border-white/15 bg-white/[0.06] text-white/70",
          className,
        )}
        title="Source is above CD quality, but the current output path is not bit-perfect."
      >
        <TriangleAlert className="h-3 w-3 text-warning/80" aria-hidden="true" strokeWidth={2.2} />
        Hi-Res · Processed
      </span>
    );
  }

  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full border border-white/10 bg-white/[0.04] px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] text-white/45 uppercase",
        className,
      )}
      title="CD-resolution source"
    >
      CD
    </span>
  );
}
