use crate::db::entities::{conflict, doujinshi_file, filename_alias, scan_event};
use anyhow::{anyhow, Result};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub enum IdentifyOutcome {
    AlreadyKnown(i64),
    NewIdentified(i64),
    Conflict { a_id: i64, b_path: PathBuf },
    Error(String),
}

/// Error categories surfaced by the identifier. The frontend maps
/// these to specific UI cards (download links for UnrarNotInstalled,
/// size dialog for TooLarge, etc.) — see InboxView's RAR error cards
/// added in task #7-5.
#[derive(Debug, Error)]
pub enum IdentifierError {
    #[error("本机未安装 RAR 解压工具（WinRAR / 7-Zip）")]
    UnrarNotInstalled,
    #[error("RAR 文件过大 ({size_mb:.0} MB > {limit_mb} MB)")]
    TooLarge { size_mb: f64, limit_mb: u64 },
    #[error("磁盘空间不足：解压需 {needed_mb:.0} MB，剩余 {available_mb} MB")]
    InsufficientSpace { needed_mb: f64, available_mb: u64 },
    #[error("RAR 解压失败: {0}")]
    ExtractionFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<std::io::Error> for IdentifierError {
    fn from(e: std::io::Error) -> Self {
        IdentifierError::Other(anyhow::Error::from(e))
    }
}

impl From<sea_orm::DbErr> for IdentifierError {
    fn from(e: sea_orm::DbErr) -> Self {
        IdentifierError::Other(anyhow::Error::from(e))
    }
}

/// Wire format emitted to the frontend when `identify_file` fails on
/// a RAR. Mirrors the `RarError` discriminated union in
/// `src/types/api.ts`. JSON-encoded and sent over the
/// `rar-error` Tauri event.
#[derive(Debug, serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RarErrorPayload {
    UnrarNotInstalled,
    TooLarge { size_mb: f64, limit_mb: u64 },
    InsufficientSpace { needed_mb: f64, available_mb: u64 },
    ExtractionFailed { message: String },
}

impl IdentifierError {
    /// Map an `IdentifierError` to its serializable payload variant.
    /// Returns `None` for non-RAR errors (i.e. `Other`) — callers
    /// only forward this when the file under inspection is `.rar`.
    pub fn to_rar_payload(&self) -> Option<RarErrorPayload> {
        match self {
            IdentifierError::UnrarNotInstalled => {
                Some(RarErrorPayload::UnrarNotInstalled)
            }
            IdentifierError::TooLarge { size_mb, limit_mb } => {
                Some(RarErrorPayload::TooLarge {
                    size_mb: *size_mb,
                    limit_mb: *limit_mb,
                })
            }
            IdentifierError::InsufficientSpace {
                needed_mb,
                available_mb,
            } => Some(RarErrorPayload::InsufficientSpace {
                needed_mb: *needed_mb,
                available_mb: *available_mb,
            }),
            IdentifierError::ExtractionFailed(msg) => {
                Some(RarErrorPayload::ExtractionFailed { message: msg.clone() })
            }
            IdentifierError::Other(_) => None,
        }
    }
}

