use std::path::PathBuf;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub resources_dir: PathBuf,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let project_root = std::env::current_dir()?.parent()
            .ok_or_else(|| anyhow::anyhow!("cannot determine project root"))?
            .to_path_buf();
        let resources = project_root.join("resources");
        Ok(Self { resources_dir: resources })
    }

    pub fn inbox_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi") }
    pub fn identified_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi-identified") }
    pub fn will_delete_dir(&self) -> PathBuf { self.resources_dir.join("doujinshi-will-delete") }
    pub fn covers_dir(&self) -> PathBuf { self.resources_dir.join("covers") }
    pub fn db_path(&self) -> PathBuf { self.resources_dir.join("data.db") }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [self.inbox_dir(), self.identified_dir(),
                    self.will_delete_dir(), self.covers_dir()] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }
}

