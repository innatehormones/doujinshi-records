import { invoke } from "@tauri-apps/api/core"
import type {
  FileSummary,
  SettingsView,
  ConflictItem,
  ConflictAction,
  DirtyEntry,
  Page,
  RecyclePage,
  CircleCount,
  ScanStatus,
  ReparseResult,
} from "@/types/api"

export const api = {
  listLibrary: (
    q?: string,
    viewed?: string,
    status?: string,
    limit?: number,
    offset?: number,
  ) =>
    invoke<Page<FileSummary>>("list_library", { q, viewed, status, limit, offset }),
  topCircles: (limit?: number) =>
    invoke<CircleCount[]>("top_circles", { limit }),
  getById: (id: number) => invoke<FileSummary>("get_by_id", { id }),
  reparseMetadata: (id: number) => invoke<ReparseResult>("reparse_metadata", { id }),
  markForDelete: (id: number) => invoke<void>("mark_for_delete", { id }),
  archive: (id: number) => invoke<void>("archive", { id }),
  restore: (id: number) => invoke<void>("restore", { id }),
  listRecycle: (
    presentLimit?: number,
    presentOffset?: number,
    goneLimit?: number,
    goneOffset?: number,
  ) =>
    invoke<RecyclePage>("list_recycle", {
      presentLimit,
      presentOffset,
      goneLimit,
      goneOffset,
    }),
  permanentDelete: (id: number) => invoke<void>("permanent_delete", { id }),
  restoreFromRecycle: (id: number) => invoke<void>("restore_from_recycle", { id }),
  listConflicts: (limit?: number, offset?: number) =>
    invoke<Page<ConflictItem>>("list_conflicts", { limit, offset }),
  resolveConflict: (id: number, action?: ConflictAction) =>
    invoke<void>("resolve_conflict", { id, action }),
  listDirty: (limit?: number, offset?: number) =>
    invoke<Page<DirtyEntry>>("list_dirty", { limit, offset }),
  getSettings: () => invoke<SettingsView>("get_settings"),
  manualScan: () => invoke<number>("manual_scan"),
  getScanStatus: () => invoke<ScanStatus>("get_scan_status"),
  regenerateAuthToken: () => invoke<string>("regenerate_auth_token"),
  setHttpPort: (port: number) => invoke<void>("set_http_port", { port }),
  forceExtract: (path: string) => invoke<void>("force_extract", { path }),
}

