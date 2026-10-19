import type { JSX } from "react";
import { Command, MonitorPlay, Terminal } from "lucide-react";

import { AppLogo } from "@/components/AppLogo";

/**
 * Shown when the bundle is rendered outside the Tauri shell.
 *
 * Prism has no web mode by design: playback, scanning and artwork all run in the
 * Rust process, so a browser tab genuinely cannot do anything. This screen
 * states that plainly instead of rendering an interface full of empty states
 * that look like a bug.
 */
export function DesktopRequired(): JSX.Element {
  return (
    <div className="relative flex h-full items-center justify-center overflow-hidden bg-oled-950 px-8 text-white">
      <div
        aria-hidden="true"
        className="pointer-events-none absolute -top-40 left-1/2 h-[36rem] w-[36rem] -translate-x-1/2 rounded-full opacity-25 blur-[140px]"
        style={{ background: "var(--prism-accent)" }}
      />

      <div className="relative w-full max-w-[520px]">
        <div className="flex items-center gap-3">
          <AppLogo className="h-10 w-10" />
          <div>
            <p className="text-[15px] font-semibold tracking-tight">Prism</p>
            <p className="text-[10px] font-medium tracking-[0.22em] text-white/30 uppercase">
              Reference player
            </p>
          </div>
        </div>

        <h1 className="mt-9 text-[26px] leading-tight font-semibold tracking-[-0.03em] text-balance">
          This bundle needs the desktop shell.
        </h1>
        <p className="mt-3 text-[13.5px] leading-relaxed text-white/45">
          The audio engine, the SQLite catalogue and the artwork fetchers run in a native Rust
          process. A browser tab has no device access, so Prism intentionally renders nothing it
          cannot back with real data.
        </p>

        <div className="mt-7 space-y-2">
          <CommandRow
            icon={Terminal}
            label="Development"
            command="pnpm tauri dev"
            hint="Builds the Rust binary and opens the window"
          />
          <CommandRow
            icon={MonitorPlay}
            label="Release"
            command="pnpm tauri build"
            hint="Produces .msi / .exe on Windows, .deb / .AppImage on Linux"
          />
        </div>

        <p className="mt-8 flex items-center gap-2 text-[11.5px] text-white/25">
          <Command className="h-3.5 w-3.5" aria-hidden="true" />
          Everything you see in the desktop window is read from your library.
        </p>
      </div>
    </div>
  );
}

function CommandRow({
  icon: Icon,
  label,
  command,
  hint,
}: {
  icon: typeof Terminal;
  label: string;
  command: string;
  hint: string;
}): JSX.Element {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-white/[0.07] bg-white/[0.025] px-3.5 py-2.5">
      <Icon className="h-4 w-4 shrink-0 text-white/30" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="text-[10px] font-semibold tracking-[0.14em] text-white/25 uppercase">
          {label}
        </p>
        <code className="block truncate font-mono text-[12px] text-white/80">{command}</code>
      </div>
      <span className="hidden max-w-[190px] truncate text-right text-[10.5px] text-white/25 sm:block">
        {hint}
      </span>
    </div>
  );
}
