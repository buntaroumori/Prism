import type { JSX } from "react";

import { ArtworkModule } from "@/components/nowplaying/ArtworkModule";
import { SpectrumModule } from "@/components/nowplaying/SpectrumModule";
import { LyricsPanel } from "@/components/player/LyricsPanel";
import { useLyrics } from "@/hooks/useLyrics";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";
import type { PlayerStatus } from "@/types/ipc";

interface NowPlayingDeckProps {
  status: PlayerStatus | null;
  trackId: string | null;
  album: string;
  artist: string;
  title: string;
}

/**
 * The single Now Playing layout: artwork on the left, lyrics and spectrum on
 * the right. A track without lyrics does not leave an empty panel behind; the
 * spectrum expands into the complete secondary column instead.
 */
export function NowPlayingDeck({
  status,
  trackId,
  album,
  artist,
  title,
}: NowPlayingDeckProps): JSX.Element {
  const lyrics = useLyrics(trackId);
  const showSpectrum = useUiStore((state) => state.showSpectrum);
  const hasLyrics =
    lyrics.document !== null &&
    (lyrics.document.lines.length > 0 || lyrics.document.plainText.trim().length > 0);
  // Loading and read errors still need their real state and retry control. A
  // successfully resolved empty document is the only state that hides lyrics.
  const showLyrics = hasLyrics || lyrics.loading || lyrics.error !== null;
  const hasSecondaryColumn = showLyrics || showSpectrum;

  return (
    <div className="grid grid-cols-1 items-stretch gap-5 lg:grid-cols-12">
      <section
        aria-label="Current album artwork"
        className={cn(
          "min-w-0 rounded-3xl border border-white/[0.05] bg-white/[0.015] p-5",
          hasSecondaryColumn ? "lg:col-span-7" : "lg:col-span-12",
        )}
      >
        <div className="flex h-full items-center justify-center">
          <ArtworkModule trackId={trackId} album={album} artist={artist} title={title} />
        </div>
      </section>

      {hasSecondaryColumn ? (
        <div className="flex min-h-[520px] min-w-0 flex-col gap-5 lg:col-span-5">
          {showLyrics ? (
            <section
              aria-label="Lyrics"
              className={cn("min-h-[320px]", showSpectrum ? "flex-1" : "h-full")}
            >
              <LyricsPanel lyrics={lyrics} />
            </section>
          ) : null}
          {showSpectrum ? (
            <section
              aria-label="Spectrum"
              className={showLyrics ? "h-[210px] shrink-0" : "min-h-[520px] flex-1"}
            >
              <SpectrumModule sampleRate={status?.sourceFormat?.sampleRate ?? null} />
            </section>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