/// RAR size gate: >1 GB 直接拒（用户在前端确认"仍要解压"后由 `force_extract`
/// 跳过这条护栏，参照 `force_extract` 命令）。
const RAR_MEDIUM_BYTES: u64 = 1024 * 1024 * 1024; // 1 GB

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
    // For RAR files only. When `false` (default), files in the
    // medium tier (200 MB~1 GB) are refused with TooLarge so the
    // frontend can show a confirmation dialog. When `true`, the
    // size gate is skipped and extraction proceeds.
    skip_size_gate: bool,
) -> Result<IdentifyOutcome, IdentifierError> {
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

    // 0) RAR size gate (zip 永远不受影响)
    if ext == "rar" && !skip_size_gate {
        check_rar_size(file_path)?;
    }

    // 1) hash（zip 和 rar 都算源文件本身的 hash——rar 算压缩包 hash，
    //    不是解压后内容的 hash。这样去重逻辑对两种格式一视同仁。）
    let hash = crate::services::hasher::hash_file(file_path).await?;

    // 2) hash exists?
    if let Some(existing) = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Hash.eq(&hash))
        .one(conn)
        .await?
    {
        if existing.status == "in_library" {
            // 同 hash 已存在且在 in_library：inbox 副本是冗余的，不应该
            // 进入 identified 也不应该触发冲突（否则就退化成 step 4 的
            // name+ext 检查）。直接把 inbox 的副本删掉，仅刷 alias +
            // filename + updated_at，last_seen_path 仍指原 identified
            // 副本——否则 dirty_scanner 会把原来的 identified/[...].zip
            // 误判为孤儿。
            store_alias(conn, existing.id, &filename).await?;
            let _ = std::fs::remove_file(file_path);
            let mut am: doujinshi_file::ActiveModel = existing.clone().into();
            am.filename = Set(filename);
            am.updated_at = Set(chrono::Utc::now());
            am.update(conn).await?;
        } else {
            // V4：行处于 archived / recycle / deleted，移源文件到 identified/
            // 并恢复为 status='in_library'。deleted 行复活也走同一条。
            let identified_dir = crate::config::AppConfig::load()?.identified_dir();
            reactivate_row(conn, existing.id, file_path, &identified_dir).await?;
        }
        return Ok(IdentifyOutcome::AlreadyKnown(existing.id));
    }

    // 3) parse filename
    let parsed = crate::services::filename_parser::parse(&filename);

    // 4) check name+ext collision — skipped when `force_rename` is
    // set, because the caller (conflict resolution) has already
    // acknowledged a name+ext collision and the post-rename filename
    // is guaranteed not to collide with the kept entry. The move
    // (step 6) still guards against a same-renamed-filename file
    // already sitting in identified_dir.
    let collision = if force_rename.is_none() {
        // V4：在 3 个"活的"状态里查 (filename, ext) 撞名——`deleted` 不参与
        // （已销毁的记录不占用 filename）。`force_rename` 表示调用方已确认
        // 是别名冲突，强制走"加后缀"路径，绕过该检查。
        doujinshi_file::Entity::find()
            .filter(
                doujinshi_file::Column::Filename
                    .eq(&filename)
                    .and(doujinshi_file::Column::Ext.eq(&ext))
                    .and(doujinshi_file::Column::Status.is_in([
                        "in_library",
                        "archived",
                        "recycle",
                    ])),
            )
            .one(conn)
            .await?
    } else {
        None
    };
    if let Some(a) = collision {
        record_conflict(conn, a.id, file_path, &filename).await?;
        return Ok(IdentifyOutcome::Conflict {
            a_id: a.id,
            b_path: file_path.to_owned(),
        });
    }

    // 5) extract cover (best-effort, format-specific)
    let cover_rel = extract_cover(file_path, &ext, &hash, covers_dir)
        .await
        .ok()
        .flatten();

    // 6) finalize: move + insert + alias + event
    finalize_identification(
        conn,
        file_path,
        &filename,
        &ext,
        size_bytes,
        &hash,
        &parsed.title,
        parsed.circle,
        parsed.series,
        parsed.translator,
        parsed.version_tag,
        cover_rel,
        identified_dir,
        force_rename,
    )
    .await
}

/// Refuse RAR files that exceed the hard limit. Medium-tier files
/// are also rejected here (the frontend re-invokes with
/// `skip_size_gate=true` after the user confirms).
fn check_rar_size(path: &Path) -> Result<(), IdentifierError> {
    let size = std::fs::metadata(path)?.len();
    if size > RAR_MEDIUM_BYTES {
        return Err(IdentifierError::TooLarge {
            size_mb: size as f64 / 1024.0 / 1024.0,
            limit_mb: RAR_MEDIUM_BYTES / 1024 / 1024,
        });
    }
    Ok(())
}

