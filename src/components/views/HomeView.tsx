import { motion } from "framer-motion";
import { useState, type JSX } from "react";
import { ArrowRight, FolderPlus, HardDrive, Library, Play, Shuffle } from "lucide-react";

import { AlbumCard } from "@/components/library/AlbumCard";
import { Button } from "@/components/ui/Button";
import { formatCount, formatLibraryDuration, formatSampleRate } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { AlbumRow } from "@/types/ipc";

const GUTTER = "mx-auto w-full max-w-[1420px] px-8";

/**
 * Editorial home.
 *
 * Everything on this screen is derived from the real catalogue: no sample data,
 * no invented counts. The hero adapts to the library it finds — a fresh install
 * is invited to add a folder, a scanned library gets an entry point plus the
 * most recent additions, and an empty-but-configured library says so.
 */
export function HomeView(): JSX.Element {
  const stats = useLibraryStore((state) => state.stats);
  const roots = useLibraryStore((state) => state.roots);
  const recent = useLibraryStore((state) => state.recent);
  const error = useLibraryStore((state) => state.error);
  const setRoute = useUiStore((state) => state.setRoute);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const queue = usePlaybackStore((state) => state.queue);
  const currentIndex = usePlaybackStore((state) => state.index);
  const [busyAlbum, setBusyAlbum] = useState<string | null>(null);

  const empty = stats !== null && stats.trackCount === 0;

  /**
   * Queue an entire album.
   *
   * The request is capped at the backend's page limit and ordered by disc and
   * track number, so a 40-track box set plays in the right sequence.
   */
  const playAlbum = async (album: AlbumRow, shuffle = false): Promise<void> => {
    setBusyAlbum(album.id);
    try {
      const page = await libraryApi.search({
        albumId: album.id,
        sort: "album",
        limit: 500,
      });
      if (page.items.length === 0) {
        return;
      }
      const ordered = [...page.items].sort((a, b) => {
        const disc = (a.discNo ?? 1) - (b.discNo ?? 1);
        if (disc !== 0) {
          return disc;
        }
        return (a.trackNo ?? 0) - (b.trackNo ?? 0);
      });
      const finalOrder = shuffle ? shuffleDeterministic(ordered) : ordered;
      await playTracks(finalOrder, 0);
    } finally {
      setBusyAlbum(null);
    }
  };

  if (stats === null) {
    return (
      <div className={cn(GUTTER, "pt-10")}>
        <div className="h-[220px] animate-pulse rounded-3xl bg-white/[0.03]" />
      </div>
    );
  }

  return (
    <div className={cn(GUTTER, "space-y-12 pt-8 pb-12")}>
      {/* ------------------------------------------------------------ Hero -- */}
      <motion.section
        initial={{ opacity: 0, y: 14 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
        className="relative overflow-hidden rounded-[28px] border border-white/[0.07] bg-white/[0.025] p-9 backdrop-blur-2xl"
      >
        <div
          aria-hidden="true"
          className="pointer-events-none absolute -top-32 -right-24 h-72 w-72 rounded-full opacity-40 blur-[110px]"
          style={{ background: "var(--prism-accent)" }}
        />

        <p className="text-[10.5px] font-semibold tracking-[0.24em] text-white/30 uppercase">
          {empty ? "Your library is empty" : "Reference listening"}
        </p>

        <h1 className="mt-3 max-w-3xl text-[40px] leading-[1.05] font-semibold tracking-[-0.035em] text-balance text-white">
          {empty ? "Point Prism at your music." : "Every bit, exactly as it was mastered."}
        </h1>

        <p className="mt-4 max-w-2xl text-[14px] leading-relaxed text-white/45">
          {empty
            ? "Add a folder and Prism will index it — tags, technical details, embedded artwork and CUE sheets — then hand it to the engine untouched."
            : `${formatCount(stats.trackCount)} tracks across ${formatCount(stats.albumCount)} albums, ${formatLibraryDuration(stats.totalDurationMs)} of listening. ${
                stats.maxBitDepth !== null && stats.maxBitDepth > 16
                  ? `Your highest-resolution file is ${String(stats.maxBitDepth)}-bit / ${formatSampleRate(stats.maxSampleRate)}.`
                  : "Everything here is CD resolution or better."
              }`}
        </p>

        <div className="mt-7 flex flex-wrap items-center gap-2.5">
          {empty ? (
            <Button
              variant="primary"
              icon={FolderPlus}
              onClick={() => {
                setRoute("settings");
              }}
            >
              Add a music folder
            </Button>
          ) : (
            <>
              <Button
                variant="primary"
                icon={Play}
                onClick={() => {
                  if (recent[0] !== undefined) {
                    void playAlbum(recent[0]);
                  }
                }}
                disabled={recent.length === 0}
              >
                Play latest addition
              </Button>
              <Button
                variant="secondary"
                icon={Shuffle}
                onClick={() => {
                  if (recent[0] !== undefined) {
                    void playAlbum(recent[0], true);
                  }
                }}
                disabled={recent.length === 0}
              >
                Shuffle it
              </Button>
            </>
          )}
          <Button
            variant="ghost"
            icon={ArrowRight}
            onClick={() => {
              setRoute("now-playing");
            }}
            disabled={queue.length === 0}
          >
            Now playing
          </Button>
        </div>

        {error !== null ? (
          <p role="alert" className="mt-5 text-[12px] text-critical">
            {error.message}
          </p>
        ) : null}
      </motion.section>

      {/* --------------------------------------------------------- Counters -- */}
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat
          icon={Library}
          label="Tracks"
          value={formatCount(stats.trackCount)}
          detail={`${formatCount(stats.cueTrackCount)} from CUE sheets`}
        />
        <Stat
          icon={HardDrive}
          label="Library size"
          value={formatLibraryDuration(stats.totalDurationMs)}
          detail={`${String(roots.length)} folder${roots.length === 1 ? "" : "s"} indexed`}
        />
        <Stat
          icon={Library}
          label="Albums"
          value={formatCount(stats.albumCount)}
          detail={`${formatCount(stats.artistCount)} artists`}
        />
        <Stat
          icon={HardDrive}
          label="Resolution"
          value={stats.maxSampleRate === null ? "—" : formatSampleRate(stats.maxSampleRate)}
          detail={
            stats.maxBitDepth === null ? "No files yet" : `${String(stats.maxBitDepth)}-bit ceiling`
          }
        />
      </section>

      {/* ----------------------------------------------------------- Recent -- */}
      <section>
        <div className="mb-4 flex items-end justify-between">
          <div>
            <h2 className="text-[19px] font-semibold tracking-[-0.02em] text-white">
              Recently added
            </h2>
            <p className="mt-0.5 text-[12px] text-white/35">
              The last albums the scanner committed to the catalogue.
            </p>
          </div>
          <span className="tabular text-[11px] text-white/25">
            {currentIndex + 1 > queue.length || queue.length === 0
              ? ""
              : `${String(queue.length)} in queue`}
          </span>
        </div>

        {recent.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-white/[0.09] px-6 py-10 text-center">
            <p className="text-[13px] text-white/45">
              {empty
                ? "No albums yet — add a folder in Settings and run a scan."
                : "The catalogue has no albums. Run a scan to populate it."}
            </p>
          </div>
        ) : (
          <div className="no-scrollbar -mx-1 flex gap-5 overflow-x-auto px-1 pt-1 pb-3">
            {recent.map((album) => (
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
      </section>
    </div>
  );
}

/** One editorial counter tile. */
function Stat({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Library;
  label: string;
  value: string;
  detail: string;
}): JSX.Element {
  return (
    <div className="rounded-2xl border border-white/[0.06] bg-white/[0.02] p-4">
      <div className="flex items-center gap-2 text-white/30">
        <Icon className="h-3.5 w-3.5" aria-hidden="true" />
        <span className="text-[10px] font-semibold tracking-[0.16em] uppercase">{label}</span>
      </div>
      <p className="mt-2.5 text-[24px] leading-none font-semibold tracking-[-0.03em] text-white">
        {value}
      </p>
      <p className="mt-1.5 truncate text-[10.5px] text-white/30">{detail}</p>
    </div>
  );
}

/**
 * Deterministic shuffle.
 *
 * `Math.random` in a render path is a bug waiting to happen (and eslint rightly
 * flags it); a seeded Fisher–Yates over the track list gives a shuffle that is
 * reproducible for the same album, which also makes the behaviour testable.
 */
function shuffleDeterministic<T>(items: T[]): T[] {
  const copy = [...items];
  let seed = copy.length * 2_654_435_761;
  for (let index = copy.length - 1; index > 0; index -= 1) {
    seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
    const swap = seed % (index + 1);
    const a = copy[index];
    const b = copy[swap];
    if (a !== undefined && b !== undefined) {
      copy[index] = b;
      copy[swap] = a;
    }
  }
  return copy;
}

/** Re-export for the router. */
export const HOME_TITLE = "Home";
