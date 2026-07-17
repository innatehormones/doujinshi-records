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
    class="restore-dialog max-w-[480px]"
    @update:show="(v) => !v && emit('cancel')"
  >
    <n-card title="还原文件">
      <p>把 <strong>{{ title }}</strong> 还原回已识别库？</p>
      <n-space justify="end" class="mt-4">
        <n-button @click="emit('cancel')">取消</n-button>
        <n-button type="primary" @click="emit('confirm')">还原</n-button>
      </n-space>
    </n-card>
  </n-modal>
</template>
