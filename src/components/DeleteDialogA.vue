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
    <n-card style="width: 420px" title="标记为待删除">
      <p>把 <strong>{{ title }}</strong> 标记为待删除？</p>
      <p style="color: #aaa; font-size: 12px">
        在卡片上点「取消标记」可以撤销。标记后才能进入下一步（移到待删除目录）。
      </p>
      <n-space justify="end">
        <n-button @click="emit('cancel')">取消</n-button>
        <n-button type="warning" @click="emit('confirm')">
          标记删除
        </n-button>
      </n-space>
    </n-card>
  </n-modal>
</template>

