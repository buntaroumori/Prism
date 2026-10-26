import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  ChevronLeft,
  ChevronRight,
  Image as ImageIcon,
  LayoutGrid,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type JSX } from "react";

import { AlbumCard } from "@/components/library/AlbumCard";
import { PopoverSelect } from "@/components/ui/PopoverSelect";
import { formatAlbumCount, formatCount } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { useBrowseStore, type AlbumSortField } from "@/stores/browseStore";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore, type LibraryContinuation } from "@/stores/playbackStore";
import { COVER_SIZE_MAX, COVER_SIZE_MIN, useUiStore } from "@/stores/uiStore";
import type { AlbumCollection, AlbumRow, AlbumSort, Facets } from "@/types/ipc";

const PAGE_SIZE = 120;

const COLLECTION_TITLES: Record<AlbumCollection, { title: string; caption: string }> = {
  recentlyAdded: {
    title: "Recently added",
    caption: "Every record that joined your library, newest first.",
  },
  recentlyPlayed: {
    title: "Recently played",
    caption: "Your listening history, most recent first.",
  },
  mostPlayed: {
    title: "On repeat",
    caption: "The records you return to most often.",
  },
  highResolution: {
    title: "High resolution",
    caption: "Every 24-bit, DSD or above-48 kHz record in your library.",
  },
  recentReleases: {
    title: "Fresh releases",
    caption: "Every record from the newest release year in your collection.",
  },
  rediscover: {
    title: "Rediscover",
    caption: "Records you have not heard recently. Playing one removes it from this shelf.",
  },
};

interface LibraryViewProps {
  collection?: AlbumCollection | null;
  genre?: string | null;
}

