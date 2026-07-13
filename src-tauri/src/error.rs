use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("db: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Other(String),
    #[error("anyhow: {0}")]
    Anyhow(#[from] anyhow::Error),
    /// 文件存在未解决的命名冲突，拒绝归档 / 移到回收站 / 彻底删除等
    /// 会改变文件位置或状态的操作。`count` 是该文件挂着的未解决冲突数。
    #[error("文件存在 {count} 个未解决的命名冲突，请先在「待识别」页面处理后再操作")]
    ConflictPending { count: usize },
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;