import type { JSX } from "react";
import { MonitorPlay } from "lucide-react";

import { AppLogo } from "@/components/AppLogo";

/** A clear destination when the interface is opened outside the desktop app. */
export function DesktopRequired(): JSX.Element {
  return (
    <div className="relative flex h-full items-center justify-center overflow-hidden bg-oled-950 px-8 text-white">
      <div
        aria-hidden="true"
        className="pointer-events-none absolute -top-40 left-1/2 h-[36rem] w-[36rem] -translate-x-1/2 rounded-full opacity-25 blur-[140px]"
        style={{ background: "var(--prism-accent)" }}
      />
      <div className="relative w-full max-w-[480px] text-center">
        <AppLogo className="mx-auto h-16 w-16" />
        <p className="mt-4 text-[16px] font-semibold">Prism</p>
        <MonitorPlay className="mx-auto mt-8 h-7 w-7 text-white/25" aria-hidden="true" />
        <h1 className="mt-4 text-[26px] leading-tight font-semibold tracking-[-0.03em]">
          Open Prism on your desktop
        </h1>
        <p className="mt-3 text-[13.5px] leading-relaxed text-white/45">
          Prism is a desktop music player for Windows and Linux. Launch the installed app to play
          and organise your music.
        </p>
      </div>
    </div>
  );
}
