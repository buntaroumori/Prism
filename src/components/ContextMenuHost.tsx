import { Disc3, Info, ListPlus, Play, Plus, Trash2, X } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type SyntheticEvent,
  type JSX,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

import { CoverArt } from "@/components/ui/CoverArt";
import { Button } from "@/components/ui/Button";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { usePlaylistStore } from "@/stores/playlistStore";
import { useUiStore } from "@/stores/uiStore";
import type { AlbumDetail, AlbumRow, PlaylistRow, TrackRow } from "@/types/ipc";

/** The only context menu and metadata dialogs in the desktop shell. */
export function ContextMenuHost(): JSX.Element {
  const menu = useContextMenuStore((state) => state.menu);
  const close = useContextMenuStore((state) => state.close);
  const target = menu?.target ?? null;
  const playlists = usePlaylistStore((state) => state.playlists);
  const addTracks = usePlaylistStore((state) => state.addTracks);
  const removeTrack = usePlaylistStore((state) => state.removeTrack);
  const deletePlaylist = usePlaylistStore((state) => state.deletePlaylist);
  const error = usePlaylistStore((state) => state.error);
  const notice = usePlaylistStore((state) => state.notice);
  const clearNotice = usePlaylistStore((state) => state.clearNotice);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const openAlbum = useUiStore((state) => state.openAlbum);
  const openPlaylist = useUiStore((state) => state.openPlaylist);
  const setRoute = useUiStore((state) => state.setRoute);

  const menuRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const [details, setDetails] = useState<AlbumDetail | null>(null);
  const [detailsAlbum, setDetailsAlbum] = useState<AlbumRow | null>(null);
  const [detailsError, setDetailsError] = useState<string | null>(null);
  const [creatingFor, setCreatingFor] = useState<TrackRow | null>(null);
  const [creatingName, setCreatingName] = useState("");
  const [creatingBusy, setCreatingBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<PlaylistRow | null>(null);
  const create = usePlaylistStore((state) => state.create);

  useLayoutEffect(() => {
    if (menu === null) {
      return;
    }
    const width = menuRef.current?.offsetWidth ?? 280;
    const height = menuRef.current?.offsetHeight ?? 360;
    setPosition({
      x: Math.max(8, Math.min(menu.x, window.innerWidth - width - 8)),
      y: Math.max(8, Math.min(menu.y, window.innerHeight - height - 8)),
    });
  }, [menu]);

  useEffect(() => {
    if (menu === null) {
      return;
    }
    const outside = (event: PointerEvent): void => {
      if (menuRef.current !== null && !menuRef.current.contains(event.target as Node)) {
        close();
      }
    };
    const key = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        close();
      }
    };
    window.addEventListener("pointerdown", outside);
    window.addEventListener("keydown", key);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("pointerdown", outside);
      window.removeEventListener("keydown", key);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [close, menu]);

  useEffect(() => {
    if (notice === null) {
      return;
    }
    const timer = window.setTimeout(clearNotice, 3200);
    return () => {
      window.clearTimeout(timer);
    };
  }, [clearNotice, notice]);

  const showDetails = (album: AlbumRow): void => {
    close();
    setDetailsAlbum(album);
    setDetails(null);
    setDetailsError(null);
    void libraryApi
      .albumDetail(album.id)
      .then(setDetails)
      .catch((cause: unknown) => {
        setDetailsError(cause instanceof Error ? cause.message : "Details could not be loaded");
      });
  };

  const playAlbum = (album: AlbumRow): void => {
    close();
    void libraryApi.search({ albumId: album.id, sort: "album", limit: 500 }).then((page) => {
      if (page.items.length > 0) {
        void playTracks(page.items, 0);
      }
    });
  };

  const createAndAdd = async (event: SyntheticEvent): Promise<void> => {
    event.preventDefault();
    if (creatingFor === null || !creatingName.trim()) {
      return;
    }
    setCreatingBusy(true);
    const playlist = await create(creatingName);
    if (playlist !== null) {
      await addTracks(playlist.id, [creatingFor.id]);
    }
    setCreatingBusy(false);
    if (playlist !== null) {
      setCreatingFor(null);
      setCreatingName("");
    }
  };

  return createPortal(
    <>
      {menu !== null ? (
        <div
          ref={menuRef}
          role="menu"
          aria-label="Prism actions"
          onContextMenu={(event) => {
            event.preventDefault();
          }}
          className="glass fixed z-[90] max-h-[min(560px,calc(100vh-16px))] w-[280px] overflow-y-auto rounded-2xl p-1.5 shadow-2xl"
          style={{ left: position.x, top: position.y }}
        >
          {target?.kind === "album" ? (
            <>
              <MenuHeader title={target.album.title} subtitle={target.album.albumArtist} />
              <MenuItem
                icon={Disc3}
                label="Open album"
                onClick={() => {
                  openAlbum(target.album.id);
                  close();
                }}
              />
              <MenuItem
                icon={Play}
                label="Play album"
                onClick={() => {
                  playAlbum(target.album);
                }}
              />
              <MenuItem
                icon={Info}
                label="Album details"
                onClick={() => {
                  showDetails(target.album);
                }}
              />
            </>
          ) : null}

          {target?.kind === "track" ? (
            <>
              <MenuHeader title={target.track.title} subtitle={target.track.artist} />
              <MenuItem
                icon={Play}
                label="Play now"
                onClick={() => {
                  void playTracks([target.track], 0);
                  close();
                }}
              />
              {target.track.albumId !== null ? (
                <MenuItem
                  icon={Disc3}
                  label="Open album"
                  onClick={() => {
                    const albumId = target.track.albumId;
                    if (albumId !== null) {
                      openAlbum(albumId);
                    }
                    close();
                  }}
                />
              ) : null}
              <div className="mx-2 my-1.5 h-px bg-white/[0.07]" />
              <p className="flex items-center gap-2 px-2.5 py-1.5 text-[10px] font-semibold tracking-[0.12em] text-white/30 uppercase">
                <ListPlus className="h-3.5 w-3.5" aria-hidden="true" />
                Add to playlist
              </p>
              <div className="max-h-[190px] overflow-y-auto">
                {playlists.map((playlist) => (
                  <MenuItem
                    key={playlist.id}
                    icon={ListPlus}
                    label={playlist.name}
                    onClick={() => {
                      void addTracks(playlist.id, [target.track.id]);
                      close();
                    }}
                  />
                ))}
              </div>
              <MenuItem
                icon={Plus}
                label="New playlist…"
                onClick={() => {
                  setCreatingFor(target.track);
                  setCreatingName("");
                  close();
                }}
              />
              {target.playlistId !== undefined ? (
                <MenuItem
                  icon={X}
                  label="Remove from this playlist"
                  onClick={() => {
                    void removeTrack(target.playlistId ?? "", target.track.id);
                    close();
                  }}
                />
              ) : null}
            </>
          ) : null}

          {target?.kind === "playlist" ? (
            <>
              <MenuHeader
                title={target.playlist.name}
                subtitle={`${formatCount(target.playlist.trackCount)} songs`}
              />
              <MenuItem
                icon={ListPlus}
                label="Open playlist"
                onClick={() => {
                  openPlaylist(target.playlist.id);
                  close();
                }}
              />
              <MenuItem
                icon={Trash2}
                label="Delete playlist"
                danger
                onClick={() => {
                  setConfirmDelete(target.playlist);
                  close();
                }}
              />
            </>
          ) : null}
        </div>
      ) : null}

      {detailsAlbum !== null ? (
        <Dialog
          label={`Details for ${detailsAlbum.title}`}
          onClose={() => {
            setDetailsAlbum(null);
            setDetails(null);
          }}
        >
          <div className="flex items-start justify-between gap-4">
            <div className="flex min-w-0 items-center gap-4">
              <CoverArt
                path={detailsAlbum.coverPath}
                url={detailsAlbum.coverUrl}
                alt={detailsAlbum.title}
                className="h-20 w-20 shrink-0"
                rounded="rounded-xl"
              />
              <div className="min-w-0">
                <p className="text-[10px] font-semibold tracking-[0.16em] text-white/30 uppercase">
                  Album details
                </p>
                <h2 className="mt-1 text-[21px] leading-tight font-semibold text-white">
                  {detailsAlbum.title}
                </h2>
                <p className="mt-1 text-[12.5px] text-white/45">{detailsAlbum.albumArtist}</p>
              </div>
            </div>
            <button
              type="button"
              aria-label="Close details"
              onClick={() => {
                setDetailsAlbum(null);
                setDetails(null);
              }}
              className="grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/40 hover:bg-white/[0.06] hover:text-white"
            >
              <X className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>

          {detailsError !== null ? (
            <p role="alert" className="mt-5 text-[12px] text-critical">
              {detailsError}
            </p>
          ) : details === null ? (
            <p className="mt-6 text-[12px] text-white/35">Reading details…</p>
          ) : (
            <dl className="mt-6 grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-3">
              <Detail label="Artist" value={details.album.albumArtist || "Unknown artist"} />
              <Detail label="Year" value={details.album.year?.toString() ?? "Unknown"} />
              <Detail label="Release" value={details.album.releaseType ?? "Album"} />
              <Detail label="Discs" value={formatCount(details.album.discCount)} />
              <Detail label="Songs" value={formatCount(details.album.trackCount)} />
              <Detail label="Length" value={formatLibraryDuration(details.album.durationMs)} />
              <Detail
                label="Genres"
                value={unique(details.tracks.items.map((track) => track.genre)).join(", ") || "—"}
                wide
              />
              <Detail label="Audio" value={details.album.quality} />
              <Detail
                label="Formats"
                value={unique(details.tracks.items.map((track) => track.format.toUpperCase())).join(
                  ", ",
                )}
              />
            </dl>
          )}
        </Dialog>
      ) : null}

      {creatingFor !== null ? (
        <Dialog
          label="Create a playlist"
          onClose={() => {
            if (!creatingBusy) {
              setCreatingFor(null);
            }
          }}
        >
          <h2 className="text-[20px] font-semibold text-white">New playlist</h2>
          <p className="mt-1 truncate text-[12px] text-white/40">Add “{creatingFor.title}” to:</p>
          <form onSubmit={(event) => void createAndAdd(event)} className="mt-5 space-y-3">
            <input
              autoFocus
              value={creatingName}
              onChange={(event) => {
                setCreatingName(event.target.value);
              }}
              maxLength={128}
              aria-label="New playlist name"
              placeholder="Playlist name"
              className="h-10 w-full rounded-xl border border-white/10 bg-white/[0.04] px-3 text-[13px] text-white placeholder:text-white/25 focus:border-accent/40 focus:outline-none"
            />
            {error !== null ? <p className="text-[11.5px] text-critical">{error}</p> : null}
            <div className="flex justify-end gap-2">
              <Button
                variant="ghost"
                disabled={creatingBusy}
                onClick={() => {
                  setCreatingFor(null);
                }}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                variant="primary"
                loading={creatingBusy}
                disabled={!creatingName.trim()}
              >
                Create and add
              </Button>
            </div>
          </form>
        </Dialog>
      ) : null}

      {confirmDelete !== null ? (
        <Dialog
          label="Delete playlist"
          onClose={() => {
            setConfirmDelete(null);
          }}
        >
          <h2 className="text-[20px] font-semibold text-white">Delete “{confirmDelete.name}”?</h2>
          <p className="mt-2 text-[12.5px] leading-relaxed text-white/40">
            Only the playlist will be removed. Your music stays in the library.
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setConfirmDelete(null);
              }}
            >
              Cancel
            </Button>
            <Button
              variant="secondary"
              icon={Trash2}
              onClick={() => {
                const id = confirmDelete.id;
                void deletePlaylist(id).then((removed) => {
                  if (removed && useUiStore.getState().params.playlistId === id) {
                    setRoute("playlists");
                  }
                });
                setConfirmDelete(null);
              }}
            >
              Delete
            </Button>
          </div>
        </Dialog>
      ) : null}

      {notice !== null ? (
        <div
          role="status"
          className="glass fixed bottom-[104px] left-1/2 z-[110] -translate-x-1/2 rounded-full px-4 py-2 text-[12px] text-white/80 shadow-xl"
        >
          {notice}
        </div>
      ) : null}
    </>,
    document.body,
  );
}

