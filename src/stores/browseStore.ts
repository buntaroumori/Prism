import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import type { AlbumSort } from "@/types/ipc";

export type AlbumSortField = "artist" | "title" | "year";

interface BrowseState {
  sort: AlbumSort;
  genre: string | null;
  cycleSort: (field: AlbumSortField) => void;
  setGenre: (genre: string | null) => void;
}

/** Library choices survive route changes, rescans and application restarts. */
export const useBrowseStore = create<BrowseState>()(
  persist(
    (set) => ({
      sort: "artist",
      genre: null,

      cycleSort: (field) => {
        set((state) => {
          if (field === "artist") {
            return { sort: state.sort === "artist" ? "artistDesc" : "artist" };
          }
          if (field === "title") {
            return { sort: state.sort === "title" ? "titleDesc" : "title" };
          }
          return { sort: state.sort === "year" ? "yearAsc" : "year" };
        });
      },

      setGenre: (genre) => {
        set({ genre: genre?.trim() ? genre : null });
      },
    }),
    {
      name: "prism.library.browse",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({ sort: state.sort, genre: state.genre }),
    },
  ),
);
