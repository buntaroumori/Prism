import { AnimatePresence, motion } from "framer-motion";
import { useCallback, useEffect, useRef, useState, type JSX, type ReactNode } from "react";

import { cn } from "@/lib/utils";

interface PopoverProps {
  /** The trigger is rendered as-is; it receives the click handler. */
  trigger: (props: { open: boolean; toggle: () => void }) => ReactNode;
  children: ReactNode;
  /** Horizontal placement relative to the trigger. */
  align?: "start" | "end";
  /** Vertical placement; the player bar opens upwards, the top bar downwards. */
  side?: "top" | "bottom";
  className?: string;
  label: string;
  /** Optional persisted/controlled open state. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

/**
 * Anchored popover with outside-click and Escape handling.
 *
 * Used by the output-device selector and the account menu in the top bar. It is
 * deliberately not a modal: a device list must not trap focus or dim the
 * player bar, because the user is usually changing output *while* listening.
 */
export function Popover({
  trigger,
  children,
  align = "end",
  side = "top",
  className,
  label,
  open: controlledOpen,
  onOpenChange,
}: PopoverProps): JSX.Element {
  const [internalOpen, setInternalOpen] = useState(false);
  const open = controlledOpen ?? internalOpen;
  const container = useRef<HTMLDivElement | null>(null);
  const setOpen = useCallback(
    (next: boolean): void => {
      if (controlledOpen === undefined) {
        setInternalOpen(next);
      }
      onOpenChange?.(next);
    },
    [controlledOpen, onOpenChange],
  );

  useEffect(() => {
    if (!open) {
      return;
    }
    const onPointerDown = (event: MouseEvent): void => {
      if (container.current !== null && !container.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, setOpen]);

  return (
    <div ref={container} className="relative">
      {trigger({
        open,
        toggle: () => {
          setOpen(!open);
        },
      })}

      <AnimatePresence>
        {open ? (
          <motion.div
            role="dialog"
            aria-label={label}
            initial={{ opacity: 0, y: 6, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 4, scale: 0.985 }}
            transition={{ duration: 0.16, ease: [0.22, 1, 0.36, 1] }}
            className={cn(
              "glass absolute z-50 min-w-[260px] rounded-2xl p-1.5",
              side === "top"
                ? "bottom-[calc(100%+10px)] origin-bottom"
                : "top-[calc(100%+10px)] origin-top",
              align === "end" ? "right-0" : "left-0",
              className,
            )}
          >
            {children}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
