import { Image as ImageIcon, LayoutGrid, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type JSX } from "react";

import { AlbumCard } from "@/components/library/AlbumCard";
import { Segmented } from "@/components/ui/Segmented";
import { formatCount } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { COVER_SIZE_MAX, COVER_SIZE_MIN, useUiStore } from "@/stores/uiStore";
import type { AlbumRow, AlbumSort } from "@/types/ipc";

/** The eight orderings the backend exposes, worded the way a library reads. */
const SORTS: readonly { value: AlbumSort; label: string }[] = [
  { value: "added", label: "Newest" },
  { value: "addedAsc", label: "Oldest" },
  { value: "artist", label: "Artist A–Z" },
  { value: "artistDesc", label: "Artist Z–A" },
  { value: "title", label: "Album A–Z" },
  { value: "titleDesc", label: "Album Z–A" },
  { value: "year", label: "Year ↓" },
  { value: "yearAsc", label: "Year ↑" },
];

const PAGE_SIZE = 120;

/**
 * The library, as a mosaic of records.
 *
 * A track table is the wrong shape for browsing a collection: it answers "what
 * do I own" a line at a time. The mosaic answers it a record at a time, which
 * is how music is filed — and the track list is one click away on every album
 * page. Cover size is a user setting because the same grid has to serve a
 * laptop and a 4K desk.
 */
export function LibraryView(): JSX.Element {
  const stats = useLibraryStore((state) => state.stats);
  const query = useLibraryStore((state) => state.query);
  const setQuery = useLibraryStore((state) => state.setQuery);
  const coverSize = useUiStore((state) => state.coverSize);
  const setAppearance = useUiStore((state) => state.setAppearance);
  const playTracks = usePlaybackStore((state) => state.playTracks);

  const [sort, setSort] = useState<AlbumSort>("artist");
  const [albums, setAlbums] = useState<AlbumRow[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyAlbum, setBusyAlbum] = useState<string | null>(null);
  const sentinel = useRef<HTMLDivElement | null>(null);

  // Reset the window whenever the query or the ordering changes: a stale
  // offset against a new filter is how grids end up with holes.
  useEffect(() => {
    setAlbums([]);
    setOffset(0);
    setLoading(true);
  }, [query, sort]);

  useEffect(() => {
    let active = true;
    void libraryApi
      .albums({
        ...(query.length > 0 ? { text: query } : {}),
        sort,
        limit: PAGE_SIZE,
        offset,
      })
      .then((page) => {
        if (!active) {
          return;
        }
        setTotal(page.total);
        setAlbums((current) =>
          offset === 0
            ? page.items
            : [
                ...current,
                ...page.items.filter((next) => !current.some((album) => album.id === next.id)),
              ],
        );
        setError(null);
      })
      .catch((cause: unknown) => {
        if (active) {
          setError(cause instanceof Error ? cause.message : "Cannot read the catalogue");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [query, sort, offset]);

  // Infinite scroll: the mosaic is the only surface in the app that may grow
  // unbounded, so it pages instead of virtualising (cards are cheap, rows are
  // not — and a virtualised mosaic cannot animate into place).
  useEffect(() => {
    const node = sentinel.current;
    if (node === null) {
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting) && !loading && albums.length < total) {
          setOffset(albums.length);
        }
      },
      { rootMargin: "600px" },
    );
    observer.observe(node);
    return () => {
      observer.disconnect();
    };
  }, [albums.length, loading, total]);

  const playAlbum = async (album: AlbumRow): Promise<void> => {
    setBusyAlbum(album.id);
    try {
      const page = await libraryApi.search({ albumId: album.id, sort: "album", limit: 500 });
      if (page.items.length === 0) {
        return;
      }
      const ordered = [...page.items].sort((a, b) => {
        const disc = (a.discNo ?? 1) - (b.discNo ?? 1);
        return disc !== 0 ? disc : (a.trackNo ?? 0) - (b.trackNo ?? 0);
      });
      await playTracks(ordered, 0);
    } finally {
      setBusyAlbum(null);
    }
  };

  const columns = useMemo(
    () => `repeat(auto-fill, minmax(${String(coverSize)}px, 1fr))`,
    [coverSize],
  );

  return (
    <div className="mx-auto w-full max-w-[1560px] px-8 pt-7 pb-10">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="min-w-0">
          <h1 className="flex items-center gap-2 text-[26px] leading-none font-semibold tracking-[-0.03em] text-white">
            <LayoutGrid className="h-5 w-5 text-white/25" aria-hidden="true" strokeWidth={1.5} />
            Library
          </h1>
          <p className="mt-1.5 text-[12.5px] text-white/40">
            {stats === null
              ? "Reading the catalogue…"
              : `${formatCount(total)} albums · ${formatCount(stats.trackCount)} tracks · ${formatCount(stats.artistCount)} artists`}
            {query.length > 0 ? (
              <>
                {" · filtered by "}
                <span className="text-white/70">“{query}”</span>
              </>
            ) : null}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-2 rounded-full border border-white/[0.07] bg-white/[0.03] px-3 py-1.5">
            <ImageIcon className="h-3.5 w-3.5 text-white/30" aria-hidden="true" />
            <input
              type="range"
              min={COVER_SIZE_MIN}
              max={COVER_SIZE_MAX}
              step={4}
              value={coverSize}
              aria-label="Cover size"
              onChange={(event) => {
                setAppearance({ coverSize: Number(event.target.value) });
              }}
              className="h-1 w-24 cursor-pointer appearance-none rounded-full bg-white/15 accent-white [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white"
            />
            <span className="tabular w-8 text-right text-[10px] text-white/35">{coverSize}</span>
          </div>

          {query.length > 0 ? (
            <button
              type="button"
              onClick={() => {
                setQuery("");
              }}
              className="flex items-center gap-1.5 rounded-full border border-white/10 px-3 py-1.5 text-[11px] text-white/50 hover:text-white"
            >
              <X className="h-3 w-3" aria-hidden="true" />
              Clear
            </button>
          ) : null}
        </div>
      </header>

      <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
        <Segmented
          options={SORTS}
          value={sort}
          onChange={setSort}
          label="Sort albums"
          layoutId="library-album-sort"
        />
      </div>

      {error !== null ? (
        <p role="alert" className="mt-4 text-[12px] text-critical">
          {error}
        </p>
      ) : null}

      {total === 0 && !loading ? (
        <div className="mt-10 rounded-2xl border border-dashed border-white/[0.09] px-6 py-14 text-center">
          <p className="text-[13px] font-medium text-white/55">
            {query.length > 0 ? "No album matches that search" : "The catalogue is empty"}
          </p>
          <p className="mx-auto mt-1 max-w-md text-[11.5px] leading-relaxed text-white/30">
            {query.length > 0
              ? "Try a shorter query. Search runs over titles, artists and track names."
              : "Add a folder in Settings → Library and run a scan."}
          </p>
        </div>
      ) : (
        <div
          className="mt-5 grid gap-x-5 gap-y-7"
          style={{ gridTemplateColumns: columns }}
          data-cover-size={coverSize}
        >
          {albums.map((album) => (
            <AlbumCard
              key={album.id}
              album={album}
              busy={busyAlbum === album.id}
              onPlay={(target) => {
                void playAlbum(target);
              }}
            />
          ))}
        </div>
      )}

      <div ref={sentinel} aria-hidden="true" className="h-8" />

      {loading ? (
        <div className={cn("mt-2 flex items-center gap-2 text-[11px] text-white/30")}>
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-white/40" />
          Reading the catalogue…
        </div>
      ) : null}
    </div>
  );
}
