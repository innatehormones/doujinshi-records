<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import {
  NCard, NSpace, NButton, NTag, NSpin, useMessage, NCode, NDivider,
  NInputNumber, NSwitch,
} from "naive-ui"
import { useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"

const store = useSettingsStore()
const message = useMessage()
const scanResult = ref<number | null>(null)
const scanning = ref(false)

const portInput = ref<number>(0)
const portLocked = ref(false)

onMounted(() => store.load())

watch(
  () => store.data,
  (d) => {
    if (d) {
      portInput.value = d.http_port
      portLocked.value = d.http_port_locked
    }
  },
  { immediate: true },
)

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

async function savePort() {
  await api.setHttpPort(portLocked.value ? portInput.value : 0)
  message.success("已保存，重启后生效")
}

async function regenToken() {
  const newToken = await api.regenerateAuthToken()
  await store.load()
  message.success("Token 已重新生成，旧 Token 立刻失效")
  await copy(newToken)
}

const apiLines = computed(() => [
  "GET  " + store.apiBase + "/api/health              健康检查（无需 Token）",
  "GET  " + store.apiBase + "/api/doujinshi/search?q=关键词",
  "GET  " + store.apiBase + "/api/doujinshi/check?hash=<blake3>  检查哈希是否在库",
  "GET  " + store.apiBase + "/api/doujinshi/by-hash/<hash>       按哈希查询",
  "GET  " + store.apiBase + "/api/doujinshi/<id>                  按 ID 查询",
  "POST " + store.apiBase + "/api/doujinshi/<id>/viewed          标记已看",
  "GET  " + store.apiBase + "/api/covers/by-hash/<hash>          按哈希取封面",
  "GET  " + store.apiBase + "/api/covers/<file_id>               按 ID 取封面",
])
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h1>设置</h1>
      <span class="count mono">运行时 + API</span>
    </header>
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

      <n-card title="HTTP 端口">
        <p style="color: #aaa; font-size: 12px; margin-top: 0">
          锁定端口 = 应用启动时尝试绑定这个端口；占用则按 100/200/300ms 重试 3 次后回退到随机端口。
          关闭锁定 = 每次启动由操作系统分配空闲端口。
        </p>
        <n-space align="center">
          <n-input-number
            v-model:value="portInput"
            :min="0"
            :max="65535"
            :disabled="!portLocked"
            placeholder="0 = 随机"
            style="width: 140px"
          />
          <n-switch v-model:value="portLocked" />
          <span style="color: #aaa; font-size: 12px">
            {{ portLocked ? "固定端口" : "随机端口" }}
          </span>
          <n-button type="primary" size="small" @click="savePort">
            保存（重启后生效）
          </n-button>
          <n-tag v-if="store.data" size="small">
            当前：{{ store.data.http_port }}（{{ store.data.http_port_locked ? "已锁定" : "随机" }}）
          </n-tag>
        </n-space>
      </n-card>

      <n-card title="HTTP Token">
        <p style="color: #aaa; font-size: 12px; margin-top: 0">
          浏览器扩展和外部脚本调用 HTTP API 时需要在 <code>Authorization: Bearer &lt;token&gt;</code> 头里带这个值。
          重新生成后旧 Token 立刻失效。
        </p>
        <n-space align="center" style="width: 100%">
          <n-code :code="store.data?.auth_token ?? ''" style="flex: 1; overflow-x: auto" />
          <n-button size="small" @click="copy(store.data?.auth_token ?? '')">复制</n-button>
          <n-button size="small" type="warning" @click="regenToken">重新生成</n-button>
        </n-space>
      </n-card>

      <n-card title="Inbox 目录">
        <p style="color: #aaa; font-size: 12px; margin-top: 0">
          待识别压缩包放这里，应用会自动处理。
        </p>
        <n-input :value="store.data?.inbox_dir ?? ''" readonly />
      </n-card>

      <n-card title="HTTP API（供浏览器扩展使用）">
        <p style="color: #aaa; font-size: 12px">
          本应用在 127.0.0.1 暴露本地 HTTP API，给外部工具（浏览器扩展、脚本）查询本库。
          除 <code>/api/health</code> 外所有路由都需要带 <code>Authorization: Bearer &lt;token&gt;</code> 头（见上方 HTTP Token 卡片）。
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
  gap: var(--spacing-16);
}
.page-header h1 {
  font-size: var(--text-heading-sm);
  font-weight: var(--font-weight-medium);
  color: var(--color-snow);
  letter-spacing: var(--tracking-body);
}
.page-header .count {
  font-size: var(--text-caption);
  color: var(--color-smoke);
  letter-spacing: 0.1em;
}
</style>
