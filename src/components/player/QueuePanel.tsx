import { Reorder, motion, useDragControls } from "framer-motion";
import { GripVertical, ListMusic, Play, X } from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type JSX,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { createPortal } from "react-dom";

import { CoverArt } from "@/components/ui/CoverArt";
import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { usePlaybackStore, type QueueEntry } from "@/stores/playbackStore";

interface QueuePanelProps {
  onClose: () => void;
}

interface QueueBlock {
  id: string;
  kind: "album" | "track";
  title: string;
  artist: string;
  entries: QueueEntry[];
}

function makeBlocks(entries: QueueEntry[]): QueueBlock[] {
  const blocks: QueueBlock[] = [];
  for (const entry of entries) {
    const previous = blocks.at(-1);
    if (previous?.id === entry.groupId) {
      previous.entries.push(entry);
    } else {
      blocks.push({
        id: entry.groupId,
        kind: entry.groupKind,
        title: entry.groupTitle,
        artist: entry.catalogue.albumArtist || entry.artist,
        entries: [entry],
      });
    }
  }
  return blocks;
}

/** Full queue editor. Only upcoming entries move; the loaded song stays fixed. */
export function QueuePanel({ onClose }: QueuePanelProps): JSX.Element {
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const updateQueue = usePlaybackStore((state) => state.updateQueue);
  const jumpTo = usePlaybackStore((state) => state.jumpTo);
  const [draft, setDraft] = useState<QueueEntry[] | null>(null);
  const draftRef = useRef<QueueEntry[] | null>(null);
  const shownQueue = draft ?? queue;
  const shownIndex = Math.min(index, Math.max(shownQueue.length - 1, 0));
  const current = shownQueue[shownIndex] ?? null;
  const upcoming = shownQueue.slice(shownIndex + 1);
  const blocks = useMemo(() => makeBlocks(upcoming), [upcoming]);
  const positions = useMemo(
    () => new Map(shownQueue.map((entry, position) => [entry.queueId, position])),
    [shownQueue],
  );

  useEffect(() => {
    const escape = (event: KeyboardEvent): void => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("keydown", escape);
    };
  }, [onClose]);

  const changeDraft = (next: QueueEntry[]): void => {
    draftRef.current = next;
    setDraft(next);
  };

  const beginDrag = (): void => {
    if (draftRef.current === null) changeDraft(shownQueue);
  };

  const commitDraft = (): void => {
    const next = draftRef.current;
    draftRef.current = null;
    setDraft(null);
    if (next !== null) void updateQueue(next);
  };

  const jump = (queueId: string): void => {
    const target = usePlaybackStore
      .getState()
      .queue.findIndex((entry) => entry.queueId === queueId);
    if (target >= 0) void jumpTo(target);
  };

  return createPortal(
    <>
      <motion.button
        type="button"
        aria-label="Close queue"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        onClick={onClose}
        className="fixed inset-0 z-[39] cursor-default bg-black/25"
      />
      <motion.aside
        role="dialog"
        aria-label="Playback queue"
        initial={{ opacity: 0, x: 24, scale: 0.985 }}
        animate={{ opacity: 1, x: 0, scale: 1 }}
        exit={{ opacity: 0, x: 18, scale: 0.99 }}
        transition={{ type: "spring", stiffness: 360, damping: 34 }}
        className="glass fixed top-4 right-4 bottom-[108px] z-40 flex w-[min(470px,calc(100vw-32px))] flex-col overflow-hidden rounded-3xl border border-white/[0.09] shadow-2xl"
      >
        <header className="flex shrink-0 items-center justify-between border-b border-white/[0.07] px-5 py-4">
          <div>
            <h2 className="flex items-center gap-2 text-[14px] font-semibold text-white">
              <ListMusic className="h-4 w-4 text-accent" aria-hidden="true" />
              Queue
            </h2>
            <p className="mt-0.5 text-[11px] text-white/35">
              {queue.length === 0
                ? "Nothing queued"
                : `${String(index + 1)} of ${String(queue.length)} · ${String(Math.max(queue.length - index - 1, 0))} upcoming`}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close queue"
            onClick={onClose}
            className="grid h-8 w-8 place-items-center rounded-full text-white/40 hover:bg-white/[0.07] hover:text-white"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto p-3" data-interactive>
          {current !== null ? (
            <section aria-label="Current song" className="mb-3">
              <p className="px-2 pb-1.5 text-[9.5px] font-semibold tracking-[0.15em] text-accent/70 uppercase">
                Now playing
              </p>
              <QueueRow
                entry={current}
                position={(positions.get(current.queueId) ?? 0) + 1}
                current
                onJump={jump}
              />
            </section>
          ) : null}

          {blocks.length === 0 ? (
            <div className="grid place-items-center px-6 py-14 text-center">
              <ListMusic className="mb-3 h-7 w-7 text-white/15" aria-hidden="true" />
              <p className="text-[12px] text-white/40">
                {current === null ? "Play an album, playlist or track." : "End of the queue."}
              </p>
              <p className="mt-1 text-[10.5px] text-white/25">
                Use Play next on any album or track to add it here.
              </p>
            </div>
          ) : (
            <section aria-label="Upcoming songs">
              <p className="px-2 pb-2 text-[9.5px] font-semibold tracking-[0.15em] text-white/30 uppercase">
                Up next
              </p>
              <Reorder.Group
                axis="y"
                values={blocks}
                onReorder={(nextBlocks) => {
                  changeDraft([
                    ...shownQueue.slice(0, shownQueue.length - upcoming.length),
                    ...nextBlocks.flatMap((block) => block.entries),
                  ]);
                }}
                className="space-y-2"
              >
                {blocks.map((block) => (
                  <QueueBlockItem
                    key={block.id}
                    block={block}
                    positions={positions}
                    onJump={jump}
                    onReorder={(entries) => {
                      changeDraft([
                        ...shownQueue.slice(0, shownQueue.length - upcoming.length),
                        ...blocks.flatMap((candidate) =>
                          candidate.id === block.id ? entries : candidate.entries,
                        ),
                      ]);
                    }}
                    onCommit={commitDraft}
                    onBeginDrag={beginDrag}
                  />
                ))}
              </Reorder.Group>
            </section>
          )}
        </div>
        <footer className="shrink-0 border-t border-white/[0.06] px-5 py-2.5 text-[10px] text-white/25">
          Drag albums or songs to reorder · Right-click to remove
        </footer>
      </motion.aside>
    </>,
    document.body,
  );
}

