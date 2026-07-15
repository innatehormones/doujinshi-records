# Spec — 数据备份与还原

> 日期：2026-07-15
> 状态：draft（待用户 review）
> 范围：**`data.db` 的本地备份**——只备份数据，不备份 .zip/.rar 文件本体
> 触发：用户希望自动 + 手动都能备份，并能从一个备份文件还原

## 背景

`resources/data.db` 装着用户最值钱的东西：

- 手工填的元数据（title / circle / series / translator / note / rating）
- 状态信息（status / file_state / viewed / marked_for_delete）
- 浏览/扫描历史（scan_event / dirty_data）
- 配置（auth_token / api_port / 自定义路径）

这一坨数据**只在本机存在**（不开云同步），一旦 DB 文件损坏或丢失——

- 所有手工填的元数据都没了（重建只能靠重扫压缩包，而圆周/系列/翻译这些用户编辑过的信息拿不回来）
- `dirty_data` 标记的孤儿文件历史也没了
- `auth_token` 重生会令浏览器扩展的旧 token 全部失效

现有的 `db/recovery.rs` 只在启动时检测 SQLite magic 头，发现损坏就 rename 成 `data.db.bak-<ts>` 后重建——这只是兜底，不算用户控制的备份。

压缩文件本体（`resources/doujinshi*/`）暂不纳入备份范围：体积大、可按 hash 重新入库、用户明确"只是数据，不是文件"。

## 目标

1. **自动备份**：app 启动时若距上次备份超 24 小时，自动备一份；运行期间不再追加定时器
2. **手动备份**：Settings 页一键立即备份
3. **BLAKE3 去重**：前后内容相同时不创建新文件，只更新时间戳
4. **保留最新 N 个**：超过保留数自动删旧（默认 10，可配）
5. **还原**：选备份文件 → 写"还原待执行"标记 → 用户关 app → 下次启动自动替换 data.db → 走正常 schema 迁移
6. **解耦 / 高效 / 安全 / 易读 / 易扩展**（用户在 2026-07-15 强调）

## 非目标

- 不备份 .zip/.rar 压缩文件本体（用户明确范围）
- 不做增量备份（DB 小，full snapshot 完全够）
- 不做远程备份（保留扩展点：用户把 `backup_dir` 指向 OneDrive/Dropbox 同步盘即等同云备份）
- 不暴露 HTTP API（备份/还原是破坏性操作，浏览器扩展不能触发）

## 备份机制选型

| 方案 | 评估 | 选用 |
|---|---|---|
| `VACUUM INTO '<path>'` | 单 SQL 原子复制；自带 compact；不依赖外部锁；空 DB 也工作 | ✅ 选用 |
| `fs::copy` + 短暂 BEGIN IMMEDIATE | 简单但要小心 journal 边界；DB 在写入时会拿到 .db-journal 临时文件 | ❌ |
| `rusqlite::backup` API | 在线备份语义最完整；要拉 rusqlite 0.31 做底层（SeaORM 上层 API 不暴露） | ❌ 过度工程 |

`VACUUM INTO` 是 SQLite 3.27+ 标准 SQL（项目用 SQLite 3.x via libsqlite3-sys），SeaORM 通过 `Database::execute(Statement::from_string(...))` 即可触发。

## 核心设计

### 存储后端 trait（解耦 + 易扩展）

```rust
pub trait BackupStorage: Send + Sync {
    /// 在指定路径写入一份 data.db 的拷贝（调用方保证原子性 + temp+rename）
    fn write_snapshot(&self, dst: &Path) -> Result<()>;
    /// 列已有快照：返回 (path, mtime, size)
    fn list_snapshots(&self, dir: &Path) -> Result<Vec<SnapshotInfo>>;
    /// 删除一个快照
    fn delete_snapshot(&self, path: &Path) -> Result<()>;
    /// 打开备份目录到 OS 文件管理器
    fn reveal_in_file_manager(&self, dir: &Path) -> Result<()>;
}
```

第一版只实现 `LocalFsStorage`，未来加 `S3Storage` / `WebDavStorage` 只需新加一个 struct + `impl BackupStorage`，commands 层不变。

### 配置 schema（app_setting 表）

