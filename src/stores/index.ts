import { defineStore } from "pinia"
import { ref, computed, watch } from "vue"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { api } from "@/api/tauri"
import { fetchCompare, fetchDetailImages, patchMetadata } from "@/api/http"
import type {
  FileSummary,
  SettingsView,
  ConflictItem,
  ConflictCompare,
  ConflictAction,
  DetailImagesResponse,
  MetadataPatch,
  RarErrorEntry,
  DirtyEntry,
  CircleCount,
  ScanStatus,
} from "@/types/api"

/// Library 页分页大小。第一页 <= 24 时隐藏分页器（用户多看一两个就是
/// "我库就这么大"，分页器只是噪音）。24 是经验值——sider 64px 加内容区
/// 至少能塞 3 列，每列 ≤ 8 行（3:4 比例）的总和正好 ~24。
export const LIBRARY_PAGE_SIZE = 24
export const RECYCLE_PAGE_SIZE = 24
export const INBOX_PAGE_SIZE = 50
export const DIRTY_PAGE_SIZE = 50

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

/// 主题状态：dark / light。持久化到 localStorage，启动时读不到则
/// 跟随系统偏好，再读不到则默认 dark（与现有设计对齐）。
///
/// 切换流程：setMode() 写 mode + localStorage + 切 document.documentElement
/// 的 data-theme 属性，tailwind.css 的 :root[data-theme='light'] 立刻接管
/// 所有 CSS 变量。Naive UI 组件色由 App.vue 配合
/// `buildThemeOverrides(isDark)` 同步生效。
///
/// 模式：用户偏好持久化（"light" / "dark" / "system"）。
/// "system" 模式订阅 prefers-color-scheme，系统切换时自动跟随。
export type ThemeMode = "system" | "light" | "dark"
export const useThemeStore = defineStore("theme", () => {
  const STORAGE_KEY = "theme"
  const mode = ref<ThemeMode>("system")
  /// 系统偏好单独存一个 ref，监听 change 事件刷新——和 mode 解耦。
  const systemPrefersDark = ref(true)
  let initialized = false

  if (typeof window !== "undefined") {
    const mql = window.matchMedia?.("(prefers-color-scheme: dark)")
    if (mql) {
      systemPrefersDark.value = mql.matches
      mql.addEventListener("change", (e) => {
        systemPrefersDark.value = e.matches
      })
    }
  }

  /// 暴露给外部的"当前是不是深色"——system 模式跟随系统，否则按用户选择。
  const isDark = computed(() =>
    mode.value === "system" ? systemPrefersDark.value : mode.value === "dark",
  )

  function applyToDom() {
    if (typeof document === "undefined") return
    document.documentElement.dataset.theme = isDark.value ? "dark" : "light"
  }

  /// mode 或系统偏好变化都触发 DOM 更新——mode 是用户切换，系统是自动跟随。
  watch([mode, systemPrefersDark], () => applyToDom())

  function setMode(next: ThemeMode) {
    mode.value = next
    try {
      localStorage.setItem(STORAGE_KEY, next)
    } catch {}
  }

  function init() {
    if (initialized) return
    initialized = true
    try {
      const saved = localStorage.getItem(STORAGE_KEY)
      if (saved === "light" || saved === "dark" || saved === "system") {
        mode.value = saved
      }
    } catch {}
    applyToDom()
  }

  return { mode, isDark, setMode, init }
})

