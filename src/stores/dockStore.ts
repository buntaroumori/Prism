import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { z } from "zod";

import { AlbumRowSchema, PlaylistRowSchema, type AlbumRow, type PlaylistRow } from "@/types/ipc";

const DockItemSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("album"), album: AlbumRowSchema }),
  z.object({ kind: z.literal("playlist"), playlist: PlaylistRowSchema }),
]);

export type DockItem = z.infer<typeof DockItemSchema>;

const key = (kind: DockItem["kind"], id: string): string => `${kind}:${id}`;
const itemKey = (item: DockItem): string =>
  item.kind === "album" ? key("album", item.album.id) : key("playlist", item.playlist.id);

interface DockState {
  items: DockItem[];
  pinAlbum: (album: AlbumRow) => void;
  unpinAlbum: (id: string) => void;
  updateAlbum: (album: AlbumRow) => void;
  pinPlaylist: (playlist: PlaylistRow) => void;
  unpinPlaylist: (id: string) => void;
  updatePlaylist: (playlist: PlaylistRow) => void;
  reorder: (items: DockItem[]) => void;
}

export const useDockStore = create<DockState>()(
  persist(
    (set) => ({
      items: [],
      pinAlbum: (album) => {
        set((state) => ({
          items: [
            { kind: "album", album },
            ...state.items.filter((item) => itemKey(item) !== key("album", album.id)),
          ],
        }));
      },
      unpinAlbum: (id) => {
        set((state) => ({
          items: state.items.filter((item) => itemKey(item) !== key("album", id)),
        }));
      },
      updateAlbum: (album) => {
        set((state) => ({
          items: state.items.map((item) =>
            item.kind === "album" && item.album.id === album.id ? { kind: "album", album } : item,
          ),
        }));
      },
      pinPlaylist: (playlist) => {
        set((state) => ({
          items: [
            { kind: "playlist", playlist },
            ...state.items.filter((item) => itemKey(item) !== key("playlist", playlist.id)),
          ],
        }));
      },
      unpinPlaylist: (id) => {
        set((state) => ({
          items: state.items.filter((item) => itemKey(item) !== key("playlist", id)),
        }));
      },
      updatePlaylist: (playlist) => {
        set((state) => ({
          items: state.items.map((item) =>
            item.kind === "playlist" && item.playlist.id === playlist.id
              ? { kind: "playlist", playlist }
              : item,
          ),
        }));
      },
      reorder: (items) => {
        set((state) => {
          const byId = new Map(state.items.map((item) => [itemKey(item), item]));
          const ordered = items
            .map((item) => byId.get(itemKey(item)))
            .filter((item) => item !== undefined);
          const included = new Set(ordered.map(itemKey));
          return {
            items: [...ordered, ...state.items.filter((item) => !included.has(itemKey(item)))],
          };
        });
      },
    }),
    {
      name: "prism.dock",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({ items: state.items }),
      merge: (persisted, current) => {
        const parsed = z.object({ items: z.array(DockItemSchema).max(500) }).safeParse(persisted);
        return parsed.success ? { ...current, items: parsed.data.items } : current;
      },
    },
  ),
);
