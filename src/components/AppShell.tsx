import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useEffect, type CSSProperties, type JSX } from "react";
import { TriangleAlert, X } from "lucide-react";

import { AmbientBackground } from "@/components/AmbientBackground";
import { PlayerBar } from "@/components/player/PlayerBar";
import { Sidebar } from "@/components/Sidebar";
import { TopBar } from "@/components/TopBar";
import { HomeView } from "@/components/views/HomeView";
import { LibraryView } from "@/components/views/LibraryView";
import { NowPlayingView } from "@/components/views/NowPlayingView";
import { SettingsView } from "@/components/views/SettingsView";
import { SystemView } from "@/components/views/SystemView";
import { useAlbumCoverKey } from "@/hooks/useAlbumCoverKey";
import { useArtworkPalette } from "@/hooks/useArtworkPalette";
import { usePlayerBridge } from "@/hooks/usePlayerBridge";
import { useScanEvents } from "@/hooks/useScanEvents";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore, type ShellRoute } from "@/stores/uiStore";
import type { CoverPalette } from "@/types/ipc";

/**
 * Application shell.
 *
 * Owns three long-lived subscriptions (engine, scanner, palette), the palette →
 * CSS-variable projection, and the routed content column. The player bar is a
 * sibling of the scroller, not an overlay, so it can never cover the last row of
 * a list and the content column reserves its height by construction (CLS = 0).
 */
export function AppShell(): JSX.Element {
  usePlayerBridge();
  useScanEvents();
  const loadOverview = useLibraryStore((state) => state.loadOverview);
  const libraryError = useLibraryStore((state) => state.error);
  const playbackError = usePlaybackStore((state) => state.error);
  const clearLibraryError = useLibraryStore((state) => state.clearError);
  const clearPlaybackError = usePlaybackStore((state) => state.clearError);

  useEffect(() => {
    void loadOverview();
  }, [loadOverview]);

  const route = useUiStore((state) => state.route);
  const intensity = useUiStore((state) => state.ambientIntensity);

  // The palette follows the *album* currently loaded, so a queue of tracks from
  // one record does not re-tint the window on every skip.
  const trackId = usePlaybackStore((state) => state.status?.track?.trackId ?? null);
  const album = usePlaybackStore((state) => state.status?.track?.album ?? null);
  const coverKey = useAlbumCoverKey(album, trackId);
  const palette = useArtworkPalette(coverKey);
  const tokens = paletteTokens(palette);

  return (
    <div
      className="relative flex h-full overflow-hidden bg-oled-950 text-white"
      style={tokens}
      data-route={route}
    >
      <AmbientBackground palette={palette} intensity={intensity} />

      <Sidebar />

      <div className="relative z-10 flex min-w-0 flex-1 flex-col">
        <TopBar />

        <main className="relative min-h-0 flex-1 overflow-y-auto">
          {(libraryError !== null || playbackError !== null) && (
            <div className="mx-auto w-full max-w-[1420px] px-8 pt-4">
              {[libraryError, playbackError].map((failure, index) =>
                failure === null ? null : (
                  <div
                    key={`${failure.code}-${String(index)}`}
                    role="alert"
                    className="mb-2 flex items-start gap-3 rounded-2xl border border-critical/20 bg-critical/[0.06] px-4 py-2.5"
                  >
                    <TriangleAlert
                      className="mt-0.5 h-4 w-4 shrink-0 text-critical"
                      aria-hidden="true"
                    />
                    <div className="min-w-0 flex-1">
                      <p className="text-[12px] font-medium text-critical">
                        {failure.code === "output" ? "Output device" : "Request failed"} ·{" "}
                        {failure.code}
                      </p>
                      <p className="mt-0.5 text-[11.5px] text-white/50">{failure.message}</p>
                    </div>
                    <button
                      type="button"
                      aria-label="Dismiss"
                      onClick={() => {
                        clearLibraryError();
                        clearPlaybackError();
                      }}
                      className="grid h-6 w-6 shrink-0 place-items-center rounded-full text-white/30 hover:bg-white/[0.06] hover:text-white/70"
                    >
                      <X className="h-3.5 w-3.5" aria-hidden="true" />
                    </button>
                  </div>
                ),
              )}
            </div>
          )}

          <PageTransition route={route} />
        </main>

        <PlayerBar />
      </div>
    </div>
  );
}

/**
 * Route content with a short cross-fade.
 *
 * 180 ms, opacity + 6 px of travel, `mode="wait"`: long enough to feel like a
 * change of place, short enough that it never delays an interaction. Reduced
 * motion collapses it to a plain swap.
 */
function PageTransition({ route }: { route: ShellRoute }): JSX.Element {
  const reduce = useReducedMotion();

  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={route}
        initial={reduce === true ? false : { opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        exit={reduce === true ? { opacity: 1, y: 0 } : { opacity: 0, y: -6 }}
        transition={{ duration: 0.18, ease: [0.22, 1, 0.36, 1] }}
        className="h-full"
      >
        {route === "home" ? <HomeView /> : null}
        {route === "library" ? <LibraryView /> : null}
        {route === "now-playing" ? <NowPlayingView /> : null}
        {route === "settings" ? <SettingsView /> : null}
        {route === "system" ? <SystemView /> : null}
      </motion.div>
    </AnimatePresence>
  );
}

/**
 * Project the palette onto CSS custom properties.
 *
 * One style object on the shell root is the entire runtime cost of the theming
 * system: every component reads `var(--prism-accent)` in CSS, so a track change
 * writes six strings and lets the compositor do the rest. With no palette yet
 * the browser keeps the neutral tokens declared in `globals.css`.
 */
function paletteTokens(palette: CoverPalette | null): CSSProperties {
  if (palette === null) {
    return {};
  }
  return {
    "--prism-accent": palette.accent,
    "--prism-accent-soft": palette.accentSoft,
    "--prism-surface": palette.surface,
    "--prism-background": palette.background,
    "--prism-highlight": palette.highlight,
    "--prism-highlight-alt": palette.highlightAlt,
  } as CSSProperties;
}
