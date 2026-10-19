import { fileURLToPath, URL } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

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

export default defineConfig({
  plugins: [react(), tailwindcss()],
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
