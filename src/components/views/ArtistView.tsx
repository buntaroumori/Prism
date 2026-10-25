import { ArrowLeft, Play, TriangleAlert } from "lucide-react";
import { useEffect, useState, type JSX } from "react";

import { AlbumCard } from "@/components/library/AlbumCard";
import { CoverArt } from "@/components/ui/CoverArt";
import { Button } from "@/components/ui/Button";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { ArtistDetail } from "@/types/ipc";

/**
 * One artist: everything the catalogue holds under that name.
 *
 * The albums keep release-year order, which is the order a discography is read
 * in, and each card is the same component the mosaic uses — one album tile in
 * the whole application, so a change to it is a change everywhere.
 */
export function ArtistView({ artistId }: { artistId: string }): JSX.Element {
  const back = useUiStore((state) => state.back);
  const coverSize = useUiStore((state) => state.coverSize);
  const openAlbum = useUiStore((state) => state.openAlbum);
  const playTracks = usePlaybackStore((state) => state.playTracks);

  const [detail, setDetail] = useState<ArtistDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    setDetail(null);
    setError(null);
    void libraryApi
      .artistDetail(artistId)
      .then((next) => {
        if (active) {
          setDetail(next);
        }
      })
      .catch((cause: unknown) => {
        if (active) {
          setError(cause instanceof Error ? cause.message : "Cannot open this artist");
        }
      });
    return () => {
      active = false;
    };
  }, [artistId]);

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
        <div className="h-[200px] animate-pulse rounded-3xl bg-white/[0.03]" />
      </div>
    );
  }

  const artist = detail.artist;
  const albums = detail.albums.items;

  const playAll = async (): Promise<void> => {
    const first = albums[0];
    if (first === undefined) {
      return;
    }
    setBusy(true);
    try {
      const page = await libraryApi.search({
        artistId: artist.id,
        sort: "album",
        limit: 500,
      });
      if (page.items.length > 0) {
        await playTracks(page.items, 0);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mx-auto w-full max-w-[1560px] px-8 pt-6 pb-12">
      <button
        type="button"
        onClick={back}
        className="mb-5 flex items-center gap-1.5 text-[11.5px] text-white/40 transition-colors hover:text-white"
      >
        <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
        Back
      </button>

      <header className="flex flex-col gap-6 sm:flex-row sm:items-end">
        <CoverArt
          path={artist.coverPath}
          alt={artist.name}
          className="h-[180px] w-[180px] shrink-0 shadow-[0_30px_80px_-40px_rgb(0_0_0/1)]"
          rounded="rounded-full"
        />
        <div className="min-w-0 flex-1">
          <p className="text-[10px] font-semibold tracking-[0.2em] text-white/30 uppercase">
            Artist
          </p>
          <h1
            data-selectable="artist"
            className="mt-2 text-[32px] leading-[1.05] font-semibold tracking-[-0.035em] text-balance text-white"
          >
            {artist.name}
          </h1>
          <p className="tabular mt-2 text-[12px] text-white/35">
            {formatCount(artist.albumCount)} albums · {formatCount(artist.trackCount)} tracks ·{" "}
            {formatLibraryDuration(artist.totalDurationMs)}
          </p>
          <div className="mt-5">
            <Button
              variant="primary"
              icon={Play}
              loading={busy}
              onClick={() => {
                void playAll();
              }}
              disabled={albums.length === 0}
            >
              Play artist
            </Button>
          </div>
        </div>
      </header>

      <section className="mt-10">
        <h2 className="mb-4 text-[15px] font-semibold tracking-tight text-white">
          Albums
          <span className="tabular ml-2 text-[11px] font-normal text-white/30">
            {formatCount(albums.length)}
          </span>
        </h2>

        <div
          className="grid gap-x-5 gap-y-7"
          style={{
            gridTemplateColumns: `repeat(auto-fill, minmax(${String(coverSize)}px, 1fr))`,
          }}
        >
          {albums.map((album) => (
            <AlbumCard
              key={album.id}
              album={album}
              onPlay={(target) => {
                openAlbum(target.id);
              }}
            />
          ))}
        </div>
      </section>
    </div>
  );
}
