use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize)]
#[sea_orm(table_name = "dirty_data")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub file_path: String,
    pub file_size: i64,
    pub detected_dir: String,
    pub reason: String,
    pub first_seen_at: String,
    /// 软删除戳 ——「重新入库」成功后写入（RFC3339）。`list_dirty` 过滤掉非 null
    /// 行，scanner 也不会重写同 file_path 的活跃脏数据。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}