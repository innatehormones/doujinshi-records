<script setup lang="ts">
/// DetailView：列表（Library）/回收站点进来的详情页。
///
/// 缩略图渲染管线见 `composables/useThumbnailPipeline.ts`：
/// - IntersectionObserver 触发 `pipeline.request(index)`，仅调度可见 cell
/// - 已缓存：直挂后端图（命中 webp，未命中走原图 mime）
/// - 未缓存：Worker 转 800px webp → PUT 落 LRU → blob URL 展示
///
/// 缩略图视觉防闪烁见 CSS：骨架 div 永远在底层，`<img>` `opacity:0` 挂载
/// 到 onLoad 期间不可见，onLoad 后切 `.thumb-img-loaded` 淡入。

import { ref, onMounted, onUnmounted, computed, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import {
  NCard, NSpace, NButton, NSpin, NInput, NSelect, NEmpty, NAlert, useMessage,
} from "naive-ui"
import { useLibraryStore, useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"
import type { FileSummary, MetadataPatch, DetailImage } from "@/types/api"
import FullscreenPreview from "@/components/FullscreenPreview.vue"
import { useThumbnailPipeline } from "@/composables/useThumbnailPipeline"
import { usePreviewState } from "@/composables/usePreviewState"

/// IntersectionObserver 预读上下各 ~2 行（grid item 高度 200px + gap 8px）。
const IO_ROOT_MARGIN = "420px 0px"

const route = useRoute()
const router = useRouter()
const store = useLibraryStore()
const settings = useSettingsStore()
const message = useMessage()

const id = computed(() => Number(route.params.id))
const file = ref<FileSummary | null>(null)
const images = ref<DetailImage[]>([])
const zipMissing = ref(false)
const loading = ref(false)
const saving = ref(false)

const pipeline = useThumbnailPipeline({
  fileId: id,
  apiBase: computed(() => settings.apiBase),
  images,
})
const preview = usePreviewState()

/// ---- IntersectionObserver：cell 进入视口才 request ----
const observers = new Map<number, IntersectionObserver>()

function attachObserver(cell: HTMLElement | null, index: number) {
  if (!cell) return
  const prev = observers.get(index)
  if (prev) prev.disconnect()
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) pipeline.request(index)
      }
    },
    { rootMargin: IO_ROOT_MARGIN, threshold: 0.01 },
  )
  io.observe(cell)
  observers.set(index, io)
}

function detachAllObservers() {
  for (const io of observers.values()) io.disconnect()
  observers.clear()
}

/// 切走文件：detach 所有 observer，下一波由新 images 触发的 template ref 重建。
watch(id, () => {
  detachAllObservers()
  load()
})

/// ---- 元数据编辑表单（保存走 store.updateMetadataFor → PATCH） ----
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
    /// 这些字段不在 FileSummary 里（保留为本地态，等保存后下次进页面再拉新值）
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
onUnmounted(detachAllObservers)

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
  <div class="page">
    <header class="flex items-baseline gap-4">
      <n-button text @click="router.back()">← 返回</n-button>
      <h1 class="text-heading-sm font-medium text-snow tracking-body">
        {{ file?.title ?? `文件 #${id}` }}
      </h1>
      <span
        v-if="file"
        class="font-mono text-caption text-smoke tracking-[0.1em]"
      >
        id {{ file.id }}
      </span>
    </header>
    <n-spin :show="loading || saving">
      <div v-if="file" class="detail-grid">
        <n-card title="图片预览" class="preview-pane">
          <n-alert
            v-if="zipMissing"
            type="warning"
            title="压缩包已不在磁盘"
            class="mb-3"
          />
          <n-empty
            v-else-if="images.length === 0"
            description="zip 内无图片"
          />
          <div v-else class="album-grid">
            <div
              v-for="(img, idx) in images"
              :key="img.name"
              :ref="(el) => attachObserver(el as HTMLElement | null, idx)"
              class="thumb-cell"
              @click="preview.show(idx)"
            >
              <div class="thumb-skeleton" />
              <img
                v-if="pipeline.thumbSrc.value[idx]"
                :src="pipeline.thumbSrc.value[idx]!"
                :alt="img.name"
                class="thumb-img"
                :class="{ 'thumb-img-loaded': pipeline.loaded.value.has(idx) }"
                loading="lazy"
                decoding="async"
                @load="pipeline.markLoaded(idx)"
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

    <FullscreenPreview
      v-if="preview.open.value"
      :file-id="id"
      :images="images"
      :initial-index="preview.index.value"
      :api-base="settings.apiBase"
      @close="preview.close()"
      @change="preview.setIndex"
    />
  </div>
</template>

<style scoped>
.detail-grid {
  display: grid;
  gap: 16px;
  grid-template-columns: minmax(0, 1fr) 320px;
  grid-template-rows: auto auto;
  grid-template-areas:
    "preview meta"
    "preview action";
}
.preview-pane { grid-area: preview; min-width: 0; }
.meta-pane    { grid-area: meta; }
.action-pane  { grid-area: action; }
@media (max-width: 1100px) {
  .detail-grid {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto auto;
    grid-template-areas:
      "preview"
      "meta"
      "action";
  }
}
.album-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: 8px;
  /* 详情页是单页内容，预览 grid 占满主区可视高度，溢出滚动而非占满屏。
     calc 100vh 减去 header+padding+card title 高度：避免在大窗口下被切到底。 */
  max-height: calc(100vh - 200px);
  min-height: 0;
  overflow-y: auto;
  padding: 4px;
}
/* 缩略图 cell：骨架永远占底，<img> 覆盖在骨架上解码期间 opacity:0，
   onLoad 切 .thumb-img-loaded 才淡入——避免"灰→黑→图"三段闪烁。 */
.thumb-cell {
  position: relative;
  width: 100%;
  aspect-ratio: 4 / 5;
  overflow: hidden;
  border-radius: 4px;
  background: var(--color-obsidian-deep);
  cursor: zoom-in;
}
.thumb-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  position: absolute;
  inset: 0;
  opacity: 0;
  transition: opacity 0.18s ease-out;
}
.thumb-img-loaded { opacity: 1; }
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