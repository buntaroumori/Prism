import { Search, SlidersHorizontal, X } from "lucide-react";
import { useEffect, useMemo, useState, type JSX } from "react";

import { VirtualTrackList } from "@/components/library/VirtualTrackList";
import { SectionHeading } from "@/components/SectionHeading";
import { Segmented } from "@/components/ui/Segmented";
import { formatCount } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { PAGE_SIZE } from "@/hooks/useTrackWindow";
import type { TrackQuery, TrackSort } from "@/types/ipc";

const SORTS: readonly { value: TrackSort; label: string }[] = [
  { value: "title", label: "Title" },
  { value: "artist", label: "Artist" },
  { value: "album", label: "Album" },
  { value: "added", label: "Added" },
];

/**
 * The library browser.
 *
 * A 500 000-track catalogue on screen at once is the headline requirement, so the
 * page is built around one rule: never materialise the library. The list
 * virtualises, the data layer pages, and "play from here" queues one bounded
 * window instead of pretending a 500k-item queue fits through IPC.
 *
 * Search is debounced locally (250 ms) and executed by SQLite FTS5 in the
 * backend, so typing never blocks the list.
 */
export function LibraryView(): JSX.Element {
  const stats = useLibraryStore((state) => state.stats);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const [text, setText] = useState("");
  const [debounced, setDebounced] = useState("");
  const [sort, setSort] = useState<TrackSort>("title");

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(text.trim());
    }, 250);
    return () => {
      window.clearTimeout(timer);
    };
  }, [text]);

  const query = useMemo<TrackQuery>(
    () => ({
      text: debounced.length > 0 ? debounced : null,
      sort,
      includeMissing: true,
    }),
    [debounced, sort],
  );

  /**
   * Queue the page the row belongs to, starting at the row.
   *
   * Playing a 500k-track queue would mean shipping 500k `TrackRequest` objects
   * through IPC and pinning them in the engine; one page is the honest unit and
   * matches what the viewport can actually show.
   */
  const playFromIndex = async (index: number): Promise<void> => {
    const offset = Math.floor(index / PAGE_SIZE) * PAGE_SIZE;
    const page = await libraryApi.search({ ...query, limit: PAGE_SIZE, offset });
    if (page.items.length === 0) {
      return;
    }
    await playTracks(page.items, index - offset);
  };

  return (
    <div className="mx-auto flex h-full w-full max-w-[1420px] flex-col px-8 pt-8 pb-6">
      <SectionHeading
        eyebrow="Library"
        title={stats === null ? "Tracks" : `${formatCount(stats.trackCount)} tracks`}
        description="Every row is fetched on demand and painted at a fixed height: the list stays at 60 rows in the DOM whether the catalogue holds hundreds or half a million."
        action={
          <Segmented
            options={SORTS}
            value={sort}
            onChange={setSort}
            label="Sort tracks"
            layoutId="library-sort"
          />
        }
      />

      <div className="mt-6 flex items-center gap-3">
        <div className="group relative flex h-10 flex-1 items-center rounded-full border border-white/[0.08] bg-white/[0.03] px-4 focus-within:border-accent/40">
          <Search className="h-4 w-4 shrink-0 text-white/30" aria-hidden="true" />
          <input
            value={text}
            onChange={(event) => {
              setText(event.target.value);
            }}
            placeholder="Search titles, artists, albums, genres…"
            aria-label="Search the library"
            className="ml-2.5 h-full w-full bg-transparent text-[13px] text-white placeholder:text-white/25 focus:outline-none"
          />
          {text.length > 0 ? (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => {
                setText("");
              }}
              className="grid h-6 w-6 place-items-center rounded-full text-white/30 hover:bg-white/[0.06] hover:text-white/70"
            >
              <X className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          ) : null}
        </div>
        <div className="hidden items-center gap-1.5 rounded-full border border-white/[0.06] px-3 py-2 text-[11px] text-white/30 sm:flex">
          <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden="true" />
          FTS5 · indexed
        </div>
      </div>

      <VirtualTrackList
        query={query}
        className="mt-4 flex-1"
        onActivate={(_row, index) => {
          void playFromIndex(index);
        }}
        onPlayFrom={(index) => {
          void playFromIndex(index);
        }}
        emptyTitle={debounced.length > 0 ? "Nothing matches that search" : "The library is empty"}
        emptyBody={
          debounced.length > 0
            ? "Try a shorter query, or clear the search to browse everything."
            : "Add a folder in Settings and run a scan; Prism reads embedded tags, CUE sheets and artwork."
        }
      />
    </div>
  );
}
