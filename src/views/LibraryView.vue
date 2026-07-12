<script setup lang="ts">
import { onMounted, watch, computed } from "vue"
import { useRouter } from "vue-router"
import { NSpace, NInput, NSelect, NSpin, NEmpty, NTag, useMessage } from "naive-ui"
import { useLibraryStore, useSettingsStore, useRecycleStore } from "@/stores"
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

/// 当前页条数（store 没有 total 字段；后端 limit=50，列表分页未启用）。
const totalLabel = computed(() => `${store.items.length} 条`)

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
    await useRecycleStore().permanentDelete(id)
    await store.load()
  } catch (e) {
    message.error(String(e))
  }
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="page-header-left">
        <h1>我的同人志</h1>
        <span class="count mono">{{ totalLabel }}</span>
      </div>
      <div class="page-header-right">
        <n-input
          :value="store.getQuery()"
          @update:value="store.setQuery"
          placeholder="搜索标题 / 社团 / 文件名"
          clearable
          size="medium"
          class="search-input"
        />
        <n-select
          v-model:value="store.status"
          :options="statusOptions"
          size="medium"
          class="filter-select"
        />
        <n-select
          v-model:value="store.locationFilter"
          :options="locationOptions"
          size="medium"
          class="filter-select"
        />
      </div>
    </header>

    <n-space
      v-if="store.topCircles.length > 0"
      class="circle-chips"
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
      <div v-else class="card-grid">
        <file-card
          v-for="f in store.items"
          :key="f.id"
          :file="f"
          :api-base="apiBase"
          @open="onCardOpen"
          @viewed="store.markViewed"
          @archive="onCardArchive"
          @restore="onCardRestore"
          @mark-delete="onCardMarkDelete"
          @permanent-delete="onCardPermanentDelete"
        />
      </div>
    </n-spin>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-24);
  padding: var(--page-pad-y) var(--page-pad-x);
}
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-24);
  flex-wrap: wrap;
}
.page-header-left {
  display: flex;
  align-items: baseline;
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
.page-header-right {
  display: flex;
  align-items: center;
  gap: var(--spacing-8);
  flex-wrap: wrap;
}
.search-input {
  width: 280px;
  max-width: 100%;
}
.filter-select {
  width: 140px;
}
.circle-chips {
  margin-top: -8px;
}
.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 16px;
}
</style>
