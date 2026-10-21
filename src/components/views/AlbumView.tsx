import { ArrowLeft, Disc3, Play, Shuffle, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useState, type JSX } from "react";

import { CoverArt } from "@/components/ui/CoverArt";
import { Button } from "@/components/ui/Button";
import { formatCount, formatDuration, formatLibraryDuration } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { AlbumDetail, TrackRow } from "@/types/ipc";

/**
 * One record.
 *
 * Ordered by disc and track number (never by the sort the browse grid uses),
 * with every artist name a link and every row a play button. This is the only
 * surface where a track list is the primary content, which is why the library
 * itself does not need one.
 */
export function AlbumView({ albumId }: { albumId: string }): JSX.Element {
  const back = useUiStore((state) => state.back);
  const openArtist = useUiStore((state) => state.openArtist);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const currentId = usePlaybackStore((state) => state.status?.track?.trackId ?? null);
  const playing = usePlaybackStore((state) => state.status?.state === "playing");

  const [detail, setDetail] = useState<AlbumDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setDetail(null);
    setError(null);
    void libraryApi
      .albumDetail(albumId)
      .then((next) => {
        if (active) {
          setDetail(next);
        }
      })
      .catch((cause: unknown) => {
        if (active) {
          setError(cause instanceof Error ? cause.message : "Cannot open this album");
        }
      });
    return () => {
      active = false;
    };
  }, [albumId]);

  const tracks = useMemo(() => {
    if (detail === null) {
      return [];
    }
    return [...detail.tracks.items].sort((a, b) => {
      const disc = (a.discNo ?? 1) - (b.discNo ?? 1);
      return disc !== 0 ? disc : (a.trackNo ?? 0) - (b.trackNo ?? 0);
    });
  }, [detail]);

  if (error !== null) {
    return (
      <div className="mx-auto max-w-[1560px] px-8 pt-10">
        <div className="flex items-start gap-2.5 rounded-2xl border border-critical/20 bg-critical/[0.06] px-4 py-3 text-[12px] text-critical">
          <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          {error}
        </div>
      </div>
    );
  }

  if (detail === null) {
    return (
      <div className="mx-auto max-w-[1560px] px-8 pt-8">
        <div className="h-[260px] animate-pulse rounded-3xl bg-white/[0.03]" />
      </div>
    );
  }

  const album = detail.album;

  const play = (index: number, shuffle = false): void => {
    if (tracks.length === 0) {
      return;
    }
    const order = shuffle ? shuffled(tracks) : tracks;
    void playTracks(order, index);
  };

  return (
    <div className="mx-auto w-full max-w-[1560px] px-8 pt-6 pb-12">
      <button
        type="button"
        onClick={back}
        className="mb-5 flex items-center gap-1.5 text-[11.5px] text-white/40 transition-colors hover:text-white"
      >
        <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
        Library
      </button>

      <header className="flex flex-col gap-6 sm:flex-row sm:items-end">
        <CoverArt
          path={album.coverPath}
          url={album.coverUrl}
          alt={album.title}
          className="h-[220px] w-[220px] shrink-0 shadow-[0_30px_80px_-40px_rgb(0_0_0/1)]"
          rounded="rounded-2xl"
        />
        <div className="min-w-0 flex-1">
          <p className="text-[10px] font-semibold tracking-[0.2em] text-white/30 uppercase">
            {album.releaseType ?? "Album"}
          </p>
          <h1 className="mt-2 text-[32px] leading-[1.08] font-semibold tracking-[-0.035em] text-balance text-white">
            {album.title}
          </h1>
          <p className="mt-2.5 text-[13px] text-white/50">
            <ArtistLink id={album.artistId} name={album.albumArtist} onOpen={openArtist} />
            {album.year === null ? null : <span className="text-white/25"> · {album.year}</span>}
          </p>
          <p className="tabular mt-1 text-[11.5px] text-white/30">
            {formatCount(album.trackCount)} tracks · {formatLibraryDuration(album.durationMs)} ·{" "}
            {album.quality}
          </p>

          <div className="mt-5 flex items-center gap-2.5">
            <Button
              variant="primary"
              icon={Play}
              onClick={() => {
                play(0);
              }}
              disabled={tracks.length === 0}
            >
              Play
            </Button>
            <Button
              variant="secondary"
              icon={Shuffle}
              onClick={() => {
                play(0, true);
              }}
              disabled={tracks.length === 0}
            >
              Shuffle
            </Button>
          </div>
        </div>
      </header>

      <ol className="mt-9 divide-y divide-white/[0.045]">
        {tracks.map((track, index) => (
          <TrackLine
            key={track.id}
            track={track}
            index={index}
            current={currentId === track.id}
            playing={playing && currentId === track.id}
            onPlay={() => {
              play(index);
            }}
            onOpenArtist={openArtist}
          />
        ))}
      </ol>

      {tracks.length === 0 ? (
        <p className="mt-8 text-[12.5px] text-white/35">
          This album has no playable files. Run a scan from Settings → Library.
        </p>
      ) : null}
    </div>
  );
}

