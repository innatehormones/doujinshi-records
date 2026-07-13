import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Tauri expects a fixed port (1420) and ignores runtime errors.
// https://v2.tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST;
// VITE_SMOKE lets the Playwright smoke pass run on a free port and IPv4
// loopback instead of Tauri's default. Production Tauri runs are unaffected.
const smoke = process.env.VITE_SMOKE === "1";
const port = smoke ? Number(process.env.VITE_SMOKE_PORT ?? 5173) : 1420;
const strictPort = !smoke;

// 把产物按依赖用途拆 chunk，避免 single chunk bundle 过大触发 Vite
// 500 KB 警告（实际优化首屏缓存：naive-ui 不变可长缓存，app code
// 改了不会让 vendor 失效）。
function manualChunks(id: string): string | undefined {
  if (id.includes("node_modules/naive-ui")) return "naive-ui";
  if (
    id.includes("node_modules/vue/")
    || id.includes("node_modules/@vue/")
    || id.includes("node_modules/pinia")
    || id.includes("node_modules/vue-router")
    || id.includes("node_modules/@vueuse")
  ) {
    return "vue-vendor";
  }
  if (id.includes("node_modules/")) return "vendor";
  return undefined;
}

export default defineConfig(async () => ({
  plugins: [vue(), tailwindcss()],
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
  build: {
    // naive-ui 是 monolithic barrel export（约 1.2 MB minified），用
    // manualChunks 把它单拆出来后无法再分；提高阈值避免误报。其它
    // chunk 都远低于默认 500 KB。
    chunkSizeWarningLimit: 1300,
    rollupOptions: {
      output: {
        manualChunks,
      },
    },
  },
}));
