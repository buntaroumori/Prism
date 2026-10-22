import { motion } from "framer-motion";
import { useEffect, useMemo, useState, type JSX } from "react";
import { ArrowRight, FolderPlus, Disc3, Clock3, Flame, Sparkles, Star, Waves } from "lucide-react";

import { AlbumCard } from "@/components/library/AlbumCard";
import { Rail } from "@/components/library/Rail";
import { Button } from "@/components/ui/Button";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { AlbumCollection, AlbumRow, HomeRails } from "@/types/ipc";

const GUTTER = "mx-auto w-full max-w-[1560px] px-8";

/** Rail definitions in display order; a rail with no albums is not rendered. */
const RAILS: readonly {
  key: AlbumCollection;
  title: string;
  caption: string;
  icon: typeof Disc3;
}[] = [
  {
    key: "recentlyPlayed",
    title: "Recently played",
    caption: "Where you left off — most recent first.",
    icon: Clock3,
  },
  {
    key: "recentlyAdded",
    title: "Recently added",
    caption: "The latest records added to your library.",
    icon: Sparkles,
  },
  { key: "mostPlayed", title: "On repeat", caption: "The records you return to.", icon: Flame },
  {
    key: "highResolution",
    title: "High resolution",
    caption: "Above CD quality: 24-bit, DSD or rates beyond 48 kHz.",
    icon: Waves,
  },
  {
    key: "recentReleases",
    title: "Fresh releases",
    caption: "Records from the newest release year in your collection.",
    icon: Star,
  },
  {
    key: "rediscover",
    title: "Rediscover",
    caption: "Albums you own and have not played in a while.",
    icon: Disc3,
  },
];

/**
 * Home.
 *
 * No hero: this is a listening surface, not a landing page. Six shelves, each
 * built by a single backend call from the real catalogue, each scrollable by
 * drag and by the arrows in its corner. Empty shelves are omitted rather than
 * drawn as placeholders.
 */
