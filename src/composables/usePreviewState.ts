/// 全屏预览状态机：open / index / 切换。DetailView 只关心"哪个 cell 被点"
/// 和"用户是否关闭"，不直接管键盘/翻页——这些是 FullscreenPreview 内部的事。

import { ref } from "vue"

export function usePreviewState() {
  const open = ref(false)
  const index = ref(0)

  function show(at: number) {
    index.value = at
    open.value = true
  }
  function close() {
    open.value = false
  }
  function setIndex(at: number) {
    index.value = at
  }

  return { open, index, show, close, setIndex }
}