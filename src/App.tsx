import type { JSX } from "react";
import { TriangleAlert } from "lucide-react";

import { AmbientBackground } from "@/components/AmbientBackground";
import { BootScreen } from "@/components/BootScreen";
import { ErrorScreen } from "@/components/ErrorScreen";
import { Sidebar } from "@/components/Sidebar";
import { WindowHeader, type HeaderStatus } from "@/components/WindowHeader";
import { DatabaseSection } from "@/components/sections/DatabaseSection";
import { OverviewSection } from "@/components/sections/OverviewSection";
import { RuntimeSection } from "@/components/sections/RuntimeSection";
import { useSystemSnapshot } from "@/hooks/useSystemSnapshot";

/**
 * Application shell.
 *
 * The sidebar and header paint immediately from static geometry while the
 * snapshot loads; the content column swaps skeleton → diagnostics without
 * moving either of them. Refresh failures degrade to an inline banner instead
 * of replacing a valid screen.
 */
export function App(): JSX.Element {
  const { snapshot, error, isLoading, refreshing, refresh } = useSystemSnapshot();

  const headerStatus: HeaderStatus =
    snapshot !== null ? snapshot.health.status : error !== null ? "offline" : "loading";

  return (
    <div className="relative flex h-full overflow-hidden bg-oled-950 text-white">
      <AmbientBackground />

      <Sidebar appInfo={snapshot?.appInfo ?? null} healthStatus={snapshot?.health.status ?? null} />

      <div className="relative z-10 flex min-w-0 flex-1 flex-col">
        <WindowHeader
          status={headerStatus}
          capturedAt={snapshot?.capturedAt ?? null}
          refreshing={refreshing}
          onRefresh={refresh}
        />

        <main className="relative min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-[1180px] px-8 pt-9 pb-20">
            {isLoading && snapshot === null ? <BootScreen /> : null}

            {!isLoading && snapshot === null ? (
              <ErrorScreen
                error={
                  error ?? {
                    code: "unexpected",
                    message: "The backend returned no data and no diagnostic message.",
                  }
                }
                refreshing={refreshing}
                onRetry={refresh}
              />
            ) : null}

            {snapshot !== null ? (
              <div className="space-y-16">
                {error !== null ? (
                  <div
                    role="alert"
                    className="flex items-start gap-3 rounded-2xl border border-warning/20 bg-warning/[0.06] px-4 py-3"
                  >
                    <TriangleAlert
                      className="mt-0.5 h-4 w-4 shrink-0 text-warning"
                      aria-hidden="true"
                    />
                    <div>
                      <p className="text-[12px] font-medium text-warning">
                        Last refresh failed · {error.code}
                      </p>
                      <p className="mt-0.5 text-[12px] text-white/45">{error.message}</p>
                      <p className="mt-1 text-[11px] text-white/30">
                        Showing the most recent successful snapshot.
                      </p>
                    </div>
                  </div>
                ) : null}

                <OverviewSection snapshot={snapshot} refreshing={refreshing} onRefresh={refresh} />
                <DatabaseSection snapshot={snapshot} />
                <RuntimeSection snapshot={snapshot} />

                <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-white/[0.06] pt-6 text-[11px] text-white/25">
                  <p>
                    {snapshot.appInfo.name} {snapshot.appInfo.version} · MIT licensed
                  </p>
                  <p>Rust 2024 · Tauri v2 · SQLite WAL · Built for close listening</p>
                </footer>
              </div>
            ) : null}
          </div>
        </main>
      </div>
    </div>
  );
}
