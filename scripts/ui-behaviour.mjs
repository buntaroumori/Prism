#!/usr/bin/env node
/**
 * Behaviour test — the interactions the smoke test cannot see.
 *
 * `smoke-ui.mjs` answers "does the bundle mount"; this answers "does the
 * product work". It serves the production bundle under the real CSP, feeds the
 * shell a small but *complete* catalogue through a stubbed IPC, and then drives
 * the window with a pointer and a keyboard exactly as a person would:
 *
 *   * every album cover and artist name on Home navigates;
 *   * the seek bar advances with the engine clock and follows a drag;
 *   * the volume rail follows a drag and the mute button toggles;
 *   * the player bar contains no output-device picker.
 *
 * Each assertion names the defect it guards, because these are regressions
 * reported from the field, not hypotheticals. Exit codes: 0 pass · 1 fail ·
 * 2 tooling.
 *
 * Usage:
 *   node scripts/ui-behaviour.mjs            # build, serve, drive
 *   node scripts/ui-behaviour.mjs --no-build # reuse dist/
 */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const DIST = join(ROOT, "dist");
const CONFIG = JSON.parse(
  execFileSync(
    "node",
    ["-e", "process.stdout.write(JSON.stringify(require('./src-tauri/tauri.conf.json')))"],
    { cwd: ROOT, encoding: "utf8" },
  ),
);
const args = new Set(process.argv.slice(2));

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".webp": "image/webp",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ico": "image/x-icon",
};

async function loadPlaywright() {
  const candidates = [
    () => createRequire(join(ROOT, "package.json"))("playwright"),
    () => import(join(ROOT, "node_modules", "playwright", "index.mjs")),
  ];
  for (const candidate of candidates) {
    try {
      return await candidate();
    } catch {
      // try the next one
    }
  }
  console.error("ui-behaviour: Playwright is not installed — run `pnpm install` first");
  process.exit(2);
}

function buildIfNeeded() {
  if (args.has("--no-build")) {
    if (!existsSync(join(DIST, "index.html"))) {
      console.error("ui-behaviour: dist/index.html is missing — run without --no-build");
      process.exit(2);
    }
    return;
  }
  execFileSync("node", ["node_modules/vite/bin/vite.js", "build"], {
    cwd: ROOT,
    stdio: "inherit",
    env: { ...process.env, TAURI_ENV_PLATFORM: process.env.TAURI_ENV_PLATFORM ?? "linux" },
  });
}

// ---------------------------------------------------------------------------
// Fixture catalogue
// ---------------------------------------------------------------------------

/**
 * The stub backend, as source.
 *
 * It is embedded with `toString()` and evaluated in the page, so the handlers
 * may be real functions (JSON would silently drop them, and every detail
 * command would look "unstubbed" — a harness bug that reads exactly like a
 * product bug).
 */
