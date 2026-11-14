import { createReadStream, promises as fs } from "node:fs";
import { homedir } from "node:os";
import { extname, join, relative, isAbsolute } from "node:path";
import { fileURLToPath, URL } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

/**
 * Vite configuration tuned for Tauri v2.
 *
 *  * `port: 1420` + `strictPort` — the dev URL is baked into `tauri.conf.json`.
 *  * HMR host is overridden when developing against a physical device / remote
 *    display (TAURI_DEV_HOST).
 *  * `src-tauri` is excluded from the watcher: Rust rebuilds are Tauri's job and
 *    watching `target/` would melt the file watcher.
 *  * Release builds target the exact WebView2 (Windows) and WebKitGTK (Linux)
 *    baselines that ship with supported OS versions.
 */
const host = process.env["TAURI_DEV_HOST"];

// In development WebKit can reject asset:// images from the HTTP Vite origin.
// Serve only images inside Prism's own cache and data directories, never music
// folders or arbitrary paths. Production keeps Tauri's scoped asset protocol.
function developmentArtwork(): Plugin {
  return {
    name: "prism-development-artwork",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/__prism_artwork", (req, res) => {
        void (async () => {
          try {
            const requested = new URL(req.url ?? "", "http://localhost").searchParams.get("path");
            if (requested === null || !isAbsolute(requested)) throw new Error("invalid path");
            const path = await fs.realpath(requested);
            const home = homedir();
            const roots = [
              join(process.env["XDG_CACHE_HOME"] ?? join(home, ".cache"), "io.prism.player"),
              join(process.env["XDG_DATA_HOME"] ?? join(home, ".local/share"), "io.prism.player"),
            ];
            const realRoots = await Promise.all(
              roots.map((root) => fs.realpath(root).catch(() => "")),
            );
            if (
              !realRoots.some((root) => {
                if (!root) return false;
                const inside = relative(root, path);
                return inside !== "" && !inside.startsWith("..") && !isAbsolute(inside);
              })
            )
              throw new Error("outside artwork scope");
            const mime: Record<string, string> = {
              ".jpg": "image/jpeg",
              ".jpeg": "image/jpeg",
              ".png": "image/png",
              ".webp": "image/webp",
            };
            const type = mime[extname(path).toLowerCase()];
            const stat = await fs.stat(path);
            if (type === undefined || !stat.isFile() || stat.size > 20 * 1024 * 1024)
              throw new Error("invalid image");
            res.setHeader("Content-Type", type);
            res.setHeader("Cache-Control", "private, max-age=3600");
            const stream = createReadStream(path);
            stream.on("error", () => res.destroy());
            stream.pipe(res);
          } catch {
            res.statusCode = 404;
            res.end();
          }
        })();
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), developmentArtwork()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    ...(host ? { hmr: { protocol: "ws" as const, host, port: 1421 } } : {}),
    watch: {
      ignored: ["**/src-tauri/**", "**/.git/**"],
    },
  },
  build: {
    target: process.env["TAURI_ENV_PLATFORM"] === "windows" ? "chrome105" : "safari13",
    minify: process.env["TAURI_ENV_DEBUG"] ? false : "esbuild",
    sourcemap: Boolean(process.env["TAURI_ENV_DEBUG"]),
    reportCompressedSize: false,
    cssMinify: "lightningcss",
    rollupOptions: {
      output: {
        manualChunks: {
          react: ["react", "react-dom"],
          motion: ["framer-motion"],
          icons: ["lucide-react"],
        },
      },
    },
  },
});
