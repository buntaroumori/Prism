import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { Info, TriangleAlert, X } from "lucide-react";

import { AmbientBackground } from "@/components/AmbientBackground";
import { ContextMenuHost } from "@/components/ContextMenuHost";
import { LyricsSheet } from "@/components/player/LyricsSheet";
import { PlayerBar } from "@/components/player/PlayerBar";
import { Sidebar } from "@/components/Sidebar";
import { TopBar } from "@/components/TopBar";
import { HomeView } from "@/components/views/HomeView";
import { LibraryView } from "@/components/views/LibraryView";
import { NowPlayingView } from "@/components/views/NowPlayingView";
import { PlaylistsView } from "@/components/views/PlaylistsView";
import { PlaylistView } from "@/components/views/PlaylistView";
import { QueueView } from "@/components/views/QueueView";
import { AlbumView } from "@/components/views/AlbumView";
import { ArtistView } from "@/components/views/ArtistView";
import { SettingsView } from "@/components/views/SettingsView";
import { useAlbumCoverKey } from "@/hooks/useAlbumCoverKey";
import { useArtworkPalette } from "@/hooks/useArtworkPalette";
import { useFrameBudget } from "@/hooks/useFrameBudget";
import { useDesktopControls } from "@/hooks/useDesktopControls";
import { usePlayerBridge } from "@/hooks/usePlayerBridge";
import { useScanEvents } from "@/hooks/useScanEvents";
import { useLibraryStore } from "@/stores/libraryStore";
import { useOutputStore } from "@/stores/outputStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { usePlaylistStore } from "@/stores/playlistStore";
import { useUiStore, type RouteParams, type ShellRoute } from "@/stores/uiStore";
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
  useDesktopControls();
  // Measures its own frame pacing and, on a compositor that cannot keep up,
  // sheds the layers that need a real GPU. See `useFrameBudget`.
  useFrameBudget();
  const loadOverview = useLibraryStore((state) => state.loadOverview);
  const loadPlaylists = usePlaylistStore((state) => state.load);
  const applyStoredOutput = useOutputStore((state) => state.applyStored);
  const libraryError = useLibraryStore((state) => state.error);
  const playbackError = usePlaybackStore((state) => state.error);
  const clearLibraryError = useLibraryStore((state) => state.clearError);
  const clearPlaybackError = usePlaybackStore((state) => state.clearError);
  const notice = usePlaybackStore((state) => state.notice);
  const clearNotice = usePlaybackStore((state) => state.clearNotice);
  const theme = useUiStore((state) => state.theme);
  const [resolvedTheme, setResolvedTheme] = useState<"light" | "dark">(() =>
    document.documentElement.classList.contains("light") ? "light" : "dark",
  );

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = (): void => {
      const resolved = theme === "system" ? (media.matches ? "dark" : "light") : theme;
      const root = document.documentElement;
      root.classList.toggle("light", resolved === "light");
      root.classList.toggle("dark", resolved === "dark");
      root.style.colorScheme = resolved;
      root.style.setProperty(
        "--prism-boot-background",
        resolved === "light" ? "#f4f4f6" : "#08080a",
      );
      document
        .querySelector('meta[name="theme-color"]')
        ?.setAttribute("content", resolved === "light" ? "#f4f4f6" : "#08080a");
      document
        .querySelector('meta[name="color-scheme"]')
        ?.setAttribute("content", resolved === "light" ? "light" : "dark");
      setResolvedTheme(resolved);
    };
    apply();
    media.addEventListener("change", apply);
    return () => {
      media.removeEventListener("change", apply);
    };
  }, [theme]);

  useEffect(() => {
    void loadOverview();
    void loadPlaylists();
  }, [loadOverview, loadPlaylists]);

  // The device, buffer and rate policy are remembered between runs, so they
  // have to be handed to the engine once the shell is alive — otherwise the
  // panel shows a choice the engine never received.
  useEffect(() => {
    void applyStoredOutput();
  }, [applyStoredOutput]);

  const route = useUiStore((state) => state.route);
  const params = useUiStore((state) => state.params);
  const sidebarHidden = useUiStore((state) => state.sidebarHidden);
  const scrollRestore = useUiStore((state) => state.scrollRestore);
  const lyricsOpen = useUiStore((state) => state.lyricsOpen);
  const setLyricsOpen = useUiStore((state) => state.setLyricsOpen);
  const mainRef = useRef<HTMLElement | null>(null);
  const [sidebarPeek, setSidebarPeek] = useState(false);
  const intensity = useUiStore((state) => state.ambientIntensity);
  const accentSource = useUiStore((state) => state.accentSource);
  const accentColor = useUiStore((state) => state.accentColor);
  const glassOpacity = useUiStore((state) => state.glassOpacity);
  const tintStrength = useUiStore((state) => state.tintStrength);
  const corners = useUiStore((state) => state.corners);
  const density = useUiStore((state) => state.density);
  const measuredEffects = useUiStore((state) => state.measuredEffects);
  const effectsPreference = useUiStore((state) => state.effects);
  const resolvedEffects =
    effectsPreference === "auto" ? (measuredEffects ?? "full") : effectsPreference;

  // The palette follows the *album* currently loaded, so a queue of tracks from
  // one record does not re-tint the window on every skip.
  const trackId = usePlaybackStore((state) => state.status?.track?.trackId ?? null);
  const album = usePlaybackStore((state) => state.status?.track?.album ?? null);
  const artist = usePlaybackStore((state) => state.status?.track?.artist ?? null);
  const coverKey = useAlbumCoverKey(album, trackId, artist);
  const palette = useArtworkPalette(accentSource === "artwork" ? coverKey : null);
  useEffect(() => {
    setSidebarPeek(false);
  }, [route, sidebarHidden]);

  useLayoutEffect(() => {
    if (scrollRestore === null) return;
    const scroller = mainRef.current;
    if (scroller === null) return;

    let restored = false;
    const restore = (): void => {
      if (restored) return;
      const top = Math.max(0, scrollRestore.top);
      const hasRoom = scroller.scrollHeight - scroller.clientHeight >= top - 1;
      if (!hasRoom) return;

      // One write, before paint whenever the content is already available.
      // If pagination still needs to grow, later mutations retry the geometry
      // check but never drag the user through every intermediate offset.
      restored = true;
      const behavior = scroller.style.scrollBehavior;
      scroller.style.scrollBehavior = "auto";
      scroller.scrollTop = top;
      scroller.style.scrollBehavior = behavior;
      observer.disconnect();
    };

    const observer = new MutationObserver(restore);
    observer.observe(scroller, { childList: true, subtree: true });
    restore();
    const frame = window.requestAnimationFrame(restore);
    window.addEventListener("resize", restore);
    return () => {
      restored = true;
      observer.disconnect();
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", restore);
    };
  }, [scrollRestore]);

  const tokens = paletteTokens(palette, {
    accentSource,
    accentColor,
    glassOpacity,
    tintStrength,
    corners,
    density,
    light: resolvedTheme === "light",
  });

  return (
    <div
      className="relative flex h-full overflow-hidden bg-oled-950 text-white"
      style={tokens}
      data-route={route}
      data-theme={resolvedTheme}
      data-lyrics-open={lyricsOpen}
      data-density={density}
      data-corners={corners}
      data-effects={resolvedEffects}
    >
      <AmbientBackground
        palette={palette}
        intensity={intensity}
        colourBleed={tintStrength / 100}
        reduced={resolvedEffects === "reduced"}
      />

      {!sidebarHidden ? <Sidebar /> : null}
      {sidebarHidden ? (
        <div
          className="fixed inset-y-0 left-0 z-[60] w-3"
          onPointerEnter={() => {
            setSidebarPeek(true);
          }}
          onPointerLeave={() => {
            setSidebarPeek(false);
          }}
        >
          <div
            aria-hidden="true"
            className="absolute inset-y-0 left-0 w-[2px] bg-accent/25 opacity-60"
          />
          {sidebarPeek ? (
            <div className="absolute inset-y-0 left-0 shadow-[24px_0_70px_-30px_rgb(0_0_0/0.95)]">
              <Sidebar />
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="relative z-10 flex min-w-0 flex-1 flex-col">
        <TopBar />

        <div className="relative flex min-h-0 min-w-0 flex-1">
          <main
            ref={mainRef}
            data-prism-scroll
            className={
              route === "now-playing"
                ? "relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
                : "relative flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto"
            }
          >
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

            {notice !== null ? (
              <div className="mx-auto w-full max-w-[1420px] px-8 pt-4">
                <div
                  role="status"
                  className="flex items-start gap-3 rounded-2xl border border-warning/20 bg-warning/[0.05] px-4 py-2.5"
                >
                  <Info className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-hidden="true" />
                  <div className="min-w-0 flex-1">
                    <p className="text-[12px] font-medium text-warning">Skipped · {notice.title}</p>
                    <p className="mt-0.5 text-[11.5px] text-white/50">{notice.message}</p>
                  </div>
                  <button
                    type="button"
                    aria-label="Dismiss"
                    onClick={clearNotice}
                    className="grid h-6 w-6 shrink-0 place-items-center rounded-full text-white/30 hover:bg-white/[0.06] hover:text-white/70"
                  >
                    <X className="h-3.5 w-3.5" aria-hidden="true" />
                  </button>
                </div>
              </div>
            ) : null}

            <PageTransition route={route} params={params} restoring={scrollRestore !== null} />
          </main>

          <AnimatePresence initial={false}>
            {lyricsOpen && route !== "now-playing" ? (
              <LyricsSheet
                onClose={() => {
                  setLyricsOpen(false);
                }}
              />
            ) : null}
          </AnimatePresence>
        </div>

        <div className={route === "now-playing" ? "hidden" : "contents"}>
          <PlayerBar />
        </div>
      </div>

      <ContextMenuHost />
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
function PageTransition({
  route,
  params,
  restoring,
}: {
  route: ShellRoute;
  params: RouteParams;
  restoring: boolean;
}): JSX.Element {
  const reduce = useReducedMotion();
  const key = `${route}:${params.albumId ?? ""}:${params.artistId ?? ""}:${params.collection ?? ""}:${params.genre ?? ""}:${params.playlistId ?? ""}`;

  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={key}
        initial={reduce === true || restoring ? false : { opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: 0, transition: { duration: 0 } }}
        transition={{ duration: restoring ? 0 : 0.18, ease: [0.22, 1, 0.36, 1] }}
        className={route === "now-playing" ? "min-h-0 w-full flex-1" : "min-h-full w-full"}
      >
        {route === "home" ? <HomeView /> : null}
        {route === "library" ? <LibraryView /> : null}
        {route === "collection" ? (
          <LibraryView collection={params.collection} genre={params.genre} />
        ) : null}
        {route === "playlists" ? <PlaylistsView /> : null}
        {route === "playlist" && params.playlistId !== null ? (
          <PlaylistView playlistId={params.playlistId} />
        ) : null}
        {route === "album" && params.albumId !== null ? (
          <AlbumView albumId={params.albumId} />
        ) : null}
        {route === "artist" && params.artistId !== null ? (
          <ArtistView artistId={params.artistId} />
        ) : null}
        {route === "now-playing" ? <NowPlayingView /> : null}
        {route === "queue" ? <QueueView /> : null}
        {route === "settings" ? <SettingsView /> : null}
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
function paletteTokens(
  palette: CoverPalette | null,
  appearance: {
    accentSource: "artwork" | "fixed";
    accentColor: string;
    glassOpacity: number;
    tintStrength: number;
    corners: "sharp" | "soft" | "round";
    density: "comfortable" | "compact";
    light: boolean;
  },
): CSSProperties {
  const tokens: Record<string, string> = {
    // Appearance knobs become CSS variables once, at the shell root. Every
    // surface reads them, so a slider move is one style write, not a repaint
    // cascade through fifty components.
    "--prism-glass": (0.012 + (appearance.glassOpacity / 100) * 0.06).toFixed(4),
    "--prism-radius":
      appearance.corners === "sharp"
        ? "0rem"
        : appearance.corners === "round"
          ? "1.5rem"
          : "0.75rem",
    "--prism-row-gap": appearance.density === "compact" ? "0.3rem" : "0.65rem",
    "--prism-grid-gap": appearance.density === "compact" ? "1rem" : "1.75rem",
  };

  if (appearance.accentSource === "fixed") {
    tokens["--prism-accent"] = appearance.accentColor;
    tokens["--prism-accent-soft"] = hexAlpha(appearance.accentColor, 0.18);
  }

  if (palette === null) {
    return tokens;
  }

  const neutralSurface = appearance.light ? "#ffffff" : "#12131c";
  const neutralBackground = appearance.light ? "#f4f4f6" : "#08080a";
  const strength = appearance.light ? appearance.tintStrength * 0.22 : appearance.tintStrength;
  const surfaceColour = appearance.light ? palette.highlight : palette.surface;
  const backgroundColour = appearance.light ? palette.accent : palette.background;

  return {
    ...tokens,
    ...(appearance.accentSource === "artwork"
      ? {
          "--prism-accent": palette.accent,
          "--prism-accent-soft": palette.accentSoft,
          "--prism-surface": mixTint(surfaceColour, neutralSurface, strength),
          "--prism-background": mixTint(backgroundColour, neutralBackground, strength),
          "--prism-highlight": palette.highlight,
          "--prism-highlight-alt": palette.highlightAlt,
        }
      : {
          "--prism-surface": mixTint(surfaceColour, neutralSurface, strength),
          "--prism-background": mixTint(backgroundColour, neutralBackground, strength),
        }),
  } as CSSProperties;
}

/** Mix an artwork colour with the neutral shell at the user's chosen strength. */
function mixTint(colour: string, neutral: string, strength: number): string {
  const percent = Math.min(100, Math.max(0, strength));
  return `color-mix(in srgb, ${colour} ${String(percent)}%, ${neutral})`;
}

/** `#rrggbb` → rgba() at the given alpha, for the soft accent token. */
function hexAlpha(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  if (value.length !== 6) {
    return hex;
  }
  const r = Number.parseInt(value.slice(0, 2), 16);
  const g = Number.parseInt(value.slice(2, 4), 16);
  const b = Number.parseInt(value.slice(4, 6), 16);
  return `rgba(${String(r)}, ${String(g)}, ${String(b)}, ${String(alpha)})`;
}
