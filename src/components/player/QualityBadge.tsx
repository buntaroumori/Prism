import type { JSX } from "react";

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
 * `BIT-PERFECT` is claimed only when the engine confirms an untouched path to
 * an exclusive endpoint; source resolution is stated separately, from the
 * file's own format. Icon-free on purpose — a badge is a label, not a sticker —
 * and it never wraps: in a narrow window the trailing word is dropped, not the
 * whole badge, so the transport column keeps its geometry at every width.
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
  const resolution = rate === null ? null : `${String(rate / 1_000)}k`;

  if (status.bitPerfect) {
    return (
      <Badge
        tone="positive"
        label={status.dop ? "DSD" : "Bit-perfect"}
        tail={resolution}
        title={
          status.dop
            ? `DoP at ${String(rate ?? 0)} Hz — the DSD markers reach the DAC untouched`
            : "Exclusive endpoint, exact format match, no DSP, unity gain"
        }
        className={className}
      />
    );
  }

  if (hiRes) {
    return (
      <Badge
        tone="neutral"
        label={bits === null ? "Hi-Res" : `${String(bits)}/${resolution ?? "—"}`}
        tail="Processed"
        title={`Source is ${bits === null ? "high resolution" : `${String(bits)}-bit`} at ${String(rate ?? 0)} Hz${
          status.outputFormat === null
            ? ""
            : `, output ${String(status.outputFormat.sampleRate)} Hz`
        }. The current path is not bit-perfect (shared mixer, resampling or DSP).`}
        className={className}
      />
    );
  }

  return (
    <Badge
      tone="muted"
      label="CD"
      tail={resolution}
      title="CD-resolution source"
      className={className}
    />
  );
}

function Badge({
  tone,
  label,
  tail,
  title,
  className,
}: {
  tone: "positive" | "neutral" | "muted";
  label: string;
  tail: string | null;
  title: string;
  className?: string | undefined;
}): JSX.Element {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex max-w-full shrink-0 items-center gap-1 overflow-hidden rounded-full border px-2 py-0.5",
        "text-[9.5px] leading-4 font-semibold tracking-[0.1em] whitespace-nowrap uppercase",
        tone === "positive" && "border-positive/35 bg-positive/[0.10] text-positive",
        tone === "neutral" && "border-white/15 bg-white/[0.06] text-white/70",
        tone === "muted" && "border-white/10 bg-white/[0.04] text-white/45",
        className,
      )}
    >
      <span className="truncate">{label}</span>
      {tail === null ? null : (
        <span className="hidden truncate text-white/30 min-[1180px]:inline">· {tail}</span>
      )}
    </span>
  );
}
