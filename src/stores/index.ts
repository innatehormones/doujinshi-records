import { defineStore } from "pinia"
import { ref, computed } from "vue"
import { api } from "@/api/tauri"
import type { FileSummary, SettingsView, ConflictItem } from "@/types/api"

export const useSettingsStore = defineStore("settings", () => {
  const data = ref<SettingsView | null>(null)
  const apiBase = computed(() => {
    if (!data.value) return ""
    return data.value.api_url
  })
  async function load() {
    data.value = await api.getSettings()
  }
  return { data, apiBase, load }
})

export const useLibraryStore = defineStore("library", () => {
  const items = ref<FileSummary[]>([])
  const query = ref("")
  const status = ref<"all" | "viewed" | "not_viewed" | "marked">("all")
  const loading = ref(false)

  async function load() {
    loading.value = true
    try {
      items.value = await api.listLibrary(
        query.value || undefined,
        status.value === "all" ? undefined : status.value
      )
    } finally {
      loading.value = false
    }
  }

  async function markViewed(id: number) {
    await api.markViewed(id)
    const f = items.value.find((f) => f.id === id)
    if (f) f.viewed = true
  }

  async function startDelete(id: number) {
    await api.markForDelete(id)
    const f = items.value.find((f) => f.id === id)
    if (f) f.marked_for_delete = true
  }

  async function cancelDelete(id: number) {
    await api.unmarkForDelete(id)
    const f = items.value.find((f) => f.id === id)
    if (f) f.marked_for_delete = false
  }

  async function confirmMoveToWillDelete(id: number) {
    await api.moveToWillDelete(id)
    items.value = items.value.filter((f) => f.id !== id)
  }

  return {
    items, query, status, loading,
    load, markViewed, startDelete, cancelDelete, confirmMoveToWillDelete,
  }
})

export const useRecycleStore = defineStore("recycle", () => {
  const present = ref<FileSummary[]>([])
  const gone = ref<FileSummary[]>([])
  const loading = ref(false)

  async function load() {
    loading.value = true
    try {
      const [p, g] = await api.listRecycle()
      present.value = p
      gone.value = g
    } finally {
      loading.value = false
    }
  }

  async function permanentDelete(id: number) {
    await api.permanentDelete(id)
    const f = present.value.find((f) => f.id === id)
    if (f) {
      f.physically_deleted = true
      gone.value.push(f)
      present.value = present.value.filter((x) => x.id !== id)
    }
  }

  async function restore(id: number) {
    await api.restoreFromRecycle(id)
    present.value = present.value.filter((f) => f.id !== id)
  }

  return { present, gone, loading, load, permanentDelete, restore }
})

export const useInboxStore = defineStore("inbox", () => {
  const conflicts = ref<ConflictItem[]>([])
  const loading = ref(false)

  async function load() {
    loading.value = true
    try {
      conflicts.value = await api.listConflicts()
    } finally {
      loading.value = false
    }
  }

  async function resolve(id: number) {
    await api.resolveConflict(id)
    conflicts.value = conflicts.value.filter((c) => c.id !== id)
  }

  return { conflicts, loading, load, resolve }
})
