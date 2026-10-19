import { useEffect, useRef, type JSX } from "react";

import { acquireSpectrum, latestSpectrum, subscribeSpectrum } from "@/lib/spectrum";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";
import type { SpectrumFrame } from "@/types/ipc";

const BAND_COUNT = 31;
/** How fast a bar catches up with its target each frame (0..1). */
const EASE = 0.22;
/** How fast peak caps fall, per frame. */
const PEAK_FALL = 0.006;
const FALLBACK_ACCENT = "#8ba6ff";
const FALLBACK_HIGHLIGHT = "#f4f6ff";

/** `noUncheckedIndexedAccess` turns every typed-array read into `number | undefined`. */
function at(values: Float32Array, index: number): number {
  return values[index] ?? 0;
}

interface SpectrumVisualiserProps {
  className?: string;
  /** Mirror the bars around the vertical centre. */
  mirrored?: boolean;
  /** Accessible description; the canvas is otherwise silent. */
  label?: string;
}

/**
 * 31-band spectrum analyser.
 *
 * The transform runs in Rust on the audio thread; this component receives a
 * normalised 31-float snapshot at 60 Hz and draws it with `requestAnimationFrame`,
 * interpolating between frames so motion is smooth on a 120 Hz panel without
 * raising the IPC rate. Nothing here touches React state during playback: the
 * sample arrays live in refs and the canvas is painted imperatively, so a
 * spectrum cannot cause a single component re-render.
 *
 * With `prefers-reduced-motion` the interpolation loop is skipped and each frame
 * is drawn once, statically.
 */
export function SpectrumVisualiser({
  className,
  mirrored = false,
  label = "Spectrum analyser",
}: SpectrumVisualiserProps): JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) {
      return;
    }
    const context = canvas.getContext("2d");
    if (context === null) {
      return;
    }

    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const target = new Float32Array(BAND_COUNT);
    const current = new Float32Array(BAND_COUNT);
    const peaks = new Float32Array(BAND_COUNT);
    let width = 0;
    let height = 0;
    let frameId = 0;
    let disposed = false;
    let accent = FALLBACK_ACCENT;
    let highlight = FALLBACK_HIGHLIGHT;

    const readPalette = (): void => {
      const styles = getComputedStyle(canvas);
      accent = styles.getPropertyValue("--prism-accent").trim() || FALLBACK_ACCENT;
      highlight = styles.getPropertyValue("--prism-highlight").trim() || FALLBACK_HIGHLIGHT;
    };

    const resize = (): void => {
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      const bounds = canvas.getBoundingClientRect();
      width = Math.max(1, Math.floor(bounds.width));
      height = Math.max(1, Math.floor(bounds.height));
      canvas.width = Math.floor(width * ratio);
      canvas.height = Math.floor(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      readPalette();
    };

    const applyFrame = (frame: SpectrumFrame): void => {
      const bands = frame.bands;
      for (let index = 0; index < BAND_COUNT; index += 1) {
        // The backend always sends 31 bands; a shorter frame is padded with
        // silence rather than shifting every bar to the wrong slot.
        const value = bands[index] ?? 0;
        target[index] = value;
        if (value > at(peaks, index)) {
          peaks[index] = value;
        }
      }
    };

    const draw = (): void => {
      context.clearRect(0, 0, width, height);

      const gap = width > 360 ? 4 : 3;
      const barWidth = Math.max(2, (width - gap * (BAND_COUNT - 1)) / BAND_COUNT);
      const baseline = mirrored ? height / 2 : height;
      const maxHeight = mirrored ? height / 2 - 2 : height - 2;

      const gradient = context.createLinearGradient(0, baseline, 0, baseline - maxHeight);
      gradient.addColorStop(0, accent);
      gradient.addColorStop(1, highlight);

      context.fillStyle = gradient;
      context.shadowColor = accent;
      context.shadowBlur = 12;

      for (let index = 0; index < BAND_COUNT; index += 1) {
        const value = at(current, index);
        const barHeight = Math.max(1.5, value * maxHeight);
        const x = index * (barWidth + gap);
        const y = baseline - barHeight;
        const radius = Math.min(barWidth / 2, 3);

        context.globalAlpha = 0.35 + value * 0.65;
        context.beginPath();
        context.roundRect(x, y, barWidth, barHeight, radius);
        context.fill();

        if (mirrored) {
          context.globalAlpha = 0.18 + value * 0.32;
          context.beginPath();
          context.roundRect(x, baseline, barWidth, barHeight, radius);
          context.fill();
        }

        const peak = at(peaks, index);
        if (peak > 0.02) {
          context.globalAlpha = 0.85;
          const peakY = mirrored
            ? baseline - peak * maxHeight - 3
            : baseline - peak * maxHeight - 3;
          context.beginPath();
          context.roundRect(x, peakY, barWidth, 1.5, 0.75);
          context.fill();
        }
      }
      context.globalAlpha = 1;
      context.shadowBlur = 0;
    };

    const tick = (): void => {
      for (let index = 0; index < BAND_COUNT; index += 1) {
        const value = at(current, index) + (at(target, index) - at(current, index)) * EASE;
        current[index] = value;
        if (at(peaks, index) > value) {
          peaks[index] = Math.max(value, at(peaks, index) - PEAK_FALL);
        }
      }
      draw();
      if (!disposed) {
        frameId = window.requestAnimationFrame(tick);
      }
    };

    const frame = latestSpectrum();
    if (frame !== null) {
      applyFrame(frame);
      current.set(target);
      peaks.set(target);
    }

    resize();
    const observer = new ResizeObserver(() => {
      resize();
      draw();
    });
    observer.observe(canvas);

    if (reduce) {
      // Static: one paint per arriving frame, no animation loop.
      draw();
    } else {
      frameId = window.requestAnimationFrame(tick);
    }

    const release = acquireSpectrum();
    const unsubscribe = subscribeSpectrum((next) => {
      applyFrame(next);
      if (reduce) {
        current.set(target);
        for (let index = 0; index < BAND_COUNT; index += 1) {
          peaks[index] = at(target, index);
        }
        draw();
      }
    });

    // The palette changes when the album does. The CSS variables are written by
    // the shell in the same commit, so the re-read is deferred one frame.
    let paletteFrame = 0;
    const unsubscribePalette = useUiStore.subscribe((state, previous) => {
      if (state.paletteKey === previous.paletteKey) {
        return;
      }
      window.cancelAnimationFrame(paletteFrame);
      paletteFrame = window.requestAnimationFrame(() => {
        readPalette();
        draw();
      });
    });

    return () => {
      disposed = true;
      window.cancelAnimationFrame(frameId);
      observer.disconnect();
      unsubscribePalette();
      window.cancelAnimationFrame(paletteFrame);
      unsubscribe();
      release();
    };
  }, [mirrored]);

  return (
    <canvas
      ref={canvasRef}
      role="img"
      aria-label={label}
      className={cn("block h-full w-full", className)}
    />
  );
}
