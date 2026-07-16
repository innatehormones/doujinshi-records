<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import {
  NButton, NTag, NSpin, useMessage, NCode,
  NInputNumber, NSwitch, NInput, NPopconfirm, NTooltip,
} from "naive-ui"
import { RefreshCw, ClipboardCopy, RotateCw, Play, Save, Trash2, FolderOpen } from "@lucide/vue"
import { useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"
import type { BackupConfig, BackupSnapshot } from "@/types/api"
import ApiTestDialog from "@/components/ApiTestDialog.vue"

const store = useSettingsStore()
const message = useMessage()
const scanResult = ref<number | null>(null)
const scanning = ref(false)

const portInput = ref<number>(0)
const portLocked = ref(false)

const backupCfg = ref<BackupConfig | null>(null)
const backupDirInput = ref<string>("")
const retentionInput = ref<number>(10)
const snapshots = ref<BackupSnapshot[]>([])
const backingUp = ref(false)

/// HTTP API 测试弹窗（V4.8）。`activeRoute` 持有当前选中的路由，绑定到
/// `<api-test-dialog>` 的 method / path prop；`showDialog` 复用 n-modal 的
/// show prop。点关闭按钮时 `@update:show` 把 showDialog 写回 false。
const activeRoute = ref<{ method: string; path: string } | null>(null)
const showDialog = ref(false)
function openTest(r: { method: string; path: string }) {
  activeRoute.value = r
  showDialog.value = true
}

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
  (d) => { if (d) loadBackup() },
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

/// 「资源目录」列表每行的「打开」按钮：调后端 spawn explorer.exe。
/// fire-and-forget——explorer.exe 的退出码不关心，UI 不阻塞。
async function openDir(path: string) {
  try {
    await api.openPath(path)
  } catch (e) {
    message.error(`打开失败：${e}`)
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
    message.warning("已标记还原。退出应用并重新启动生效。")
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

const apiRoutes = computed(() => [
  { method: "GET", path: "/api/health",                        note: "健康检查（无需 Token）" },
  { method: "GET", path: "/api/doujinshi/search?q=...",        note: "标题/社团/文件名模糊搜索（需 Token）" },
  { method: "GET", path: "/api/doujinshi/check?hash=<blake3>",note: "检查哈希是否在库（需 Token）" },
  { method: "GET", path: "/api/doujinshi/by-hash/<hash>",      note: "按哈希查询（需 Token）" },
  { method: "GET", path: "/api/doujinshi/<id>",                note: "按 ID 查询（需 Token）" },
  { method: "GET", path: "/api/covers/by-hash/<hash>",         note: "按哈希取封面（需 Token）" },
  { method: "GET", path: "/api/covers/<file_id>",              note: "按 ID 取封面（需 Token）" },
])

function fmtSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(1) + " MB"
  if (bytes >= 1024) return Math.round(bytes / 1024) + " KB"
  return bytes + " B"
}

/// RFC3339 (UTC) → 本地时区 YYYY-MM-DD HH:MM。后端 chrono::Utc 写入，
/// 文件名也用 UTC 时刻；前端展示成本地时间让用户判断「是不是我想要的那份」。
function fmtMtime(iso: string): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  const pad = (n: number) => String(n).padStart(2, "0")
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
         `${pad(d.getHours())}:${pad(d.getMinutes())}`
}
</script>

<template>
  <div class="page settings-page">
    <header class="flex items-baseline justify-between gap-4">
      <h1 class="text-heading-sm font-medium text-snow tracking-body">设置</h1>
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">运行时 · 数据 · 外部集成</span>
    </header>

    <!-- ==================== 运行时 ==================== -->
    <section class="settings-section">
      <h2 class="text-subheading font-medium text-snow tracking-body">运行时</h2>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">HTTP 端口</h3>
            <n-tag v-if="store.data" size="small" type="success">
              当前 {{ store.data.http_port }}（{{ store.data.http_port_locked ? "已锁定" : "随机" }}）
            </n-tag>
          </header>
          <p class="settings-card-desc">
            锁定端口 = 启动时尝试绑定；占用时按 100/200/300ms 重试 3 次后回退随机端口。关闭锁定 = 由操作系统分配空闲端口。
          </p>
          <div class="settings-card-controls">
            <div class="control-row">
              <n-input-number
                v-model:value="portInput"
                :min="0" :max="65535" :disabled="!portLocked"
                placeholder="0 = 随机" class="w-[140px]"
              />
              <n-switch v-model:value="portLocked" />
              <span class="text-caption text-silver-mist">
                {{ portLocked ? "固定端口" : "随机端口" }}
              </span>
            </div>
          </div>
          <div class="settings-card-actions">
            <n-button type="primary" size="small" @click="savePort">
              <template #icon><Save :size="13" :stroke-width="1.8" /></template>
              保存
            </n-button>
            <n-tag size="small" type="warning">重启后生效</n-tag>
          </div>
        </div>
      </article>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">HTTP Token</h3>
          </header>
          <p class="settings-card-desc">
            浏览器扩展或外部脚本调用 HTTP API 时在 <code>Authorization: Bearer &lt;token&gt;</code> 头里带这个值。重新生成后旧 Token 立刻失效。
          </p>
          <div class="settings-card-controls">
            <div class="token-display">
              <span class="token-label">Token</span>
              <code class="token-value">{{ store.data?.auth_token ?? '' }}</code>
              <n-button size="tiny" @click="copy(store.data?.auth_token ?? '')">
                <template #icon><ClipboardCopy :size="12" :stroke-width="1.8" /></template>
                复制
              </n-button>
            </div>
          </div>
          <div class="settings-card-actions">
            <n-popconfirm @positive-click="regenToken">
              <template #trigger>
                <n-button size="small" type="warning">
                  <template #icon><RotateCw :size="13" :stroke-width="1.8" /></template>
                  重新生成
                </n-button>
              </template>
              重新生成 Token 后旧值立刻失效，确认继续？
            </n-popconfirm>
          </div>
        </div>
      </article>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">Inbox 目录</h3>
          </header>
          <p class="settings-card-desc">
            把压缩包拖到这里，应用会监听并自动处理（hash 去重 / 抽封面 / 入库）。撞名压缩包会留在 Inbox 等用户在「入库冲突处理」页解决。
          </p>
          <div class="settings-card-controls">
            <n-input :value="store.data?.inbox_dir ?? ''" readonly />
          </div>
          <div class="settings-card-actions">
            <n-button size="small" @click="copy(store.data?.inbox_dir ?? '')">
              <template #icon><ClipboardCopy :size="13" :stroke-width="1.8" /></template>
              复制路径
            </n-button>
          </div>
        </div>
      </article>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">手动扫描</h3>
            <n-tag v-if="scanResult !== null" size="small">
              上次处理 {{ scanResult }} 个
            </n-tag>
          </header>
          <p class="settings-card-desc">
            后台已监听 <code>resources/doujinshi/</code> 顶层文件变化（2 秒防抖）。如果怀疑漏掉了某个文件，用「立即扫描」跑一遍。
          </p>
          <div class="settings-card-actions">
            <n-button type="primary" :loading="scanning" @click="runScan">
              <template #icon><Play :size="13" :stroke-width="1.8" /></template>
              立即扫描
            </n-button>
          </div>
        </div>
      </article>
    </section>

    <!-- ==================== 数据 ==================== -->
    <section class="settings-section">
      <h2 class="text-subheading font-medium text-snow tracking-body">数据</h2>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">资源目录</h3>
            <n-button size="tiny" @click="store.load()">
              <template #icon><RefreshCw :size="12" :stroke-width="1.8" /></template>
              刷新
            </n-button>
          </header>
          <p class="settings-card-desc">
            应用运行时数据根目录。所有 4 个数据目录（Inbox / 已识别 / 文件回收站 / 归档）都在它下面。
          </p>
          <n-spin :show="!store.data">
            <ul v-if="store.data" class="path-list">
              <li v-for="row in [
                { label: '资源根', value: store.data.resources_dir },
                { label: '入库', value: store.data.inbox_dir },
                { label: '已识别', value: store.data.identified_dir },
                { label: '文件回收站', value: store.data.will_delete_dir },
                { label: '封面缓存', value: store.data.covers_dir },
              ]" :key="row.label" class="path-row">
                <span class="path-key">{{ row.label }}</span>
                <n-code :code="row.value" class="path-value" />
                <button
                  class="path-act"
                  :title="`在文件管理器中打开 ${row.value}`"
                  :aria-label="`打开 ${row.label}`"
                  @click="openDir(row.value)"
                >
                  <FolderOpen :size="14" :stroke-width="1.6" />
                </button>
              </li>
            </ul>
          </n-spin>
        </div>
      </article>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">数据备份</h3>
            <n-tag v-if="backupCfg" size="small">
              当前：{{ backupCfg.dir || "默认目录" }} · 保留 {{ backupCfg.retention_count }}
            </n-tag>
          </header>
          <p class="settings-card-desc">
            仅备份 <code>data.db</code>（不含压缩文件）。目录留空 = 默认 <code>resources/backups/</code>。内容未变时自动跳过；启动期超过 24h 没成功备份会自动补一次。
          </p>
          <div class="settings-card-controls">
            <div class="control-row">
              <span class="control-label">备份目录</span>
              <n-input
                v-model:value="backupDirInput"
                placeholder="默认 resources/backups/"
                class="control-input"
              />
            </div>
            <div class="control-row">
              <span class="control-label">保留最近</span>
              <n-input-number
                v-model:value="retentionInput" :min="0" :max="999"
                placeholder="0 = 不限" class="w-[140px]"
              />
              <span class="text-caption text-silver-mist">个快照（0 = 不限）</span>
            </div>
          </div>
          <div class="settings-card-actions">
            <n-button type="primary" size="small" @click="saveBackupConfig">
              <template #icon><Save :size="13" :stroke-width="1.8" /></template>
              保存
            </n-button>
            <n-popconfirm @positive-click="doBackupNow">
              <template #trigger>
                <n-button :loading="backingUp" size="small">
                  <template #icon><Play :size="13" :stroke-width="1.8" /></template>
                  立即备份
                </n-button>
              </template>
              立即执行一次数据库备份？耗时几秒。
            </n-popconfirm>
          </div>

          <div v-if="snapshots.length > 0" class="snapshot-list">
            <header class="snapshot-head">
              <span class="col-path">路径</span>
              <span class="col-time">时间</span>
              <span class="col-size">大小</span>
              <span class="col-act">操作</span>
            </header>
            <ul class="snapshot-body">
              <li v-for="s in snapshots" :key="s.path" class="snapshot-row">
                <n-tooltip :show-arrow="true" placement="top">
                  <template #trigger>
                    <span class="col-path truncate" :title="s.path">{{ s.path }}</span>
                  </template>
                  {{ s.path }}
                </n-tooltip>
                <span class="col-time font-mono text-caption text-smoke">{{ fmtMtime(s.mtime) }}</span>
                <span class="col-size font-mono text-caption text-smoke">{{ fmtSize(s.size_bytes) }}</span>
                <span class="col-act">
                  <n-button size="tiny" @click="stageRestore(s)">恢复</n-button>
                  <n-popconfirm @positive-click="deleteSnap(s)">
                    <template #trigger>
                      <n-button size="tiny" type="error">
                        <template #icon><Trash2 :size="12" :stroke-width="1.8" /></template>
                        删除
                      </n-button>
                    </template>
                    确认删除该快照？无法恢复。
                  </n-popconfirm>
                </span>
              </li>
            </ul>
          </div>
          <p v-else class="text-caption text-silver-mist">还没有备份。</p>
        </div>
      </article>
    </section>

    <!-- ==================== 外部集成 ==================== -->
    <section class="settings-section">
      <h2 class="text-subheading font-medium text-snow tracking-body">外部集成</h2>

      <article class="settings-card">
        <div class="settings-card-body">
          <header class="settings-card-head">
            <h3 class="settings-card-title">HTTP API</h3>
          </header>
          <p class="settings-card-desc">
            应用在 127.0.0.1 暴露本地 HTTP API，给浏览器扩展、脚本等外部工具查询本库。除 <code>/api/health</code> 外所有路由都需要 <code>Authorization: Bearer &lt;token&gt;</code> 鉴权（见上方「HTTP Token」）。
          </p>

          <div class="api-base">
            <span class="api-base-label">接口地址</span>
            <n-code :code="store.apiBase" class="api-base-url" />
            <n-button size="tiny" @click="copy(store.apiBase)">
              <template #icon><ClipboardCopy :size="12" :stroke-width="1.8" /></template>
              复制
            </n-button>
          </div>

          <table class="api-table">
            <thead>
              <tr>
                <th class="col-method">METHOD</th>
                <th class="col-path">PATH</th>
                <th class="col-note">描述</th>
                <th class="col-act">操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in apiRoutes" :key="r.path">
                <td class="col-method"><span class="method">{{ r.method }}</span></td>
                <td class="col-path"><code class="api-path">{{ r.path }}</code></td>
                <td class="col-note">{{ r.note }}</td>
                <td class="col-act">
                  <n-button size="tiny" @click="openTest(r)">测试</n-button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </article>
    </section>

    <api-test-dialog
      :show="showDialog"
      :method="activeRoute?.method ?? ''"
      :path="activeRoute?.path ?? ''"
      @update:show="showDialog = $event"
    />
  </div>
