import { ListMusic } from "lucide-react";
import type { JSX } from "react";

import { Scrubber } from "@/components/player/Scrubber";
import { TransportControls } from "@/components/player/TransportControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { cn } from "@/lib/utils";
import { durationMs, EMPTY_STATUS, playheadMs, usePlaybackStore } from "@/stores/playbackStore";
import { usePlaybackPreferencesStore } from "@/stores/playbackPreferencesStore";
import { useUiStore } from "@/stores/uiStore";

/** Complete embedded transport for the bounded Now Playing surface. */
export function NowPlayingControls({ className }: { className?: string }): JSX.Element {
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
  const queueOpen = useUiStore((state) => state.route === "queue");
  const toggleQueue = useUiStore((state) => state.toggleQueue);

  const entry = queue.find((item) => item.trackId === status?.track?.trackId) ?? queue[index];
  const position = playheadMs(status, tick);
  const duration = durationMs(status, tick, entry?.durationMs ?? null);
  const canSkip =
    queue.length > 0 &&
    (index + 1 < queue.length || (continuation !== null && albumEnd === "continue-library"));
  const volumeDb = pendingVolumeDb ?? committedVolumeDb ?? status?.volumeDb ?? 0;

  return (
    <div
      data-now-playing-controls
      className={cn("shrink-0 border-t border-white/[0.055] pt-3", className)}
    >
      <div className="flex items-center justify-center gap-1.5">
        <VolumeControl
          compact
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
        <TransportControls
          compact
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
        <button
          type="button"
          aria-label={queueOpen ? "Close queue" : "Open queue"}
          aria-expanded={queueOpen}
          title="Queue"
          onClick={toggleQueue}
          className={
            queueOpen
              ? "grid h-8 w-8 shrink-0 place-items-center rounded-full bg-accent/[0.14] text-accent"
              : "grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/40 transition-colors hover:bg-white/[0.07] hover:text-white"
          }
        >
          <ListMusic className="h-[17px] w-[17px]" aria-hidden="true" />
        </button>
      </div>
      <div className="mx-auto mt-2 w-full max-w-[620px]">
        <Scrubber
          status={status ?? EMPTY_STATUS}
          positionMs={position}
          durationMs={duration}
          onSeek={(ms) => {
            void usePlaybackStore.getState().seekMs(ms);
          }}
        />
      </div>
    </div>
  );
}
