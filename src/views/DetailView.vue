<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import {
  NCard, NSpace, NButton, NSpin, NImage, NInput, NSelect, NEmpty, NAlert, useMessage,
} from "naive-ui"
import { useLibraryStore, useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"
import { putImageThumb } from "@/api/http"
import type { FileSummary, MetadataPatch, DetailImage } from "@/types/api"

const PREVIEW_MAX_EDGE = 1000
const THUMB_UPLOAD_CONCURRENCY = 1

type ThumbJob = {
  img: DetailImage
  index: number
  el: HTMLImageElement
  fileId: number
}

const route = useRoute()
const router = useRouter()
const store = useLibraryStore()
const settings = useSettingsStore()
const message = useMessage()

// 每张图的加载状态。img-props 的 onLoad/onError 触发 set 更新。
const loadedSet = ref(new Set<string>())
const failedSet = ref(new Set<string>())
const thumbUploadedSet = ref(new Set<string>())
const thumbQueue: ThumbJob[] = []
let activeThumbJobs = 0
const loadedCount = computed(() => loadedSet.value.size)
function markLoaded(name: string) {
  if (loadedSet.value.has(name)) return
  const next = new Set(loadedSet.value)
  next.add(name)
  loadedSet.value = next
}
function markFailed(name: string) {
  if (failedSet.value.has(name)) return
  const next = new Set(failedSet.value)
  next.add(name)
  failedSet.value = next
}
function handleImageLoad(img: DetailImage, index: number, event: Event) {
  markLoaded(img.name)
  const el = event.target
  if (!(el instanceof HTMLImageElement)) return
  enqueueThumb(img, index, el)
}
function enqueueThumb(img: DetailImage, index: number, el: HTMLImageElement) {
  if (img.thumb_cached) return
  const fileId = id.value
  const key = `${fileId}:${img.url}`
  if (thumbUploadedSet.value.has(key)) return
  const next = new Set(thumbUploadedSet.value)
  next.add(key)
  thumbUploadedSet.value = next
  thumbQueue.push({ img, index, el, fileId })
  drainThumbQueue()
}
function drainThumbQueue() {
  while (activeThumbJobs < THUMB_UPLOAD_CONCURRENCY && thumbQueue.length > 0) {
    const job = thumbQueue.shift()
    if (!job) return
    activeThumbJobs += 1
    void runThumbJob(job)
  }
}
function waitForIdle(): Promise<void> {
  return new Promise((resolve) => {
    if ("requestIdleCallback" in window) {
      window.requestIdleCallback(() => resolve(), { timeout: 1000 })
      return
    }
    globalThis.setTimeout(resolve, 32)
  })
}
async function runThumbJob(job: ThumbJob) {
  try {
    if (job.fileId !== id.value) return
    await waitForIdle()
    if (job.fileId !== id.value) return
    await uploadThumb(job)
  } catch (e) {
    console.warn("thumbnail conversion failed", e)
  } finally {
    activeThumbJobs -= 1
    drainThumbQueue()
  }
}
async function uploadThumb(job: ThumbJob) {
  const { index, el, fileId } = job
  if (!el.naturalWidth || !el.naturalHeight) return
  if (el.naturalWidth <= PREVIEW_MAX_EDGE && el.naturalHeight <= PREVIEW_MAX_EDGE) return

  const scale = Math.min(PREVIEW_MAX_EDGE / el.naturalWidth, PREVIEW_MAX_EDGE / el.naturalHeight)
  const width = Math.max(1, Math.round(el.naturalWidth * scale))
  const height = Math.max(1, Math.round(el.naturalHeight * scale))
  const canvas = document.createElement("canvas")
  canvas.width = width
  canvas.height = height
  const ctx = canvas.getContext("2d")
  if (!ctx) return
  ctx.drawImage(el, 0, 0, width, height)
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/webp", 0.7))
  if (!blob || blob.type !== "image/webp" || fileId !== id.value) return
  const resp = await putImageThumb(fileId, index, blob)
  if (!resp.ok) {
    console.warn("thumbnail upload failed", resp.status, await resp.text())
    return
  }
  if (images.value[index]?.url === job.img.url) {
    images.value[index].thumb_cached = true
  }
}
// 切换文件时清空进度状态（watch 声明放在 images 之后，避免 TDZ）。
const id = computed(() => Number(route.params.id))
const file = ref<FileSummary | null>(null)
const images = ref<DetailImage[]>([])
const zipMissing = ref(false)
const loading = ref(false)
const saving = ref(false)
watch(images, () => {
  loadedSet.value = new Set()
  failedSet.value = new Set()
  thumbUploadedSet.value = new Set()
  thumbQueue.length = 0
})

// 编辑表单（编辑后保存通过 store.updateMetadataFor → PATCH）
const editTitle = ref("")
const editCircle = ref("")
const editSeries = ref("")
const editTranslator = ref("")
const editVersion = ref("")
const editNote = ref("")
const editRating = ref<number | null>(null)

const ratingOptions = [
  { label: "★", value: 1 },
  { label: "★★", value: 2 },
  { label: "★★★", value: 3 },
  { label: "★★★★", value: 4 },
  { label: "★★★★★", value: 5 },
]

async function load() {
  loading.value = true
  try {
    let f = store.items.find((x) => x.id === id.value) ?? null
    if (!f) {
      f = await api.getById(id.value)
    }
    file.value = f
    editTitle.value = f.title
    editCircle.value = f.circle ?? ""
    // 这些字段不在 FileSummary 里（保留为本地态，等保存后下次进页面再拉新值）
    editSeries.value = ""
    editTranslator.value = ""
    editVersion.value = ""
    editNote.value = ""
    editRating.value = null
    const r = await store.fetchDetailImagesFor(id.value)
    images.value = r.images
    zipMissing.value = r.zip_missing
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(id, load)

async function save() {
  saving.value = true
  try {
    const patch: MetadataPatch = {
      title: editTitle.value,
      circle: editCircle.value || null,
      series: editSeries.value || null,
      translator: editTranslator.value || null,
      version: editVersion.value || null,
      note: editNote.value || null,
      rating: editRating.value,
    }
    await store.updateMetadataFor(id.value, patch)
    message.success("已保存")
  } catch (e) {
    message.error(String(e))
  } finally {
    saving.value = false
  }
}

async function markViewed() {
  if (!file.value) return
  try {
    await api.markViewed(id.value)
    file.value.viewed = true
    message.success("已标记已看")
  } catch (e) {
    message.error(String(e))
  }
}

async function archive() {
  if (!file.value) return
  try {
    await store.archive(id.value)
    file.value = await api.getById(id.value)
    message.success("已归档")
  } catch (e) {
    message.error(String(e))
  }
}

async function restore() {
  if (!file.value) return
  try {
    await store.restore(id.value)
    file.value = await api.getById(id.value)
    message.success("已取回到已入库")
  } catch (e) {
    message.error(String(e))
  }
}

async function moveToWillDelete() {
  if (!file.value) return
  try {
    await store.markForDelete(id.value)
    file.value = await api.getById(id.value)
    message.success("已移到回收站")
  } catch (e) {
    message.error(String(e))
  }
}

function locationLabel(): string {
  if (!file.value) return ""
  switch (file.value.current_location) {
    case "will_delete": return "回收站"
    case "archived": return "归档"
    case "inbox": return "待入库"
    default: return "已入库"
  }
}
</script>

<template>
  <div>
    <div class="page-header">
      <n-button text @click="router.back()">← 返回</n-button>
      <h1>{{ file?.title ?? `文件 #${id}` }}</h1>
    </div>
    <n-spin :show="loading || saving">
      <div v-if="file" class="detail-grid">
        <n-card title="图片预览" class="preview-pane">
          <n-alert
            v-if="zipMissing"
            type="warning"
            title="压缩包已不在磁盘"
            style="margin-bottom: 12px"
          />
          <n-empty
            v-else-if="images.length === 0"
            description="zip 内无图片"
          />
          <div v-else class="album-grid">
            <div class="album-progress">
              <n-progress
                type="line"
                :percentage="images.length === 0 ? 0 : Math.round((loadedCount * 100) / images.length)"
                :show-indicator="false"
                :height="6"
                style="margin-bottom: 6px;"
              />
              <span class="album-progress-text">
                已加载 {{ loadedCount }} / {{ images.length }}
                <template v-if="failedSet.size > 0">· 失败 {{ failedSet.size }}</template>
              </span>
            </div>
            <div v-for="(img, idx) in images" :key="img.name" class="thumb-cell">
              <div
                v-if="!loadedSet.has(img.name) && !failedSet.has(img.name)"
                class="thumb-skeleton"
              />
              <n-image
                :src="settings.apiBase + img.url"
                :alt="img.name"
                width="160"
                height="200"
                object-fit="cover"
                show-toolbar
                :img-props="{
                  style: 'cursor: zoom-in; width: 160px; height: 200px; object-fit: cover; display: block;',
                  loading: 'lazy',
                  crossorigin: 'anonymous',
                  onLoad: (event: Event) => { void handleImageLoad(img, idx, event) },
                  onError: () => markFailed(img.name),
                }"
                class="album-thumb"
              />
            </div>
          </div>
        </n-card>

        <n-card title="元数据" class="meta-pane">
          <n-space vertical>
            <n-input v-model:value="editTitle" placeholder="标题" />
            <n-input v-model:value="editCircle" placeholder="社团 (circle)" />
            <n-input v-model:value="editSeries" placeholder="系列 (series)" />
            <n-input v-model:value="editTranslator" placeholder="翻译 (translator)" />
            <n-input v-model:value="editVersion" placeholder="版本 (version)" />
            <n-input v-model:value="editNote" type="textarea" placeholder="备注" />
            <n-select
              v-model:value="editRating"
              :options="ratingOptions"
              placeholder="评分"
              clearable
            />
            <n-button type="primary" @click="save" :loading="saving">保存</n-button>
          </n-space>
        </n-card>

        <n-card title="操作" class="action-pane">
          <n-space vertical>
            <n-button :disabled="file.viewed" @click="markViewed">
              {{ file.viewed ? "已查看" : "标记已看" }}
            </n-button>
            <n-button
              v-if="file.current_location === 'identified'"
              @click="archive"
            >
              归档
            </n-button>
            <n-button
              v-if="file.current_location === 'identified'"
              @click="moveToWillDelete"
            >
              移到回收站
            </n-button>
            <n-button
              v-if="file.current_location === 'will_delete' || file.current_location === 'archived'"
              @click="restore"
            >
              取回到已入库
            </n-button>
            <div class="status-row">
              <n-tag v-if="file.viewed" type="success">已看</n-tag>
              <n-tag :type="file.current_location === 'will_delete' ? 'warning' : (file.current_location === 'archived' ? 'info' : 'default')">
                {{ locationLabel() }}
              </n-tag>
              <n-tag v-if="!file.has_physical_file" type="error">文件丢失</n-tag>
            </div>
            <div class="file-meta mono">
              <div>id: {{ file.id }}</div>
              <div>hash: {{ file.hash.slice(0, 16) }}…</div>
              <div>{{ file.current_location }}</div>
            </div>
          </n-space>
        </n-card>
      </div>
    </n-spin>
  </div>
</template>

<style scoped>
.page-header {
  display: flex;
  align-items: baseline;
  gap: 16px;
  margin-bottom: var(--spacing-24);
  padding-bottom: var(--spacing-16);
  border-bottom: 1px solid var(--surface-border);
}
.page-header h1 {
  font-size: var(--text-heading-sm);
  font-weight: var(--font-weight-medium);
  color: var(--color-snow);
  letter-spacing: var(--tracking-body);
}
.detail-grid {
  display: grid;
  grid-template-columns: 3fr 2fr;
  grid-template-rows: auto auto;
  gap: 16px;
}
.preview-pane { grid-row: span 2; }
.album-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: 8px;
  max-height: 75vh;
  overflow-y: auto;
  padding: 4px;
}
.album-thumb {
  border-radius: 4px;
  overflow: hidden;
  background: var(--surface-muted, transparent);
}
.album-progress {
  grid-column: 1 / -1;
  margin-bottom: 8px;
  font-size: 12px;
  color: var(--n-text-color-3, #888);
}
.album-progress-text { margin-left: 2px; }
.thumb-cell {
  position: relative;
  width: 160px;
  height: 200px;
  overflow: hidden;
  border-radius: 4px;
}
.thumb-skeleton {
  position: absolute;
  inset: 0;
  background: linear-gradient(
    90deg,
    rgba(220, 220, 220, 0.6) 0%,
    rgba(200, 200, 200, 0.9) 50%,
    rgba(220, 220, 220, 0.6) 100%
  );
  background-size: 200% 100%;
  animation: skeleton-shimmer 1.5s ease-in-out infinite;
}
@keyframes skeleton-shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}
.status-row { display: flex; gap: 6px; flex-wrap: wrap; }
.file-meta {
  font-size: 11px;
  color: var(--color-smoke);
  word-break: break-all;
}
</style>