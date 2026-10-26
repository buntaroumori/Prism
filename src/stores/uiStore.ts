import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import { useBrowseStore } from "@/stores/browseStore";
import { useLibraryStore } from "@/stores/libraryStore";
import type { AlbumCollection } from "@/types/ipc";

/**
 * Top-level destinations.
 *
 * Album and artist pages are real routes, not overlays: every track and artist
 * name in the application is a link to one of them, and a link that cannot be
 * deep-linked (or survives a reload) is not a link.
 */
export type ShellRoute =
  | "home"
  | "library"
  | "collection"
  | "playlists"
  | "playlist"
  | "now-playing"
  | "queue"
  | "settings"
  | "album"
  | "artist";

/** Detail payload for the parameterised routes. */
export interface RouteParams {
  albumId: string | null;
  artistId: string | null;
  collection: AlbumCollection | null;
  genre: string | null;
  playlistId: string | null;
}

/** One return destination, including the scroller position it owned. */
export interface NavigationEntry {
  route: ShellRoute;
  params: RouteParams;
  scrollTop: number;
}

export interface ScrollRestoreRequest {
  nonce: number;
  top: number;
}

const NO_PARAMS: RouteParams = {
  albumId: null,
  artistId: null,
  collection: null,
  genre: null,
  playlistId: null,
};

/** Ambient canvas intensity, persisted per user. */
export type AmbientIntensity = "off" | "subtle" | "full";

/** How far the shell rounds its large surfaces. */
export type CornerStyle = "sharp" | "soft" | "round";

/** Vertical rhythm of every list and grid. */
export type Density = "comfortable" | "compact";

/** Accent policy: follow the artwork, or pin a colour. */
export type AccentSource = "artwork" | "fixed";

/** Fixed colour scheme, or the operating system's current light/dark choice. */
export type ThemePreference = "light" | "dark" | "system";

/**
 * How much of the interface may cost the compositor.
 *
 * `auto` lets the shell measure its own frame pacing and decide; `full` and
 * `reduced` override it. `reduced` keeps the palette, the covers and the
 * typography and drops the layers that need a real GPU: the drifting blurred
 * orbs, the backdrop filters and the grain overlay.
 */
export type EffectsLevel = "auto" | "full" | "reduced";
export type ResolvedEffects = "full" | "reduced";

interface UiState {
  sidebarCollapsed: boolean;
  route: ShellRoute;
  params: RouteParams;
  ambientIntensity: AmbientIntensity;
  /** Palette key currently driving the shell (`null` = neutral). */
  paletteKey: string | null;
  /** In-memory return stack; intentionally never persisted. */
  history: NavigationEntry[];
  scrollRestore: ScrollRestoreRequest | null;
  navigationNonce: number;
  /** Full rail visibility, distinct from the existing narrow/expanded mode. */
  sidebarHidden: boolean;
  /** The right-hand lyrics column is shell state, not a Player Bar dialog. */
  lyricsOpen: boolean;

  /* ------------------------------------------------------------ appearance */
  theme: ThemePreference;
  accentSource: AccentSource;
  accentColor: string;
  /** 0–100: opacity of the frosted surfaces over the OLED black. */
  glassOpacity: number;
  /** 0–100: how strongly the cover palette bleeds into the surface tint. */
  tintStrength: number;
  corners: CornerStyle;
  density: Density;
  /** Cover edge length, in CSS pixels, of the library mosaic. */
  coverSize: number;
  showRailArrows: boolean;
  showHomeStats: boolean;
  showSpectrum: boolean;
  effects: EffectsLevel;
  /** Result of the one-shot frame-pacing probe; `null` until it has run. */
  measuredEffects: ResolvedEffects | null;

  /* ------------------------------------------------------------ navigation */
  toggleSidebar: () => void;
  toggleSidebarHidden: () => void;
  setSidebarHidden: (hidden: boolean) => void;
  navigate: (route: ShellRoute, params?: Partial<RouteParams>) => void;
  openAlbum: (albumId: string) => void;
  openArtist: (artistId: string) => void;
  openCollection: (collection: AlbumCollection | null, genre?: string) => void;
  openPlaylist: (playlistId: string) => void;
  openLibraryGenre: (genre: string) => void;
  setRoute: (route: ShellRoute) => void;
  toggleSettings: () => void;
  toggleQueue: () => void;
  toggleLyrics: () => void;
  setLyricsOpen: (open: boolean) => void;
  back: () => void;

  setMeasuredEffects: (effects: ResolvedEffects) => void;
  /** What the shell should actually render, given the preference and the probe. */
  resolvedEffects: () => ResolvedEffects;
  setAmbientIntensity: (intensity: AmbientIntensity) => void;
  setPaletteKey: (key: string | null) => void;
  setAppearance: (patch: Partial<AppearanceState>) => void;
  resetAppearance: () => void;
}

