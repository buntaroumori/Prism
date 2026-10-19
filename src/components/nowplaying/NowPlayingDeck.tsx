import { motion, useReducedMotion } from "framer-motion";
import type { JSX } from "react";
import { LayoutGrid, RotateCcw } from "lucide-react";

import { ArtworkModule } from "@/components/nowplaying/ArtworkModule";
import { QueueModule } from "@/components/nowplaying/QueueModule";
import { SpectrumModule } from "@/components/nowplaying/SpectrumModule";
import { TechnicalModule } from "@/components/nowplaying/TechnicalModule";
import { LyricsPanel } from "@/components/player/LyricsPanel";
import { Popover } from "@/components/ui/Popover";
import { cn } from "@/lib/utils";
import {
  MODULE_LABELS,
  NOW_PLAYING_MODULES,
  PRESETS,
  useLayoutStore,
  type NowPlayingModule,
  type NowPlayingPreset,
} from "@/stores/layoutStore";
import type { PlayerStatus } from "@/types/ipc";

const PRESET_OPTIONS: readonly { value: Exclude<NowPlayingPreset, "custom">; label: string }[] = [
  { value: "editorial", label: "Editorial" },
  { value: "listening", label: "Listening" },
  { value: "analysis", label: "Analysis" },
];

/** Column footprint of each module on the twelve-column desktop grid. */
const SPAN: Record<NowPlayingModule, string> = {
  artwork: "lg:col-span-7",
  lyrics: "lg:col-span-5",
  spectrum: "lg:col-span-7",
  technical: "lg:col-span-12",
  queue: "lg:col-span-12",
};

interface NowPlayingDeckProps {
  status: PlayerStatus | null;
  trackId: string | null;
  album: string;
  title: string;
}

/**
 * Modular Now Playing surface.
 *
 * The deck renders whatever `useLayoutStore` says, in the stored order. Modules
 * are real panels with their own data source — artwork, lyrics, spectrum, signal
 * path and queue — so removing one removes its work: the spectrum stops the
 * audio-thread tap when it unmounts, lyrics stop fetching when they leave the
 * tree. The layout itself animates with Framer Motion `layout`, which moves
 * boxes with transforms and costs nothing on the main thread.
 */
export function NowPlayingDeck({
  status,
  trackId,
  album,
  title,
}: NowPlayingDeckProps): JSX.Element {
  const modules = useLayoutStore((state) => state.modules);
  const preset = useLayoutStore((state) => state.preset);
  const setPreset = useLayoutStore((state) => state.setPreset);
  const toggleModule = useLayoutStore((state) => state.toggleModule);
  const moveModule = useLayoutStore((state) => state.moveModule);
  const reset = useLayoutStore((state) => state.reset);
  const reduce = useReducedMotion();

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <p className="text-[10px] font-medium tracking-[0.18em] text-white/25 uppercase">
          Layout · {preset === "custom" ? "Custom" : preset}
        </p>
        <div className="flex items-center gap-1">
          {PRESET_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              onClick={() => {
                setPreset(option.value);
              }}
              aria-pressed={preset === option.value}
              className={cn(
                "rounded-full px-3 py-1 text-[11px] transition-colors",
                preset === option.value
                  ? "bg-white/[0.09] text-white"
                  : "text-white/35 hover:bg-white/[0.05] hover:text-white/70",
              )}
            >
              {option.label}
            </button>
          ))}

          <Popover
            align="end"
            side="bottom"
            label="Configure modules"
            className="w-64"
            trigger={({ open, toggle }) => (
              <button
                type="button"
                onClick={toggle}
                aria-label="Configure modules"
                aria-expanded={open}
                className="ml-1 grid h-7 w-7 place-items-center rounded-full text-white/35 transition-colors hover:bg-white/[0.06] hover:text-white/80"
              >
                <LayoutGrid className="h-3.5 w-3.5" aria-hidden="true" />
              </button>
            )}
          >
            <div className="p-3">
              <p className="px-1 pb-2 text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase">
                Modules
              </p>
              <ul className="space-y-0.5">
                {NOW_PLAYING_MODULES.map((module) => {
                  const visible = modules.includes(module);
                  return (
                    <li key={module} className="flex items-center gap-1">
                      <button
                        type="button"
                        onClick={() => {
                          toggleModule(module);
                        }}
                        aria-pressed={visible}
                        className="flex flex-1 items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[12px] text-white/65 hover:bg-white/[0.05]"
                      >
                        <span
                          aria-hidden="true"
                          className={cn(
                            "h-1.5 w-1.5 rounded-full",
                            visible ? "bg-accent" : "bg-white/15",
                          )}
                        />
                        {MODULE_LABELS[module]}
                      </button>
                      <button
                        type="button"
                        disabled={!visible}
                        onClick={() => {
                          moveModule(module, -1);
                        }}
                        aria-label={`Move ${MODULE_LABELS[module]} up`}
                        className="grid h-6 w-5 place-items-center rounded text-white/25 hover:bg-white/[0.05] hover:text-white/70 disabled:opacity-25"
                      >
                        ↑
                      </button>
                      <button
                        type="button"
                        disabled={!visible}
                        onClick={() => {
                          moveModule(module, 1);
                        }}
                        aria-label={`Move ${MODULE_LABELS[module]} down`}
                        className="grid h-6 w-5 place-items-center rounded text-white/25 hover:bg-white/[0.05] hover:text-white/70 disabled:opacity-25"
                      >
                        ↓
                      </button>
                    </li>
                  );
                })}
              </ul>
              <button
                type="button"
                onClick={reset}
                className="mt-2 flex w-full items-center justify-center gap-1.5 rounded-lg border border-white/[0.07] px-2 py-1.5 text-[11px] text-white/40 hover:bg-white/[0.04] hover:text-white/70"
              >
                <RotateCcw className="h-3 w-3" aria-hidden="true" />
                Reset to Editorial
              </button>
              <p className="mt-2 px-1 text-[10px] leading-relaxed text-white/20">
                {PRESETS.editorial.length} modules in the default preset. Hidden modules stop
                running: no canvas, no lyrics fetch, no spectrum tap.
              </p>
            </div>
          </Popover>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-5 lg:grid-flow-row-dense lg:grid-cols-12">
        {modules.map((module) => (
          <motion.section
            key={module}
            layout={reduce !== true}
            transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
            className={cn("min-w-0", SPAN[module])}
          >
            {module === "artwork" ? (
              <div className="rounded-3xl border border-white/[0.05] bg-white/[0.015] p-5">
                <ArtworkModule trackId={trackId} album={album} title={title} />
              </div>
            ) : null}
            {module === "lyrics" ? (
              <div className="h-[420px]">
                <LyricsPanel />
              </div>
            ) : null}
            {module === "spectrum" ? (
              <div className="h-[200px]">
                <SpectrumModule sampleRate={status?.sourceFormat?.sampleRate ?? null} />
              </div>
            ) : null}
            {module === "technical" ? <TechnicalModule status={status} /> : null}
            {module === "queue" ? <QueueModule /> : null}
          </motion.section>
        ))}
      </div>
    </div>
  );
}
