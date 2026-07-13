import { createApp } from "vue"
import { createPinia } from "pinia"
import naive from "naive-ui"
import App from "./App.vue"
import router from "./router"
import { listen } from "@tauri-apps/api/event"
import { useLibraryStore, useRecycleStore, useInboxStore, useThemeStore } from "@/stores"
import type { RarErrorEntry } from "@/types/api"
import "./styles/base.css"

const pinia = createPinia()
const app = createApp(App)
app.use(pinia)
app.use(router)
app.use(naive)

// 主题必须在 mount 前 init()——避免首次渲染先走 :root 默认 dark，
// 再被 data-theme='light' 切到亮色，导致闪一下。
useThemeStore().init()
app.mount("#app")

// Live-update: backend scanner emits "library-updated" after every scan.
listen("library-updated", () => {
  const lib = useLibraryStore()
  const rec = useRecycleStore()
  const inb = useInboxStore()
  lib.load().catch(() => {})
  rec.load().catch(() => {})
  inb.load().catch(() => {})
})

// RAR errors surface as one-shot events (not part of the regular
// conflicts list). Forward them to the inbox store so the user sees
// actionable cards. The backend only emits this for .rar files.
listen<RarErrorEntry>("rar-error", (event) => {
  const inb = useInboxStore()
  inb.pushRarError(event.payload)
})
