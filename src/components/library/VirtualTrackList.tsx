import { useVirtualizer } from "@tanstack/react-virtual";
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type JSX,
  type KeyboardEvent,
} from "react";
import { AudioLines, Disc3, Play, TriangleAlert } from "lucide-react";

import { formatDuration, formatCount } from "@/lib/format";
import { cn } from "@/lib/utils";
import {
  libraryTrackFetcher,
  useTrackWindow,
  type TrackFetcher,
  type TrackWindow,
} from "@/hooks/useTrackWindow";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { useUiStore } from "@/stores/uiStore";
import type { TrackQuery, TrackRow } from "@/types/ipc";

/** Fixed row height: the virtualiser only works because this never changes. */
const ROW_HEIGHT = 52;
const OVERSCAN = 12;

/**
 * Virtualiser options, hoisted on purpose.
 *
 * `@tanstack/react-virtual` memoises its measurement cache on the identity of
 * these callbacks. Inline arrows are new objects on every render, so the cache
 * is rebuilt from scratch on every scroll frame — over 500 000 rows that is the
 * single most expensive thing on the main thread (measured: 21–30 % of the
 * frame's JS samples inside `getMeasurements`). Module-level functions are
 * created once, so the cache survives scrolling.
 */
const estimateRowSize = (): number => ROW_HEIGHT;
const rowKey = (index: number): number => index;

interface VirtualTrackListProps {
  query: TrackQuery;
  /** Called with the row the user activated and its absolute index. */
  onActivate: (row: TrackRow, index: number) => void;
  /** Called with the absolute index when the user asks to play from there. */
  onPlayFrom: (index: number) => void;
  emptyTitle?: string;
  emptyBody?: string;
  className?: string;
  /** Page source; production uses the Tauri-backed default. */
  fetcher?: TrackFetcher;
}

/**
 * Track list for libraries of any size.
 *
 * The DOM contains ~30 rows no matter whether the library holds 500 or 500 000
 * entries: `@tanstack/react-virtual` maps scroll position to indices, and
 * `useTrackWindow` fetches only the pages those indices live in. Each row is
 * positioned with a single `transform`, so scrolling is compositor work with no
 * layout thrash; the container uses `contain: strict` and the row count is
 * reserved with one spacer of `totalSize`.
 *
 * Interaction mirrors a desktop player: single click selects, double click or
 * Enter plays, Space is left to the global transport.
 */