/// Extract a cover image from either a zip (via the zip crate) or a
/// RAR (via unrar/7z subprocess into a tempdir). Returns the relative
/// `covers/{hash}.jpg` path on success, None if no image was found.
async fn extract_cover(
    file_path: &Path,
    ext: &str,
    hash: &str,
    covers_dir: &Path,
) -> Result<Option<String>> {
    let picked = match ext {
        "zip" => {
            let list = crate::services::archive::list_images(file_path)?;
            crate::services::archive::pick_cover(&list).map(|e| e.data.clone())
        }
        "rar" => {
            let tool = crate::services::rar_detect::detect()
                .ok_or(IdentifierError::UnrarNotInstalled)?;
            let stats = preflight_rar_disk_space(file_path, &tool).await?;
            if stats > 0 {
                let available = crate::services::disk_space::available_bytes(file_path)
                    .unwrap_or(u64::MAX);
                if stats > available {
                    return Err(IdentifierError::InsufficientSpace {
                        needed_mb: stats as f64 / 1024.0 / 1024.0,
                        available_mb: available / 1024 / 1024,
                    }
                    .into());
                }
            }
            let tmp = tempfile::tempdir()?;
            crate::services::archive::extract_rar(file_path, tmp.path(), &tool)
                .await
                .map_err(|e| IdentifierError::ExtractionFailed(e.to_string()))?;
            let list = crate::services::archive::list_images_in_dir(tmp.path())?;
            // tempdir 在函数退出时自动 drop 清理
            crate::services::archive::pick_cover(&list).map(|e| e.data.clone())
        }
        other => return Err(anyhow!("unsupported extension: {}", other)),
    };
    let Some(data) = picked else {
        return Ok(None);
    };
    let out = covers_dir.join(format!("{}.pwb", hash));
    let written = crate::services::cover::extract_and_save(&data, &out).await?;
    Ok(Some(format!(
        "covers/{}",
        written.file_name().unwrap().to_string_lossy()
    )))
}

/// Try to learn the unpacked size of a RAR before extracting, so
/// we can refuse early if the disk can't fit it. Returns 0 if the
/// tool doesn't support listing sizes (7z `-slt` parsing is messy
/// enough that we treat it as unknown and rely on the post-extract
/// check).
async fn preflight_rar_disk_space(
    path: &Path,
    tool: &crate::services::rar_detect::RarLocation,
) -> Result<u64> {
    use crate::services::rar_detect::RarTool;
    let output = match tool.tool {
        RarTool::Unrar => {
            // unrar l 输出第二列是 size（十进制字节数）
            tokio::process::Command::new(&tool.path)
                .args(["l", "-p-", path.to_str().unwrap()])
                .output()
                .await?
        }
        RarTool::SevenZip => {
            tokio::process::Command::new(&tool.path)
                .args(["l", "-slt", path.to_str().unwrap()])
                .output()
                .await?
        }
    };
    if !output.status.success() {
        return Ok(0);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(sum_rar_listed_sizes(&stdout, tool.tool))
}

fn sum_rar_listed_sizes(stdout: &str, tool: crate::services::rar_detect::RarTool) -> u64 {
    use crate::services::rar_detect::RarTool;
    let mut total = 0u64;
    match tool {
        RarTool::Unrar => {
            // 第二列是 size
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(n) = parts[1].replace(',', "").parse::<u64>() {
                        // 跳过 header 行（"Size" 标题）和 dashes
                        if line.contains("------") {
                            continue;
                        }
                        total += n;
                    }
                }
            }
        }
        RarTool::SevenZip => {
            // 7z l -slt 每条记录有 "Size = 12345" 一行
            for line in stdout.lines() {
                if let Some(rest) = line.strip_prefix("Size = ") {
                    if let Ok(n) = rest.trim().parse::<u64>() {
                        total += n;
                    }
                }
            }
        }
    }
    total
}

