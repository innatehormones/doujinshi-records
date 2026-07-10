import { invoke } from "@tauri-apps/api/core"
import type { FileSummary, SettingsView, ConflictItem } from "@/types/api"

export const api = {
  listLibrary: (q?: string, status?: string) =>
    invoke<FileSummary[]>("list_library", { q, status }),
  markViewed: (id: number) => invoke<void>("mark_viewed", { id }),
  unmarkViewed: (id: number) => invoke<void>("unmark_viewed", { id }),
  markForDelete: (id: number) => invoke<void>("mark_for_delete", { id }),
  unmarkForDelete: (id: number) => invoke<void>("unmark_for_delete", { id }),
  moveToWillDelete: (id: number) => invoke<void>("move_to_will_delete", { id }),
  listRecycle: () =>
    invoke<[FileSummary[], FileSummary[]]>("list_recycle"),
  permanentDelete: (id: number) => invoke<void>("permanent_delete", { id }),
  restoreFromRecycle: (id: number) => invoke<void>("restore_from_recycle", { id }),
  listConflicts: () => invoke<ConflictItem[]>("list_conflicts"),
  resolveConflict: (id: number) => invoke<void>("resolve_conflict", { id }),
  getSettings: () => invoke<SettingsView>("get_settings"),
  manualScan: () => invoke<number>("manual_scan"),
}

