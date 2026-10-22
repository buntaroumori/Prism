import { ListMusic, Plus } from "lucide-react";
import { useState, type SyntheticEvent, type JSX } from "react";

import { CoverArt } from "@/components/ui/CoverArt";
import { Button } from "@/components/ui/Button";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { usePlaylistStore } from "@/stores/playlistStore";
import { useUiStore } from "@/stores/uiStore";

/** Every user-owned playlist. */
export function PlaylistsView(): JSX.Element {
  const playlists = usePlaylistStore((state) => state.playlists);
  const loading = usePlaylistStore((state) => state.loading);
  const error = usePlaylistStore((state) => state.error);
  const create = usePlaylistStore((state) => state.create);
  const openPlaylist = useUiStore((state) => state.openPlaylist);
  const openAt = useContextMenuStore((state) => state.openAt);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async (event: SyntheticEvent): Promise<void> => {
    event.preventDefault();
    if (!name.trim()) {
      return;
    }
    setBusy(true);
    const created = await create(name);
    setBusy(false);
    if (created !== null) {
      setName("");
      setCreating(false);
      openPlaylist(created.id);
    }
  };

  return (
    <div className="mx-auto w-full max-w-[1560px] px-8 pt-7 pb-12">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[26px] leading-none font-semibold tracking-[-0.03em] text-white">
            <ListMusic className="h-5 w-5 text-white/25" aria-hidden="true" />
            Playlists
          </h1>
          <p className="mt-1.5 text-[12.5px] text-white/40">
            Your own selections, saved in your library.
          </p>
        </div>
        <Button
          variant="primary"
          icon={Plus}
          onClick={() => {
            setCreating((value) => !value);
          }}
        >
          New playlist
        </Button>
      </header>

      {creating ? (
        <form onSubmit={(event) => void submit(event)} className="mt-5 flex flex-wrap gap-2">
          <input
            autoFocus
            value={name}
            onChange={(event) => {
              setName(event.target.value);
            }}
            maxLength={128}
            placeholder="Playlist name"
            aria-label="Playlist name"
            className="h-9 min-w-0 flex-1 rounded-xl border border-white/10 bg-white/[0.04] px-3 text-[12.5px] text-white placeholder:text-white/25 focus:border-accent/40 focus:outline-none"
          />
          <Button type="submit" variant="secondary" loading={busy} disabled={!name.trim()}>
            Create
          </Button>
        </form>
      ) : null}

      {error !== null ? (
        <p role="alert" className="mt-4 text-[12px] text-critical">
          {error}
        </p>
      ) : null}

      {playlists.length === 0 && !loading ? (
        <div className="mt-10 rounded-2xl border border-dashed border-white/[0.09] px-6 py-16 text-center">
          <ListMusic className="mx-auto h-7 w-7 text-white/20" aria-hidden="true" />
          <p className="mt-3 text-[13px] font-medium text-white/55">No playlists yet</p>
          <p className="mx-auto mt-1 max-w-sm text-[11.5px] leading-relaxed text-white/30">
            Create one here, or right-click any song and choose “Add to playlist”.
          </p>
        </div>
      ) : (
        <div className="mt-7 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {playlists.map((playlist) => (
            <button
              key={playlist.id}
              type="button"
              onClick={() => {
                openPlaylist(playlist.id);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                openAt(event.clientX, event.clientY, { kind: "playlist", playlist });
              }}
              className="group flex items-center gap-4 rounded-2xl border border-white/[0.07] bg-white/[0.025] p-3 text-left transition-colors hover:border-white/[0.14] hover:bg-white/[0.045]"
            >
              <CoverArt
                path={playlist.coverPath}
                url={playlist.coverUrl}
                alt={playlist.name}
                className="h-[76px] w-[76px] shrink-0"
                rounded="rounded-xl"
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[14px] font-medium text-white/90 group-hover:text-white">
                  {playlist.name}
                </span>
                <span className="tabular mt-1 block text-[11px] text-white/35">
                  {formatCount(playlist.trackCount)} songs ·{" "}
                  {formatLibraryDuration(playlist.durationMs)}
                </span>
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
