<script setup lang="ts">
import { NPopconfirm } from "naive-ui"
import { Archive, Trash2, RotateCcw, X, AlertCircle, Inbox } from "@lucide/vue"
import type { FileSummary } from '@/types/api'

const props = defineProps<{
  file: FileSummary
  apiBase: string
}>()

const emit = defineEmits<{
(e: 'archive', id: number): void
(e: 'restore', id: number): void
(e: 'mark-delete', id: number): void
(e: 'permanent-delete', id: number): void
(e: 'open', id: number): void
}>()

function coverSrc(): string {
  if (!props.file.cover_url) return ""
  return props.apiBase + props.file.cover_url
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B"
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB"
  return (bytes / 1024 / 1024).toFixed(1) + " MB"
}

</script>

<template>
  <article
    class="relative flex flex-col overflow-hidden rounded-cards border border-border bg-card transition-[border-color,transform] duration-150 hover:border-slate"
    @click="emit('open', file.id)"
  >
    <div class="relative aspect-[3/4] overflow-hidden border-b border-border bg-obsidian-deep">
      <img v-if="file.cover_url" :src="coverSrc()" alt="" class="size-full object-cover" />
      <div
        v-else
        class="flex size-full items-center justify-center font-mono text-caption uppercase tracking-[0.1em] text-smoke"
      >
        <span>暂无封面</span>
      </div>
      <div class="absolute top-2 left-2 flex gap-1">
        <span
          v-if="file.current_location === 'will_delete'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-ember-orange backdrop-blur-sm"
          title="回收"
        >
          <Trash2 :size="12" :stroke-width="1.8" />
        </span>
        <span
          v-if="file.current_location === 'archived'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-archive-blue backdrop-blur-sm"
          title="归档"
        >
          <Archive :size="12" :stroke-width="1.8" />
        </span>
        <span
          v-if="file.current_location === 'inbox'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-snow backdrop-blur-sm"
          title="待入库"
        >
          <Inbox :size="12" :stroke-width="1.8" />
        </span>
        <span
          v-if="!file.has_physical_file"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-ember-red backdrop-blur-sm"
          title="文件丢失"
        >
          <AlertCircle :size="12" :stroke-width="1.8" />
        </span>
      </div>
    </div>
    <div class="flex flex-col gap-2 p-4">
      <div class="truncate text-body-sm font-medium leading-[1.3] text-snow" :title="file.title">
        {{ file.title }}
      </div>
      <div class="flex min-h-4 items-center justify-between text-caption text-smoke">
        <span v-if="file.circle" class="max-w-[60%] truncate">{{ file.circle }}</span>
        <span class="font-mono text-graphite tracking-[0.05em]">{{ formatSize(file.size_bytes) }}</span>
      </div>
      <div class="mt-2 flex flex-nowrap gap-1.5" @click.stop>
        <!-- identified: 归档 + 回收 -->
        <template v-if="file.current_location === 'identified'">
          <n-popconfirm
            positive-text="归档"
            negative-text="取消"
            @positive-click="emit('archive', file.id)"
          >
            <template #trigger>
              <button class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4">
                <Archive :size="13" :stroke-width="1.8" />
                归档
              </button>
            </template>
            把《{{ file.title }}》移到归档目录？
          </n-popconfirm>
          <n-popconfirm
            positive-text="移到回收站"
            negative-text="取消"
            @positive-click="emit('mark-delete', file.id)"
          >
            <template #trigger>
              <button class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-ember-orange bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-ember-orange transition-[border-color,background-color] duration-150 hover:bg-ember-orange/8">
                <Trash2 :size="13" :stroke-width="1.8" />
                回收
              </button>
            </template>
            把《{{ file.title }}》移到回收站？随时可在回收站页取回。
          </n-popconfirm>
        </template>

        <!-- will_delete: 取回 + 删除 -->
        <template v-else-if="file.current_location === 'will_delete'">
          <button
            class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4"
            @click="emit('restore', file.id)"
          >
            <RotateCcw :size="13" :stroke-width="1.8" />
            取回
          </button>
          <n-popconfirm
            v-if="file.has_physical_file"
            positive-text="永久删除"
            negative-text="取消"
            :positive-button-props="{ type: 'error' }"
            @positive-click="emit('permanent-delete', file.id)"
          >
            <template #trigger>
              <button class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-ember-red bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-ember-red transition-[border-color,background-color] duration-150 hover:bg-ember-red/8">
                <X :size="13" :stroke-width="1.8" />
                删除
              </button>
            </template>
            彻底清理将从硬盘删除 zip 文件（DB 记录保留，元数据可搜索）。
          </n-popconfirm>
        </template>

        <!-- archived: 取回 -->
        <template v-else-if="file.current_location === 'archived'">
          <button
            class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4"
            @click="emit('restore', file.id)"
          >
            <RotateCcw :size="13" :stroke-width="1.8" />
            取回
          </button>
        </template>
      </div>
    </div>
  </article>
</template>
