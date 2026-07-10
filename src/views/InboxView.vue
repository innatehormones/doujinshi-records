<script setup lang="ts">
import { onMounted } from "vue"
import { NList, NListItem, NThing, NTag, NSpace, NButton, NSpin, NEmpty, NCard } from "naive-ui"
import { useInboxStore } from "@/stores"

const store = useInboxStore()
onMounted(() => store.load())
</script>

<template>
  <div>
    <div class="page-header">
      <h1>待识别</h1>
      <span class="count">{{ store.conflicts.length }} 个待处理</span>
    </div>
    <n-card title="待识别 · 文件名冲突">
      <p style="color: #aaa">
        文件名与已识别库内文件相同的压缩包会停在这里。点「跳过」让新文件留在 inbox 不动，或等 V2 上线做内容比对。
      </p>
    </n-card>

    <h3 style="margin-top: 16px">
      待处理冲突 ({{ store.conflicts.length }})
    </h3>

    <n-spin :show="store.loading">
      <n-empty
        v-if="!store.loading && store.conflicts.length === 0"
        description="没有待处理冲突。"
      />
      <n-list bordered>
        <n-list-item v-for="c in store.conflicts" :key="c.id">
          <n-thing>
            <template #header>
              <n-tag type="warning" size="small">conflict</n-tag>
              <span style="margin-left: 8px">{{ c.b_filename }}</span>
            </template>
            <template #description>
              <div style="color: #aaa; font-size: 12px">
                已在库中: <strong>{{ c.a_title }}</strong>
                (id={{ c.a_file_id }})
              </div>
              <div style="color: #888; font-size: 11px">
                {{ c.b_file_path }}
              </div>
            </template>
          </n-thing>
          <template #suffix>
            <n-space>
              <n-tag size="small" :bordered="false" type="info">V2: 内容比对</n-tag>
              <n-button size="small" @click="store.resolve(c.id)">
                跳过
              </n-button>
            </n-space>
          </template>
        </n-list-item>
      </n-list>
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
.section {
  margin-bottom: var(--spacing-32);
}
.section-title {
  font-size: var(--text-caption);
  font-weight: var(--font-weight-medium);
  color: var(--color-smoke);
  letter-spacing: 0.1em;
  text-transform: uppercase;
  margin-bottom: var(--spacing-8);
}
.hint {
  color: var(--color-silver-mist);
  font-size: var(--text-body-sm);
  line-height: var(--leading-body-sm);
  padding: var(--spacing-16) 0;
  border-bottom: 1px solid var(--surface-border);
}
</style>
