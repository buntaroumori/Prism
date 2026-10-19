import type { JSX } from "react";
import { ChevronRight, FolderSearch, Loader2, RefreshCw } from "lucide-react";

import { StatusPill } from "@/components/ui/StatusPill";
import { Button } from "@/components/ui/Button";
import { formatCount } from "@/lib/format";
import { useLibraryStore } from "@/stores/libraryStore";
import { useUiStore, type ShellRoute } from "@/stores/uiStore";

const ROUTE_LABELS: Record<ShellRoute, string> = {
  home: "Home",
  library: "Library",
  "now-playing": "Now Playing",
  settings: "Settings",
  system: "System",
};

/**
 * Window chrome and page context.
 *
 * The right cluster is a live scan indicator, not a decoration: when a scan runs
 * it shows the phase and the file counter ticking, when it finishes it collapses
 * back to a quiet "Scan" button. Everything here is `no-drag` except the bar
 * itself, so the window can be moved from the empty middle.
 */
export function TopBar(): JSX.Element {
  const route = useUiStore((state) => state.route);
  const scan = useLibraryStore((state) => state.scan);
  const progress = useLibraryStore((state) => state.progress);
  const stats = useLibraryStore((state) => state.stats);
  const startScan = useLibraryStore((state) => state.startScan);
  const refreshStats = useLibraryStore((state) => state.refreshStats);

  const running = scan?.running === true || progress !== null;
  const label = ROUTE_LABELS[route];

  return (
    <header
      data-tauri-drag-region
      className="drag-region relative z-20 flex h-[62px] shrink-0 items-center justify-between gap-4 border-b border-white/[0.05] px-7"
    >
      <div className="flex min-w-0 items-center gap-2 text-[12.5px]">
        <span className="text-white/30">Prism</span>
        <ChevronRight className="h-3.5 w-3.5 text-white/15" aria-hidden="true" />
        <span className="font-medium tracking-tight text-white/80">{label}</span>
        {stats !== null ? (
          <span className="ml-2 hidden text-[11px] text-white/25 sm:inline">
            {formatCount(stats.trackCount)} tracks · {formatCount(stats.albumCount)} albums
          </span>
        ) : null}
      </div>

      <div className="no-drag flex items-center gap-2.5">
        {running ? (
          <div className="flex items-center gap-2.5 rounded-full border border-white/10 bg-white/[0.05] py-1 pr-3 pl-2.5">
            <Loader2 className="h-3.5 w-3.5 animate-spin text-white/50" aria-hidden="true" />
            <span className="text-[11px] text-white/60">
              {progress === null
                ? "Scanning"
                : `${capitalise(progress.phase)} · ${formatCount(progress.filesIndexed)} indexed`}
            </span>
            {progress?.percent !== null && progress?.percent !== undefined ? (
              <span className="tabular text-[10.5px] text-white/35">
                {Math.round(progress.percent * 100)}%
              </span>
            ) : null}
          </div>
        ) : (
          <StatusPill
            tone={scan?.lastRun?.status === "failed" ? "warning" : "neutral"}
            pulse={false}
          >
            {scan?.lastRun === null || scan?.lastRun === undefined
              ? "Never scanned"
              : scan.lastRun.status === "failed"
                ? "Last scan failed"
                : "Library indexed"}
          </StatusPill>
        )}

        <Button
          variant="ghost"
          icon={RefreshCw}
          onClick={() => {
            void refreshStats();
          }}
          aria-label="Refresh counters"
          className="h-8 w-8 rounded-full px-0"
        />

        <Button
          variant="secondary"
          icon={FolderSearch}
          loading={running}
          onClick={() => {
            void startScan();
          }}
          disabled={running}
          className="h-8 px-3.5 text-[12px]"
        >
          {running ? "Scanning" : "Scan library"}
        </Button>
      </div>
    </header>
  );
}

function capitalise(value: string): string {
  return value.length === 0 ? value : `${value[0]?.toUpperCase() ?? ""}${value.slice(1)}`;
}