/** Album mosaic shared by Library and every Home “See all” destination. */
export function LibraryView({ collection = null, genre = null }: LibraryViewProps): JSX.Element {
  const stats = useLibraryStore((state) => state.stats);
  const query = useLibraryStore((state) => state.query);
  const setQuery = useLibraryStore((state) => state.setQuery);
  const coverSize = useUiStore((state) => state.coverSize);
  const setAppearance = useUiStore((state) => state.setAppearance);
  const back = useUiStore((state) => state.back);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const storedSort = useBrowseStore((state) => state.sort);
  const storedGenre = useBrowseStore((state) => state.genre);
  const decade = useBrowseStore((state) => state.decade);
  const year = useBrowseStore((state) => state.year);
  const cycleStoredSort = useBrowseStore((state) => state.cycleSort);
  const setStoredGenre = useBrowseStore((state) => state.setGenre);
  const clearFilters = useBrowseStore((state) => state.clearFilters);
  const setDecade = useBrowseStore((state) => state.setDecade);
  const setYear = useBrowseStore((state) => state.setYear);

  const curated = collection !== null || genre !== null;
  const [collectionSort, setCollectionSort] = useState<AlbumSort | null>(null);
  const [albums, setAlbums] = useState<AlbumRow[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyAlbum, setBusyAlbum] = useState<string | null>(null);
  const [facets, setFacets] = useState<Facets | null>(null);
  const sentinel = useRef<HTMLDivElement | null>(null);

  const sort = curated ? collectionSort : storedSort;
  const activeGenre = curated ? genre : storedGenre;
  const activeDecade = curated ? null : decade;
  const activeYear = curated ? null : year;
  const yearFrom = activeYear ?? activeDecade;
  const yearTo = activeYear ?? (activeDecade === null ? null : activeDecade + 9);
  const libraryVersion = stats?.trackCount ?? 0;

  useEffect(() => {
    setCollectionSort(null);
  }, [collection, genre]);

  useEffect(() => {
    if (curated) {
      return;
    }
    let active = true;
    void libraryApi.facets().then((next) => {
      if (active) {
        setFacets(next);
      }
    });
    return () => {
      active = false;
    };
  }, [curated]);

  useEffect(() => {
    setAlbums([]);
    setOffset(0);
    setLoading(true);
  }, [query, sort, activeGenre, yearFrom, yearTo, collection, libraryVersion]);

  useEffect(() => {
    let active = true;
    void libraryApi
      .albums({
        ...(query.length > 0 ? { text: query } : {}),
        ...(activeGenre !== null ? { genre: activeGenre } : {}),
        ...(yearFrom !== null ? { yearFrom } : {}),
        ...(yearTo !== null ? { yearTo } : {}),
        ...(collection !== null ? { collection } : {}),
        ...(sort !== null ? { sort } : {}),
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
          setError(cause instanceof Error ? cause.message : "The albums could not be loaded");
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
  }, [query, sort, activeGenre, yearFrom, yearTo, collection, offset, libraryVersion]);

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

  const libraryContinuation = {
    ...(query.length > 0 ? { text: query } : {}),
    ...(activeGenre !== null ? { genre: activeGenre } : {}),
    ...(yearFrom !== null ? { yearFrom } : {}),
    ...(yearTo !== null ? { yearTo } : {}),
    ...(collection !== null ? { collection } : {}),
    ...(sort !== null ? { sort } : {}),
  } satisfies Omit<LibraryContinuation, "currentAlbumId">;

  const playAlbum = async (album: AlbumRow): Promise<void> => {
    setBusyAlbum(album.id);
    try {
      const page = await libraryApi.search({ albumId: album.id, sort: "album", limit: 500 });
      if (page.items.length === 0) {
        return;
      }
      await playTracks(page.items, 0, {
        album,
        continuation: { currentAlbumId: album.id, ...libraryContinuation },
      });
    } finally {
      setBusyAlbum(null);
    }
  };

  const columns = useMemo(
    () => `repeat(auto-fill, minmax(${String(coverSize)}px, 1fr))`,
    [coverSize],
  );

  const heading = curated
    ? genre !== null
      ? { title: genre, caption: "Every album in your library with this genre." }
      : collection !== null
        ? COLLECTION_TITLES[collection]
        : { title: "Library", caption: "" }
    : null;

  const decadeValues = (facets?.decades ?? [])
    .map((facet) => Number.parseInt(facet.value, 10))
    .filter(Number.isFinite)
    .sort((a, b) => a - b);
  const decadeIndex = activeDecade === null ? -1 : decadeValues.indexOf(activeDecade);
  const yearsInDecade = (facets?.years ?? []).filter((facet) => {
    const value = Number(facet.value);
    return activeDecade !== null && value >= activeDecade && value <= activeDecade + 9;
  });

  const changeSort = (field: AlbumSortField): void => {
    if (curated) {
      setCollectionSort((current) => nextSort(current, field));
    } else {
      cycleStoredSort(field);
    }
  };

  return (
    <div className="mx-auto w-full max-w-[1560px] px-8 pt-7 pb-10">
      {curated ? (
        <button
          type="button"
          onClick={back}
          className="mb-5 flex items-center gap-1.5 text-[11.5px] text-white/40 transition-colors hover:text-white"
        >
          <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
          Back
        </button>
      ) : null}

      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="min-w-0">
          <h1 className="flex items-center gap-2 text-[26px] leading-none font-semibold tracking-[-0.03em] text-white">
            <LayoutGrid className="h-5 w-5 text-white/25" aria-hidden="true" strokeWidth={1.5} />
            {heading?.title ?? "Library"}
          </h1>
          <p className="mt-1.5 text-[12.5px] text-white/40">
            {heading !== null
              ? heading.caption
              : stats === null
                ? "Reading your music…"
                : `${formatCount(total)} albums · ${formatCount(stats.trackCount)} tracks · ${formatCount(stats.artistCount)} artists`}
          </p>
          {query.length > 0 ? (
            <p className="mt-1 text-[11.5px] text-white/45">
              Search: <span className="text-white/75">“{query}”</span>
            </p>
          ) : null}
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
              Clear search
            </button>
          ) : null}
        </div>
      </header>

      <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-1.5" aria-label="Sort albums">
          <SortButton field="artist" label="Artist" sort={sort} onChange={changeSort} />
          <SortButton field="title" label="Album" sort={sort} onChange={changeSort} />
          <SortButton field="year" label="Year" sort={sort} onChange={changeSort} />
        </div>

        {!curated ? (
          <div className="flex flex-wrap items-center gap-2">
            {storedGenre !== null || activeDecade !== null ? (
              <button
                type="button"
                onClick={clearFilters}
                title="Clear genre, decade and year filters"
                className="flex h-8 items-center gap-1.5 rounded-full border border-white/10 px-3 text-[11px] text-white/50 transition-colors hover:bg-white/[0.05] hover:text-white"
              >
                <X className="h-3 w-3" aria-hidden="true" />
                Clear filters
              </button>
            ) : null}
            <PopoverSelect
              ariaLabel="Filter by genre"
              value={storedGenre ?? ""}
              onChange={(value) => {
                setStoredGenre(value || null);
              }}
              options={[
                { value: "", label: "All genres" },
                ...(facets?.genres.map((facet) => ({
                  value: facet.value,
                  label: `${facet.value} (${formatAlbumCount(facet.count)})`,
                })) ?? []),
                ...(storedGenre !== null && !facets?.genres.some((g) => g.value === storedGenre)
                  ? [{ value: storedGenre, label: storedGenre }]
                  : []),
              ]}
              buttonClassName="w-[220px]"
              menuClassName="w-[280px]"
            />
            <PopoverSelect
              ariaLabel="Filter by decade"
              value={activeDecade === null ? "" : String(activeDecade)}
              onChange={(value) => {
                setDecade(value.length === 0 ? null : Number(value));
              }}
              options={[
                { value: "", label: "All decades" },
                ...(facets?.decades.map((facet) => {
                  const start = Number.parseInt(facet.value, 10);
                  return {
                    value: String(start),
                    label: `${String(start)}s (${formatAlbumCount(facet.count)})`,
                  };
                }) ?? []),
              ]}
              buttonClassName="w-[190px]"
              menuClassName="w-[230px]"
            />
          </div>
        ) : null}
      </div>

      {activeDecade !== null ? (
        <div
          aria-label="Filter by year"
          className="mt-3 flex flex-wrap items-center gap-1.5 rounded-2xl border border-white/[0.055] bg-white/[0.015] px-2 py-2"
        >
          <button
            type="button"
            aria-label="Previous decade"
            title="Previous decade"
            disabled={decadeIndex <= 0}
            onClick={() => {
              const previous = decadeValues[decadeIndex - 1];
              if (previous !== undefined) setDecade(previous);
            }}
            className="grid h-7 w-7 place-items-center rounded-full text-white/45 transition-colors hover:bg-white/[0.07] hover:text-white disabled:opacity-20 disabled:hover:bg-transparent"
          >
            <ChevronLeft className="h-4 w-4" aria-hidden="true" />
          </button>
          <span className="px-1 text-[10px] font-semibold tracking-[0.12em] text-white/30 uppercase">
            {activeDecade}s
          </span>
          <span className="mx-1 h-4 w-px bg-white/[0.07]" aria-hidden="true" />
          <button
            type="button"
            aria-pressed={activeYear === null}
            onClick={() => {
              setYear(null);
            }}
            className={
              activeYear === null
                ? "h-7 rounded-full bg-accent/[0.14] px-2.5 text-[11px] font-medium text-accent"
                : "h-7 rounded-full px-2.5 text-[11px] text-white/45 hover:bg-white/[0.06] hover:text-white"
            }
          >
            All years
          </button>
          {yearsInDecade.map((facet) => {
            const value = Number(facet.value);
            return (
              <button
                key={facet.value}
                type="button"
                aria-pressed={activeYear === value}
                onClick={() => {
                  setYear(activeYear === value ? null : value);
                }}
                className={
                  activeYear === value
                    ? "h-7 rounded-full bg-accent/[0.14] px-2.5 text-[11px] font-medium text-accent"
                    : "h-7 rounded-full px-2.5 text-[11px] text-white/45 hover:bg-white/[0.06] hover:text-white"
                }
              >
                {facet.value}
              </button>
            );
          })}
          <button
            type="button"
            aria-label="Next decade"
            title="Next decade"
            disabled={decadeIndex < 0 || decadeIndex >= decadeValues.length - 1}
            onClick={() => {
              const next = decadeValues[decadeIndex + 1];
              if (next !== undefined) setDecade(next);
            }}
            className="ml-auto grid h-7 w-7 place-items-center rounded-full text-white/45 transition-colors hover:bg-white/[0.07] hover:text-white disabled:opacity-20 disabled:hover:bg-transparent"
          >
            <ChevronRight className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      ) : null}

      {error !== null ? (
        <p role="alert" className="mt-4 text-[12px] text-critical">
          {error}
        </p>
      ) : null}

      {total === 0 && !loading ? (
        <div className="mt-10 rounded-2xl border border-dashed border-white/[0.09] px-6 py-14 text-center">
          <p className="text-[13px] font-medium text-white/55">No albums to show</p>
          <p className="mx-auto mt-1 max-w-md text-[11.5px] leading-relaxed text-white/30">
            {query.length > 0 || activeGenre !== null || activeDecade !== null
              ? "Try another search or clear the genre and decade filters."
              : "Add a music folder in Settings and run a scan."}
          </p>
        </div>
      ) : (
        <div
          className="density-grid mt-5 grid gap-x-5"
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
              continuation={libraryContinuation}
            />
          ))}
        </div>
      )}

      <div ref={sentinel} aria-hidden="true" className="h-8" />

      {loading ? (
        <div className="mt-2 flex items-center gap-2 text-[11px] text-white/30">
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-white/40" />
          Reading your music…
        </div>
      ) : null}
    </div>
  );
}

