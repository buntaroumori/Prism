import { AudioLines } from "lucide-react";
import { useEffect, useMemo, useState, type JSX } from "react";

import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import type { TrackRow } from "@/types/ipc";

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
    const result = new Map<number, { track: TrackRow; index: number }[]>();
    tracks.forEach((track, index) => {
      const disc = track.discNo ?? 1;
      const group = result.get(disc) ?? [];
      group.push({ track, index });
      result.set(disc, group);
    });
    return [...result.entries()].sort(([a], [b]) => a - b);
  }, [tracks]);

  const showDiscs = discCount > 1 || groups.length > 1;

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
                      String(track.trackNo ?? index + 1).padStart(2, "0")
                    )}
                  </span>

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

                  {track.genre !== "" ? (
                    <span className="hidden max-w-[140px] truncate text-[10.5px] text-white/25 lg:inline">
                      {track.genre}
                    </span>
                  ) : null}

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
            })}
          </ol>
        </section>
      ))}
    </div>
  );
}
