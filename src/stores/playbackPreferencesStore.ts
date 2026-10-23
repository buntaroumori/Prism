import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

export type AlbumEndBehaviour = "stop" | "continue-library";

interface PlaybackPreferencesState {
  albumEnd: AlbumEndBehaviour;
  setAlbumEnd: (behaviour: AlbumEndBehaviour) => void;
}

/** Playback choices that belong to the listener rather than the output device. */
export const usePlaybackPreferencesStore = create<PlaybackPreferencesState>()(
  persist(
    (set) => ({
      albumEnd: "stop",
      setAlbumEnd: (albumEnd) => {
        set({ albumEnd });
      },
    }),
    {
      name: "prism.playback.preferences",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({ albumEnd: state.albumEnd }),
    },
  ),
);
