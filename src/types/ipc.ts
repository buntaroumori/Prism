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

/* ------------------------------------------------------------------------- */
/* FASE 3 — library, search, scanning, artwork, rich presence                */
/* ------------------------------------------------------------------------- */

/** Sort orders accepted by `library_search` / `library_albums`. */
export const TrackSortSchema = z.enum([
  "relevance",
  "title",
  "artist",
  "album",
  "year",
  "added",
  "duration",
  "bitrate",
]);
export type TrackSort = z.infer<typeof TrackSortSchema>;

/** Orderings accepted by `library_albums` (mirrors Rust `AlbumSort`). */
export const AlbumSortSchema = z.enum([
  "added",
  "addedAsc",
  "year",
  "yearAsc",
  "artist",
  "artistDesc",
  "title",
  "titleDesc",
  "played",
  "tracks",
]);
export type AlbumSort = z.infer<typeof AlbumSortSchema>;

export const AlbumCollectionSchema = z.enum([
  "recentlyAdded",
  "recentlyPlayed",
  "mostPlayed",
  "highResolution",
  "recentReleases",
  "rediscover",
]);
export type AlbumCollection = z.infer<typeof AlbumCollectionSchema>;

/** One row of the virtualized track list. */
export const TrackRowSchema = z.object({
  id: z.string().min(1),
  path: z.string().min(1),
  folder: z.string(),
  filename: z.string(),
  source: z.enum(["file", "cue"]),
  title: z.string().min(1),
  artistId: z.string().nullable(),
  artist: z.string(),
  albumId: z.string().nullable(),
  album: z.string(),
  albumArtist: z.string(),
  composer: z.string(),
  genre: z.string(),
  year: z.number().int().nullable(),
  trackNo: z.number().int().nullable(),
  discNo: z.number().int().nullable(),
  format: z.string().min(1),
  codecProfile: z.string().nullable(),
  bitDepth: z.number().int().nullable(),
  sampleRate: z.number().int().nullable(),
  channels: z.number().int().nullable(),
  bitrateKbps: z.number().int().nullable(),
  dsdRate: z.number().int().nullable(),
  durationMs: z.number().int().nonnegative(),
  quality: z.string().min(1),
  lossless: z.boolean(),
  hasEmbeddedArt: z.boolean(),
  missing: z.boolean(),
  rating: z.number().int().min(0).max(5),
  playCount: z.number().int().nonnegative(),
  addedAt: z.string().min(1),
  cueStartMs: z.number().int().nullable(),
  cueEndMs: z.number().int().nullable(),
  coverId: z.string().nullable(),
  coverPath: z.string().nullable(),
  coverUrl: z.string().nullable(),
  highlight: z.string().nullable(),
});
export type TrackRow = z.infer<typeof TrackRowSchema>;

export const AlbumRowSchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  sortTitle: z.string(),
  year: z.number().int().nullable(),
  releaseDate: z.string().nullable(),
  genre: z.string().nullable(),
  albumArtist: z.string(),
  /** Album artist id, so the name is a link wherever it appears. */
  artistId: z.string().nullable(),
  isCompilation: z.boolean(),
  discCount: z.number().int().nonnegative(),
  trackCount: z.number().int().nonnegative(),
  durationMs: z.number().int().nonnegative(),
  releaseType: z.string().nullable(),
  coverId: z.string().nullable(),
  coverPath: z.string().nullable(),
  coverThumbnailPath: z.string().nullable(),
  coverUrl: z.string().nullable(),
  availableTracks: z.number().int().nonnegative(),
  maxSampleRate: z.number().int().nullable(),
  maxBitDepth: z.number().int().nullable(),
  format: z.string().nullable(),
  quality: z.string().min(1),
});
export type AlbumRow = z.infer<typeof AlbumRowSchema>;

export const ArtistRowSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  sortName: z.string(),
  albumCount: z.number().int().nonnegative(),
  trackCount: z.number().int().nonnegative(),
  totalDurationMs: z.number().int().nonnegative(),
  coverId: z.string().nullable(),
  coverPath: z.string().nullable(),
});
export type ArtistRow = z.infer<typeof ArtistRowSchema>;

