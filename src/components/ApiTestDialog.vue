<script setup lang="ts">
/// ApiTestDialog — Settings 页 HTTP API 路由表的测试入口
///
/// 边界（V4.8 spec 限定）：单组件 + SettingsView 一行 import + 一行渲染，
/// 删组件 = 移除功能。Self-contained：用 `fetch()` 直接打后端，token 由
/// `useSettingsStore().data.auth_token` 提供，不依赖 Tauri invoke。
import { computed, ref, watch } from "vue"
import {
  NModal, NButton, NInput, NCode, NEmpty, NSpin, useMessage,
} from "naive-ui"
import { ClipboardCopy, Play } from "@lucide/vue"
import { useSettingsStore } from "@/stores"

const props = defineProps<{
  show: boolean
  method: string
  path: string
}>()

const emit = defineEmits<{
  (e: "update:show", v: boolean): void
}>()

const store = useSettingsStore()
const message = useMessage()

/// `<id>` / `<hash>` / `<file_id>` / `<blake3>` 这类 placeholder 名字。
/// 用户在弹窗输入 → 通过 `params` map 注入 curl 拼接 + fetch URL。
const params = ref<Record<string, string>>({})

/// 最近一次响应。null = 未发送。
const response = ref<{
  status: number
  ms: number
  body: string
  error?: string
} | null>(null)

const sending = ref(false)

/// 路径里所有 `<placeholder>` 名字（`<id>` / `<hash>` / `<file_id>`）。
const pathPlaceholders = computed(() => {
  const [p] = props.path.split("?")
  const out: string[] = []
  const re = /<(\w+)>/g
  let m: RegExpExecArray | null
  while ((m = re.exec(p ?? "")) !== null) out.push(m[1])
  return out
})

/// Query 串里的"待填"参数。识别两种风格：
///   - `name=<placeholder>` → 拿 placeholder 名字当 key（如 `<blake3>`）
///   - `name=...` → 拿 key 名字当参数名（如 `q`）
/// 两种都自动生成输入框，分别做 `<name>` / `...` 替换。
const querySlots = computed(() => {
  const idx = props.path.indexOf("?")
  if (idx < 0) return []
  const q = props.path.slice(idx + 1)
  const out: { name: string; source: "placeholder" | "dots" }[] = []
  const re = /(\w+)=(?:<(\w+)>|\.{3})/g
  let m: RegExpExecArray | null
  while ((m = re.exec(q)) !== null) {
    if (m[2]) out.push({ name: m[2], source: "placeholder" })
    else if (m[1]) out.push({ name: m[1], source: "dots" })
  }
  return out
})

/// 合并 path + query 占位输入框（顺序：path → query）。
const allInputs = computed(() => {
  return [
    ...pathPlaceholders.value.map((n) => ({ name: n, kind: "path" })),
    ...querySlots.value.map((s) => ({ name: s.name, kind: "query" })),
  ]
})

/// path + query 里的占位替换成用户输入值：
///   - `<placeholder>` → params[name]（未填留 `<name>`）
///   - `name=...` → `name=value`（未填保留 `...`）
const resolvedPath = computed(() => {
  let p = props.path
  p = p.replace(/<(\w+)>/g, (full, name) =>
    params.value[name] ? params.value[name] : full,
  )
  p = p.replace(/(\w+)=\.{3}/g, (full, key) => {
    const v = params.value[key]
    return v ? `${key}=${v}` : full
  })
  return p
})

const fullUrl = computed(() => {
  const base = store.apiBase || ""
  return base.replace(/\/$/, "") + resolvedPath.value
})

const token = computed(() => store.data?.auth_token ?? "")

const curlText = computed(() => {
  const lines = [
    `curl -X ${props.method || "GET"} \\\n  '${fullUrl.value}'`,
  ]
  if (token.value) {
    lines.push(`  -H 'Authorization: Bearer ${token.value}'`)
  }
  return lines.join(" \\\n")
})

watch(
  () => props.show,
  (v) => {
    if (!v) {
      params.value = {}
      response.value = null
      sending.value = false
    }
  },
)

function copyCurl() {
  navigator.clipboard?.writeText(curlText.value)
    .then(() => message.success("已复制 cURL"))
    .catch(() => message.error("复制失败"))
}

async function send() {
  if (sending.value) return
  sending.value = true
  response.value = null
  const t0 = performance.now()
  try {
    const res = await fetch(fullUrl.value, {
      method: props.method || "GET",
      headers: token.value
        ? { Authorization: `Bearer ${token.value}` }
        : undefined,
    })
    const ms = Math.round(performance.now() - t0)
    const body = await res.text()
    response.value = { status: res.status, ms, body }
  } catch (e: unknown) {
    const ms = Math.round(performance.now() - t0)
    response.value = {
      status: 0,
      ms,
      body: "",
      error: String(e instanceof Error ? e.message : e),
    }
  } finally {
    sending.value = false
  }
}

/// 非 JSON body 原样；JSON body pretty-print（2 空格）。
function fmtBody(body: string): string {
  const t = body.trim()
  if (!t) return body
  if (!(t.startsWith("{") || t.startsWith("["))) return body
  try {
    return JSON.stringify(JSON.parse(t), null, 2)
  } catch {
    return body
  }
}

const statusKind = computed(() => {
  if (!response.value) return null
  const s = response.value.status
  if (s >= 500) return "err"
  if (s >= 400) return "warn"
  if (s > 0) return "ok"
  return "err"
})
</script>

