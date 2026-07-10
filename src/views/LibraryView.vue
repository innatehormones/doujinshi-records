<script setup lang="ts">
import { onMounted, ref, watch, computed } from "vue"
import { NGrid, NGi, NSpace, NInput, NSelect, NSpin, NEmpty } from "naive-ui"
import { useLibraryStore, useSettingsStore } from "@/stores"
import FileCard from "@/components/FileCard.vue"
import DeleteDialogA from "@/components/DeleteDialogA.vue"
import DeleteDialogB from "@/components/DeleteDialogB.vue"

const store = useLibraryStore()
const settings = useSettingsStore()

const statusOptions = [
  { label: "全部", value: "all" },
  { label: "未查看", value: "not_viewed" },
  { label: "已查看", value: "viewed" },
  { label: "已标记", value: "marked" },
]

const target = ref<{ id: number; title: string; size: string } | null>(null)
const showA = ref(false)
const showB = ref(false)

const apiBase = computed(() => settings.apiBase)

onMounted(async () => {
  await settings.load()
  await store.load()
})

watch(() => store.query, () => store.load())
watch(() => store.status, () => store.load())

async function onCardDelete(id: number) {
  const f = store.items.find((f) => f.id === id)
  if (!f) return
  if (f.marked_for_delete) {
    await store.cancelDelete(id)
  } else {
    target.value = { id, title: f.title, size: formatSize(f.size_bytes) }
    showA.value = true
  }
}

async function confirmA() {
  if (!target.value) return
  await store.startDelete(target.value.id)
  showA.value = false
  showB.value = true
}

async function confirmB() {
  if (!target.value) return
  await store.confirmMoveToWillDelete(target.value.id)
  showB.value = false
  target.value = null
}

function cancelAB() {
  showA.value = false
  showB.value = false
  target.value = null
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B"
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB"
  return (bytes / 1024 / 1024).toFixed(1) + " MB"
}
</script>

<template>
  <div>
    <n-space style="margin-bottom: 16px">
      <n-input
        v-model:value="store.query"
        placeholder="搜索标题 / 社团 / 文件名"
        clearable
        style="width: 300px"
      />
      <n-select
        v-model:value="store.status"
        :options="statusOptions"
        style="width: 140px"
      />
    </n-space>

    <n-spin :show="store.loading">
      <n-empty
        v-if="!store.loading && store.items.length === 0"
        description="还没有文件，把压缩包丢进 resources/doujinshi/ 即可。"
      />
      <n-grid x-gap="12" y-gap="12" cols="6">
        <n-gi v-for="f in store.items" :key="f.id">
          <file-card
            :file="f"
            :api-base="apiBase"
            @viewed="store.markViewed"
            @delete="onCardDelete"
          />
        </n-gi>
      </n-grid>
    </n-spin>

    <delete-dialog-a
      :show="showA"
      :title="target?.title ?? ''"
      @cancel="cancelAB"
      @confirm="confirmA"
    />
    <delete-dialog-b
      :show="showB"
      :title="target?.title ?? ''"
      :size="target?.size ?? ''"
      @cancel="cancelAB"
      @confirm="confirmB"
    />
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
.toolbar {
  margin-bottom: var(--spacing-24);
  gap: var(--spacing-8);
}
.empty {
  padding: 80px 0;
  text-align: center;
  border: 1px dashed var(--surface-border);
  border-radius: var(--radius-cards);
  color: var(--color-smoke);
}
</style>