function backendFixture() {
  // `?noduration` reproduces the field report: the engine opens the file but
  // the container declares no length, so `durationFrames` is null all the way
  // through the snapshot and the tick. The catalogue still knows the length.
  const noDuration = window.location.search.includes("noduration");
  const synchronizedLyrics = window.location.search.includes("lyrics");
  const albums = [
    {
      id: "al-one",
      title: "Kid A",
      artistId: "ar-one",
      albumArtist: "Radiohead",
      year: 2000,
      quality: "16/44.1 · FLAC",
      trackCount: 2,
      durationMs: 480000,
    },
    {
      id: "al-two",
      title: "Lemonade",
      artistId: "ar-two",
      albumArtist: "Beyonce",
      year: 2016,
      quality: "24/96 · FLAC",
      trackCount: 2,
      durationMs: 360000,
    },
  ];

  const albumRow = (album) => ({
    id: album.id,
    title: album.title,
    sortTitle: album.title,
    year: album.year,
    releaseDate: album.year === null ? null : `${String(album.year)}-10-01`,
    genre: "Electronic",
    albumArtist: album.albumArtist,
    artistId: album.artistId,
    isCompilation: false,
    discCount: album.id === "al-two" ? 2 : 1,
    trackCount: album.trackCount,
    durationMs: album.durationMs,
    releaseType: "album",
    coverId: null,
    coverPath: null,
    coverThumbnailPath: null,
    coverUrl: null,
    availableTracks: album.trackCount,
    maxSampleRate: album.id === "al-two" ? 96000 : 44100,
    maxBitDepth: album.id === "al-two" ? 24 : 16,
    format: "flac",
    quality: album.quality,
  });

  const trackRow = (album, index) => ({
    id: album.id + "-t" + String(index),
    path: "/music/" + album.albumArtist + "/" + album.title + "/" + String(index) + ".flac",
    folder: "/music/" + album.albumArtist + "/" + album.title,
    filename: String(index) + ".flac",
    source: "file",
    title: album.title + " track " + String(index),
    artistId: album.artistId,
    artist: album.albumArtist,
    albumId: album.id,
    album: album.title,
    albumArtist: album.albumArtist,
    composer: "",
    genre: "Electronic",
    year: album.year,
    trackNo: index,
    discNo: album.id === "al-two" ? index : 1,
    format: "flac",
    codecProfile: null,
    bitDepth: album.id === "al-two" ? 24 : 16,
    sampleRate: album.id === "al-two" ? 96000 : 44100,
    channels: 2,
    bitrateKbps: 900,
    dsdRate: null,
    durationMs: 240000,
    quality: "16/44.1 · FLAC",
    lossless: true,
    hasEmbeddedArt: false,
    missing: false,
    rating: 0,
    playCount: 0,
    addedAt: "2026-10-01T10:00:00.000Z",
    cueStartMs: null,
    cueEndMs: null,
    coverId: null,
    coverPath: null,
    coverUrl: null,
    highlight: null,
  });

  const artistRow = (album) => ({
    id: album.artistId,
    name: album.albumArtist,
    sortName: album.albumArtist,
    albumCount: 1,
    trackCount: album.trackCount,
    totalDurationMs: album.durationMs,
    coverId: null,
    coverPath: null,
  });

  const page = (items) => ({ items, total: items.length, limit: 500, offset: 0 });

  /** The engine snapshot the shell hydrates from, and the one it re-reads. */
  const status = () => ({
    state: "playing",
    track: {
      trackId: "al-one-t1",
      title: "Kid A track 1",
      artist: "Radiohead",
      album: "Kid A",
      path: "/music/Radiohead/Kid A/1.flac",
      durationFrames: noDuration ? null : 10584000,
      sourceFormat: { sampleRate: 44100, channels: 2, sampleFormat: "s16" },
    },
    positionFrames: 0,
    durationFrames: noDuration ? null : 10584000,
    sourceFormat: { sampleRate: 44100, channels: 2, sampleFormat: "s16" },
    outputFormat: { sampleRate: 44100, channels: 2, sampleFormat: "f32" },
    backend: "wasapiShared",
    deviceName: "Sennheiser HD 600",
    deviceFormat: { sampleRate: 192000, channels: 2, sampleFormat: "s24" },
    bitPerfect: false,
    dop: false,
    volumeDb: 0,
    muted: false,
    latencyFrames: 512,
    underruns: 0,
    overruns: 0,
    queueIndex: 0,
    queueLen: 2,
  });

  /** Mutable engine state, so a command is visible in the next snapshot. */
  const live = {
    volumeDb: 0,
    muted: false,
    positionFrames: 0,
    state: "playing",
    fullscreen: false,
    maximized: false,
    hasPlayed: false,
    track: null,
    queueIndex: 0,
    queueLen: 2,
  };
  const playlists = [];
  const playlistItems = new Map();
  const playlistRow = (playlist) => ({
    ...playlist,
    genre: playlist.genre ?? null,
    trackCount: (playlistItems.get(playlist.id) ?? []).length,
    durationMs: (playlistItems.get(playlist.id) ?? []).length * 240000,
  });
  const snapshot = () => ({
    ...status(),
    state: live.state,
    track: live.hasPlayed ? live.track : status().track,
    positionFrames: live.positionFrames,
    volumeDb: live.volumeDb,
    muted: live.muted,
    queueIndex: live.queueIndex,
    queueLen: live.queueLen,
  });

  return {
    get_app_info: {
      name: "Prism",
      version: "1.2.0",
      tauriVersion: "2.11.6",
      targetOs: "linux",
      targetArch: "x86_64",
      buildProfile: "release",
      dataDir: "/tmp/prism",
      databasePath: "/tmp/prism/prism.db",
      logDir: "/tmp/prism/logs",
    },
    get_library_stats: {
      trackCount: 4,
      albumCount: 2,
      artistCount: 2,
      totalDurationMs: 840000,
      totalBytes: 120000000,
      missingCount: 0,
      unhealthyCount: 0,
      cueTrackCount: 0,
      maxSampleRate: 96000,
      maxBitDepth: 24,
    },
    library_roots: [
      {
        id: "root-1",
        path: "/music",
        name: "Music",
        enabled: true,
        recursive: true,
        watch: true,
        available: true,
        tracks: 4,
      },
    ],
    library_scan_status: { running: false, progress: null, lastRun: null },
    library_scan_runs: [],
    library_scan_errors: [],
    library_home: {
      recentlyAdded: albums.map(albumRow),
      recentlyPlayed: albums.map(albumRow),
      mostPlayed: albums.map(albumRow),
      highResolution: [albumRow(albums[1])],
      recentReleases: albums.map(albumRow),
      rediscover: albums.map(albumRow),
      genres: [{ name: "Electronic", albumCount: 2, albums: albums.map(albumRow) }],
    },
    library_albums: () => page(albums.map(albumRow)),
    library_artists: page(albums.map(artistRow)),
    library_album_detail: (args) => {
      const album = albums.find((candidate) => candidate.id === args.id) || albums[0];
      return { album: albumRow(album), tracks: page([1, 2].map((i) => trackRow(album, i))) };
    },
    library_artist_detail: (args) => {
      const album = albums.find((candidate) => candidate.artistId === args.id) || albums[0];
      return { artist: artistRow(album), albums: page([albumRow(album)]) };
    },
    library_search: (args) => {
      const query = args.query ?? {};
      if (query.albumId) {
        const album = albums.find((candidate) => candidate.id === query.albumId) || albums[0];
        return page([1, 2].map((index) => trackRow(album, index)));
      }
      return page([]);
    },
    library_global_search: {
      query: "",
      matchExpression: null,
      tracks: page([]),
      albums: [],
      artists: [],
      tookMs: 1,
    },
    library_facets: {
      genres: [
        { value: "Electronic", count: 2 },
        ...Array.from({ length: 18 }, (_, index) => ({
          value: `Fixture genre ${String(index + 1).padStart(2, "0")}`,
          count: 0,
        })),
      ],
      formats: [],
      decades: [
        { value: "2000s", count: 1 },
        { value: "2010s", count: 1 },
        ...Array.from({ length: 14 }, (_, index) => ({
          value: `${String(2020 + index * 10)}s`,
          count: 0,
        })),
      ],
      sampleRates: [],
      bitDepths: [],
      years: [
        { value: "2000", count: 1 },
        { value: "2016", count: 1 },
      ],
    },
    library_track_cover: {
      key: "a".repeat(64),
      provider: "embedded",
      localPath: null,
      thumbnailPath: null,
      remoteUrl: null,
      width: null,
      height: null,
    },
    library_album_cover: {
      key: "a".repeat(64),
      provider: "embedded",
      localPath: null,
      thumbnailPath: null,
      remoteUrl: null,
      width: null,
      height: null,
    },
    library_cover_palette: {
      key: "a".repeat(64),
      source: "derived",
      background: "#08080a",
      surface: "#123456",
      accent: "#7dd3fc",
      accentSoft: "#a7b2ff",
      highlight: "#ff6b9a",
      highlightAlt: "#6bd5ff",
      contrast: 4.5,
      averageLightness: 0.4,
      swatches: [],
    },
    library_note_play: true,
    playlist_list: () => playlists.map(playlistRow),
    playlist_create: (args) => {
      const playlist = {
        id: "pl-" + String(playlists.length + 1),
        name: String(args.name).trim(),
        description: null,
        kind: "static",
        createdAt: "2026-10-22T10:00:00.000Z",
        updatedAt: "2026-10-22T10:00:00.000Z",
        coverId: null,
        coverPath: null,
        coverUrl: null,
      };
      playlists.push(playlist);
      playlistItems.set(playlist.id, []);
      return playlistRow(playlist);
    },
    playlist_update: (args) => {
      const playlist = playlists.find((candidate) => candidate.id === args.id);
      if (!playlist) throw new Error("unknown playlist");
      playlist.name = String(args.name).trim();
      playlist.description = args.description ?? null;
      playlist.genre = args.genre ?? null;
      playlist.updatedAt = "2026-10-22T10:00:30.000Z";
      return playlistRow(playlist);
    },
    playlist_add_tracks: (args) => {
      const playlist = playlists.find((candidate) => candidate.id === args.playlistId);
      const items = playlistItems.get(args.playlistId) ?? [];
      for (const id of args.trackIds) if (!items.includes(id)) items.push(id);
      playlistItems.set(args.playlistId, items);
      playlist.updatedAt = "2026-10-22T10:01:00.000Z";
      return playlistRow(playlist);
    },
    playlist_set_custom_cover: (args) => {
      const playlist = playlists.find((candidate) => candidate.id === args.playlistId);
      playlist.coverPath = "/tmp/prism/playlist-covers/" + args.playlistId + ".jpg";
      playlist.coverUrl = null;
      playlist.updatedAt = "2026-10-22T10:01:30.000Z";
      return playlistRow(playlist);
    },
    playlist_detail: (args) => {
      const playlist = playlists.find((candidate) => candidate.id === args.id);
      const ids = playlistItems.get(args.id) ?? [];
      const tracks = albums.flatMap((album) => [trackRow(album, 1), trackRow(album, 2)]);
      return {
        playlist: playlistRow(playlist),
        tracks: page(tracks.filter((track) => ids.includes(track.id))),
      };
    },
    playlist_remove_track: (args) => {
      const playlist = playlists.find((candidate) => candidate.id === args.playlistId);
      playlistItems.set(
        args.playlistId,
        (playlistItems.get(args.playlistId) ?? []).filter((id) => id !== args.trackId),
      );
      playlist.updatedAt = "2026-10-22T10:02:00.000Z";
      return playlistRow(playlist);
    },
    playlist_delete: (args) => {
      const index = playlists.findIndex((candidate) => candidate.id === args.id);
      if (index >= 0) playlists.splice(index, 1);
      playlistItems.delete(args.id);
      return index >= 0;
    },
    "plugin:window|start_dragging": () => null,
    window_set_fullscreen: (args) => {
      live.fullscreen = Boolean(args.fullscreen);
      return null;
    },
    window_is_fullscreen: () => live.fullscreen,
    player_status: () => snapshot(),
    player_devices: [
      {
        id: "wasapiShared:Sennheiser HD 600",
        name: "Sennheiser HD 600",
        kind: "wasapiShared",
        isDefault: true,
        channels: 2,
        sampleRates: [44100, 48000, 96000, 192000],
        sampleFormats: ["s16", "s24", "s32", "f32"],
        supportsExclusive: false,
      },
    ],
    player_device_format: { sampleRate: 192000, channels: 2, sampleFormat: "s24" },
    player_play: (args) => {
      const request = args.tracks[args.index];
      const baseline = status().track;
      live.state = "playing";
      live.positionFrames = 0;
      live.hasPlayed = true;
      live.queueIndex = Number(args.index);
      live.queueLen = args.tracks.length;
      live.track = {
        trackId: request.trackId,
        title: request.title,
        artist: request.artist,
        album: request.album,
        path: request.path,
        durationFrames: noDuration ? null : 10584000,
        sourceFormat: baseline.sourceFormat,
      };
      return null;
    },
    player_update_queue: (args) => {
      live.queueIndex = Number(args.index);
      live.queueLen = args.tracks.length;
      return null;
    },
    player_pause: () => {
      live.state = "paused";
      return null;
    },
    player_resume: () => {
      live.state = "playing";
      return null;
    },
    player_stop: () => {
      live.state = "stopped";
      live.positionFrames = 0;
      live.hasPlayed = true;
      live.track = null;
      live.queueIndex = 0;
      live.queueLen = 0;
      return null;
    },
    player_seek: (args) => {
      live.positionFrames = Math.round(Number(args.positionMs) * 44.1);
      return null;
    },
    player_set_volume: (args) => {
      live.volumeDb = Number(args.volumeDb);
      return null;
    },
    player_set_muted: (args) => {
      live.muted = Boolean(args.muted);
      return null;
    },
    player_set_dsp: null,
    player_set_output: null,
    player_set_rate_policy: null,
    player_set_ram_play: null,
    lyrics_get: (args) => ({
      trackId: args.trackId,
      provider: synchronizedLyrics ? "lrclib" : "none",
      synced: synchronizedLyrics,
      language: synchronizedLyrics ? "en" : null,
      offsetMs: 0,
      lines: synchronizedLyrics
        ? [
            { timeMs: 0, text: "First synchronized line" },
            { timeMs: 5000, text: "Second synchronized line" },
            { timeMs: 10000, text: "Third synchronized line" },
          ]
        : [],
      plainText: synchronizedLyrics
        ? "First synchronized line\nSecond synchronized line\nThird synchronized line"
        : "",
      sourcePath: synchronizedLyrics ? "https://lrclib.net/api/get/42" : null,
      fetchedAt: synchronizedLyrics ? "2026-10-25T10:00:00.000Z" : null,
    }),
    discord_artwork_overrides_path: "/tmp/prism/discord-artwork.json",
    discord_status: () => ({
      enabled: false,
      configured: false,
      clientId: "",
      state: "disabled",
      attempts: 0,
      lastError: null,
      lastUpdateAt: null,
      showCover: true,
      showTimestamps: true,
      showQuality: true,
      showPaused: true,
    }),
    discord_configure: (args) => ({
      enabled: Boolean(args.config.enabled),
      configured: Boolean(args.config.enabled),
      clientId: args.config.clientId,
      state: args.config.enabled ? "connected" : "disabled",
      attempts: 0,
      lastError: null,
      lastUpdateAt: null,
      showCover: args.config.showCover,
      showTimestamps: args.config.showTimestamps,
      showQuality: args.config.showQuality,
      showPaused: args.config.showPaused,
    }),
    "plugin:dialog|open": (args) =>
      args.options?.title === "Choose playlist cover" ? "/tmp/prism/evening.png" : null,
    "plugin:window|is_maximized": () => live.maximized === true,
    "plugin:window|minimize": null,
    "plugin:window|toggle_maximize": () => {
      live.maximized = live.maximized !== true;
      return null;
    },
    "plugin:window|close": null,
    get_runtime_health: {
      status: "ok",
      uptimeMs: 1000,
      database: {
        journalMode: "wal",
        synchronous: 1,
        pageSize: 4096,
        pageCount: 100,
        freelistCount: 0,
        sizeBytes: 409600,
        migrationVersion: 6,
        appliedMigrations: 6,
        integrity: "ok",
        poolSize: 6,
        poolIdle: 6,
      },
    },
  };
}

