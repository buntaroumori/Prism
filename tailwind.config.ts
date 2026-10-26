import type { Config } from "tailwindcss";

/**
 * Tailwind CSS v4 design system for Prism.
 *
 * Tokens are authored here (loaded via `@config` in `src/styles/globals.css`) so
 * they are shared by Tailwind, editor tooling and the eventual docs site. The
 * palette follows an OLED-neutral philosophy: absolute black is reserved for
 * true black displays, surfaces step up in ~4 % luminance increments, and every
 * border is a low-opacity white so glass layers keep working over artwork.
 */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Neutral ink and surfaces are semantic RGB tokens. Dark keeps the
        // original OLED values; `.light` flips the same utilities everywhere.
        white: "rgb(var(--prism-ink) / <alpha-value>)",
        oled: {
          950: "rgb(var(--prism-surface-950) / <alpha-value>)",
          900: "rgb(var(--prism-surface-900) / <alpha-value>)",
          850: "rgb(var(--prism-surface-850) / <alpha-value>)",
          800: "rgb(var(--prism-surface-800) / <alpha-value>)",
          750: "rgb(var(--prism-surface-750) / <alpha-value>)",
          700: "rgb(var(--prism-surface-700) / <alpha-value>)",
        },
        glass: {
          DEFAULT: "rgb(var(--prism-ink) / 0.06)",
          strong: "rgb(var(--prism-ink) / 0.10)",
          border: "rgb(var(--prism-ink) / 0.10)",
        },
        accent: {
          DEFAULT: "#7c8cff",
          soft: "#a7b2ff",
          deep: "#4d5bdb",
        },
        spectrum: {
          pink: "#ff6b9a",
          amber: "#ffb86b",
          lime: "#b8f26b",
          cyan: "#6bd5ff",
          violet: "#b08cff",
        },
        positive: "#4ade80",
        warning: "#fbbf24",
        critical: "#f87171",
      },
      fontFamily: {
        sans: [
          "Inter Variable",
          "SF Pro Display",
          "SF Pro Text",
          "Segoe UI Variable Display",
          "Segoe UI",
          "system-ui",
          "sans-serif",
        ],
        mono: ["SF Mono", "Cascadia Code", "JetBrains Mono", "ui-monospace", "monospace"],
      },
      letterSpacing: {
        tightest: "-0.045em",
      },
      borderRadius: {
        "4xl": "2rem",
        "5xl": "2.5rem",
      },
      boxShadow: {
        float: "0 24px 60px -24px rgb(0 0 0 / 0.65), 0 2px 8px -2px rgb(0 0 0 / 0.4)",
        glass: "inset 0 1px 0 rgb(255 255 255 / 0.08), 0 1px 0 rgb(0 0 0 / 0.5)",
        glow: "0 0 0 1px rgb(124 140 255 / 0.35), 0 8px 32px -8px rgb(124 140 255 / 0.45)",
      },
      backdropBlur: {
        "3xl": "64px",
      },
      backgroundImage: {
        "noise-soft":
          "radial-gradient(120% 120% at 15% 0%, rgb(124 140 255 / 0.18) 0%, transparent 45%), radial-gradient(90% 90% at 85% 10%, rgb(255 107 154 / 0.12) 0%, transparent 50%)",
      },
      keyframes: {
        "fade-up": {
          "0%": { opacity: "0", transform: "translateY(12px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        breathe: {
          "0%, 100%": { opacity: "0.55", transform: "scale(1)" },
          "50%": { opacity: "0.9", transform: "scale(1.04)" },
        },
        shimmer: {
          "0%": { backgroundPosition: "-200% 0" },
          "100%": { backgroundPosition: "200% 0" },
        },
      },
      animation: {
        "fade-up": "fade-up 420ms cubic-bezier(0.22, 1, 0.36, 1) both",
        breathe: "breathe 9s ease-in-out infinite",
        shimmer: "shimmer 2.4s linear infinite",
      },
    },
  },
  plugins: [],
} satisfies Config;
