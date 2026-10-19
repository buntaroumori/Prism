import type { JSX } from "react";
import { FolderOpen, LibraryBig, RefreshCw, Waves } from "lucide-react";
import { motion, useReducedMotion } from "framer-motion";

import { SectionHeading } from "@/components/SectionHeading";
import { Button } from "@/components/ui/Button";
import { StatTile } from "@/components/ui/InfoCard";
import { StatusPill } from "@/components/ui/StatusPill";
import { useAsyncAction } from "@/hooks/useAsyncAction";
import { formatBytes, formatCount, formatLibraryDuration, formatSampleRate } from "@/lib/format";
import { systemApi } from "@/lib/ipc";
import type { SystemSnapshot } from "@/types/ipc";

interface OverviewSectionProps {
  snapshot: SystemSnapshot;
  refreshing: boolean;
  onRefresh: () => void;
}

/**
 * Editorial overview: identity, live engine state and library aggregates.
 *
 * Every number is read from SQLite through the validated IPC layer; nothing on
 * this screen is static copy pretending to be data.
 */
export function OverviewSection({
  snapshot,
  refreshing,
  onRefresh,
}: OverviewSectionProps): JSX.Element {
  const { appInfo, health, library } = snapshot;
  const prefersReduced = useReducedMotion();
  const folderAction = useAsyncAction();

  const maxDepth = library.maxBitDepth === null ? "—" : `${String(library.maxBitDepth)}-bit`;
  const maxRate = formatSampleRate(library.maxSampleRate);

  return (
    <div className="space-y-14">
      <motion.section
        id="overview"
        aria-labelledby="overview-title"
        className="scroll-mt-24"
        initial={prefersReduced === true ? false : { opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
      >
        <div className="flex flex-wrap items-end justify-between gap-6">
          <div className="max-w-2xl">
            <p className="text-[10px] font-medium tracking-[0.2em] text-accent-soft/70 uppercase">
              Foundation release · v{appInfo.version}
            </p>
            <h1
              id="overview-title"
              className="text-spectrum mt-2 text-[clamp(2.4rem,5vw,3.75rem)] leading-[1.02] font-semibold tracking-[-0.045em]"
            >
              Audio, refracted.
            </h1>
            <p className="mt-4 max-w-xl text-[14px] leading-relaxed text-white/45">
              A bit-perfect desktop engine in Rust, wrapped in a glass interface built for people
              who read liner notes. This build exposes the storage engine, the IPC contract and the
              runtime health of the core — the foundation every later module is verified against.
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button
              icon={FolderOpen}
              loading={folderAction.isRunning}
              onClick={() => {
                folderAction.run(() => systemApi.openDataFolder());
              }}
            >
              Data folder
            </Button>
            <Button variant="primary" icon={RefreshCw} loading={refreshing} onClick={onRefresh}>
              Refresh
            </Button>
          </div>
        </div>

        <div className="mt-7 flex flex-wrap items-center gap-2">
          <StatusPill tone={health.status === "ok" ? "positive" : "warning"} pulse>
            {health.status === "ok" ? "Engine online" : "Degraded"}
          </StatusPill>
          <StatusPill tone="neutral">SQLite · WAL</StatusPill>
          <StatusPill tone="neutral">Tauri {appInfo.tauriVersion}</StatusPill>
          <StatusPill tone="neutral">Rust 2024 edition</StatusPill>
          <StatusPill tone="neutral">
            {appInfo.targetOs} · {appInfo.targetArch}
          </StatusPill>
        </div>

        {folderAction.error !== null ? (
          <p role="alert" className="mt-4 text-[12px] text-critical">
            {folderAction.error.message}
          </p>
        ) : null}
      </motion.section>

      <section id="library" aria-labelledby="library-heading" className="scroll-mt-24">
        <SectionHeading
          eyebrow="Catalogue"
          title="Your library"
          description="Aggregates computed by the embedded SQLite catalogue. Counters include CUE-sheet virtual tracks."
        />
        <h2 id="library-heading" className="sr-only">
          Library statistics
        </h2>

        <motion.div
          className="mt-6 grid grid-cols-2 gap-3 lg:grid-cols-4"
          initial={prefersReduced === true ? false : { opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.45, delay: 0.08, ease: [0.22, 1, 0.36, 1] }}
        >
          <StatTile
            label="Tracks"
            value={formatCount(library.trackCount)}
            hint={formatLibraryDuration(library.totalDurationMs)}
          />
          <StatTile
            label="Albums"
            value={formatCount(library.albumCount)}
            hint="Distinct releases"
          />
          <StatTile
            label="Artists"
            value={formatCount(library.artistCount)}
            hint="Primary artists"
          />
          <StatTile
            label="Library size"
            value={formatBytes(library.totalBytes)}
            hint={`${formatCount(library.cueTrackCount)} CUE tracks`}
          />
        </motion.div>

        <div className="glass-subtle mt-3 flex flex-wrap items-center justify-between gap-4 rounded-2xl px-5 py-4">
          <div className="flex items-center gap-3">
            <span className="grid h-9 w-9 place-items-center rounded-xl border border-white/10 bg-white/[0.04] text-accent-soft">
              <Waves className="h-[18px] w-[18px]" aria-hidden="true" />
            </span>
            <div>
              <p className="text-[10px] font-medium tracking-[0.14em] text-white/35 uppercase">
                Highest resolution indexed
              </p>
              <p className="tabular mt-0.5 text-[13px] text-white/75">
                {maxDepth} · {maxRate}
              </p>
            </div>
          </div>
          <p className="text-[11px] text-white/30">
            {library.missingCount > 0
              ? `${formatCount(library.missingCount)} files currently unreachable`
              : "All indexed files reachable"}
            {library.unhealthyCount > 0 ? ` · ${formatCount(library.unhealthyCount)} flagged` : ""}
          </p>
        </div>

        {library.trackCount === 0 ? (
          <div className="mt-6 flex items-start gap-4 rounded-2xl border border-white/[0.07] bg-white/[0.02] px-5 py-5">
            <LibraryBig
              className="mt-0.5 h-5 w-5 shrink-0 text-accent-soft/80"
              aria-hidden="true"
            />
            <div>
              <p className="text-[13px] font-medium text-white/80">No music indexed yet</p>
              <p className="mt-1 max-w-2xl text-[12px] leading-relaxed text-white/40">
                The catalogue is initialised and empty. Path identity, FTS5 search structures,
                CUE-aware track constraints and integrity flags are already in place, ready to
                receive the native scanner.
              </p>
            </div>
          </div>
        ) : null}
      </section>
    </div>
  );
}
