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
    albumArtist: album.albumArtist,
    artistId: album.artistId,
    isCompilation: false,
    discCount: 1,
    trackCount: album.trackCount,
    durationMs: album.durationMs,
    releaseType: "album",
    coverId: null,
    coverPath: null,
    coverUrl: null,
    availableTracks: album.trackCount,
    maxSampleRate: 44100,
    maxBitDepth: 16,
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
    genre: "Test",
    year: album.year,
    trackNo: index,
    discNo: 1,
    format: "flac",
    codecProfile: null,
    bitDepth: 16,
    sampleRate: 44100,
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
  const live = { volumeDb: 0, muted: false, positionFrames: 0, state: "playing" };
  const snapshot = () => ({
    ...status(),
    state: live.state,
    positionFrames: live.positionFrames,
    volumeDb: live.volumeDb,
    muted: live.muted,
  });

  return {
    get_app_info: {
      name: "Prism",
      version: "0.1.3",
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
    },
    library_albums: page(albums.map(albumRow)),
    library_artists: page(albums.map(artistRow)),
    library_album_detail: (args) => {
      const album = albums.find((candidate) => candidate.id === args.id) || albums[0];
      return { album: albumRow(album), tracks: page([1, 2].map((i) => trackRow(album, i))) };
    },
    library_artist_detail: (args) => {
      const album = albums.find((candidate) => candidate.artistId === args.id) || albums[0];
      return { artist: artistRow(album), albums: page([albumRow(album)]) };
    },
    library_search: page([]),
    library_global_search: {
      query: "",
      matchExpression: null,
      tracks: page([]),
      albums: [],
      artists: [],
      tookMs: 1,
    },
    library_facets: {
      genres: [],
      formats: [],
      decades: [],
      sampleRates: [],
      bitDepths: [],
      years: [],
    },
    library_track_cover: { key: "", localPath: null, remoteUrl: null, width: null, height: null },
    library_album_cover: { key: "", localPath: null, remoteUrl: null, width: null, height: null },
    library_cover_palette: { accent: "#7dd3fc", secondary: "#38bdf8", surface: "#0b1220" },
    library_note_play: true,
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
    player_play: () => {
      live.state = "playing";
      live.positionFrames = 0;
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
    discord_status: {
      state: "disabled",
      connected: false,
      clientId: null,
      detail: null,
      activity: null,
      lastError: null,
    },
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
        migrationVersion: 4,
        appliedMigrations: 4,
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
