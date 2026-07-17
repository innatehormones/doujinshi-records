<script setup lang="ts">
import { ref, onMounted, computed } from "vue"
import { useRoute, useRouter } from "vue-router"
import { ArrowLeft } from "@lucide/vue"
import { NCard, NButton, NSpin, NList, NListItem, NEmpty, NAlert, useMessage } from "naive-ui"
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
  <div class="page">
    <header class="flex items-baseline justify-between gap-4">
      <n-button text @click="router.back()">
        <template #icon>
          <ArrowLeft :size="16" :stroke-width="1.8" />
        </template>
        返回
      </n-button>
      <h1 class="text-heading-sm font-medium text-snow tracking-body flex-1">冲突对比</h1>
      <span class="font-mono text-caption text-smoke tracking-[0.1em]">
        冲突 #{{ conflictId }}
      </span>
    </header>

    <n-spin :show="loading || acting">
      <n-empty
        v-if="!loading && !data"
        description="加载失败或该冲突已处理"
      />
      <div v-if="data" class="grid grid-cols-[repeat(auto-fit,minmax(420px,1fr))] gap-4">
        <n-card title="A · 已识别">
          <div class="mb-3 flex gap-4">
            <img
              v-if="data.a.cover_url"
              :src="settings.apiBase + data.a.cover_url"
              alt="A 封面"
              class="max-w-40 rounded border border-border"
            />
            <div class="flex-1 text-[13px]">
              <div><strong>标题：</strong> {{ data.a.title }}</div>
              <div v-if="data.a.hash" class="break-all font-mono text-caption text-smoke">
                作品文件哈希: {{ data.a.hash }}
              </div>
              <div class="break-all font-mono text-smoke">作品入库序号: {{ data.a.file_id }}</div>
            </div>
          </div>
          <n-alert
            v-if="data.a.zip_missing"
            type="warning"
            title="A 文件已不在磁盘"
            class="my-2"
          />
          <n-alert
            v-if="data.a.zip_error"
            type="error"
            :title="data.a.zip_error"
            class="my-2"
          />
          <h3 class="text-subheading font-medium text-snow tracking-body">文件列表 ({{ data.a.image_names.length }})</h3>
          <n-empty
            v-if="data.a.image_names.length === 0"
            description="(无图片)"
            size="small"
          />
          <n-list v-else bordered class="max-h-[calc(100vh-488px)] overflow-auto">
            <n-list-item v-for="n in data.a.image_names" :key="n">
              {{ n }}
            </n-list-item>
          </n-list>
        </n-card>

        <n-card title="B · inbox 待处理">
          <div class="mb-3 flex">
            <div class="flex-1 text-[13px]">
              <div class="text-[13px]">
                <div><strong>文件名：</strong> {{ data.b.title }}</div>
                <div class="break-all font-mono text-smoke">文件路径: {{ data.b.file_path || "(未取)" }}</div>
              </div>
              <n-alert
                v-if="data.b.zip_missing"
                type="warning"
                title="B 文件已不在磁盘"
                class="my-2"
              />
              <n-alert
                v-if="data.b.zip_error"
                type="error"
                :title="data.b.zip_error"
                class="my-2"
              />
            </div>
            <div class="w-0 -z-1">
              <!-- 由a的图片撑开高度，使得布局统一 -->
              <img
                v-if="data.a.cover_url"
                :src="settings.apiBase + data.a.cover_url"
                alt="A 封面"
                class="max-w-40 rounded border border-border"
              />
            </div>
          </div>
          <h3 class="text-subheading font-medium text-snow tracking-body">文件列表 ({{ data.b.image_names.length }})</h3>
          <n-empty
            v-if="data.b.image_names.length === 0"
            description="(无图片)"
            size="small"
          />
          <n-list v-else bordered class="max-h-[calc(100vh-488px)] overflow-auto">
            <n-list-item v-for="n in data.b.image_names" :key="n">
              {{ n }}
            </n-list-item>
          </n-list>
        </n-card>
      </div>

      <div v-if="data" class="mt-4 flex flex-wrap justify-end gap-2">
        <n-button @click="act('skip')">都跳过</n-button>
        <n-button type="primary" @click="act('keep_both')">都保留（加后缀入库）</n-button>
        <n-button @click="act('keep_a')">保留 A（删 B）</n-button>
        <n-button type="warning" @click="act('replace_b')">替换为 B（删 A）</n-button>
      </div>
    </n-spin>
  </div>
</template>
