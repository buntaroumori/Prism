import type { JSX } from "react";

import { AppLogo } from "@/components/AppLogo";

/**
 * Boot placeholder shown while the Rust backend opens the database.
 *
 * Structurally identical to the loaded layout (sidebar rail, header, four
 * counters) so the first commit after data arrives is a content swap, not a
 * layout shift — the CLS budget for startup is 0.
 */
export function BootScreen(): JSX.Element {
  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      className="animate-[fade-up_420ms_cubic-bezier(0.22,1,0.36,1)_both] pt-10"
    >
      <div className="flex items-center gap-3">
        <AppLogo className="h-10 w-10 animate-pulse" />
        <div>
          <p className="text-[10px] font-medium tracking-[0.18em] text-white/30 uppercase">
            Initialising
          </p>
          <p className="text-sm text-white/60">Opening the library catalogue…</p>
        </div>
      </div>

      <div className="mt-10 h-10 w-80 animate-pulse rounded-2xl bg-white/[0.05]" />
      <div className="mt-4 h-4 w-[28rem] max-w-full animate-pulse rounded-full bg-white/[0.035]" />

      <div className="mt-12 grid grid-cols-2 gap-3 lg:grid-cols-4">
        {["Tracks", "Albums", "Artists", "Size"].map((label) => (
          <div key={label} className="glass-subtle rounded-2xl px-5 py-4">
            <p className="text-[10px] font-medium tracking-[0.14em] text-white/25 uppercase">
              {label}
            </p>
            <div className="mt-3 h-6 w-16 animate-pulse rounded-md bg-white/[0.06]" />
          </div>
        ))}
      </div>

      <div className="mt-10 grid gap-4 lg:grid-cols-2">
        <div className="glass h-56 animate-pulse rounded-3xl" />
        <div className="glass h-56 animate-pulse rounded-3xl" />
      </div>

      <span className="sr-only">Loading application diagnostics</span>
    </div>
  );
}
