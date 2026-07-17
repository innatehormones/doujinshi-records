<script setup lang="ts">
import { onMounted, ref } from "vue"
import { NSpin, NEmpty, NTag, NPagination, NButton, useMessage } from "naive-ui"
import { Image, Rows3 } from "@lucide/vue"
import { useRecycleStore, useSettingsStore } from "@/stores"
import PermanentDeleteDialog from "@/components/PermanentDeleteDialog.vue"
import RestoreDialog from "@/components/RestoreDialog.vue"
import type { FileSummary } from "@/types/api"
import { formatBytes } from "@/lib/format"

const store = useRecycleStore()
const settings = useSettingsStore()
const message = useMessage()

const target = ref<FileSummary | null>(null)
const showDelete = ref(false)
const showRestore = ref(false)

/// 「待删除文件」列表的封面显示开关。FileSummary 自带 cover_url，
/// 开启时把缩略图渲染到每条左侧。默认关——避免一上来全表 IO。
const showCover = ref(true)

onMounted(() => store.load())

function coverSrc(f: FileSummary): string {
  if (!f.cover_url) return ""
  return settings.apiBase + f.cover_url
}

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
</script>

<template>
  <div class="page">
    <header class="flex items-baseline justify-between gap-4">
      <h1 class="text-heading-sm font-medium text-snow tracking-body">文件回收站</h1>
    </header>
    <div class="rounded-cards border border-border bg-card px-5 py-4">
      <p class="text-caption leading-[1.5] text-silver-mist">
        这里的文件已经移出已识别库，但仍占用硬盘空间。「永久删除」会把文件从硬盘移除；「还原」会让文件回到库内。两种操作都会保留数据记录。
      </p>
    </div>
    <div class="flex items-center justify-between gap-3">
      <h2 class="text-subheading font-medium text-snow tracking-body">
        待删除文件 ({{ store.presentTotal }})
      </h2>
      <button
        class="cover-toggle"
        :class="{ 'is-active': showCover }"
        :aria-label="showCover ? '隐藏封面' : '显示封面'"
        :title="showCover ? '隐藏封面' : '显示封面'"
        @click="showCover = !showCover"
      >
        <component
          :is="showCover ? Rows3 : Image"
          :size="16"
          :stroke-width="1.6"
        />
      </button>
    </div>
    <n-spin :show="store.loading">
      <n-empty
        v-if="!store.loading && store.presentTotal === 0"
        description="文件回收站为空"
      />
      <div v-else class="flex flex-col gap-2">
        <article
          v-for="f in store.present"
          :key="f.id"
          v-memo="[f.id, showCover]"
          class="flex items-start gap-4 rounded-cards border border-border bg-card p-4"
        >
          <img
            v-if="showCover && f.cover_url"
            :src="coverSrc(f)"
            alt=""
            loading="lazy"
            class="size-16 shrink-0 rounded border border-border object-cover"
          />
          <div v-else-if="showCover" class="size-16 shrink-0" aria-hidden="true" />
          <div class="flex min-w-0 flex-1 flex-col gap-1.5">
            <div class="flex items-center gap-2">
              <n-tag size="small">回收</n-tag>
              <span class="truncate text-body-sm font-medium text-snow">
                {{ f.title }}
              </span>
              <span class="ml-auto font-mono text-caption text-smoke">{{ formatBytes(f.size_bytes) }}</span>
            </div>
            <div class="text-caption text-silver-mist">文件哈希：{{ f.hash }}</div>
            <div class="text-caption break-all text-silver-mist">文件名称：{{ f.filename }}</div>
          </div>
          <div class="flex shrink-0 gap-2">
            <n-button size="small" @click="askRestore(f)">还原</n-button>
            <n-button size="small" type="warning" @click="askDelete(f)">删除</n-button>
          </div>
        </article>
      </div>

      <div v-if="store.showPresentPager" class="mt-6 flex justify-center">
        <n-pagination
          :page="store.presentPage"
          :page-count="store.presentTotalPages"
          :page-slot="5"
          @update:page="store.gotoPresentPage"
        />
      </div>
    </n-spin>

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
  </div>
</template>

<style scoped>
.cover-toggle {
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 6px;
  color: var(--color-silver-mist);
  cursor: pointer;
  transition: color 0.15s, background-color 0.15s, border-color 0.15s;
}
.cover-toggle:hover {
  color: var(--color-snow);
  background: var(--color-ash);
  border-color: var(--surface-border);
}
</style>
