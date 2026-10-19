import type { JSX, ReactNode } from "react";

import { cn } from "@/lib/utils";

interface SectionHeadingProps {
  eyebrow: string;
  title: string;
  description: string;
  action?: ReactNode;
  className?: string;
}

/** Editorial section header: eyebrow, display title, one-sentence rationale. */
export function SectionHeading({
  eyebrow,
  title,
  description,
  action,
  className,
}: SectionHeadingProps): JSX.Element {
  return (
    <div className={cn("flex flex-wrap items-end justify-between gap-4", className)}>
      <div>
        <p className="text-[10px] font-medium tracking-[0.18em] text-accent-soft/70 uppercase">
          {eyebrow}
        </p>
        <h2 className="mt-1.5 text-[22px] font-semibold tracking-tight text-white">{title}</h2>
        <p className="mt-1.5 max-w-lg text-[13px] leading-relaxed text-white/40">{description}</p>
      </div>
      {action}
    </div>
  );
}