export function VirtualTrackList({
  query,
  onActivate,
  onPlayFrom,
  emptyTitle = "No tracks match",
  emptyBody = "Adjust the filters, or scan a folder from Settings.",
  className,
  fetcher = libraryTrackFetcher,
}: VirtualTrackListProps): JSX.Element {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const window = useTrackWindow(query, fetcher);
  const [selected, setSelected] = useState<number>(-1);

  useEffect(() => {
    const clear = (event: PointerEvent): void => {
      if (!(event.target as Element | null)?.closest("[role='gridcell']")) {
        setSelected(-1);
      }
    };
    document.addEventListener("pointerdown", clear);
    return () => {
      document.removeEventListener("pointerdown", clear);
    };
  }, []);

  // Rows read the transport themselves through atomic selectors: this component
  // never subscribes to the playback store, so a track change re-renders the two
  // affected rows instead of the whole list.

  const getScrollElement = useCallback(() => scrollRef.current, []);
  const virtualizer = useVirtualizer({
    count: window.total,
    getScrollElement,
    estimateSize: estimateRowSize,
    overscan: OVERSCAN,
    // Rows are a fixed height, so measurement is a multiplication, not an
    // observation pass.
    getItemKey: rowKey,
  });

  const items = virtualizer.getVirtualItems();
  const firstIndex = items[0]?.index ?? 0;
  const lastIndex = items[items.length - 1]?.index ?? 0;
  const { ensureRange } = window;

  useEffect(() => {
    if (window.total > 0) {
      ensureRange(firstIndex, lastIndex);
    }
  }, [ensureRange, firstIndex, lastIndex, window.total]);

  // A new query means a new list: selection and scroll both reset.
  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  useEffect(() => {
    setSelected(-1);
    scrollRef.current?.scrollTo({ top: 0 });
  }, [queryKey]);

  // Every handler below is stable for the life of the list: the newest props and
  // the newest data window are read through refs, so a parent re-render never
  // invalidates sixty memoised rows.
  const latest = useRef({ window, onActivate, onPlayFrom });
  latest.current = { window, onActivate, onPlayFrom };

  const selectIndex = useCallback((index: number) => {
    setSelected(index);
  }, []);
  const activateIndex = useCallback((index: number) => {
    const { window: current, onActivate: activate } = latest.current;
    const row = current.rowAt(index);
    if (row !== undefined) {
      activate(row, index);
    }
  }, []);
  const playIndex = useCallback((index: number) => {
    latest.current.onPlayFrom(index);
  }, []);

  const moveSelection = (delta: number): void => {
    if (window.total === 0) {
      return;
    }
    const next = Math.min(Math.max(selected + delta, 0), window.total - 1);
    setSelected(next);
    virtualizer.scrollToIndex(next, { align: "auto" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        moveSelection(event.shiftKey ? 10 : 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        moveSelection(event.shiftKey ? -10 : -1);
        break;
      case "PageDown":
        event.preventDefault();
        moveSelection(12);
        break;
      case "PageUp":
        event.preventDefault();
        moveSelection(-12);
        break;
      case "Home":
        event.preventDefault();
        moveSelection(-window.total);
        break;
      case "End":
        event.preventDefault();
        moveSelection(window.total);
        break;
      case "Enter": {
        event.preventDefault();
        const row = window.rowAt(selected);
        if (row !== undefined) {
          onActivate(row, selected);
        }
        break;
      }
      default:
        break;
    }
  };

  return (
    <div className={cn("flex min-h-0 flex-col", className)}>
      <ListHeader />

      {window.error !== null ? (
        <div className="flex items-center gap-2 border-b border-critical/15 bg-critical/[0.06] px-4 py-2.5 text-[12px] text-critical">
          <TriangleAlert className="h-3.5 w-3.5" aria-hidden="true" />
          {window.error.message}
        </div>
      ) : null}

      <div
        ref={scrollRef}
        role="grid"
        aria-rowcount={window.total}
        aria-label="Tracks"
        tabIndex={0}
        onKeyDown={onKeyDown}
        className="relative min-h-0 flex-1 scrollbar-thin overflow-x-hidden overflow-y-auto focus:outline-none"
        style={{ contain: "strict" }}
        data-loading={window.loading}
      >
        {window.total === 0 && !window.loading ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 px-8 text-center">
            <AudioLines className="h-6 w-6 text-white/20" aria-hidden="true" strokeWidth={1.5} />
            <p className="text-[13px] font-medium text-white/55">{emptyTitle}</p>
            <p className="max-w-sm text-[11.5px] leading-relaxed text-white/30">{emptyBody}</p>
          </div>
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }}>
            {items.map((item) => {
              const row = window.rowAt(item.index);
              return (
                <div
                  key={item.key}
                  role="row"
                  aria-rowindex={item.index + 1}
                  className="absolute top-0 left-0 w-full will-change-transform"
                  style={{ height: item.size, transform: `translateY(${String(item.start)}px)` }}
                >
                  {row === undefined ? (
                    <SkeletonRow />
                  ) : (
                    <TrackRowItem
                      row={row}
                      index={item.index}
                      selected={selected === item.index}
                      onSelectIndex={selectIndex}
                      onActivateIndex={activateIndex}
                      onPlayFromIndex={playIndex}
                    />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      <ListFooter window={window} />
    </div>
  );
}

function ListHeader(): JSX.Element {
  return (
    <div
      role="row"
      className="grid shrink-0 grid-cols-[36px_36px_minmax(0,3fr)_minmax(0,2fr)_88px] items-center gap-3 border-b border-white/[0.06] px-4 pb-2 text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase"
    >
      <span aria-hidden="true">#</span>
      <span aria-hidden="true" />
      <span>Title</span>
      <span>Album</span>
      <span className="text-right">Length</span>
    </div>
  );
}

function ListFooter({ window }: { window: TrackWindow }): JSX.Element {
  return (
    <div className="flex shrink-0 items-center justify-between border-t border-white/[0.06] px-4 py-2 text-[11px] text-white/30">
      <span className="tabular">
        {window.loading ? "Loading…" : `${formatCount(window.total)} tracks`}
      </span>
      <span className="text-[10px] text-white/20">Double-click a song to play it</span>
    </div>
  );
}

function SkeletonRow(): JSX.Element {
  return (
    <div
      aria-hidden="true"
      className="grid h-[52px] grid-cols-[36px_36px_minmax(0,3fr)_minmax(0,2fr)_88px] items-center gap-3 px-4"
    >
      <span className="h-2.5 w-4 animate-pulse rounded bg-white/[0.06]" />
      <span className="h-2.5 w-4 animate-pulse rounded-full bg-white/[0.06]" />
      <span className="h-2.5 w-40 animate-pulse rounded bg-white/[0.06]" />
      <span className="h-2.5 w-28 animate-pulse rounded bg-white/[0.04]" />
      <span className="h-2.5 w-8 animate-pulse justify-self-end rounded bg-white/[0.04]" />
    </div>
  );
}

interface TrackRowItemProps {
  row: TrackRow;
  index: number;
  selected: boolean;
  onSelectIndex: (index: number) => void;
  onActivateIndex: (index: number) => void;
  onPlayFromIndex: (index: number) => void;
}

/**
 * One row.
 *
 * Memoised on its own data plus two atomic store selectors: the transport state
 * reaches the row directly, so a track change costs two re-renders rather than
 * sixty, and scrolling costs none at all (the `row` object reference is stable
 * while the page cache keeps it).
 */
const TrackRowItem = memo(function TrackRowItem({
  row,
  index,
  selected,
  onSelectIndex,
  onActivateIndex,
  onPlayFromIndex,
}: TrackRowItemProps): JSX.Element {
  const current = usePlaybackStore((state) => state.status?.track?.trackId === row.id);
  const playing = usePlaybackStore(
    (state) => state.status?.track?.trackId === row.id && state.status.state === "playing",
  );
  const openAt = useContextMenuStore((state) => state.openAt);

  return (
    <div
      role="gridcell"
      onClick={() => {
        onSelectIndex(index);
      }}
      onDoubleClick={() => {
        onActivateIndex(index);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        onSelectIndex(index);
        openAt(event.clientX, event.clientY, { kind: "track", track: row });
      }}
      className={cn(
        "group grid h-[52px] grid-cols-[36px_36px_minmax(0,3fr)_minmax(0,2fr)_88px] items-center gap-3 px-4 transition-colors duration-100",
        selected ? "bg-white/[0.055]" : "hover:bg-white/[0.03]",
        current && "bg-accent/[0.06]",
        row.missing && "opacity-45",
      )}
    >
      <span className="tabular grid h-7 w-7 place-items-center text-[11px] text-white/25">
        {current ? (
          <AudioLines
            className={cn("h-3.5 w-3.5 text-accent", playing && "animate-pulse")}
            aria-hidden="true"
          />
        ) : (
          String(index + 1).padStart(2, "0")
        )}
      </span>
      <button
        type="button"
        onClick={(event) => {
          event.stopPropagation();
          if (event.detail === 1) onPlayFromIndex(index);
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
        }}
        aria-label={`Play ${row.title}`}
        title={`Play ${row.title}`}
        className="grid h-7 w-7 place-items-center rounded-full text-white/25 transition-colors hover:bg-accent/10 hover:text-accent focus-visible:outline-2 focus-visible:outline-accent"
      >
        <Play className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
      </button>

      <div className="min-w-0">
        <p
          className={cn(
            "truncate text-[13px] font-medium",
            current ? "text-accent" : "text-white/85",
          )}
          title={row.title}
        >
          {row.title}
        </p>
        <p className="flex min-w-0 items-center gap-1.5 truncate text-[11px] text-white/35">
          <ArtistButton id={row.artistId} name={row.artist} />
          {row.cueStartMs !== null ? (
            <span className="ml-1.5 rounded bg-white/[0.06] px-1 py-px text-[9px] tracking-wide text-white/40 uppercase">
              CUE
            </span>
          ) : null}
        </p>
      </div>

      <div className="flex min-w-0 items-center gap-1.5 text-[11.5px] text-white/40">
        <Disc3 className="h-3 w-3 shrink-0 text-white/20" aria-hidden="true" />
        <AlbumButton id={row.albumId} title={row.album} />
      </div>

      <span className="tabular justify-self-end text-[11.5px] text-white/35">
        {row.durationMs > 0 ? formatDuration(row.durationMs) : "—"}
      </span>
    </div>
  );
});

/** Artist name inside a row: a link when the row knows its artist id. */
function ArtistButton({ id, name }: { id: string | null; name: string }): JSX.Element {
  const openArtist = useUiStore((state) => state.openArtist);
  const label = name.length > 0 ? name : "Unknown artist";
  if (id === null) {
    return <span className="truncate">{label}</span>;
  }
  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        openArtist(id);
      }}
      className="max-w-full truncate text-left hover:text-white hover:underline hover:decoration-white/30 hover:underline-offset-2"
      title={label}
    >
      {label}
    </button>
  );
}

/** Album name inside a row: a link when the row knows its album id. */
function AlbumButton({ id, title }: { id: string | null; title: string }): JSX.Element {
  const openAlbum = useUiStore((state) => state.openAlbum);
  const label = title.length > 0 ? title : "—";
  if (id === null) {
    return <span className="truncate">{label}</span>;
  }
  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        openAlbum(id);
      }}
      className="min-w-0 truncate text-left hover:text-white hover:underline hover:decoration-white/30 hover:underline-offset-2"
      title={label}
    >
      {label}
    </button>
  );
}
