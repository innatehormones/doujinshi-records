<script setup lang="ts">
import { onMounted } from "vue"
import { NEmpty, NSpin, NTag, NPagination, NButton, NPopconfirm, useMessage } from "naive-ui"
import { useDirtyStore } from "@/stores"

const store = useDirtyStore()
const message = useMessage()
onMounted(() => store.load())

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B"
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB"
  return (bytes / 1024 / 1024).toFixed(1) + " MB"
}

function dirLabel(dir: string): string {
  switch (dir) {
    case "identified": return "入库目录"
    case "will_delete": return "文件回收站"
    case "archived": return "归档"
    default: return dir
  }
}

/// dirty_data.reason 的中文标签。后端存的是稳定英文标识符，这里是
/// 前端唯一权威翻译——用户理解"这条记录为什么算脏"的入口。
function reasonLabel(reason: string): string {
  switch (reason) {
    case "orphan_file":                    return "孤儿文件"
    case "db_row_file_missing":            return "文件已丢失"
    case "location_path_mismatch":         return "路径漂走"
    case "location_path_mismatch_resolved":return "路径已自愈"
    case "overwritten_by_state_switch":    return "转移时覆盖旧文件"
    default: return reason
  }
}

/// warning = 用户需介入（孤儿 / 文件丢失 / 路径漂走）；
/// default = 系统已自愈或仅作审计。
function reasonTagType(reason: string): "default" | "warning" {
  switch (reason) {
    case "orphan_file":
    case "db_row_file_missing":
    case "location_path_mismatch":
      return "warning"
    default:
      return "default"
  }
}

async function onReingest(id: number) {
  try {
    await store.reingest(id)
    message.success("已重新入库")
  } catch (e) {
    message.error(String(e))
  }
}
</script>

<template>
  <div class="page">
    <header class="flex items-baseline justify-between gap-4">
      <h1 class="text-heading-sm font-medium text-snow tracking-body">脏数据</h1>
    </header>
    <div class="rounded-cards border border-border bg-card px-5 py-4">
      <p class="text-caption leading-[1.5] text-silver-mist">启动扫描发现：这些文件位于入库目录 / 文件回收站 / 归档目录，但数据库无对应行。手动清理，或者对入库目录里的孤儿文件点「重新入库」让 scanner 再走一次入库流程。</p>
    </div>
    <h2 class="text-subheading font-medium text-snow tracking-body">脏数据条目 ({{ store.total }})</h2>
    <n-spin :show="store.loading">
      <n-empty v-if="!store.loading && store.entries.length === 0" description="无脏数据" />
      <div v-else class="flex flex-col gap-2">
        <article v-for="e in store.entries" :key="e.id" v-memo="[e.id]" class="flex items-start gap-4 rounded-cards border border-border bg-card p-4">
          <div class="flex min-w-0 flex-1 flex-col gap-1.5">
            <div class="flex items-center gap-2">
              <n-tag size="small">{{ dirLabel(e.detected_dir) }}</n-tag>
              <n-tag size="small" :type="reasonTagType(e.reason)">{{ reasonLabel(e.reason) }}</n-tag>
              <span class="font-mono text-caption text-smoke">{{ formatSize(e.file_size) }}</span>
              <span class="ml-auto font-mono text-[11px] text-smoke">{{ e.first_seen_at }}</span>
            </div>
            <div class="break-all font-mono text-[13px] text-snow">{{ e.file_path }}</div>
          </div>
          <n-popconfirm
            v-if="e.reason === 'orphan_file'"
            positive-text="确认重新入库"
            negative-text="取消"
            @positive-click="onReingest(e.id)"
          >
            <template #trigger>
              <n-button size="small" type="primary" ghost>重新入库</n-button>
            </template>
            重新入库会把文件搬到入库目录让 scanner 自动入库。是否继续？
          </n-popconfirm>
        </article>
      </div>

      <div v-if="store.showPager" class="mt-6 flex justify-center">
        <n-pagination
          :page="store.page"
          :page-count="store.totalPages"
          :page-slot="5"
          @update:page="store.gotoPage"
        />
      </div>
    </n-spin>
  </div>
</template>
