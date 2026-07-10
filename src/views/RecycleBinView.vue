<script setup lang="ts">
import { onMounted, ref } from "vue"
import {
  NCard, NSpace, NButton, NTag, NSpin, NEmpty, NGrid, NGi, NImage, NDivider, useMessage,
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
  <div>
    <div class="page-header">
      <h1>回收站</h1>
      <span class="count">{{ store.present.length + store.gone.length }} 条</span>
    </div>
  <n-spin :show="store.loading">
    <n-card title="待删除文件（仍在硬盘上）">
      <p style="color: #aaa; font-size: 12px">
        这里的文件已经移出已识别库，但仍占用硬盘空间。「永久删除」会把文件从硬盘移除；「还原」会让文件回到库内。两种操作都会保留数据记录。
      </p>
    </n-card>

    <n-empty
      v-if="!store.loading && store.present.length === 0"
      description="回收站为空。"
      style="margin-top: 24px"
    />

    <n-grid x-gap="12" y-gap="12" cols="6" style="margin-top: 16px">
      <n-gi v-for="f in store.present" :key="f.id">
        <n-card hoverable class="file-card">
          <template #cover>
            <n-image
              v-if="f.cover_url"
              :src="coverUrl(f)"
              object-fit="cover"
              style="width: 100%; height: 200px"
              preview-disabled
            />
            <div v-else class="no-cover">暂无封面</div>
          </template>
          <div class="title-line">
            <span class="title">{{ f.title }}</span>
          </div>
          <div class="meta">
            <span>{{ fmtSize(f.size_bytes) }}</span>
          </div>
          <n-space size="small" style="margin-top: 8px">
            <n-button size="tiny" type="primary" @click="askRestore(f)">
              还原
            </n-button>
            <n-button size="tiny" type="error" @click="askDelete(f)">
              永久删除
            </n-button>
          </n-space>
        </n-card>
      </n-gi>
    </n-grid>

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
.file-card { width: 200px; }
.no-cover {
  width: 100%;
  height: 200px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #888;
  background: #222;
}
.title-line { display: flex; align-items: center; gap: 6px; }
.title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.meta {
  display: flex;
  justify-content: space-between;
  color: #aaa;
  font-size: 12px;
  margin-top: 4px;
}
</style>
