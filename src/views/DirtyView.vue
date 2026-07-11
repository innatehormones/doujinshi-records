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
  <div>
    <div class="page-header">
      <h1>脏数据</h1>
      <span class="count">{{ store.entries.length }} 条</span>
    </div>
    <p style="color: #aaa; margin-bottom: 16px">
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
          <div style="display: flex; flex-direction: column; gap: 4px; width: 100%">
            <div style="display: flex; gap: 8px; align-items: center">
              <n-tag size="small">{{ dirLabel(e.detected_dir) }}</n-tag>
              <span style="color: var(--color-smoke); font-family: var(--font-mono); font-size: 12px">
                {{ formatSize(e.file_size) }}
              </span>
              <span style="color: var(--color-smoke); font-family: var(--font-mono); font-size: 11px; margin-left: auto">
                {{ e.first_seen_at }}
              </span>
            </div>
            <div style="color: var(--color-snow); font-size: 13px; word-break: break-all">
              {{ e.file_path }}
            </div>
            <div style="color: var(--color-graphite); font-size: 11px">
              reason: {{ e.reason }}
            </div>
          </div>
        </n-list-item>
      </n-list>
    </n-spin>
  </div>
</template>

<style scoped>
.page-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
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
.page-header .count {
  font-family: var(--font-mono);
  font-size: var(--text-caption);
  color: var(--color-smoke);
  letter-spacing: 0.1em;
}
</style>