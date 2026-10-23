import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

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

  /* ------------------------------------------------------------ appearance */
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
  navigate: (route: ShellRoute, params?: Partial<RouteParams>) => void;
  openAlbum: (albumId: string) => void;
  openArtist: (artistId: string) => void;
  openCollection: (collection: AlbumCollection | null, genre?: string) => void;
  openPlaylist: (playlistId: string) => void;
  setRoute: (route: ShellRoute) => void;
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
  showSpectrum: true,
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
      route: "home",
      params: { ...NO_PARAMS },
      paletteKey: null,
      measuredEffects: null,
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

      navigate: (route, params) => {
        set((state) => ({
          route,
          params: {
            albumId: params?.albumId ?? null,
            artistId: params?.artistId ?? null,
            collection: params?.collection ?? null,
            genre: params?.genre ?? null,
            playlistId: params?.playlistId ?? null,
          },
          ...(route === "home" ? { paletteKey: state.paletteKey } : {}),
        }));
      },

      openAlbum: (albumId) => {
        set({ route: "album", params: { ...NO_PARAMS, albumId } });
      },

      openArtist: (artistId) => {
        set({ route: "artist", params: { ...NO_PARAMS, artistId } });
      },

      openCollection: (collection, genre) => {
        set({
          route: "collection",
          params: { ...NO_PARAMS, collection, genre: genre ?? null },
        });
      },

      openPlaylist: (playlistId) => {
        set({ route: "playlist", params: { ...NO_PARAMS, playlistId } });
      },

      setRoute: (route) => {
        set({ route, params: { ...NO_PARAMS } });
      },

      back: () => {
        // One level only: detail pages go back to the browse surface that owns
        // them. A full history stack would be a router, and this is not one.
        const route = get().route;
        if (route === "album" || route === "artist") {
          set({ route: "library", params: { ...NO_PARAMS } });
        } else if (route === "collection") {
          set({ route: "home", params: { ...NO_PARAMS } });
        } else if (route === "playlist") {
          set({ route: "playlists", params: { ...NO_PARAMS } });
        }
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
      version: 4,
      storage: createJSONStorage(() => localStorage),
      partialize: (state): AppearanceState & { sidebarCollapsed: boolean } => ({
        sidebarCollapsed: state.sidebarCollapsed,
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
      // v4 lets the compositor budget be chosen or measured.
      migrate: (persisted, version) => {
        if (version < 2) {
          return { ...DEFAULT_APPEARANCE, sidebarCollapsed: false };
        }
        if (version < 4) {
          return { ...DEFAULT_APPEARANCE, ...(persisted as object) };
        }
        return persisted;
      },
    },
  ),
);

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
