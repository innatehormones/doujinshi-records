// scripts/screenshot.mjs
// Drives Vite dev server with Playwright. Stubs `window.__TAURI_INTERNALS__`
// so the four views render with mock data (frontend uses Tauri IPC, not HTTP).

import { chromium } from "playwright";
import { readFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const EVIDENCE = path.join(ROOT, "docs/superpowers/evidence");

// Library lives at "/" in router.ts, not "/library"
const VIEWS = [
  { path: "/",         name: "library" },
  { path: "/inbox",    name: "inbox" },
  { path: "/recycle",  name: "recycle" },
  { path: "/settings", name: "settings" },
];

const fixture = JSON.parse(
  await readFile(
    path.join(EVIDENCE, "fixtures", "mock_library.json"),
    "utf8",
  ),
);

// Mock data for the other store commands the views call in onMounted.
const SETTINGS = {
  resources_dir: "/tmp/doujinshi-records/resources",
  inbox_dir: "/tmp/doujinshi-records/resources/doujinshi/inbox",
  identified_dir: "/tmp/doujinshi-records/resources/doujinshi/identified",
  will_delete_dir: "/tmp/doujinshi-records/resources/doujinshi/will_delete",
  covers_dir: "/tmp/doujinshi-records/resources/covers",
  api_url: "http://127.0.0.1:5180",
  scanner_watching: true,
};

const CONFLICTS = [
  {
    id: 7,
    a_file_id: 3,
    a_title: "Megurine Luka Live",
    b_filename: "(C93) [Piapro] Megurine Luka Live.zip",
    b_file_path:
      "/tmp/doujinshi-records/resources/doujinshi/inbox/(C93) [Piapro] Megurine Luka Live.zip",
    created_at: "2026-07-09T12:00:00Z",
  },
];

const browser = await chromium.launch();
const ctx = await browser.newContext({
  viewport: { width: 1280, height: 800 },
});

// Pass data as JSON because the page-side handler must be self-contained.
await ctx.addInitScript(
  ({ library, settings, conflicts }) => {
    window.__TAURI_INTERNALS__ = {
      invoke: (cmd, _args, _options) => {
        switch (cmd) {
          case "list_library":
            return Promise.resolve(library.items);
          case "get_settings":
            return Promise.resolve(settings);
          case "list_recycle":
            return Promise.resolve([library.items.slice(2, 3), []]);
          case "list_conflicts":
            return Promise.resolve(conflicts);
          case "manual_scan":
            return Promise.resolve(0);
          default:
            return Promise.resolve(null);
        }
      },
      transformCallback: () => 0,
      ipc: { postMessage: () => 0 },
      metadata: {
        currentWindowLabel: "main",
        currentWebviewWindowLabel: "main",
      },
      plugins: {},
      runtime: { isTauri: false },
      unregisterCallback: () => {},
    };
  },
  { library: fixture, settings: SETTINGS, conflicts: CONFLICTS },
);

const page = await ctx.newPage();
const consoleErrors = [];
page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));
page.on("console", (msg) => {
  if (msg.type() === "error") consoleErrors.push(`console.error: ${msg.text()}`);
});

await mkdir(EVIDENCE, { recursive: true });
const devUrl =
  process.env.VITE_SMOKE_URL ??
  `http://127.0.0.1:${process.env.VITE_SMOKE_PORT ?? 5173}`;

for (const v of VIEWS) {
  await page.goto(devUrl + v.path, { waitUntil: "networkidle" });
  await page
    .waitForFunction(
      () =>
        document.body && document.body.innerText.includes("同人志档案"),
      null,
      { timeout: 8000 },
    )
    .catch(() => {});
  await page.waitForTimeout(500);
  const file = path.join(EVIDENCE, `${v.name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  console.log("wrote", file);
}

await browser.close();

const fatal = consoleErrors.filter(
  (e) =>
    !e.includes("__TAURI_INTERNALS__") &&
    !e.includes("Tauri") &&
    !e.includes("hljs"),
);
if (fatal.length) {
  console.error("Console errors:\n" + fatal.join("\n"));
  process.exitCode = 1;
}
