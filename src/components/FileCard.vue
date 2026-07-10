<script setup lang="ts">
import type { FileSummary } from '@/types/api'

const props = defineProps<{
  file: FileSummary
  apiBase: string
}>()

const emit = defineEmits<{
(e: 'viewed', id: number): void
(e: 'delete', id: number): void
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
  <article class="card">
    <div class="cover">
      <img v-if="file.cover_url" :src="coverSrc()" alt="" />
      <div v-else class="no-cover">
        <span>暂无封面</span>
      </div>
      <div class="badges">
        <span v-if="file.viewed" class="badge badge-viewed" title="已查看">V</span>
        <span v-if="file.marked_for_delete" class="badge badge-marked" title="已标记">M</span>
        <span v-if="file.physically_deleted" class="badge badge-gone" title="已删除">X</span>
      </div>
    </div>
    <div class="body">
      <div class="title" :title="file.title">{{ file.title }}</div>
      <div class="meta">
        <span v-if="file.circle" class="circle">{{ file.circle }}</span>
        <span class="size mono">{{ formatSize(file.size_bytes) }}</span>
      </div>
      <div class="actions">
        <button class="btn" @click="emit('viewed', file.id)">
          {{ file.viewed ? "取消已看" : "标记已看" }}
        </button>
        <button
          class="btn"
          :class="file.marked_for_delete ? 'btn-active' : ''"
          @click="emit('delete', file.id)"
        >
          {{ file.marked_for_delete ? "取消标记" : "标记删除" }}
        </button>
      </div>
    </div>
  </article>
</template>

<style scoped>
/* Supabase feature-card style: canvas-colored card defined only by border */
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
.badge-marked { color: var(--color-smoke); }
.badge-gone   { color: var(--color-smoke); text-decoration: line-through; }

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
}
/* Ghost / pill buttons */
.btn {
  flex: 1;
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
.btn-active {
  border-color: var(--color-mint-pulse);
  color: var(--color-mint-pulse);
}
.btn-active:hover {
  background: rgba(0, 197, 115, 0.08);
}
</style>
