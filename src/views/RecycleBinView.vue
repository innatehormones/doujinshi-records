<script setup lang="ts">
import { onMounted, ref } from "vue"
import { useRouter } from "vue-router"
import {
  NSpin, NEmpty, NDivider, NPagination, useMessage,
} from "naive-ui"
import { Trash2, RotateCcw, X } from "@lucide/vue"
import { useRecycleStore, useSettingsStore } from "@/stores"
import PermanentDeleteDialog from "@/components/PermanentDeleteDialog.vue"
import RestoreDialog from "@/components/RestoreDialog.vue"
import type { FileSummary } from "@/types/api"

const store = useRecycleStore()
const settings = useSettingsStore()
const router = useRouter()
const message = useMessage()

const target = ref<FileSummary | null>(null)
const showDelete = ref(false)
const showRestore = ref(false)

onMounted(async () => {
  await settings.load()
  await store.load()
})

function onCardOpen(id: number) {
  router.push({ name: 'detail', params: { id } })
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
      <h1 class="text-heading-sm font-medium text-snow tracking-body">文件回收站</h1>
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">
        共 {{ store.presentTotal + store.goneTotal }} 条
      </span>
    </header>
    <n-spin :show="store.loading">
      <section>
        <h2 class="text-subheading font-medium text-snow tracking-body">
          待删除文件 ({{ store.presentTotal }})
        </h2>
        <p class="mt-2 text-caption leading-[1.5] text-silver-mist">
          这里的文件已经移出已识别库，但仍占用硬盘空间。「永久删除」会把文件从硬盘移除；「还原」会让文件回到库内。两种操作都会保留数据记录。
        </p>

        <n-empty
          v-if="!store.loading && store.presentTotal === 0"
          description="文件回收站为空。"
          class="mt-4"
        />

        <div
          v-else
          class="mt-4 grid grid-cols-4 gap-5 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-7 3xl:grid-cols-8"
        >
          <article
            v-for="f in store.present"
            :key="f.id"
            v-memo="[f.id, f.file_state]"
            class="relative flex flex-col overflow-hidden rounded-cards border border-border bg-card transition-[border-color,transform] duration-150 hover:border-slate"
            @click="onCardOpen(f.id)"
          >
            <div class="relative aspect-[3/4] overflow-hidden border-b border-border bg-obsidian-deep">
              <img v-if="f.cover_url" :src="coverUrl(f)" alt="" loading="lazy" class="size-full object-cover" />
              <div
                v-else
                class="flex size-full items-center justify-center font-mono text-caption uppercase tracking-[0.1em] text-smoke"
              >
                暂无封面
              </div>
              <div class="absolute top-2 left-2 flex gap-1">
                <span
                  class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-ember-orange backdrop-blur-sm"
                  title="回收"
                >
                  <Trash2 :size="12" :stroke-width="1.8" />
                </span>
              </div>
            </div>
            <div class="flex flex-col gap-2 p-4">
              <div class="truncate text-body-sm font-medium leading-[1.3] text-snow" :title="f.title">
                {{ f.title }}
              </div>
              <div class="flex min-h-4 items-center justify-between text-caption text-smoke">
                <span v-if="f.circle" class="max-w-[60%] truncate">{{ f.circle }}</span>
                <span class="font-mono text-graphite tracking-[0.05em]">{{ fmtSize(f.size_bytes) }}</span>
              </div>
              <div class="mt-2 flex flex-nowrap gap-1.5" @click.stop>
                <button
                  class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4"
                  @click="askRestore(f)"
                >
                  <RotateCcw :size="13" :stroke-width="1.8" />
                  还原
                </button>
                <button
                  class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-ember-red bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-ember-red transition-[border-color,background-color] duration-150 hover:bg-ember-red/8"
                  @click="askDelete(f)"
                >
                  <X :size="13" :stroke-width="1.8" />
                  删除
                </button>
              </div>
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
      </section>

      <n-divider />

      <section>
        <h2 class="text-subheading font-medium text-snow tracking-body">
          已从硬盘删除 ({{ store.goneTotal }})
        </h2>
        <p class="mt-2 text-caption leading-[1.5] text-silver-mist">
          数据记录保留供搜索 / 外部工具使用。这里的文件无法再还原。
        </p>
        <n-empty
          v-if="store.goneTotal === 0"
          description="暂无已删除记录。"
          size="small"
          class="mt-4"
        />
        <div v-else class="mt-4 flex flex-col gap-2">
          <article
            v-for="f in store.gone"
            :key="f.id"
            v-memo="[f.id]"
            class="flex items-center gap-3 rounded-cards border border-border bg-card p-4"
          >
            <span
              class="inline-flex size-5 shrink-0 items-center justify-center rounded-full border border-current bg-obsidian-deep text-ember-red"
              title="已删除"
            >
              <X :size="12" :stroke-width="1.8" />
            </span>
            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="truncate text-body-sm font-medium text-snow">{{ f.title }}</span>
              <span class="font-mono text-[11px] text-smoke">
                {{ fmtSize(f.size_bytes) }} · hash {{ f.hash.slice(0, 12) }}…
              </span>
            </div>
          </article>
        </div>

        <div v-if="store.showGonePager" class="mt-6 flex justify-center">
          <n-pagination
            :page="store.gonePage"
            :page-count="store.goneTotalPages"
            :page-slot="5"
            @update:page="store.gotoGonePage"
          />
        </div>
      </section>

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