function QueueBlockItem({
  block,
  positions,
  onJump,
  onReorder,
  onCommit,
  onBeginDrag,
}: {
  block: QueueBlock;
  positions: ReadonlyMap<string, number>;
  onJump: (queueId: string) => void;
  onReorder: (entries: QueueEntry[]) => void;
  onCommit: () => void;
  onBeginDrag: () => void;
}): JSX.Element {
  const controls = useDragControls();
  const openAt = useContextMenuStore((state) => state.openAt);
  const album = block.kind === "album";
  const first = block.entries[0];

  return (
    <Reorder.Item
      value={block}
      dragListener={!album}
      dragControls={controls}
      onDragStart={onBeginDrag}
      onDragEnd={onCommit}
      className={cn(
        "relative list-none overflow-hidden rounded-2xl border bg-oled-950/95",
        album ? "border-accent/[0.13] bg-accent/[0.025]" : "border-white/[0.055]",
      )}
    >
      {album && first !== undefined ? (
        <div
          onPointerDown={(event: ReactPointerEvent<HTMLDivElement>) => {
            onBeginDrag();
            controls.start(event);
          }}
          onContextMenu={(event) => {
            event.preventDefault();
            openAt(event.clientX, event.clientY, {
              kind: "queue-album",
              groupId: block.id,
              firstQueueId: first.queueId,
              title: block.title,
              artist: block.artist,
            });
          }}
          className="flex cursor-grab touch-none items-center gap-3 border-b border-white/[0.055] px-3 py-2.5 select-none active:cursor-grabbing"
        >
          <CoverArt
            path={first.catalogue.coverPath}
            url={first.catalogue.coverUrl}
            alt=""
            className="h-9 w-9 shrink-0"
            rounded="rounded-lg"
            fallbackClassName="h-4 w-4"
          />
          <div className="min-w-0 flex-1">
            <p className="truncate text-[11.5px] font-medium text-white/75">{block.title}</p>
            <p className="truncate text-[10px] text-white/30">
              {block.artist} · Album · {String(block.entries.length)} songs
            </p>
          </div>
          <GripVertical className="h-4 w-4 shrink-0 text-white/20" aria-hidden="true" />
        </div>
      ) : null}

      {album ? (
        <Reorder.Group axis="y" values={block.entries} onReorder={onReorder} className="p-1">
          {block.entries.map((entry) => (
            <Reorder.Item
              key={entry.queueId}
              value={entry}
              onDragStart={onBeginDrag}
              onDragEnd={onCommit}
              className="relative list-none rounded-xl bg-oled-950/95"
            >
              <QueueRow
                entry={entry}
                position={(positions.get(entry.queueId) ?? 0) + 1}
                onJump={onJump}
              />
            </Reorder.Item>
          ))}
        </Reorder.Group>
      ) : first !== undefined ? (
        <QueueRow
          entry={first}
          position={(positions.get(first.queueId) ?? 0) + 1}
          onJump={onJump}
        />
      ) : null}
    </Reorder.Item>
  );
}

