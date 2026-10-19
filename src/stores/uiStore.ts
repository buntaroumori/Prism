import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** Anchor targets of the single-page diagnostics shell. */
export type ShellSection = "overview" | "library" | "database" | "runtime";

interface UiState {
  sidebarCollapsed: boolean;
  activeSection: ShellSection;
  toggleSidebar: () => void;
  setActiveSection: (section: ShellSection) => void;
}

/**
 * Persisted shell preferences.
 *
 * Only layout state lives here. Library, playback and DSP state will get
 * separate stores in FASE 4 so that a sidebar toggle can never invalidate the
 * 500k-row track list.
 */
export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      sidebarCollapsed: false,
      activeSection: "overview",
      toggleSidebar: () => {
        set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed }));
      },
      setActiveSection: (section) => {
        set({ activeSection: section });
      },
    }),
    {
      name: "prism.shell.preferences",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        sidebarCollapsed: state.sidebarCollapsed,
        activeSection: state.activeSection,
      }),
    },
  ),
);