| key | 类型 | 默认 | 说明 |
|---|---|---|---|
| `backup_dir` | string | `""` | 空字符串 = 用默认 `resources/backups/`；非空 = 绝对路径 |
| `backup_retention_count` | string(int) | `"10"` | 保留最新 N 个 |
| `backup_last_md5` | string | `""` | 上次成功备份的 BLAKE3 hex；dedup 用 |
| `backup_last_at` | string(RFC3339) | `""` | 上次备份时间；自动备份阈值判断 |

> 用 string 存数值是延续现有约定（`api_port` 也是 string）。避免 schema 变更。

### 文件名格式

`data-{RFC3339_compact}.db`

例：`data-2026-07-15T18-30-45Z.db`

RFC3339 紧凑版：日期 `T` 时间 `Z`（去冒号，文件名安全）；同一秒内多次备份由 BLAKE3 dedup 兜底——若 BLAKE3 与上次相同直接 skip。

### BLAKE3 计算位置

复用项目已有的 `services::hasher::BLAKE3`（不引新依赖）。算 data.db 的 BLAKE3：

```rust
fn hash_db_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let hash = blake3::hash(&bytes);
    Ok(hash.to_hex().to_string())
}
```

数据规模 KB~MB 级，整文件读入内存 + BLAKE3（已是项目最熟路径）足够快。

### 标记文件：`.restore-pending.json`

启动时由 `main.rs::recovery` 阶段读取：

```json
{
  "src": "C:/path/to/backup.db",
  "requested_at": "2026-07-15T18:30:45Z"
}
```

启动检测到标记：
1. 验证 `src` 仍存在 + magic bytes 是 SQLite
2. `fs::copy(src, data_path)`（保留源备份不动）
3. 删 `.restore-pending.json`
4. 走正常 `init_schema_versioned`（旧 schema 自动迁移到当前 v9）

### Tauri 命令（commands/backup.rs，4 条）

| 命令 | 入参 | 返回 | 用途 |
|---|---|---|---|
| `backup_now` | — | `BackupResult { path, skipped_reason?, md5, size_bytes }` | 立即触发一次 |
| `restore_from_backup` | `path: String` | `()` | 写 .restore-pending 标记；UI toast 提示用户关 app |
| `list_backups` | — | `Vec<BackupInfo { path, mtime, size_bytes, md5 }>` | 渲染设置页备份列表 |
| `set_backup_config` | `BackupConfig { dir, retention }` | `()` | 改 dir / retention_count |
| `get_backup_config` | — | `BackupConfig` | 渲染时读 |
| `open_backup_dir` | — | `()` | `explorer.exe` 打开目录 |

不暴露 HTTP（破坏性操作）。

### Service 模块（services/backup.rs）

```rust
pub struct BackupService {
    db_path: PathBuf,
    default_dir: PathBuf,
    storage: Arc<dyn BackupStorage>,
    settings: SettingsHandle,
    /// 同一时刻只允许一个 backup_now 跑（防止用户连点 + 自动备份并发）
    inflight: tokio::sync::Mutex<()>,
}

impl BackupService {
    pub fn new(...) -> Self;
    pub async fn backup_now(&self) -> Result<BackupResult>;
    pub async fn list_backups(&self) -> Result<Vec<BackupInfo>>;
    pub async fn set_config(&self, dir: Option<&str>, retention: u32) -> Result<()>;
    pub async fn get_config(&self) -> Result<BackupConfig>;
    pub fn stage_restore(&self, src: &Path) -> Result<()>;
    /// main.rs 启动时调用：检测 .restore-pending 标记 → 复制 → 删标记
    pub fn apply_pending_restore(&self) -> Result<RestoreOutcome>;
    /// 启动期调用：判断是否需要自动备份
    pub async fn should_auto_backup(&self, threshold: Duration) -> Result<bool>;
}
```

`SettingsHandle` 是一个轻 trait，把 `read_setting` / `write_setting` 包一层，方便 service 单测（mock settings 而非 mock 整个 DB）。

**并发控制**：`inflight: tokio::sync::Mutex<()>` 串行化所有 backup_now 调用——用户连点「立即备份」+ 自动备份并发时，只有一个能跑，其他直接返回"已在进行中"提示。

**原子写入**：VACUUM INTO 可能在中途失败（SQLite 文档明确说"destination file may be left in an incomplete state"）。所以实际写法是：
```rust
// 1. VACUUM INTO '<dir>/.tmp-<uuid>.db'
// 2. fs::rename(tmp, final)  // atomic on same fs
```
final 命名仍是 `data-{RFC3339}.db`；rename 在同一文件系统下是原子操作。

