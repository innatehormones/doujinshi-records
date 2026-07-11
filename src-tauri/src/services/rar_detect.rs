//! Probe the system for a RAR-extracting executable.
//!
//! Two tools can extract RAR on Windows:
//! - **unrar** (ships with WinRAR): single-purpose, simple CLI
//! - **7z** (ships with 7-Zip): general archiver that also handles RAR
//!
//! We prefer unrar when present because its list/extract flags are
//! simpler; we fall back to 7z because 7-Zip is much more commonly
//! installed. Detection order:
//! 1. `unrar` / `unrar.exe` on PATH
//! 2. WinRAR default install locations (Program Files + x86)
//! 3. 7-Zip default install locations
//!
//! All paths are hard-coded Windows locations because the app only
//! ships on Windows per the project spec; this avoids dragging in
//! the `which` crate for a 20-line helper.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// 接受 Windows + Unix 两种命名（带 / 不带 .exe 后缀都试）。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_cmd_in_path() {
        // cmd.exe 一定在 Windows PATH 里 —— 用它来确认 which() 自身工作。
        let p = which("cmd");
        assert!(p.is_some(), "cmd should be in PATH on Windows");
    }

    #[test]
    fn which_returns_none_for_missing_command() {
        assert!(which("definitely-not-a-real-binary-xyz").is_none());
    }

    #[test]
    fn rar_tool_equality() {
        assert_eq!(RarTool::Unrar, RarTool::Unrar);
        assert_ne!(RarTool::Unrar, RarTool::SevenZip);
    }

    /// `detect()` 是否返回 Some 取决于运行环境。
    /// 测试机如果装了 unrar 或 7z 就返回 Some —— 这是预期行为。
    #[test]
    fn detect_runs_without_panic() {
        let _ = detect();
    }
}