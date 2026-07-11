export interface FileSummary {
  id: number
  title: string
  circle: string | null
  hash: string
  ext: string
  size_bytes: number
  viewed: boolean
  marked_for_delete: boolean
  physically_deleted: boolean
  current_location: "inbox" | "identified" | "will_delete"
  cover_url: string | null
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

export interface SearchResult {
  items: FileSummary[]
  total: number
}

