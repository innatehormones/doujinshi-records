import { defineStore } from "pinia"
import { ref, computed, watch } from "vue"
import { api } from "@/api/tauri"
import { fetchCompare, fetchDetailImages, patchMetadata } from "@/api/http"
import type { FileSummary, SettingsView, ConflictItem, ConflictCompare, ConflictAction, DetailImagesResponse, MetadataPatch, RarErrorEntry } from "@/types/api"

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
  // User input vs debounced version — keep `queryInput` for v-model
  // and let `query` track what the last completed search was. UI binds
  // to `queryInput` via setQuery/getQuery so we can debounce internally
  // without exposing timer details to the view.
  const queryInput = ref("")
  const query = ref("")
  const status = ref<"all" | "viewed" | "not_viewed" | "marked">("all")
  const loading = ref(false)

  let debounceTimer: number | undefined
  watch(queryInput, (v) => {
    if (debounceTimer) clearTimeout(debounceTimer)
    debounceTimer = window.setTimeout(() => {
      query.value = v
    }, 300)
  })

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

  async function fetchDetailImagesFor(id: number): Promise<DetailImagesResponse> {
    return fetchDetailImages(id)
  }

  async function updateMetadataFor(id: number, patch: MetadataPatch) {
    await patchMetadata(id, patch)
    await load()
  }

  /// Top 10 circles (by file count) for the chip bar. Circles with
  /// no `circle` field are skipped — LibraryView only shows real tags.
  const topCircles = computed(() => {
    const counts = new Map<string, number>()
    for (const f of items.value) {
      if (!f.circle) continue
      counts.set(f.circle, (counts.get(f.circle) ?? 0) + 1)
    }
    return Array.from(counts.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10)
      .map(([name, count]) => ({ name, count }))
  })

  function setQuery(v: string) {
    queryInput.value = v
  }

  function getQuery(): string {
    return queryInput.value
  }

  return {
    items, query, status, loading,
    load, markViewed, startDelete, cancelDelete, confirmMoveToWillDelete,
    fetchDetailImagesFor, updateMetadataFor,
    topCircles, setQuery, getQuery,
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
  const rarErrors = ref<RarErrorEntry[]>([])
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

  async function loadCompare(id: number): Promise<ConflictCompare> {
    return fetchCompare(id)
  }

  async function resolveConflict(id: number, action: ConflictAction) {
    await api.resolveConflict(id, action)
    conflicts.value = conflicts.value.filter((c) => c.id !== id)
  }

  /// Drop a RAR error card (user clicked "dismiss"). Keyed by
  /// file_path because the same filename could appear multiple
  /// times across scans.
  function dismissRarError(filePath: string) {
    rarErrors.value = rarErrors.value.filter((e) => e.file_path !== filePath)
  }

  /// Re-invoke the identifier on a previously-failed RAR with the
  /// size gate skipped. Used after the medium-size confirmation
  /// dialog. Refreshes the inbox/library so the resulting row is
  /// visible immediately.
  async function retryExtractLarge(filePath: string) {
    await api.forceExtract(filePath)
    rarErrors.value = rarErrors.value.filter((e) => e.file_path !== filePath)
    await load()
    // 不直接 import useLibraryStore（避免循环依赖），由 main.ts 的
    // library-updated 监听负责刷新；这里是显式触发，避免用户视觉延迟。
    const { useLibraryStore } = await import("@/stores")
    await useLibraryStore().load()
  }

  /// Test/dev helper: prepend a RAR error so the UI can render
  /// without a real RAR. Wired to the `rar-error` Tauri event by
  /// `main.ts`.
  function pushRarError(entry: RarErrorEntry) {
    if (rarErrors.value.some((e) => e.file_path === entry.file_path)) return
    rarErrors.value.push(entry)
  }

  return {
    conflicts, rarErrors, loading,
    load, resolve, loadCompare, resolveConflict,
    dismissRarError, retryExtractLarge, pushRarError,
  }
})
