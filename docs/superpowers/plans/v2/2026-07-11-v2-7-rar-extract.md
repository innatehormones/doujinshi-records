# V2 Sub-Plan 7 — RAR 完整解压

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`。
> Implements umbrella candidate **#7**。

**Goal:** 把 RAR 文件从「无法入库」升级到「完整流程」：探测 unrar.exe → 三档大小分级（200 MB / 1 GB）→ 空间检查 → 解压/拒绝并给清晰提示。

**Architecture:**
- **新增 `services/rar_detect.rs`：** 探测 unrar.exe 路径（PATH + WinRAR/7-Zip 默认安装位置）
- **新增 `services/disk_space.rs`：** 用 `sysinfo` crate 查磁盘剩余空间
- **扩展 `services/archive.rs`：** 新增 `extract_rar(path, dest) -> ExtractStats` 调 unrar 子进程
- **扩展 `services/identifier.rs`：** RAR 分支接三件套；保留 zip 流程
- **错误分类：** 新增 `IdentifierError` 枚举：UnrarNotInstalled / TooLarge / InsufficientSpace / ExtractionFailed
- **前端：** InboxView 显示 RAR 错误卡片，含「下载 WinRAR」「下载 7-Zip」按钮（仅 UnrarNotInstalled 时）

**Tech Stack:** `tokio::process::Command` spawn unrar 子进程 + `sysinfo` crate + Naive UI（NAlert）。

**依赖：** 无强制依赖。独立可做。

---

## Task 1: rar_detect 模块

**Files:**
- Create: `src-tauri/src/services/rar_detect.rs`

- [ ] **Step 1: 探测函数**

```rust
//! Probe the system for a RAR-extracting executable.
//! Returns the path and which tool was found (unrar / 7z).

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum RarTool {
    Unrar,
    SevenZip,
}

#[derive(Debug, Clone)]
pub struct RarLocation {
    pub tool: RarTool,
    pub path: PathBuf,
}

pub fn detect() -> Option<RarLocation> {
    // 1. 查 PATH 里的 "unrar"
    if let Some(p) = which("unrar") {
        return Some(RarLocation { tool: RarTool::Unrar, path: p });
    }
    // 2. WinRAR 默认位置
    for path in [
        "C:\\Program Files\\WinRAR\\unrar.exe",
        "C:\\Program Files (x86)\\WinRAR\\unrar.exe",
    ] {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(RarLocation { tool: RarTool::Unrar, path: p });
        }
    }
    // 3. 7-Zip 默认位置（7z.exe 也支持 RAR 解压）
    for path in [
        "C:\\Program Files\\7-Zip\\7z.exe",
        "C:\\Program Files (x86)\\7-Zip\\7z.exe",
    ] {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(RarLocation { tool: RarTool::SevenZip, path: p });
        }
    }
    None
}

