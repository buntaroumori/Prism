import { create } from "zustand";

import { playlistApi } from "@/lib/ipc";
import type { PlaylistRow } from "@/types/ipc";

interface PlaylistStore {
  playlists: PlaylistRow[];
  loading: boolean;
  error: string | null;
  notice: string | null;
  load: () => Promise<void>;
  create: (name: string) => Promise<PlaylistRow | null>;
  addTracks: (playlistId: string, trackIds: string[]) => Promise<boolean>;
  removeTrack: (playlistId: string, trackId: string) => Promise<boolean>;
  deletePlaylist: (id: string) => Promise<boolean>;
  clearNotice: () => void;
}

function message(cause: unknown, fallback: string): string {
  return cause instanceof Error ? cause.message : fallback;
}

function ordered(items: PlaylistRow[]): PlaylistRow[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name));
}

export const usePlaylistStore = create<PlaylistStore>()((set) => ({
  playlists: [],
  loading: false,
  error: null,
  notice: null,

  load: async () => {
    set({ loading: true });
    try {
      const playlists = await playlistApi.list();
      set({ playlists, loading: false, error: null });
    } catch (cause) {
      set({ loading: false, error: message(cause, "Playlists could not be loaded") });
    }
  },

  create: async (name) => {
    try {
      const playlist = await playlistApi.create(name);
      set((state) => ({
        playlists: ordered([...state.playlists, playlist]),
        error: null,
      }));
      return playlist;
    } catch (cause) {
      set({ error: message(cause, "The playlist could not be created") });
      return null;
    }
  },

  addTracks: async (playlistId, trackIds) => {
    try {
      const updated = await playlistApi.addTracks(playlistId, trackIds);
      set((state) => ({
        playlists: ordered(
          state.playlists.map((playlist) => (playlist.id === updated.id ? updated : playlist)),
        ),
        notice: `${trackIds.length === 1 ? "Song" : `${String(trackIds.length)} songs`} added to ${updated.name}`,
        error: null,
      }));
      return true;
    } catch (cause) {
      set({ error: message(cause, "The song could not be added") });
      return false;
    }
  },

  removeTrack: async (playlistId, trackId) => {
    try {
      const updated = await playlistApi.removeTrack(playlistId, trackId);
      set((state) => ({
        playlists: state.playlists.map((playlist) =>
          playlist.id === updated.id ? updated : playlist,
        ),
        error: null,
      }));
      return true;
    } catch (cause) {
      set({ error: message(cause, "The song could not be removed") });
      return false;
    }
  },

  deletePlaylist: async (id) => {
    try {
      const removed = await playlistApi.delete(id);
      if (removed) {
        set((state) => ({
          playlists: state.playlists.filter((playlist) => playlist.id !== id),
          error: null,
        }));
      }
      return removed;
    } catch (cause) {
      set({ error: message(cause, "The playlist could not be deleted") });
      return false;
    }
  },

  clearNotice: () => {
    set({ notice: null });
  },
}));