function QueueRow({
  entry,
  position,
  onJump,
  current = false,
}: {
  entry: QueueEntry;
  position: number;
  onJump: (queueId: string) => void;
  current?: boolean;
}): JSX.Element {
  const openAt = useContextMenuStore((state) => state.openAt);
  return (
    <div
      onDoubleClick={() => {
        if (!current) onJump(entry.queueId);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        openAt(event.clientX, event.clientY, { kind: "queue-track", entry });
      }}
      className={cn(
        "group flex min-w-0 items-center gap-2 rounded-xl px-2 py-2 select-none",
        current ? "bg-accent/[0.055]" : "cursor-pointer hover:bg-white/[0.04]",
      )}
    >
      <span className="tabular w-5 shrink-0 text-center text-[10px] text-white/20">
        {String(position).padStart(2, "0")}
      </span>
      <button
        type="button"
        aria-label={`Play ${entry.title}`}
        title={current ? `${entry.title} is playing` : `Play ${entry.title}`}
        disabled={current}
        onClick={(event) => {
          event.stopPropagation();
          if (event.detail === 1 && !current) onJump(entry.queueId);
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
        }}
        className="grid h-7 w-7 shrink-0 place-items-center rounded-full text-white/25 hover:bg-accent/10 hover:text-accent disabled:text-accent/70"
      >
        <Play className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
      </button>
      <CoverArt
        path={entry.catalogue.coverPath}
        url={entry.catalogue.coverUrl}
        alt=""
        className="h-9 w-9 shrink-0"
        rounded="rounded-lg"
        fallbackClassName="h-4 w-4"
      />
      <div className="min-w-0 flex-1">
        <p className={cn("truncate text-[11.5px]", current ? "text-accent" : "text-white/80")}>
          {entry.title}
        </p>
        <p className="truncate text-[10px] text-white/30">
          {entry.artist || "Unknown artist"}
          {entry.album.length > 0 ? ` · ${entry.album}` : ""}
        </p>
      </div>
      <span className="tabular shrink-0 text-[10px] text-white/25">
        {entry.durationMs > 0 ? formatDuration(entry.durationMs) : "—"}
      </span>
      {!current ? (
        <GripVertical className="h-3.5 w-3.5 shrink-0 text-white/15" aria-hidden="true" />
      ) : null}
    </div>
  );
}
