import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useState, type JSX } from "react";
import { AudioWaveform, Clock3, Gauge, Radio } from "lucide-react";

import { CoverArt } from "@/components/ui/CoverArt";
import { QualityBadge } from "@/components/player/QualityBadge";
import { formatAudioFormat } from "@/lib/audio";
import { formatLatency, formatPosition } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import {
  durationMs as resolveDuration,
  EMPTY_STATUS,
  playheadMs,
  usePlaybackStore,
} from "@/stores/playbackStore";
import { useUiStore } from "@/stores/uiStore";
import type { CoverAsset } from "@/types/ipc";

const FALLBACK = "—";

/**
 * Full-window now-playing surface.
 *
 * The artwork is the loudest element in the product, so it gets the room: a
 * single square, crossfaded between tracks by `AnimatePresence` with the album
 * key as identity. The technical column is deliberately unglamorous — it is the
 * panel a listener checks to confirm what the DAC is really receiving.
 */
export function NowPlayingView(): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const tick = usePlaybackStore((state) => state.tick);
  const queue = usePlaybackStore((state) => state.queue);
  const index = usePlaybackStore((state) => state.index);
  const setRoute = useUiStore((state) => state.setRoute);
  const [asset, setAsset] = useState<CoverAsset | null>(null);

  const track = status?.track ?? null;
  const trackId = track?.trackId ?? null;

  useEffect(() => {
    if (trackId === null) {
      setAsset(null);
      return;
    }
    let active = true;
    void libraryApi
      .trackCover(trackId)
      .then((next) => {
        if (active) {
          setAsset(next);
        }
      })
      .catch(() => {
        if (active) {
          setAsset(null);
        }
      });
    return () => {
      active = false;
    };
  }, [trackId]);

  if (track === null) {
    return (
      <div className="mx-auto flex w-full max-w-[1420px] flex-col items-center justify-center px-8 py-28 text-center">
        <div className="grid h-20 w-20 place-items-center rounded-full border border-white/[0.08] bg-white/[0.03]">
          <AudioWaveform className="h-7 w-7 text-white/25" aria-hidden="true" strokeWidth={1.4} />
        </div>
        <h2 className="mt-6 text-[20px] font-semibold tracking-[-0.02em] text-white">
          Nothing is playing
        </h2>
        <p className="mt-2 max-w-sm text-[13px] leading-relaxed text-white/40">
          Pick an album from Home, or add a folder in Settings and run a scan to build your library.
        </p>
        <button
          type="button"
          onClick={() => {
            setRoute("home");
          }}
          className="accent-soft-text mt-5 text-[12.5px] underline-offset-4 hover:underline"
        >
          Back to Home
        </button>
      </div>
    );
  }

  const position = playheadMs(status, tick);
  const duration = resolveDuration(status, tick);
  const rate = status?.sourceFormat?.sampleRate ?? null;
  const latency = status === null ? null : formatLatency(toMs(status.latencyFrames, rate) ?? 0);
  const queuePosition =
    queue.length > 0 ? `${String(index + 1)} / ${String(queue.length)}` : FALLBACK;

  return (
    <div className="mx-auto w-full max-w-[1420px] px-8 pt-10 pb-12">
      <div className="grid grid-cols-1 items-center gap-12 lg:grid-cols-[minmax(0,1fr)_minmax(0,420px)]">
        {/* ------------------------------------------------------- Artwork -- */}
        <div className="relative mx-auto w-full max-w-[560px]">
          <div
            aria-hidden="true"
            className="absolute inset-6 rounded-full opacity-50 blur-[90px]"
            style={{ background: "var(--prism-accent)" }}
          />
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.div
              key={asset?.key ?? "no-cover"}
              initial={{ opacity: 0, scale: 0.985, filter: "blur(6px)" }}
              animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
              exit={{ opacity: 0, scale: 1.015, filter: "blur(8px)" }}
              transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
              className="relative"
            >
              <CoverArt
                path={asset?.localPath ?? null}
                alt={track.album.length > 0 ? track.album : track.title}
                className="aspect-square w-full rounded-[26px] shadow-[0_50px_120px_-60px_rgb(0_0_0/0.95)]"
                rounded="rounded-[26px]"
                fallbackClassName="h-16 w-16"
              />
            </motion.div>
          </AnimatePresence>
        </div>

        {/* ---------------------------------------------------- Information -- */}
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <QualityBadge status={status ?? EMPTY_STATUS} />
            {status?.dop === true ? (
              <span className="rounded-full border border-spectrum-violet/30 bg-spectrum-violet/[0.10] px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] text-spectrum-violet uppercase">
                DSD over PCM
              </span>
            ) : null}
            {status?.state === "buffering" ? (
              <span className="rounded-full border border-white/15 bg-white/[0.06] px-2.5 py-1 text-[10px] font-semibold tracking-[0.14em] text-white/60 uppercase">
                Buffering
              </span>
            ) : null}
          </div>

          <h1 className="mt-5 text-[34px] leading-[1.08] font-semibold tracking-[-0.035em] text-balance text-white">
            {track.title}
          </h1>
          <p className="mt-2 text-[15px] text-white/55">
            {track.artist.length > 0 ? track.artist : "Unknown artist"}
          </p>
          {track.album.length > 0 ? (
            <p className="mt-0.5 text-[13px] text-white/30">{track.album}</p>
          ) : null}

          <div className="mt-6 flex items-baseline gap-2">
            <span className="tabular text-[13px] text-white/70">{formatPosition(position)}</span>
            <span className="text-[13px] text-white/20">/</span>
            <span className="tabular text-[13px] text-white/35">
              {duration === null ? FALLBACK : formatPosition(duration)}
            </span>
            <span className="ml-auto text-[11px] text-white/25">{queuePosition}</span>
          </div>

          <dl className="mt-8 grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-white/[0.07] bg-white/[0.03] sm:grid-cols-2">
            <Fact
              icon={Radio}
              label="Output device"
              value={status?.deviceName ?? FALLBACK}
              detail={status?.backend ?? FALLBACK}
            />
            <Fact
              icon={AudioWaveform}
              label="Device format"
              value={formatAudioFormat(status?.outputFormat ?? null)}
              detail="Negotiated by the supervisor"
            />
            <Fact
              icon={Gauge}
              label="Source format"
              value={formatAudioFormat(status?.sourceFormat ?? null)}
              detail={status?.dop === true ? "DSD over PCM carrier" : "Decoded natively"}
            />
            <Fact
              icon={Clock3}
              label="Pipeline"
              value={latency ?? FALLBACK}
              detail={`${String(status?.underruns ?? 0)} underruns · ${String(status?.overruns ?? 0)} overruns`}
            />
          </dl>

          <p className="mt-5 text-[11px] leading-relaxed text-white/25">
            {status?.bitPerfect === true
              ? "The signal reaches the DAC without a single arithmetic stage: exclusive endpoint, exact format, unity gain, no DSP."
              : "The engine is applying processing (volume, DSP, resampling or a shared mixer). The badge above tells you exactly which."}
          </p>
        </div>
      </div>
    </div>
  );
}

/** One row of the technical read-out. */
function Fact({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Radio;
  label: string;
  value: string;
  detail: string;
}): JSX.Element {
  return (
    <div className="bg-oled-950/40 px-4 py-3.5">
      <dt className="flex items-center gap-1.5 text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase">
        <Icon className="h-3 w-3" aria-hidden="true" />
        {label}
      </dt>
      <dd className={cn("mt-1.5 truncate text-[12.5px] text-white/80")} title={value}>
        {value}
      </dd>
      <dd className="truncate text-[10.5px] text-white/25">{detail}</dd>
    </div>
  );
}

function toMs(frames: number, rate: number | null): number | null {
  if (rate === null || rate <= 0) {
    return null;
  }
  return (frames / rate) * 1000;
}
