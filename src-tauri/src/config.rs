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
        let project_root = std::env::current_dir()?.parent()
            .ok_or_else(|| anyhow::anyhow!("cannot determine project root"))?
            .to_path_buf();
        let resources = project_root.join("resources");
        Ok(Self {
            resources_dir: resources,
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