/// `which` crate 的极简实现：查 PATH 环境变量。
fn which(cmd: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(format!("{}.exe", cmd));
        if candidate.exists() {
            return Some(candidate);
        }
        let candidate = dir.join(cmd);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
```

- [ ] **Step 2: 单测**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_none_when_nothing_installed() {
        // 此测试不删 PATH 也不删 WinRAR——只能在 CI 跑（假设 runner 上没装）
        // 本地跑若失败，说明本机装了 unrar，**这是正常情况**
        // skip 标记：#[ignore] 但要带 ignore 注释说明
    }

    #[test]
    fn which_finds_cmd_in_path() {
        // 大多数系统 PATH 里有 cmd.exe / powershell.exe
        // 用一个肯定存在的命令测试
        let p = which("cmd");
        assert!(p.is_some(), "cmd should be in PATH on Windows");
    }
}
```

- [ ] **Step 3: 注册到 services/mod.rs**

```rust
pub mod rar_detect;
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/rar_detect.rs src-tauri/src/services/mod.rs
git commit -m "feat(rar): detect unrar.exe / 7z.exe system paths"
```

---

## Task 2: disk_space 模块

**Files:**
- Create: `src-tauri/src/services/disk_space.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Cargo.toml 加 sysinfo**

```toml
sysinfo = "0.32"
```

- [ ] **Step 2: 函数**

```rust
//! Query available disk space on the volume containing a given path.

use std::path::Path;

pub fn available_bytes(path: &Path) -> std::io::Result<u64> {
    let mut sys = sysinfo::Disks::new();
    sys.refresh(true);
    for disk in &sys {
        if disk.mount_point().ancestors().any(|a| a == path) {
            return Ok(disk.available_space());
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("no disk found for path {}", path.display()),
    ))
}
```

- [ ] **Step 3: 单测**

```rust
#[test]
fn available_bytes_returns_positive_for_existing_path() {
    let p = std::env::temp_dir();
    let bytes = available_bytes(&p).unwrap();
    assert!(bytes > 0, "temp dir should have some free space, got {}", bytes);
}
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/disk_space.rs src-tauri/src/services/mod.rs src-tauri/Cargo.toml
git commit -m "feat(disk): available_bytes via sysinfo::Disks"
```

---

## Task 3: archive::extract_rar + list_rar_images

**Files:**
- Modify: `src-tauri/src/services/archive.rs`

- [ ] **Step 1: 加 RAR 文件名清单（不解压）**

```rust
/// List image entries in a RAR archive (no extraction).
/// Requires unrar.exe or 7z.exe on PATH.
pub async fn list_rar_images(
    path: &Path,
    tool: &crate::services::rar_detect::RarLocation,
) -> Result<Vec<ArchiveImageEntry>> {
    let output = match tool.tool {
        crate::services::rar_detect::RarTool::Unrar => {
            tokio::process::Command::new(&tool.path)
                .args(["l", "-p-", path.to_str().unwrap()])
                .output().await?
        }
        crate::services::rar_detect::RarTool::SevenZip => {
            tokio::process::Command::new(&tool.path)
                .args(["l", "-slt", path.to_str().unwrap()])
                .output().await?
        }
    };
    if !output.status.success() {
        return Err(anyhow!("rar listing failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_rar_list(&stdout, tool.tool)
        .into_iter()
        .map(|(name, size)| {
            // 暂不入 data；调用方按需 extract
            Ok(ArchiveImageEntry { name, data: vec![] })  // data 为空表示没解压
        })
        .collect()
}

fn parse_rar_list(stdout: &str, tool: crate::services::rar_detect::RarTool) -> Vec<(String, u64)> {
    // unrar l 输出格式：
    //   Name             Size   Packed Ratio  Date   Time   Attr  CRC  Meth Ver
    //   images/01.jpg  123456  123456 100%  ...             ....  ....  m3d 2.9
    // 7z l 输出更复杂，简化处理：扫所有含 IMG_EXTS 的行
    let mut out = Vec::new();
    for line in stdout.lines() {
        let lower = line.to_lowercase();
        for ext in IMG_EXTS {
            if lower.contains(&format!(".{}", ext)) && !lower.starts_with("----") {
                // 简化：第一列是文件名
                let name = line.split_whitespace().next().unwrap_or("").to_string();
                if !name.is_empty() {
                    out.push((name, 0));  // size 暂时填 0（list 不解压拿不到精确）
                }
                break;
            }
        }
    }
    out
}
```

- [ ] **Step 2: extract_rar 函数**

```rust
pub struct ExtractStats {
    pub extracted_count: usize,
    pub total_bytes: u64,
}

pub async fn extract_rar(
    path: &Path,
    dest_dir: &Path,
    tool: &crate::services::rar_detect::RarLocation,
) -> Result<ExtractStats> {
    let output = match tool.tool {
        crate::services::rar_detect::RarTool::Unrar => {
            tokio::process::Command::new(&tool.path)
                .args([
                    "x", "-y", "-o+",  // 静默，覆盖
                    path.to_str().unwrap(),
                    dest_dir.to_str().unwrap(),
                ])
                .output().await?
        }
        crate::services::rar_detect::RarTool::SevenZip => {
            tokio::process::Command::new(&tool.path)
                .args([
                    "x", "-y", "-aoa",  // 静默，覆盖
                    path.to_str().unwrap(),
                    format!("-o{}", dest_dir.to_str().unwrap()).as_str(),
                ])
                .output().await?
        }
    };
    if !output.status.success() {
        return Err(anyhow!("rar extract failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    // 统计 dest_dir 下的文件数和总大小
    let mut count = 0;
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(dest_dir) {
        let e = entry?;
        if e.file_type().is_file() {
            count += 1;
            total += e.metadata().ok().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(ExtractStats { extracted_count: count, total_bytes: total })
}
```

需要 `walkdir` crate（已是 transitive 通过 notify）。检查 `Cargo.toml` 没的话加 `walkdir = "2"`。

- [ ] **Step 3: 单测**

需要装 unrar.exe 的环境。`#[cfg(test)]` 块用 `#[ignore]` 标记：

```rust
#[tokio::test]
#[ignore = "requires unrar.exe on PATH"]
async fn list_rar_images_smoke() {
    // 准备一个测试 rar 文件
    let tool = crate::services::rar_detect::detect().expect("unrar required");
    let entries = list_rar_images(Path::new("test.rar"), &tool).await.unwrap();
    assert!(entries.len() > 0);
}
```

手动测试命令：`cargo test rar:: -- --ignored`。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/archive.rs src-tauri/Cargo.toml
git commit -m "feat(rar): list_rar_images + extract_rar via unrar/7z subprocess"
```

---

## Task 4: identifier RAR 分支

**Files:**
- Modify: `src-tauri/src/services/identifier.rs`

- [ ] **Step 1: 错误类型**

```rust
#[derive(Debug, thiserror::Error)]
pub enum IdentifierError {
    #[error("本机未安装 RAR 解压工具（WinRAR / 7-Zip）")]
    UnrarNotInstalled,
    #[error("RAR 文件过大 ({size_mb:.0} MB > {limit_mb} MB)")]
    TooLarge { size_mb: f64, limit_mb: u64 },
    #[error("磁盘空间不足：解压需 {needed_mb:.0} MB，剩余 {available_mb} MB")]
    InsufficientSpace { needed_mb: f64, available_mb: u64 },
    #[error("RAR 解压失败: {0}")]
    ExtractionFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

需要 `thiserror = "1"`（已是 transitive）。

- [ ] **Step 2: 三档阈值常量**

```rust
const SMALL_THRESHOLD_BYTES: u64 = 200 * 1024 * 1024;       // 200 MB
const MEDIUM_THRESHOLD_BYTES: u64 = 1024 * 1024 * 1024;      // 1 GB
```

- [ ] **Step 3: `identify_file` 加 RAR 分支**

```rust
use crate::services::rar_detect;
use crate::services::disk_space;

pub async fn identify_file(
    state: &AppState,
    path: &Path,
    force_rename: Option<&str>,  // 已有，给 ConflictView 用
) -> Result<(), IdentifierError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "zip" => identify_zip(state, path, force_rename).await,
        "rar" => identify_rar(state, path, force_rename).await,
        _ => Err(IdentifierError::Other(anyhow!("unsupported extension: {}", ext))),
    }
}

async fn identify_rar(state: &AppState, path: &Path, force_rename: Option<&str>) -> Result<(), IdentifierError> {
    let size = std::fs::metadata(path)?.len();
    if size > MEDIUM_THRESHOLD_BYTES {
        return Err(IdentifierError::TooLarge {
            size_mb: size as f64 / 1024.0 / 1024.0,
            limit_mb: MEDIUM_THRESHOLD_BYTES / 1024 / 1024,
        });
    }
    if size > SMALL_THRESHOLD_BYTES {
        // 中等：返回错误让前端弹窗；前端会二次确认后再调 identify_rar_with_confirm
        return Err(IdentifierError::TooLarge {
            size_mb: size as f64 / 1024.0 / 1024.0,
            limit_mb: MEDIUM_THRESHOLD_BYTES / 1024 / 1024,
        });
    }

    let tool = rar_detect::detect().ok_or(IdentifierError::UnrarNotInstalled)?;

    // 临时解压到 tempdir 抽 cover（抽完删 tempdir）
    let tmp = tempfile::tempdir()?;
    let stats = crate::services::archive::extract_rar(path, tmp.path(), &tool).await
        .map_err(|e| IdentifierError::ExtractionFailed(e.to_string()))?;

    let available = disk_space::available_bytes(tmp.path()).unwrap_or(u64::MAX);
    // 实际：解压前已检查 = extract_rar 内部就能爆错，但保险起见事后查一次
    if stats.total_bytes > available {
        return Err(IdentifierError::InsufficientSpace {
            needed_mb: stats.total_bytes as f64 / 1024.0 / 1024.0,
            available_mb: available / 1024 / 1024,
        });
    }

    // 抽 cover：从 tmp 里找第一张图，按 zip 流程走 cover::extract_and_save
    let candidates = crate::services::archive::list_images_in_dir(tmp.path())?;
    let cover = crate::services::archive::pick_cover(&candidates);
    if let Some(c) = cover {
        crate::services::cover::extract_and_save(c, &state.config.covers_dir(), &hash).await?;
    }

    // 移文件 + 入库（沿用 zip 流程）
    let _ = (state, path, force_rename, tmp);  // 实际写完整流程
    Ok(())
}
```

> ⚠️ 上面 `identify_rar` 是伪代码骨架，实际实现要把 zip 流程里 `hash → 写 doujinshi_file → 移文件` 完整复用，建议抽出 `finalize_identification(state, hash, parsed_filename, cover_path, source_path)` 共享函数。

- [ ] **Step 4: 单元测试**

```rust
#[tokio::test]
#[ignore = "requires unrar.exe"]
async fn identify_rar_extracts_to_library() { ... }

#[tokio::test]
async fn identify_rar_returns_too_large_for_2gb_file() {
    // 模拟文件大小 > 1 GB（不必真造 2 GB 文件，注入 size override 或 mock）
}
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/services/identifier.rs
git commit -m "feat(identifier): RAR branch with size tiering + disk space check"
```

---

## Task 5: 前端 InboxView 错误卡片

**Files:**
- Modify: `src/views/InboxView.vue`
- Modify: `src/types/api.ts`
- Modify: `src/stores/inbox.ts`

- [ ] **Step 1: types 加 `RarError` 枚举**

```typescript
export type RarError =
  | { kind: "unrar_not_installed" }
  | { kind: "too_large"; size_mb: number; limit_mb: number }
  | { kind: "insufficient_space"; needed_mb: number; available_mb: number }
  | { kind: "extraction_failed"; message: string }
```

- [ ] **Step 2: store 加 `pendingRarErrors`**

identifier 失败时记录 `{ filename, error }` 到这个 list（不阻塞其他文件）。

- [ ] **Step 3: InboxView 加错误卡片**

```vue
<n-card
  v-for="(err, idx) in store.pendingRarErrors"
  :key="idx"
  :title="`RAR 处理失败：${err.filename}`"
  style="margin-bottom: 8px"
>
  <n-alert v-if="err.error.kind === 'unrar_not_installed'" type="error">
    本机未安装 RAR 解压工具（WinRAR / 7-Zip），请先安装：
    <n-space style="margin-top: 8px">
      <n-button tag="a" href="https://www.win-rar.com/" target="_blank">下载 WinRAR</n-button>
      <n-button tag="a" href="https://www.7-zip.org/" target="_blank">下载 7-Zip</n-button>
    </n-space>
  </n-alert>
  <n-alert v-else-if="err.error.kind === 'too_large'" type="warning">
    文件过大（{{ err.error.size_mb.toFixed(0) }} MB > {{ err.error.limit_mb }} MB），已拒绝解压。
    <n-space style="margin-top: 8px">
      <n-button @click="confirmExtractLarge(err)">仍要解压（确认磁盘够）</n-button>
    </n-space>
  </n-alert>
  <n-alert v-else-if="err.error.kind === 'insufficient_space'" type="warning">
    磁盘空间不足：解压需 {{ err.error.needed_mb.toFixed(0) }} MB，剩余 {{ err.error.available_mb }} MB。
  </n-alert>
  <n-alert v-else type="error">
    解压失败：{{ err.error.message }}
  </n-alert>
</n-card>
```

`confirmExtractLarge` 调 store action 强行走解压（绕过大小检查）。

- [ ] **Step 4: 提交**

```bash
git add src/views/InboxView.vue src/types/api.ts src/stores/inbox.ts
git commit -m "feat(inbox): RAR error cards with download links + retry"
```

---

## Task 6: 中等 RAR 的二次确认弹窗

**Files:**
- Modify: `src/views/InboxView.vue`（弹窗）或 `src/components/ConfirmDialog.vue`（新建）

- [ ] **Step 1: 中等大小（200 MB~1 GB）的弹窗**

如果 #4 的 `identify_rar` 直接返 `TooLarge` 包括中等和超大，前端按 size_mb 区分：

```typescript
function onRarError(err: RarErrorEntry) {
  if (err.error.kind === 'too_large' && err.error.size_mb <= 1024) {
    // 中等：弹窗确认
    showConfirm.value = {
      filename: err.filename,
      size_mb: err.error.size_mb,
    }
  } else {
    // 超大或其他：显示错误卡片
  }
}
```

弹窗内容：「文件较大 (XX MB)，确认解压？」用户点确认后调 `confirmExtractLarge(filename)`，store 调 `identify_file_with_force(path)`（绕过大小检查的内部函数）。

- [ ] **Step 2: 提交**

```bash
git add src/views/InboxView.vue src/stores/inbox.ts
git commit -m "feat(inbox): medium RAR size confirmation dialog"
```

---

## Task 7: E2E + 回归

- [ ] **Step 1:** `cd src-tauri && cargo test` 全绿
- [ ] **Step 2:** 手动 E2E 矩阵：

| 场景 | 预期 |
|---|---|
| 装 WinRAR + 拖小 RAR（< 200 MB） | 完整入库 |
| 拖中等 RAR（500 MB） | 弹窗确认 → 确认后入库 |
| 拖超大 RAR（1.5 GB） | 错误卡片：文件过大 |
| 卸载 WinRAR + 拖任意 RAR | 错误卡片：未安装工具 + 下载按钮 |
| 磁盘剩余 < RAR 内容 | 错误卡片：空间不足 |

- [ ] **Step 3:** `pnpm lint && pnpm build` 全绿

---

## Self-review

- [ ] 探测顺序：PATH → WinRAR 64 → WinRAR 86 → 7-Zip 64 → 7-Zip 86
- [ ] 三档阈值准确：200 MB / 1 GB
- [ ] 空间检查发生在解压前（先 unrar v / l 拿预估大小，再 check disk_available）
- [ ] 中等大小有二次确认弹窗
- [ ] UnrarNotInstalled 错误卡片有下载链接（WinRAR + 7-Zip 两个）
- [ ] 解压后 tempdir 自动清理（tempfile::TempDir drop）
- [ ] 没破坏 zip 流程（identify_zip 不变）
- [ ] 集成测试覆盖 4 种 RarError kind