<script setup lang="ts">
import { onMounted, ref } from "vue"
import {
  NCard, NSpace, NButton, NTag, NSpin, NEmpty, NDivider, useMessage,
} from "naive-ui"
import { useRecycleStore, useSettingsStore } from "@/stores"
import PermanentDeleteDialog from "@/components/PermanentDeleteDialog.vue"
import RestoreDialog from "@/components/RestoreDialog.vue"
import type { FileSummary } from "@/types/api"

const store = useRecycleStore()
const settings = useSettingsStore()
const message = useMessage()

const target = ref<FileSummary | null>(null)
const showDelete = ref(false)
const showRestore = ref(false)

onMounted(async () => {
  await settings.load()
  await store.load()
})

function askDelete(f: FileSummary) {
  target.value = f
  showDelete.value = true
}

function askRestore(f: FileSummary) {
  target.value = f
  showRestore.value = true
}

async function confirmDelete() {
  if (!target.value) return
  const id = target.value.id
  const title = target.value.title
  showDelete.value = false
  target.value = null
  try {
    await store.permanentDelete(id)
    message.success(`「${title}」已从硬盘永久删除。`)
  } catch (e: unknown) {
    message.error(String(e))
  }
}

async function confirmRestore() {
  if (!target.value) return
  const id = target.value.id
  const title = target.value.title
  showRestore.value = false
  target.value = null
  try {
    await store.restore(id)
    message.success(`「${title}」已还原回已识别库。`)
  } catch (e: unknown) {
    message.error(String(e))
  }
}

function coverUrl(f: FileSummary): string {
  if (!f.cover_url) return ""
  return settings.apiBase + f.cover_url
}

function fmtSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B"
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB"
  return (bytes / 1024 / 1024).toFixed(1) + " MB"
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h1>回收站</h1>
      <span class="count mono">{{ store.present.length + store.gone.length }} 条</span>
    </header>
  <n-spin :show="store.loading">
    <n-card title="待删除文件（仍在硬盘上）">
      <p style="color: #aaa; font-size: 12px">
        这里的文件已经移出已识别库，但仍占用硬盘空间。「永久删除」会把文件从硬盘移除；「还原」会让文件回到库内。两种操作都会保留数据记录。
      </p>
    </n-card>

    <n-empty
      v-if="!store.loading && store.present.length === 0"
      description="回收站为空。"
    />

    <div v-if="store.present.length > 0" class="card-grid">
      <article v-for="f in store.present" :key="f.id" class="recycle-card">
        <div class="recycle-cover">
          <img v-if="f.cover_url" :src="coverUrl(f)" alt="" />
          <div v-else class="no-cover">暂无封面</div>
        </div>
        <div class="recycle-body">
          <div class="title-line">
            <span class="title">{{ f.title }}</span>
          </div>
          <div class="meta">
            <span>{{ fmtSize(f.size_bytes) }}</span>
          </div>
          <n-space size="small" class="recycle-actions">
            <n-button size="tiny" type="primary" @click="askRestore(f)">还原</n-button>
            <n-button size="tiny" type="error" @click="askDelete(f)">永久删除</n-button>
          </n-space>
        </div>
      </article>
    </div>

    <n-divider />

    <n-card title="已从硬盘删除">
      <p style="color: #aaa; font-size: 12px">
        数据记录保留供搜索 / 外部工具使用。这里的文件无法再还原。
      </p>
      <n-empty
        v-if="store.gone.length === 0"
        description="暂无已删除记录。"
        size="small"
        style="margin-top: 8px"
      />
      <n-list v-else bordered>
        <n-list-item v-for="f in store.gone" :key="f.id">
          <n-thing>
            <template #header>
              <n-tag size="small" type="error">已删除</n-tag>
              <span style="margin-left: 8px">{{ f.title }}</span>
            </template>
            <template #description>
              <span style="color: #888; font-size: 11px">
                {{ fmtSize(f.size_bytes) }} · {{ f.hash.slice(0, 12) }}…
              </span>
            </template>
          </n-thing>
        </n-list-item>
      </n-list>
    </n-card>

    <permanent-delete-dialog
      :show="showDelete"
      :title="target?.title ?? ''"
      @cancel="showDelete = false; target = null"
      @confirm="confirmDelete"
    />
    <restore-dialog
      :show="showRestore"
      :title="target?.title ?? ''"
      @cancel="showRestore = false; target = null"
      @confirm="confirmRestore"
    />
  </n-spin>
  </div>
</template>

<style scoped>
.page-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--spacing-16);
}
.page-header h1 {
  font-size: var(--text-heading-sm);
  font-weight: var(--font-weight-medium);
  color: var(--color-snow);
  letter-spacing: var(--tracking-body);
}
.page-header .count {
  font-size: var(--text-caption);
  color: var(--color-smoke);
  letter-spacing: 0.1em;
}
.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 16px;
}
.recycle-card {
  background: var(--surface-card);
  border: 1px solid var(--surface-border);
  border-radius: var(--radius-cards);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.recycle-cover {
  position: relative;
  aspect-ratio: 3 / 4;
  background: var(--color-obsidian-deep);
  overflow: hidden;
  border-bottom: 1px solid var(--surface-border);
}
.recycle-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.no-cover {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-smoke);
  font-family: var(--font-mono);
  font-size: var(--text-caption);
  letter-spacing: 0.1em;
  text-transform: uppercase;
}
.recycle-body {
  padding: var(--spacing-16);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-8);
}
.title-line { display: flex; align-items: center; gap: 6px; }
.title {
  flex: 1;
  color: var(--color-snow);
  font-size: var(--text-body-sm);
  font-weight: var(--font-weight-medium);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.meta {
  color: var(--color-smoke);
  font-size: var(--text-caption);
  min-height: 16px;
}
.recycle-actions { margin-top: 4px; }
</style>
