import { ArrowLeft, ImagePlus, Pencil, Play, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState, type JSX } from "react";

import { TrackTable } from "@/components/library/TrackTable";
import { PlaylistEditorDialog } from "@/components/playlists/PlaylistEditorDialog";
import { Button } from "@/components/ui/Button";
import { CoverArt } from "@/components/ui/CoverArt";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { playlistApi } from "@/lib/ipc";
import { pickPlaylistCover } from "@/lib/playlistCover";
import { usePlaybackStore } from "@/stores/playbackStore";
import { usePlaylistStore } from "@/stores/playlistStore";
import { useUiStore } from "@/stores/uiStore";
import type { PlaylistDetail } from "@/types/ipc";

/** One playlist in its explicit order. */
export function PlaylistView({ playlistId }: { playlistId: string }): JSX.Element {
  const back = useUiStore((state) => state.back);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const setCustomCover = usePlaylistStore((state) => state.setCustomCover);
  const [coverBusy, setCoverBusy] = useState(false);
  const [coverError, setCoverError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const updatedAt = usePlaylistStore(
    (state) => state.playlists.find((playlist) => playlist.id === playlistId)?.updatedAt,
  );
  const [detail, setDetail] = useState<PlaylistDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (): Promise<void> => {
    try {
      const next = await playlistApi.detail(playlistId);
      setDetail(next);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "This playlist could not be opened");
    }
  }, [playlistId]);

  useEffect(() => {
    setDetail(null);
    void load();
  }, [load, updatedAt]);

  if (error !== null) {
    return (
      <div className="mx-auto max-w-[1200px] px-8 pt-10">
        <p className="flex items-center gap-2 text-[12px] text-critical" role="alert">
          <TriangleAlert className="h-4 w-4" aria-hidden="true" />
          {error}
        </p>
      </div>
    );
  }

  if (detail === null) {
    return <div className="mx-8 mt-8 h-[220px] animate-pulse rounded-3xl bg-white/[0.03]" />;
  }

  const { playlist } = detail;
  const tracks = detail.tracks.items;
  const chooseCover = async (): Promise<void> => {
    setCoverBusy(true);
    setCoverError(null);
    try {
      const path = await pickPlaylistCover();
      if (path === null) return;
      const updated = await setCustomCover(playlist.id, path);
      if (updated !== null) {
        setDetail((current) => (current === null ? null : { ...current, playlist: updated }));
      } else {
        setCoverError(usePlaylistStore.getState().error ?? "The cover could not be changed.");
      }
    } catch (cause) {
      setCoverError(cause instanceof Error ? cause.message : "The cover could not be changed.");
    } finally {
      setCoverBusy(false);
    }
  };

  return (
    <div className="mx-auto w-full max-w-[1400px] px-8 pt-6 pb-12">
      <button
        type="button"
        onClick={back}
        className="mb-5 flex items-center gap-1.5 text-[11.5px] text-white/40 hover:text-white"
      >
        <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
        Back
      </button>

      <header className="flex flex-col gap-5 sm:flex-row sm:items-end">
        <CoverArt
          path={playlist.coverPath}
          url={playlist.coverUrl}
          alt={playlist.name}
          className="h-[164px] w-[164px] shrink-0"
          rounded="rounded-2xl"
        />
        <div className="min-w-0">
          <p className="text-[10px] font-semibold tracking-[0.2em] text-white/30 uppercase">
            Playlist
          </p>
          <div className="mt-2 flex items-center gap-2.5">
            <h1 className="min-w-0 text-[32px] leading-tight font-semibold tracking-[-0.035em] text-white">
              {playlist.name}
            </h1>
            <button
              type="button"
              aria-label="Edit playlist"
              title="Edit playlist"
              onClick={() => {
                setEditing(true);
              }}
              className="grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/35 transition-colors hover:bg-white/[0.07] hover:text-white"
            >
              <Pencil className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
          {playlist.description !== null ? (
            <p className="mt-2 max-w-2xl text-[12.5px] leading-relaxed whitespace-pre-wrap text-white/45">
              {playlist.description}
            </p>
          ) : null}
          {playlist.genre !== null ? (
            <p className="mt-1.5 text-[10px] font-medium tracking-[0.14em] text-white/30 uppercase">
              {playlist.genre}
            </p>
          ) : null}
          <p className="tabular mt-2 text-[11.5px] text-white/35">
            {formatCount(playlist.trackCount)} songs · {formatLibraryDuration(playlist.durationMs)}
          </p>
          <div className="mt-5 flex flex-wrap items-center gap-2.5">
            <Button
              variant="primary"
              icon={Play}
              disabled={tracks.length === 0}
              onClick={() => {
                void playTracks(tracks, 0);
              }}
            >
              Play
            </Button>
            <Button
              variant="secondary"
              icon={ImagePlus}
              loading={coverBusy}
              onClick={() => {
                void chooseCover();
              }}
            >
              Change cover
            </Button>
          </div>
          {coverError !== null ? (
            <p role="alert" className="mt-2 text-[11.5px] text-critical">
              {coverError}
            </p>
          ) : null}
        </div>
      </header>

      <div className="mt-9">
        <TrackTable
          tracks={tracks}
          playlistId={playlist.id}
          onPlay={(index) => {
            void playTracks(tracks, index);
          }}
        />
      </div>

      {tracks.length === 0 ? (
        <p className="mt-8 text-[12.5px] text-white/35">
          This playlist is empty. Right-click a song anywhere in Prism to add it.
        </p>
      ) : null}

      {editing ? (
        <PlaylistEditorDialog
          playlist={playlist}
          onClose={() => {
            setEditing(false);
          }}
          onSaved={(updated) => {
            setDetail((current) => (current === null ? null : { ...current, playlist: updated }));
          }}
        />
      ) : null}
    </div>
  );
}
