import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useState, type JSX } from "react";
import { ListMusic, Maximize2, Mic2 } from "lucide-react";

import { CoverArt } from "@/components/ui/CoverArt";
import { QualityBadge } from "@/components/player/QualityBadge";
import { LyricsSheet } from "@/components/player/LyricsSheet";
import { QueuePanel } from "@/components/player/QueuePanel";
import { Scrubber } from "@/components/player/Scrubber";
import { TransportControls } from "@/components/player/TransportControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { trackCover } from "@/lib/trackCover";
import {
  durationMs as resolveDuration,
  EMPTY_STATUS,
  playheadMs,
  usePlaybackStore,
} from "@/stores/playbackStore";
import { usePlaybackPreferencesStore } from "@/stores/playbackPreferencesStore";
import { useUiStore } from "@/stores/uiStore";
import { useContextMenuStore } from "@/stores/contextMenuStore";

/**
 * Persistent transport.
 *
 * Reads four independent slices (status, tick, queue, volume) so the 8 Hz tick
 * subscription re-renders the scrubber subtree only — the cover, the quality
 * badge and the device selector never see a playhead update. The bar slides in
 * once, when the first track is queued, and never animates again.
 */
export function PlayerBar(): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const tick = usePlaybackStore((state) => state.tick);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const busy = usePlaybackStore((state) => state.busy);
  const pendingVolumeDb = usePlaybackStore((state) => state.pendingVolumeDb);
  const committedVolumeDb = usePlaybackStore((state) => state.committedVolumeDb);
  const toggle = usePlaybackStore((state) => state.toggle);
  const shuffled = usePlaybackStore((state) => state.shuffleSnapshot !== null);
  const continuation = usePlaybackStore((state) => state.continuation);
  const albumEnd = usePlaybackPreferencesStore((state) => state.albumEnd);
  const toggleShuffle = usePlaybackStore((state) => state.toggleShuffle);
  const next = usePlaybackStore((state) => state.next);
  const previous = usePlaybackStore((state) => state.previous);
  const stop = usePlaybackStore((state) => state.stop);
  const seekMs = usePlaybackStore((state) => state.seekMs);
  const setVolumeDb = usePlaybackStore((state) => state.setVolumeDb);
  const previewVolumeDb = usePlaybackStore((state) => state.previewVolumeDb);
  const setMuted = usePlaybackStore((state) => state.setMuted);
  const setRoute = useUiStore((state) => state.setRoute);
  const route = useUiStore((state) => state.route);
  const sidebarHidden = useUiStore((state) => state.sidebarHidden);
  const toggleSidebarHidden = useUiStore((state) => state.toggleSidebarHidden);
  const openAlbum = useUiStore((state) => state.openAlbum);
  const openArtist = useUiStore((state) => state.openArtist);
  const openAt = useContextMenuStore((state) => state.openAt);

  const [coverPath, setCoverPath] = useState<string | null>(null);
  const [coverThumbnailPath, setCoverThumbnailPath] = useState<string | null>(null);
  const [queueOpen, setQueueOpen] = useState(false);
  const [lyricsOpen, setLyricsOpen] = useState(false);
  const trackId = status?.track?.trackId ?? null;

  useEffect(() => {
    if (route === "now-playing") {
      setQueueOpen(false);
      setLyricsOpen(false);
    }
  }, [route]);

  // Cover lookup is keyed by track id, not by the whole snapshot: a volume
  // change must not re-query the artwork service.
  useEffect(() => {
    if (trackId === null) {
      setCoverPath(null);
      setCoverThumbnailPath(null);
      return;
    }
    let active = true;
    void trackCover(trackId, status?.track?.artist, status?.track?.album)
      .then((asset) => {
        if (active) {
          setCoverPath(asset.localPath);
          setCoverThumbnailPath(asset.thumbnailPath);
        }
      })
      .catch(() => {
        if (active) {
          setCoverPath(null);
          setCoverThumbnailPath(null);
        }
      });
    return () => {
      active = false;
    };
  }, [trackId, status?.track?.artist, status?.track?.album]);

  const position = playheadMs(status, tick);
  // The catalogue knows the length of the file the engine opened, even when the
  // container did not declare one: a bar without a maximum cannot be dragged.
  const track = status?.track ?? null;
  const playing = status?.state === "playing" || status?.state === "buffering";
  const visible = track !== null;

  // Display strings are computed outside the AnimatePresence branch: TypeScript
  // narrows `track` from the `visible` alias inside it, and keeping the reads
  // here means the JSX stays free of repeated null handling.
  const coverAlt = track !== null && track.album.length > 0 ? track.album : "Album art";
  const title = track?.title ?? "";
  const artist = track !== null && track.artist.length > 0 ? track.artist : "Unknown artist";
  // The names in the bar are links, like every other name in the product: the
  // queue entry carries the catalogue ids the engine does not need. The index is
  // a fallback for the instant between a track change and the store catching up.
  const entry =
    queue.find((item) => item.trackId === status?.track?.trackId) ?? queue[index] ?? null;
  const duration = resolveDuration(status, tick, entry?.durationMs ?? null);
  const albumId = entry?.albumId ?? null;
  const artistId = entry?.artistId ?? null;
  const canSkip =
    queue.length > 0 &&
    (index + 1 < queue.length || (continuation !== null && albumEnd === "continue-library"));
  const volumeDb = pendingVolumeDb ?? committedVolumeDb ?? status?.volumeDb ?? 0;

  return (
    <AnimatePresence>
      {visible ? (
        <motion.footer
          key="player-bar"
          initial={{ y: 96, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: 96, opacity: 0 }}
          transition={{ type: "spring", stiffness: 320, damping: 34, mass: 0.9 }}
          className="glass-bar relative z-30 shrink-0 border-t border-white/[0.07]"
          data-playing={playing}
        >
          {/* Hairline progress line: the whole width, so the state is legible
              from across the room. */}
          <div className="absolute inset-x-0 top-0 h-px overflow-hidden">
            <div
              className="accent-bg h-full transition-[width] duration-150 ease-linear"
              style={{
                width:
                  duration === null || duration <= 0
                    ? "0%"
                    : `${String((position / duration) * 100)}%`,
                opacity: playing ? 0.9 : 0.35,
              }}
            />
          </div>

          <div className="mx-auto grid h-[92px] w-full max-w-[1560px] grid-cols-[minmax(0,1fr)_minmax(320px,540px)_minmax(0,1fr)] items-center gap-6 px-6">
            {/* ------------------------------------------------ Now playing -- */}
            <div
              className="flex min-w-0 items-center gap-3.5"
              onContextMenu={(event) => {
                if (entry !== null) {
                  event.preventDefault();
                  openAt(event.clientX, event.clientY, {
                    kind: "track",
                    track: entry.catalogue,
                  });
                }
              }}
            >
              <button
                type="button"
                onClick={() => {
                  setRoute("now-playing");
                }}
                className="group relative shrink-0 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none"
                aria-label="Open now playing"
              >
                <CoverArt
                  path={coverPath}
                  thumbnailPath={coverThumbnailPath}
                  alt={coverAlt}
                  className="h-[58px] w-[58px] transition-transform duration-300 group-hover:scale-[1.03]"
                  rounded="rounded-xl"
                  fallbackClassName="h-5 w-5"
                />
                <span className="pointer-events-none absolute inset-0 grid place-items-center rounded-xl bg-black/45 opacity-0 transition-opacity group-hover:opacity-100">
                  <Maximize2 className="h-4 w-4 text-white" aria-hidden="true" />
                </span>
              </button>

              <div className="min-w-0">
                <button
                  type="button"
                  disabled={albumId === null}
                  onClick={() => {
                    if (albumId !== null) {
                      openAlbum(albumId);
                    }
                  }}
                  className="block max-w-full truncate text-left text-[13px] font-semibold tracking-tight text-white focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none enabled:hover:underline enabled:hover:decoration-white/30 enabled:hover:underline-offset-2 disabled:cursor-default"
                  aria-label={albumId === null ? undefined : `Open ${coverAlt}`}
                >
                  {title}
                </button>
                <button
                  type="button"
                  disabled={artistId === null}
                  onClick={() => {
                    if (artistId !== null) {
                      openArtist(artistId);
                    }
                  }}
                  data-selectable="artist"
                  className="block max-w-full truncate text-left text-[11.5px] text-white/45 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none enabled:hover:text-white/70 enabled:hover:underline enabled:hover:decoration-white/20 enabled:hover:underline-offset-2 disabled:cursor-default"
                  aria-label={artistId === null ? undefined : `Open ${artist}`}
                >
                  {artist}
                </button>
                <div className="mt-1 flex max-w-full items-center gap-1.5 overflow-hidden">
                  {status !== null ? (
                    <QualityBadge status={status} track={entry?.catalogue ?? null} />
                  ) : null}
                  {status?.dop === true ? (
                    <span className="rounded-full border border-spectrum-violet/30 bg-spectrum-violet/[0.10] px-2 py-0.5 text-[9.5px] font-semibold tracking-[0.12em] text-spectrum-violet uppercase">
                      DoP
                    </span>
                  ) : null}
                </div>
              </div>
            </div>

            {/* -------------------------------------------------- Transport -- */}
            <div className="flex flex-col items-center gap-1">
              <TransportControls
                state={status?.state ?? "stopped"}
                canSkip={canSkip}
                busy={busy}
                shuffled={shuffled}
                sidebarHidden={sidebarHidden}
                onToggleSidebar={toggleSidebarHidden}
                onToggleShuffle={() => {
                  void toggleShuffle();
                }}
                onToggle={() => {
                  void toggle();
                }}
                onNext={() => {
                  void next();
                }}
                onPrevious={() => {
                  void previous();
                }}
                onStop={() => {
                  void stop();
                }}
              />
              <Scrubber
                status={status ?? EMPTY_STATUS}
                positionMs={position}
                durationMs={duration}
                onSeek={(ms) => {
                  void seekMs(ms);
                }}
              />
            </div>

            {/* ----------------------------------------------------- Right -- */}
            <div className="flex min-w-0 items-center justify-end gap-3">
              <button
                type="button"
                aria-label="Open queue"
                aria-expanded={queueOpen}
                title="Queue"
                onClick={() => {
                  setLyricsOpen(false);
                  setQueueOpen((open) => !open);
                }}
                className={
                  queueOpen
                    ? "grid h-9 w-9 place-items-center rounded-full bg-accent/[0.14] text-accent"
                    : "grid h-9 w-9 place-items-center rounded-full text-white/45 transition-colors hover:bg-white/[0.07] hover:text-white"
                }
              >
                <ListMusic className="h-[18px] w-[18px]" aria-hidden="true" />
              </button>

              <button
                type="button"
                aria-label={lyricsOpen ? "Hide synchronized lyrics" : "Show synchronized lyrics"}
                aria-expanded={lyricsOpen}
                title="Synchronized lyrics"
                onClick={() => {
                  setQueueOpen(false);
                  setLyricsOpen((open) => !open);
                }}
                className={
                  lyricsOpen
                    ? "grid h-9 w-9 place-items-center rounded-full bg-accent/[0.14] text-accent"
                    : "grid h-9 w-9 place-items-center rounded-full text-white/45 transition-colors hover:bg-white/[0.07] hover:text-white"
                }
              >
                <Mic2 className="h-[18px] w-[18px]" aria-hidden="true" />
              </button>

              <VolumeControl
                volumeDb={volumeDb}
                muted={status?.muted ?? false}
                onPreview={previewVolumeDb}
                onCommit={(db) => {
                  void setVolumeDb(db);
                }}
                onToggleMute={() => {
                  void setMuted(!(status?.muted ?? false));
                }}
                onUnmute={() => {
                  void setMuted(false);
                }}
              />
            </div>
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
          <AnimatePresence>
            {lyricsOpen ? (
              <LyricsSheet
                trackId={trackId}
                onClose={() => {
                  setLyricsOpen(false);
                }}
              />
            ) : null}
          </AnimatePresence>
        </motion.footer>
      ) : null}
    </AnimatePresence>
  );
}
