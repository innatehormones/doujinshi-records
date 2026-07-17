export interface FileSummary {
  id: number
  title: string
  filename: string
  circle: string | null
  series: string | null
  translator: string | null
  note: string | null
  rating: number | null
  hash: string
  ext: string
  size_bytes: number
  viewed: boolean
  /// V4：业务状态。`in_library` / `archived` / `recycle` / `deleted`。
  /// V3 时代的 `current_location`（含 `inbox` 投影）已废弃。
  status: "in_library" | "archived" | "recycle" | "deleted"
  /// V4：文件状态。`present` 文件还在期望位置；`missing` 启动时
  /// dirty_scanner 发现文件丢失；`absent_confirmed` 走永久删除
  /// 流程后用户已确认。前端按 (status, file_state) 组合决定 UI 提示。
  file_state: "present" | "missing" | "absent_confirmed"
  cover_url: string | null
  /// V4：后端 `commands::guards::ensure_no_open_conflict` 兜底拦截，
  /// 浏览器扩展或直接 HTTP 也绕不开。
  has_open_conflict: boolean
}

export interface DirtyEntry {
  id: number
  file_path: string
  file_size: number
  detected_dir: "identified" | "will_delete" | "archived"
  reason: string
  first_seen_at: string
  resolved_at: string | null
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
  a_cover_url: string | null
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

/// `filename_parser` 在已入库文件名上的回显结果——DetailView 「重新解析
/// 元数据」按钮的返回。**纯函数，不写 DB**：前端拿到后更新表单，等用户
/// 点「保存」才落库。
export interface ReparseResult {
  filename: string
  title: string
  circle: string | null
  series: string | null
  translator: string | null
}

/// 通用分页响应：与 Rust `Page<T>` 一一对应。
/// `limit` 是请求方持有的当前页大小，不必回传——前端 store 内部跟踪。
export interface Page<T> {
  items: T[]
  total: number
}

/// 文件回收站首页专属 shape：V4.6 起只展示「待删除文件」
/// （status='recycle' + file_state='present'）。原本按 file_state 三态
/// 分 present / gone 两段，gone 段已移除——对应记录可在 Library 用
/// status filter（recycle / deleted）找到。
export interface RecyclePage {
  present: Page<FileSummary>
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

/// 扫描器状态：与 Rust `services::scanner::ScanStatus` 一一对应。
/// `is_scanning` true 期间 `processed` 持续增长；完成时 `is_scanning`
/// 变 false 并保留 `processed / total / failed` 供浮窗显示最终结果。
export interface ScanStatus {
  is_scanning: boolean
  processed: number
  total: number
  failed: number
}

/// 备份配置：与 Rust `BackupConfig` 一一对应。`dir=""` 表示用默认目录。
export interface BackupConfig {
  dir: string
  retention_count: number
}

/// 单条备份快照：与 Rust `SnapshotInfo` 一一对应。
export interface BackupSnapshot {
  path: string
  size_bytes: number
  /// RFC3339 字符串（atime）；前端直接展示，不解析
  mtime: string
}

/// `backup_now` 返回：与 Rust `BackupResult` 一一对应。
/// `skipped` 非空表示本次因内容未变跳过。
export interface BackupResult {
  path: string
  size_bytes: number
  md5: string
  skipped?: string
}

