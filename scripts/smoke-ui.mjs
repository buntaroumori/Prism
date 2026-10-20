#!/usr/bin/env node
/**
 * Boot smoke test — does the built bundle actually render inside the shell?
 *
 * The unit tests, the type checker and the 120 FPS audit all passed while the
 * shipped application showed a black window, because none of them reproduced
 * the *runtime* the WebView provides. This script does, and it is the only test
 * that would have caught that regression:
 *
 *   * the production bundle from `dist/` (not the dev server),
 *   * the exact CSP of `tauri.conf.json` (not "no policy"),
 *   * `freezePrototype` applied the same way Tauri applies it, read from the
 *     configuration, so the test follows the app instead of assuming,
 *   * a `__TAURI_INTERNALS__` stub that answers every command with a structured
 *     failure, which is what a fresh install looks like before a library exists,
 *   * the window sized like `tauri.conf.json` says.
 *
 * A pass means React committed a tree and the boot watchdog never fired.
 *
 * Usage:
 *   node scripts/smoke-ui.mjs            # build, serve, assert
 *   node scripts/smoke-ui.mjs --no-build # reuse dist/
 *   node scripts/smoke-ui.mjs --headed   # watch it
 *
 * Exit codes: 0 rendered · 1 did not render · 2 tooling.
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
    {
      cwd: ROOT,
      encoding: "utf8",
    },
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

/** Resolve Playwright the same way the perf audit does. */
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
  console.error("smoke-ui: Playwright is not installed — run `pnpm install` first");
  process.exit(2);
}

function buildIfNeeded() {
  if (args.has("--no-build")) {
    if (!existsSync(join(DIST, "index.html"))) {
      console.error("smoke-ui: dist/index.html is missing — run without --no-build");
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

/**
 * Everything Tauri does to the page before the first line of app code runs.
 *
 * `freezePrototype` is the one that matters: `Object.freeze(Object.prototype)`
 * makes any library that assigns a shadowing property (`obj.toString = …`, which
 * zod does at module scope) throw a `TypeError` in strict mode, so the entry
 * module never finishes evaluating and the window stays black.
 */
function shellInitScript(config) {
  const security = config.app?.security ?? {};
  return `(() => {
    ${
      security.freezePrototype
        ? "Object.freeze(Object.prototype);"
        : "/* freezePrototype disabled in tauri.conf.json */"
    }
    const fail = (command) => {
      const error = new Error("no backend in the smoke test: " + command);
      error.code = "unavailable";
      return Promise.reject(error);
    };
    window.__TAURI_INTERNALS__ = {
      invoke: (command) => fail(command),
      transformCallback: (callback) => callback,
      convertFileSrc: (path) => "asset://localhost/" + encodeURIComponent(path),
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    };
  })();`;
}

async function main() {
  buildIfNeeded();
  const { chromium } = await loadPlaywright();

  const csp = CONFIG.app?.security?.csp ?? "";
  const indexHtml = await readFile(join(DIST, "index.html"), "utf8");

  /**
   * Tauri hashes every inline `<script>` of the embedded HTML and appends the
   * hash to `script-src` before serving the page (`inject_script_hashes` in
   * tauri-codegen). Reproducing it here keeps the boot watchdog executable in
   * the smoke test instead of silently blocked.
   */
  function cspWithInlineHashes(policy, html) {
    const inlineScripts = [
      ...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g),
    ].map((match) => match[1]);
    if (inlineScripts.length === 0 || policy === "") {
      return policy;
    }
    const hashes = inlineScripts.map(
      (script) => `'sha256-${createHash("sha256").update(script, "utf8").digest("base64")}'`,
    );
    return policy.replace(/script-src([^;]*)/, (directive) => `${directive} ${hashes.join(" ")}`);
  }

  const effectiveCsp = cspWithInlineHashes(csp, indexHtml);
  const window_ = CONFIG.app?.windows?.[0] ?? {};
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
  const page = await browser.newPage({
    viewport: { width: window_.width ?? 1440, height: window_.height ?? 900 },
  });

  const consoleErrors = [];
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") {
      consoleErrors.push(message.text());
    }
  });

  await page.addInitScript(shellInitScript(CONFIG));
  await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: "networkidle" });

  let rendered = false;
  for (let attempt = 0; attempt < 30 && !rendered; attempt += 1) {
    rendered = await page.evaluate(
      () => (document.getElementById("root")?.childElementCount ?? 0) > 0,
    );
    if (!rendered) {
      await page.waitForTimeout(100);
    }
  }

  const watchdog = await page.evaluate(() => document.getElementById("boot-failure") !== null);
  const visibleText = await page.evaluate(() => document.getElementById("root")?.textContent ?? "");
  if (args.has("--headed")) {
    await page.screenshot({ path: join(ROOT, "dist", "smoke.png") });
  }

  await browser.close();
  server.close();

  // Rejections produced by the IPC stub are the point: a fresh install has no
  // library, and the shell has to render its empty state instead of throwing.
  const expected = (text) => text.includes("no backend in the smoke test");
  const unexpectedErrors = [...pageErrors, ...consoleErrors].filter((text) => !expected(text));

  const lines = [
    `shell          ${CONFIG.productName} ${CONFIG.version}`,
    `csp            ${csp === "" ? "(none configured)" : "applied"}`,
    `freezePrototype ${String(CONFIG.app?.security?.freezePrototype ?? false)}`,
    `rendered       ${String(rendered)}`,
    `watchdog       ${watchdog ? "FIRED" : "quiet"}`,
    `text           ${visibleText.replace(/\s+/g, " ").trim().slice(0, 90)}`,
    `backend errors ${pageErrors.filter(expected).length} (expected: stubbed IPC)`,
    `unexpected     ${unexpectedErrors.length === 0 ? "none" : unexpectedErrors.slice(0, 3).join(" | ")}`,
  ];
  console.log(lines.join("\n"));

  if (!rendered || watchdog || unexpectedErrors.length > 0) {
    console.error(
      unexpectedErrors.length > 0
        ? "\nsmoke-ui: the shell rendered but reported unexpected errors."
        : "\nsmoke-ui: the bundle did not mount in a Tauri-shaped runtime. This is the black-window bug.",
    );
    process.exit(1);
  }
}

main().catch((error) => {
  console.error("smoke-ui: failed —", error);
  process.exit(2);
});
