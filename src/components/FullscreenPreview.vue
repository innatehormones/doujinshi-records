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
  <div class="preview-overlay">
    <button class="preview-close" type="button" aria-label="关闭" @click="emit('close')">×</button>
    <button
      class="preview-nav preview-prev"
      type="button"
      :disabled="prevDisabled"
      aria-label="上一张"
      @click="index = index - 1"
    >
      ‹
    </button>
    <button
      class="preview-nav preview-next"
      type="button"
      :disabled="nextDisabled"
      aria-label="下一张"
      @click="index = index + 1"
    >
      ›
    </button>
    <div class="preview-stage">
      <img v-if="currentSrc" :src="currentSrc" :alt="images[index]?.name ?? ''" class="preview-img" />
    </div>
    <div v-if="images.length > 0" class="preview-counter">
      {{ index + 1 }} / {{ images.length }}
    </div>
    <!-- 预读左右各 1 张：隐藏 img 让浏览器提前建连/缓存。 -->
    <div class="preview-preload" aria-hidden="true">
      <img v-if="prevSrc" :src="prevSrc" alt="" />
      <img v-if="nextSrc" :src="nextSrc" alt="" />
    </div>
  </div>
</template>

<style scoped>
.preview-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.88);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  user-select: none;
}
.preview-close {
  position: absolute;
  top: 12px;
  right: 16px;
  width: 36px;
  height: 36px;
  border: none;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
  font-size: 24px;
  line-height: 1;
  cursor: pointer;
}
.preview-close:hover { background: rgba(255, 255, 255, 0.2); }
.preview-nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  width: 48px;
  height: 64px;
  border: none;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.08);
  color: #fff;
  font-size: 36px;
  line-height: 1;
  cursor: pointer;
}
.preview-nav:hover:not(:disabled) { background: rgba(255, 255, 255, 0.18); }
.preview-nav:disabled { opacity: 0.25; cursor: not-allowed; }
.preview-prev { left: 12px; }
.preview-next { right: 12px; }
.preview-stage {
  max-width: 92vw;
  max-height: 88vh;
  display: flex;
  align-items: center;
  justify-content: center;
}
.preview-img {
  max-width: 92vw;
  max-height: 88vh;
  object-fit: contain;
  display: block;
}
.preview-counter {
  position: absolute;
  bottom: 16px;
  left: 50%;
  transform: translateX(-50%);
  color: rgba(255, 255, 255, 0.85);
  font-size: 13px;
  background: rgba(0, 0, 0, 0.35);
  padding: 4px 10px;
  border-radius: 12px;
}
.preview-preload {
  position: absolute;
  width: 0;
  height: 0;
  overflow: hidden;
  visibility: hidden;
  pointer-events: none;
}
</style>