function MenuHeader({ title, subtitle }: { title: string; subtitle: string }): JSX.Element {
  return (
    <div className="mb-1 border-b border-white/[0.07] px-2.5 pt-2 pb-2.5">
      <p className="truncate text-[12px] font-medium text-white/85">{title}</p>
      <p className="truncate text-[10.5px] text-white/35">{subtitle}</p>
    </div>
  );
}

function MenuItem({
  icon: Icon,
  label,
  onClick,
  danger = false,
}: {
  icon: typeof Play;
  label: string;
  onClick: () => void;
  danger?: boolean;
}): JSX.Element {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className={
        danger
          ? "flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left text-[12px] text-critical/80 hover:bg-critical/[0.08] hover:text-critical"
          : "flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left text-[12px] text-white/65 hover:bg-white/[0.06] hover:text-white"
      }
    >
      <Icon className="h-3.5 w-3.5 shrink-0 opacity-70" aria-hidden="true" />
      <span className="truncate">{label}</span>
    </button>
  );
}

function Dialog({
  label,
  onClose,
  children,
}: {
  label: string;
  onClose: () => void;
  children: ReactNode;
}): JSX.Element {
  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-6"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={label}
        className="glass max-h-[85vh] w-full max-w-[620px] overflow-y-auto rounded-3xl border-white/[0.1] bg-oled-950/95 p-6 shadow-2xl"
        onContextMenu={(event) => {
          event.preventDefault();
        }}
      >
        {children}
      </div>
    </div>
  );
}

function Detail({ label, value, wide = false }: { label: string; value: string; wide?: boolean }) {
  return (
    <div className={wide ? "col-span-2 sm:col-span-3" : ""}>
      <dt className="text-[9.5px] font-semibold tracking-[0.14em] text-white/25 uppercase">
        {label}
      </dt>
      <dd className="mt-1 text-[12.5px] break-words text-white/75">{value}</dd>
    </div>
  );
}

function unique(values: string[]): string[] {
  return [...new Set(values.map((value) => value.trim()).filter(Boolean))].sort();
}
