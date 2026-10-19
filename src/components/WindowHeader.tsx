import type { JSX } from "react";
import { ChevronRight, RefreshCw } from "lucide-react";

import { StatusPill, type StatusTone } from "@/components/ui/StatusPill";
import { Button } from "@/components/ui/Button";
import { formatClock } from "@/lib/format";

export type HeaderStatus = "loading" | "ok" | "degraded" | "offline";

interface WindowHeaderProps {
  status: HeaderStatus;
  capturedAt: Date | null;
  refreshing: boolean;
  onRefresh: () => void;
}

const STATUS_COPY: Record<HeaderStatus, { tone: StatusTone; label: string }> = {
  loading: { tone: "neutral", label: "Connecting" },
  ok: { tone: "positive", label: "Engine online" },
  degraded: { tone: "warning", label: "Degraded" },
  offline: { tone: "critical", label: "Backend offline" },
};

/**
 * Window chrome: breadcrumb, live status, refresh and capture timestamp.
 *
 * Marked as a Tauri drag region so future frameless builds keep a natural
 * drag handle; interactive children opt out with `no-drag`.
 */
export function WindowHeader({
  status,
  capturedAt,
  refreshing,
  onRefresh,
}: WindowHeaderProps): JSX.Element {
  const copy = STATUS_COPY[status];

  return (
    <header
      data-tauri-drag-region
      className="drag-region relative z-10 flex h-16 shrink-0 items-center justify-between gap-4 border-b border-white/[0.06] px-8"
    >
      <nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-1.5 text-[12.5px]">
        <span className="text-white/35">Prism</span>
        <ChevronRight className="h-3.5 w-3.5 text-white/20" aria-hidden="true" />
        <span className="font-medium tracking-tight text-white/80">System</span>
      </nav>

      <div className="no-drag flex items-center gap-3">
        {capturedAt !== null ? (
          <span className="tabular hidden text-[11px] text-white/25 sm:inline">
            Updated {formatClock(capturedAt)}
          </span>
        ) : null}

        <StatusPill tone={copy.tone} pulse={status === "ok"}>
          {copy.label}
        </StatusPill>

        <Button
          variant="ghost"
          icon={RefreshCw}
          loading={refreshing}
          onClick={onRefresh}
          aria-label="Refresh diagnostics"
          className="h-8 px-3"
        >
          Refresh
        </Button>
      </div>
    </header>
  );
}
