import { invoke } from "@tauri-apps/api/core";
import { ZodError, z, type ZodType } from "zod";

import {
  AlbumDetailSchema,
  AlbumRowSchema,
  ArtistDetailSchema,
  HomeRailsSchema,
  RootRemovalSchema,
  AppInfoSchema,
  CoverPaletteSchema,
  EngineEventSchema,
  AudioFormatSchema,
  OutputDeviceSchema,
  PlayerStatusSchema,
  PlayerTickSchema,
  SystemMediaCommandSchema,
  PlaylistDetailSchema,
  PlaylistRowSchema,
  PluginMessageSchema,
  TrackRequestSchema,
  ArtistRowSchema,
  CoverAssetSchema,
  DiscordStatusSchema,
  FacetsSchema,
  GlobalSearchSchema,
  LibraryRootSchema,
  LibraryStatsSchema,
  LyricsDocumentSchema,
  PluginSnapshotSchema,
  PrismErrorPayloadSchema,
  RuntimeHealthSchema,
  ScanErrorSchema,
  ScanRunSchema,
  ScanStartedSchema,
  ScanStatusSchema,
  SpectrumFrameSchema,
  TrackRowSchema,
  pageSchema,
  type AlbumDetail,
  type AlbumRow,
  type AppInfo,
  type ArtistDetail,
  type AudioFormat,
  type ArtistRow,
  type CoverAsset,
  type CoverPalette,
  type EngineEvent,
  type DiscordConfig,
  type DiscordStatus,
  type Facets,
  type GlobalSearch,
  type HomeRails,
  type LibraryRoot,
  type LibraryStats,
  type LyricsDocument,
  type OutputDevice,
  type OutputMode,
  type PlayerStatus,
  type PlayerTick,
  type SystemMediaCommand,
  type PlaylistDetail,
  type PlaylistRow,
  type RatePolicy,
  type PluginMessage,
  type PluginSnapshot,
  type PrismErrorPayload,
  type RuntimeHealth,
  type ScanError,
  type ScanOptions,
  type ScanRun,
  type RootRemoval,
  type ScanStatus,
  type SpectrumFrame,
  type TrackQuery,
  type TrackRequest,
  type TrackRow,
  type AlbumSort,
  type AlbumCollection,
} from "@/types/ipc";

/**
 * Structured failure raised by any backend call.
 *
 * `code` is a stable machine token (`database`, `io`, `invalid_input`, ...)
 * produced by the Rust error type; `message` is safe to render verbatim.
 */
export class IpcFailure extends Error {
  readonly code: string;

  constructor(payload: PrismErrorPayload) {
    super(payload.message);
    this.name = "IpcFailure";
    this.code = payload.code;
  }
}

function toPayload(error: unknown): PrismErrorPayload {
  const structured = PrismErrorPayloadSchema.safeParse(error);
  if (structured.success) {
    return structured.data;
  }
  if (error instanceof Error) {
    return { code: "transport", message: error.message };
  }
  return {
    code: "transport",
    message: "The desktop backend did not return a structured error.",
  };
}

/**
 * Invoke a command and validate its payload.
 *
 * Two distinct failure classes are surfaced:
 *   * `transport` / backend codes — the command ran and failed;
 *   * `contract` — the command succeeded but the payload no longer matches the
 *     TypeScript schema (deploy skew), which is a bug that must be visible.
 */
export async function command<T>(
  name: string,
  schema: ZodType<T>,
  args?: Record<string, unknown>,
): Promise<T> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>(name, args);
  } catch (error) {
    throw new IpcFailure(toPayload(error));
  }

  try {
    return schema.parse(raw);
  } catch (error) {
    if (error instanceof ZodError) {
      const paths = error.issues
        .map((issue) => (issue.path.length > 0 ? issue.path.join(".") : "<root>"))
        .join(", ");
      throw new IpcFailure({
        code: "contract",
        message: `Unexpected payload from '${name}' (invalid: ${paths}).`,
      });
    }
    throw error;
  }
}

/** Invoke a command that returns unit (`null` on the wire). */
export async function commandVoid(name: string): Promise<void> {
  try {
    await invoke(name);
  } catch (error) {
    throw new IpcFailure(toPayload(error));
  }
}

/** Invoke a command with arguments that returns unit (`null` on the wire). */
async function commandVoidWithArgs(name: string, args: Record<string, unknown>): Promise<void> {
  try {
    await invoke(name, args);
  } catch (error) {
    throw new IpcFailure(toPayload(error));
  }
}

