use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "conflict")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub a_file_id: i64,
    pub b_file_path: String,
    pub b_filename: String,
    pub b_hash: Option<String>,
    pub reason: String,
    pub resolved: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

