<script setup lang="ts">
import { NModal, NSpace, NButton, NCard } from "naive-ui"

defineProps<{ show: boolean; title: string }>()
const emit = defineEmits<{
  (e: "cancel"): void
  (e: "confirm"): void
}>()
</script>

<template>
  <n-modal
    :show="show"
    @update:show="(v) => !v && emit('cancel')"
  >
    <n-card style="width: 480px" title="永久删除">
      <p>确认永久删除 <strong>{{ title }}</strong>？<</p>
      <p style="color: #aaa; font-size: 12px">
        文件将从硬盘删除。数据记录保留（标题、哈希、元数据仍可搜索到）。
      </p>
      <n-space justify="space-between">
        <n-button @click="emit('cancel')">取消</n-button>
        <n-button type="error" @click="emit('confirm')">
          永久删除
        </n-button>
      </n-space>
    </n-card>
  </n-modal>
</template>