export function HomeView(): JSX.Element {
  const stats = useLibraryStore((state) => state.stats);
  const roots = useLibraryStore((state) => state.roots);
  const setRoute = useUiStore((state) => state.setRoute);
  const openCollection = useUiStore((state) => state.openCollection);
  const showStats = useUiStore((state) => state.showHomeStats);
  const playTracks = usePlaybackStore((state) => state.playTracks);
  const queueLength = usePlaybackStore((state) => state.queue.length);
  const trackId = usePlaybackStore((state) => state.status?.track?.trackId ?? null);

  const [rails, setRails] = useState<HomeRails | null>(null);
  const [busyAlbum, setBusyAlbum] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void libraryApi
      .home(24)
      .then((next) => {
        if (active) {
          setRails(next);
        }
      })
      .catch((cause: unknown) => {
        if (active) {
          setFailed(cause instanceof Error ? cause.message : "Cannot read the catalogue");
        }
      });
    return () => {
      active = false;
    };
    // A new track changes "recently played" and "on repeat": refetch once per
    // start, not on every playhead tick (that lives in a separate store slice).
  }, [stats?.trackCount, trackId]);

  const playAlbum = async (album: AlbumRow): Promise<void> => {
    setBusyAlbum(album.id);
    try {
      const page = await libraryApi.search({ albumId: album.id, sort: "album", limit: 500 });
      if (page.items.length === 0) {
        return;
      }
      const ordered = [...page.items].sort((a, b) => {
        const disc = (a.discNo ?? 1) - (b.discNo ?? 1);
        return disc !== 0 ? disc : (a.trackNo ?? 0) - (b.trackNo ?? 0);
      });
      await playTracks(ordered, 0);
    } finally {
      setBusyAlbum(null);
    }
  };

  const empty = stats !== null && stats.trackCount === 0;
  const visible = useMemo(
    () => RAILS.filter((rail) => (rails?.[rail.key].length ?? 0) > 0),
    [rails],
  );

  if (stats === null) {
    return (
      <div className={cn(GUTTER, "space-y-6 pt-8")}>
        {[0, 1, 2].map((key) => (
          <div key={key} className="h-[240px] animate-pulse rounded-3xl bg-white/[0.03]" />
        ))}
      </div>
    );
  }

  return (
    <div className={cn(GUTTER, "space-y-9 pt-7 pb-12")}>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-[26px] leading-none font-semibold tracking-[-0.03em] text-white">
            Home
          </h1>
          <p className="mt-1.5 text-[12.5px] text-white/40">
            {empty
              ? "Add a music folder and Prism will organise your albums, artwork and songs."
              : `${formatCount(stats.trackCount)} tracks · ${formatCount(stats.albumCount)} albums · ${formatCount(stats.artistCount)} artists · ${formatLibraryDuration(stats.totalDurationMs)}`}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {queueLength > 0 ? (
            <Button
              variant="ghost"
              icon={ArrowRight}
              onClick={() => {
                setRoute("now-playing");
              }}
            >
              Now playing
            </Button>
          ) : null}
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
          ) : null}
        </div>
      </header>

      {failed !== null ? (
        <p role="alert" className="text-[12px] text-critical">
          {failed}
        </p>
      ) : null}

      {empty ? (
        <div className="rounded-2xl border border-dashed border-white/[0.09] px-6 py-14 text-center">
          <Disc3 className="mx-auto h-6 w-6 text-white/20" aria-hidden="true" strokeWidth={1.5} />
          <p className="mt-3 text-[13px] font-medium text-white/55">Nothing indexed yet</p>
          <p className="mx-auto mt-1 max-w-md text-[11.5px] leading-relaxed text-white/30">
            {roots.length === 0
              ? "Add a music folder in Settings, then run a scan."
              : "Your folders are ready. Run a scan from Settings to fill your library."}
          </p>
        </div>
      ) : null}

      {visible.map((rail, index) => (
        <motion.div
          key={rail.key}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{
            duration: 0.32,
            delay: Math.min(index * 0.03, 0.15),
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <Rail
            title={rail.title}
            caption={rail.caption}
            action={
              <button
                type="button"
                onClick={() => {
                  openCollection(rail.key);
                }}
                className="hidden items-center gap-1 text-[11px] text-white/30 transition-colors hover:text-white/70 sm:flex"
              >
                See all
                <ArrowRight className="h-3 w-3" aria-hidden="true" />
              </button>
            }
          >
            {rails?.[rail.key].map((album) => (
              <AlbumCard
                key={`${rail.key}-${album.id}`}
                album={album}
                busy={busyAlbum === album.id}
                onPlay={(target) => {
                  void playAlbum(target);
                }}
              />
            ))}
          </Rail>
        </motion.div>
      ))}

      {rails?.genres.map((genre, index) => (
        <motion.div
          key={`genre-${genre.name}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{
            duration: 0.32,
            delay: Math.min((visible.length + index) * 0.03, 0.15),
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <Rail
            title={genre.name}
            caption={`${formatCount(genre.albumCount)} albums in your library.`}
            action={
              <button
                type="button"
                onClick={() => {
                  openCollection(null, genre.name);
                }}
                className="hidden items-center gap-1 text-[11px] text-white/30 transition-colors hover:text-white/70 sm:flex"
              >
                See all
                <ArrowRight className="h-3 w-3" aria-hidden="true" />
              </button>
            }
          >
            {genre.albums.map((album) => (
              <AlbumCard
                key={`genre-${genre.name}-${album.id}`}
                album={album}
                busy={busyAlbum === album.id}
                onPlay={(target) => {
                  void playAlbum(target);
                }}
              />
            ))}
          </Rail>
        </motion.div>
      ))}

      {showStats && !empty ? (
        <section className="grid grid-cols-2 gap-3 border-t border-white/[0.06] pt-6 lg:grid-cols-4">
          <Fact label="Tracks" value={formatCount(stats.trackCount)} />
          <Fact label="Albums" value={formatCount(stats.albumCount)} />
          <Fact label="Listening time" value={formatLibraryDuration(stats.totalDurationMs)} />
          <Fact
            label="Resolution ceiling"
            value={
              stats.maxBitDepth === null
                ? "—"
                : `${String(stats.maxBitDepth)}-bit · ${formatCount(stats.maxSampleRate ?? 0)} Hz`
            }
          />
        </section>
      ) : null}
    </div>
  );
}

function Fact({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <div>
      <p className="text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase">
        {label}
      </p>
      <p className="tabular mt-1 text-[15px] font-medium tracking-tight text-white/80">{value}</p>
    </div>
  );
}
