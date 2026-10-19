import { z } from "zod";

/**
 * Runtime contracts for every Rust → TypeScript boundary.
 *
 * The Rust structs derive `Serialize` with `rename_all = "camelCase"`; the
 * schemas below mirror them exactly and reject contract drift at the IPC edge
 * instead of letting `undefined` leak into the render tree. Both sides are
 * versioned together: changing a field here without changing the backend (or
 * vice versa) fails loudly and immediately.
 */

export const PrismErrorPayloadSchema = z.object({
  code: z.string().min(1),
  message: z.string().min(1),
});
export type PrismErrorPayload = z.infer<typeof PrismErrorPayloadSchema>;

export const DatabaseHealthSchema = z.object({
  journalMode: z.string().min(1),
  synchronous: z.number().int(),
  pageSize: z.number().int().positive(),
  pageCount: z.number().int().nonnegative(),
  freelistCount: z.number().int().nonnegative(),
  sizeBytes: z.number().int().nonnegative(),
  migrationVersion: z.number().int().nonnegative(),
  appliedMigrations: z.number().int().nonnegative(),
  integrity: z.string().min(1),
  poolSize: z.number().int().nonnegative(),
  poolIdle: z.number().int().nonnegative(),
});
export type DatabaseHealth = z.infer<typeof DatabaseHealthSchema>;

export const RuntimeHealthSchema = z.object({
  status: z.enum(["ok", "degraded"]),
  uptimeMs: z.number().int().nonnegative(),
  database: DatabaseHealthSchema,
});
export type RuntimeHealth = z.infer<typeof RuntimeHealthSchema>;

export const AppInfoSchema = z.object({
  name: z.string().min(1),
  version: z.string().min(1),
  tauriVersion: z.string().min(1),
  targetOs: z.string().min(1),
  targetArch: z.string().min(1),
  buildProfile: z.enum(["debug", "release"]),
  dataDir: z.string().min(1),
  databasePath: z.string().min(1),
  logDir: z.string().min(1),
});
export type AppInfo = z.infer<typeof AppInfoSchema>;

export const LibraryStatsSchema = z.object({
  trackCount: z.number().int().nonnegative(),
  albumCount: z.number().int().nonnegative(),
  artistCount: z.number().int().nonnegative(),
  totalDurationMs: z.number().int().nonnegative(),
  totalBytes: z.number().int().nonnegative(),
  missingCount: z.number().int().nonnegative(),
  unhealthyCount: z.number().int().nonnegative(),
  cueTrackCount: z.number().int().nonnegative(),
  maxSampleRate: z.number().int().positive().nullable(),
  maxBitDepth: z.number().int().positive().nullable(),
});
export type LibraryStats = z.infer<typeof LibraryStatsSchema>;

/** Fully validated boot snapshot consumed by the overview. */
export interface SystemSnapshot {
  appInfo: AppInfo;
  health: RuntimeHealth;
  library: LibraryStats;
  capturedAt: Date;
}