### 自动备份启动钩子

`lib.rs::run` 在 `db::migrations::init_schema_versioned` 之后、scanner 启动之前：

```rust
// 自动备份：距上次 > 24h 则补一份。失败只 log warn，不影响启动。
tokio::spawn(async move {
    let svc = BackupService::new(...);
    if svc.should_auto_backup(Duration::from_secs(24 * 3600)).await {
        if let Err(e) = svc.backup_now().await {
            tracing::warn!("auto backup failed: {:?}", e);
        }
    }
});
```

`should_auto_backup`：读 `backup_last_at`；无记录 / 距今超阈值 → true。

不引入定时器：app 关了定时器就没了；启动钩子足够。

## 还原流程（用户视角）

1. 用户在 Settings 「数据备份」卡片看到备份列表
2. 点某行「还原」按钮 → n-popconfirm 弹窗：「⚠ 这将覆盖当前所有数据。请关闭 app 后下次启动自动应用。」
3. 确认 → `restore_from_backup(path)` Tauri command
4. 后端：验 magic → 写 `.restore-pending.json` → 返回成功
5. UI toast：「已记录还原请求。请关闭 app，下次启动会从 `<path>` 还原。」
6. 用户手动关 app
7. 启动时 main.rs 检测到标记 → 复制备份到 data.db → 删标记 → 走 schema 迁移

## UI 改动

`SettingsView.vue` 新增 `<n-card title="数据备份">`：

```
┌─ 数据备份 ────────────────────────────────────────┐
│ 备份目录：resources/backups/      [修改] [打开]   │
│ 保留份数：[10]                                      │
│ [立即备份]   上次：2026-07-15 18:30:45（1 小时前） │
│                                                  │
│ ┌────────────────┬────────┬────────┬────────┐  │
│ │ 时间           │ 大小   │ MD5 前8│ 操作   │  │
│ ├────────────────┼────────┼────────┼────────┤  │
│ │ 07-15 18:30    │ 128 KB │ a1b2c3 │ 还原 删除 │ │
│ │ 07-14 18:30    │ 127 KB │ d4e5f6 │ 还原 删除 │ │
│ │ ...            │        │        │          │ │
│ └────────────────┴────────┴────────┴────────┘  │
└──────────────────────────────────────────────────┘
```

如果检测到 `.restore-pending.json`，卡片顶部红 alert：
> 检测到还原待执行：将从 `<path>` 还原。请关闭 app 后下次启动自动应用。

## 错误处理

| 场景 | 行为 |
|---|---|
| `backup_dir` 路径不存在 / 不可写 | `backup_now` 返回 Err；UI toast「无法写入备份目录」 |
| VACUUM INTO 失败 | 返回 Err；不更新 `backup_last_md5` / `backup_last_at`；下次启动仍会重试 |
| 还原源文件已被用户删 / magic 不匹配 | 拒收，返回 Err；标记文件不写 |
| 启动时检测到 `.restore-pending` 但 src 失效 | 删标记 + log warn「还原失败：<原因>，请手动处理」；正常启动 |
| 保留 N=0 | 视为禁用保留（不删旧）；仅创建新备份 |
| backup_dir 在云同步盘上 | VACUUM INTO 仍能工作（单文件 SQL 复制），只是会比本地慢；提示文案「备份位置在网盘/外置盘，IO 较慢」 |

## 质量要求落实

### 解耦

- `BackupStorage` trait 把存储后端抽出来；commands 只调 `BackupService`，不直接碰 `std::fs`
- `BackupService` 不依赖 Tauri / HTTP；commands/backup.rs 是唯一与 Tauri 耦合的层
- 设置读写走 `SettingsHandle` trait，单测可注入 fake

### 高效

- BLAKE3 dedup：内容相同直接 skip，不创建空文件
- 单 SQL `VACUUM INTO` 复制，无外部锁开销
- 保留清理只在新建备份后跑一次
- 启动期一次检查，无后台定时器

### 安全

