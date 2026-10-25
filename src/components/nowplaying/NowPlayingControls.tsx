import { AnimatePresence } from "framer-motion";
import { ListMusic } from "lucide-react";
import { useState, type JSX } from "react";

import { QueuePanel } from "@/components/player/QueuePanel";
import { Scrubber } from "@/components/player/Scrubber";
import { TransportControls } from "@/components/player/TransportControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { durationMs, EMPTY_STATUS, playheadMs, usePlaybackStore } from "@/stores/playbackStore";
import { usePlaybackPreferencesStore } from "@/stores/playbackPreferencesStore";
import { useUiStore } from "@/stores/uiStore";

/** Complete embedded transport for the bounded Now Playing surface. */
export function NowPlayingControls(): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const tick = usePlaybackStore((state) => state.tick);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const busy = usePlaybackStore((state) => state.busy);
  const pendingVolumeDb = usePlaybackStore((state) => state.pendingVolumeDb);
  const committedVolumeDb = usePlaybackStore((state) => state.committedVolumeDb);
  const shuffled = usePlaybackStore((state) => state.shuffleSnapshot !== null);
  const continuation = usePlaybackStore((state) => state.continuation);
  const albumEnd = usePlaybackPreferencesStore((state) => state.albumEnd);
  const sidebarHidden = useUiStore((state) => state.sidebarHidden);
  const toggleSidebarHidden = useUiStore((state) => state.toggleSidebarHidden);
  const [queueOpen, setQueueOpen] = useState(false);

  const entry = queue.find((item) => item.trackId === status?.track?.trackId) ?? queue[index];
  const position = playheadMs(status, tick);
  const duration = durationMs(status, tick, entry?.durationMs ?? null);
  const canSkip =
    queue.length > 0 &&
    (index + 1 < queue.length || (continuation !== null && albumEnd === "continue-library"));
  const volumeDb = pendingVolumeDb ?? committedVolumeDb ?? status?.volumeDb ?? 0;

  return (
    <div className="shrink-0 border-t border-white/[0.055] pt-3">
      <div className="flex justify-center">
        <TransportControls
          state={status?.state ?? "stopped"}
          canSkip={canSkip}
          busy={busy}
          shuffled={shuffled}
          sidebarHidden={sidebarHidden}
          onToggleSidebar={toggleSidebarHidden}
          onToggleShuffle={() => {
            void usePlaybackStore.getState().toggleShuffle();
          }}
          onToggle={() => {
            void usePlaybackStore.getState().toggle();
          }}
          onNext={() => {
            void usePlaybackStore.getState().next();
          }}
          onPrevious={() => {
            void usePlaybackStore.getState().previous();
          }}
          onStop={() => {
            void usePlaybackStore.getState().stop();
          }}
        />
      </div>
      <div className="mx-auto mt-1 w-full max-w-[620px]">
        <Scrubber
          status={status ?? EMPTY_STATUS}
          positionMs={position}
          durationMs={duration}
          onSeek={(ms) => {
            void usePlaybackStore.getState().seekMs(ms);
          }}
        />
      </div>
      <div className="mt-1 flex items-center justify-center gap-3">
        <button
          type="button"
          aria-label="Open queue"
          aria-expanded={queueOpen}
          title="Queue"
          onClick={() => {
            setQueueOpen((open) => !open);
          }}
          className={
            queueOpen
              ? "grid h-8 w-8 place-items-center rounded-full bg-accent/[0.14] text-accent"
              : "grid h-8 w-8 place-items-center rounded-full text-white/40 transition-colors hover:bg-white/[0.07] hover:text-white"
          }
        >
          <ListMusic className="h-[17px] w-[17px]" aria-hidden="true" />
        </button>
        <VolumeControl
          volumeDb={volumeDb}
          muted={status?.muted ?? false}
          onPreview={usePlaybackStore.getState().previewVolumeDb}
          onCommit={(db) => {
            void usePlaybackStore.getState().setVolumeDb(db);
          }}
          onToggleMute={() => {
            void usePlaybackStore.getState().setMuted(!(status?.muted ?? false));
          }}
          onUnmute={() => {
            void usePlaybackStore.getState().setMuted(false);
          }}
        />
      </div>
      <AnimatePresence>
        {queueOpen ? (
          <QueuePanel
            onClose={() => {
              setQueueOpen(false);
            }}
          />
        ) : null}
      </AnimatePresence>
    </div>
  );
}
