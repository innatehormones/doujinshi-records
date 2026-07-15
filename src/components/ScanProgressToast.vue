<script setup lang="ts">
import { computed } from "vue"
import { NProgress, NIcon } from "naive-ui"
import { X, Loader2, CheckCircle2 } from "@lucide/vue"
import { useScanStatusStore } from "@/stores"

const store = useScanStatusStore()

const percent = computed(() => {
  const s = store.status
  if (!s || s.total === 0) return 0
  return Math.round((s.processed / s.total) * 100)
})

const status = computed(() => store.status)
</script>

<template>
  <transition name="scan-toast">
    <div
      v-if="store.visible"
      class="scan-toast fixed bottom-6 right-6 z-50 w-[320px] rounded-cards border border-border bg-card/95 p-4 shadow-2xl backdrop-blur"
    >
      <div class="flex items-start justify-between gap-3">
        <div class="flex min-w-0 flex-1 items-center gap-2">
          <n-icon v-if="status?.is_scanning" :size="16" class="scan-spin text-archive-blue">
            <Loader2 :stroke-width="1.8" />
          </n-icon>
          <n-icon v-else :size="16" class="text-phosphor-green">
            <CheckCircle2 :stroke-width="1.8" />
          </n-icon>
          <span class="truncate text-body-sm font-medium text-snow">
            {{ status?.is_scanning ? "正在识别压缩包" : "扫描已完成" }}
          </span>
        </div>
        <button
          class="inline-flex size-5 shrink-0 items-center justify-center rounded-full text-silver-mist transition-colors hover:bg-snow/8 hover:text-snow"
          aria-label="关闭"
          @click="store.dismiss()"
        >
          <X :size="14" :stroke-width="1.8" />
        </button>
      </div>

      <n-progress
        type="line"
        :percentage="percent"
        :show-indicator="false"
        :height="6"
        class="mt-3"
      />

      <div class="mt-2 flex items-center justify-between text-caption text-silver-mist">
        <span class="font-mono tracking-[0.05em]">
          {{ status?.processed ?? 0 }} / {{ status?.total ?? 0 }}
        </span>
        <span v-if="(status?.failed ?? 0) > 0" class="text-ember-red">
          {{ status?.failed }} 失败
        </span>
      </div>
    </div>
  </transition>
</template>

<style scoped>
.scan-spin {
  animation: spin 1s linear infinite;
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.scan-toast-enter-active,
.scan-toast-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.scan-toast-enter-from,
.scan-toast-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
