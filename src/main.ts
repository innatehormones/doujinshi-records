import { createApp } from "vue"
import { createPinia } from "pinia"
import naive from "naive-ui"
import App from "./App.vue"
import router from "./router"
import { listen } from "@tauri-apps/api/event"
import { useLibraryStore, useRecycleStore, useInboxStore } from "@/stores"
import type { RarErrorEntry } from "@/types/api"
import "./styles/base.css"

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.use(naive)
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