/** Page envelope shared by every paged read model. */
export function pageSchema<T extends z.ZodTypeAny>(item: T) {
  return z.object({
    items: z.array(item),
    total: z.number().int().nonnegative(),
    limit: z.number().int().positive(),
    offset: z.number().int().nonnegative(),
  });
}
export interface Page<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

/** Input contract for `library_search` (all fields optional). */
export const TrackQuerySchema = z.object({
  text: z.string().nullable().optional(),
  artistId: z.string().nullable().optional(),
  albumId: z.string().nullable().optional(),
  genre: z.string().nullable().optional(),
  format: z.string().nullable().optional(),
  yearFrom: z.number().int().nullable().optional(),
  yearTo: z.number().int().nullable().optional(),
  minSampleRate: z.number().int().nullable().optional(),
  minBitDepth: z.number().int().nullable().optional(),
  losslessOnly: z.boolean().optional(),
  includeMissing: z.boolean().optional(),
  sort: TrackSortSchema.optional(),
  limit: z.number().int().positive().max(500).optional(),
  offset: z.number().int().nonnegative().optional(),
});
export type TrackQuery = z.infer<typeof TrackQuerySchema>;

export const FacetSchema = z.object({ value: z.string(), count: z.number().int().nonnegative() });
export const FacetsSchema = z.object({
  genres: z.array(FacetSchema),
  formats: z.array(FacetSchema),
  decades: z.array(FacetSchema),
  years: z.array(FacetSchema),
});
export type Facets = z.infer<typeof FacetsSchema>;

/** Every Home rail, built by one backend call. */
export const HomeRailsSchema = z.object({
  recentlyAdded: z.array(AlbumRowSchema),
  recentlyPlayed: z.array(AlbumRowSchema),
  mostPlayed: z.array(AlbumRowSchema),
  highResolution: z.array(AlbumRowSchema),
  recentReleases: z.array(AlbumRowSchema),
  rediscover: z.array(AlbumRowSchema),
  genres: z.array(
    z.object({
      name: z.string().min(1),
      albumCount: z.number().int().nonnegative(),
      albums: z.array(AlbumRowSchema),
    }),
  ),
});
export type HomeRails = z.infer<typeof HomeRailsSchema>;

/** One album page: identity plus its ordered tracks. */
export const AlbumDetailSchema = z.object({
  album: AlbumRowSchema,
  tracks: pageSchema(TrackRowSchema),
});
export type AlbumDetail = z.infer<typeof AlbumDetailSchema>;

/** One artist page: identity plus every album we hold. */
export const ArtistDetailSchema = z.object({
  artist: ArtistRowSchema,
  albums: pageSchema(AlbumRowSchema),
});
export type ArtistDetail = z.infer<typeof ArtistDetailSchema>;

export const PlaylistRowSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  description: z.string().nullable(),
  genre: z.string().nullable(),
  kind: z.enum(["static", "smart"]),
  createdAt: z.string().min(1),
  updatedAt: z.string().min(1),
  trackCount: z.number().int().nonnegative(),
  durationMs: z.number().int().nonnegative(),
  coverId: z.string().nullable(),
  coverPath: z.string().nullable(),
  coverUrl: z.string().nullable(),
});
export type PlaylistRow = z.infer<typeof PlaylistRowSchema>;

export const PlaylistDetailSchema = z.object({
  playlist: PlaylistRowSchema,
  tracks: pageSchema(TrackRowSchema),
});
export type PlaylistDetail = z.infer<typeof PlaylistDetailSchema>;

/** What removing a library root took out of the catalogue. */
export const RootRemovalSchema = z.object({
  removedRoot: z.boolean(),
  tracksRemoved: z.number().int().nonnegative(),
  albumsRemoved: z.number().int().nonnegative(),
  artistsRemoved: z.number().int().nonnegative(),
  cueSheetsRemoved: z.number().int().nonnegative(),
});
export type RootRemoval = z.infer<typeof RootRemovalSchema>;

