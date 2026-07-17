# Changelog

本项目的版本变更记录。详细 plan / spec 见 `docs/superpowers/`。

## [0.2.0] - 2026-07-17

V0.2.0 是 V1 上线后的第一个清理 / 优化版本。11 条 commit 覆盖：精简文档、移除死代码、代码解耦、代码优化、移除冗余。**无功能变更**——纯内部重构与文档整理。详见 [`docs/superpowers/plans/2026-07-17-v020-release.md`](docs/superpowers/plans/2026-07-17-v020-release.md)。

### Docs（精简文档）

- 删除 V1 / V3 / V3.1 历史 spec 与 plan（已被 V4 取代）
- archive V3 排查与中文分析文档（仅供追溯）
- 收口 README / CLAUDE.md 关键文档索引（修 typo 与死链）
- V1.x / V2 已完成 plan 收口到 `docs/superpowers/plans/CHANGELOG.md`
- 新增 `docs/superpowers/plans/2026-07-17-v020-release.md` 作为本次 release 的实施 plan

### Dead code（移除死代码）

**后端：**
- 删除 `commands/library.rs::reparse_metadata` Tauri command（HTTP handler 保留）
- 删除 `commands/recycle.rs::restore_from_recycle`（与 `restore` 重复）
- 删除 `services/state_machine.rs` 4 个 V3 死 helper（`transition` / `non_deleted_statuses` / `is_deleted_status` / `expected_dir_for_status`）
- DROP `doujinshi_file.has_physical_file` 列（v11 migration）
- DROP `filename_alias` 表（v12 migration，删除 entity + 移除 `identifier::store_alias` 函数）

**前端：**
- 删除 `SearchResult` 类型（V2/V3 旧 shape，已无引用）
- 删除 `reparseMetadata` wrapper + `ReparseResult` 导入

### Decoupling（代码解耦）

- 把 V3 时代的 `identifier::store_alias` 同 hash 别名记录从「写 alias 表」改为「刷 `doujinshi_file.filename` + 删 inbox 副本」——alias 表本身被 v12 一并删掉
- `AppError::NotFound` 收口 5 处 `AppError::Other(format!("... not found"))` 模式 + `http/api.rs` patch_metadata 改用精确 match（不再字符串嗅探）

### Optimization（代码优化）

- 抽出 `src/lib/format.ts::formatBytes`，替换 4 个 view + 1 个 component 的本地 `formatSize` / `fmtSize`
- 抽出 `src/lib/file-state.ts::statusTagType` / `fileStateTagType`，替换 3 处本地副本
- `identifier::finalize_identification` self-rename 分支移除（V4 状态机搬运路径不再触达该分支）

### Redundancy（移除冗余）

- 删 `restore_from_recycle` 入口（Tauri 侧），统一走 `restore`
- 删 `doujinshi_file.has_physical_file` 写入路径 4 处（dirty_scanner × 3、inbox × 1、recycle × 1）
- 删 `filename_alias` 表写入路径 3 处（identifier.rs）

### Migration 升级说明

`schema_version` 从 v10 升到 v12。两步都是幂等的：

- v11：`ALTER TABLE doujinshi_file DROP COLUMN has_physical_file`
- v12：`DROP TABLE IF EXISTS filename_alias`

无任何数据迁移——两列 / 一表自 V4 起就是冗余。升级前若用户库有 `has_physical_file=0` 的行，对应 `file_state` 在 v8 已正确填为 `missing`，语义保留。