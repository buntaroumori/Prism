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
 * One shared grid aligns the secondary column with the artwork's top and the
 * transport's bottom. Metadata, artwork and controls never depend on lyric
 * length or playhead updates, and the route itself never scrolls.
 */
export function NowPlayingView(): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const back = useUiStore((state) => state.back);
  const openAlbum = useUiStore((state) => state.openAlbum);
  const openArtist = useUiStore((state) => state.openArtist);
  const openLibraryGenre = useUiStore((state) => state.openLibraryGenre);
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
  const albumId = entry?.albumId ?? null;
  const artistId = entry?.artistId ?? null;
  const artist = track.artist.length > 0 ? track.artist : "Unknown artist";
  const genre = entry?.catalogue.genre.trim() ?? "";

  return (
    <div
      data-now-playing
      className="mx-auto grid h-full min-h-0 w-full max-w-[1500px] grid-cols-2 grid-rows-[auto_minmax(0,1fr)_auto] gap-x-8 overflow-hidden px-8 py-6"
    >
      <header className="col-start-1 row-start-1 min-w-0 pb-4">
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
          {albumId === null ? (
            track.title
          ) : (
            <button
              type="button"
              data-now-playing-title
              aria-label={`Open album for ${track.title}`}
              title={`Open ${track.album}`}
              onClick={() => {
                openAlbum(albumId);
              }}
              className="block max-w-full truncate text-left hover:underline hover:decoration-white/30 hover:underline-offset-4 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
            >
              {track.title}
            </button>
          )}
        </h1>
        <p data-selectable="artist" className="mt-1 truncate text-[14px] text-white/55">
          {artistId === null ? (
            artist
          ) : (
            <button
              type="button"
              data-now-playing-artist
              aria-label={`Open artist ${artist}`}
              onClick={() => {
                openArtist(artistId);
              }}
              className="block max-w-full truncate hover:text-white/80 hover:underline hover:underline-offset-2 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
            >
              {artist}
            </button>
          )}
        </p>
        {track.album.length > 0 ? (
          <p data-selectable="album" className="mt-0.5 truncate text-[12.5px] text-white/30">
            {albumId === null ? (
              track.album
            ) : (
              <button
                type="button"
                data-now-playing-album
                aria-label={`Open album ${track.album}`}
                onClick={() => {
                  openAlbum(albumId);
                }}
                className="block max-w-full truncate hover:text-white/60 hover:underline hover:underline-offset-2 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
              >
                {track.album}
              </button>
            )}
          </p>
        ) : null}
        {genre.length > 0 ? (
          <button
            type="button"
            data-now-playing-genre
            aria-label={`Show ${genre} albums in Library`}
            onClick={() => {
              openLibraryGenre(genre);
            }}
            className="mt-1 block max-w-full truncate text-[11px] text-white/35 transition-colors hover:text-accent hover:underline hover:underline-offset-2 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
          >
            {genre}
          </button>
        ) : null}
      </header>

      <div
        data-now-playing-artwork-region
        className="col-start-1 row-start-2 flex min-h-0 items-center justify-center overflow-hidden"
      >
        <ArtworkModule
          trackId={trackId}
          album={track.album}
          artist={track.artist}
          title={track.title}
        />
      </div>

      <NowPlayingControls className="col-start-1 row-start-3 mt-4" />

      <div data-now-playing-secondary className="col-start-2 row-start-2 row-end-4 h-full min-h-0">
        <NowPlayingDeck status={status} trackId={trackId} />
      </div>
    </div>
  );
}
