/// V4 业务 status / 文件 file_state 的 Naive UI tag 颜色映射——唯一权威。
///
/// 中文标签**不**在此收口：FileCard 用紧凑短标签（入库 / 回收），
/// DetailView 用完整标签（已入库 / 文件回收站），是各自语境下的产品
/// 意图，故只共享颜色逻辑，标签留在组件本地。

export type TagType = "default" | "primary" | "info" | "success" | "warning" | "error"

const STATUS_TAG_TYPE: Record<string, TagType> = {
  in_library: "success",
  archived: "info",
  recycle: "warning",
  deleted: "error",
}

const FILE_STATE_TAG_TYPE: Record<string, TagType> = {
  present: "success",
  missing: "error",
  absent_confirmed: "error",
}

export function statusTagType(status: string): TagType {
  return STATUS_TAG_TYPE[status] ?? "default"
}

export function fileStateTagType(state: string): TagType {
  return FILE_STATE_TAG_TYPE[state] ?? "default"
}
