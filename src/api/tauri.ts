import { invoke } from "@tauri-apps/api/core"
import type { FileSummary, SettingsView, ConflictItem, ConflictAction, DirtyEntry } from "@/types/api"

export const api = {
  listLibrary: (q?: string, location?: string) =>
    invoke<FileSummary[]>("list_library", { q, location }),
  getById: (id: number) => invoke<FileSummary>("get_by_id", { id }),
  markForDelete: (id: number) => invoke<void>("mark_for_delete", { id }),
  unmarkForDelete: (id: number) => invoke<void>("unmark_for_delete", { id }),
  moveToWillDelete: (id: number) => invoke<void>("move_to_will_delete", { id }),
  archive: (id: number) => invoke<void>("archive", { id }),
  restore: (id: number) => invoke<void>("restore", { id }),
  listRecycle: () =>
    invoke<[FileSummary[], FileSummary[]]>("list_recycle"),
  permanentDelete: (id: number) => invoke<void>("permanent_delete", { id }),
  restoreFromRecycle: (id: number) => invoke<void>("restore_from_recycle", { id }),
  listConflicts: () => invoke<ConflictItem[]>("list_conflicts"),
  resolveConflict: (id: number, action?: ConflictAction) =>
    invoke<void>("resolve_conflict", { id, action }),
  listDirty: () => invoke<DirtyEntry[]>("list_dirty"),
  getSettings: () => invoke<SettingsView>("get_settings"),
  manualScan: () => invoke<number>("manual_scan"),
  regenerateAuthToken: () => invoke<string>("regenerate_auth_token"),
  setHttpPort: (port: number) => invoke<void>("set_http_port", { port }),
  forceExtract: (path: string) => invoke<void>("force_extract", { path }),
}