/** Typed façade over the backend command surface. */
export const systemApi = {
  getAppInfo: (): Promise<AppInfo> => command("get_app_info", AppInfoSchema),
  getRuntimeHealth: (): Promise<RuntimeHealth> =>
    command("get_runtime_health", RuntimeHealthSchema),
  getLibraryStats: (): Promise<LibraryStats> => command("get_library_stats", LibraryStatsSchema),
  openLogFolder: (): Promise<void> => commandVoid("open_log_folder"),
  openDataFolder: (): Promise<void> => commandVoid("open_data_folder"),
};

export const windowApi = {
  setFullscreen: (fullscreen: boolean): Promise<void> =>
    commandVoidWithArgs("window_set_fullscreen", { fullscreen }),
  isFullscreen: (): Promise<boolean> => command("window_is_fullscreen", z.boolean()),
};

const Z_BOOLEAN = z.boolean();
const Z_NUMBER = z.number().int();
const Z_ROOTS = z.array(LibraryRootSchema);
const Z_SCAN_RUNS = z.array(ScanRunSchema);
const Z_SCAN_ERRORS = z.array(ScanErrorSchema);

const TrackPageSchema = pageSchema(TrackRowSchema);
const AlbumPageSchema = pageSchema(AlbumRowSchema);
const ArtistPageSchema = pageSchema(ArtistRowSchema);

/**
 * Library scan, browse, search and artwork.
 *
 * Every call validates the payload against the schema before it reaches a
 * store, so a backend/frontend version skew surfaces as a `contract` failure
 * instead of `undefined` rendering in the grid.
 */
export const libraryApi = {
  scanStart: (
    options?: ScanOptions,
  ): Promise<{ accepted: boolean; roots: number; full: boolean }> =>
    command("library_scan_start", ScanStartedSchema, { options: options ?? null }),
  scanCancel: (): Promise<boolean> => command("library_scan_cancel", Z_BOOLEAN),
  scanStatus: (): Promise<ScanStatus> => command("library_scan_status", ScanStatusSchema),
  scanRuns: (limit?: number): Promise<ScanRun[]> =>
    command("library_scan_runs", Z_SCAN_RUNS, { limit: limit ?? null }),
  scanErrors: (runId: string, limit?: number): Promise<ScanError[]> =>
    command("library_scan_errors", Z_SCAN_ERRORS, { runId, limit: limit ?? null }),

  roots: (): Promise<LibraryRoot[]> => command("library_roots", Z_ROOTS),
  addRoot: (path: string, name?: string): Promise<LibraryRoot> =>
    command("library_add_root", LibraryRootSchema, { path, name: name ?? null }),
  removeRoot: (id: string): Promise<RootRemoval> =>
    command("library_remove_root", RootRemovalSchema, { id, purge: true }),
  setRootEnabled: (id: string, enabled: boolean): Promise<boolean> =>
    command("library_set_root_enabled", Z_BOOLEAN, { id, enabled }),

  search: (
    query: TrackQuery = {},
  ): Promise<{ items: TrackRow[]; total: number; limit: number; offset: number }> =>
    command("library_search", TrackPageSchema, { query }),
  albums: (options?: {
    text?: string;
    artistId?: string;
    genre?: string;
    yearFrom?: number;
    yearTo?: number;
    collection?: AlbumCollection;
    sort?: AlbumSort;
    limit?: number;
    offset?: number;
  }): Promise<{ items: AlbumRow[]; total: number; limit: number; offset: number }> =>
    command("library_albums", AlbumPageSchema, {
      text: options?.text ?? null,
      artistId: options?.artistId ?? null,
      genre: options?.genre ?? null,
      yearFrom: options?.yearFrom ?? null,
      yearTo: options?.yearTo ?? null,
      collection: options?.collection ?? null,
      sort: options?.sort ?? null,
      limit: options?.limit ?? null,
      offset: options?.offset ?? null,
    }),
  artists: (options?: {
    text?: string;
    limit?: number;
    offset?: number;
  }): Promise<{ items: ArtistRow[]; total: number; limit: number; offset: number }> =>
    command("library_artists", ArtistPageSchema, {
      text: options?.text ?? null,
      limit: options?.limit ?? null,
      offset: options?.offset ?? null,
    }),
  facets: (): Promise<Facets> => command("library_facets", FacetsSchema),
  globalSearch: (text: string, limit?: number): Promise<GlobalSearch> =>
    command("library_global_search", GlobalSearchSchema, { text, limit: limit ?? null }),

  home: (limit?: number): Promise<HomeRails> =>
    command("library_home", HomeRailsSchema, { limit: limit ?? null }),
  albumDetail: (id: string): Promise<AlbumDetail> =>
    command("library_album_detail", AlbumDetailSchema, { id }),
  artistDetail: (id: string): Promise<ArtistDetail> =>
    command("library_artist_detail", ArtistDetailSchema, { id }),
  notePlay: (trackId: string, msPlayed?: number, completed?: boolean): Promise<boolean> =>
    command("library_note_play", Z_BOOLEAN, {
      trackId,
      msPlayed: msPlayed ?? null,
      completed: completed ?? null,
    }),
  indexCovers: (batch?: number): Promise<number> =>
    command("library_index_covers", Z_NUMBER, { batch: batch ?? null }),

  coverPalette: (key: string): Promise<CoverPalette> =>
    command("library_cover_palette", CoverPaletteSchema, { key }),

  albumCover: (albumId: string): Promise<CoverAsset> =>
    command("library_album_cover", CoverAssetSchema, { albumId }),
  trackCover: (trackId: string): Promise<CoverAsset> =>
    command("library_track_cover", CoverAssetSchema, { trackId }),
};

