<script setup lang="ts">
import { onMounted } from "vue"
import { NList, NListItem, NEmpty, NSpin, NTag } from "naive-ui"
import { useDirtyStore } from "@/stores"

const store = useDirtyStore()
onMounted(() => store.load())

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B"
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB"
  return (bytes / 1024 / 1024).toFixed(1) + " MB"
}

function dirLabel(dir: string): string {
  switch (dir) {
    case "identified": return "已入库"
    case "will_delete": return "回收站"
    case "archived": return "归档"
    default: return dir
  }
}
</script>

<template>
  <div class="page">
    <header class="flex items-baseline justify-between gap-4">
      <h1 class="text-heading-sm font-medium text-snow tracking-body">脏数据</h1>
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">
        {{ store.entries.length }} 条
      </span>
    </header>
    <p class="mb-4 text-silver-mist">
      启动扫描发现：这些文件位于已入库 / 回收站 / 归档目录，但数据库无对应行。
      V3 不提供自动处理——手动清理或重新入库。
    </p>
    <n-spin :show="store.loading">
      <n-empty
        v-if="!store.loading && store.entries.length === 0"
        description="无脏数据。"
      />
      <n-list bordered>
        <n-list-item v-for="e in store.entries" :key="e.id">
          <div class="flex w-full flex-col gap-1">
            <div class="flex items-center gap-2">
              <n-tag size="small">{{ dirLabel(e.detected_dir) }}</n-tag>
              <span class="font-mono text-caption text-smoke">
                {{ formatSize(e.file_size) }}
              </span>
              <span class="ml-auto font-mono text-[11px] text-smoke">
                {{ e.first_seen_at }}
              </span>
            </div>
            <div class="break-all text-[13px] text-snow">
              {{ e.file_path }}
            </div>
            <div class="text-[11px] text-graphite">
              reason: {{ e.reason }}
            </div>
          </div>
        </n-list-item>
      </n-list>
    </n-spin>
  </div>
</template>
