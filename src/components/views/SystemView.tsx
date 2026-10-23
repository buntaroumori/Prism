import type { JSX } from "react";
import { TriangleAlert } from "lucide-react";

import { DatabaseSection } from "@/components/sections/DatabaseSection";
import { RuntimeSection } from "@/components/sections/RuntimeSection";
import { ErrorScreen } from "@/components/ErrorScreen";
import { useSystemSnapshot } from "@/hooks/useSystemSnapshot";

/**
 * Diagnostics.
 *
 * The FASE 1 screen, kept intact and moved behind its own route: database
 * pragmas, migration ledger, engine guarantees and the captured-at timestamp.
 * It is the place to look when something sounds wrong, so it is deliberately
 * plain — no palette tinting on any value that a user might need to read out
 * loud over the phone.
 */
export function SystemView({ embedded = false }: { embedded?: boolean }): JSX.Element {
  const { snapshot, error, isLoading, refreshing, refresh } = useSystemSnapshot();

  if (isLoading && snapshot === null) {
    return (
      <div className="mx-auto w-full max-w-[980px] px-8 pt-8">
        <div className="h-40 animate-pulse rounded-2xl bg-white/[0.03]" />
      </div>
    );
  }

  if (snapshot === null) {
    return (
      <div className="mx-auto w-full max-w-[980px] px-8 pt-10">
        <ErrorScreen
          error={
            error ?? {
              code: "unexpected",
              message: "Prism could not read its system information. Try again.",
            }
          }
          refreshing={refreshing}
          onRetry={refresh}
        />
      </div>
    );
  }

  return (
    <div
      className={
        embedded
          ? "space-y-8 border-t border-white/[0.06] pt-8"
          : "mx-auto w-full max-w-[980px] space-y-12 px-8 pt-8 pb-14"
      }
    >
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 className="text-[20px] font-semibold tracking-[-0.02em] text-white">System</h2>
          <p className="mt-1 text-[13px] text-white/40">
            {snapshot.appInfo.name} {snapshot.appInfo.version} · {snapshot.appInfo.targetOs}
          </p>
        </div>
        <p className="font-mono text-[10.5px] text-white/25">{snapshot.appInfo.databasePath}</p>
      </header>

      {error !== null ? (
        <div
          role="alert"
          className="flex items-start gap-3 rounded-2xl border border-warning/20 bg-warning/[0.06] px-4 py-3"
        >
          <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-hidden="true" />
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

      <DatabaseSection snapshot={snapshot} />
      <RuntimeSection snapshot={snapshot} />

      <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-white/[0.06] pt-6 text-[11px] text-white/25">
        <p>{snapshot.appInfo.name} · MIT licensed</p>
        <p>Your music stays on this device</p>
      </footer>
    </div>
  );
}
