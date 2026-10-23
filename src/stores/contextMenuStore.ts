import { create } from "zustand";

import type { LibraryContinuation, QueueEntry } from "@/stores/playbackStore";
import type { AlbumRow, PlaylistRow, TrackRow } from "@/types/ipc";

export type ContextTarget =
  | { kind: "album"; album: AlbumRow; continuation?: LibraryContinuation }
  | { kind: "track"; track: TrackRow; playlistId?: string }
  | { kind: "playlist"; playlist: PlaylistRow }
  | { kind: "queue-track"; entry: QueueEntry }
  | {
      kind: "queue-album";
      groupId: string;
      firstQueueId: string;
      title: string;
      artist: string;
    }
  | { kind: "dock-album"; album: AlbumRow }
  | { kind: "dock-playlist"; playlist: PlaylistRow };

interface ContextMenuState {
  menu: { x: number; y: number; target: ContextTarget } | null;
  openAt: (x: number, y: number, target: ContextTarget) => void;
  close: () => void;
}

/** One application-owned menu; browser context menus are disabled globally. */
export const useContextMenuStore = create<ContextMenuState>()((set) => ({
  menu: null,
  openAt: (x, y, target) => {
    set({ menu: { x, y, target } });
  },
  close: () => {
    set({ menu: null });
  },
}));
