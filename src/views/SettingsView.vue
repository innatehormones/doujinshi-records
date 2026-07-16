<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import {
  NCard, NSpace, NButton, NTag, NSpin, useMessage, NCode, NDivider,
  NInputNumber, NSwitch, NInput, NPopconfirm,
} from "naive-ui"
import { useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"
import type { BackupConfig, BackupSnapshot } from "@/types/api"

const store = useSettingsStore()
const message = useMessage()
const scanResult = ref<number | null>(null)
const scanning = ref(false)

const portInput = ref<number>(0)
const portLocked = ref(false)

// 备份状态
const backupCfg = ref<BackupConfig | null>(null)
const backupDirInput = ref<string>("")
const retentionInput = ref<number>(10)
const snapshots = ref<BackupSnapshot[]>([])
const backingUp = ref(false)

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

watch(
  () => store.data,
  (d) => {
    if (d) loadBackup()
  },
)

async function loadBackup() {
  const [cfg, list] = await Promise.all([
    api.getBackupConfig(),
    api.listBackups(),
  ])
  backupCfg.value = cfg
  backupDirInput.value = cfg.dir
  retentionInput.value = cfg.retention_count
  snapshots.value = list
}

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

async function saveBackupConfig() {
  try {
    await api.setBackupConfig(backupDirInput.value || null, retentionInput.value)
    message.success("备份配置已保存")
    await loadBackup()
  } catch (e) {
    message.error(String(e))
  }
}

async function doBackupNow() {
  backingUp.value = true
  try {
    const result = await api.backupNow()
    if (result.skipped) {
      message.info("内容未变，跳过")
    } else {
      message.success(`备份成功：${result.path}`)
    }
    await loadBackup()
  } catch (e) {
    message.error(String(e))
  } finally {
    backingUp.value = false
  }
}

async function stageRestore(snap: BackupSnapshot) {
  try {
    await api.stageRestore(snap.path)
    message.warning("已标记还原。请退出应用并重新启动生效。")
  } catch (e) {
    message.error(String(e))
  }
}

async function deleteSnap(snap: BackupSnapshot) {
  try {
    await api.deleteBackup(snap.path)
    message.success("已删除")
    await loadBackup()
  } catch (e) {
    message.error(String(e))
  }
}

const apiLines = computed(() => [
  "GET  " + store.apiBase + "/api/health              健康检查（无需 Token）",
  "GET  " + store.apiBase + "/api/doujinshi/search?q=关键词",
  "GET  " + store.apiBase + "/api/doujinshi/check?hash=<blake3>  检查哈希是否在库",
  "GET  " + store.apiBase + "/api/doujinshi/by-hash/<hash>       按哈希查询",
  "GET  " + store.apiBase + "/api/doujinshi/<id>                  按 ID 查询",
  "GET  " + store.apiBase + "/api/covers/by-hash/<hash>          按哈希取封面",
  "GET  " + store.apiBase + "/api/covers/<file_id>               按 ID 取封面",
])
</script>

<template>
  <div class="page">
    <header class="flex items-baseline justify-between gap-4">
      <h1 class="text-heading-sm font-medium text-snow tracking-body">设置</h1>
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">运行时 + API</span>
    </header>
    <n-spin :show="scanning">
      <n-space vertical size="large">
        <n-card title="路径">
          <n-spin :show="!store.data">
            <div v-if="store.data" class="flex flex-col gap-1">
              <div>资源目录: <n-tag>{{ store.data.resources_dir }}</n-tag></div>
              <div>入库冲突处理: <n-tag>{{ store.data.inbox_dir }}</n-tag></div>
              <div>已识别: <n-tag>{{ store.data.identified_dir }}</n-tag></div>
              <div>待删除: <n-tag>{{ store.data.will_delete_dir }}</n-tag></div>
              <div>封面: <n-tag>{{ store.data.covers_dir }}</n-tag></div>
            </div>
          </n-spin>
          <n-button size="small" class="mt-2" @click="store.load()">
            刷新
          </n-button>
        </n-card>

        <n-card title="HTTP 端口">
          <p class="text-caption text-silver-mist">
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
              class="w-[140px]"
            />
            <n-switch v-model:value="portLocked" />
            <span class="text-caption text-silver-mist">
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
          <p class="text-caption text-silver-mist">
            浏览器扩展和外部脚本调用 HTTP API 时需要在 <code>Authorization: Bearer &lt;token&gt;</code> 头里带这个值。
            重新生成后旧 Token 立刻失效。
          </p>
          <n-space align="center" class="w-full">
            <n-code :code="store.data?.auth_token ?? ''" class="min-w-0 flex-1 overflow-x-auto" />
            <n-button size="small" @click="copy(store.data?.auth_token ?? '')">复制</n-button>
            <n-button size="small" type="warning" @click="regenToken">重新生成</n-button>
          </n-space>
        </n-card>

        <n-card title="Inbox 目录">
          <p class="text-caption text-silver-mist">
            入库冲突处理压缩包放这里，应用会自动处理。
          </p>
          <n-input :value="store.data?.inbox_dir ?? ''" readonly />
        </n-card>

        <n-card title="数据备份">
          <p class="text-caption text-silver-mist">
            仅备份 data.db（不含压缩文件）。目录留空 = 默认 <code>resources/backups/</code>。
            内容未变时会自动跳过；启动期超过 24h 没成功备份会自动补一次。
          </p>
          <n-space align="center" class="mt-2 flex-wrap">
            <n-input
              v-model:value="backupDirInput"
              placeholder="默认目录"
              class="min-w-[260px]"
            />
            <span class="text-caption text-silver-mist">目录</span>
            <n-input-number
              v-model:value="retentionInput"
              :min="0"
              :max="999"
              placeholder="保留数"
              class="w-[120px]"
            />
            <span class="text-caption text-silver-mist">保留最近 N 个（0 = 不限）</span>
            <n-button type="primary" size="small" @click="saveBackupConfig">保存</n-button>
          </n-space>
          <n-space class="mt-3">
            <n-button :loading="backingUp" @click="doBackupNow">立即备份</n-button>
            <n-tag v-if="backupCfg" size="small">
              当前：{{ backupCfg.dir || "默认目录" }}，保留 {{ backupCfg.retention_count }}
            </n-tag>
          </n-space>
          <div v-if="snapshots.length === 0" class="mt-3 text-caption text-silver-mist">
            还没有备份
          </div>
          <div v-else class="mt-3 grid gap-2">
            <div
              v-for="s in snapshots"
              :key="s.path"
              class="flex items-center gap-2 font-mono text-caption"
            >
              <span class="flex-1 truncate" :title="s.path">{{ s.path }}</span>
              <n-tag size="small">{{ Math.round(s.size_bytes / 1024) }} KB</n-tag>
              <n-button size="tiny" type="warning" @click="stageRestore(s)">
                恢复
              </n-button>
              <n-popconfirm @positive-click="deleteSnap(s)">
                <template #trigger>
                  <n-button size="tiny" type="error">删除</n-button>
                </template>
                确认删除该快照？无法恢复。
              </n-popconfirm>
            </div>
          </div>
        </n-card>

        <n-card title="HTTP API（供浏览器扩展使用）">
          <p class="text-caption text-silver-mist">
            本应用在 127.0.0.1 暴露本地 HTTP API，给外部工具（浏览器扩展、脚本）查询本库。
            除 <code>/api/health</code> 外所有路由都需要带 <code>Authorization: Bearer &lt;token&gt;</code> 头（见上方 HTTP Token 卡片）。
          </p>
          <div class="mb-2">
            <span>接口地址: </span>
            <n-tag type="success">{{ store.apiBase }}</n-tag>
            <n-button size="tiny" class="ml-2" @click="copy(store.apiBase)">
              Copy
            </n-button>
          </div>
          <n-divider class="my-3!" />
          <div class="font-mono text-caption">
            <div v-for="line in apiLines" :key="line" class="mb-1">
              <n-code :code="line" />
            </div>
          </div>
        </n-card>

        <n-card title="扫描">
          <p class="text-caption text-silver-mist">
            后台监听 <code>resources/doujinshi/</code>，自动处理新放入的压缩包。如果你怀疑漏掉了某个文件，可以点下面手动扫描。
          </p>
          <n-space>
            <n-button type="primary" @click="runScan">手动扫描入库冲突处理目录</n-button>
            <n-tag v-if="scanResult !== null">上次处理: {{ scanResult }} 个</n-tag>
          </n-space>
        </n-card>
      </n-space>
    </n-spin>
  </div>
</template>