</template>

<style scoped>
/* ===== Section ===== */
.settings-page {
  gap: var(--spacing-32);
}
.settings-section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-16);
}

/* ===== Card ===== */
.settings-card {
  background: var(--surface-card);
  border: 1px solid var(--surface-border);
  border-radius: var(--radius-cards);
}
.settings-card-body {
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.settings-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.settings-card-title {
  font-size: var(--text-subheading);
  font-weight: 500;
  color: var(--color-snow);
  letter-spacing: var(--tracking-body);
  margin: 0;
}
.settings-card-desc {
  font-size: var(--text-caption);
  line-height: 1.55;
  color: var(--color-silver-mist);
  margin: 0;
}
.settings-card-desc :deep(code) {
  font-family: var(--font-mono);
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--color-ash);
  color: var(--color-snow);
}
.settings-card-controls {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.settings-card-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 4px;
}

/* ===== Control row ===== */
.control-row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.control-label {
  font-size: var(--text-caption);
  color: var(--color-smoke);
  min-width: 64px;
}
.control-input {
  flex: 1;
  min-width: 0;
}

/* ===== Token display ===== */
.token-display {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: var(--color-obsidian-deep);
  border: 1px solid var(--surface-border);
  border-radius: 8px;
}
.token-label {
  font-size: var(--text-caption);
  color: var(--color-smoke);
  white-space: nowrap;
}
.token-value {
  flex: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-snow);
  overflow-x: auto;
  white-space: nowrap;
  padding: 0;
  background: transparent;
}

