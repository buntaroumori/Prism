import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/**
 * Top-level destinations.
 *
 * The library, album and artist browsers arrive in FASE 5 with the virtualised
 * grid, so they are not routes yet: a navigation entry that renders a "coming
 * soon" panel is exactly the placeholder this project forbids. Every route
 * declared here is fully implemented.
 */
export type ShellRoute = "home" | "now-playing" | "settings" | "system";

/** Ambient canvas intensity, persisted per user. */
export type AmbientIntensity = "off" | "subtle" | "full";

interface UiState {
  sidebarCollapsed: boolean;
  route: ShellRoute;
  ambientIntensity: AmbientIntensity;
  /** Palette key currently driving the shell (`null` = neutral). */
  paletteKey: string | null;
  toggleSidebar: () => void;
  setRoute: (route: ShellRoute) => void;
  setAmbientIntensity: (intensity: AmbientIntensity) => void;
  setPaletteKey: (key: string | null) => void;
}

/**
 * Persisted shell preferences.
 *
 * Only layout lives here. Playback, library data and the playhead have their own
 * stores so that toggling the sidebar can never invalidate a 500k-row list.
 */
export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      route: "home",
      ambientIntensity: "full",
      paletteKey: null,
      toggleSidebar: () => {
        set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed }));
      },
      setRoute: (route) => {
        set({ route });
      },
      setAmbientIntensity: (ambientIntensity) => {
        set({ ambientIntensity });
      },
      setPaletteKey: (paletteKey) => {
        set({ paletteKey });
      },
    }),
    {
      name: "prism.shell.preferences",
      version: 2,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        sidebarCollapsed: state.sidebarCollapsed,
        route: state.route,
        ambientIntensity: state.ambientIntensity,
      }),
      // v1 stored `activeSection` anchored sections; v2 is a real router.
      migrate: (persisted, version) => {
        if (version < 2) {
          return { sidebarCollapsed: false, route: "home", ambientIntensity: "full" };
        }
        return persisted;
      },
    },
  ),
);
