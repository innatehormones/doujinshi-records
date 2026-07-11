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
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}