<template>
  <n-modal
    :show="show"
    preset="card"
    style="width: 720px; max-width: calc(100vw - 32px);"
    :on-update-show="(v: boolean) => emit('update:show', v)"
    :mask-closable="true"
  >
    <template #header>
      <div class="dialog-head">
        <span class="dialog-method">{{ method || "GET" }}</span>
        <code class="dialog-path">{{ path }}</code>
      </div>
    </template>

    <div class="dialog-body">
      <section class="dialog-pane">
        <header class="pane-head">
          <h3 class="pane-title">Request</h3>
          <span class="pane-hint">用当前 Token 鉴权</span>
        </header>

        <div v-if="allInputs.length" class="param-grid">
          <div v-for="slot in allInputs" :key="slot.name" class="param-row">
            <label class="param-label">{{ slot.kind }} · {{ slot.name }}</label>
            <n-input
              :value="params[slot.name] ?? ''"
              @update:value="(v: string) => (params[slot.name] = v)"
              :placeholder="`<${slot.name}>`"
              size="small"
            />
          </div>
        </div>

        <div class="curl-block">
          <header class="curl-head">
            <span class="curl-label">cURL</span>
            <n-button size="tiny" @click="copyCurl">
              <template #icon><ClipboardCopy :size="12" :stroke-width="1.8" /></template>
              复制
            </n-button>
          </header>
          <n-code :code="curlText" language="bash" class="curl-code" />
        </div>

        <div class="send-row">
          <span class="send-url">{{ fullUrl }}</span>
          <n-button type="primary" :loading="sending" size="small" @click="send">
            <template #icon><Play :size="13" :stroke-width="1.8" /></template>
            发送
          </n-button>
        </div>
      </section>

      <section class="dialog-pane">
        <header class="pane-head">
          <h3 class="pane-title">Response</h3>
        </header>

        <n-empty v-if="!response && !sending" description="未发送" />
        <n-spin v-else-if="sending" />
        <div v-else-if="response" class="response-block">
          <div class="response-meta" :class="`kind-${statusKind}`">
            <span class="response-status">
              <template v-if="response.status > 0">{{ response.status }}</template>
              <template v-else>ERR</template>
            </span>
            <span class="response-ms">{{ response.ms }} ms</span>
            <span v-if="response.error" class="response-error">{{ response.error }}</span>
          </div>
          <n-code
            v-if="!response.error"
            :code="fmtBody(response.body)"
            language="json"
            class="response-body"
          />
          <p v-else class="response-raw">{{ response.body }}</p>
        </div>
      </section>
    </div>
  </n-modal>
</template>

<style scoped>
.dialog-head {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: var(--text-caption);
}
.dialog-method {
  font-family: var(--font-mono);
  font-size: 11px;
  letter-spacing: 0.05em;
  color: var(--color-snow);
  background: var(--color-ash);
  padding: 3px 8px;
  border-radius: 4px;
}
.dialog-path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-snow);
  overflow-x: auto;
  white-space: nowrap;
  max-width: 600px;
  padding: 0;
  background: transparent;
}

.dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.dialog-pane {
  display: flex;
  flex-direction: column;
  gap: 12px;
  background: var(--surface-card);
  border: 1px solid var(--surface-border);
  border-radius: var(--radius-cards);
  padding: 14px 16px;
  min-width: 0;
  min-height: 0;
}
.pane-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.pane-title {
  margin: 0;
  font-size: var(--text-caption);
  font-weight: 500;
  letter-spacing: 0.05em;
  color: var(--color-silver-mist);
  text-transform: uppercase;
}
.pane-hint {
  font-size: var(--text-caption);
  color: var(--color-smoke);
}

.param-grid {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.param-row {
  display: grid;
  grid-template-columns: 92px 1fr;
  gap: 8px;
  align-items: center;
}
.param-label {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--color-smoke);
}

.curl-block {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.curl-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.curl-label {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--color-smoke);
}
.curl-code :deep(pre) {
  max-height: 220px;
  overflow: auto;
  margin: 0;
}

.send-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-top: 4px;
  border-top: 1px solid var(--surface-border);
}
.send-url {
  flex: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--color-snow);
  overflow-x: auto;
  white-space: nowrap;
}

.response-block {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.response-meta {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 10px;
  border-radius: 6px;
  font-family: var(--font-mono);
  font-size: 12px;
  border: 1px solid var(--surface-border);
}
.response-meta.kind-ok {
  border-color: rgba(72, 167, 96, 0.4);
  background: rgba(72, 167, 96, 0.08);
}
.response-meta.kind-warn {
  border-color: rgba(217, 167, 72, 0.4);
  background: rgba(217, 167, 72, 0.08);
}
.response-meta.kind-err {
  border-color: rgba(217, 96, 96, 0.4);
  background: rgba(217, 96, 96, 0.08);
}
.response-status {
  font-weight: 500;
  color: var(--color-snow);
}
.response-ms {
  color: var(--color-smoke);
}
.response-error {
  color: var(--color-snow);
  overflow-x: auto;
  white-space: nowrap;
}
.response-body :deep(pre) {
  max-height: 360px;
  overflow: auto;
  margin: 0;
}
.response-raw {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--color-silver-mist);
  margin: 0;
  padding: 8px;
  background: var(--color-obsidian-deep);
  border: 1px solid var(--surface-border);
  border-radius: 6px;
}
</style>