/* ===== Path list ===== */
.path-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.path-row {
  display: grid;
  grid-template-columns: 96px 1fr auto;
  align-items: center;
  gap: 12px;
}
.path-key {
  font-size: var(--text-caption);
  color: var(--color-smoke);
}
.path-value {
  font-size: 12px;
  color: var(--color-snow);
  overflow-x: auto;
  white-space: nowrap;
}
.path-act {
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 6px;
  color: var(--color-silver-mist);
  cursor: pointer;
  transition: color 0.15s, background-color 0.15s, border-color 0.15s;
}
.path-act:hover {
  color: var(--color-snow);
  background: var(--color-ash);
  border-color: var(--surface-border);
}

/* ===== Snapshot list ===== */
.snapshot-list {
  margin-top: 4px;
  border: 1px solid var(--surface-border);
  border-radius: 10px;
  overflow: hidden;
}
.snapshot-head,
.snapshot-row {
  display: grid;
  grid-template-columns: 1fr 130px 70px 132px;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
}
.snapshot-head {
  background: var(--color-ash);
  border-bottom: 1px solid var(--surface-border);
}
.snapshot-head > span {
  font-size: 11px;
  color: var(--color-smoke);
}
.snapshot-body {
  list-style: none;
  margin: 0;
  padding: 0;
}
.snapshot-row {
  border-bottom: 1px solid var(--surface-border);
}
.snapshot-row:last-child { border-bottom: none; }
.snapshot-row:hover { background: var(--color-ash); }
.snapshot-row .col-act {
  display: flex;
  gap: 6px;
  justify-content: flex-end;
}

/* ===== API table ===== */
.api-base {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  background: var(--color-ash);
  border: 1px solid var(--surface-border);
  border-radius: 8px;
}
.api-base-label {
  font-size: var(--text-caption);
  color: var(--color-smoke);
  white-space: nowrap;
}
.api-base-url { flex: 1; min-width: 0; }

.api-table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  border: 1px solid var(--surface-border);
  border-radius: 10px;
  overflow: hidden;
  font-size: 12px;
}
.api-table th,
.api-table td {
  text-align: left;
  padding: 8px 12px;
  border-bottom: 1px solid var(--surface-border);
  vertical-align: middle;
}
.api-table th {
  background: var(--color-ash);
  font-size: 11px;
  color: var(--color-smoke);
  font-weight: 500;
}
.api-table tr:last-child td { border-bottom: none; }
.api-table tr:hover td { background: var(--color-ash); }

.col-method { width: 80px; }
.col-path   { width: 320px; }
.col-note   { color: var(--color-silver-mist); }
.col-act    { width: 88px; text-align: right; }

.method {
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.05em;
  color: var(--color-snow);
}
.api-path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-snow);
  background: transparent;
  padding: 0;
}
</style>