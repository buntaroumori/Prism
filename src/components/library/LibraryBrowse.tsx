import { useEffect, useState, type JSX } from "react";

import { VirtualCardGrid } from "@/components/library/VirtualCardGrid";
import { loadAllPages } from "@/lib/loadAllPages";
import { VirtualTrackList } from "@/components/library/VirtualTrackList";
import { ArtistArt } from "@/components/library/ArtistArt";
import { libraryApi } from "@/lib/ipc";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { ArtistRow, TrackQuery } from "@/types/ipc";

export type LibraryMode = "albums" | "artists" | "tracks";

export function LibraryTabs({
  mode,
  onChange,
}: {
  mode: LibraryMode;
  onChange: (mode: LibraryMode) => void;
}): JSX.Element {
  return (
    <div
      role="tablist"
      aria-label="Library view"
      className="flex w-fit gap-1 rounded-lg border border-white/[0.08] p-1"
    >
      {(["albums", "artists", "tracks"] as const).map((item) => (
        <button
          key={item}
          type="button"
          role="tab"
          aria-selected={mode === item}
          onClick={() => {
            onChange(item);
          }}
          className={
            mode === item
              ? "rounded-md bg-white/[0.12] px-3 py-1.5 text-[12px] text-white capitalize"
              : "rounded-md px-3 py-1.5 text-[12px] text-white/45 capitalize hover:text-white"
          }
        >
          {item}
        </button>
      ))}
    </div>
  );
}

export function ArtistsBrowser({ query }: { query: string }): JSX.Element {
  const openArtist = useUiStore((state) => state.openArtist);
  const [artists, setArtists] = useState<ArtistRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    setArtists([]);
    setLoading(true);
    void loadAllPages(
      (offset, limit) => libraryApi.artists({ ...(query ? { text: query } : {}), limit, offset }),
      () => active,
    )
      .then((page) => {
        if (active && page !== null) {
          setArtists(page.items);
          setError(null);
        }
      })
      .catch((cause: unknown) => {
        if (active) setError(cause instanceof Error ? cause.message : "Cannot load artists");
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [query]);
  return (
    <div className="mt-5">
      {error !== null ? (
        <p role="alert" className="text-critical">
          {error}
        </p>
      ) : null}
      <VirtualCardGrid
        items={artists}
        minimum={150}
        extraHeight={76}
        render={(artist) => (
          <button
            key={artist.id}
            type="button"
            onClick={() => {
              openArtist(artist.id);
            }}
            className="min-w-0 text-left"
          >
            <ArtistArt artist={artist} className="aspect-square w-full" rounded="rounded-lg" />
            <span className="mt-2 block truncate text-[13px] font-medium text-white/85">
              {artist.name}
            </span>
            <span className="text-[11px] text-white/35">{artist.albumCount} albums</span>
          </button>
        )}
      />
      {!loading && artists.length === 0 ? (
        <p className="py-8 text-[12px] text-white/40">No artists found.</p>
      ) : null}
      {loading ? <p className="py-4 text-[12px] text-white/40">Reading your artists…</p> : null}
    </div>
  );
}

export function TracksBrowser({ query }: { query: TrackQuery }): JSX.Element {
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const [error, setError] = useState<string | null>(null);
  const playFrom = (index: number): void => {
    void libraryApi
      .search({ ...query, offset: index, limit: 500 })
      .then((page) => {
        if (page.items.length > 0) return playTracks(page.items, 0);
        return undefined;
      })
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : "Cannot play tracks");
      });
  };
  return (
    <div className="mt-5">
      {error !== null ? (
        <p role="alert" className="mb-2 text-[12px] text-critical">
          {error}
        </p>
      ) : null}
      <VirtualTrackList
        className="h-[min(70vh,760px)]"
        query={query}
        onActivate={(_row, index) => {
          playFrom(index);
        }}
        onPlayFrom={playFrom}
      />
    </div>
  );
}
