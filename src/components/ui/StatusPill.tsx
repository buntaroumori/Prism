import type { JSX } from "react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

export type StatusTone = "positive" | "warning" | "critical" | "neutral";

interface StatusPillProps {
  tone: StatusTone;
  children: ReactNode;
  pulse?: boolean;
  className?: string;
}

const TONE_CLASSES: Record<StatusTone, { pill: string; dot: string }> = {
  positive: {
    pill: "border-positive/25 bg-positive/[0.08] text-positive",
    dot: "bg-positive",
  },
  warning: {
    pill: "border-warning/25 bg-warning/[0.08] text-warning",
    dot: "bg-warning",
  },
  critical: {
    pill: "border-critical/25 bg-critical/[0.08] text-critical",
    dot: "bg-critical",
  },
  neutral: {
    pill: "border-white/10 bg-white/[0.05] text-white/60",
    dot: "bg-white/40",
  },
};

/** Compact status chip with an optional breathing indicator dot. */
export function StatusPill({
  tone,
  children,
  pulse = false,
  className,
}: StatusPillProps): JSX.Element {
  const classes = TONE_CLASSES[tone];

  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-full border px-2.5 py-1 text-[11px] font-medium tracking-[0.02em]",
        classes.pill,
        className,
      )}
    >
      <span className="relative flex h-1.5 w-1.5" aria-hidden="true">
        {pulse ? (
          <span
            className={cn(
              "absolute inline-flex h-full w-full animate-ping rounded-full opacity-60",
              classes.dot,
            )}
          />
        ) : null}
        <span className={cn("relative inline-flex h-1.5 w-1.5 rounded-full", classes.dot)} />
      </span>
      {children}
    </span>
  );
}
