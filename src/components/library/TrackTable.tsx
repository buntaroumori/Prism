import { AudioLines, Play } from "lucide-react";
import { useEffect, useMemo, useState, type JSX } from "react";

import { CoverArt } from "@/components/ui/CoverArt";
import { formatDuration } from "@/lib/format";
import { cachedTrackCover, trackCover } from "@/lib/trackCover";
import { cn } from "@/lib/utils";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { CoverAsset, TrackRow } from "@/types/ipc";

interface TrackTableProps {
  tracks: TrackRow[];
  onPlay: (index: number) => void;
  discCount?: number;
  playlistId?: string;
}

/**
 * Desktop track interaction: one click selects, a second distinct click does
 * nothing special, and a double click anywhere on the row plays it. Selection
 * is cleared by pressing outside every track row.
 */
export function TrackTable({
  tracks,
  onPlay,
  discCount = 1,
  playlistId,
}: TrackTableProps): JSX.Element {
  const [selected, setSelected] = useState<string | null>(null);
  const openAt = useContextMenuStore((state) => state.openAt);
  const openAlbum = useUiStore((state) => state.openAlbum);
  const currentId = usePlaybackStore((state) => state.status?.track?.trackId ?? null);
  const playing = usePlaybackStore((state) => state.status?.state === "playing");

  useEffect(() => {
    setSelected(null);
  }, [tracks]);

  useEffect(() => {
    const clear = (event: PointerEvent): void => {
      if (!(event.target as Element | null)?.closest("[data-track-row]")) {
        setSelected(null);
      }
    };
    window.addEventListener("pointerdown", clear);
    return () => {
      window.removeEventListener("pointerdown", clear);
    };
  }, []);

  const groups = useMemo(() => {
    if (playlistId !== undefined) {
      return [[1, tracks.map((track, index) => ({ track, index }))] as const];
    }
    const result = new Map<number, { track: TrackRow; index: number }[]>();
    tracks.forEach((track, index) => {
      const disc = track.discNo ?? 1;
      const group = result.get(disc) ?? [];
      group.push({ track, index });
      result.set(disc, group);
    });
    return [...result.entries()].sort(([a], [b]) => a - b);
  }, [playlistId, tracks]);

  const showDiscs = playlistId === undefined && (discCount > 1 || groups.length > 1);

  return (
    <div className={cn(showDiscs && "space-y-7")}>
      {groups.map(([disc, rows]) => (
        <section key={disc} aria-label={showDiscs ? `Disc ${String(disc)}` : undefined}>
          {showDiscs ? (
            <h2 className="mb-2 border-b border-white/[0.07] pb-2 text-[10px] font-semibold tracking-[0.16em] text-white/35 uppercase">
              Disc {disc}
            </h2>
          ) : null}
          <ol className="divide-y divide-white/[0.045]">
            {rows.map(({ track, index }) => {
              const current = currentId === track.id;
              return (
                <li
                  key={track.id}
                  data-track-row
                  data-interactive
                  aria-selected={selected === track.id}
                  onClick={() => {
                    setSelected(track.id);
                  }}
                  onDoubleClick={() => {
                    onPlay(index);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setSelected(track.id);
                    openAt(event.clientX, event.clientY, {
                      kind: "track",
                      track,
                      ...(playlistId !== undefined ? { playlistId } : {}),
                    });
                  }}
                  className={cn(
                    "density-row group flex cursor-pointer items-center gap-4 px-2 transition-colors select-none",
                    "hover:bg-white/[0.03]",
                    selected === track.id && "bg-accent/[0.09]",
                    current && selected !== track.id && "bg-accent/[0.045]",
                    track.missing && "opacity-45",
                  )}
                >
                  <span className="grid h-7 w-7 shrink-0 place-items-center text-[11px] text-white/30">
                    {current ? (
                      <AudioLines
                        className={cn("h-3.5 w-3.5 text-accent", playing && "animate-pulse")}
                        aria-hidden="true"
                      />
                    ) : (
                      String(
                        playlistId === undefined ? (track.trackNo ?? index + 1) : index + 1,
                      ).padStart(2, "0")
                    )}
                  </span>

                  {playlistId !== undefined ? <PlaylistTrackCover track={track} /> : null}

                  <button
                    type="button"
                    aria-label={`Play ${track.title}`}
                    title={`Play ${track.title}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      if (event.detail === 1) onPlay(index);
                    }}
                    onDoubleClick={(event) => {
                      // The first click starts playback; the second is consumed.
                      event.stopPropagation();
                    }}
                    className="grid h-7 w-7 shrink-0 place-items-center rounded-full text-white/25 transition-colors hover:bg-accent/10 hover:text-accent focus-visible:outline-2 focus-visible:outline-accent"
                  >
                    <Play className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
                  </button>

                  <div className="min-w-0 flex-1">
                    <p
                      className={cn(
                        "truncate text-[13px]",
                        current ? "text-accent" : "text-white/85",
                      )}
                    >
                      {track.title}
                    </p>
                    {track.artist !== "" && track.artist !== track.albumArtist ? (
                      <p className="truncate text-[11px] text-white/35">{track.artist}</p>
                    ) : null}
                  </div>

                  {playlistId !== undefined ? (
                    <div className="w-[clamp(120px,20vw,260px)] min-w-0 shrink-0 text-[11.5px] text-white/35">
                      {track.albumId === null ? (
                        <span className="block truncate">{track.album || "—"}</span>
                      ) : (
                        <button
                          type="button"
                          data-playlist-album
                          title={track.album}
                          onClick={(event) => {
                            event.stopPropagation();
                            if (track.albumId !== null) openAlbum(track.albumId);
                          }}
                          className="block max-w-full truncate text-left hover:text-white/70 hover:underline hover:underline-offset-2 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
                        >
                          {track.album}
                        </button>
                      )}
                    </div>
                  ) : null}

                  <span className="tabular shrink-0 text-[11.5px] text-white/30">
                    {track.durationMs > 0 ? formatDuration(track.durationMs) : "—"}
                  </span>
                </li>
              );
            })}
          </ol>
        </section>
      ))}
    </div>
  );
}

/** Small, cached source-album artwork used only by playlist rows. */
function PlaylistTrackCover({ track }: { track: TrackRow }): JSX.Element {
  const [asset, setAsset] = useState<CoverAsset | null>(() =>
    cachedTrackCover(track.id, track.artist, track.album),
  );

  useEffect(() => {
    let active = true;
    const cached = cachedTrackCover(track.id, track.artist, track.album);
    if (cached !== null) {
      setAsset(cached);
      return;
    }
    void trackCover(track.id, track.artist, track.album)
      .then((next) => {
        if (active) setAsset(next);
      })
      .catch(() => {
        if (active) setAsset(null);
      });
    return () => {
      active = false;
    };
  }, [track.id, track.artist, track.album]);

  return (
    <div data-playlist-cover className="shrink-0">
      <CoverArt
        path={asset?.localPath ?? track.coverPath}
        thumbnailPath={asset?.thumbnailPath ?? null}
        url={asset?.remoteUrl ?? track.coverUrl}
        alt=""
        className="h-9 w-9"
        rounded="rounded-lg"
        fallbackClassName="h-4 w-4"
      />
    </div>
  );
}
