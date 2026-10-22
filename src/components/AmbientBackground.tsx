import type { JSX } from "react";

import type { CoverPalette } from "@/types/ipc";

interface AmbientBackgroundProps {
  palette: CoverPalette | null;
  /** `off` paints the OLED flat colour only. */
  intensity: "off" | "subtle" | "full";
  /** 0–1 amount of artwork colour allowed into the canvas. */
  colourBleed: number;
  /**
   * `reduced` swaps the three blurred, drifting orbs for one static gradient.
   *
   * The blobs are the most expensive thing in the product on a software
   * compositor: three 50-rem surfaces with a 170 px blur, translated every
   * frame. A single radial gradient over the same palette keeps the mood and
   * costs one paint.
   */
  reduced?: boolean;
}

/**
 * Ambient canvas.
 *
 * Three blurred, palette-tinted orbs composited on the GPU. Nothing animates on
 * the main thread: the drift is a CSS transform animation, the tint is three
 * custom properties, and the only per-track cost is a style write on the
 * container. That keeps the FASE 6 idle-CPU budget reachable with a full-screen
 * background effect, which a per-frame canvas loop would not.
 *
 * The orbs are scaled by the artwork's mean lightness: a white album sleeve
 * produces the same bloom as a dark one without washing the interface out.
 */
export function AmbientBackground({
  palette,
  intensity,
  colourBleed,
  reduced = false,
}: AmbientBackgroundProps): JSX.Element {
  if (intensity === "off") {
    return <div aria-hidden="true" className="pointer-events-none absolute inset-0 bg-oled-950" />;
  }

  const bleed = Math.min(1, Math.max(0, colourBleed));
  const strength = (intensity === "subtle" ? 0.45 : 1) * bleed;
  const lift = palette === null ? 0.5 : 0.35 + palette.averageLightness * 0.9;
  const accent = palette?.accent ?? "var(--prism-accent)";
  const highlight = palette?.highlight ?? "var(--prism-highlight)";
  const alt = palette?.highlightAlt ?? "var(--prism-highlight-alt)";

  // Opacity is derived, not fixed: a bright cover gets less pigment because the
  // same alpha over a lighter base reads much stronger.
  const primary = (0.3 * strength * (0.55 + (1 - lift) * 0.75)).toFixed(3);
  const secondary = (0.2 * strength * (0.55 + (1 - lift) * 0.8)).toFixed(3);
  const tertiary = (0.16 * strength).toFixed(3);

  if (reduced) {
    return (
      <div aria-hidden="true" className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute inset-0 bg-oled-950" />
        <div
          className="absolute inset-0"
          style={{
            background: `radial-gradient(90% 60% at 20% 0%, ${accent} 0%, transparent 60%), radial-gradient(70% 55% at 92% 12%, ${highlight} 0%, transparent 62%)`,
            opacity: Number(primary) * 0.55,
          }}
        />
        <div className="absolute inset-0 bg-[radial-gradient(120%_85%_at_50%_0%,transparent_18%,rgb(8_8_10/0.6)_72%,rgb(8_8_10/0.92)_100%)]" />
      </div>
    );
  }

  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0 overflow-hidden">
      <div className="absolute inset-0 bg-oled-950" />

      <div
        className="ambient-orb absolute -top-[26rem] left-[12%] h-[54rem] w-[54rem] rounded-full blur-[170px]"
        style={{
          background: accent,
          opacity: Number(primary),
          animationDelay: "-2s",
        }}
      />
      <div
        className="ambient-orb absolute top-[6%] -right-[20rem] h-[46rem] w-[46rem] rounded-full blur-[160px]"
        style={{
          background: highlight,
          opacity: Number(secondary),
          animationDelay: "-7s",
        }}
      />
      <div
        className="ambient-orb absolute -bottom-[28rem] left-[26%] h-[50rem] w-[50rem] rounded-full blur-[180px]"
        style={{
          background: alt,
          opacity: Number(tertiary),
          animationDelay: "-11s",
        }}
      />

      {/* Vignette: keeps the outer 15 % of the window readable against the orbs. */}
      <div className="absolute inset-0 bg-[radial-gradient(120%_85%_at_50%_0%,transparent_18%,rgb(8_8_10/0.6)_72%,rgb(8_8_10/0.92)_100%)]" />
      <div className="grain absolute inset-0 opacity-[0.03] mix-blend-overlay" />
    </div>
  );
}
