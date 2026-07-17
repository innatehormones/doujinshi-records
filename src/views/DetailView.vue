<script setup lang="ts">
/// DetailView：列表（Library）/文件回收站点进来的详情页。
///
/// 缩略图渲染管线见 `composables/useThumbnailPipeline.ts`：
/// - IntersectionObserver 触发 `pipeline.request(index)`，仅调度可见 cell
/// - 已缓存：直挂后端图（命中 webp，未命中走原图 mime）
/// - 未缓存：Worker 转 800px webp → PUT 落 LRU → blob URL 展示
///
/// 缩略图视觉防闪烁见 CSS：骨架 div 永远在底层，`<img>` `opacity:0` 挂载
/// 到 onLoad 期间不可见，onLoad 后切 `.thumb-img-loaded` 淡入。

import { ref, onMounted, onUnmounted, computed, watch, h } from "vue"
import { useRoute, useRouter } from "vue-router"
import {
  NCard, NSpace, NButton, NSpin, NInput, NSelect, NEmpty, NAlert, useMessage,
} from "naive-ui"
import { ArrowLeft, Star, Archive, Trash2, RotateCcw } from "@lucide/vue"
import { useLibraryStore, useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"
import { fetchReparse } from "@/api/http"
import type { FileSummary, MetadataPatch, DetailImage, ReparseResult } from "@/types/api"
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
const reparsing = ref(false)
/// 最近一次「重新解析」的结果；显示在 meta 卡片顶部 alert 里，
/// 让用户看清是哪条文件名、解析出了什么。
const reparseNotice = ref<ReparseResult | null>(null)

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
const editNote = ref("")
const editRating = ref<number | null>(null)

const ratingOptions = [
  { label: "1 星", value: 1 },
  { label: "2 星", value: 2 },
  { label: "3 星", value: 3 },
  { label: "4 星", value: 4 },
  { label: "5 星", value: 5 },
]

function renderRatingStars(option: { value: number, label: string }) {
  const v = (editRating.value ?? 0) as number
  return h(
    "span",
    { style: "display: inline-flex; gap: 1px; align-items: center;" },
    Array.from({ length: option.value }, (_, i) => {
      const filled = v >= i + 1
      return h(Star, {
        size: 14,
        "stroke-width": 1.5,
        fill: "currentColor",
        style: filled
          ? "color: var(--color-phosphor-green);"
          : "color: var(--color-phosphor-green); opacity: 0.4;",
      })
    }),
  )
}

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
    /// 元数据走 FileSummary 直接拿回，不再本地保留空字符串
    editSeries.value = f.series ?? ""
    editTranslator.value = f.translator ?? ""
    editNote.value = f.note ?? ""
    editRating.value = f.rating ?? null
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
      note: editNote.value || null,
      rating: editRating.value,
    }
    await store.updateMetadataFor(id.value, patch)
    message.success("已保存")
    reparseNotice.value = null
  } catch (e) {
    message.error(String(e))
  } finally {
    saving.value = false
  }
}

