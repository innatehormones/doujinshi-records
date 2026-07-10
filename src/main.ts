import { createApp } from "vue"
import { createPinia } from "pinia"
import naive from "naive-ui"
import App from "./App.vue"
import router from "./router"
import { listen } from "@tauri-apps/api/event"
import { useLibraryStore, useRecycleStore, useInboxStore } from "@/stores"
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
