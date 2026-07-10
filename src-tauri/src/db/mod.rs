pub mod entities;
pub mod migrations;
pub mod recovery;

use sea_orm::{Database, DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter};
use std::path::Path;

pub async fn connect(path: &Path) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let url = format!("sqlite://{}?mode=rwc", path.display());
    Database::connect(&url).await
}

/// Read an `app_setting` row by key. Returns `None` if the key is absent.
pub async fn read_setting(
    conn: &DatabaseConnection,
    key: &str,
) -> Result<Option<String>, sea_orm::DbErr> {
    use entities::app_setting;
    let row = app_setting::Entity::find()
        .filter(app_setting::Column::Key.eq(key))
        .one(conn)
        .await?;
    Ok(row.and_then(|m| m.value))
}

/// Upsert an `app_setting` row. If a row with the same key exists its
/// value is updated; otherwise a new row is inserted.
pub async fn write_setting(
    conn: &DatabaseConnection,
    key: &str,
    value: &str,
) -> Result<(), sea_orm::DbErr> {
    use entities::app_setting;
    use sea_orm::ActiveModelTrait;
    let now = chrono::Utc::now();
    if let Some(existing) = app_setting::Entity::find()
        .filter(app_setting::Column::Key.eq(key))
        .one(conn)
        .await?
    {
        let mut am: app_setting::ActiveModel = existing.into();
        am.value = Set(Some(value.to_string()));
        am.updated_at = Set(now);
        am.update(conn).await?;
    } else {
        let am = app_setting::ActiveModel {
            key: Set(key.to_string()),
            value: Set(Some(value.to_string())),
            updated_at: Set(now),
            ..Default::default()
        };
        // exec_without_returning skips the post-insert SELECT that
        // ActiveModel::insert() performs, which trips on entities whose
        // only "primary key" is a caller-supplied TEXT column.
        app_setting::Entity::insert(am)
            .exec_without_returning(conn)
            .await?;
    }
    Ok(())
}
