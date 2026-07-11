<script setup lang="ts">
import { onMounted, watch, computed } from "vue"
import { useRouter } from "vue-router"
import { NGrid, NGi, NSpace, NInput, NSelect, NSpin, NEmpty, NTag, useMessage } from "naive-ui"
import { useLibraryStore, useSettingsStore } from "@/stores"
import FileCard from "@/components/FileCard.vue"

const store = useLibraryStore()
const settings = useSettingsStore()
const router = useRouter()
const message = useMessage()

function onCardOpen(id: number) {
  router.push({ name: 'detail', params: { id } })
}

function onChipClick(name: string) {
  store.setQuery(store.getQuery() === name ? "" : name)
}

const statusOptions = [
  { label: "全部", value: "all" },
  { label: "未查看", value: "not_viewed" },
  { label: "已查看", value: "viewed" },
]

const locationOptions = [
  { label: "全部", value: "all" },
  { label: "已入库", value: "identified" },
  { label: "回收站", value: "will_delete" },
  { label: "归档", value: "archived" },
]

const apiBase = computed(() => settings.apiBase)

onMounted(async () => {
  await settings.load()
  await store.load()
})

watch(() => store.query, () => store.load())
watch(() => store.status, () => store.load())
watch(() => store.locationFilter, () => store.load())

async function onCardArchive(id: number) {
  try {
    await store.archive(id)
  } catch (e) {
    message.error(String(e))
  }
}

async function onCardRestore(id: number) {
  try {
    await store.restore(id)
  } catch (e) {
    message.error(String(e))
  }
}

async function onCardMarkDelete(id: number) {
  try {
    await store.markForDelete(id)
  } catch (e) {
    message.error(String(e))
  }
}

async function onCardPermanentDelete(id: number) {
  try {
    const { useRecycleStore } = await import("@/stores")
    await useRecycleStore().permanentDelete(id)
    await store.load()
  } catch (e) {
    message.error(String(e))
  }
}
</script>

<template>
  <div>
    <n-space style="margin-bottom: 16px" :wrap="true">
      <n-input
        :value="store.getQuery()"
        @update:value="store.setQuery"
        placeholder="搜索标题 / 社团 / 文件名"
        clearable
        style="width: 300px"
      />
      <n-select
        v-model:value="store.status"
        :options="statusOptions"
        style="width: 140px"
      />
      <n-select
        v-model:value="store.locationFilter"
        :options="locationOptions"
        style="width: 140px"
      />
    </n-space>

    <n-space
      v-if="store.topCircles.length > 0"
      style="margin-bottom: 16px"
      :wrap="true"
    >
      <n-tag
        v-for="c in store.topCircles"
        :key="c.name"
        :type="store.getQuery() === c.name ? 'primary' : 'default'"
        checkable
        @click="onChipClick(c.name)"
      >
        {{ c.name }} ({{ c.count }})
      </n-tag>
      <n-tag
        v-if="store.getQuery()"
        type="warning"
        @click="store.setQuery('')"
      >
        清除
      </n-tag>
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
            @open="onCardOpen"
            @viewed="store.markViewed"
            @archive="onCardArchive"
            @restore="onCardRestore"
            @mark-delete="onCardMarkDelete"
            @permanent-delete="onCardPermanentDelete"
          />
        </n-gi>
      </n-grid>
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