/// 重新跑 filename_parser 解析已入库文件名。**不写 DB**——结果只回填
/// 表单 + 顶部 alert，等用户点「保存」才落库。`fetchReparse` 走 HTTP
/// Bearer 鉴权路径（与 settings.apiBase 同源），与 store 刷新一致。
async function reparse() {
  reparsing.value = true
  try {
    const r = await fetchReparse(id.value)
    reparseNotice.value = r
    editTitle.value = r.title
    editCircle.value = r.circle ?? ""
    editSeries.value = r.series ?? ""
    editTranslator.value = r.translator ?? ""
    message.info("已重新解析表单，请检查后点「保存」")
  } catch (e) {
    message.error(String(e))
  } finally {
    reparsing.value = false
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

function statusLabel(): string {
  if (!file.value) return ""
  switch (file.value.status) {
    case "recycle": return "文件回收站"
    case "archived": return "归档"
    case "deleted": return "已删除"
    default: return "已入库"
  }
}

function statusTagType(): "default" | "primary" | "info" | "success" | "warning" | "error" {
  if (!file.value) return "default"
  switch (file.value.status) {
    case "in_library": return "success"
    case "archived": return "info"
    case "recycle": return "warning"
    case "deleted": return "error"
  }
}

function fileStateTitle(s: string): string {
  switch (s) {
    case "present": return "文件存在"
    case "missing": return "文件已丢失"
    case "absent_confirmed": return "文件已销毁"
    default: return ""
  }
}

function fileStateTagType(s: string): "default" | "primary" | "info" | "success" | "warning" | "error" {
  switch (s) {
    case "present": return "success"
    case "missing": return "error"
    case "absent_confirmed": return "error"
    default: return "default"
  }
}
</script>

<template>
  <div class="page">
    <header class="flex items-baseline gap-3 border-b border-border pb-4">
      <n-button text @click="router.back()">
        <template #icon>
          <ArrowLeft :size="16" :stroke-width="1.8" />
        </template>
        返回
      </n-button>
      <h1 class="text-heading-sm font-medium text-snow tracking-body">
        {{ file?.title ?? `文件 #${id}` }}
      </h1>
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
          <n-alert
            v-else-if="file.file_state !== 'present'"
            :type="file.file_state === 'absent_confirmed' ? 'error' : 'warning'"
            :title="fileStateTitle(file.file_state)"
            class="mb-3"
          >
            <template v-if="file.file_state === 'missing'">
              文件已不在预期路径。预览不可用；元数据可正常查看和修改。
            </template>
            <template v-else-if="file.file_state === 'absent_confirmed'">
              文件已被销毁。记录仍保留，可在 Library 恢复为入库态。
            </template>
          </n-alert>
          <n-empty
            v-else-if="images.length === 0"
            description="zip 内无图片"
          />
          <div v-else class="album-grid">
          <div class="grid-content">
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
              <div class="px-badge">P{{ idx + 1 }}</div>
            </div>
          </div>
          </div>
        </n-card>

        <n-card title="元数据" class="meta-pane">
          <n-space vertical>
            <n-alert
              v-if="reparseNotice"
              type="info"
              title="已重新解析（未保存）"
              closable
              @close="reparseNotice = null"
              class="mb-1"
            >
              <div class="reparse-detail">
                <div>文件名：{{ reparseNotice.filename }}</div>
                <div>标题：{{ reparseNotice.title }}</div>
                <div>社团：{{ reparseNotice.circle || '—' }}</div>
                <div>系列：{{ reparseNotice.series || '—' }}</div>
                <div>翻译：{{ reparseNotice.translator || '—' }}</div>
                <div class="reparse-hint">表单已回填上述值。检查无误后点「保存」生效。</div>
              </div>
            </n-alert>
            <n-input v-model:value="editTitle" placeholder="标题" />
            <n-input v-model:value="editCircle" placeholder="社团 (circle)" />
            <n-input v-model:value="editSeries" placeholder="系列 (series)" />
            <n-input v-model:value="editTranslator" placeholder="翻译 (translator)" />
            <n-input v-model:value="editNote" type="textarea" placeholder="备注" />
            <n-select
              v-model:value="editRating"
              :options="ratingOptions"
              :render-label="renderRatingStars"
              placeholder="评分"
              clearable
            />
            <n-space>
              <n-button type="primary" @click="save" :loading="saving">保存</n-button>
              <n-button ghost @click="reparse" :loading="reparsing">重新解析元数据</n-button>
            </n-space>
          </n-space>
        </n-card>

        <n-card title="操作" class="action-pane">
          <n-space vertical>
            <div class="file-meta mono">
              <div>作品入库序号：{{ file.id }}</div>
              <div>入库文件哈希：{{ file.hash.slice(0, 16) }}…</div>
              <div>入库文件名称：{{ file.filename }}</div>
              <div class="mt-0.5">
                  业务状态：
                  <n-tag size="small" :type="statusTagType()">
                    {{ statusLabel() }}
                  </n-tag>
              </div>
              <div class="mt-0.5">
                文件状态：
                <n-tag size="small" :type="fileStateTagType(file.file_state)">
                  {{ fileStateTitle(file.file_state) }}
                </n-tag>
              </div>

            </div>
            <div class="action-divider" />
            <n-space>
              <n-button
                v-if="file.status === 'in_library'"
                type="primary"
                ghost
                @click="archive"
              >
                <template #icon>
                  <Archive :size="14" :stroke-width="1.8" />
                </template>
                归档
              </n-button>
              <n-button
                v-if="file.status === 'in_library'"
                type="warning"
                ghost
                @click="moveToWillDelete"
              >
                <template #icon>
                  <Trash2 :size="14" :stroke-width="1.8" />
                </template>
                移到回收站
              </n-button>
              <n-button
                v-if="file.status === 'recycle' || file.status === 'archived' || file.status === 'deleted'"
                type="primary"
                ghost
                @click="restore"
              >
                <template #icon>
                  <RotateCcw :size="14" :stroke-width="1.8" />
                </template>
                {{ file.status === 'deleted' ? '恢复为入库' : '取回到已入库' }}
              </n-button>
            </n-space>
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
  max-height: calc(100vh - 160px);
  min-height: 0;
  height: 100%;
  overflow-y: auto;
}
.grid-content {
  padding-right: 4px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: 8px;
  /* 详情页是单页内容，预览 grid 占满主区可视高度，溢出滚动而非占满屏。
     calc 100vh 减去 header+padding+card title 高度：避免在大窗口下被切到底。 */
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
/* 缩略图右下角页码角标：固定白字半透黑底。
   同人志图多为黑白漫画，白底为主，黑色只占线条/阴影，黑底白字在
   任意区域都能读。彩色图也适用。 */
.px-badge {
  position: absolute;
  right: 6px;
  bottom: 6px;
  padding: 2px 6px;
  border-radius: 4px;
  font-family: var(--font-mono);
  font-size: 11px;
  letter-spacing: 0.05em;
  pointer-events: none;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  backdrop-filter: blur(2px);
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
.action-divider {
  height: 1px;
  background: var(--color-border);
  margin: 4px 0;
}
.file-meta {
  font-size: 11px;
  color: var(--color-smoke);
  word-break: break-all;
}
.reparse-detail {
  display: grid;
  gap: 2px;
  font-size: 12px;
  color: var(--color-snow);
}
.reparse-hint {
  margin-top: 4px;
  font-size: 11px;
  color: var(--color-smoke);
}
</style>
