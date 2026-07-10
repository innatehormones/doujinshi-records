<script setup lang="ts">
import { ref, onMounted, computed } from "vue"
import {
  NCard, NSpace, NButton, NTag, NSpin, useMessage, NCode, NDivider,
} from "naive-ui"
import { useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"

const store = useSettingsStore()
const message = useMessage()
const scanResult = ref<number | null>(null)
const scanning = ref(false)

onMounted(() => store.load())

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    message.success("已复制")
  } catch {
    message.error("剪贴板不可用")
  }
}

async function runScan() {
  scanning.value = true
  try {
    scanResult.value = await api.manualScan()
    message.info(`扫描完成，处理 ${scanResult.value} 个文件。`)
  } catch (e: unknown) {
    message.error(String(e))
  } finally {
    scanning.value = false
  }
}

const apiLines = computed(() => [
  "GET  " + store.apiBase + "/api/health        健康检查",
  "GET  " + store.apiBase + "/api/doujinshi/search?q=关键词",
  "GET  " + store.apiBase + "/api/doujinshi/by-hash/<hash>    按哈希查询",
  "GET  " + store.apiBase + "/api/doujinshi/<id>     按 ID 查询",
  "GET  " + store.apiBase + "/api/covers/<file_id>  封面图片",
])
</script>

<template>
  <div>
    <div class="page-header">
      <h1>设置</h1>
      <span class="count">运行时 + API</span>
    </div>
  <n-spin :show="scanning">
    <n-space vertical size="large">
      <n-card title="路径">
        <n-spin :show="!store.data">
          <div v-if="store.data">
            <div>资源目录: <n-tag>{{ store.data.resources_dir }}</n-tag></div>
            <div style="margin-top: 4px">待识别: <n-tag>{{ store.data.inbox_dir }}</n-tag></div>
            <div style="margin-top: 4px">已识别: <n-tag>{{ store.data.identified_dir }}</n-tag></div>
            <div style="margin-top: 4px">待删除: <n-tag>{{ store.data.will_delete_dir }}</n-tag></div>
            <div style="margin-top: 4px">封面: <n-tag>{{ store.data.covers_dir }}</n-tag></div>
          </div>
        </n-spin>
        <n-button size="small" style="margin-top: 8px" @click="store.load()">
          刷新
        </n-button>
      </n-card>

      <n-card title="HTTP API（供浏览器扩展使用）">
        <p style="color: #aaa; font-size: 12px">
          本应用在 127.0.0.1 暴露本地 HTTP API，给外部工具（浏览器扩展、脚本）查询本库。
        </p>
        <div style="margin-bottom: 8px">
          <span>接口地址: </span>
          <n-tag type="success">{{ store.apiBase }}</n-tag>
          <n-button
            size="tiny"
            style="margin-left: 8px"
            @click="copy(store.apiBase)"
          >
            Copy
          </n-button>
        </div>
        <n-divider style="margin: 12px 0" />
        <div style="font-family: monospace; font-size: 12px">
          <div v-for="line in apiLines" :key="line" style="margin-bottom: 4px">
            <n-code :code="line" />
          </div>
        </div>
      </n-card>

      <n-card title="扫描">
        <p style="color: #aaa; font-size: 12px">
          后台监听 <code>resources/doujinshi/</code>，自动处理新放入的压缩包。如果你怀疑漏掉了某个文件，可以点下面手动扫描。
        </p>
        <n-space>
          <n-button type="primary" @click="runScan">手动扫描待识别目录</n-button>
          <n-tag v-if="scanResult !== null">上次处理: {{ scanResult }} 个</n-tag>
        </n-space>
      </n-card>
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
</style>
