import type { JSX, ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";

import { useDragScroll } from "@/hooks/useDragScroll";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";

interface RailProps {
  title: string;
  /** One line under the title. Keep it factual; it is not a slogan. */
  caption?: string;
  /** Rendered to the right of the title, before the arrows. */
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

/**
 * A horizontal shelf of cards.
 *
 * Three ways to move it, because three different people will use it: drag with
 * the mouse, the arrow buttons in the corner, and the native wheel/shift-wheel
 * the browser already provides. The arrows are disabled — not hidden — at the
 * ends so the header never reflows.
 */
export function Rail({ title, caption, action, children, className }: RailProps): JSX.Element {
  const rail = useDragScroll();
  const showArrows = useUiStore((state) => state.showRailArrows);

  return (
    <section className={cn("group/rail relative", className)}>
      <header className="mb-3.5 flex items-end justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[17px] leading-tight font-semibold tracking-[-0.02em] text-white">
            {title}
          </h2>
          {caption !== undefined && caption.length > 0 ? (
            <p className="mt-0.5 truncate text-[11.5px] text-white/35">{caption}</p>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {action}
          {showArrows ? (
            <div className="flex items-center gap-1">
              <RailArrow
                direction={-1}
                disabled={!rail.canScrollLeft}
                onClick={() => {
                  rail.scrollBy(-1);
                }}
              />
              <RailArrow
                direction={1}
                disabled={!rail.canScrollRight}
                onClick={() => {
                  rail.scrollBy(1);
                }}
              />
            </div>
          ) : null}
        </div>
      </header>

      <div
        ref={rail.ref}
        {...rail.handlers}
        className={cn(
          "no-scrollbar -mx-1 flex gap-4 overflow-x-auto px-1 pt-1 pb-3",
          "cursor-grab active:cursor-grabbing",
          // Cards must not start a native image drag while the rail is dragged.
          "[&_img]:pointer-events-none [&_img]:select-none",
        )}
      >
        {children}
      </div>
    </section>
  );
}

function RailArrow({
  direction,
  disabled,
  onClick,
}: {
  direction: 1 | -1;
  disabled: boolean;
  onClick: () => void;
}): JSX.Element {
  const Icon = direction === 1 ? ChevronRight : ChevronLeft;
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={direction === 1 ? "Scroll right" : "Scroll left"}
      className={cn(
        "grid h-7 w-7 place-items-center rounded-full border border-white/10 bg-white/[0.04] text-white/60",
        "transition-colors hover:border-white/20 hover:bg-white/[0.09] hover:text-white",
        "disabled:pointer-events-none disabled:opacity-25",
        "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
      )}
    >
      <Icon className="h-3.5 w-3.5" aria-hidden="true" strokeWidth={2.2} />
    </button>
  );
}
