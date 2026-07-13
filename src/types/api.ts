export interface FileSummary {
  id: number
  title: string
  circle: string | null
  hash: string
  ext: string
  size_bytes: number
  current_location: "inbox" | "identified" | "will_delete" | "archived"
  has_physical_file: boolean
  cover_url: string | null
}

export interface DirtyEntry {
  id: number
  file_path: string
  file_size: number
  detected_dir: "identified" | "will_delete" | "archived"
  reason: string
  first_seen_at: string
}

export interface SettingsView {
  resources_dir: string
  inbox_dir: string
  identified_dir: string
  will_delete_dir: string
  covers_dir: string
  api_url: string
  scanner_watching: boolean
  auth_token: string
  http_port: number
  http_port_locked: boolean
}

export interface ConflictItem {
  id: number
  a_file_id: number
  a_title: string
  b_filename: string
  b_file_path: string
  created_at: string
}

export interface ConflictCompareSide {
  file_id: number
  title: string
  hash: string | null
  cover_url: string | null
  image_names: string[]
  file_path: string
  zip_missing: boolean
  zip_error: string | null
}

export interface ConflictCompare {
  conflict_id: number
  a: ConflictCompareSide
  b: ConflictCompareSide
}

export type ConflictAction = "keep_a" | "replace_b" | "keep_both" | "skip"

/// Partial update body for `PATCH /api/doujinshi/:id`. Fields set
/// to `undefined` are left untouched on the server side.
export interface MetadataPatch {
  title?: string
  circle?: string | null
  series?: string | null
  translator?: string | null
  version?: string | null
  note?: string | null
  rating?: number | null
}

export interface DetailImage {
  name: string
  /// Path-only image URL — frontend prepends `useSettingsStore.apiBase`.
  /// Bytes served by `GET /api/doujinshi/:id/images/:index`.
  url: string
  /// True when the backend already has a webp thumbnail for this image.
  thumb_cached: boolean
}

export interface DetailImagesResponse {
  file_id: number
  images: DetailImage[]
  /// `true` when the archive file no longer exists on disk.
  zip_missing: boolean
}

export interface SearchResult {
  items: FileSummary[]
  total: number
}

/// 通用分页响应：与 Rust `Page<T>` 一一对应。
/// `limit` 是请求方持有的当前页大小，不必回传——前端 store 内部跟踪。
export interface Page<T> {
  items: T[]
  total: number
}

/// 回收站首页专属 shape：present（硬盘还有）+ gone（已清走）。
/// 两段各自独立分页，各自的 total 独立计算。
export interface RecyclePage {
  present: Page<FileSummary>
  gone: Page<FileSummary>
}

/// Library 顶部社团 chip：按文件数倒序，独立端点而不是从当前页聚合。
export interface CircleCount {
  circle: string
  count: number
}

/// RAR 处理的错误分类。前端按 kind 渲染不同的卡片：
/// `unrar_not_installed` 显示下载链接；`too_large` 可弹窗确认；
/// `insufficient_space` 仅展示；`extraction_failed` 仅展示。
export type RarError =
  | { kind: "unrar_not_installed" }
  | { kind: "too_large"; size_mb: number; limit_mb: number }
  | { kind: "insufficient_space"; needed_mb: number; available_mb: number }
  | { kind: "extraction_failed"; message: string }

/// 单条 RAR 错误记录（带文件名，方便用户在 inbox 里识别）。
export interface RarErrorEntry {
  filename: string
  file_path: string
  error: RarError
}

