use std::path::{Path, PathBuf};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub resources_dir: PathBuf,
    /// LRU preview cache 容量上限（字节）。200 MiB 默认。
    pub preview_cache_max_bytes: u64,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let resources_dir = if cfg!(debug_assertions) {
            // dev 模式：`cargo run` 跑在 src-tauri/ 下，但 resources 在项目根。
            // 用 CARGO_MANIFEST_DIR（编译期常量，不依赖 CWD）→ 项目根/resources，
            // 跟 dev 模式一直以来的位置一致（pnpm tauri dev 期望 resources
            // 跟 src-tauri/ 同级，方便开发者直接拖文件到 resources/doujinshi/）。
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .ok_or_else(|| anyhow::anyhow!("cannot determine project root"))?
                .join("resources")
        } else {
            // release 模式：resources 跟 exe 同级（`<exe_dir>/resources/`）。
            // 这样 setup 安装到 `D:\Temp\Test\` → 创建 `doujinshi-records/`
            // 子目录时，resources 也会落在该子目录里（与 exe 同级），
            // 跟 dev 模式「resources 跟 src-tauri/ 同级」结构对称。
            let exe = std::env::current_exe()?;
            exe.parent()
                .ok_or_else(|| anyhow::anyhow!("cannot determine exe directory"))?
                .join("resources")
        };

        // V0.2.0 之前的 release 把 resources 放在 exe 目录的 parent
        // （`D:\Temp\Test\resources\` 而不是 `D:\Temp\Test\doujinshi-records\resources\`）。
        // 一次性迁移：检测老位置有数据 + 新位置空 → fs::rename 到新位置。
        // 已存在新位置（用户手动搬过 / 多 exe 复用）→ 跳过。
        if !cfg!(debug_assertions) {
            Self::migrate_legacy_resources(&resources_dir);
        }

        Ok(Self {
            resources_dir,
            preview_cache_max_bytes: 200 * 1024 * 1024,
        })
    }

    /// 检测 `<exe_dir>/../resources/`（V0.2.0 之前的位置）是否还有内容；
    /// 有且 `<exe_dir>/resources/` 不存在 → 一次性 rename 到新位置。
    /// 跨设备（Windows 上 `ERROR_NOT_SAME_DEVICE=17`）自动 copy + remove 兜底。
    fn migrate_legacy_resources(new_path: &Path) {
        let Some(new_parent) = new_path.parent() else { return };
        let Some(legacy_parent) = new_parent.parent() else { return };
        let legacy = legacy_parent.join("resources");

        if !legacy.exists() || new_path.exists() {
            return;
        }

        // 跳过空目录（用户可能只是手贱建了个 resources/ 空壳）
        let has_content = match std::fs::read_dir(&legacy) {
            Ok(mut d) => d.next().is_some(),
            Err(_) => false,
        };
        if !has_content {
            return;
        }

        match std::fs::rename(&legacy, new_path) {
            Ok(()) => println!(
                "INFO: migrated resources {} → {}",
                legacy.display(),
                new_path.display()
            ),
            Err(e) if e.raw_os_error() == Some(17) || e.kind() == std::io::ErrorKind::CrossesDevices => {
                // 跨设备 fallback：copy + remove。少见但要兜底。
                if let Err(copy_err) = copy_dir_recursive(&legacy, new_path) {
                    eprintln!(
                        "WARN: failed to migrate resources (cross-device): {}",
                        copy_err
                    );
                    return;
                }
                if let Err(rm_err) = std::fs::remove_dir_all(&legacy) {
                    eprintln!(
                        "WARN: migrated but failed to remove legacy {}: {}",
                        legacy.display(),
                        rm_err
                    );
                } else {
                    println!(
                        "INFO: migrated resources (cross-device) {} → {}",
                        legacy.display(),
                        new_path.display()
                    );
                }
            }
            Err(e) => eprintln!(
                "WARN: failed to migrate resources {} → {}: {}",
                legacy.display(),
                new_path.display(),
                e
            ),
        }
    }

    pub fn inbox_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi") }
    pub fn identified_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi-identified") }
    pub fn will_delete_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi-will-delete") }
    pub fn archived_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi-archived") }
    pub fn preview_cache_dir(&self) -> PathBuf { self.resources_dir.join("_preview_cache") }
    pub fn covers_dir(&self) -> PathBuf { self.resources_dir.join("covers") }
    pub fn backups_dir(&self) -> PathBuf { self.resources_dir.join("backups") }
    pub fn db_path(&self) -> PathBuf { self.resources_dir.join("data.db") }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            self.inbox_dir(),
            self.identified_dir(),
            self.will_delete_dir(),
            self.archived_dir(),
            self.preview_cache_dir(),
            self.covers_dir(),
            self.backups_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archived_dir_lives_under_resources() {
        let cfg = AppConfig {
            resources_dir: std::path::PathBuf::from("r"),
            preview_cache_max_bytes: 0,
        };
        assert_eq!(
            cfg.archived_dir(),
            std::path::PathBuf::from("r/doujinshi-archived")
        );
    }

    #[test]
    fn preview_cache_dir_lives_under_resources() {
        let cfg = AppConfig {
            resources_dir: std::path::PathBuf::from("r"),
            preview_cache_max_bytes: 0,
        };
        assert_eq!(
            cfg.preview_cache_dir(),
            std::path::PathBuf::from("r/_preview_cache")
        );
    }

    #[test]
    fn ensure_dirs_creates_archived_and_preview_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig {
            resources_dir: dir.path().to_path_buf(),
            preview_cache_max_bytes: 0,
        };
        cfg.ensure_dirs().unwrap();
        assert!(dir.path().join("doujinshi-archived").exists());
        assert!(dir.path().join("_preview_cache").exists());
    }

    #[test]
    fn preview_cache_max_bytes_defaults_to_200mib() {
        let cfg = AppConfig {
            resources_dir: std::path::PathBuf::from("r"),
            preview_cache_max_bytes: 200 * 1024 * 1024,
        };
        assert_eq!(cfg.preview_cache_max_bytes, 209_715_200);
    }

    #[test]
    fn ensure_dirs_creates_preview_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig {
            resources_dir: dir.path().to_path_buf(),
            preview_cache_max_bytes: 0,
        };
        cfg.ensure_dirs().unwrap();
        assert!(dir.path().join("_preview_cache").exists());
    }

    /// V0.2.0 之前 release 把 resources 放在 exe_dir/../resources/；
    /// 修后应该是 exe_dir/resources/。如果老位置还有数据 + 新位置不存在，
    /// 应自动迁移过去。
    #[test]
    fn migrate_legacy_resources_moves_non_empty_legacy_to_new() {
        let root = tempfile::tempdir().unwrap();
        // 模拟目录结构：root/exe_dir/ 是 exe 所在目录，root/resources/ 是 legacy
        let exe_dir = root.path().join("doujinshi-records");
        std::fs::create_dir(&exe_dir).unwrap();
        let legacy = root.path().join("resources");
        std::fs::create_dir(&legacy).unwrap();
        // legacy 有数据（data.db 模拟）
        std::fs::write(legacy.join("data.db"), b"fake-db-bytes").unwrap();

        let new_path = exe_dir.join("resources");
        AppConfig::migrate_legacy_resources(&new_path);

        assert!(!legacy.exists(), "legacy 应被移走");
        assert!(new_path.exists(), "新位置应存在");
        assert_eq!(
            std::fs::read(new_path.join("data.db")).unwrap(),
            b"fake-db-bytes"
        );
    }

    /// legacy 是空目录（用户可能只是手贱建了空壳）→ 不迁移。
    #[test]
    fn migrate_legacy_resources_skips_empty_legacy() {
        let root = tempfile::tempdir().unwrap();
        let exe_dir = root.path().join("doujinshi-records");
        std::fs::create_dir(&exe_dir).unwrap();
        let legacy = root.path().join("resources");
        std::fs::create_dir(&legacy).unwrap();
        // 空 legacy

        let new_path = exe_dir.join("resources");
        AppConfig::migrate_legacy_resources(&new_path);

        assert!(legacy.exists(), "空 legacy 不应被移走");
        assert!(!new_path.exists());
    }

    /// 新位置已有数据（用户手动搬过 / 多 exe 复用）→ 不覆盖。
    #[test]
    fn migrate_legacy_resources_skips_when_new_already_exists() {
        let root = tempfile::tempdir().unwrap();
        let exe_dir = root.path().join("doujinshi-records");
        std::fs::create_dir(&exe_dir).unwrap();
        let legacy = root.path().join("resources");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("data.db"), b"legacy-bytes").unwrap();

        let new_path = exe_dir.join("resources");
        std::fs::create_dir(&new_path).unwrap();
        std::fs::write(new_path.join("data.db"), b"new-bytes").unwrap();

        AppConfig::migrate_legacy_resources(&new_path);

        // legacy 不动，新位置内容不变
        assert!(legacy.exists());
        assert_eq!(
            std::fs::read(new_path.join("data.db")).unwrap(),
            b"new-bytes"
        );
    }

    /// legacy 不存在（首次安装 / dev 模式）→ 静默跳过。
    #[test]
    fn migrate_legacy_resources_noop_when_legacy_missing() {
        let root = tempfile::tempdir().unwrap();
        let exe_dir = root.path().join("doujinshi-records");
        std::fs::create_dir(&exe_dir).unwrap();
        let new_path = exe_dir.join("resources");

        AppConfig::migrate_legacy_resources(&new_path);

        assert!(!new_path.exists());
    }
}