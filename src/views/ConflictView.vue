<script setup lang="ts">
import { ref, onMounted, computed } from "vue"
import { useRoute, useRouter } from "vue-router"
import { NCard, NSpace, NButton, NSpin, NList, NListItem, NEmpty, NAlert, useMessage } from "naive-ui"
import { useInboxStore, useSettingsStore } from "@/stores"
import type { ConflictCompare, ConflictAction as ConflictActionType } from "@/types/api"

const route = useRoute()
const router = useRouter()
const inbox = useInboxStore()
const settings = useSettingsStore()
const message = useMessage()

const data = ref<ConflictCompare | null>(null)
const loading = ref(false)
const acting = ref(false)

const conflictId = computed(() => Number(route.params.id))

onMounted(async () => {
  loading.value = true
  try {
    data.value = await inbox.loadCompare(conflictId.value)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
})

async function act(action: ConflictActionType) {
  acting.value = true
  try {
    await inbox.resolveConflict(conflictId.value, action)
    message.success("已处理")
    router.push({ name: "inbox" })
  } catch (e) {
    message.error(String(e))
  } finally {
    acting.value = false
  }
}
</script>

<template>
  <div>
    <div class="page-header">
      <h1>冲突对比</h1>
      <span class="count">conflict #{{ conflictId }}</span>
    </div>

    <n-spin :show="loading || acting">
      <n-empty
        v-if="!loading && !data"
        description="加载失败或该冲突已处理"
      />
      <div v-if="data" class="compare-grid">
        <n-card title="A · 已识别">
          <div class="cover-row">
            <img
              v-if="data.a.cover_url"
              :src="settings.apiBase + data.a.cover_url"
              alt="A 封面"
            />
            <div class="meta">
              <div><strong>标题:</strong> {{ data.a.title }}</div>
              <div v-if="data.a.hash" class="hash">
                哈希: {{ data.a.hash.slice(0, 16) }}…
              </div>
              <div class="file-id">file_id: {{ data.a.file_id }}</div>
            </div>
          </div>
          <n-alert
            v-if="data.a.zip_missing"
            type="warning"
            title="A 文件已不在磁盘"
            style="margin: 8px 0"
          />
          <n-alert
            v-if="data.a.zip_error"
            type="error"
            :title="data.a.zip_error"
            style="margin: 8px 0"
          />
          <h4>文件列表 ({{ data.a.image_names.length }})</h4>
          <n-empty
            v-if="data.a.image_names.length === 0"
            description="(无图片)"
            size="small"
          />
          <n-list v-else bordered>
            <n-list-item v-for="n in data.a.image_names" :key="n">
              {{ n }}
            </n-list-item>
          </n-list>
        </n-card>

        <n-card title="B · inbox 待处理">
          <div class="meta">
            <div><strong>文件名:</strong> {{ data.b.title }}</div>
            <div class="file-id">path: {{ data.b.file_path || "(未取)" }}</div>
          </div>
          <n-alert
            v-if="data.b.zip_missing"
            type="warning"
            title="B 文件已不在磁盘"
            style="margin: 8px 0"
          />
          <n-alert
            v-if="data.b.zip_error"
            type="error"
            :title="data.b.zip_error"
            style="margin: 8px 0"
          />
          <h4>文件列表 ({{ data.b.image_names.length }})</h4>
          <n-empty
            v-if="data.b.image_names.length === 0"
            description="(无图片)"
            size="small"
          />
          <n-list v-else bordered>
            <n-list-item v-for="n in data.b.image_names" :key="n">
              {{ n }}
            </n-list-item>
          </n-list>
        </n-card>
      </div>

      <n-space v-if="data" style="margin-top: 16px" justify="end">
        <n-button @click="act('keep_a')">保留 A（删 B）</n-button>
        <n-button type="warning" @click="act('replace_b')">替换为 B（删 A）</n-button>
        <n-button @click="act('keep_both')">都保留（B 加后缀入库）</n-button>
        <n-button @click="act('skip')">都跳过（保留 B 在 inbox）</n-button>
      </n-space>
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
.compare-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
}
.cover-row {
  display: flex;
  gap: 16px;
  margin-bottom: 12px;
}
.cover-row img {
  max-width: 160px;
  border: 1px solid var(--surface-border);
  border-radius: 4px;
}
.meta {
  flex: 1;
  font-size: 13px;
}
.meta .hash {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-smoke);
}
.meta .file-id {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--color-smoke);
  word-break: break-all;
}
</style>