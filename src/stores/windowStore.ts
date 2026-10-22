import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import { windowApi } from "@/lib/ipc";

interface WindowState {
  fullscreen: boolean;
  error: string | null;
  restore: () => Promise<void>;
  setFullscreen: (fullscreen: boolean) => Promise<void>;
  toggleFullscreen: () => Promise<void>;
}

/** Borderless, non-exclusive fullscreen; Alt-Tab remains an OS operation. */
export const useWindowStore = create<WindowState>()(
  persist(
    (set, get) => ({
      fullscreen: false,
      error: null,

      restore: async () => {
        if (!get().fullscreen) {
          return;
        }
        try {
          await windowApi.setFullscreen(true);
        } catch (cause) {
          set({
            fullscreen: false,
            error: cause instanceof Error ? cause.message : "Fullscreen could not be restored",
          });
        }
      },

      setFullscreen: async (fullscreen) => {
        try {
          await windowApi.setFullscreen(fullscreen);
          set({ fullscreen, error: null });
        } catch (cause) {
          set({
            error: cause instanceof Error ? cause.message : "Fullscreen could not be changed",
          });
        }
      },

      toggleFullscreen: async () => {
        await get().setFullscreen(!get().fullscreen);
      },
    }),
    {
      name: "prism.window",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({ fullscreen: state.fullscreen }),
    },
  ),
);
