<script setup lang="ts">
import { NPopconfirm } from "naive-ui"
import type { FileSummary } from '@/types/api'

const props = defineProps<{
  file: FileSummary
  apiBase: string
}>()

const emit = defineEmits<{
(e: 'viewed', id: number): void
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

function locationLabel(): string {
  switch (props.file.current_location) {
    case "will_delete": return "回收"
    case "archived": return "归档"
    case "inbox": return "待入库"
    default: return ""
  }
}
</script>

<template>
  <article class="card" @click="emit('open', file.id)">
    <div class="cover">
      <img v-if="file.cover_url" :src="coverSrc()" alt="" />
      <div v-else class="no-cover">
        <span>暂无封面</span>
      </div>
      <div class="badges">
        <span v-if="file.viewed" class="badge badge-viewed" title="已查看">V</span>
        <span v-if="locationLabel()" class="badge badge-loc" :title="locationLabel()">
          {{ locationLabel().charAt(0) }}
        </span>
        <span v-if="!file.has_physical_file" class="badge badge-gone" title="文件丢失">!</span>
      </div>
    </div>
    <div class="body">
      <div class="title" :title="file.title">{{ file.title }}</div>
      <div class="meta">
        <span v-if="file.circle" class="circle">{{ file.circle }}</span>
        <span class="size mono">{{ formatSize(file.size_bytes) }}</span>
      </div>
      <div class="actions" @click.stop>
        <button class="btn" @click="emit('viewed', file.id)">
          {{ file.viewed ? "取消已看" : "标记已看" }}
        </button>

        <!-- identified: 归档 + 移到回收站 -->
        <template v-if="file.current_location === 'identified'">
          <n-popconfirm
            positive-text="归档"
            negative-text="取消"
            @positive-click="emit('archive', file.id)"
          >
            <template #trigger>
              <button class="btn">归档</button>
            </template>
            把《{{ file.title }}》移到归档目录？
          </n-popconfirm>
          <n-popconfirm
            positive-text="移到回收站"
            negative-text="取消"
            @positive-click="emit('mark-delete', file.id)"
          >
            <template #trigger>
              <button class="btn btn-warn">移到回收站</button>
            </template>
            把《{{ file.title }}》移到回收站？随时可在回收站页取回。
          </n-popconfirm>
        </template>

        <!-- will_delete: 取回 + 彻底清理 -->
        <template v-else-if="file.current_location === 'will_delete'">
          <button class="btn" @click="emit('restore', file.id)">取回</button>
          <n-popconfirm
            v-if="file.has_physical_file"
            positive-text="永久删除"
            negative-text="取消"
            :positive-button-props="{ type: 'error' }"
            @positive-click="emit('permanent-delete', file.id)"
          >
            <template #trigger>
              <button class="btn btn-danger">彻底清理</button>
            </template>
            彻底清理将从硬盘删除 zip 文件（DB 记录保留，元数据可搜索）。
          </n-popconfirm>
        </template>

        <!-- archived: 取回 -->
        <template v-else-if="file.current_location === 'archived'">
          <button class="btn" @click="emit('restore', file.id)">取回</button>
        </template>
      </div>
    </div>
  </article>
</template>

<style scoped>
.card {
  background: var(--surface-card);
  border: 1px solid var(--surface-border);
  border-radius: var(--radius-cards);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  transition: border-color 0.15s ease, transform 0.15s ease;
}
.card:hover {
  border-color: var(--color-slate);
}

.cover {
  position: relative;
  aspect-ratio: 3 / 4;
  background: var(--color-obsidian-deep);
  overflow: hidden;
  border-bottom: 1px solid var(--surface-border);
}
.cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.no-cover {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-smoke);
  font-family: var(--font-mono);
  font-size: var(--text-caption);
  letter-spacing: 0.1em;
  text-transform: uppercase;
}
.badges {
  position: absolute;
  top: var(--spacing-8);
  left: var(--spacing-8);
  display: flex;
  gap: 4px;
}
.badge {
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: var(--font-weight-medium);
  border-radius: 9999px;
  background: rgba(18, 18, 18, 0.85);
  backdrop-filter: blur(4px);
  border: 1px solid currentColor;
}
.badge-viewed { color: var(--color-phosphor-green); }
.badge-loc    { color: var(--color-snow); }
.badge-gone   { color: var(--color-ember-orange); }

.body {
  padding: var(--spacing-16);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-8);
}
.title {
  color: var(--color-snow);
  font-size: var(--text-body-sm);
  font-weight: var(--font-weight-medium);
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  color: var(--color-smoke);
  font-size: var(--text-caption);
  min-height: 16px;
}
.circle {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 60%;
}
.size {
  color: var(--color-graphite);
  letter-spacing: 0.05em;
}

.actions {
  display: flex;
  gap: var(--spacing-8);
  margin-top: var(--spacing-8);
  flex-wrap: wrap;
}
.btn {
  flex: 1;
  min-width: 60px;
  background: transparent;
  color: var(--color-snow);
  border: 1px solid var(--color-slate);
  border-radius: 9999px;
  padding: 6px 12px;
  font-family: var(--font-ui);
  font-size: var(--text-caption);
  font-weight: var(--font-weight-medium);
  letter-spacing: 0;
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}
.btn:hover {
  border-color: var(--color-graphite);
  background: rgba(255, 255, 255, 0.04);
}
.btn-warn { border-color: var(--color-ember-orange); color: var(--color-ember-orange); }
.btn-warn:hover { background: rgba(255, 140, 0, 0.08); }
.btn-danger { border-color: var(--color-ember-red); color: var(--color-ember-red); }
.btn-danger:hover { background: rgba(220, 38, 38, 0.08); }
</style>