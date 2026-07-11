<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import {
  NCard, NSpace, NButton, NSpin, NCarousel, NInput, NSelect, NEmpty, NAlert, useMessage,
} from "naive-ui"
import { useLibraryStore } from "@/stores"
import { api } from "@/api/tauri"
import type { FileSummary, MetadataPatch, DetailImage } from "@/types/api"

const route = useRoute()
const router = useRouter()
const store = useLibraryStore()
const message = useMessage()

const id = computed(() => Number(route.params.id))
const file = ref<FileSummary | null>(null)
const images = ref<DetailImage[]>([])
const zipMissing = ref(false)
const loading = ref(false)
const saving = ref(false)

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

async function startDelete() {
  if (!file.value) return
  try {
    await store.startDelete(id.value)
    file.value.marked_for_delete = true
    message.success("已标记删除（移入回收站请回 Library 操作）")
  } catch (e) {
    message.error(String(e))
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
          <n-carousel v-else show-arrow autoplay>
            <img
              v-for="img in images"
              :key="img.name"
              :src="img.data_url"
              :alt="img.name"
            />
          </n-carousel>
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
              :disabled="file.marked_for_delete"
              @click="startDelete"
            >
              {{ file.marked_for_delete ? "已标记删除" : "标记删除" }}
            </n-button>
            <div class="status-row">
              <n-tag v-if="file.viewed" type="success">已看</n-tag>
              <n-tag v-if="file.marked_for_delete" type="warning">已标记删除</n-tag>
              <n-tag v-if="file.physically_deleted" type="error">已删除</n-tag>
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
img {
  max-width: 100%;
  max-height: 80vh;
  object-fit: contain;
}
.status-row { display: flex; gap: 6px; flex-wrap: wrap; }
.file-meta {
  font-size: 11px;
  color: var(--color-smoke);
  word-break: break-all;
}
</style>