export const GlobalSearchSchema = z.object({
  query: z.string(),
  matchExpression: z.string().nullable(),
  tracks: pageSchema(TrackRowSchema),
  albums: z.array(AlbumRowSchema),
  artists: z.array(ArtistRowSchema),
  tookMs: z.number().int().nonnegative(),
});
export type GlobalSearch = z.infer<typeof GlobalSearchSchema>;

export const ScanPhaseSchema = z.enum([
  "walking",
  "parsing",
  "writing",
  "finalising",
  "completed",
  "cancelled",
  "failed",
]);
export const ScanProgressSchema = z.object({
  runId: z.string().min(1),
  phase: ScanPhaseSchema,
  filesSeen: z.number().int().nonnegative(),
  filesIndexed: z.number().int().nonnegative(),
  filesSkipped: z.number().int().nonnegative(),
  filesFailed: z.number().int().nonnegative(),
  bytesRead: z.number().int().nonnegative(),
  percent: z.number().min(0).max(1).nullable(),
  currentPath: z.string().nullable(),
});
export type ScanProgress = z.infer<typeof ScanProgressSchema>;

export const ScanOptionsSchema = z.object({
  roots: z.array(z.string().min(1)).optional(),
  full: z.boolean().optional(),
  prune: z.boolean().optional(),
});
export type ScanOptions = z.infer<typeof ScanOptionsSchema>;

export const ScanStartedSchema = z.object({
  accepted: z.boolean(),
  roots: z.number().int().nonnegative(),
  full: z.boolean(),
});

export const ScanRunSchema = z.object({
  id: z.string().min(1),
  status: z.enum(["running", "completed", "failed", "cancelled"]),
  trigger: z.enum(["startup", "manual", "watch", "scheduled", "cli"]),
  filesSeen: z.number().int().nonnegative(),
  filesIndexed: z.number().int().nonnegative(),
  filesSkipped: z.number().int().nonnegative(),
  filesFailed: z.number().int().nonnegative(),
  tracksRemoved: z.number().int().nonnegative(),
  bytesRead: z.number().int().nonnegative(),
  startedAt: z.string().min(1),
  finishedAt: z.string().nullable(),
  durationMs: z.number().int().nonnegative().nullable(),
  errorMessage: z.string().nullable(),
  errors: z.number().int().nonnegative(),
});
export type ScanRun = z.infer<typeof ScanRunSchema>;

export const ScanOutcomeSchema = z.object({
  runId: z.string().min(1),
  status: z.string().min(1),
  filesSeen: z.number().int().nonnegative(),
  filesIndexed: z.number().int().nonnegative(),
  filesSkipped: z.number().int().nonnegative(),
  filesFailed: z.number().int().nonnegative(),
  tracksMissing: z.number().int().nonnegative(),
  tracksPruned: z.number().int().nonnegative(),
  bytesRead: z.number().int().nonnegative(),
  durationMs: z.number().int().nonnegative(),
});
export type ScanOutcome = z.infer<typeof ScanOutcomeSchema>;

export const ScanStatusSchema = z.object({
  running: z.boolean(),
  progress: ScanProgressSchema.nullable(),
  lastRun: ScanRunSchema.nullable(),
});
export type ScanStatus = z.infer<typeof ScanStatusSchema>;

export const ScanErrorSchema = z.object({
  path: z.string().min(1),
  errorCode: z.string().min(1),
  message: z.string(),
  occurredAt: z.string().min(1),
});
export type ScanError = z.infer<typeof ScanErrorSchema>;

export const LibraryRootSchema = z.object({
  id: z.string().min(1),
  path: z.string().min(1),
  name: z.string(),
  enabled: z.boolean(),
  recursive: z.boolean(),
  watch: z.boolean(),
  available: z.boolean(),
  tracks: z.number().int().nonnegative(),
});
export type LibraryRoot = z.infer<typeof LibraryRootSchema>;