- 还原是「COPY 不是 MOVE」——原备份保留
- 启动替换前再验一次 magic（用户可能中途动过文件）
- 写标记 + 关 app + 启动替换，三步序列确保 DB 连接全部释放后再替换
- 破坏性操作（restore、delete）走 n-popconfirm 二次确认
- 备份文件包含 `auth_token`——是用户选择（见"配置 schema"），UI 在备份目录编辑处说明

### 易读

- 单文件 `services/backup.rs` ~200 行；单文件 `commands/backup.rs` ~100 行
- 命名直白：`backup_now` / `list_backups` / `stage_restore` / `apply_pending_restore`
- 测试即文档：每条 test name 说明意图（`backup_now_writes_vacuum_into_file`）
- 关键决策在代码注释里讲 WHY，不讲 WHAT（CLAUDE.md 约定）

### 易扩展

- 新存储后端：`struct S3Storage; impl BackupStorage { ... }`，commands 层不变
- 新配置项：app_setting 加 key + service 加方法 + SettingsView 加 input；现有调用方零改动
- 未来「远程备份」：用户把 `backup_dir` 指向 rclone mount 即可，service 层零改动
- 未来「备份标签」：文件名后缀可加 `{auto,manual}` 区分，retention 策略可按标签细分

## 测试

| 测试 | 验证 |
|---|---|
| `filename_compact_rfc3339` | `2026-07-15T18:30:45Z` → `data-2026-07-15T18-30-45Z.db` |
| `hash_db_file_known_content` | BLAKE3("SQLite format 3\0...") == 期望 hex |
| `backup_now_writes_vacuum_into_file` | 临时 DB → backup_now → 检查 dir 下有新文件 + 内容是有效 SQLite |
| `backup_now_skips_when_md5_unchanged` | 跑两次 backup_now → 第二次返回 skipped + dir 下仍 1 个文件 |
| `backup_now_trims_to_retention_count` | retention=2，造 4 个备份 → 留下最新的 2 个 |
| `backup_now_serializes_concurrent_callers` | 同时 spawn 两个 backup_now → 一个跑、一个立刻返回 Err("already in progress") |
| `backup_now_atomic_tmp_then_rename` | VACUUM INTO 写入 `.tmp-uuid.db`，最后 rename 到 `data-*.db`；模拟中途失败应不留半截文件 |
| `restore_pending_marker_round_trip` | `stage_restore(path)` 写标记 → `apply_pending_restore` 读标记 → data.db == path 内容 |
| `apply_pending_restore_refuses_non_sqlite` | 标记指向非 SQLite 文件 → 不替换 + 删标记 + 错误日志 |
| `apply_pending_restore_refuses_missing_src` | 标记指向已删文件 → 删标记 + warn |
| `storage_local_fs_list_filters_only_backup_files` | dir 下混着 `data-*.db` 和 `foo.txt` → 只返回前者 |

## 不做

- 压缩备份（gzip / zstd）—— DB 太小，gzip 后 100KB → 30KB 对个人库毫无意义；解压还多一步
- 加密备份 —— 个人本地应用，磁盘已加密（BitLocker / FileVault）够用；想做可以未来加 `BackupStorage` 实现
- 备份到云 —— 用户没要求，扩展点已留
- 备份到 USB 自动检测 —— 复杂且不可靠
- 备份完整性校验（SHA-256 比对）—— BLAKE3 已经在 dedup 路径上算过一遍，不需要额外成本
- 多 DB 并发备份 —— 单 DB 单 app，没场景

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| VACUUM INTO 期间 app 卡顿 | 已知；对 ~MB 级 DB 通常 < 100ms；future 可放后台 tokio task + UI spinner |
| 用户误操作还原 | n-popconfirm 二次确认 + 启动替换前再次校验 |
| 备份目录被外部进程持续写入（网盘同步） | 提示文案提醒；同步盘通常对 100KB 量级无问题 |
| 备份文件本身损坏 | 启动替换前 magic 校验；用户可手动选其他备份重试 |
| 跨设备 rename 备份到外置盘失败 | 用 `fs::copy` 而不是 `fs::rename`；失败返回 Err 不留半截 |

## 后续可能性（非本次范围）

- 「数据导出 JSON」：dump 所有 doujinshi_file + alias 为 JSON，方便分享 / 编辑 / 再导入
- 「跨设备迁移」：备份 + 在另一台机器还原 = 数据搬家；当前已可做到
- 「按状态分卷备份」：分 `in_library` / `recycle` / `archived` 多文件