/// Shared finalization: move the file to `identified_dir`, insert
/// the doujinshi_file row, record the filename alias, and emit the
/// `new_file` scan event. Pulled out so zip and RAR flows don't
/// duplicate these steps.
#[allow(clippy::too_many_arguments)]
async fn finalize_identification(
    conn: &DatabaseConnection,
    file_path: &Path,
    filename: &str,
    ext: &str,
    size_bytes: i64,
    hash: &str,
    title: &str,
    circle: Option<String>,
    series: Option<String>,
    translator: Option<String>,
    version_tag: Option<String>,
    cover_rel: Option<String>,
    identified_dir: &Path,
    force_rename: Option<&str>,
) -> Result<IdentifyOutcome, IdentifierError> {
    // Apply force_rename suffix when the caller asked for one (used
    // by conflict resolve "keep_both"). Strip the extension, append
    // the suffix, then put the extension back so filename still
    // matches what the parser expects on subsequent scans.
    let move_filename = match force_rename {
        Some(suffix) if !suffix.is_empty() => {
            let stem = std::path::Path::new(filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(filename)
                .to_string();
            let ext = std::path::Path::new(filename)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if ext.is_empty() {
                format!("{} {}", stem, suffix)
            } else {
                format!("{} {}.{}", stem, suffix, ext)
            }
        }
        _ => filename.to_string(),
    };
    std::fs::create_dir_all(identified_dir)?;
    let new_path = identified_dir.join(&move_filename);
    if new_path.exists() {
        return Ok(IdentifyOutcome::Error("target exists with different hash".into()));
    }
    std::fs::rename(file_path, &new_path)?;

    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set(title.to_string()),
        filename: Set(move_filename.clone()),
        hash: Set(hash.to_string()),
        ext: Set(ext.to_string()),
        size_bytes: Set(size_bytes),
        circle: Set(circle),
        series: Set(series),
        translator: Set(translator),
        version_tag: Set(version_tag),
        last_seen_path: Set(new_path.to_string_lossy().into_owned()),
        status: Set("in_library".into()),
        cover_path: Set(cover_rel),
        marked_for_delete: Set(false),
        file_state: Set("present".into()),
        viewed: Set(false),
        note: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let inserted = am.insert(conn).await?;

    store_alias(conn, inserted.id, &move_filename).await?;
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

/// 把源文件从 inbox 移到 identified_dir/，并把行状态恢复到 identified。
/// 仅在 hash 命中但行处于非 identified 状态（will_delete/archived）时调用。
/// V2 行为是源文件不动仅更新路径；V3 强制移动以保证
/// current_location='identified' ⇒ 文件在 identified/ 下的不变量。
pub async fn reactivate_row(
    conn: &DatabaseConnection,
    row_id: i64,
    src_file_path: &Path,
    identified_dir: &Path,
) -> Result<i64> {
    use sea_orm::EntityTrait;
    let row = doujinshi_file::Entity::find_by_id(row_id)
        .one(conn)
        .await?
        .ok_or_else(|| anyhow!("file {} not found", row_id))?;

    std::fs::create_dir_all(identified_dir)?;
    let filename = src_file_path
        .file_name()
        .ok_or_else(|| anyhow!("invalid source path: {}", src_file_path.display()))?;
    let dest = identified_dir.join(filename);

    if let Err(e) = std::fs::rename(src_file_path, &dest) {
        if matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
            || e.raw_os_error() == Some(17)
        {
            std::fs::copy(src_file_path, &dest)?;
            std::fs::remove_file(src_file_path)?;
        } else {
            return Err(e.into());
        }
    }

    store_alias(
        conn,
        row_id,
        &dest.file_name().unwrap().to_string_lossy(),
    )
    .await?;

    let mut am: doujinshi_file::ActiveModel = row.into();
    am.last_seen_path = Set(dest.to_string_lossy().into_owned());
    am.status = Set("in_library".into());
    am.file_state = Set("present".into());
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;
    record_event(conn, row_id, "reactivated", None).await?;
    Ok(row_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_rar_listed_sizes_unrar_parses_second_column() {
        let stdout = "\
Archive: test.rar
Details: RAR 5

  Name             Size   Packed Ratio  Date   Time   Attr  CRC  Meth Ver
-------------------------------------------------------------------------------
  images/01.jpg  123456  123456 100%  2024-01-01 00:00  ....  ....  m3d 2.9
  images/02.png   65536   65536 100%  2024-01-01 00:00  ....  ....  m3d 2.9
-------------------------------------------------------------------------------
";
        let total = sum_rar_listed_sizes(
            stdout,
            crate::services::rar_detect::RarTool::Unrar,
        );
        assert_eq!(total, 123456 + 65536);
    }

    #[test]
    fn sum_rar_listed_sizes_7z_parses_size_lines() {
        let stdout = "\
7-Zip [64] 17.04

Listing archive: test.rar

----------
Path = images/01.jpg
Size = 123456
Packed Size = 123456
----------
Path = images/02.png
Size = 65536
Packed Size = 65536
----------
";
        let total = sum_rar_listed_sizes(
            stdout,
            crate::services::rar_detect::RarTool::SevenZip,
        );
        assert_eq!(total, 123456 + 65536);
    }

    #[test]
    fn parse_rar_list_picks_image_extensions() {
        let stdout = "\
Archive: test.rar
  Name             Size
-------------------------------------------------------------------------------
  images/01.jpg  123456
  readme.txt        100
  images/02.png   65536
-------------------------------------------------------------------------------
";
        let names = crate::services::archive::parse_rar_list(stdout);
        assert!(names.iter().any(|n| n.ends_with("01.jpg")));
        assert!(names.iter().any(|n| n.ends_with("02.png")));
        assert!(!names.iter().any(|n| n.ends_with("readme.txt")));
    }

    #[tokio::test]
    async fn reactivate_row_moves_file_and_updates_location() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();

        let inbox = dir.path().join("inbox");
        let identified = dir.path().join("identified");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::create_dir_all(&identified).unwrap();
        let src = inbox.join("f.zip");
        std::fs::write(&src, b"data").unwrap();

        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(4),
            last_seen_path: Set("placeholder".into()),
            // reactivate_row 的来源：V4 任何非 in_library 状态都能复活，
            // 包括 deleted（用户把同 hash 新文件拖进 inbox 把 ghost 行拉回）。
            status: Set("deleted".into()),
            file_state: Set("absent_confirmed".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let id = m.insert(&conn).await.unwrap().id;

        reactivate_row(&conn, id, &src, &identified).await.unwrap();

        assert!(!src.exists(), "src 应被移走");
        assert!(identified.join("f.zip").exists(), "dest 应在 identified/");
        let row = doujinshi_file::Entity::find_by_id(id).one(&conn).await.unwrap().unwrap();
        assert_eq!(row.status, "in_library");
        assert_eq!(row.file_state, "present");
    }

    /// V4：collision check 排除 status='deleted'
    #[tokio::test]
    async fn identify_file_skips_collision_check_for_deleted_rows() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();

        // seed 一行 status='deleted'，filename='f.zip'
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h_deleted".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set("placeholder".into()),
            status: Set("deleted".into()),
            file_state: Set("absent_confirmed".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        // collision check：3 个活状态里查，不包含 deleted
        let collision = doujinshi_file::Entity::find()
            .filter(
                doujinshi_file::Column::Filename
                    .eq("f.zip")
                    .and(doujinshi_file::Column::Ext.eq("zip"))
                    .and(doujinshi_file::Column::Status.is_in([
                        "in_library",
                        "archived",
                        "recycle",
                    ])),
            )
            .one(&conn)
            .await
            .unwrap();
        assert!(
            collision.is_none(),
            "deleted 行不应参与撞名检查"
        );
    }
}