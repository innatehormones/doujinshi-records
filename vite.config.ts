import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

// Tauri expects a fixed port (1420) and ignores runtime errors.
// https://v2.tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST;
// VITE_SMOKE lets the Playwright smoke pass run on a free port and IPv4
// loopback instead of Tauri's default. Production Tauri runs are unaffected.
const smoke = process.env.VITE_SMOKE === "1";
const port = smoke ? Number(process.env.VITE_SMOKE_PORT ?? 5173) : 1420;
const strictPort = !smoke;

export default defineConfig(async () => ({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port,
    strictPort,
    host: smoke ? "127.0.0.1" : host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