export const useLibraryStore = defineStore("library", () => {
  const items = ref<FileSummary[]>([])
  const total = ref(0)
  const page = ref(1)
  // User input vs debounced version — keep `queryInput` for v-model
  // and let `query` track what the last completed search was. UI binds
  // to `queryInput` via setQuery/getQuery so we can debounce internally
  // without exposing timer details to the view.
  const queryInput = ref("")
  const query = ref("")
  /// "看 / 标记" 过滤：all / viewed / not_viewed / marked。
  const status = ref<"all" | "viewed" | "not_viewed" | "marked">("all")
  /// V4 业务 status 过滤。
  /// - `active` = 排除 recycle + deleted（UI 默认值，对应 spec 的"主列表"）
  /// - `all` = 不限
  /// - 其他 = 精确匹配
  /// `active` 传给后端时是 `undefined`，后端只认合法的 4 个 status 值；
  /// recycle + deleted 的隐藏由 `LibraryView` 渲染时过滤（避免分页
  /// 拿到的 24 条里有 5 条是 deleted，剩 19 条导致分页数偏小）。
  const statusFilter = ref<
    "active" | "all" | "in_library" | "archived" | "recycle" | "deleted"
  >("active")
  const loading = ref(false)
  /// 顶部社团 chip——单独调用 top_circles，不从当前页 items 聚合（聚合只算
  /// "当前页 top" 误导用户）。全表 GROUP BY 排序，每次 load 与翻页各自刷。
  const topCircles = ref<CircleCount[]>([])

  const totalPages = computed(() =>
    Math.max(1, Math.ceil(total.value / LIBRARY_PAGE_SIZE)),
  )
  /// 仅 1 页时隐藏分页器（按用户偏好："第一页少于等于 24 时不显示分页器"）。
  const showPager = computed(() => totalPages.value > 1)
  /// `active` 模式只显示 in_library + archived，其他 status 的行
  /// 仍由后端返回（让 total 反映真实数量），但被这条 computed 过滤。
  const visibleItems = computed(() => {
    if (statusFilter.value !== "active") return items.value
    return items.value.filter(
      (f) => f.status === "in_library" || f.status === "archived",
    )
  })

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
      const offset = (page.value - 1) * LIBRARY_PAGE_SIZE
      const statusArg =
        statusFilter.value === "all" || statusFilter.value === "active"
          ? undefined
          : statusFilter.value
      const [pageRes, circlesRes] = await Promise.all([
        api.listLibrary(
          query.value || undefined,
          status.value === "all" ? undefined : status.value,
          statusArg,
          LIBRARY_PAGE_SIZE,
          offset,
        ),
        // 全表社团 top 不带任何过滤——chip 是库级别的导航，列表查询变化
        // 不应改变 chip 集合（用户期望"我长期跟的几个社团"固定）。
        api.topCircles(10),
      ])
      items.value = pageRes.items
      total.value = pageRes.total
      topCircles.value = circlesRes
    } finally {
      loading.value = false
    }
  }

  async function gotoPage(p: number) {
    const target = Math.min(Math.max(1, p), totalPages.value)
    if (target === page.value) return
    page.value = target
    await load()
  }

  async function archive(id: number) {
    await api.archive(id)
    await load()
  }

  async function restore(id: number) {
    await api.restore(id)
    await load()
  }

  async function markForDelete(id: number) {
    await api.markForDelete(id)
    await load()
  }

  async function fetchDetailImagesFor(id: number): Promise<DetailImagesResponse> {
    return fetchDetailImages(id)
  }

  async function updateMetadataFor(id: number, patch: MetadataPatch) {
    await patchMetadata(id, patch)
    await load()
  }

  function setQuery(v: string) {
    queryInput.value = v
  }

  function getQuery(): string {
    return queryInput.value
  }

  return {
    items, visibleItems, total, page, totalPages, showPager, query,
    status, statusFilter, loading,
    topCircles,
    load, gotoPage,
    archive, restore, markForDelete,
    fetchDetailImagesFor, updateMetadataFor,
    setQuery, getQuery,
  }
})

export const useRecycleStore = defineStore("recycle", () => {
  const present = ref<FileSummary[]>([])
  const presentTotal = ref(0)
  const presentPage = ref(1)
  const loading = ref(false)

  const presentTotalPages = computed(() =>
    Math.max(1, Math.ceil(presentTotal.value / RECYCLE_PAGE_SIZE)),
  )
  const showPresentPager = computed(() => presentTotalPages.value > 1)

  async function load() {
    loading.value = true
    try {
      const presentOffset = (presentPage.value - 1) * RECYCLE_PAGE_SIZE
      const res = await api.listRecycle(RECYCLE_PAGE_SIZE, presentOffset)
      present.value = res.present.items
      presentTotal.value = res.present.total
    } finally {
      loading.value = false
    }
  }

  async function gotoPresentPage(p: number) {
    const target = Math.min(Math.max(1, p), presentTotalPages.value)
    if (target === presentPage.value) return
    presentPage.value = target
    await load()
  }

  async function permanentDelete(id: number) {
    await api.permanentDelete(id)
    // 后端把 status 推到 'deleted' + file_state='absent_confirmed'，
    // 跟本页签（status='recycle' + file_state='present'）过滤不匹配。
    // 本地直接从 present 里删，避免下次 load 之前还显示在「待删除文件」。
    present.value = present.value.filter((x) => x.id !== id)
    presentTotal.value = Math.max(0, presentTotal.value - 1)
  }

  async function restore(id: number) {
    await api.restore(id)
    present.value = present.value.filter((f) => f.id !== id)
  }

  return {
    present, presentTotal, presentPage, presentTotalPages, showPresentPager,
    loading, load, gotoPresentPage,
    permanentDelete, restore,
  }
})

