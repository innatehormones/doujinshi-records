use crate::db::entities::{conflict, doujinshi_file, filename_alias, scan_event};
use anyhow::Result;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::path::{Path, PathBuf};

pub enum IdentifyOutcome {
    AlreadyKnown(i64),
    NewIdentified(i64),
    Conflict { a_id: i64, b_path: PathBuf },
    Error(String),
}

pub async fn identify_file(
    conn: &DatabaseConnection,
    file_path: &Path,
    covers_dir: &Path,
    identified_dir: &Path,
    // When `Some(suffix)`, append ` {suffix}` to the on-disk
    // filename before moving it into `identified_dir`. Used by the
    // "keep both" conflict action so two copies of the same content
    // can coexist. Suffix is only applied when the file is moved
    // forward (step 6); it does NOT alter alias or conflict checks.
    force_rename: Option<&str>,
) -> Result<IdentifyOutcome> {
    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let size_bytes = std::fs::metadata(file_path)?.len() as i64;

    // 1) hash
    let hash = crate::services::hasher::hash_file(file_path).await?;

    // 2) hash exists?
    if let Some(existing) = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Hash.eq(&hash))
        .one(conn)
        .await?
    {
        store_alias(conn, existing.id, &filename).await?;
        let mut am: doujinshi_file::ActiveModel = existing.clone().into();
        am.filename = Set(filename);
        am.current_path = Set(file_path.to_string_lossy().into_owned());
        am.updated_at = Set(chrono::Utc::now());
        am.update(conn).await?;
        return Ok(IdentifyOutcome::AlreadyKnown(existing.id));
    }

    // 3) parse filename
    let parsed = crate::services::filename_parser::parse(&filename);

    // 4) check name+ext collision
    let collision = doujinshi_file::Entity::find()
        .filter(
            doujinshi_file::Column::Filename
                .eq(&filename)
                .and(doujinshi_file::Column::Ext.eq(&ext))
                .and(doujinshi_file::Column::PhysicallyDeleted.eq(false)),
        )
        .one(conn)
        .await?;
    if let Some(a) = collision {
        record_conflict(conn, a.id, file_path, &filename).await?;
        return Ok(IdentifyOutcome::Conflict {
            a_id: a.id,
            b_path: file_path.to_owned(),
        });
    }

    // 5) extract cover (best-effort)
    let cover_rel = match crate::services::archive::list_images(file_path) {
        Ok(list) => {
            if let Some(picked) = crate::services::archive::pick_cover(&list) {
                let out = covers_dir.join(format!("{}.jpg", hash));
                crate::services::cover::extract_and_save(&picked.data, &out)
                    .await
                    .ok()
                    .map(|p| {
                        format!(
                            "covers/{}",
                            p.file_name().unwrap().to_string_lossy()
                        )
                    })
            } else {
                None
            }
        }
        Err(_) => None,
    };

    // 6) move file to identified_dir
    std::fs::create_dir_all(identified_dir)?;
    // Apply force_rename suffix when the caller asked for one (used
    // by conflict resolve "keep_both"). Strip the extension, append
    // the suffix, then put the extension back so filename still
    // matches what the parser expects on subsequent scans.
    let move_filename = match force_rename {
        Some(suffix) if !suffix.is_empty() => {
            let stem = std::path::Path::new(&filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&filename)
                .to_string();
            let ext = std::path::Path::new(&filename)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if ext.is_empty() {
                format!("{} {}", stem, suffix)
            } else {
                format!("{} {}.{}", stem, suffix, ext)
            }
        }
        _ => filename.clone(),
    };
    let new_path = identified_dir.join(&move_filename);
    if new_path.exists() {
        return Ok(IdentifyOutcome::Error("target exists with different hash".into()));
    }
    std::fs::rename(file_path, &new_path)?;

    // 7) insert doujinshi_file row
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set(parsed.title),
        filename: Set(move_filename.clone()),
        hash: Set(hash),
        ext: Set(ext),
        size_bytes: Set(size_bytes),
        circle: Set(parsed.circle),
        series: Set(parsed.series),
        translator: Set(parsed.translator),
        version_tag: Set(parsed.version_tag),
        current_path: Set(new_path.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        cover_path: Set(cover_rel),
        marked_for_delete: Set(false),
        physically_deleted: Set(false),
        viewed: Set(false),
        note: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let inserted = am.insert(conn).await?;

    // 8) alias (same row)
    store_alias(conn, inserted.id, &move_filename).await?;

    // 9) scan_event
    record_event(conn, inserted.id, "new_file", None).await?;

    Ok(IdentifyOutcome::NewIdentified(inserted.id))
}

pub async fn store_alias(conn: &DatabaseConnection, file_id: i64, alias: &str) -> Result<()> {
    let am = filename_alias::ActiveModel {
        file_id: Set(file_id),
        alias_filename: Set(alias.to_string()),
        first_seen_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let _ = am.insert(conn).await; // ignore UniqueViolation
    Ok(())
}

pub async fn record_conflict(
    conn: &DatabaseConnection,
    a_id: i64,
    b_path: &Path,
    b_filename: &str,
) -> Result<()> {
    let am = conflict::ActiveModel {
        a_file_id: Set(a_id),
        b_file_path: Set(b_path.to_string_lossy().into_owned()),
        b_filename: Set(b_filename.to_string()),
        b_hash: Set(None),
        reason: Set("name_ext_collision".into()),
        resolved: Set(false),
        created_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let _ = am.insert(conn).await;
    Ok(())
}

pub async fn record_event(
    conn: &DatabaseConnection,
    file_id: i64,
    kind: &str,
    detail: Option<serde_json::Value>,
) -> Result<()> {
    let am = scan_event::ActiveModel {
        event_type: Set(kind.into()),
        file_id: Set(Some(file_id)),
        detail: Set(detail.map(|v| v.to_string())),
        created_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let _ = am.insert(conn).await;
    Ok(())
}

