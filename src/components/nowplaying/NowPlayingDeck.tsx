import { Activity } from "lucide-react";
import type { JSX } from "react";

import { SpectrumModule } from "@/components/nowplaying/SpectrumModule";
import { LyricsPanel } from "@/components/player/LyricsPanel";
import { useLyrics } from "@/hooks/useLyrics";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";
import type { PlayerStatus } from "@/types/ipc";

interface NowPlayingDeckProps {
  status: PlayerStatus | null;
  trackId: string | null;
}

/**
 * Symmetric secondary region for Now Playing.
 *
 * Lyrics own the region when they exist. The spectrum is opt-in alongside
 * them, and becomes the sole panel automatically when resolution produced no
 * lyric document. Both children are bounded; only the lyric sheet scrolls.
 */
export function NowPlayingDeck({ status, trackId }: NowPlayingDeckProps): JSX.Element {
  const lyrics = useLyrics(trackId);
  const showSpectrum = useUiStore((state) => state.showSpectrum);
  const setAppearance = useUiStore((state) => state.setAppearance);
  const hasLyrics =
    lyrics.document !== null &&
    (lyrics.document.lines.length > 0 || lyrics.document.plainText.trim().length > 0);
  // Preserve genuine loading/error states instead of flashing the spectrum on
  // every track change or disguising a read failure as an empty result.
  const showLyrics = hasLyrics || lyrics.loading || lyrics.error !== null;
  const spectrumVisible = !showLyrics || showSpectrum;

  return (
    <section aria-label="Lyrics and spectrum" className="flex h-full min-h-0 flex-col">
      {showLyrics ? (
        <div className="mb-2 flex h-7 shrink-0 items-center justify-end">
          <button
            type="button"
            aria-pressed={showSpectrum}
            onClick={() => {
              setAppearance({ showSpectrum: !showSpectrum });
            }}
            className={cn(
              "flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[10.5px] transition-colors",
              showSpectrum
                ? "border-accent/30 bg-accent/[0.10] text-accent"
                : "border-white/[0.07] text-white/35 hover:bg-white/[0.05] hover:text-white/70",
            )}
          >
            <Activity className="h-3.5 w-3.5" aria-hidden="true" />
            Spectrum
          </button>
        </div>
      ) : null}

      <div
        className={cn(
          "grid min-h-0 flex-1 gap-4",
          showLyrics && spectrumVisible && "grid-rows-[minmax(0,1fr)_190px]",
        )}
      >
        {showLyrics ? (
          <div className="min-h-0">
            <LyricsPanel lyrics={lyrics} />
          </div>
        ) : null}
        {spectrumVisible ? (
          <div className="min-h-0">
            <SpectrumModule sampleRate={status?.sourceFormat?.sampleRate ?? null} />
          </div>
        ) : null}
      </div>
    </section>
  );
}
