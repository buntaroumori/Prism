import { invoke } from "@tauri-apps/api/core";
import { ZodError, z, type ZodType } from "zod";

import {
  AlbumRowSchema,
  AppInfoSchema,
  ArtistRowSchema,
  CoverAssetSchema,
  DiscordStatusSchema,
  FacetsSchema,
  GlobalSearchSchema,
  LibraryRootSchema,
  LibraryStatsSchema,
  PrismErrorPayloadSchema,
  RuntimeHealthSchema,
  ScanErrorSchema,
  ScanRunSchema,
  ScanStartedSchema,
  ScanStatusSchema,
  TrackRowSchema,
  pageSchema,
  type AlbumRow,
  type AppInfo,
  type ArtistRow,
  type CoverAsset,
  type DiscordConfig,
  type DiscordStatus,
  type Facets,
  type GlobalSearch,
  type LibraryRoot,
  type LibraryStats,
  type PrismErrorPayload,
  type RuntimeHealth,
  type ScanError,
  type ScanOptions,
  type ScanRun,
  type ScanStatus,
  type TrackQuery,
  type TrackRow,
  type TrackSort,
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

/** Typed façade over the backend command surface. */
export const systemApi = {
  getAppInfo: (): Promise<AppInfo> => command("get_app_info", AppInfoSchema),
  getRuntimeHealth: (): Promise<RuntimeHealth> =>
    command("get_runtime_health", RuntimeHealthSchema),
  getLibraryStats: (): Promise<LibraryStats> => command("get_library_stats", LibraryStatsSchema),
  openLogFolder: (): Promise<void> => commandVoid("open_log_folder"),
  openDataFolder: (): Promise<void> => commandVoid("open_data_folder"),
};

const Z_BOOLEAN = z.boolean();
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
  removeRoot: (id: string): Promise<boolean> => command("library_remove_root", Z_BOOLEAN, { id }),
  setRootEnabled: (id: string, enabled: boolean): Promise<boolean> =>
    command("library_set_root_enabled", Z_BOOLEAN, { id, enabled }),

  search: (
    query: TrackQuery = {},
  ): Promise<{ items: TrackRow[]; total: number; limit: number; offset: number }> =>
    command("library_search", TrackPageSchema, { query }),
  albums: (options?: {
    text?: string;
    artistId?: string;
    sort?: TrackSort;
    limit?: number;
    offset?: number;
  }): Promise<{ items: AlbumRow[]; total: number; limit: number; offset: number }> =>
    command("library_albums", AlbumPageSchema, {
      text: options?.text ?? null,
      artistId: options?.artistId ?? null,
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
};

/** Event channel names, mirrored from the Rust constants. */
export const EVENT_CHANNELS = {
  scanProgress: "library://scan-progress",
  scanComplete: "library://scan-complete",
  playerEvent: "player://event",
} as const;
