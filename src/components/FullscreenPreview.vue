<script setup lang="ts">
/// 全屏图片预览：弹层显示当前图 + 左右翻页 + 关闭。
///
/// 性能原则：每张图就是后端 `/api/doujinshi/:id/images/:idx` 返的 ≤800px
/// webp（或 cache miss 时后端直返的原图 mime），不做放大、不另取原图。
/// 翻页即时：预读左右各 1 张的隐藏 `<img>` 让浏览器提前建连/缓存。
///
/// 关闭：仅右上 × 按钮或 Esc 键。点击遮罩空白不关（避免误触）。

import { computed, onMounted, onUnmounted } from "vue"
import type { DetailImage } from "@/types/api"

const props = defineProps<{
  fileId: number
  images: DetailImage[]
  /// 0-based 索引；非法值会被夹紧。
  initialIndex: number
  apiBase: string
}>()

const emit = defineEmits<{
  (e: "close"): void
  (e: "change", index: number): void
}>()

/// 当前显示索引：getter 夹紧越界值，setter 同步给父组件（避免双向耦合）。
const index = computed({
  get() {
    const n = props.images.length
    if (n === 0) return 0
    const i = Math.floor(props.initialIndex)
    if (i < 0) return 0
    if (i >= n) return n - 1
    return i
  },
  set(v: number) {
    const n = props.images.length
    if (n === 0) return
    const clamped = Math.max(0, Math.min(n - 1, Math.floor(v)))
    if (clamped !== props.initialIndex) emit("change", clamped)
  },
})

const prevDisabled = computed(() => index.value === 0)
const nextDisabled = computed(() => index.value >= props.images.length - 1)

function srcFor(i: number): string {
  return `${props.apiBase}/api/doujinshi/${props.fileId}/images/${i}`
}

const currentSrc = computed(() => srcFor(index.value))
const prevSrc = computed(() => (prevDisabled.value ? "" : srcFor(index.value - 1)))
const nextSrc = computed(() => (nextDisabled.value ? "" : srcFor(index.value + 1)))

function onKey(ev: KeyboardEvent) {
  /// 输入框/textarea 聚焦时让浏览器原生处理 ←/→（光标移动）。
  const t = ev.target as HTMLElement | null
  if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) {
    return
  }
  if (ev.key === "Escape") {
    ev.preventDefault()
    emit("close")
  } else if (ev.key === "ArrowLeft" && !prevDisabled.value) {
    ev.preventDefault()
    index.value = index.value - 1
  } else if (ev.key === "ArrowRight" && !nextDisabled.value) {
    ev.preventDefault()
    index.value = index.value + 1
  }
}

onMounted(() => window.addEventListener("keydown", onKey))
onUnmounted(() => window.removeEventListener("keydown", onKey))
</script>

<template>
  <div class="fixed inset-0 z-[1000] flex select-none items-center justify-center bg-black/88">
    <button
      class="absolute top-3 right-4 flex size-9 cursor-pointer items-center justify-center rounded-full border-0 bg-white/12 text-2xl leading-none text-white hover:bg-white/20"
      type="button"
      aria-label="关闭"
      @click="emit('close')"
    >
      ×
    </button>
    <button
      class="absolute top-1/2 left-3 h-16 w-12 -translate-y-1/2 cursor-pointer rounded border-0 bg-white/8 text-4xl leading-none text-white enabled:hover:bg-white/18 disabled:cursor-not-allowed disabled:opacity-25"
      type="button"
      :disabled="prevDisabled"
      aria-label="上一张"
      @click="index = index - 1"
    >
      ‹
    </button>
    <button
      class="absolute top-1/2 right-3 h-16 w-12 -translate-y-1/2 cursor-pointer rounded border-0 bg-white/8 text-4xl leading-none text-white enabled:hover:bg-white/18 disabled:cursor-not-allowed disabled:opacity-25"
      type="button"
      :disabled="nextDisabled"
      aria-label="下一张"
      @click="index = index + 1"
    >
      ›
    </button>
    <div class="flex max-h-[88vh] max-w-[92vw] items-center justify-center">
      <img
        v-if="currentSrc"
        :src="currentSrc"
        :alt="images[index]?.name ?? ''"
        class="block max-h-[88vh] max-w-[92vw] object-contain"
      />
    </div>
    <div
      v-if="images.length > 0"
      class="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-xl bg-black/35 px-2.5 py-1 text-[13px] text-white/85"
    >
      {{ index + 1 }} / {{ images.length }}
    </div>
    <!-- 预读左右各 1 张：隐藏 img 让浏览器提前建连/缓存。 -->
    <div class="invisible pointer-events-none absolute size-0 overflow-hidden" aria-hidden="true">
      <img v-if="prevSrc" :src="prevSrc" alt="" />
      <img v-if="nextSrc" :src="nextSrc" alt="" />
    </div>
  </div>
</template>
