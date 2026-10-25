import { Check, ChevronDown } from "lucide-react";
import { useEffect, useRef, useState, type JSX } from "react";

import { cn } from "@/lib/utils";

export interface PopoverSelectOption {
  value: string;
  label: string;
}

interface PopoverSelectProps {
  ariaLabel: string;
  value: string;
  options: readonly PopoverSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  className?: string;
  buttonClassName?: string;
  menuClassName?: string;
}

/**
 * Prism's compact popover selector.
 *
 * It deliberately uses the same dark, rounded surface as the application
 * context menus instead of the platform-native select popup. The selected
 * option remains visible in the trigger, Escape/outside click dismisses the
 * list, and every option is a real keyboard-accessible button.
 */
export function PopoverSelect({
  ariaLabel,
  value,
  options,
  onChange,
  disabled = false,
  className,
  buttonClassName,
  menuClassName,
}: PopoverSelectProps): JSX.Element {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const selected = options.find((option) => option.value === value) ?? options[0];

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent): void => {
      if (rootRef.current !== null && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const closeKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
      }
    };
    const closeScroll = (): void => {
      setOpen(false);
    };
    window.addEventListener("pointerdown", closeOutside);
    window.addEventListener("keydown", closeKey);
    window.addEventListener("resize", closeScroll);
    window.addEventListener("scroll", closeScroll, true);
    return () => {
      window.removeEventListener("pointerdown", closeOutside);
      window.removeEventListener("keydown", closeKey);
      window.removeEventListener("resize", closeScroll);
      window.removeEventListener("scroll", closeScroll, true);
    };
  }, [open]);

  return (
    <div ref={rootRef} className={cn("relative", className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          setOpen((current) => !current);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
        className={cn(
          "flex h-8 min-w-0 items-center justify-between gap-2 rounded-xl border px-2.5 text-left text-[11.5px] transition-colors",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none disabled:opacity-40",
          open
            ? "border-accent/35 bg-accent/[0.08] text-white"
            : "border-white/[0.09] bg-white/[0.025] text-white/70 hover:border-white/[0.16] hover:bg-white/[0.05] hover:text-white",
          buttonClassName,
        )}
      >
        <span className="truncate">{selected?.label ?? ariaLabel}</span>
        <ChevronDown
          className={cn(
            "h-3.5 w-3.5 shrink-0 text-white/30 transition-transform duration-150",
            open && "rotate-180 text-accent",
          )}
          aria-hidden="true"
        />
      </button>

      {open ? (
        <div
          role="listbox"
          aria-label={ariaLabel}
          className={cn(
            "glass absolute top-[calc(100%+6px)] right-0 z-[70] max-h-[320px] min-w-full overflow-y-auto rounded-2xl p-1.5 shadow-2xl",
            menuClassName,
          )}
        >
          {options.map((option) => {
            const active = option.value === value;
            return (
              <button
                key={option.value || "__all__"}
                type="button"
                role="option"
                aria-selected={active}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                  triggerRef.current?.focus();
                }}
                className={cn(
                  "flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left text-[12px] whitespace-nowrap transition-colors",
                  active
                    ? "bg-accent/[0.10] text-accent"
                    : "text-white/60 hover:bg-white/[0.06] hover:text-white",
                )}
              >
                <span className="min-w-0 flex-1 truncate">{option.label}</span>
                {active ? <Check className="h-3.5 w-3.5 shrink-0" aria-hidden="true" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
