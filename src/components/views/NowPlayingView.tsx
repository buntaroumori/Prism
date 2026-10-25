import { AudioWaveform } from "lucide-react";
import type { JSX } from "react";

import { ArtworkModule } from "@/components/nowplaying/ArtworkModule";
import { NowPlayingControls } from "@/components/nowplaying/NowPlayingControls";
import { NowPlayingDeck } from "@/components/nowplaying/NowPlayingDeck";
import { QualityBadge } from "@/components/player/QualityBadge";
import { EMPTY_STATUS, usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";

/**
 * Bounded full-window Now Playing surface.
 *
 * Metadata, uncovered artwork and the complete transport share the left half;
 * lyrics/spectrum occupy the symmetric right half. Nothing in this route grows
 * with lyric length or playhead updates, and the route itself never scrolls.
 */
export function NowPlayingView(): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const back = useUiStore((state) => state.back);
  const track = status?.track ?? null;
  const trackId = track?.trackId ?? null;

  if (track === null) {
    return (
      <div className="flex h-full flex-col items-center justify-center overflow-hidden px-8 text-center">
        <div className="grid h-20 w-20 place-items-center rounded-full border border-white/[0.08] bg-white/[0.03]">
          <AudioWaveform className="h-7 w-7 text-white/25" aria-hidden="true" strokeWidth={1.4} />
        </div>
        <h2 className="mt-6 text-[20px] font-semibold tracking-[-0.02em] text-white">
          Nothing is playing
        </h2>
        <p className="mt-2 max-w-sm text-[13px] leading-relaxed text-white/40">
          Pick an album from Home, or add a folder in Settings and run a scan to build your library.
        </p>
        <button
          type="button"
          onClick={back}
          className="accent-soft-text mt-5 text-[12.5px] underline-offset-4 hover:underline"
        >
          Back
        </button>
      </div>
    );
  }

  const entry = queue.find((item) => item.trackId === trackId) ?? queue[index] ?? null;
  const queuePosition = queue.length > 0 ? `${String(index + 1)} / ${String(queue.length)}` : null;

  return (
    <div
      data-now-playing
      className="mx-auto grid h-full min-h-0 w-full max-w-[1500px] grid-cols-2 gap-8 overflow-hidden px-8 py-6"
    >
      <div data-now-playing-primary className="flex min-h-0 flex-col">
        <header className="shrink-0">
          <div className="flex h-6 items-center gap-2">
            <QualityBadge status={status ?? EMPTY_STATUS} track={entry?.catalogue ?? null} />
            {status?.dop === true ? (
              <span className="rounded-full border border-spectrum-violet/30 bg-spectrum-violet/[0.10] px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] text-spectrum-violet uppercase">
                DSD over PCM
              </span>
            ) : null}
            {status?.state === "buffering" ? (
              <span className="rounded-full border border-white/15 bg-white/[0.06] px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] text-white/60 uppercase">
                Buffering
              </span>
            ) : null}
            {queuePosition !== null ? (
              <span className="tabular ml-auto text-[11px] text-white/25">{queuePosition}</span>
            ) : null}
          </div>
          <h1 className="mt-2 truncate text-[30px] leading-[1.12] font-semibold tracking-[-0.035em] text-white">
            {track.title}
          </h1>
          <p data-selectable="artist" className="mt-1 truncate text-[14px] text-white/55">
            {track.artist.length > 0 ? track.artist : "Unknown artist"}
          </p>
          {track.album.length > 0 ? (
            <p data-selectable="album" className="mt-0.5 truncate text-[12.5px] text-white/30">
              {track.album}
            </p>
          ) : null}
        </header>

        <div className="my-4 flex min-h-0 flex-1 items-center justify-center overflow-hidden">
          <ArtworkModule
            trackId={trackId}
            album={track.album}
            artist={track.artist}
            title={track.title}
          />
        </div>

        <NowPlayingControls />
      </div>

      <div data-now-playing-secondary className="h-full min-h-0">
        <NowPlayingDeck status={status} trackId={trackId} />
      </div>
    </div>
  );
}