/** Discord Rich Presence configuration and diagnostics. */
export const discordApi = {
  status: (): Promise<DiscordStatus> => command("discord_status", DiscordStatusSchema),
  configure: (config: DiscordConfig): Promise<DiscordStatus> =>
    command("discord_configure", DiscordStatusSchema, { config }),
  clear: (): Promise<boolean> => command("discord_clear", Z_BOOLEAN),
  artworkOverridesPath: (): Promise<string> =>
    command("discord_artwork_overrides_path", z.string().min(1)),
  openArtworkFolder: (): Promise<void> => commandVoid("discord_open_artwork_folder"),
};

const Z_PLAYLISTS = z.array(PlaylistRowSchema);

export const playlistApi = {
  list: (): Promise<PlaylistRow[]> => command("playlist_list", Z_PLAYLISTS),
  create: (name: string): Promise<PlaylistRow> =>
    command("playlist_create", PlaylistRowSchema, { name }),
  update: (
    id: string,
    metadata: { name: string; description: string | null; genre: string | null },
  ): Promise<PlaylistRow> => command("playlist_update", PlaylistRowSchema, { id, ...metadata }),
  detail: (id: string, limit?: number, offset?: number): Promise<PlaylistDetail> =>
    command("playlist_detail", PlaylistDetailSchema, {
      id,
      limit: limit ?? null,
      offset: offset ?? null,
    }),
  addTracks: (playlistId: string, trackIds: string[]): Promise<PlaylistRow> =>
    command("playlist_add_tracks", PlaylistRowSchema, { playlistId, trackIds }),
  removeTrack: (playlistId: string, trackId: string): Promise<PlaylistRow> =>
    command("playlist_remove_track", PlaylistRowSchema, { playlistId, trackId }),
  setCustomCover: (playlistId: string, path: string): Promise<PlaylistRow> =>
    command("playlist_set_custom_cover", PlaylistRowSchema, { playlistId, path }),
  delete: (id: string): Promise<boolean> => command("playlist_delete", Z_BOOLEAN, { id }),
};

/**
 * Transport and output control.
 *
 * Every call is a thin pass-through to the engine supervisor: the audio thread
 * owns the device, so the promise resolves as soon as the command is queued and
 * the resulting state arrives on `player://event`.
 */
