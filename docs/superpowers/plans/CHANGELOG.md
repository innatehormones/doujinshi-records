# 已完成 Plan 索引

V1.x（8 子计划 + umbrella）与 V2（7 子计划 + umbrella）全部完工并发布。
原 plan 文件是过程快照，无增量价值——每条改动的来龙去脉可从 git log
+ 现行 V4 spec 追溯。此处只留一份索引。

## V1.x（2026-07-10）

单一 umbrella 分解出 8 个 hardening 子计划，覆盖 CI / 错误恢复 / GUI 冒烟 /
HTTP 集成测试 / i18n 审计 / 大库性能 / plan 卫生 / schema 迁移。

| 候选 | 说明 | 现状 |
|---|---|---|
| v1x-ci | GitHub Actions CI（cargo check/clippy/test + vue-tsc） | 已落地 `.github/workflows/` |
| v1x-error-recovery | 启动 SQLite corruption 检测 + 备份重建 | 已落地 `db/recovery.rs` |
| v1x-gui-smoke | Tauri GUI 冒烟测试 | 已评估（Windows GUI subsystem 限制，手动验证替代） |
| v1x-http-integration-test | HTTP 端点集成测试 | 已落地 `src-tauri/tests/` |
| v1x-i18n-audit | 中文文案审计 | 已完成 |
| v1x-perf-large-library | 大库性能基准 | 已完成，结果见 `docs/superpowers/2026-07-15-large-library-perf.md` |
| v1x-plan-hygiene | plan 目录卫生 | 本次 CHANGELOG 收口即其延续 |
| v1x-schema-migrations | 版本化迁移框架 | 已落地 `db/migrations.rs`（现 CURRENT_VERSION=8） |

## V2（2026-07-11）

7 个功能子计划，全部已并入 V4 现行架构。

| 候选 | 说明 | 现状 |
|---|---|---|
| v2-1-conflict-view | 冲突处理页 | 已落地 `views/ConflictView.vue`，4 选 1 见 V4 spec |
| v2-2-settings-auth | 设置页 + Bearer token 鉴权 | 已落地 `views/SettingsView.vue` + `http/auth.rs` |
| v2-3-http-routes | HTTP 路由表 | 已落地 `http/api.rs`，现行路由见 CLAUDE.md |
| v2-4-ci-tauri-build | tauri build CI | 已落地 CI |
| v2-5-detail-view | 详情页 | 已落地 `views/DetailView.vue` |
| v2-6-search-ui | 搜索 UI | 已落地 Library 搜索 + `/api/doujinshi/search` |
| v2-7-rar-extract | RAR 解压支持 | 已落地 `services/rar_detect.rs` + size gate |