function TrackLine({
  track,
  index,
  current,
  playing,
  onPlay,
  onOpenArtist,
}: {
  track: TrackRow;
  index: number;
  current: boolean;
  playing: boolean;
  onPlay: () => void;
  onOpenArtist: (id: string) => void;
}): JSX.Element {
  return (
    <li
      className={cn(
        "group flex items-center gap-4 px-2 py-2.5 transition-colors hover:bg-white/[0.03]",
        current && "bg-accent/[0.05]",
        track.missing && "opacity-45",
      )}
    >
      <button
        type="button"
        onClick={onPlay}
        aria-label={`Play ${track.title}`}
        className="grid h-7 w-7 shrink-0 place-items-center rounded-full text-[11px] text-white/30 group-hover:text-white"
      >
        <span className="group-hover:hidden">
          {playing ? (
            <Disc3 className="h-3.5 w-3.5 animate-spin text-accent" aria-hidden="true" />
          ) : (
            String(track.trackNo ?? index + 1).padStart(2, "0")
          )}
        </span>
        <Play className="hidden h-3 w-3 fill-current group-hover:block" aria-hidden="true" />
      </button>

      <div className="min-w-0 flex-1">
        <p className={cn("truncate text-[13px]", current ? "text-accent" : "text-white/85")}>
          {track.title}
        </p>
        {track.artist !== "" && track.artist !== track.albumArtist ? (
          <p className="truncate text-[11px] text-white/35">
            <ArtistLink id={track.artistId} name={track.artist} onOpen={onOpenArtist} />
          </p>
        ) : null}
      </div>

      <span className="tabular shrink-0 text-[11.5px] text-white/30">
        {track.durationMs > 0 ? formatDuration(track.durationMs) : "—"}
      </span>

      <span
        className={cn(
          "hidden shrink-0 rounded-full border px-2 py-0.5 text-[9px] font-semibold tracking-[0.08em] uppercase sm:inline",
          track.lossless
            ? "border-positive/25 bg-positive/[0.08] text-positive/85"
            : "border-white/10 bg-white/[0.04] text-white/35",
        )}
        title={`${track.format} · ${track.quality}`}
      >
        {track.quality}
      </span>
    </li>
  );
}

/** A name that navigates. Rendered as a button so it is keyboard reachable. */
export function ArtistLink({
  id,
  name,
  onOpen,
  className,
}: {
  id: string | null;
  name: string;
  onOpen: (id: string) => void;
  className?: string;
}): JSX.Element {
  const label = name.length > 0 ? name : "Unknown artist";
  if (id === null) {
    return <span className={className}>{label}</span>;
  }
  return (
    <button
      type="button"
      onClick={() => {
        onOpen(id);
      }}
      className={cn(
        "max-w-full truncate text-left transition-colors hover:text-white hover:underline hover:decoration-white/30 hover:underline-offset-2",
        className,
      )}
    >
      {label}
    </button>
  );
}

/** Seeded shuffle: the same album shuffles to the same order twice. */
function shuffled(tracks: TrackRow[]): TrackRow[] {
  const copy = [...tracks];
  let seed = copy.length * 2_654_435_761;
  for (let index = copy.length - 1; index > 0; index -= 1) {
    seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
    const swap = seed % (index + 1);
    const a = copy[index];
    const b = copy[swap];
    if (a !== undefined && b !== undefined) {
      copy[index] = b;
      copy[swap] = a;
    }
  }
  return copy;
}
