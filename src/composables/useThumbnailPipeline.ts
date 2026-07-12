/// 缩略图管线：把"按需→Worker 转码→LRU 缓存→展示 URL"封装成一个 hook。
///
/// 解决的问题：DetailView 一次性挂几百个 `<img>` 时 WebView2 并行解码 +
/// n-image 内部 wrapper 会卡。改成：
/// 1. cell 进入视口才调度（IntersectionObserver 触发 `request(index)`）
/// 2. 已缓存（thumb_cached=true）直挂后端 URL；未缓存入队 Worker
/// 3. Worker 转 800px webp → 主线程 PUT 落 LRU（cacheable=true 时）
///
/// 为什么用 composable：调度状态（queue/inFlight/worker/blob URL 集）天然
/// 是"按当前文件 scope"的，不跨文件复用。把它从 DetailView 拆出来，
/// DetailView 只剩"何时调 request(index)"和"模板里 `:src` 用谁"。

import { onBeforeUnmount, ref, watch, type Ref } from "vue"
import type { DetailImage } from "@/types/api"
import { putImageThumb } from "@/api/http"
import type { ThumbResponse } from "@/workers/previewThumb.worker"

const PREVIEW_MAX_EDGE = 800
const WORKER_CONCURRENCY = 2

type Job = { img: DetailImage; index: number; fileId: number }

function keyFor(fileId: number, index: number): string {
  return `${fileId}:${index}`
}

function workerSupported(): boolean {
  return (
    typeof Worker !== "undefined" &&
    typeof OffscreenCanvas !== "undefined" &&
    typeof createImageBitmap === "function"
  )
}

export function useThumbnailPipeline(opts: {
  fileId: Ref<number>
  apiBase: Ref<string>
  images: Ref<DetailImage[]>
}) {
  /// 每张图的展示 URL。undefined 表示未调度；string 是 URL（http URL 或 blob:）。
  const thumbSrc = ref<Record<number, string | undefined>>({})
  /// 已加载索引：用于 `<img>` opacity 0→1 过渡，避免挂载瞬间到 onLoad
  /// 之间的视觉断层（闪烁）。
  const loaded = ref(new Set<number>())

  const blobUrls = new Set<string>()
  const queue: Job[] = []
  const jobByKey = new Map<string, Job>()
  let inFlight = 0
  let worker: Worker | null = null

  function apiThumbUrl(index: number): string {
    return `${opts.apiBase.value}/api/doujinshi/${opts.fileId.value}/images/${index}`
  }

  function getWorker(): Worker | null {
    if (!workerSupported()) return null
    if (!worker) {
      worker = new Worker(
        new URL("../workers/previewThumb.worker.ts", import.meta.url),
        { type: "module" },
      )
      worker.onmessage = (ev: MessageEvent<ThumbResponse>) => onMessage(ev.data)
    }
    return worker
  }

  function reset() {
    for (const u of blobUrls) URL.revokeObjectURL(u)
    blobUrls.clear()
    queue.length = 0
    jobByKey.clear()
    inFlight = 0
    thumbSrc.value = {}
    loaded.value = new Set()
  }

  function drain() {
    const w = getWorker()
    if (!w) return
    while (inFlight < WORKER_CONCURRENCY && queue.length > 0) {
      const job = queue.shift()
      if (!job) break
      inFlight += 1
      w.postMessage({
        id: job.fileId,
        index: job.index,
        url: apiThumbUrl(job.index),
        maxEdge: PREVIEW_MAX_EDGE,
      })
    }
  }

  function onMessage(msg: ThumbResponse) {
    inFlight = Math.max(0, inFlight - 1)
    const job = jobByKey.get(keyFor(msg.id, msg.index))
    jobByKey.delete(keyFor(msg.id, msg.index))
    /// 文件已切走 / 任务被丢弃：忽略，继续排空队列。
    if (msg.id !== opts.fileId.value || !job) {
      drain()
      return
    }
    if ("error" in msg) {
      /// 转码失败：退回展示后端图（命中 webp，未命中走原图 mime）。
      thumbSrc.value = { ...thumbSrc.value, [job.index]: apiThumbUrl(job.index) }
      drain()
      return
    }
    const url = URL.createObjectURL(msg.blob)
    blobUrls.add(url)
    thumbSrc.value = { ...thumbSrc.value, [job.index]: url }
    if (msg.cacheable) {
      /// 落盘成功且仍在当前文件：把 images[i].thumb_cached 标 true，避免
      /// 之后重新进入时再跑 Worker（detail_images 重新拉会刷新该字段）。
      void putImageThumb(msg.id, msg.index, msg.blob)
        .then((resp) => {
          if (!resp.ok || opts.fileId.value !== msg.id) return
          const arr = opts.images.value
          const cur = arr[msg.index]
          if (!cur || cur.url !== job.img.url) return
          opts.images.value = arr.map((it, i) =>
            i === msg.index ? { ...it, thumb_cached: true } : it,
          )
        })
        .catch(() => {})
    }
    drain()
  }

  /// 由 IntersectionObserver 回调触发：仅调度单张可见 cell。
  /// thumbSrc[index] 已设值（不论是缓存 URL 还是 blob URL）则跳过。
  function request(index: number) {
    if (thumbSrc.value[index] !== undefined) return
    const img = opts.images.value[index]
    if (!img) return
    const w = getWorker()
    if (img.thumb_cached || !w) {
      thumbSrc.value = { ...thumbSrc.value, [index]: apiThumbUrl(index) }
      return
    }
    const job: Job = { img, index, fileId: opts.fileId.value }
    jobByKey.set(keyFor(opts.fileId.value, index), job)
    queue.push(job)
    drain()
  }

  function markLoaded(index: number) {
    if (loaded.value.has(index)) return
    const next = new Set(loaded.value)
    next.add(index)
    loaded.value = next
  }

  /// images 重置时清空所有派生状态（URL / 队列 / blob）。
  watch(
    opts.images,
    () => {
      reset()
    },
    { flush: "post" },
  )

  onBeforeUnmount(() => {
    for (const u of blobUrls) URL.revokeObjectURL(u)
    blobUrls.clear()
    if (worker) {
      worker.terminate()
      worker = null
    }
  })

  return { thumbSrc, loaded, request, markLoaded, reset }
}