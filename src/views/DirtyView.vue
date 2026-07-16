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
    case "will_delete": return "回收站"
    case "archived": return "归档"
    default: return dir
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
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">共 {{ store.total }} 条</span>
    </header>
    <div class="rounded-cards border border-border bg-card px-5 py-4">
      <p class="text-caption leading-[1.5] text-silver-mist">启动扫描发现：这些文件位于入库目录 / 回收站 / 归档目录，但数据库无对应行。手动清理，或者对入库目录里的孤儿文件点「重新入库」让 scanner 再走一次入库流程。</p>
    </div>
    <h2 class="text-subheading font-medium text-snow tracking-body">脏数据条目</h2>
    <n-spin :show="store.loading">
      <n-empty v-if="!store.loading && store.entries.length === 0" description="无脏数据。" />
      <div v-else class="flex flex-col gap-2">
        <article v-for="e in store.entries" :key="e.id" v-memo="[e.id]" class="flex items-start gap-4 rounded-cards border border-border bg-card p-4">
          <div class="flex min-w-0 flex-1 flex-col gap-1.5">
            <div class="flex items-center gap-2">
              <n-tag size="small">{{ dirLabel(e.detected_dir) }}</n-tag>
              <n-tag v-if="e.reason === 'orphan_file'" size="small" type="warning">孤儿</n-tag>
              <span class="font-mono text-caption text-smoke">{{ formatSize(e.file_size) }}</span>
              <span class="ml-auto font-mono text-[11px] text-smoke">{{ e.first_seen_at }}</span>
            </div>
            <div class="break-all font-mono text-[13px] text-snow">{{ e.file_path }}</div>
            <div class="text-[11px] text-graphite">reason: {{ e.reason }}</div>
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
            重新入库会让 scanner 跑完整流程（BLAKE3 / 抽封面 / 入库），撞文件名会进 ConflictView。是否继续？
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