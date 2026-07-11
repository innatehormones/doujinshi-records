<script setup lang="ts">
import { onMounted, ref } from "vue"
import {
  NList, NListItem, NThing, NTag, NSpace, NButton, NSpin, NEmpty, NCard, NAlert, NModal,
} from "naive-ui"
import { useInboxStore } from "@/stores"
import type { RarErrorEntry, RarError } from "@/types/api"

const store = useInboxStore()
onMounted(() => store.load())

const WINRAR_URL = "https://www.win-rar.com/"
const SEVENZIP_URL = "https://www.7-zip.org/"

/// Pending entry waiting for the user to confirm a large-RAR
/// extraction. Sub-plan #7-6: 200 MB–1 GB tier gets a confirmation
/// dialog before we re-run the identifier with the size gate off.
const pendingConfirm = ref<RarErrorEntry | null>(null)

async function onRetryLarge(entry: RarErrorEntry) {
  if (entry.error.kind === "too_large" && entry.error.size_mb <= 1024) {
    // 中等档位：弹窗确认。
    pendingConfirm.value = entry
    return
  }
  await doRetry(entry.file_path)
}

async function doRetry(filePath: string) {
  try {
    await store.retryExtractLarge(filePath)
  } catch (e) {
    console.error("retry extract failed:", e)
  }
}

function cancelConfirm() {
  pendingConfirm.value = null
}

async function confirmDialog() {
  const entry = pendingConfirm.value
  pendingConfirm.value = null
  if (entry) await doRetry(entry.file_path)
}

function rarErrorTitle(kind: RarError["kind"]): string {
  switch (kind) {
    case "unrar_not_installed": return "未安装 RAR 工具"
    case "too_large": return "文件过大"
    case "insufficient_space": return "磁盘空间不足"
    case "extraction_failed": return "解压失败"
  }
}
</script>

<template>
  <div>
    <div class="page-header">
      <h1>待识别</h1>
      <span class="count">{{ store.conflicts.length }} 个待处理</span>
    </div>
    <n-card title="待识别 · 文件名冲突">
      <p style="color: #aaa">
        文件名与已识别库内文件相同的压缩包会停在这里。点「跳过」让新文件留在 inbox 不动，或点「内容比对」做内容级决策。
      </p>
    </n-card>

    <!-- RAR 错误卡片：扫描器通过 `rar-error` 事件上报。 -->
    <div v-if="store.rarErrors.length > 0" style="margin-top: 16px">
      <h3 style="margin-bottom: 8px">RAR 处理失败 ({{ store.rarErrors.length }})</h3>
      <n-alert
        v-for="err in store.rarErrors"
        :key="err.file_path"
        :type="err.error.kind === 'unrar_not_installed' || err.error.kind === 'extraction_failed' ? 'error' : 'warning'"
        :title="`${err.filename}：${rarErrorTitle(err.error.kind)}`"
        closable
        @close="store.dismissRarError(err.file_path)"
        style="margin-bottom: 8px"
      >
        <template v-if="err.error.kind === 'unrar_not_installed'">
          本机未安装 RAR 解压工具（WinRAR / 7-Zip），请先安装：
          <n-space style="margin-top: 8px">
            <n-button tag="a" :href="WINRAR_URL" target="_blank" type="primary">
              下载 WinRAR
            </n-button>
            <n-button tag="a" :href="SEVENZIP_URL" target="_blank">
              下载 7-Zip
            </n-button>
          </n-space>
        </template>

        <template v-else-if="err.error.kind === 'too_large'">
          文件过大（{{ err.error.size_mb.toFixed(0) }} MB &gt; {{ err.error.limit_mb }} MB），
          已拒绝解压。请确认磁盘空间足够后再试：
          <n-space style="margin-top: 8px">
            <n-button
              type="warning"
              @click="onRetryLarge(err)"
            >
              仍要解压
            </n-button>
          </n-space>
        </template>

        <template v-else-if="err.error.kind === 'insufficient_space'">
          磁盘空间不足：解压需 {{ err.error.needed_mb.toFixed(0) }} MB，
          剩余 {{ err.error.available_mb }} MB。
        </template>

        <template v-else>
          解压失败：{{ err.error.message }}
        </template>
      </n-alert>
    </div>

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
              <router-link
                :to="{ name: 'compare', params: { id: c.id } }"
                custom
                v-slot="{ navigate }"
              >
                <n-button size="small" type="primary" @click="navigate">
                  内容比对
                </n-button>
              </router-link>
              <n-button size="small" @click="store.resolve(c.id)">
                跳过
              </n-button>
            </n-space>
          </template>
        </n-list-item>
      </n-list>
    </n-spin>

    <!-- 中等 RAR 二次确认弹窗（task #7-6） -->
    <n-modal
      :show="pendingConfirm !== null"
      preset="card"
      title="确认解压较大文件"
      style="max-width: 480px"
      :mask-closable="false"
      @close="cancelConfirm"
    >
      <template v-if="pendingConfirm && pendingConfirm.error.kind === 'too_large'">
        <p>
          文件 <strong>{{ pendingConfirm.filename }}</strong>
          体积 {{ pendingConfirm.error.size_mb.toFixed(0) }} MB，
          接近系统承受上限（1024 MB）。
        </p>
        <p style="color: #aaa; font-size: 13px">
          解压到 identified 目录会先占用临时空间（自动清理），请确认目标盘剩余空间足够。
        </p>
      </template>
      <template #action>
        <n-space justify="end">
          <n-button @click="cancelConfirm">取消</n-button>
          <n-button type="warning" @click="confirmDialog">确认解压</n-button>
        </n-space>
      </template>
    </n-modal>
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