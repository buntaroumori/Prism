import type { JSX } from "react";
import { ListMusic } from "lucide-react";

import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useContextMenuStore } from "@/stores/contextMenuStore";

/** Rows painted at once; a queue module is a window onto the queue, not the queue. */
const VISIBLE_ROWS = 6;

/**
 * Upcoming tracks.
 *
 * Shows the next few entries and the one playing, with the played portion dimmed.
 * Clicking an entry re-queues the same list from that index, which is what the
 * engine already supports — no queue mutation protocol is invented here.
 */
export function QueueModule(): JSX.Element {
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const play = usePlaybackStore((state) => state.play);
  const openAt = useContextMenuStore((state) => state.openAt);

  const start = Math.min(index + 1, Math.max(queue.length - 1, 0));
  const upcoming = queue.slice(start, start + VISIBLE_ROWS);

  return (
    <div className="h-full rounded-3xl border border-white/[0.06] bg-white/[0.02] p-5 backdrop-blur-xl">
      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold tracking-[0.14em] text-white/55 uppercase">
          Queue
        </h3>
        <span className="tabular text-[10px] text-white/25">
          {queue.length > 0 ? `${String(index + 1)} / ${String(queue.length)}` : "empty"}
        </span>
      </div>

      {queue.length === 0 ? (
        <div className="mt-6 flex flex-col items-center gap-2 py-6 text-center">
          <ListMusic className="h-5 w-5 text-white/20" aria-hidden="true" strokeWidth={1.5} />
          <p className="text-[11.5px] text-white/30">Nothing queued. Play an album from Home.</p>
        </div>
      ) : (
        <ol className="mt-3 space-y-0.5">
          {upcoming.map((entry, offset) => {
            const absolute = start + offset;
            return (
              <li key={`${entry.trackId}-${String(absolute)}`}>
                <button
                  type="button"
                  onClick={() => {
                    void play(queue, absolute);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    openAt(event.clientX, event.clientY, {
                      kind: "track",
                      track: entry.catalogue,
                    });
                  }}
                  className="flex w-full items-center gap-3 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-white/[0.04]"
                >
                  <span className="tabular w-4 shrink-0 text-[10px] text-white/20">
                    {String(absolute + 1)}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12px] text-white/75">{entry.title}</span>
                    <span className="block truncate text-[10.5px] text-white/30">
                      {entry.artist}
                    </span>
                  </span>
                  <span className="tabular shrink-0 text-[10.5px] text-white/25">
                    {formatDuration(entry.durationMs)}
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      )}

      {queue.length > start + VISIBLE_ROWS ? (
        <p className={cn("mt-2 text-center text-[10px] text-white/20")}>
          +{String(queue.length - start - VISIBLE_ROWS)} more
        </p>
      ) : null}
    </div>
  );
}