function nextSort(current: AlbumSort | null, field: AlbumSortField): AlbumSort {
  if (field === "artist") {
    return current === "artist" ? "artistDesc" : "artist";
  }
  if (field === "title") {
    return current === "title" ? "titleDesc" : "title";
  }
  return current === "year" ? "yearAsc" : "year";
}

function SortButton({
  field,
  label,
  sort,
  onChange,
}: {
  field: AlbumSortField;
  label: string;
  sort: AlbumSort | null;
  onChange: (field: AlbumSortField) => void;
}): JSX.Element {
  const ascendingSort: AlbumSort =
    field === "artist" ? "artist" : field === "title" ? "title" : "yearAsc";
  const descendingSort: AlbumSort =
    field === "artist" ? "artistDesc" : field === "title" ? "titleDesc" : "year";
  const ascending = sort === ascendingSort;
  const selected = ascending || sort === descendingSort;
  const Icon = sort === descendingSort ? ArrowDown : ArrowUp;

  return (
    <button
      type="button"
      aria-pressed={selected}
      title={`Change ${label.toLowerCase()} order; click again to reverse it`}
      onClick={() => {
        onChange(field);
      }}
      className={
        selected
          ? "flex h-8 items-center gap-1.5 rounded-full border border-white/[0.12] bg-white/[0.09] px-3 text-[11.5px] text-white"
          : "flex h-8 items-center gap-1.5 rounded-full border border-white/[0.07] bg-white/[0.025] px-3 text-[11.5px] text-white/45 hover:bg-white/[0.05] hover:text-white/80"
      }
    >
      {label}
      {selected ? <Icon className="h-3 w-3" aria-hidden="true" /> : null}
      <span className="sr-only">{ascending ? "ascending" : "descending"}</span>
    </button>
  );
}
