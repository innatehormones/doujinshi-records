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
    <header class="flex items-baseline justify-between gap-4">
      <h1 class="text-heading-sm font-medium text-snow tracking-body">回收站</h1>
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">
        {{ store.present.length + store.gone.length }} 条
      </span>
    </header>
    <n-spin :show="store.loading">
      <n-card title="待删除文件（仍在硬盘上）">
        <p class="text-caption text-silver-mist">
          这里的文件已经移出已识别库，但仍占用硬盘空间。「永久删除」会把文件从硬盘移除；「还原」会让文件回到库内。两种操作都会保留数据记录。
        </p>
      </n-card>

      <n-empty
        v-if="!store.loading && store.present.length === 0"
        description="回收站为空。"
      />

      <div v-if="store.present.length > 0" class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
        <article
          v-for="f in store.present"
          :key="f.id"
          class="flex flex-col overflow-hidden rounded-cards border border-border bg-card"
        >
          <div class="relative aspect-[3/4] overflow-hidden border-b border-border bg-obsidian-deep">
            <img v-if="f.cover_url" :src="coverUrl(f)" alt="" class="size-full object-cover" />
            <div
              v-else
              class="flex size-full items-center justify-center font-mono text-caption uppercase tracking-[0.1em] text-smoke"
            >
              暂无封面
            </div>
          </div>
          <div class="flex flex-col gap-2 p-4">
            <div class="flex items-center gap-[6px]">
              <span class="flex-1 truncate text-body-sm font-medium text-snow">{{ f.title }}</span>
            </div>
            <div class="min-h-4 text-caption text-smoke">
              <span>{{ fmtSize(f.size_bytes) }}</span>
            </div>
            <n-space size="small" class="mt-1">
              <n-button size="tiny" type="primary" @click="askRestore(f)">还原</n-button>
              <n-button size="tiny" type="error" @click="askDelete(f)">永久删除</n-button>
            </n-space>
          </div>
        </article>
      </div>

      <n-divider />

      <n-card title="已从硬盘删除">
        <p class="text-caption text-silver-mist">
          数据记录保留供搜索 / 外部工具使用。这里的文件无法再还原。
        </p>
        <n-empty
          v-if="store.gone.length === 0"
          description="暂无已删除记录。"
          size="small"
          class="mt-2"
        />
        <n-list v-else bordered>
          <n-list-item v-for="f in store.gone" :key="f.id">
            <n-thing>
              <template #header>
                <n-tag size="small" type="error">已删除</n-tag>
                <span class="ml-2">{{ f.title }}</span>
              </template>
              <template #description>
                <span class="font-mono text-[11px] text-smoke">
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