/** The subset of the store the appearance panel writes. */
export type AppearanceState = Pick<
  UiState,
  | "theme"
  | "accentSource"
  | "accentColor"
  | "glassOpacity"
  | "tintStrength"
  | "corners"
  | "density"
  | "coverSize"
  | "ambientIntensity"
  | "showRailArrows"
  | "showHomeStats"
  | "showSpectrum"
  | "effects"
>;

const DEFAULT_APPEARANCE: AppearanceState = {
  theme: "dark",
  accentSource: "artwork",
  accentColor: "#7dd3fc",
  glassOpacity: 62,
  tintStrength: 45,
  corners: "soft",
  density: "comfortable",
  coverSize: 176,
  ambientIntensity: "full",
  showRailArrows: true,
  showHomeStats: true,
  showSpectrum: false,
  effects: "auto",
};

/** Bounds the slider can produce; the store clamps every write. */
export const COVER_SIZE_MIN = 120;
export const COVER_SIZE_MAX = 260;

/**
 * Persisted shell preferences.
 *
 * Layout and appearance live here. Playback, catalogue data and the playhead
 * have their own stores so toggling the sidebar can never invalidate a
 * 500 000-row list. `route` is deliberately *not* persisted: restoring a deep
 * link into an album that no longer exists is a worse first impression than
 * starting at Home.
 */
export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      sidebarCollapsed: false,
      sidebarHidden: false,
      route: "home",
      params: { ...NO_PARAMS },
      history: [],
      scrollRestore: null,
      navigationNonce: 0,
      paletteKey: null,
      measuredEffects: null,
      lyricsOpen: false,
      ...DEFAULT_APPEARANCE,

      setMeasuredEffects: (measuredEffects) => {
        set({ measuredEffects });
      },

      resolvedEffects: () => {
        const { effects, measuredEffects } = get();
        if (effects !== "auto") {
          return effects;
        }
        // Before the probe has run, assume the machine is capable: the probe
        // exists to correct a slow compositor, not to punish every start-up.
        return measuredEffects ?? "full";
      },

      toggleSidebar: () => {
        set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed }));
      },

      toggleSidebarHidden: () => {
        set((state) => ({ sidebarHidden: !state.sidebarHidden }));
      },

      setSidebarHidden: (sidebarHidden) => {
        set({ sidebarHidden });
      },

      navigate: (route, params) => {
        clearLibraryTransientState(get().route, route);
        set((state) => go(state, route, routeParams(params)));
      },

      openAlbum: (albumId) => {
        get().navigate("album", { albumId });
      },

      openArtist: (artistId) => {
        get().navigate("artist", { artistId });
      },

      openCollection: (collection, genre) => {
        get().navigate("collection", { collection, genre: genre ?? null });
      },

      openPlaylist: (playlistId) => {
        get().navigate("playlist", { playlistId });
      },

      openLibraryGenre: (genre) => {
        const normalized = genre.trim();
        if (normalized.length === 0) return;
        useLibraryStore.getState().setQuery("");
        useBrowseStore.getState().clearFilters();
        useBrowseStore.getState().setGenre(normalized);
        get().navigate("library");
      },

      setRoute: (route) => {
        get().navigate(route);
      },

      toggleSettings: () => {
        if (get().route === "settings") {
          get().back();
        } else {
          get().setRoute("settings");
        }
      },

      toggleQueue: () => {
        if (get().route === "queue") {
          get().back();
        } else {
          get().setRoute("queue");
        }
      },

      toggleLyrics: () => {
        set((state) => ({ lyricsOpen: !state.lyricsOpen }));
      },

      setLyricsOpen: (lyricsOpen) => {
        set({ lyricsOpen });
      },

      back: () => {
        const state = get();
        const previous = state.history[state.history.length - 1];
        const nonce = state.navigationNonce + 1;
        if (previous !== undefined) {
          clearLibraryTransientState(state.route, previous.route);
          if (previous.route === "library") {
            useLibraryStore.getState().setQuery("");
            useBrowseStore.getState().clearFilters();
          }
          set({
            route: previous.route,
            params: previous.params,
            history: state.history.slice(0, -1),
            navigationNonce: nonce,
            scrollRestore: { nonce, top: previous.scrollTop },
            lyricsOpen: previous.route === "now-playing" ? false : state.lyricsOpen,
          });
          return;
        }

        // Directly opened detail routes still have a sensible fallback.
        const fallback: Partial<Record<ShellRoute, ShellRoute>> = {
          album: "library",
          artist: "library",
          collection: "home",
          playlist: "playlists",
          queue: "home",
          settings: "home",
          "now-playing": "home",
        };
        const route = fallback[state.route];
        if (route === undefined) return;
        clearLibraryTransientState(state.route, route);
        set({
          route,
          params: { ...NO_PARAMS },
          navigationNonce: nonce,
          scrollRestore: { nonce, top: 0 },
          lyricsOpen: route === "now-playing" ? false : state.lyricsOpen,
        });
      },

      setAmbientIntensity: (ambientIntensity) => {
        set({ ambientIntensity });
      },

      setPaletteKey: (paletteKey) => {
        set({ paletteKey });
      },

      setAppearance: (patch) => {
        set((state) => {
          const next: AppearanceState = { ...state, ...patch };
          return {
            theme: next.theme,
            accentSource: next.accentSource,
            accentColor: next.accentColor,
            glassOpacity: clamp(Math.round(next.glassOpacity), 0, 100),
            tintStrength: clamp(Math.round(next.tintStrength), 0, 100),
            corners: next.corners,
            density: next.density,
            coverSize: clamp(Math.round(next.coverSize), COVER_SIZE_MIN, COVER_SIZE_MAX),
            ambientIntensity: next.ambientIntensity,
            showRailArrows: next.showRailArrows,
            showHomeStats: next.showHomeStats,
            showSpectrum: next.showSpectrum,
            effects: next.effects,
          };
        });
      },

      resetAppearance: () => {
        set({ ...DEFAULT_APPEARANCE });
      },
    }),
    {
      name: "prism.shell.preferences",
      version: 5,
      storage: createJSONStorage(() => localStorage),
      partialize: (
        state,
      ): AppearanceState & { sidebarCollapsed: boolean; sidebarHidden: boolean } => ({
        sidebarCollapsed: state.sidebarCollapsed,
        sidebarHidden: state.sidebarHidden,
        theme: state.theme,
        accentSource: state.accentSource,
        accentColor: state.accentColor,
        glassOpacity: state.glassOpacity,
        tintStrength: state.tintStrength,
        corners: state.corners,
        density: state.density,
        coverSize: state.coverSize,
        ambientIntensity: state.ambientIntensity,
        showRailArrows: state.showRailArrows,
        showHomeStats: state.showHomeStats,
        showSpectrum: state.showSpectrum,
        effects: state.effects,
      }),
      // v1 stored `activeSection`; v2 introduced the router; v3 split the
      // appearance knobs out of the ambient switch and added the mosaic size;
      // v4 lets the compositor budget be chosen or measured; v5 adds the
      // light/dark/system preference while retaining dark as the default.
      migrate: (persisted, version) => {
        if (version < 2) {
          return { ...DEFAULT_APPEARANCE, sidebarCollapsed: false };
        }
        if (version < 5) {
          return { ...DEFAULT_APPEARANCE, ...(persisted as object) };
        }
        return persisted;
      },
    },
  ),
);

