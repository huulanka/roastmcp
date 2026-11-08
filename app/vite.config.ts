import { fileURLToPath, URL } from "node:url";
import process from "node:process";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

// Set by `tauri dev` when the web view runs on another device (mobile); unused on desktop.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  // Tauri expects a fixed port and prints its own (Rust) errors; keep them visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    proxy: {
      "/api": { target: "http://127.0.0.1:7878", changeOrigin: true, ws: true },
    },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Source maps only for debug builds of the desktop app.
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    chunkSizeWarningLimit: 1024,
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
