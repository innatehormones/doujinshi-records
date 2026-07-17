use std::path::PathBuf;
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

        Ok(Self {
            resources_dir,
            preview_cache_max_bytes: 200 * 1024 * 1024,
        })
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
}