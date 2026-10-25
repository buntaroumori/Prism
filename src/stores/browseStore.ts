import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import type { AlbumSort } from "@/types/ipc";

export type AlbumSortField = "artist" | "title" | "year";

interface BrowseState {
  sort: AlbumSort;
  genre: string | null;
  /** First year of the selected decade, e.g. 1990 for the 1990s. */
  decade: number | null;
  /** Optional exact year inside the selected decade. */
  year: number | null;
  cycleSort: (field: AlbumSortField) => void;
  setGenre: (genre: string | null) => void;
  setDecade: (decade: number | null) => void;
  setYear: (year: number | null) => void;
}

/** Library choices survive route changes, rescans and application restarts. */
export const useBrowseStore = create<BrowseState>()(
  persist(
    (set) => ({
      sort: "artist",
      genre: null,
      decade: null,
      year: null,

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

      setDecade: (decade) => {
        set({ decade, year: null });
      },

      setYear: (year) => {
        set((state) => ({ year: state.decade === null ? null : year }));
      },
    }),
    {
      name: "prism.library.browse",
      version: 2,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        sort: state.sort,
        genre: state.genre,
        decade: state.decade,
        year: state.year,
      }),
      migrate: (persisted) => ({
        sort: "artist",
        genre: null,
        decade: null,
        year: null,
        ...(persisted as object),
      }),
    },
  ),
);
