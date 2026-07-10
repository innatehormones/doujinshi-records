<script setup lang="ts">
import { NModal, NSpace, NButton, NCard, NTag } from "naive-ui"

defineProps<{ show: boolean; title: string; size: string }>()
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
    <n-card style="width: 480px" title="确认移到待删除目录">
      <p>文件: <strong>{{ title }}</strong>（{{ size }}）</p>
      <p>
        从: <n-tag>已识别</n-tag>
        到: <n-tag type="warning">待删除</n-tag>
      </p>
      <p style="color: #aaa; font-size: 12px">
        文件将被物理移动。之后可以在回收站页面还原或永久删除（数据记录保留）。
      </p>
      <n-space justify="space-between" align="center">
        <n-button @click="emit('cancel')">取消</n-button>
        <n-button type="error" @click="emit('confirm')">
          移到待删除
        </n-button>
      </n-space>
    </n-card>
  </n-modal>
</template>

