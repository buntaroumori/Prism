import type { JSX } from "react";

import { sourceLabel } from "@/lib/sourceLabel";
import { cn } from "@/lib/utils";
import type { PlayerStatus, TrackRow } from "@/types/ipc";

interface QualityBadgeProps {
  status: PlayerStatus;
  /** Catalogue metadata retains the file's bit depth and codec. */
  track?: TrackRow | null;
  className?: string;
}

/** Source metadata only: output routing never changes what the file is. */
export function QualityBadge({
  status,
  track = null,
  className,
}: QualityBadgeProps): JSX.Element | null {
  if (status.track === null) return null;
  const format = track?.format ?? "Audio";
  const bits = track?.bitDepth ?? null;
  const rate = track?.sampleRate ?? status.sourceFormat?.sampleRate ?? null;
  const source = sourceLabel({ format, sampleRate: rate, bitDepth: bits });
  const title = `${format.toUpperCase()} source${bits === null ? "" : `, ${String(bits)}-bit`}${rate === null ? "" : ` at ${String(rate)} Hz`}`;

  return (
    <span
      title={title}
      className={cn(
        "inline-flex max-w-full shrink-0 items-center overflow-hidden rounded-full border px-2 py-0.5",
        "text-[9.5px] leading-4 font-semibold tracking-[0.08em] whitespace-nowrap uppercase",
        source.tone === "hi-res" &&
          "border-spectrum-violet/35 bg-spectrum-violet/[0.10] text-spectrum-violet",
        source.tone === "cd" && "border-positive/35 bg-positive/[0.10] text-positive",
        source.tone === "neutral" && "border-white/10 bg-white/[0.04] text-white/45",
        className,
      )}
    >
      <span className="truncate">{source.label}</span>
    </span>
  );
}