function routeParams(params?: Partial<RouteParams>): RouteParams {
  return {
    albumId: params?.albumId ?? null,
    artistId: params?.artistId ?? null,
    collection: params?.collection ?? null,
    genre: params?.genre ?? null,
    playlistId: params?.playlistId ?? null,
  };
}

function locationKey(route: ShellRoute, params: RouteParams): string {
  return `${route}:${params.albumId ?? ""}:${params.artistId ?? ""}:${params.collection ?? ""}:${params.genre ?? ""}:${params.playlistId ?? ""}`;
}

/** Navigate forward while remembering exactly where the current view was. */
function go(state: UiState, route: ShellRoute, params: RouteParams): Partial<UiState> {
  if (locationKey(state.route, state.params) === locationKey(route, params)) {
    return {
      route,
      params,
      scrollRestore: null,
      lyricsOpen: route === "now-playing" ? false : state.lyricsOpen,
    };
  }
  const scroller =
    typeof document === "undefined"
      ? null
      : document.querySelector<HTMLElement>("[data-prism-scroll]");
  const current: NavigationEntry = {
    route: state.route,
    params: state.params,
    scrollTop: scroller?.scrollTop ?? 0,
  };
  return {
    route,
    params,
    history: [...state.history, current].slice(-50),
    scrollRestore: null,
    lyricsOpen: route === "now-playing" ? false : state.lyricsOpen,
  };
}

/** Leaving Library drops transient search/filter state, never the saved sort. */
function clearLibraryTransientState(current: ShellRoute, next: ShellRoute): void {
  if (current === "library" && next !== "library") {
    useLibraryStore.getState().setQuery("");
    useBrowseStore.getState().clearFilters();
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
