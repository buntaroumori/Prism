import type { JSX } from "react";
/**
 * Static ambient canvas for the shell.
 *
 * FASE 1 ships a GPU-cheap composition of blurred gradient orbs (pure CSS:
 * compositor-only `transform`/`opacity` animation, zero main-thread work).
 * FASE 4 replaces the colour source with palette extraction from the current
 * artwork, keeping this exact DOM structure so the WebGL/Canvas upgrade does
 * not cause a layout shift.
 */
export function AmbientBackground(): JSX.Element {
  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0 overflow-hidden">
      <div className="absolute inset-0 bg-oled-950" />
      <div className="absolute -top-[28rem] left-[18%] h-[52rem] w-[52rem] animate-breathe rounded-full bg-accent/[0.16] blur-[160px]" />
      <div
        className="absolute top-[10%] -right-[18rem] h-[44rem] w-[44rem] animate-breathe rounded-full bg-spectrum-pink/[0.075] blur-[150px]"
        style={{ animationDelay: "-3s" }}
      />
      <div
        className="absolute -bottom-[26rem] left-[30%] h-[46rem] w-[46rem] animate-breathe rounded-full bg-spectrum-cyan/[0.05] blur-[170px]"
        style={{ animationDelay: "-6s" }}
      />
      <div className="absolute inset-0 bg-[radial-gradient(120%_80%_at_50%_0%,transparent_20%,rgb(8_8_10/0.55)_70%,rgb(8_8_10/0.9)_100%)]" />
      <div className="grain absolute inset-0 opacity-[0.035] mix-blend-overlay" />
    </div>
  );
}
