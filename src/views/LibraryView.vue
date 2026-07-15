<script setup lang="ts">
import { onMounted, watch, computed, ref } from "vue"
import { useRouter } from "vue-router"
import { NSpace, NInput, NSelect, NSpin, NEmpty, NTag, NPagination, useMessage } from "naive-ui"
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
  store.page = 1
}

function clearQuery() {
  store.setQuery("")
  store.page = 1
}

/// V4 业务 status 过滤下拉选项。`active` 是 UI 概念（=排除 recycle +
/// deleted），由 store 内部的 `visibleItems` 二次过滤。
const statusFilterOptions = [
  { label: "正常（排除回收/已删）", value: "active" },
  { label: "全部", value: "all" },
  { label: "入库", value: "in_library" },
  { label: "归档", value: "archived" },
  { label: "回收站", value: "recycle" },
  { label: "已删除", value: "deleted" },
]

const apiBase = computed(() => settings.apiBase)

/// "我的同人志 · 共 1324 条"。无副作用时直接读 store.total。
const totalLabel = computed(() => `共 ${store.total} 条`)

/// 社团 chip 默认折叠展示前 5 个，剩余展开查看。
const CIRCLE_INITIAL = 5
const circlesExpanded = ref(false)
const visibleCircles = computed(() => {
  if (circlesExpanded.value || store.topCircles.length <= CIRCLE_INITIAL) {
    return store.topCircles
  }
  return store.topCircles.slice(0, CIRCLE_INITIAL)
})
const hiddenCircleCount = computed(() => store.topCircles.length - CIRCLE_INITIAL)
const canCollapseCircles = computed(() =>
  store.topCircles.length > CIRCLE_INITIAL && circlesExpanded.value,
)

onMounted(async () => {
  await settings.load()
  await store.load()
})

watch(() => store.query, () => {
  store.page = 1
  store.load()
})
watch(() => store.statusFilter, () => {
  store.page = 1
  store.load()
})

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
    <header class="grid grid-cols-[1fr_auto] items-center gap-x-8 gap-y-4">
      <div class="flex items-baseline gap-3">
        <h1 class="text-heading-sm font-medium text-snow tracking-body">我的同人志</h1>
        <span class="font-mono text-caption text-smoke tracking-[0.1em]">·</span>
        <span class="font-mono text-caption text-smoke tracking-[0.1em]">{{ totalLabel }}</span>
      </div>
      <div class="flex flex-nowrap items-center gap-3">
        <n-input
          :value="store.getQuery()"
          @update:value="store.setQuery"
          placeholder="搜索标题 / 社团 / 文件名"
          clearable
          size="medium"
          style="width: 240px; flex: 0 0 240px;"
        />
        <n-select
          :value="store.statusFilter"
          @update:value="(v) => (store.statusFilter = v)"
          :options="statusFilterOptions"
          size="medium"
          style="width: 200px; flex: 0 0 200px;"
        />
      </div>
    </header>

    <n-space
      v-if="store.topCircles.length > 0"
      class="-mt-2"
      :wrap="true"
    >
      <n-tag
        v-for="c in visibleCircles"
        :key="c.circle"
        :type="store.getQuery() === c.circle ? 'primary' : 'default'"
        checkable
        @click="onChipClick(c.circle)"
      >
        {{ c.circle }} ({{ c.count }})
      </n-tag>
      <n-tag
        v-if="hiddenCircleCount > 0 && !circlesExpanded"
        @click="circlesExpanded = true"
      >
        +{{ hiddenCircleCount }}
      </n-tag>
      <n-tag
        v-else-if="canCollapseCircles"
        @click="circlesExpanded = false"
      >
        收起
      </n-tag>
      <n-tag
        v-if="store.getQuery()"
        type="warning"
        @click="clearQuery"
      >
        清除
      </n-tag>
    </n-space>

    <n-spin :show="store.loading">
      <n-empty
        v-if="!store.loading && store.visibleItems.length === 0"
        description="还没有文件，把压缩包丢进 resources/doujinshi/ 即可。"
      />
      <div v-else class="grid grid-cols-4 gap-5 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-7 3xl:grid-cols-8">
        <file-card
          v-for="f in store.visibleItems"
          :key="f.id"
          v-memo="[f.id, f.status, f.file_state]"
          :file="f"
          :api-base="apiBase"
          @open="onCardOpen"
          @archive="onCardArchive"
          @restore="onCardRestore"
          @mark-delete="onCardMarkDelete"
          @permanent-delete="onCardPermanentDelete"
        />
      </div>
    </n-spin>

    <div v-if="store.showPager" class="mt-6 flex justify-center">
      <n-pagination
        :page="store.page"
        :page-count="store.totalPages"
        :page-slot="5"
        show-quick-jumper
        @update:page="store.gotoPage"
      />
    </div>
  </div>
</template>
