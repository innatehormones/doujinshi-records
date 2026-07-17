use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "doujinshi_file")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    pub filename: String,
    pub hash: String,
    pub ext: String,
    pub size_bytes: i64,
    pub circle: Option<String>,
    pub series: Option<String>,
    pub translator: Option<String>,
    /// 业务状态：`in_library / archived / recycle / deleted`，由用户决定
    pub status: String,
    /// 最后一次确认文件存在的路径；文件丢失时保留历史值
    pub last_seen_path: String,
    pub cover_path: Option<String>,
    pub marked_for_delete: bool,
    /// 文件状态：`present / missing / absent_confirmed`，由扫描 + 销毁操作维护
    pub file_state: String,
    pub viewed: bool,
    pub note: Option<String>,
    pub rating: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}