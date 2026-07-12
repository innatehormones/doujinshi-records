/// 详情页缩略图管线 Worker。
///
/// 主线程把未缓存图片的原图 URL 入队到本 Worker：Worker 负责 fetch 原图、
/// decode、用 OffscreenCanvas 缩放到 ≤maxEdge、编码 webp q=0.7，再把结果
/// blob 交还主线程。缩放/编码全在 Worker 线程完成，避免主线程 canvas 卡顿。
///
/// 消息协议：
///   in : { id, index, url, maxEdge }
///   out: { id, index, blob, cacheable } | { id, index, error }
/// `cacheable=true` 表示图片被缩放并转成 webp，主线程应 PUT 到 /thumb 落缓存；
/// `cacheable=false` 表示原图已 ≤maxEdge，直接展示原图 blob，不写缓存。

export type ThumbRequest = {
  id: number
  index: number
  url: string
  maxEdge: number
}

export type ThumbResponse =
  | { id: number; index: number; blob: Blob; cacheable: boolean }
  | { id: number; index: number; error: string }

async function handle(req: ThumbRequest): Promise<ThumbResponse> {
  const { id, index, url, maxEdge } = req
  try {
    const resp = await fetch(url)
    if (!resp.ok) {
      return { id, index, error: `fetch ${resp.status}` }
    }
    const srcBlob = await resp.blob()
    const bitmap = await createImageBitmap(srcBlob)
    const { width, height } = bitmap

    // 原图已在阈值内：不转码，直接回原图 blob 供展示。
    if (width <= maxEdge && height <= maxEdge) {
      bitmap.close()
      return { id, index, blob: srcBlob, cacheable: false }
    }

    const scale = Math.min(maxEdge / width, maxEdge / height)
    const w = Math.max(1, Math.round(width * scale))
    const h = Math.max(1, Math.round(height * scale))
    const canvas = new OffscreenCanvas(w, h)
    const ctx = canvas.getContext("2d")
    if (!ctx) {
      bitmap.close()
      return { id, index, error: "no 2d context" }
    }
    ctx.drawImage(bitmap, 0, 0, w, h)
    bitmap.close()
    const blob = await canvas.convertToBlob({ type: "image/webp", quality: 0.7 })
    if (blob.type !== "image/webp") {
      return { id, index, error: `encode gave ${blob.type}` }
    }
    return { id, index, blob, cacheable: true }
  } catch (e) {
    return { id, index, error: String(e) }
  }
}

self.onmessage = async (ev: MessageEvent<ThumbRequest>) => {
  const out = await handle(ev.data)
  ;(self as unknown as Worker).postMessage(out)
}