export const CoverAssetSchema = z.object({
  key: z.string().length(64),
  provider: z.enum(["embedded", "folder", "itunes", "musicbrainz", "coverartarchive", "unknown"]),
  localPath: z.string().nullable(),
  thumbnailPath: z.string().nullable(),
  remoteUrl: z.string().nullable(),
  width: z.number().int().positive().nullable(),
  height: z.number().int().positive().nullable(),
});
export type CoverAsset = z.infer<typeof CoverAssetSchema>;

export const DiscordStateSchema = z.enum([
  "disabled",
  "connecting",
  "connected",
  "reconnecting",
  "unavailable",
  "stopped",
]);

export const DiscordConfigSchema = z.object({
  enabled: z.boolean(),
  clientId: z.string(),
  showCover: z.boolean(),
  showTimestamps: z.boolean(),
  showQuality: z.boolean(),
  showPaused: z.boolean(),
});
export type DiscordConfig = z.infer<typeof DiscordConfigSchema>;

export const DiscordStatusSchema = z.object({
  enabled: z.boolean(),
  configured: z.boolean(),
  clientId: z.string(),
  state: DiscordStateSchema,
  attempts: z.number().int().nonnegative(),
  lastError: z.string().nullable(),
  lastUpdateAt: z.string().nullable(),
  showCover: z.boolean(),
  showTimestamps: z.boolean(),
  showQuality: z.boolean(),
  showPaused: z.boolean(),
});
export type DiscordStatus = z.infer<typeof DiscordStatusSchema>;

/* ------------------------------------------------------------------------- */
/* FASE 4 — engine transport, devices, palette                               */
/* ------------------------------------------------------------------------- */

/** Physical sample representation, mirroring `audio::format::SampleFormat`. */
export const SampleFormatSchema = z.enum(["s16", "s24", "s24_in32", "s32", "f32", "f64"]);
export type SampleFormat = z.infer<typeof SampleFormatSchema>;

/** One sample rate / channel layout. */
export const AudioFormatSchema = z.object({
  sampleRate: z.number().int().positive(),
  channels: z.number().int().positive(),
  sampleFormat: SampleFormatSchema,
});
export type AudioFormat = z.infer<typeof AudioFormatSchema>;

export const PlaybackStateSchema = z.enum(["stopped", "buffering", "playing", "paused"]);
export type PlaybackState = z.infer<typeof PlaybackStateSchema>;

/** Metadata of the track loaded in the engine. */
export const NowPlayingSchema = z.object({
  trackId: z.string().min(1),
  title: z.string(),
  artist: z.string(),
  album: z.string(),
  path: z.string().min(1),
  durationFrames: z.number().int().nonnegative().nullable(),
  sourceFormat: AudioFormatSchema.nullable(),
});
export type NowPlaying = z.infer<typeof NowPlayingSchema>;

export const BackendKindSchema = z.enum([
  "wasapiExclusive",
  "wasapiShared",
  "asio",
  "alsaHw",
  "pipeWire",
  "cpalShared",
]);
export type BackendKind = z.infer<typeof BackendKindSchema>;

export const OutputModeSchema = z.enum(["auto", "exclusive", "shared", "asio"]);
export type OutputMode = z.infer<typeof OutputModeSchema>;

/**
 * Whether the endpoint follows the record's sample rate.
 *
 * `matchSource` asks the device for the track's own rate — the endpoint
 * reconfigures and nothing is resampled. `fixed` keeps the endpoint at its
 * configured rate and converts every track to it.
 */
export const RatePolicySchema = z.enum(["matchSource", "fixed"]);
export type RatePolicy = z.infer<typeof RatePolicySchema>;