export const useDirtyStore = defineStore("dirty", () => {
  const entries = ref<DirtyEntry[]>([])
  const total = ref(0)
  const page = ref(1)
  const loading = ref(false)

  const totalPages = computed(() =>
    Math.max(1, Math.ceil(total.value / DIRTY_PAGE_SIZE)),
  )
  const showPager = computed(() => totalPages.value > 1)

  async function load() {
    loading.value = true
    try {
      const offset = (page.value - 1) * DIRTY_PAGE_SIZE
      const res = await api.listDirty(DIRTY_PAGE_SIZE, offset)
      entries.value = res.items
      total.value = res.total
    } finally {
      loading.value = false
    }
  }

  async function gotoPage(p: number) {
    const target = Math.min(Math.max(1, p), totalPages.value)
    if (target === page.value) return
    page.value = target
    await load()
  }

  /// 重新入库一条 orphan_file 脏数据条目：mover-only，把文件搬到 inbox/
  /// 让 scanner::Scanner 异步接管入库流程，dirty_data 行立即软删。
  /// 失败由调用方通过 message.error 上报。
  async function reingest(id: number) {
    await api.reingestDirtyEntry(id)
    await load()
  }

  return { entries, total, page, totalPages, showPager, loading, load, gotoPage, reingest }
})

export const useInboxStore = defineStore("inbox", () => {
  const conflicts = ref<ConflictItem[]>([])
  const total = ref(0)
  const page = ref(1)
  const rarErrors = ref<RarErrorEntry[]>([])
  const loading = ref(false)

  const totalPages = computed(() =>
    Math.max(1, Math.ceil(total.value / INBOX_PAGE_SIZE)),
  )
  const showPager = computed(() => totalPages.value > 1)

  async function load() {
    loading.value = true
    try {
      const offset = (page.value - 1) * INBOX_PAGE_SIZE
      const res = await api.listConflicts(INBOX_PAGE_SIZE, offset)
      conflicts.value = res.items
      total.value = res.total
    } finally {
      loading.value = false
    }
  }

  async function gotoPage(p: number) {
    const target = Math.min(Math.max(1, p), totalPages.value)
    if (target === page.value) return
    page.value = target
    await load()
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
    conflicts, total, page, totalPages, showPager, rarErrors, loading,
    load, gotoPage, resolve, loadCompare, resolveConflict,
    dismissRarError, retryExtractLarge, pushRarError,
  }
})

/// 扫描进度浮窗：订阅后端 `scanner-status` 事件 + 启动时拉一次
/// 快照，避免"启动时已完成的一批"看不到。`dismissed` 让用户点 X 关掉
/// 后保持隐藏，直到下一次 `is_scanning: true` 重置。`visible` 是浮窗
/// 渲染条件：`total > 0`（扫过文件） && !dismissed。
export const useScanStatusStore = defineStore("scan-status", () => {
  const status = ref<ScanStatus | null>(null)
  const dismissed = ref(false)
  const visible = computed(
    () => (status.value?.total ?? 0) > 0 && !dismissed.value,
  )

  let unlisten: UnlistenFn | null = null

  function onEvent(next: ScanStatus) {
    status.value = next
    // 新一轮扫描开始时复位 dismissed —— 用户主动关过的浮窗只对"那一次"
    // 生效，避免被永久压制。
    if (next.is_scanning && next.processed === 0) {
      dismissed.value = false
    }
  }

  async function init() {
    if (unlisten) return
    try {
      status.value = await api.getScanStatus()
    } catch {
      // 启动早期 Tauri 可能还没好；忽略即可，等首次事件进来。
    }
    unlisten = await listen<ScanStatus>("scanner-status", (e) => onEvent(e.payload))
  }

  function dismiss() {
    dismissed.value = true
  }

  return { status, visible, init, dismiss }
})
