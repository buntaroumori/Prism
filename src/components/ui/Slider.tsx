import { useCallback, useRef, type JSX, type KeyboardEvent, type PointerEvent } from "react";

import { cn } from "@/lib/utils";

interface SliderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  /** Called continuously while dragging. */
  onChange: (value: number) => void;
  /** Called once on release / keyboard commit. */
  onCommit?: (value: number) => void;
  label: string;
  className?: string;
  /** Extra content under the track (the seek bar renders buffered ranges). */
  disabled?: boolean;
  /** Stretch to fill the parent (the seek bar) or a fixed rail (volume). */
  variant?: "seek" | "volume";
}

/**
 * Pointer-driven slider used by the seek bar and the volume control.
 *
 * Native `<input type=range>` cannot render a palette-tinted, glass-styled track
 * across WebKitGTK and WebView2 without per-engine hacks, so the interaction is
 * reimplemented on a `role="slider"` div: pointer capture for drags, full
 * keyboard support (arrows, Home/End, PageUp/PageDown), and `aria-valuetext`
 * carrying the formatted time. Drags call `onChange` for a live preview and
 * `onCommit` once on release, so a seek is a single engine call, not forty.
 */
export function Slider({
  value,
  min = 0,
  max = 100,
  step = 1,
  onChange,
  onCommit,
  label,
  className,
  disabled = false,
  variant = "seek",
}: SliderProps): JSX.Element {
  const trackRef = useRef<HTMLDivElement | null>(null);
  const dragging = useRef(false);

  const clamp = useCallback(
    (next: number): number => Math.min(max, Math.max(min, next)),
    [min, max],
  );

  const valueFromPointer = useCallback(
    (event: PointerEvent<HTMLDivElement>): number => {
      const track = trackRef.current;
      if (track === null) {
        return value;
      }
      const rect = track.getBoundingClientRect();
      if (rect.width === 0) {
        return value;
      }
      const ratio = (event.clientX - rect.left) / rect.width;
      const raw = min + ratio * (max - min);
      const quantised = Math.round(raw / step) * step;
      return clamp(quantised);
    },
    [clamp, max, min, step, value],
  );

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    if (disabled) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    dragging.current = true;
    onChange(valueFromPointer(event));
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    if (!dragging.current || disabled) {
      return;
    }
    onChange(valueFromPointer(event));
  };

  const handlePointerUp = (event: PointerEvent<HTMLDivElement>): void => {
    if (!dragging.current) {
      return;
    }
    dragging.current = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    const next = valueFromPointer(event);
    onChange(next);
    onCommit?.(next);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (disabled) {
      return;
    }
    const jump = event.shiftKey ? step * 10 : step;
    let next: number | null = null;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowUp":
        next = value + jump;
        break;
      case "ArrowLeft":
      case "ArrowDown":
        next = value - jump;
        break;
      case "PageUp":
        next = value + (max - min) / 10;
        break;
      case "PageDown":
        next = value - (max - min) / 10;
        break;
      case "Home":
        next = min;
        break;
      case "End":
        next = max;
        break;
      default:
        return;
    }
    event.preventDefault();
    const clamped = clamp(next);
    onChange(clamped);
    onCommit?.(clamped);
  };

  const ratio = max === min ? 0 : (clamp(value) - min) / (max - min);
  const percent = Math.max(0, Math.min(100, ratio * 100));

  return (
    <div
      ref={trackRef}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      aria-disabled={disabled}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      onKeyDown={handleKeyDown}
      className={cn(
        "group relative flex touch-none items-center",
        variant === "seek" ? "h-4 w-full cursor-pointer" : "h-4 w-full cursor-pointer",
        disabled && "cursor-default opacity-50",
        className,
      )}
    >
      <div
        className={cn(
          "relative w-full overflow-hidden rounded-full bg-white/[0.14]",
          variant === "seek" ? "h-1 group-hover:h-1.5" : "h-1",
          "transition-[height] duration-150",
        )}
      >
        <div
          className="accent-bg absolute inset-y-0 left-0 rounded-full"
          style={{ width: `${String(percent)}%` }}
        />
      </div>
      <span
        aria-hidden="true"
        className={cn(
          "pointer-events-none absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow-[0_1px_6px_rgb(0_0_0/0.5)]",
          "scale-0 opacity-0 transition-[transform,opacity] duration-150 group-hover:scale-100 group-hover:opacity-100 group-focus-visible:scale-100 group-focus-visible:opacity-100",
        )}
        style={{ left: `${String(percent)}%` }}
      />
    </div>
  );
}