/** Full engine snapshot returned by `player_status`. */
export const PlayerStatusSchema = z.object({
  state: PlaybackStateSchema,
  track: NowPlayingSchema.nullable(),
  positionFrames: z.number().int().nonnegative(),
  durationFrames: z.number().int().nonnegative().nullable(),
  sourceFormat: AudioFormatSchema.nullable(),
  outputFormat: AudioFormatSchema.nullable(),
  backend: BackendKindSchema.nullable(),
  deviceName: z.string().nullable(),
  /** Endpoint configuration as the driver reports it right now. */
  deviceFormat: AudioFormatSchema.nullable(),
  bitPerfect: z.boolean(),
  dop: z.boolean(),
  volumeDb: z.number(),
  muted: z.boolean(),
  latencyFrames: z.number().int().nonnegative(),
  underruns: z.number().int().nonnegative(),
  overruns: z.number().int().nonnegative(),
  queueIndex: z.number().int().nonnegative(),
  queueLen: z.number().int().nonnegative(),
});
export type PlayerStatus = z.infer<typeof PlayerStatusSchema>;

/** Playhead projection pushed on `player://tick`. */
export const PlayerTickSchema = z.object({
  positionMs: z.number().int().nonnegative(),
  durationMs: z.number().int().nonnegative().nullable(),
  playing: z.boolean(),
});
export type PlayerTick = z.infer<typeof PlayerTickSchema>;

/** Queue-aware commands emitted by native operating-system media controls. */
export const SystemMediaCommandSchema = z.enum(["next", "previous"]);
export type SystemMediaCommand = z.infer<typeof SystemMediaCommandSchema>;

/** One output endpoint reported by `player_devices`. */
export const OutputDeviceSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  kind: BackendKindSchema,
  isDefault: z.boolean(),
  channels: z.number().int().positive(),
  sampleRates: z.array(z.number().int().positive()),
  sampleFormats: z.array(SampleFormatSchema),
  supportsExclusive: z.boolean(),
});
export type OutputDevice = z.infer<typeof OutputDeviceSchema>;

/** A track handed to `player_play`. Mirrors `audio::decoder::TrackRequest`. */
export const TrackRequestSchema = z.object({
  trackId: z.string().min(1),
  title: z.string(),
  artist: z.string(),
  album: z.string(),
  path: z.string().min(1),
  startMs: z.number().int().nonnegative().nullable(),
  endMs: z.number().int().nonnegative().nullable(),
  replaygainTrackGainDb: z.number().nullable(),
  replaygainTrackPeak: z.number().nullable(),
  replaygainAlbumGainDb: z.number().nullable(),
  replaygainAlbumPeak: z.number().nullable(),
});
export type TrackRequest = z.infer<typeof TrackRequestSchema>;

/**
 * Engine push events.
 *
 * The Rust enum is internally tagged (`#[serde(tag = "type")]`), so every
 * payload is a plain object with a `type` discriminator — the reducer switches
 * on it exhaustively, and an unknown variant fails closed instead of leaking a
 * half-parsed object into the store.
 */
export const EngineEventSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("stateChanged"), state: PlaybackStateSchema }),
  z.object({
    type: z.literal("trackChanged"),
    track: NowPlayingSchema,
    index: z.number().int().nonnegative(),
  }),
  z.object({
    type: z.literal("deviceChanged"),
    device: z.string(),
    backend: BackendKindSchema,
    format: AudioFormatSchema,
    bit_perfect: z.boolean(),
  }),
  z.object({ type: z.literal("outputError"), message: z.string() }),
  z.object({ type: z.literal("underrun"), total: z.number().int().nonnegative() }),
  z.object({
    type: z.literal("trackSkipped"),
    trackId: z.string().min(1),
    title: z.string(),
    artist: z.string(),
    reason: z.string(),
  }),
]);
export type EngineEvent = z.infer<typeof EngineEventSchema>;

/** One quantised artwork swatch. */
export const SwatchSchema = z.object({
  hex: z.string().length(7),
  population: z.number().min(0).max(1),
  lightness: z.number().min(0).max(1),
});
export type Swatch = z.infer<typeof SwatchSchema>;

/** Palette extracted from (or derived for) the current cover. */
export const CoverPaletteSchema = z.object({
  key: z.string().length(64),
  source: z.enum(["cover", "derived"]),
  background: z.string().length(7),
  surface: z.string().length(7),
  accent: z.string().length(7),
  accentSoft: z.string().length(7),
  highlight: z.string().length(7),
  highlightAlt: z.string().length(7),
  contrast: z.number().min(1),
  averageLightness: z.number().min(0).max(1),
  swatches: z.array(SwatchSchema),
});
export type CoverPalette = z.infer<typeof CoverPaletteSchema>;

