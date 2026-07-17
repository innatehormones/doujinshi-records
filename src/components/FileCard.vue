<script setup lang="ts">
import { NPopconfirm, NTag } from "naive-ui"
import { Archive, Trash2, RotateCcw, X, AlertCircle } from "@lucide/vue"
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

/// V4 业务 status → 中文标签 / Naive UI tag 颜色。
const STATUS_LABEL: Record<string, string> = {
  in_library: "入库",
  archived: "归档",
  recycle: "回收",
  deleted: "已删",
}
/// V4 file_state → 中文标签 / tag 颜色。仅当 file_state ≠ present 时显示。
const FILE_STATE_LABEL: Record<string, string> = {
  missing: "文件丢失",
  absent_confirmed: "文件已删",
}
const FILE_STATE_TAG_TYPE: Record<string, TagType> = {
  missing: "error",
  absent_confirmed: "error",
}
type TagType = "default" | "primary" | "info" | "success" | "warning" | "error"
const STATUS_TAG_TYPE: Record<string, TagType> = {
  in_library: "success",
  archived: "info",
  recycle: "warning",
  deleted: "error",
}
function statusLabel(s: string): string {
  return STATUS_LABEL[s] ?? s
}
function statusTagType(s: string): TagType {
  return STATUS_TAG_TYPE[s] ?? "default"
}
function fileStateLabel(s: string): string {
  return FILE_STATE_LABEL[s] ?? s
}
function fileStateTagType(s: string): TagType {
  return FILE_STATE_TAG_TYPE[s] ?? "default"
}

</script>

<template>
  <article
    class="relative flex flex-col overflow-hidden rounded-cards border border-border bg-card transition-[border-color,transform] duration-150 hover:border-slate"
    @click="emit('open', file.id)"
  >
    <div class="relative aspect-[3/4] overflow-hidden border-b border-border bg-obsidian-deep">
      <img v-if="file.cover_url" :src="coverSrc()" alt="" loading="lazy" class="size-full object-cover" />
      <div
        v-else
        class="flex size-full items-center justify-center font-mono text-caption uppercase tracking-[0.1em] text-smoke"
      >
        <span>暂无封面</span>
      </div>
      <div class="absolute top-2 left-2 flex gap-1">
        <span
          v-if="file.status === 'recycle'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-ember-orange backdrop-blur-sm"
          title="回收"
        >
          <Trash2 :size="12" :stroke-width="1.8" />
        </span>
        <span
          v-if="file.status === 'archived'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-archive-blue backdrop-blur-sm"
          title="归档"
        >
          <Archive :size="12" :stroke-width="1.8" />
        </span>
        <span
          v-if="file.status === 'deleted'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-ember-red backdrop-blur-sm"
          title="已删"
        >
          <X :size="12" :stroke-width="1.8" />
        </span>
        <span
          v-if="file.file_state !== 'present'"
          class="inline-flex size-5 items-center justify-center rounded-full border border-current bg-obsidian/85 text-ember-red backdrop-blur-sm"
          :title="file.file_state === 'absent_confirmed' ? '用户已确认删除' : '文件丢失'"
        >
          <AlertCircle :size="12" :stroke-width="1.8" />
        </span>
      </div>
    </div>
    <div class="flex flex-col gap-2 p-4">
      <div class="flex items-center gap-1.5">
        <n-tag size="small" :type="statusTagType(file.status)">
          {{ statusLabel(file.status) }}
        </n-tag>
        <n-tag
          v-if="file.file_state !== 'present'"
          size="small"
          :type="fileStateTagType(file.file_state)"
        >
          {{ fileStateLabel(file.file_state) }}
        </n-tag>
      </div>
      <div class="truncate text-body-sm font-medium leading-[1.3] text-snow" :title="file.title">
        {{ file.title }}
      </div>
      <div class="flex min-h-4 items-center justify-between text-caption text-smoke">
        <span v-if="file.circle" class="max-w-[60%] truncate">{{ file.circle }}</span>
        <span class="font-mono text-graphite tracking-[0.05em]">{{ formatSize(file.size_bytes) }}</span>
      </div>
      <div class="mt-2 flex flex-nowrap gap-1.5" @click.stop>
        <!-- in_library: 归档 + 回收 -->
        <template v-if="file.status === 'in_library'">
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
            把《{{ file.title }}》归档？
          </n-popconfirm>
          <n-popconfirm
            positive-text="移到回收站"
            negative-text="取消"
            @positive-click="emit('mark-delete', file.id)"
          >
            <template #trigger>
              <button class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-ember-red bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-ember-red transition-[border-color,background-color] duration-150 hover:bg-ember-red/8">
                <Trash2 :size="13" :stroke-width="1.8" />
                回收
              </button>
            </template>
            把《{{ file.title }}》移到回收站？随时可在回收站页取回。
          </n-popconfirm>
        </template>

        <!-- recycle: 取回 + 销毁（仅 file_state=present 时显示销毁按钮） -->
        <template v-else-if="file.status === 'recycle'">
          <button
            class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4"
            @click="emit('restore', file.id)"
          >
            <RotateCcw :size="13" :stroke-width="1.8" />
            取回
          </button>
          <n-popconfirm
            v-if="file.file_state === 'present'"
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
        <template v-else-if="file.status === 'archived'">
          <button
            class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4"
            @click="emit('restore', file.id)"
          >
            <RotateCcw :size="13" :stroke-width="1.8" />
            取回
          </button>
        </template>

        <!-- deleted: 恢复（取回） -->
        <template v-else-if="file.status === 'deleted'">
          <button
            class="inline-flex min-w-0 flex-1 items-center justify-center gap-1 whitespace-nowrap rounded-full border border-slate bg-transparent px-2 py-1.5 font-sans text-caption font-medium text-snow transition-[border-color,background-color] duration-150 hover:border-graphite hover:bg-snow/4"
            @click="emit('restore', file.id)"
          >
            <RotateCcw :size="13" :stroke-width="1.8" />
            恢复
          </button>
        </template>
      </div>
    </div>
  </article>
</template>