function shellInitScript(config) {
  const security = config.app?.security ?? {};
  return `(() => {
    ${security.freezePrototype ? "Object.freeze(Object.prototype);" : ""}
    const fixture = (${backendFixture.toString()})();
    const callbacks = new Map();
    const listeners = [];
    let nextId = 1;
    const resolveCommand = (command, callArgs) => {
      const value = fixture[command];
      if (typeof value === "function") return value(callArgs ?? {});
      if (value === undefined) {
        const error = new Error("unstubbed command: " + command);
        error.code = "unavailable";
        throw error;
      }
      return value;
    };
    window.__TAURI_INTERNALS__ = {
      invoke: async (command, callArgs) => {
        if (command === "plugin:event|listen") {
          listeners.push({ event: callArgs.event, handler: callArgs.handler });
          return nextId++;
        }
        if (command === "plugin:event|unlisten") return null;
        return resolveCommand(command, callArgs);
      },
      transformCallback: (callback) => {
        const id = nextId++;
        callbacks.set(id, callback);
        return id;
      },
      convertFileSrc: (path) => "asset://localhost/" + encodeURIComponent(path),
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    window.__prismEmit = (event, payload) => {
      for (const entry of listeners) {
        if (entry.event !== event) continue;
        const callback = callbacks.get(entry.handler);
        if (callback) callback({ event, id: entry.handler, payload });
      }
    };
    window.__prismCalls = [];
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, callArgs) => {
      window.__prismCalls.push({ command, args: callArgs });
      return original(command, callArgs);
    };
  })();`;
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

const failures = [];
function check(name, condition, detail) {
  if (condition) {
    console.log(`  ok    ${name}`);
  } else {
    console.log(`  FAIL  ${name}${detail === undefined ? "" : ` — ${detail}`}`);
    failures.push(name);
  }
}

async function main() {
  buildIfNeeded();
  const { chromium } = await loadPlaywright();

  const csp = CONFIG.app?.security?.csp ?? "";
  const indexHtml = await readFile(join(DIST, "index.html"), "utf8");
  const inlineScripts = [
    ...indexHtml.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g),
  ].map((match) => match[1]);
  const hashes = inlineScripts.map(
    (script) => `'sha256-${createHash("sha256").update(script, "utf8").digest("base64")}'`,
  );
  const effectiveCsp =
    csp === "" || hashes.length === 0
      ? csp
      : csp.replace(/script-src([^;]*)/, (directive) => `${directive} ${hashes.join(" ")}`);

  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://prism.local");
    const pathname = url.pathname === "/" ? "/index.html" : decodeURIComponent(url.pathname);
    const file = normalize(join(DIST, pathname));
    if (!file.startsWith(DIST)) {
      response.writeHead(403).end();
      return;
    }
    try {
      const body = await readFile(file);
      response.writeHead(200, {
        "content-type": MIME[extname(file)] ?? "application/octet-stream",
        ...(effectiveCsp === "" ? {} : { "content-security-policy": effectiveCsp }),
      });
      response.end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const port = server.address().port;

  const browser = await chromium.launch({
    headless: !args.has("--headed"),
    args: ["--no-sandbox", "--disable-dev-shm-usage"],
  });
  const window_ = CONFIG.app?.windows?.[0] ?? {};
  const page = await browser.newPage({
    viewport: { width: window_.width ?? 1440, height: window_.height ?? 900 },
  });
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));

  await page.addInitScript(shellInitScript(CONFIG));
  await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: "networkidle" });
  await page.waitForSelector("main", { timeout: 15_000 });
  await page.waitForTimeout(400);

  // ---- Home entries navigate ------------------------------------------------
  console.log("Home");
  const homeCards = page.locator("main button[aria-label^='Open ']");
  check(
    "rails render album tiles",
    (await homeCards.count()) > 0,
    `${String(await homeCards.count())} tiles`,
  );
  await homeCards.first().click();
  await page.waitForTimeout(350);
  check(
    "clicking a cover opens the album page",
    (await page.locator("text=Kid A").count()) > 0 &&
      (await page.getByRole("button", { name: /^Play/ }).count()) > 0,
    `route=${String(await page.evaluate(() => document.querySelector("[data-route]")?.getAttribute("data-route")))}`,
  );

  // Back to Home through the sidebar, then the artist link.
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  await page.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "home",
    undefined,
    { timeout: 5_000 },
  );
  await page.waitForSelector("main button[title='Radiohead']", { timeout: 5_000 });
  const artistLinks = page.locator("main button[title='Radiohead']");
  check("artist names are links", (await artistLinks.count()) > 0);
  if ((await artistLinks.count()) > 0) {
    await artistLinks.first().click();
    await page.waitForTimeout(350);
    check(
      "clicking an artist name opens the artist page",
      (await page.evaluate(
        () => document.querySelector("[data-route]")?.getAttribute("data-route") ?? "",
      )) === "artist",
      String(
        await page.evaluate(() =>
          document.querySelector("[data-route]")?.getAttribute("data-route"),
        ),
      ),
    );
  }

  // ---- Player bar -----------------------------------------------------------
  console.log("Player bar");
  // Mount the bar by pressing Play on an album page: the header button is
  // always visible, unlike a tile's hover-revealed one.
  await page.getByRole("button", { name: "Library" }).first().click();
  await page.waitForTimeout(300);
  const libraryCover = page.locator("main button[aria-label^='Open ']").first();
  await libraryCover.click();
  await page.waitForTimeout(400);
  await page.locator("main").getByRole("button", { name: "Play", exact: true }).first().click();
  await page.waitForTimeout(500);
  const footer = page.locator("footer");
  check("the player bar appears once something is queued", (await footer.count()) > 0);

  const deviceControls = page.locator(
    "footer select, footer button[aria-label*='device' i], footer [role='combobox']",
  );
  check(
    "the player bar has no output-device control",
    (await deviceControls.count()) === 0,
    `${String(await deviceControls.count())} controls`,
  );

  const slider = footer.locator("[role='slider'][aria-label='Seek']").first();
  check("the seek bar exists", (await slider.count()) > 0);
  if ((await slider.count()) > 0) {
    const readPosition = () =>
      page.evaluate(() => {
        const node = document.querySelector("footer [role='slider'][aria-label='Seek']");
        return node === null ? null : Number(node.getAttribute("aria-valuenow"));
      });

    // The engine clock is the source of truth: two ticks must move the bar.
    await page.evaluate(() => {
      window.__prismEmit("player://tick", {
        positionMs: 12_000,
        durationMs: 240_000,
        playing: true,
      });
    });
    await page.waitForTimeout(150);
    const first = await readPosition();
    await page.evaluate(() => {
      window.__prismEmit("player://tick", {
        positionMs: 42_000,
        durationMs: 240_000,
        playing: true,
      });
    });
    await page.waitForTimeout(150);
    const second = await readPosition();
    check(
      "the seek bar advances with the engine clock",
      second !== null && first !== null && second > first,
      `${String(first)} → ${String(second)}`,
    );

    // Dragging to 75 % must preview, then commit a seek on release.
    const box = await slider.boundingBox();
    if (box !== null) {
      await page.mouse.move(box.x + box.width * 0.1, box.y + box.height / 2);
      await page.mouse.down();
      await page.mouse.move(box.x + box.width * 0.75, box.y + box.height / 2, { steps: 8 });
      const during = await readPosition();
      await page.mouse.up();
      await page.waitForTimeout(200);
      check(
        "dragging the seek bar moves it",
        during !== null && during > 120_000,
        `preview=${String(during)}`,
      );
      const seeks = await page.evaluate(
        () => window.__prismCalls.filter((call) => call.command === "player_seek").length,
      );
      check("releasing the seek bar sends one seek", seeks >= 1, `${String(seeks)} calls`);
    }
  }

  // ---- Volume ---------------------------------------------------------------
  console.log("Volume");
  const volume = footer.locator("[role='slider'][aria-label='Volume']").first();
  const queueButton = footer.getByRole("button", { name: "Open queue", exact: true });
  const queueBox = await queueButton.boundingBox();
  const microphoneButton = footer.getByRole("button", {
    name: "Show synchronized lyrics",
    exact: true,
  });
  const microphoneBox = await microphoneButton.boundingBox();
  const volumeBox = await volume.boundingBox();
  check(
    "Queue, microphone and volume keep their requested order",
    queueBox !== null &&
      microphoneBox !== null &&
      volumeBox !== null &&
      queueBox.x < microphoneBox.x &&
      microphoneBox.x < volumeBox.x,
  );
  const sidebarToggle = footer.getByRole("button", { name: "Hide sidebar", exact: true });
  const shuffleButton = footer.getByRole("button", { name: "Shuffle queue", exact: true });
  const sidebarToggleBox = await sidebarToggle.boundingBox();
  const shuffleBox = await shuffleButton.boundingBox();
  check(
    "the sidebar control sits immediately left of Shuffle",
    sidebarToggleBox !== null &&
      shuffleBox !== null &&
      sidebarToggleBox.x < shuffleBox.x &&
      Math.abs(sidebarToggleBox.y - shuffleBox.y) <= 1,
  );
  await sidebarToggle.click();
  check(
    "the transport control hides the full sidebar",
    (await page.locator("aside").count()) === 0,
  );
  await page.mouse.move(2, 420);
  await page.locator("aside").waitFor({ timeout: 2_000 });
  await page.mouse.move(90, 420);
  check(
    "the left edge reveals the hidden sidebar on hover",
    await page.locator("aside").isVisible(),
  );
  await footer.getByRole("button", { name: "Show sidebar", exact: true }).click();
  check("the volume rail exists", (await volume.count()) > 0);
  if ((await volume.count()) > 0) {
    const readVolume = () =>
      page.evaluate(() => {
        const node = document.querySelector("footer [role='slider'][aria-label='Volume']");
        return node === null ? null : Number(node.getAttribute("aria-valuenow"));
      });
    const box = await volume.boundingBox();
    if (box !== null) {
      const before = await readVolume();
      await page.mouse.move(box.x + box.width * 0.95, box.y + box.height / 2);
      await page.mouse.down();
      await page.mouse.move(box.x + box.width * 0.35, box.y + box.height / 2, { steps: 8 });
      const during = await readVolume();
      await page.mouse.up();
      await page.waitForTimeout(250);
      const after = await readVolume();
      check(
        "the volume rail follows the pointer while dragging",
        before !== null && during !== null && during < before - 5,
        `${String(before)} → ${String(during)}`,
      );
      check(
        "the committed level survives the release",
        after !== null && before !== null && after < before - 5,
        `${String(during)} → ${String(after)}`,
      );
      const previews = await page.evaluate(
        () => window.__prismCalls.filter((call) => call.command === "player_set_volume").length,
      );
      check("the drag reaches the engine", previews >= 1, `${String(previews)} calls`);
    }
  }

  // ---- Mute is immediate and idempotent -------------------------------------
  const mute = footer.getByRole("button", { name: "Mute", exact: true });
  if ((await mute.count()) > 0) {
    await mute.click();
    await page.waitForTimeout(150);
    check(
      "mute changes the control on the first click",
      (await footer.getByRole("button", { name: "Unmute", exact: true }).count()) === 1,
    );
    await footer.getByRole("button", { name: "Unmute", exact: true }).click();
    await page.waitForTimeout(150);
    check(
      "unmute restores the previous level",
      (await footer.getByRole("button", { name: "Mute", exact: true }).count()) === 1 &&
        Number(await volume.getAttribute("aria-valuenow")) > 0,
    );
  }

  // ---- Track selection, double click and playlists --------------------------
  console.log("Tracks and playlists");
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Library']").click();
  await page.waitForTimeout(300);
  await page.locator("main button[aria-label^='Open ']").first().click();
  await page.waitForSelector("[data-track-row]", { timeout: 5_000 });
  const rows = page.locator("[data-track-row]");
  check(
    "the album page keeps its full date and genre outside the track rows",
    (await page.locator("main header").getByText("2000-10-01", { exact: false }).count()) === 1 &&
      (await page.locator("main header").getByText("Electronic", { exact: true }).count()) === 1 &&
      (await rows.first().getByText("Electronic", { exact: true }).count()) === 0 &&
      (await rows
        .first()
        .getByText(/FLAC|16\/44\.1K/)
        .count()) === 0,
  );
  check(
    "Album tracklists do not gain playlist-only covers or source-album columns",
    (await rows.locator("[data-playlist-cover]").count()) === 0 &&
      (await rows.locator("[data-playlist-album]").count()) === 0,
  );
  const playsBeforeSelect = await page.evaluate(
    () => window.__prismCalls.filter((call) => call.command === "player_play").length,
  );
  await rows.first().click();
  check(
    "one click selects a song without playing it",
    (await rows.first().getAttribute("aria-selected")) === "true" &&
      (await page.evaluate(
        () => window.__prismCalls.filter((call) => call.command === "player_play").length,
      )) === playsBeforeSelect,
  );
  await rows.nth(1).dblclick();
  await page.waitForTimeout(200);
  const doubleClickPlay = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "player_play").at(-1),
  );
  check(
    "double clicking a row plays that exact song",
    doubleClickPlay?.args?.index === 1,
    JSON.stringify(doubleClickPlay?.args ?? {}),
  );
  await rows.first().getByRole("button", { name: "Play Kid A track 1", exact: true }).click();
  await page.waitForTimeout(200);
  const singleClickPlay = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "player_play").at(-1),
  );
  check(
    "the dedicated Play button starts a song on one click",
    singleClickPlay?.args?.index === 0,
    JSON.stringify(singleClickPlay?.args ?? {}),
  );
  await page.locator("main h1").first().click();
  check(
    "clicking outside a song clears its selection",
    (await rows.first().getAttribute("aria-selected")) === "false",
  );

  await rows.nth(1).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Play next", exact: true }).click();
  await page.waitForTimeout(250);
  const playNext = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "player_update_queue").at(-1),
  );
  check(
    "Play next inserts immediately after the current song without restarting it",
    playNext?.args?.index === 0 &&
      playNext?.args?.tracks?.length === 3 &&
      playNext?.args?.tracks?.[1]?.trackId === "al-one-t2",
    JSON.stringify(playNext?.args ?? {}),
  );
  check(
    "Queue has no Sidebar destination and is opened only by its transport button",
    (await page.locator("aside nav[aria-label='Primary'] button[aria-label='Queue']").count()) ===
      0,
  );
  await footer.getByRole("button", { name: "Open queue", exact: true }).click();
  const queuePage = page.locator("[data-queue-page]");
  await queuePage.waitFor();
  check(
    "Queue rows show title, artist and album on a full content page",
    (await queuePage.getByText("Kid A track 2", { exact: true }).count()) >= 1 &&
      (await queuePage.getByText("Radiohead · Kid A", { exact: true }).count()) >= 1 &&
      (await page.getByRole("dialog", { name: "Playback queue", exact: true }).count()) === 0 &&
      (await page.locator("[data-route]").getAttribute("data-route")) === "queue",
  );
  const queueGeometry = await queuePage.evaluate((node) => {
    const main = document.querySelector("main");
    if (!main) return null;
    const bounds = node.getBoundingClientRect();
    const host = main.getBoundingClientRect();
    return {
      position: getComputedStyle(node).position,
      widthRatio: bounds.width / host.width,
      inside: bounds.x >= host.x && bounds.right <= host.right + 1,
    };
  });
  check(
    "the queue editor uses the large route rather than a fixed floating panel",
    queueGeometry !== null &&
      queueGeometry.position !== "fixed" &&
      queueGeometry.widthRatio >= 0.85 &&
      queueGeometry.inside &&
      JSON.stringify(queueGeometry),
  );
  await queuePage.getByRole("button", { name: "Back", exact: true }).click();
  await page.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "album",
  );
  check(
    "Queue Back returns to the actual previous Album page",
    (await page.locator("[data-route]").getAttribute("data-route")) === "album",
  );
  await footer.getByRole("button", { name: "Open queue", exact: true }).click();
  await queuePage.waitFor();
  await footer.getByRole("button", { name: "Close queue", exact: true }).click();
  await page.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "album",
  );
  check(
    "the Player Bar Queue button toggles entry and exit",
    (await queuePage.count()) === 0 &&
      (await page.locator("[data-route]").getAttribute("data-route")) === "album",
  );
  const orderBeforeShuffle = playNext?.args?.tracks?.map((track) => track.trackId);
  await footer.getByRole("button", { name: "Shuffle queue", exact: true }).click();
  await page.waitForTimeout(200);
  await footer.getByRole("button", { name: "Disable shuffle", exact: true }).click();
  await page.waitForTimeout(200);
  const restoredOrder = await page.evaluate(() =>
    window.__prismCalls
      .filter((call) => call.command === "player_update_queue")
      .at(-1)
      ?.args?.tracks?.map((track) => track.trackId),
  );
  check(
    "disabling shuffle restores the exact previous queue order",
    JSON.stringify(restoredOrder) === JSON.stringify(orderBeforeShuffle),
    JSON.stringify(restoredOrder),
  );

  await rows.nth(1).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New playlist…" }).click();
  await page.getByRole("textbox", { name: "New playlist name" }).fill("Evening");
  await page.getByRole("button", { name: "Create and add", exact: true }).click();
  await page.waitForTimeout(300);
  check(
    "a song can create and join a playlist from its context menu",
    (await page.evaluate(
      () => window.__prismCalls.filter((call) => call.command === "playlist_add_tracks").length,
    )) >= 1,
  );
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Playlists']").click();
  await page.waitForTimeout(300);
  check(
    "the new playlist appears below Library",
    (await page.getByText("Evening", { exact: true }).count()) > 0,
  );
  await page.getByRole("button", { name: /Evening/ }).click();
  await page.waitForSelector("[data-track-row]", { timeout: 5_000 });
  check(
    "the playlist contains the song that was added",
    (await page.locator("[data-track-row]").count()) === 1,
  );
  const playlistCover = page.locator("[data-track-row] [data-playlist-cover]").first();
  check(
    "each playlist row fits its own source-album cover between number and Play",
    (await playlistCover.count()) === 1 &&
      (await playlistCover.evaluate((node) => {
        const row = node.closest("[data-track-row]");
        const play = row?.querySelector("button[aria-label^='Play ']");
        if (!row || !play) return false;
        const number = row.querySelector(":scope > span");
        if (!number) return false;
        return (
          node.getBoundingClientRect().x > number.getBoundingClientRect().x &&
          node.getBoundingClientRect().right < play.getBoundingClientRect().x &&
          node.getBoundingClientRect().height <= row.getBoundingClientRect().height
        );
      })) &&
      (await page.locator("[data-playlist-album]").first().textContent()) === "Kid A",
  );
  await page.locator("[data-playlist-album]").first().click();
  await page.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "album",
  );
  check(
    "the source album name in a playlist opens that album",
    (await page.locator("[data-route]").getAttribute("data-route")) === "album",
  );
  await page.locator("main").getByRole("button", { name: "Back", exact: true }).click();
  await page.waitForSelector("[data-playlist-album]");
  check(
    "playlist numbering starts at one without changing track metadata",
    (await page.locator("[data-track-row] > span").first().textContent())?.trim() === "01" &&
      (
        await page.evaluate(() =>
          window.__prismCalls.filter((call) => call.command === "playlist_add_tracks").at(-1),
        )
      )?.args?.trackIds?.[0] === "al-one-t2",
  );
  await page.getByRole("button", { name: "Change cover", exact: true }).click();
  await page.waitForTimeout(250);
  const pickerCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "plugin:dialog|open").at(-1),
  );
  const coverCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "playlist_set_custom_cover").at(-1),
  );
  check(
    "a native image picker chooses an independent custom playlist cover",
    pickerCall?.args?.options?.multiple === false &&
      pickerCall?.args?.options?.directory === false &&
      coverCall?.args?.playlistId === "pl-1" &&
      coverCall?.args?.path === "/tmp/prism/evening.png" &&
      (await page.getByRole("button", { name: "Change cover", exact: true }).count()) === 1,
    JSON.stringify({ picker: pickerCall?.args ?? {}, cover: coverCall?.args ?? {} }),
  );
  await page.locator("[data-track-row]").first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Remove from this playlist" }).click();
  await page.waitForTimeout(300);
  check(
    "a song can be removed from the playlist",
    (await page.locator("[data-track-row]").count()) === 0,
  );
  await page.getByRole("button", { name: "Edit playlist", exact: true }).click();
  const fullPlaylistEditor = page.getByRole("dialog", { name: "Edit playlist", exact: true });
  await fullPlaylistEditor
    .getByRole("textbox", { name: "Playlist description" })
    .fill("Late night listening");
  await fullPlaylistEditor.getByRole("textbox", { name: "Playlist genre" }).fill("Ambient");
  await fullPlaylistEditor.getByRole("button", { name: "Save", exact: true }).click();
  await page.waitForTimeout(250);
  const fullPlaylistUpdate = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "playlist_update").at(-1),
  );
  check(
    "the in-page pencil saves optional description and genre",
    fullPlaylistUpdate?.args?.name === "Evening" &&
      fullPlaylistUpdate?.args?.description === "Late night listening" &&
      fullPlaylistUpdate?.args?.genre === "Ambient" &&
      (await page.getByText("Late night listening", { exact: true }).count()) === 1 &&
      (await page.getByText("Ambient", { exact: true }).count()) === 1,
    JSON.stringify(fullPlaylistUpdate?.args ?? {}),
  );
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Playlists']").click();
  await page.waitForTimeout(250);
  await page
    .locator("main")
    .getByRole("button", { name: /Evening/ })
    .click({ button: "right" });
  await page.getByRole("menuitem", { name: "Pin playlist", exact: true }).click();
  const dockPlaylist = page.locator(
    "aside section[aria-label='Pinned items'] button[aria-label='Evening']",
  );
  check("playlists can join the same Dock as albums", (await dockPlaylist.count()) === 1);
  check(
    "the Sidebar no longer renders a Dock heading",
    (await page
      .locator("aside h2")
      .filter({ hasText: /^Dock$/ })
      .count()) === 0,
  );
  await page.waitForTimeout(1_000);
  const pinnedBeforeCollapse = await dockPlaylist.boundingBox();
  await page.getByRole("button", { name: "Collapse sidebar", exact: true }).click();
  await page.waitForTimeout(1_000);
  const pinnedAfterCollapse = await dockPlaylist.boundingBox();
  check(
    "collapsing the Sidebar does not vertically shift pinned items",
    pinnedBeforeCollapse !== null &&
      pinnedAfterCollapse !== null &&
      Math.abs(pinnedBeforeCollapse.y - pinnedAfterCollapse.y) <= 1,
    JSON.stringify({ before: pinnedBeforeCollapse, after: pinnedAfterCollapse }),
  );
  await dockPlaylist.click();
  await page.waitForTimeout(250);
  check(
    "the collapsed Dock keeps playlist artwork navigation available",
    (await page.getByRole("button", { name: "Expand sidebar", exact: true }).count()) === 1 &&
      (await page.evaluate(() =>
        document.querySelector("[data-route]")?.getAttribute("data-route"),
      )) === "playlist",
  );
  await page.getByRole("button", { name: "Expand sidebar", exact: true }).click();
  await dockPlaylist.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Unpin playlist", exact: true }).click();

  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Playlists']").click();
  await page
    .locator("main")
    .getByRole("button", { name: /Evening/ })
    .click({ button: "right" });
  await page.getByRole("menuitem", { name: "Rename playlist", exact: true }).click();
  const renameDialog = page.getByRole("dialog", { name: "Rename playlist", exact: true });
  check(
    "context-menu rename deliberately omits description and genre fields",
    (await renameDialog.getByRole("textbox", { name: "Playlist description" }).count()) === 0 &&
      (await renameDialog.getByRole("textbox", { name: "Playlist genre" }).count()) === 0,
  );
  await renameDialog.getByRole("textbox", { name: "Playlist title" }).fill("Evening renamed");
  await renameDialog.getByRole("button", { name: "Save", exact: true }).click();
  await page.waitForTimeout(250);
  const renameUpdate = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "playlist_update").at(-1),
  );
  check(
    "title-only rename preserves the other playlist metadata",
    renameUpdate?.args?.name === "Evening renamed" &&
      renameUpdate?.args?.description === "Late night listening" &&
      renameUpdate?.args?.genre === "Ambient",
    JSON.stringify(renameUpdate?.args ?? {}),
  );

  // ---- Album context details ------------------------------------------------
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Library']").click();
  await page.waitForTimeout(300);
  await page.locator("main button[aria-label^='Open ']").first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Album details" }).click();
  await page.waitForSelector("[role='dialog'][aria-label^='Details for']", { timeout: 5_000 });
  check(
    "album right click opens useful, designed metadata",
    (await page.getByText("Genre", { exact: true }).count()) > 0 &&
      (await page.getByText("Electronic", { exact: true }).count()) > 0,
  );
  const detailLabels = await page
    .locator("[role='dialog'][aria-label^='Details for'] dt")
    .allTextContents();
  const detailQuality = page
    .locator("[role='dialog'][aria-label^='Details for']")
    .getByText("16/44.1K · CD", { exact: true });
  check(
    "Album details keeps the requested metadata order and source label",
    JSON.stringify(detailLabels) ===
      JSON.stringify([
        "Artist Name",
        "Album Title",
        "Date",
        "Genre",
        "Discs",
        "Songs",
        "Length",
        "Quality",
        "Format",
      ]) &&
      (await detailQuality.count()) === 1 &&
      (await detailQuality.getAttribute("class"))?.includes("text-positive") === true,
    JSON.stringify(detailLabels),
  );
  await page.getByRole("button", { name: "Close details" }).click();
  await page.locator("main button[aria-label='Open Lemonade']").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Album details" }).click();
  const hiResQuality = page
    .locator("[role='dialog'][aria-label^='Details for']")
    .getByText("24/96K · Hi-Res", { exact: true });
  await hiResQuality.waitFor();
  check(
    "Hi-Res album quality uses the shared purple source treatment",
    (await hiResQuality.getAttribute("class"))?.includes("text-spectrum-violet") === true,
  );
  await page.getByRole("button", { name: "Close details" }).click();

  await page.locator("main button[aria-label='Open Kid A']").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Pin album", exact: true }).click();
  await page.locator("main button[aria-label='Open Lemonade']").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Pin album", exact: true }).click();
  const dockButtons = page.locator("aside section[aria-label='Pinned items'] button");
  check(
    "new Dock pins appear first and navigate as album entries",
    (await dockButtons.count()) === 2 &&
      (await dockButtons.first().getAttribute("aria-label")) === "Lemonade",
    JSON.stringify(await dockButtons.allTextContents()),
  );
  await dockButtons.first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Unpin album", exact: true }).click();
  check(
    "a Dock context menu unpins only that album",
    (await page
      .locator("aside section[aria-label='Pinned items'] button[aria-label='Lemonade']")
      .count()) === 0 &&
      (await page
        .locator("aside section[aria-label='Pinned items'] button[aria-label='Kid A']")
        .count()) === 1,
  );
  await page
    .locator("aside section[aria-label='Pinned items'] button[aria-label='Kid A']")
    .click({ button: "right" });
  await page.getByRole("menuitem", { name: "Unpin album", exact: true }).click();

  // ---- Home collections really filter ---------------------------------------
  console.log("Collections and saved browsing");
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  await page.waitForTimeout(300);
  const addedRail = page
    .locator("section")
    .filter({ has: page.getByRole("heading", { name: "Recently added", exact: true }) })
    .first();
  await addedRail.getByRole("button", { name: "See all" }).click();
  await page.waitForTimeout(300);
  const collectionCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "Recently added See all queries that collection",
    collectionCall?.args?.collection === "recentlyAdded",
    JSON.stringify(collectionCall?.args ?? {}),
  );
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  await page.waitForTimeout(300);
  const genreRail = page
    .locator("section")
    .filter({ has: page.getByRole("heading", { name: "Electronic", exact: true }) })
    .first();
  await genreRail.getByRole("button", { name: "See all" }).click();
  await page.waitForTimeout(300);
  const genreCollectionCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "a Home genre shelf opens all albums of that genre",
    genreCollectionCall?.args?.genre === "Electronic",
    JSON.stringify(genreCollectionCall?.args ?? {}),
  );

  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Library']").click();
  await page.waitForTimeout(300);
  const sortBar = page.locator("main div[aria-label='Sort albums']");
  await sortBar.getByRole("button", { name: /^Artist/ }).click();
  const genreFilter = page.getByRole("button", { name: "Filter by genre", exact: true });
  await genreFilter.click();
  const genreMenu = page.getByRole("listbox", { name: "Filter by genre", exact: true });
  const genreMenuBox = await genreMenu.boundingBox();
  if (genreMenuBox !== null) {
    await page.mouse.move(genreMenuBox.x + genreMenuBox.width / 2, genreMenuBox.y + 80);
    await page.mouse.wheel(0, 180);
    await page.waitForTimeout(150);
  }
  check(
    "scrolling the genre popover keeps it open",
    (await genreMenu.isVisible()) && (await genreMenu.evaluate((node) => node.scrollTop)) > 0,
  );
  if (genreMenuBox !== null) {
    await page.mouse.move(genreMenuBox.x + genreMenuBox.width - 4, genreMenuBox.y + 45);
    await page.mouse.down();
    await page.mouse.move(genreMenuBox.x + genreMenuBox.width - 4, genreMenuBox.y + 130, {
      steps: 5,
    });
    await page.mouse.up();
    await page.waitForTimeout(100);
  }
  check(
    "dragging inside the genre popover scrollbar does not dismiss it",
    await genreMenu.isVisible(),
  );
  await page.getByRole("option", { name: "Electronic (2 albums)", exact: true }).click();
  await page.waitForTimeout(250);
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Library']").click();
  await page.waitForTimeout(250);
  check(
    "sort direction survives leaving Library while genre is cleared",
    (await sortBar.getByRole("button", { name: /^Artist/ }).getAttribute("aria-pressed")) ===
      "true" && (await genreFilter.textContent())?.includes("All genres"),
  );
  const transientBrowse = await page.evaluate(() => {
    const stored = JSON.parse(localStorage.getItem("prism.library.browse") || "null");
    return stored?.state ?? null;
  });
  check(
    "only Library sorting is persisted among search and filter controls",
    transientBrowse !== null &&
      transientBrowse.sort === "artistDesc" &&
      transientBrowse.genre === undefined &&
      transientBrowse.decade === undefined &&
      transientBrowse.year === undefined,
    JSON.stringify(transientBrowse),
  );
  check(
    "the year row is hidden until a decade is active",
    (await page.locator("[aria-label='Filter by year']").count()) === 0,
  );
  const decadeFilter = page.getByRole("button", { name: "Filter by decade", exact: true });
  await decadeFilter.click();
  const decadeMenu = page.getByRole("listbox", { name: "Filter by decade", exact: true });
  const decadeMenuBox = await decadeMenu.boundingBox();
  if (decadeMenuBox !== null) {
    await page.mouse.move(decadeMenuBox.x + decadeMenuBox.width / 2, decadeMenuBox.y + 80);
    await page.mouse.wheel(0, 180);
    await page.waitForTimeout(120);
    await page.mouse.move(decadeMenuBox.x + decadeMenuBox.width - 4, decadeMenuBox.y + 45);
    await page.mouse.down();
    await page.mouse.move(decadeMenuBox.x + decadeMenuBox.width - 4, decadeMenuBox.y + 120, {
      steps: 4,
    });
    await page.mouse.up();
  }
  check(
    "decade scrolling and scrollbar dragging keep the popover open",
    (await decadeMenu.isVisible()) && (await decadeMenu.evaluate((node) => node.scrollTop)) > 0,
  );
  await page.getByRole("option", { name: "2000s (1 album)", exact: true }).click();
  await page.waitForTimeout(250);
  const decadeCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  const yearRow = page.locator("[aria-label='Filter by year']");
  check(
    "a decade requests its real range and reveals only available ordered years",
    decadeCall?.args?.yearFrom === 2000 &&
      decadeCall?.args?.yearTo === 2009 &&
      (await yearRow.getByRole("button", { name: "2000", exact: true }).count()) === 1 &&
      (await yearRow.getByRole("button", { name: "2016", exact: true }).count()) === 0 &&
      (await yearRow.getByRole("button", { name: "Previous decade", exact: true }).isDisabled()),
    JSON.stringify(decadeCall?.args ?? {}),
  );
  await yearRow.getByRole("button", { name: "Next decade", exact: true }).click();
  await yearRow.getByRole("button", { name: "2016", exact: true }).click();
  await page.waitForTimeout(250);
  const yearCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "decade arrows and year buttons produce an exact year query",
    yearCall?.args?.yearFrom === 2016 && yearCall?.args?.yearTo === 2016,
    JSON.stringify(yearCall?.args ?? {}),
  );
  await yearRow.getByRole("button", { name: "Previous decade", exact: true }).click();
  check(
    "changing decade clears a year that no longer belongs to it",
    (await yearRow
      .getByRole("button", { name: "All years", exact: true })
      .getAttribute("aria-pressed")) === "true" &&
      (await yearRow.getByRole("button", { name: "2016", exact: true }).count()) === 0,
  );

  await page.getByRole("button", { name: "Clear filters", exact: true }).click();
  await page.waitForTimeout(200);
  check(
    "Clear filters restores the unfiltered Library without changing its sort",
    (await page.locator("[aria-label='Filter by year']").count()) === 0 &&
      (await decadeFilter.textContent())?.includes("All decades") &&
      (await sortBar.getByRole("button", { name: /^Artist/ }).getAttribute("aria-pressed")) ===
        "true",
  );
  const librarySearch = page.getByRole("textbox", { name: "Search the library", exact: true });
  await librarySearch.fill("Radiohead");
  await librarySearch.press("Enter");
  await page.waitForTimeout(250);
  const searchCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "submitting a Library search applies its text query",
    searchCall?.args?.text === "Radiohead",
    JSON.stringify(searchCall?.args ?? {}),
  );
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Library']").click();
  await page.waitForTimeout(250);
  const clearedSearchCall = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "leaving and returning clears Library search text and results",
    (await librarySearch.inputValue()) === "" &&
      (clearedSearchCall?.args?.text === null || clearedSearchCall?.args?.text === undefined),
    JSON.stringify(clearedSearchCall?.args ?? {}),
  );

  // ---- Appearance controls have visible effects -----------------------------
  console.log("Appearance and desktop input");
  await page.locator("header button[aria-label='Settings']").click();
  await page.waitForTimeout(350);
  check(
    "Settings owns System diagnostics without a separate sidebar destination",
    (await page.getByRole("heading", { name: "System", exact: true }).count()) === 1 &&
      (await page.locator("aside nav[aria-label='Configuration']").count()) === 0 &&
      (await page.getByText("Library indexed", { exact: true }).count()) === 0,
  );
  const themeChoices = page.getByRole("radiogroup", { name: "Theme", exact: true });
  check(
    "Theme is the first Settings section with three symbolic Home previews",
    (await page
      .locator("main section")
      .first()
      .getByRole("heading", { name: "Theme", exact: true })
      .count()) === 1 && (await themeChoices.locator("[data-theme-choice]").count()) === 3,
  );
  await themeChoices.getByRole("radio", { name: "Light", exact: true }).click();
  await page.waitForTimeout(150);
  const lightColours = await page.evaluate(() => ({
    className: document.documentElement.className,
    background: getComputedStyle(document.body).backgroundColor,
    text: getComputedStyle(document.body).color,
    expectedText: (() => {
      const probe = document.createElement("span");
      probe.style.color = "color-mix(in oklab, rgb(28 28 32) 85%, transparent)";
      document.body.appendChild(probe);
      const value = getComputedStyle(probe).color;
      probe.remove();
      return value;
    })(),
  }));
  check(
    "Light flips semantic surfaces and ink across the shell",
    lightColours.className.includes("light") &&
      lightColours.background === "rgb(244, 244, 246)" &&
      lightColours.text === lightColours.expectedText,
    JSON.stringify(lightColours),
  );
  await themeChoices.getByRole("radio", { name: "Dark", exact: true }).click();
  await page.waitForTimeout(100);
  check(
    "Dark retains the original OLED colorimetry",
    (await page.evaluate(() => getComputedStyle(document.body).backgroundColor)) ===
      "rgb(8, 8, 10)" &&
      (await page.evaluate(() => document.documentElement.classList.contains("dark"))),
  );
  await themeChoices.getByRole("radio", { name: "System", exact: true }).click();
  await page.emulateMedia({ colorScheme: "light" });
  await page.waitForTimeout(120);
  const systemLight = await page.evaluate(() =>
    document.documentElement.classList.contains("light"),
  );
  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForTimeout(120);
  const systemDark = await page.evaluate(() => document.documentElement.classList.contains("dark"));
  check("System follows live operating-system light/dark changes", systemLight && systemDark);
  await themeChoices.getByRole("radio", { name: "Dark", exact: true }).click();
  await page.emulateMedia({ colorScheme: "light" });
  await page.waitForTimeout(100);
  check(
    "a fixed Dark choice does not follow the system preference",
    await page.evaluate(() => document.documentElement.classList.contains("dark")),
  );
  const effects = page.getByRole("radiogroup", { name: "Visual effects" });
  await effects.getByRole("radio", { name: "Reduced", exact: true }).click();
  check(
    "Visual effects can leave Auto",
    (await page.locator("[data-effects]").getAttribute("data-effects")) === "reduced",
  );
  await page
    .getByRole("radiogroup", { name: "Corner style" })
    .getByRole("radio", { name: "Round" })
    .click();
  await page
    .getByRole("radiogroup", { name: "Density", exact: true })
    .getByRole("radio", { name: "Compact" })
    .click();
  check(
    "corners and density change the rendered shell",
    (await page.locator("[data-corners]").getAttribute("data-corners")) === "round" &&
      (await page.locator("[data-density]").getAttribute("data-density")) === "compact",
  );
  await page.getByRole("slider", { name: "Colour bleed" }).focus();
  await page.keyboard.press("End");
  await page.waitForTimeout(200);
  const tintedSurface = await page
    .locator("[data-corners]")
    .evaluate((node) => getComputedStyle(node).getPropertyValue("--prism-surface"));
  check("Colour bleed reaches the surface colour", tintedSurface.includes("100%"), tintedSurface);
  await page.getByRole("slider", { name: "Cover size" }).focus();
  await page.keyboard.press("End");
  await page.getByRole("switch", { name: "Rail arrows" }).click();
  await page.getByRole("switch", { name: "Home counters" }).click();
  await page.getByRole("switch", { name: "Spectrum", exact: true }).click();
  await page.getByRole("switch", { name: "Spectrum", exact: true }).click();
  await page
    .getByRole("radiogroup", { name: "Discord activity while paused" })
    .getByRole("radio", { name: "Hide activity" })
    .click();
  const pausePreference = await page.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "discord_configure").at(-1),
  );
  check(
    "Discord can hide activity only while paused",
    pausePreference?.args?.config?.showPaused === false,
  );

  await page.getByRole("button", { name: "Enter fullscreen", exact: true }).click();
  check(
    "the fullscreen button enters borderless fullscreen",
    (await page.getByRole("button", { name: "Exit fullscreen", exact: true }).count()) === 1,
  );
  await page.keyboard.press("F11");
  await page.waitForTimeout(150);
  check(
    "F11 leaves fullscreen",
    (await page.getByRole("button", { name: "Enter fullscreen", exact: true }).count()) === 1,
  );
  await page.getByRole("button", { name: "Minimize", exact: true }).click();
  await page.getByRole("button", { name: "Maximize", exact: true }).click();
  await page.waitForTimeout(150);
  check(
    "custom window controls call minimize and maximize",
    (await page.getByRole("button", { name: "Restore window", exact: true }).count()) === 1 &&
      (await page.evaluate(
        () =>
          window.__prismCalls.filter((call) => call.command === "plugin:window|minimize").length,
      )) === 1 &&
      (await page.evaluate(
        () =>
          window.__prismCalls.filter((call) => call.command === "plugin:window|toggle_maximize")
            .length,
      )) === 1,
  );
  await page.getByRole("button", { name: "Restore window", exact: true }).click();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  check(
    "custom Close uses the native window close operation",
    (await page.evaluate(
      () => window.__prismCalls.filter((call) => call.command === "plugin:window|close").length,
    )) === 1,
  );
  const blocked = await page.evaluate(() => {
    const key = (key, ctrlKey = false) =>
      !document.dispatchEvent(
        new KeyboardEvent("keydown", { key, ctrlKey, bubbles: true, cancelable: true }),
      );
    return { find: key("f", true), print: key("p", true), select: key("a", true), tab: key("Tab") };
  });
  check(
    "browser shortcuts and Tab navigation are suppressed",
    blocked.find && blocked.print && blocked.select && blocked.tab,
    JSON.stringify(blocked),
  );
  const globalSearch = page.getByRole("textbox", { name: "Search the library", exact: true });
  await globalSearch.fill("Radiohead");
  await globalSearch.press("Control+A");
  const editableSelection = await globalSearch.evaluate((node) => ({
    start: node.selectionStart,
    end: node.selectionEnd,
    length: node.value.length,
  }));
  check(
    "Ctrl+A selects editable text while global selection stays disabled",
    editableSelection.start === 0 &&
      editableSelection.end === editableSelection.length &&
      (await page
        .locator("main h1")
        .first()
        .evaluate((node) => getComputedStyle(node).userSelect)) === "none",
    JSON.stringify(editableSelection),
  );
  await globalSearch.fill("");
  const dragsBeforeInteractiveInput = await page.evaluate(
    () =>
      window.__prismCalls.filter((call) => call.command === "plugin:window|start_dragging").length,
  );
  await globalSearch.click();
  check(
    "Top Bar controls and search never initiate window dragging",
    dragsBeforeInteractiveInput === 0 &&
      (await page.evaluate(
        () =>
          window.__prismCalls.filter((call) => call.command === "plugin:window|start_dragging")
            .length,
      )) === 0,
  );
  await page.getByRole("banner").getByText("Prism", { exact: true }).click();
  await page.waitForTimeout(100);
  check(
    "non-interactive Top Bar space starts native window dragging",
    (await page.evaluate(
      () =>
        window.__prismCalls.filter((call) => call.command === "plugin:window|start_dragging")
          .length,
    )) === 1,
  );
  const pointer = await page
    .getByRole("button", { name: "Enter fullscreen", exact: true })
    .evaluate((node) => getComputedStyle(node).cursor);
  check("interactive controls use the pointer cursor", pointer === "pointer", pointer);

  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  await page.waitForTimeout(250);
  check(
    "rail arrows and Home counters really turn off",
    (await page.getByRole("button", { name: "Scroll right" }).count()) === 0 &&
      (await page.getByText("Resolution ceiling", { exact: true }).count()) === 0,
  );
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Now Playing']").click();
  await page.waitForTimeout(300);
  check(
    "the Spectrum switch mounts the analyser in the single Now Playing layout",
    (await page.getByRole("heading", { name: "Spectrum", exact: true }).count()) > 0 &&
      (await page.getByRole("region", { name: "Lyrics", exact: true }).count()) === 0 &&
      (await page.getByRole("button", { name: "Configure modules", exact: true }).count()) === 0,
  );

  // ---- Everything is still chosen after a reload ----------------------------
  await page.locator("header button[aria-label='Settings']").click();
  await page
    .getByRole("radiogroup", { name: "Theme", exact: true })
    .getByRole("radio", { name: "Light", exact: true })
    .click();
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForSelector("main", { timeout: 15_000 });
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Library']").click();
  await page.waitForTimeout(300);
  check(
    "theme and Library sorting preferences survive a reload without restoring filters",
    (await page.evaluate(() => document.documentElement.classList.contains("light"))) &&
      (
        await page.getByRole("button", { name: "Filter by genre", exact: true }).textContent()
      )?.includes("All genres") &&
      (await page.locator("[data-cover-size]").getAttribute("data-cover-size")) === "260" &&
      (await page.locator("[data-density]").getAttribute("data-density")) === "compact",
  );
  await page.locator("header button[aria-label='Settings']").click();
  await page
    .getByRole("radiogroup", { name: "Theme", exact: true })
    .getByRole("radio", { name: "Dark", exact: true })
    .click();
  await page.locator("aside nav[aria-label='Primary'] button[aria-label='Now Playing']").click();
  try {
    await page.getByRole("region", { name: "Spectrum", exact: true }).waitFor({ timeout: 5_000 });
  } catch (error) {
    console.error("Now Playing reload diagnostics", {
      main: await page.locator("main").innerText(),
      footers: await page.locator("footer").count(),
      pageErrors,
      preferences: await page.evaluate(() => localStorage.getItem("prism.shell.preferences")),
    });
    throw error;
  }
  check(
    "the fixed layout survives reload and Spectrum replaces absent lyrics",
    (await page.getByRole("region", { name: "Spectrum", exact: true }).count()) === 1 &&
      (await page.getByRole("region", { name: "Lyrics", exact: true }).count()) === 0 &&
      (await page.getByRole("button", { name: "Configure modules", exact: true }).count()) === 0,
  );

  // ---- Fixed Now Playing, synchronized lyrics and real history ---------------
  console.log("Now Playing, synchronized lyrics and navigation");
  const synchronized = await browser.newPage({
    viewport: { width: window_.width ?? 1440, height: window_.height ?? 900 },
  });
  synchronized.on("pageerror", (error) => pageErrors.push(error.message));
  await synchronized.addInitScript(shellInitScript(CONFIG));
  await synchronized.goto(`http://127.0.0.1:${port}/?lyrics=1`, {
    waitUntil: "networkidle",
  });
  await synchronized.waitForSelector("main", { timeout: 15_000 });
  await synchronized
    .locator("aside nav[aria-label='Primary'] button[aria-label='Library']")
    .click();
  await synchronized.locator("main button[aria-label^='Open ']").first().click();
  await synchronized
    .locator("main")
    .getByRole("button", { name: "Play", exact: true })
    .first()
    .click();
  const synchronizedFooter = synchronized.locator("footer");
  await synchronizedFooter.waitFor({ timeout: 5_000 });
  const mainBeforeLyrics = await synchronized.locator("main").boundingBox();
  await synchronizedFooter
    .getByRole("button", { name: "Show synchronized lyrics", exact: true })
    .click();
  const lyricSheet = synchronized.locator("[data-lyrics-dock]");
  await lyricSheet.waitFor({ timeout: 3_000 });
  await synchronized.waitForFunction(() => {
    const node = document.querySelector("[data-lyrics-dock]");
    if (!node) return false;
    const transform = getComputedStyle(node).transform;
    return transform === "none" || transform === "matrix(1, 0, 0, 1, 0, 0)";
  });
  const sheetGeometry = await lyricSheet.evaluate((node) => {
    const main = document.querySelector("main");
    const topbar = document.querySelector("[data-topbar]");
    const player = document.querySelector("footer");
    if (!main || !topbar || !player) return null;
    const bounds = node.getBoundingClientRect();
    const mainBounds = main.getBoundingClientRect();
    return {
      x: bounds.x,
      width: bounds.width,
      viewport: window.innerWidth,
      topDifference: Math.abs(bounds.top - topbar.getBoundingClientRect().bottom),
      bottomDifference: Math.abs(bounds.bottom - player.getBoundingClientRect().top),
      mainRightDifference: Math.abs(mainBounds.right - bounds.left),
      rightDifference: Math.abs(bounds.right - window.innerWidth),
      mainWidth: mainBounds.width,
      position: getComputedStyle(node).position,
      siblings: node.parentElement === main.parentElement,
    };
  });
  check(
    "Lyrics is a bounded right column that shrinks the current page",
    sheetGeometry !== null &&
      mainBeforeLyrics !== null &&
      sheetGeometry.width >= 300 &&
      sheetGeometry.width <= 432 &&
      sheetGeometry.topDifference <= 1 &&
      sheetGeometry.bottomDifference <= 1 &&
      sheetGeometry.mainRightDifference <= 1 &&
      sheetGeometry.rightDifference <= 1 &&
      Math.abs(mainBeforeLyrics.width - sheetGeometry.mainWidth - sheetGeometry.width) <= 2 &&
      sheetGeometry.position !== "fixed" &&
      sheetGeometry.siblings,
    JSON.stringify(sheetGeometry),
  );
  await synchronized.evaluate(() => {
    window.__prismEmit("player://tick", {
      positionMs: 6_000,
      durationMs: 240_000,
      playing: true,
    });
  });
  await synchronized.waitForFunction(() =>
    document
      .querySelector("[data-lyrics-dock] [aria-current='true']")
      ?.textContent?.includes("Second synchronized line"),
  );
  check(
    "the shared playhead moves synchronized highlighting in real time",
    (await lyricSheet.locator("[aria-current='true']").textContent()) ===
      "Second synchronized line",
  );
  await synchronizedFooter
    .getByRole("button", { name: "Hide synchronized lyrics", exact: true })
    .click();
  await lyricSheet.waitFor({ state: "hidden", timeout: 3_000 });
  await synchronized.waitForTimeout(300);
  const mainAfterLyrics = await synchronized.locator("main").boundingBox();
  check(
    "closing the lyrics column restores the page width",
    mainBeforeLyrics !== null &&
      mainAfterLyrics !== null &&
      Math.abs(mainBeforeLyrics.width - mainAfterLyrics.width) <= 1,
    JSON.stringify({ before: mainBeforeLyrics, after: mainAfterLyrics }),
  );

  await synchronized
    .locator("aside nav[aria-label='Primary'] button[aria-label='Now Playing']")
    .click();
  const nowPlaying = synchronized.locator("[data-now-playing]");
  const secondary = synchronized.locator("[data-now-playing-secondary]");
  await nowPlaying.waitFor({ timeout: 3_000 });
  await synchronized.getByRole("region", { name: "Lyrics", exact: true }).waitFor();
  await synchronized.waitForFunction(() => {
    const route = document.querySelector("[data-now-playing]")?.parentElement;
    if (!route) return false;
    const transform = getComputedStyle(route).transform;
    return transform === "none" || transform === "matrix(1, 0, 0, 1, 0, 0)";
  });
  check(
    "Now Playing hides only the global Player Bar",
    !(await synchronizedFooter.isVisible()) &&
      (await synchronized.locator("[data-route]").getAttribute("data-route")) === "now-playing",
  );
  const fixedGeometry = await synchronized.evaluate(() => {
    const main = document.querySelector("main");
    const artworkRegion = document.querySelector("[data-now-playing-artwork-region]");
    const secondaryNode = document.querySelector("[data-now-playing-secondary]");
    const controls = document.querySelector("[data-now-playing-controls]");
    const artwork = document.querySelector("[data-now-playing-artwork]");
    if (!main || !artworkRegion || !secondaryNode || !controls || !artwork) return null;
    const left = artworkRegion.getBoundingClientRect();
    const right = secondaryNode.getBoundingClientRect();
    const art = artwork.getBoundingClientRect();
    return {
      overflow: main.scrollHeight - main.clientHeight,
      widthDifference: Math.abs(left.width - right.width),
      topDifference: Math.abs(art.top - right.top),
      bottomDifference: Math.abs(controls.getBoundingClientRect().bottom - right.bottom),
      artworkWidth: art.width,
      artworkHeight: art.height,
    };
  });
  check(
    "the Lyrics/Spectrum column aligns with artwork top and playback bottom",
    fixedGeometry !== null &&
      fixedGeometry.overflow <= 1 &&
      fixedGeometry.widthDifference <= 1 &&
      fixedGeometry.topDifference <= 1 &&
      fixedGeometry.bottomDifference <= 1 &&
      fixedGeometry.artworkWidth >= 100 &&
      fixedGeometry.artworkHeight >= 100 &&
      Math.abs(fixedGeometry.artworkWidth - fixedGeometry.artworkHeight) <= 1,
    JSON.stringify(fixedGeometry),
  );
  check(
    "the embedded Now Playing transport is complete",
    (await nowPlaying.getByRole("button", { name: "Pause", exact: true }).count()) === 1 &&
      (await nowPlaying.getByRole("button", { name: "Previous track", exact: true }).count()) ===
        1 &&
      (await nowPlaying.getByRole("button", { name: "Next track", exact: true }).count()) === 1 &&
      (await nowPlaying.getByRole("button", { name: "Stop", exact: true }).count()) === 1 &&
      (await nowPlaying.getByRole("button", { name: "Open queue", exact: true }).count()) === 1 &&
      (await nowPlaying.getByRole("slider", { name: "Seek", exact: true }).count()) === 1 &&
      (await nowPlaying.getByRole("slider", { name: "Volume", exact: true }).count()) === 1,
  );
  const transportGeometry = await synchronized.evaluate(() => {
    const controls = document.querySelector("[data-now-playing-controls]");
    const volume = controls?.querySelector("[role='slider'][aria-label='Volume']");
    const queue = controls?.querySelector("button[aria-label='Open queue']");
    const play = controls?.querySelector("button[aria-label='Pause']");
    const seek = controls?.querySelector("[role='slider'][aria-label='Seek']");
    if (!volume || !queue || !play || !seek) return null;
    const volumeBox = volume.getBoundingClientRect();
    const queueBox = queue.getBoundingClientRect();
    const playBox = play.getBoundingClientRect();
    const seekBox = seek.getBoundingClientRect();
    return {
      volumeAboveSeek: volumeBox.bottom <= seekBox.top,
      queueAboveSeek: queueBox.bottom <= seekBox.top,
      volumeLevel: Math.abs(volumeBox.y + volumeBox.height / 2 - (playBox.y + playBox.height / 2)),
      queueLevel: Math.abs(queueBox.y + queueBox.height / 2 - (playBox.y + playBox.height / 2)),
      volumeLeft: volumeBox.right < playBox.x,
      queueRight: queueBox.x > playBox.right,
    };
  });
  check(
    "Volume and Queue flank the transport above the seek bar",
    transportGeometry !== null &&
      transportGeometry.volumeAboveSeek &&
      transportGeometry.queueAboveSeek &&
      transportGeometry.volumeLevel <= 8 &&
      transportGeometry.queueLevel <= 8 &&
      transportGeometry.volumeLeft &&
      transportGeometry.queueRight,
    JSON.stringify(transportGeometry),
  );
  check(
    "lyrics exist without enabling Spectrum by default",
    (await synchronized.getByRole("region", { name: "Lyrics", exact: true }).count()) === 1 &&
      (await synchronized.getByRole("region", { name: "Spectrum", exact: true }).count()) === 0,
  );
  await nowPlaying.getByRole("button", { name: "Open queue", exact: true }).click();
  const embeddedQueue = synchronized.locator("[data-queue-page]");
  await embeddedQueue.waitFor();
  check(
    "the embedded Queue button opens the same full page",
    (await synchronized.locator("[data-route]").getAttribute("data-route")) === "queue" &&
      (await synchronized.getByRole("dialog", { name: "Playback queue", exact: true }).count()) ===
        0,
  );
  await embeddedQueue.getByRole("button", { name: "Back", exact: true }).click();
  await nowPlaying.waitFor();
  await synchronized
    .locator("[data-now-playing-secondary]")
    .getByRole("button", { name: "Spectrum", exact: true })
    .click();
  await synchronized.getByRole("region", { name: "Spectrum", exact: true }).waitFor();
  const panelWidths = await synchronized.evaluate(() => {
    const secondaryNode = document.querySelector("[data-now-playing-secondary]");
    const lyrics = document.querySelector("[aria-label='Lyrics']");
    const spectrum = document.querySelector("[aria-label='Spectrum']");
    if (!secondaryNode || !lyrics || !spectrum) return null;
    return {
      container: secondaryNode.getBoundingClientRect().width,
      lyrics: lyrics.getBoundingClientRect().width,
      spectrum: spectrum.getBoundingClientRect().width,
    };
  });
  check(
    "Spectrum can coexist with lyrics at the same bounded width",
    panelWidths !== null &&
      Math.abs(panelWidths.container - panelWidths.lyrics) <= 1 &&
      Math.abs(panelWidths.container - panelWidths.spectrum) <= 1,
    JSON.stringify(panelWidths),
  );
  await synchronized.locator("[data-now-playing-title]").click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "album",
  );
  check(
    "the Now Playing song title opens its source album",
    (await synchronized.locator("[data-route]").getAttribute("data-route")) === "album",
  );
  await synchronized.locator("main").getByRole("button", { name: "Back", exact: true }).click();
  await nowPlaying.waitFor();
  await synchronized.locator("[data-now-playing-album]").click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "album",
  );
  check(
    "the Now Playing album name opens that album",
    (await synchronized.locator("[data-route]").getAttribute("data-route")) === "album",
  );
  await synchronized.locator("main").getByRole("button", { name: "Back", exact: true }).click();
  await nowPlaying.waitFor();
  await synchronized.locator("[data-now-playing-artist]").click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "artist",
  );
  check(
    "the Now Playing artist name opens that artist",
    (await synchronized.locator("[data-route]").getAttribute("data-route")) === "artist",
  );
  await synchronized.locator("main").getByRole("button", { name: "Back", exact: true }).click();
  await nowPlaying.waitFor();
  await synchronized.locator("[data-now-playing-genre]").click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "library",
  );
  await synchronized.waitForTimeout(250);
  const nowPlayingGenreCall = await synchronized.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "Now Playing genre explicitly opens that genre in Library",
    nowPlayingGenreCall?.args?.genre === "Electronic" &&
      (
        await synchronized
          .getByRole("button", { name: "Filter by genre", exact: true })
          .textContent()
      )?.includes("Electronic"),
    JSON.stringify(nowPlayingGenreCall?.args ?? {}),
  );

  await synchronized.locator("aside nav[aria-label='Primary'] button[aria-label='Home']").click();
  check(
    "the global Player Bar returns immediately outside Now Playing",
    await synchronizedFooter.isVisible(),
  );

  await synchronized
    .locator("aside nav[aria-label='Primary'] button[aria-label='Library']")
    .click();
  await synchronized.locator("main button[aria-label^='Open ']").first().click();
  await synchronized
    .getByRole("button", { name: "Show Electronic albums in Library", exact: true })
    .click();
  await synchronized.waitForTimeout(250);
  const albumGenreCall = await synchronized.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "album genre enters Library with that genre selected",
    albumGenreCall?.args?.genre === "Electronic" &&
      (
        await synchronized
          .getByRole("button", { name: "Filter by genre", exact: true })
          .textContent()
      )?.includes("Electronic"),
    JSON.stringify(albumGenreCall?.args ?? {}),
  );
  await synchronized.locator("main button[aria-label^='Open ']").first().click();
  const albumArtistLink = synchronized
    .locator("main header")
    .getByRole("button", { name: "Radiohead", exact: true });
  await albumArtistLink.waitFor({ timeout: 5_000 });
  await synchronized.waitForTimeout(250);
  await albumArtistLink.click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "artist",
  );
  await synchronized.locator("main").getByRole("button", { name: "Back", exact: true }).click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "album",
  );
  check(
    "a detail Back arrow returns to the actual previous route",
    (await synchronized.locator("[data-route]").getAttribute("data-route")) === "album",
  );
  await synchronized.setViewportSize({
    width: window_.width ?? 1440,
    height: 360,
  });
  const albumScroll = await synchronized.locator("main").evaluate((node) => {
    const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
    if (descriptor?.get && descriptor?.set) {
      Object.defineProperty(node, "scrollTop", {
        configurable: true,
        get() {
          return descriptor.get.call(this);
        },
        set(value) {
          window.__prismScrollWrites = (window.__prismScrollWrites ?? 0) + 1;
          descriptor.set.call(this, value);
        },
      });
    }
    node.scrollTop = 160;
    window.__prismScrollWrites = 0;
    return node.scrollTop;
  });
  check("the originating Album can be scrolled for restoration", albumScroll > 0);
  await synchronized.locator("header button[aria-label='Settings']").click();
  await synchronized.waitForFunction(
    () => document.querySelector("[data-route]")?.getAttribute("data-route") === "settings",
  );
  await synchronized.locator("header button[aria-label='Settings']").click();
  await synchronized.waitForFunction(
    (expected) =>
      document.querySelector("[data-route]")?.getAttribute("data-route") === "album" &&
      Math.abs((document.querySelector("main")?.scrollTop ?? 0) - expected) <= 1,
    albumScroll,
    { timeout: 5_000 },
  );
  check(
    "clicking Settings again restores the prior route and exact scroll position",
    (await synchronized.locator("main").evaluate((node) => node.scrollTop)) === albumScroll,
    `expected=${String(albumScroll)}`,
  );
  await synchronized.waitForTimeout(450);
  const restorationWrites = await synchronized.evaluate(() => window.__prismScrollWrites ?? -1);
  check(
    "Back performs one instantaneous scroll write instead of visible paginated scrolling",
    restorationWrites === 1,
    `scrollTop writes=${String(restorationWrites)}`,
  );
  await synchronized.close();

  // ---- Album end and originating Library order ------------------------------
  console.log("Album continuation");
  const continuation = await browser.newPage({
    viewport: { width: window_.width ?? 1440, height: window_.height ?? 900 },
  });
  continuation.on("pageerror", (error) => pageErrors.push(error.message));
  await continuation.addInitScript(shellInitScript(CONFIG));
  await continuation.goto(`http://127.0.0.1:${port}/`, { waitUntil: "networkidle" });
  await continuation.waitForSelector("main", { timeout: 15_000 });
  await continuation
    .locator("aside nav[aria-label='Primary'] button[aria-label='Library']")
    .click();
  await continuation.waitForTimeout(300);
  await continuation
    .locator("main div[aria-label='Sort albums']")
    .getByRole("button", { name: /^Year/ })
    .click();
  await continuation
    .locator("main")
    .getByRole("button", { name: "Play Kid A", exact: true })
    .click();
  await continuation.waitForTimeout(400);
  await continuation
    .locator("footer")
    .getByRole("button", { name: "Next track", exact: true })
    .click();
  await continuation.waitForTimeout(250);
  const emitLastTrack = async () => {
    await continuation.evaluate(() => {
      window.__prismEmit("player://event", {
        type: "trackChanged",
        index: 1,
        track: {
          trackId: "al-one-t2",
          title: "Kid A track 2",
          artist: "Radiohead",
          album: "Kid A",
          path: "/music/Radiohead/Kid A/2.flac",
          durationFrames: 10584000,
          sourceFormat: { sampleRate: 44100, channels: 2, sampleFormat: "s16" },
        },
      });
    });
    await continuation.waitForTimeout(350);
  };
  await emitLastTrack();
  check(
    "album playback stops at the album boundary by default",
    (await continuation.evaluate(
      () => window.__prismCalls.filter((call) => call.command === "player_update_queue").length,
    )) === 0,
  );
  await continuation.evaluate(() => {
    window.__prismCalls.length = 0;
  });
  await continuation.locator("main button[aria-label='Open Kid A']").click({ button: "right" });
  await continuation.getByRole("menuitem", { name: "Play album", exact: true }).click();
  await continuation.waitForTimeout(400);
  await continuation
    .locator("footer")
    .getByRole("button", { name: "Next track", exact: true })
    .click();
  await continuation.waitForTimeout(250);
  await continuation.locator("header button[aria-label='Settings']").click();
  await continuation.getByRole("radio", { name: "Continue in library order", exact: true }).click();
  await emitLastTrack();
  const continuedQueue = await continuation.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "player_update_queue").at(-1),
  );
  const continuationQuery = await continuation.evaluate(() =>
    window.__prismCalls.filter((call) => call.command === "library_albums").at(-1),
  );
  check(
    "context-menu album Play continues in full Library order without restarting",
    continuedQueue?.args?.index === 1 &&
      JSON.stringify(continuedQueue?.args?.tracks?.map((track) => track.trackId)) ===
        JSON.stringify(["al-one-t1", "al-one-t2", "al-two-t1", "al-two-t2"]) &&
      continuationQuery?.args?.sort === "year" &&
      (await continuation.evaluate(
        () => window.__prismCalls.filter((call) => call.command === "player_play").length,
      )) === 2,
    JSON.stringify({ queue: continuedQueue?.args ?? {}, query: continuationQuery?.args ?? {} }),
  );
  await continuation.close();

  // ---- A track whose container declares no length ---------------------------
  console.log("Unknown duration");
  const noDuration = await browser.newPage({
    viewport: { width: window_.width ?? 1440, height: window_.height ?? 900 },
  });
  noDuration.on("pageerror", (error) => pageErrors.push(error.message));
  await noDuration.addInitScript(shellInitScript(CONFIG));
  await noDuration.goto(`http://127.0.0.1:${port}/?noduration`, { waitUntil: "networkidle" });
  await noDuration.waitForSelector("main", { timeout: 15_000 });
  await noDuration.getByRole("button", { name: "Library" }).first().click();
  await noDuration.waitForTimeout(300);
  await noDuration.locator("main button[aria-label^='Open ']").first().click();
  await noDuration.waitForTimeout(400);
  await noDuration
    .locator("main")
    .getByRole("button", { name: "Play", exact: true })
    .first()
    .click();
  await noDuration.waitForTimeout(500);
  await noDuration.evaluate(() => {
    window.__prismEmit("player://tick", { positionMs: 30000, durationMs: null, playing: true });
  });
  await noDuration.waitForTimeout(200);
  const unknownSlider = noDuration.locator("footer [role='slider'][aria-label='Seek']").first();
  const unknownValue = await unknownSlider.getAttribute("aria-valuenow");
  check(
    "a track without a declared length still shows its position",
    unknownValue !== null && Number(unknownValue) > 20_000,
    `aria-valuenow=${String(unknownValue)}`,
  );
  check(
    "a track without a declared length still accepts a seek",
    (await unknownSlider.getAttribute("aria-disabled")) !== "true",
  );
  await noDuration.close();

  await browser.close();
  server.close();

  const unexpected = pageErrors.filter((text) => !text.includes("unstubbed command"));
  if (unexpected.length > 0) {
    console.log(`  FAIL  no uncaught page errors — ${unexpected.slice(0, 3).join(" | ")}`);
    failures.push("page errors");
  }

  if (failures.length > 0) {
    console.error(`\nui-behaviour: ${String(failures.length)} assertion(s) failed.`);
    process.exit(1);
  }
  console.log("\nui-behaviour: every interaction answered.");
}

await main();
