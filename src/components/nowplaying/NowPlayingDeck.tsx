import { useState, type JSX } from "react";

import { SpectrumModule } from "@/components/nowplaying/SpectrumModule";
import { LyricsPanel } from "@/components/player/LyricsPanel";
import { useLyrics } from "@/hooks/useLyrics";
import { cn } from "@/lib/utils";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { PlayerStatus } from "@/types/ipc";

interface NowPlayingDeckProps {
  status: PlayerStatus | null;
  trackId: string | null;
}

type DeckTab = "lyrics" | "queue" | "spectrum";

export function NowPlayingDeck({ status, trackId }: NowPlayingDeckProps): JSX.Element {
  const lyrics = useLyrics(trackId);
  const showSpectrum = useUiStore((state) => state.showSpectrum);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const jumpTo = usePlaybackStore((state) => state.jumpTo);
  const [tab, setTab] = useState<DeckTab>(showSpectrum ? "spectrum" : "lyrics");

  return (
    <section aria-label="Now Playing details" className="flex h-full min-h-0 flex-col">
      <div
        role="tablist"
        aria-label="Now Playing details"
        className="mb-3 flex shrink-0 gap-1 border-b border-white/[0.08]"
      >
        {(["lyrics", "queue", "spectrum"] as const).map((item) => (
          <button
            key={item}
            type="button"
            role="tab"
            aria-selected={tab === item}
            onClick={() => {
              setTab(item);
            }}
            className={cn(
              "border-b-2 px-3 py-2 text-[12px] capitalize",
              tab === item
                ? "border-accent text-white"
                : "border-transparent text-white/40 hover:text-white/75",
            )}
          >
            {item}
          </button>
        ))}
      </div>
      <div role="tabpanel" className="min-h-0 flex-1">
        {tab === "lyrics" ? <LyricsPanel lyrics={lyrics} /> : null}
        {tab === "spectrum" ? (
          <SpectrumModule sampleRate={status?.sourceFormat?.sampleRate ?? null} />
        ) : null}
        {tab === "queue" ? (
          <ol className="h-full overflow-y-auto">
            {queue.slice(index).map((entry, relativePosition) => {
              const position = index + relativePosition;
              return (
                <li key={entry.queueId}>
                  <button
                    type="button"
                    onClick={() => {
                      void jumpTo(position);
                    }}
                    className={cn(
                      "flex w-full gap-3 rounded-lg px-3 py-2 text-left hover:bg-white/[0.04]",
                      position === index ? "text-accent" : "text-white/75",
                    )}
                  >
                    <span className="w-6 shrink-0 text-right text-[11px] text-white/30 tabular-nums">
                      {position + 1}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[12px]">{entry.title}</span>
                    <span className="max-w-[40%] truncate text-[11px] text-white/35">
                      {entry.artist}
                    </span>
                  </button>
                </li>
              );
            })}
          </ol>
        ) : null}
      </div>
    </section>
  );
}