export const playerApi = {
  status: (): Promise<PlayerStatus> => command("player_status", PlayerStatusSchema),
  play: (tracks: TrackRequest[], index: number): Promise<void> =>
    commandVoidWithArgs("player_play", {
      tracks: tracks.map((track) => TrackRequestSchema.parse(track)),
      index,
    }),
  /** Replace upcoming tracks without reloading the current decoder or output. */
  updateQueue: (tracks: TrackRequest[], currentIndex: number): Promise<void> =>
    commandVoidWithArgs("player_update_queue", {
      tracks: tracks.map((track) => TrackRequestSchema.parse(track)),
      index: currentIndex,
    }),
  pause: (): Promise<void> => commandVoid("player_pause"),
  resume: (): Promise<void> => commandVoid("player_resume"),
  stop: (): Promise<void> => commandVoid("player_stop"),
  /** Seek by position in milliseconds; the engine converts to frames. */
  seek: (positionMs: number): Promise<void> =>
    commandVoidWithArgs("player_seek", { positionMs: Math.max(0, Math.round(positionMs)) }),
  setVolumeDb: (volumeDb: number): Promise<void> =>
    commandVoidWithArgs("player_set_volume", { volumeDb }),
  setMuted: (muted: boolean): Promise<void> => commandVoidWithArgs("player_set_muted", { muted }),
  setOutput: (mode: OutputMode, device: string | null, bufferFrames: number): Promise<void> =>
    commandVoidWithArgs("player_set_output", { mode, device, bufferFrames }),
  setRatePolicy: (policy: RatePolicy): Promise<void> =>
    commandVoidWithArgs("player_set_rate_policy", { policy }),
  setRamPlay: (enabled: boolean): Promise<void> =>
    commandVoidWithArgs("player_set_ram_play", { enabled }),
  devices: (mode: OutputMode): Promise<OutputDevice[]> =>
    command("player_devices", z.array(OutputDeviceSchema), { mode }),
  deviceFormat: (mode: OutputMode, device: string | null): Promise<AudioFormat | null> =>
    command("player_device_format", AudioFormatSchema.nullable(), { mode, device }),
};

/**
 * Lyrics for the now-playing panel.
 *
 * `refresh` bypasses the backend cache so a `.lrc` dropped next to the file is
 * picked up without restarting the player.
 */
export const lyricsApi = {
  get: (trackId: string, refresh = false): Promise<LyricsDocument> =>
    command("lyrics_get", LyricsDocumentSchema, { trackId, refresh }),
  clear: (trackId: string): Promise<boolean> => command("lyrics_clear", z.boolean(), { trackId }),
};

/**
 * Spectrum analyser control.
 *
 * `setEnabled` is reference-counted by the frontend: the audio thread only
 * copies samples while at least one visualiser is mounted.
 */
export const visualiserApi = {
  setEnabled: (enabled: boolean): Promise<boolean> =>
    command("visualiser_set_enabled", z.boolean(), { enabled }),
  frame: (): Promise<SpectrumFrame | null> =>
    command("visualiser_frame", SpectrumFrameSchema.nullable()),
};

/** Plugin discovery, enabling and configuration. */
export const pluginsApi = {
  list: (): Promise<PluginSnapshot> => command("plugins_list", PluginSnapshotSchema),
  reload: (): Promise<PluginSnapshot> => command("plugins_reload", PluginSnapshotSchema),
  setEnabled: (id: string, enabled: boolean): Promise<PluginSnapshot> =>
    command("plugins_set_enabled", PluginSnapshotSchema, { id, enabled }),
  setConfig: (id: string, key: string, value: string | number | boolean): Promise<PluginSnapshot> =>
    command("plugins_set_config", PluginSnapshotSchema, { id, key, value }),
  openFolder: (): Promise<void> => commandVoid("plugins_open_folder"),
};

/** Event channel names, mirrored from the Rust constants. */
export const EVENT_CHANNELS = {
  scanProgress: "library://scan-progress",
  scanComplete: "library://scan-complete",
  playerEvent: "player://event",
  playerTick: "player://tick",
  mediaCommand: "media://command",
  visualiserFrame: "visualiser://frame",
  pluginMessage: "plugin://message",
} as const;

/** Parse an engine push event, returning `null` instead of throwing. */
export function parseEngineEvent(payload: unknown): EngineEvent | null {
  const parsed = EngineEventSchema.safeParse(payload);
  return parsed.success ? parsed.data : null;
}

/** Parse a playhead tick. */
export function parsePlayerTick(payload: unknown): PlayerTick | null {
  const parsed = PlayerTickSchema.safeParse(payload);
  return parsed.success ? parsed.data : null;
}

/** Parse a native operating-system media command. */
export function parseSystemMediaCommand(payload: unknown): SystemMediaCommand | null {
  const parsed = SystemMediaCommandSchema.safeParse(payload);
  return parsed.success ? parsed.data : null;
}

/** Parse a spectrum frame, returning `null` on contract drift. */
export function parseSpectrumFrame(payload: unknown): SpectrumFrame | null {
  const parsed = SpectrumFrameSchema.safeParse(payload);
  return parsed.success ? parsed.data : null;
}

/** Parse a plugin message, returning `null` on contract drift. */
export function parsePluginMessage(payload: unknown): PluginMessage | null {
  const parsed = PluginMessageSchema.safeParse(payload);
  return parsed.success ? parsed.data : null;
}
