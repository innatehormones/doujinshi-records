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
    pub version_tag: Option<String>,
    pub current_path: String,
    pub current_location: String,
    pub cover_path: Option<String>,
    pub marked_for_delete: bool,
    pub has_physical_file: bool,
    pub viewed: bool,
    pub note: Option<String>,
    pub rating: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
