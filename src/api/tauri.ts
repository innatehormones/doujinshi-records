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
  BackupConfig,
  BackupSnapshot,
  BackupResult,
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
  markForDelete: (id: number) => invoke<void>("mark_for_delete", { id }),
  archive: (id: number) => invoke<void>("archive", { id }),
  restore: (id: number) => invoke<void>("restore", { id }),
  listRecycle: (
    presentLimit?: number,
    presentOffset?: number,
  ) =>
    invoke<RecyclePage>("list_recycle", {
      presentLimit,
      presentOffset,
    }),
  permanentDelete: (id: number) => invoke<void>("permanent_delete", { id }),
  restoreFromRecycle: (id: number) => invoke<void>("restore_from_recycle", { id }),
  listConflicts: (limit?: number, offset?: number) =>
    invoke<Page<ConflictItem>>("list_conflicts", { limit, offset }),
  resolveConflict: (id: number, action?: ConflictAction) =>
    invoke<void>("resolve_conflict", { id, action }),
  listDirty: (limit?: number, offset?: number) =>
    invoke<Page<DirtyEntry>>("list_dirty", { limit, offset }),
  reingestDirtyEntry: (id: number) =>
    invoke<void>("reingest_dirty_entry", { id }),
  getSettings: () => invoke<SettingsView>("get_settings"),
  manualScan: () => invoke<number>("manual_scan"),
  getScanStatus: () => invoke<ScanStatus>("get_scan_status"),
  regenerateAuthToken: () => invoke<string>("regenerate_auth_token"),
  setHttpPort: (port: number) => invoke<void>("set_http_port", { port }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  forceExtract: (path: string) => invoke<void>("force_extract", { path }),
  getBackupConfig: () => invoke<BackupConfig>("get_backup_config"),
  setBackupConfig: (dir: string | null, retentionCount: number) =>
    invoke<void>("set_backup_config", { dir, retentionCount }),
  listBackups: () => invoke<BackupSnapshot[]>("list_backups"),
  backupNow: () => invoke<BackupResult>("backup_now"),
  stageRestore: (src: string) => invoke<void>("stage_restore", { src }),
  deleteBackup: (snapshot: string) => invoke<void>("delete_backup", { snapshot }),
}

