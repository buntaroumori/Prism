import { motion } from "framer-motion";
import type { JSX } from "react";

import { cn } from "@/lib/utils";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  hint?: string;
}

interface SegmentedProps<T extends string> {
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  className?: string;
  /** Shared layout id; must be unique per instance on screen. */
  layoutId: string;
}

/**
 * Segmented control with a shared-layout selection pill.
 *
 * Framer Motion's `layoutId` animates the indicator between segments with a
 * single transform, so switching a four-way output mode costs one compositor
 * animation instead of four background transitions.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  className,
  layoutId,
}: SegmentedProps<T>): JSX.Element {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cn(
        "inline-flex items-center gap-1 rounded-full border border-white/[0.08] bg-white/[0.04] p-1",
        className,
      )}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            title={option.hint}
            onClick={() => {
              if (!selected) {
                onChange(option.value);
              }
            }}
            className={cn(
              "relative h-7 rounded-full px-3 text-[11.5px] font-medium tracking-tight transition-colors duration-150",
              "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
              selected ? "text-white" : "text-white/45 hover:text-white/75",
            )}
          >
            {selected ? (
              <motion.span
                layoutId={layoutId}
                transition={{ type: "spring", stiffness: 520, damping: 38, mass: 0.6 }}
                className="absolute inset-0 rounded-full bg-white/[0.12] ring-1 ring-white/[0.08] ring-inset"
              />
            ) : null}
            <span className="relative">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
