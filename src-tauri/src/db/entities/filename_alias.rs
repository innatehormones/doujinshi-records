use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "filename_alias")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub file_id: i64,
    pub alias_filename: String,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

