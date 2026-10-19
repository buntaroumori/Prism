import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useState, type JSX } from "react";
import { AudioLines, Maximize2 } from "lucide-react";

import { CoverArt } from "@/components/ui/CoverArt";
import { OutputSelector } from "@/components/player/OutputSelector";
import { QualityBadge } from "@/components/player/QualityBadge";
import { Scrubber } from "@/components/player/Scrubber";
import { TransportControls } from "@/components/player/TransportControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { formatAudioFormat } from "@/lib/audio";
import { libraryApi } from "@/lib/ipc";
import {
  durationMs as resolveDuration,
  EMPTY_STATUS,
  playheadMs,
  usePlaybackStore,
} from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";

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
  const toggle = usePlaybackStore((state) => state.toggle);
  const next = usePlaybackStore((state) => state.next);
  const previous = usePlaybackStore((state) => state.previous);
  const stop = usePlaybackStore((state) => state.stop);
  const seekMs = usePlaybackStore((state) => state.seekMs);
  const setVolumeDb = usePlaybackStore((state) => state.setVolumeDb);
  const previewVolumeDb = usePlaybackStore((state) => state.previewVolumeDb);
  const setMuted = usePlaybackStore((state) => state.setMuted);
  const setRoute = useUiStore((state) => state.setRoute);

  const [coverPath, setCoverPath] = useState<string | null>(null);
  const trackId = status?.track?.trackId ?? null;

  // Cover lookup is keyed by track id, not by the whole snapshot: a volume
  // change must not re-query the artwork service.
  useEffect(() => {
    if (trackId === null) {
      setCoverPath(null);
      return;
    }
    let active = true;
    void libraryApi
      .trackCover(trackId)
      .then((asset) => {
        if (active) {
          setCoverPath(asset.localPath);
        }
      })
      .catch(() => {
        if (active) {
          setCoverPath(null);
        }
      });
    return () => {
      active = false;
    };
  }, [trackId]);

  const position = playheadMs(status, tick);
  const duration = resolveDuration(status, tick);
  const track = status?.track ?? null;
  const playing = status?.state === "playing" || status?.state === "buffering";
  const visible = track !== null;

  // Display strings are computed outside the AnimatePresence branch: TypeScript
  // narrows `track` from the `visible` alias inside it, and keeping the reads
  // here means the JSX stays free of repeated null handling.
  const coverAlt = track !== null && track.album.length > 0 ? track.album : "Album art";
  const title = track?.title ?? "";
  const artist = track !== null && track.artist.length > 0 ? track.artist : "Unknown artist";
  const canSkip = queue.length > 0 && index + 1 < queue.length;
  const volumeDb = pendingVolumeDb ?? status?.volumeDb ?? 0;

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
            <div className="flex min-w-0 items-center gap-3.5">
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
                <p className="truncate text-[13px] font-semibold tracking-tight text-white">
                  {title}
                </p>
                <p className="truncate text-[11.5px] text-white/45">{artist}</p>
                <div className="mt-1 flex items-center gap-1.5">
                  {status !== null ? <QualityBadge status={status} /> : null}
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
            <div className="flex min-w-0 items-center justify-end gap-4">
              <div className="hidden min-w-0 flex-col items-end xl:flex">
                <span className="flex items-center gap-1.5 text-[10px] tracking-[0.1em] text-white/30 uppercase">
                  <AudioLines className="h-3 w-3" aria-hidden="true" />
                  {formatAudioFormat(status?.outputFormat ?? null)}
                </span>
                <span className="tabular text-[10px] text-white/20">
                  {status?.deviceName ?? "—"}
                </span>
              </div>

              <OutputSelector status={status} />

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
              />
            </div>
          </div>
        </motion.footer>
      ) : null}
    </AnimatePresence>
  );
}
