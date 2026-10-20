import { fileURLToPath, URL } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/**
 * Build configuration for the performance harness.
 *
 * Deliberately separate from `vite.config.ts`: the harness must never be a route
 * of the shipped application, and the shipped bundle must never grow because a
 * benchmark imports a synthetic catalogue. `perf.html` is the only entry.
 */
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  build: {
    outDir: ".perf-dist",
    emptyOutDir: true,
    target: "chrome105",
    reportCompressedSize: false,
    rollupOptions: {
      input: fileURLToPath(new URL("./perf.html", import.meta.url)),
    },
  },
  server: {
    port: 1422,
    strictPort: true,
    host: "0.0.0.0",
    fs: { allow: [fileURLToPath(new URL(".", import.meta.url))] },
  },
});
