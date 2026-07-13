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
    <n-card class="w-[420px]" title="还原文件">
      <p>把 <strong>{{ title }}</strong> 还原回已识别库？</p>
      <n-space justify="space-between">
        <n-button type="primary" @click="emit('confirm')">还原</n-button>
        <n-button @click="emit('cancel')">取消</n-button>
      </n-space>
    </n-card>
  </n-modal>
</template>

