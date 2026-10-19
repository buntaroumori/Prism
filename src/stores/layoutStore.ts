import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/**
 * Now Playing modules.
 *
 * The listening surface is composed, not hard-coded: every module below is a
 * self-contained panel that can be shown, hidden and reordered independently.
 * The order in this type is the order used by the preset definitions and by the
 * settings UI.
 */
export type NowPlayingModule = "artwork" | "lyrics" | "spectrum" | "technical" | "queue";

export type NowPlayingPreset = "editorial" | "listening" | "analysis" | "custom";

/** Every module, in the order the configuration list shows them. */
export const NOW_PLAYING_MODULES: readonly NowPlayingModule[] = [
  "artwork",
  "lyrics",
  "spectrum",
  "technical",
  "queue",
];

export const MODULE_LABELS: Record<NowPlayingModule, string> = {
  artwork: "Artwork",
  lyrics: "Lyrics",
  spectrum: "Spectrum",
  technical: "Signal path",
  queue: "Queue",
};

/** Presets are opinionated starting points, not modes: editing one makes it custom. */
export const PRESETS: Record<Exclude<NowPlayingPreset, "custom">, NowPlayingModule[]> = {
  editorial: ["artwork", "technical", "lyrics"],
  listening: ["artwork", "lyrics", "spectrum"],
  analysis: ["spectrum", "technical", "queue"],
};

const DEFAULT_MODULES = PRESETS.editorial;

interface LayoutState {
  preset: NowPlayingPreset;
  modules: NowPlayingModule[];
  setPreset: (preset: Exclude<NowPlayingPreset, "custom">) => void;
  toggleModule: (module: NowPlayingModule) => void;
  moveModule: (module: NowPlayingModule, direction: -1 | 1) => void;
  reset: () => void;
}

/**
 * Which modules the Now Playing surface renders, in order.
 *
 * Persisted separately from the shell preferences so a layout experiment cannot
 * collapse the sidebar or change the route. The store guarantees at least one
 * module stays visible: an empty deck would leave the page looking broken with no
 * way back.
 */
export const useLayoutStore = create<LayoutState>()(
  persist(
    (set, get) => ({
      preset: "editorial",
      modules: [...DEFAULT_MODULES],

      setPreset: (preset) => {
        set({ preset, modules: [...PRESETS[preset]] });
      },

      toggleModule: (module) => {
        const { modules } = get();
        if (modules.includes(module)) {
          if (modules.length === 1) {
            return;
          }
          set({ preset: "custom", modules: modules.filter((entry) => entry !== module) });
        } else {
          set({ preset: "custom", modules: [...modules, module] });
        }
      },

      moveModule: (module, direction) => {
        const { modules } = get();
        const index = modules.indexOf(module);
        const target = index + direction;
        if (index < 0 || target < 0 || target >= modules.length) {
          return;
        }
        const next = [...modules];
        next.splice(index, 1);
        next.splice(target, 0, module);
        set({ preset: "custom", modules: next });
      },

      reset: () => {
        set({ preset: "editorial", modules: [...DEFAULT_MODULES] });
      },
    }),
    {
      name: "prism.layout.now-playing",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({ preset: state.preset, modules: state.modules }),
      // A stored layout from a future/older build may name modules that no
      // longer exist: keep only known ids, and never persist an empty deck.
      migrate: (persisted) => {
        const candidate = persisted as Partial<LayoutState> | null;
        const known: NowPlayingModule[] = ["artwork", "lyrics", "spectrum", "technical", "queue"];
        const modules = (candidate?.modules ?? []).filter((module) => known.includes(module));
        if (modules.length === 0) {
          return { preset: "editorial", modules: [...DEFAULT_MODULES] };
        }
        return {
          preset: candidate?.preset ?? "custom",
          modules,
        };
      },
    },
  ),
);
