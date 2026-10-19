import { AudioWaveform } from "lucide-react";
import type { JSX } from "react";

import { NowPlayingDeck } from "@/components/nowplaying/NowPlayingDeck";
import { QualityBadge } from "@/components/player/QualityBadge";
import { formatPosition } from "@/lib/format";
import {
  durationMs as resolveDuration,
  EMPTY_STATUS,
  playheadMs,
  usePlaybackStore,
} from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";

const FALLBACK = "—";

/**
 * Full-window now-playing surface.
 *
 * The header states what is playing and how it is being delivered; everything
 * below it is the modular deck, whose composition the listener owns (see
 * `useLayoutStore`). The view itself holds no layout opinion beyond the title
 * block, so adding a module never means editing this file.
 */
export function NowPlayingView(): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const tick = usePlaybackStore((state) => state.tick);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const setRoute = useUiStore((state) => state.setRoute);
  const track = status?.track ?? null;
  const trackId = track?.trackId ?? null;

  if (track === null) {
    return (
      <div className="mx-auto flex w-full max-w-[1420px] flex-col items-center justify-center px-8 py-28 text-center">
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
          onClick={() => {
            setRoute("home");
          }}
          className="accent-soft-text mt-5 text-[12.5px] underline-offset-4 hover:underline"
        >
          Back to Home
        </button>
      </div>
    );
  }

  const position = playheadMs(status, tick);
  const duration = resolveDuration(status, tick);
  const queuePosition =
    queue.length > 0 ? `${String(index + 1)} / ${String(queue.length)}` : FALLBACK;

  return (
    <div className="mx-auto w-full max-w-[1420px] px-8 pt-10 pb-12">
      <header className="flex flex-wrap items-end justify-between gap-6">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <QualityBadge status={status ?? EMPTY_STATUS} />
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
          </div>

          <h1 className="mt-4 max-w-2xl text-[34px] leading-[1.08] font-semibold tracking-[-0.035em] text-balance text-white">
            {track.title}
          </h1>
          <p className="mt-2 text-[15px] text-white/55">
            {track.artist.length > 0 ? track.artist : "Unknown artist"}
          </p>
          {track.album.length > 0 ? (
            <p className="mt-0.5 text-[13px] text-white/30">{track.album}</p>
          ) : null}
        </div>

        <div className="flex items-baseline gap-2">
          <span className="tabular text-[13px] text-white/70">{formatPosition(position)}</span>
          <span className="text-[13px] text-white/20">/</span>
          <span className="tabular text-[13px] text-white/35">
            {duration === null ? FALLBACK : formatPosition(duration)}
          </span>
          <span className="ml-3 text-[11px] text-white/25">{queuePosition}</span>
        </div>
      </header>

      <div className="mt-8">
        <NowPlayingDeck status={status} trackId={trackId} album={track.album} title={track.title} />
      </div>
    </div>
  );
}