/* ------------------------------------------------------------------------- */
/* FASE 5 — lyrics, spectrum, plugins                                        */
/* ------------------------------------------------------------------------- */

/** One timed lyric line, in milliseconds from the start of the track. */
export const LyricLineSchema = z.object({
  timeMs: z.number().int().nonnegative(),
  text: z.string(),
});
export type LyricLine = z.infer<typeof LyricLineSchema>;

/**
 * Resolved lyrics for a track.
 *
 * `provider` is `embedded`, `sidecar`, `lrclib` or `none`. `synced` is true
 * only when line timestamps are usable, which decides between a karaoke panel
 * and a static lyric sheet. `offsetMs` is the source's global shift, already
 * applied. LRCLIB supplies line-level timing; no word-level timing is implied.
 */
export const LyricsDocumentSchema = z.object({
  trackId: z.string().min(1),
  provider: z.enum(["embedded", "sidecar", "lrclib", "none"]),
  synced: z.boolean(),
  language: z.string().nullable(),
  offsetMs: z.number().int(),
  lines: z.array(LyricLineSchema),
  plainText: z.string(),
  sourcePath: z.string().nullable(),
  fetchedAt: z.string().nullable(),
});
export type LyricsDocument = z.infer<typeof LyricsDocumentSchema>;

/** One 31-band frame from `visualiser://frame`. */
export const SpectrumFrameSchema = z.object({
  bands: z.array(z.number().min(0).max(1)),
  level: z.number().min(0).max(1),
  silent: z.boolean(),
  sampleRate: z.number().int().nonnegative(),
});
export type SpectrumFrame = z.infer<typeof SpectrumFrameSchema>;

/** Setting value types a plugin manifest may declare. */
export const PluginSettingTypeSchema = z.enum(["string", "number", "integer", "boolean", "select"]);
export type PluginSettingType = z.infer<typeof PluginSettingTypeSchema>;

/** A plugin setting: schema plus the value currently stored. */
export const PluginSettingSchema = z.object({
  key: z.string().min(1),
  label: z.string().min(1),
  type: PluginSettingTypeSchema,
  default: z.union([z.string(), z.number(), z.boolean()]),
  min: z.number().nullable(),
  max: z.number().nullable(),
  options: z.array(z.string()),
  help: z.string().nullable(),
  value: z.union([z.string(), z.number(), z.boolean()]),
});
export type PluginSetting = z.infer<typeof PluginSettingSchema>;

/** One installed plugin, as the configuration panel sees it. */
export const PluginDescriptorSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  version: z.string().min(1),
  api: z.number().int().nonnegative(),
  description: z.string(),
  author: z.string(),
  homepage: z.string().nullable(),
  runtime: z.enum(["lua", "wasm", "unknown"]),
  capabilities: z.array(z.string()),
  settings: z.array(PluginSettingSchema),
  enabled: z.boolean(),
  loaded: z.boolean(),
  error: z.string().nullable(),
  path: z.string().min(1),
});
export type PluginDescriptor = z.infer<typeof PluginDescriptorSchema>;

/** Plugin list plus which runtimes this binary was built with. */
export const PluginSnapshotSchema = z.object({
  plugins: z.array(PluginDescriptorSchema),
  runtimes: z.object({
    lua: z.boolean(),
    wasm: z.boolean(),
    api: z.number().int().positive(),
  }),
  directory: z.string().min(1),
});
export type PluginSnapshot = z.infer<typeof PluginSnapshotSchema>;

/** A log line or notification produced by a plugin. */
export const PluginMessageSchema = z.object({
  pluginId: z.string().min(1),
  kind: z.enum(["log", "notification"]),
  level: z.enum(["debug", "info", "warn", "error"]),
  message: z.string(),
  topic: z.string().nullable(),
});
export type PluginMessage = z.infer<typeof PluginMessageSchema>;
