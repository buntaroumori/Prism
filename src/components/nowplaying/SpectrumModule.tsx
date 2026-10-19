import { useEffect, useState, type JSX } from "react";
import { Activity } from "lucide-react";

import { SpectrumVisualiser } from "@/components/player/SpectrumVisualiser";
import { cn } from "@/lib/utils";
import { subscribeSpectrum } from "@/lib/spectrum";

/**
 * Spectrum analyser card.
 *
 * The canvas does the drawing; this wrapper adds the read-out (broadband level
 * and the source rate) and keeps its own re-render rate low: the level text is
 * sampled at 5 Hz from the same frame stream instead of on every frame.
 */
export function SpectrumModule({ sampleRate }: { sampleRate: number | null }): JSX.Element {
  const [level, setLevel] = useState(0);

  useEffect(() => {
    let last = 0;
    return subscribeSpectrum((frame) => {
      const now = performance.now();
      if (now - last < 200) {
        return;
      }
      last = now;
      setLevel(frame.level);
    });
  }, []);

  return (
    <div className="flex h-full flex-col rounded-3xl border border-white/[0.06] bg-white/[0.02] p-5 backdrop-blur-xl">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Activity className="h-3.5 w-3.5 text-accent-soft" aria-hidden="true" />
          <h3 className="text-[11px] font-semibold tracking-[0.14em] text-white/55 uppercase">
            Spectrum
          </h3>
        </div>
        <span className="tabular text-[10px] text-white/25">
          20 Hz – 20 kHz · 31 bands
          {sampleRate !== null ? ` · ${(sampleRate / 1000).toFixed(1)} kHz` : ""}
        </span>
      </div>

      <div className="relative mt-3 min-h-[120px] flex-1">
        <SpectrumVisualiser mirrored label="Real-time spectrum analyser" />
      </div>

      <div className="mt-3 flex items-center gap-3">
        <div className="h-1 flex-1 overflow-hidden rounded-full bg-white/[0.06]">
          <div
            className={cn("h-full rounded-full bg-accent transition-[width] duration-100")}
            style={{ width: `${String(Math.round(level * 100))}%` }}
          />
        </div>
        <span className="tabular w-9 text-right text-[10px] text-white/30">
          {String(Math.round(level * 100)).padStart(2, "0")}%
        </span>
      </div>
    </div>
  );
}
