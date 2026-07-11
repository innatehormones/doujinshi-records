//! Tests for the 4-action conflict resolution flow.
//!
//! Each test stands up a real `DatabaseConnection` + on-disk
//! directory layout under a fresh `tempdir`, inserts an A row + zip
//! and a B inbox zip, then calls `resolve_conflict_inner` directly.
//!
//! We deliberately avoid `AppState` here: that struct holds an
//! `Arc<Scanner>` which transitively references `tauri::AppHandle`
//! and pulls the whole `tauri` + `tao` + `wry` stack into the test
//! binary. On Windows the GUI subsystem DLLs that brings in are not
//! loadable by `cargo test`'s runner, and the binary crashes at
//! startup with STATUS_ENTRYPOINT_NOT_FOUND. The inner helper
//! accepts only the bits it actually needs (DB handle + paths).

mod common;

use base64::Engine;
use doujinshi_records::commands::inbox::{ConflictAction, resolve_conflict_inner};
use doujinshi_records::db::{self, entities::{conflict, doujinshi_file}};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

struct TestEnv {
    conn: DatabaseConnection,
    covers_dir: std::path::PathBuf,
    identified_dir: std::path::PathBuf,
    inbox_dir: std::path::PathBuf,
    // Keep the TempDir alive for the test duration.
    _resources_dir: tempfile::TempDir,
}

async fn make_env() -> TestEnv {
    let resources_dir = tempfile::tempdir().unwrap();
    let covers_dir = resources_dir.path().join("covers");
    let identified_dir = resources_dir.path().join("identified");
    let inbox_dir = resources_dir.path().join("inbox");
    std::fs::create_dir_all(&covers_dir).unwrap();
    std::fs::create_dir_all(&identified_dir).unwrap();
    std::fs::create_dir_all(&inbox_dir).unwrap();
    let db_path = resources_dir.path().join("data.db");
    let conn = db::connect(&db_path).await.expect("connect");
    db::migrations::init_schema_versioned(&conn).await.expect("init");
    TestEnv {
        conn,
        covers_dir,
        identified_dir,
        inbox_dir,
        _resources_dir: resources_dir,
    }
}

fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }
    buf.into_inner()
}

/// Returns `(a_id, a_path, c_id, b_hash)`.
async fn seed_conflict(
    env: &TestEnv,
    filename: &str,
) -> (i64, std::path::PathBuf, i64, String) {
    // A (already identified) — placed under identified_dir.
    let a_zip = build_zip(&[("01.jpg", b"AAAAfakejpg"), ("02.png", b"AAAAfakepng")]);
    let a_path = env.identified_dir.join(filename);
    std::fs::write(&a_path, &a_zip).unwrap();
    let hash_a = blake_b64(&a_zip);
    let now = chrono::Utc::now();
    let a_am = doujinshi_file::ActiveModel {
        title: Set("A title".into()),
        filename: Set(filename.to_string()),
        hash: Set(hash_a.clone()),
        ext: Set("zip".into()),
        size_bytes: Set(a_zip.len() as i64),
        current_path: Set(a_path.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let a_inserted = a_am.insert(&env.conn).await.unwrap();

    // B (still in inbox, different content => different hash).
    let b_zip = build_zip(&[("01.jpg", b"BBBBfakejpg"), ("02.png", b"BBBBfakepng")]);
    let b_path = env.inbox_dir.join(filename);
    std::fs::write(&b_path, &b_zip).unwrap();
    let hash_b = blake_b64(&b_zip);

    let c_am = conflict::ActiveModel {
        a_file_id: Set(a_inserted.id),
        b_file_path: Set(b_path.to_string_lossy().into_owned()),
        b_filename: Set(filename.to_string()),
        b_hash: Set(Some(hash_b.clone())),
        reason: Set("name_ext_collision".into()),
        resolved: Set(false),
        created_at: Set(now),
        ..Default::default()
    };
    let c_inserted = c_am.insert(&env.conn).await.unwrap();

    (a_inserted.id, a_path, c_inserted.id, hash_b)
}

fn blake_b64(data: &[u8]) -> String {
    use blake3::Hasher;
    let mut h = Hasher::new();
    h.update(data);
    let bytes = h.finalize().as_bytes().to_vec();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes[..32])
}

async fn conflict_is_resolved(conn: &DatabaseConnection, id: i64) -> bool {
    conflict::Entity::find_by_id(id)
        .one(conn)
        .await
        .unwrap()
        .map(|m| m.resolved)
        .unwrap_or(false)
}

#[tokio::test]
async fn resolve_skip_leaves_b_in_inbox() {
    let env = make_env().await;
    let (a_id, a_path, c_id, _b_hash) = seed_conflict(&env, "skip_me.zip").await;

    resolve_conflict_inner(
        &env.conn,
        &env.covers_dir,
        &env.identified_dir,
        c_id,
        ConflictAction::Skip,
    )
    .await
    .unwrap();

    // B still on disk
    assert!(env.inbox_dir.join("skip_me.zip").exists());
    // A still on disk
    assert!(a_path.exists());
    // A row untouched
    let a = doujinshi_file::Entity::find_by_id(a_id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a.title, "A title");
    // Conflict marked resolved
    assert!(conflict_is_resolved(&env.conn, c_id).await);
}

#[tokio::test]
async fn resolve_keep_a_deletes_b_file() {
    let env = make_env().await;
    let (a_id, a_path, c_id, _b_hash) = seed_conflict(&env, "keep_a.zip").await;

    resolve_conflict_inner(
        &env.conn,
        &env.covers_dir,
        &env.identified_dir,
        c_id,
        ConflictAction::KeepA,
    )
    .await
    .unwrap();

    // B removed from inbox
    assert!(!env.inbox_dir.join("keep_a.zip").exists());
    // A still on disk and row intact
    assert!(a_path.exists());
    let a = doujinshi_file::Entity::find_by_id(a_id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a.title, "A title");
    // No new row inserted
    let count = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Filename.eq("keep_a.zip"))
        .all(&env.conn)
        .await
        .unwrap()
        .len();
    assert_eq!(count, 1, "expected only A, got {} rows", count);
    assert!(conflict_is_resolved(&env.conn, c_id).await);
}

#[tokio::test]
async fn resolve_replace_b_promotes_b_to_library() {
    let env = make_env().await;
    let (a_id, a_path, c_id, b_hash) = seed_conflict(&env, "replace_b.zip").await;

    resolve_conflict_inner(
        &env.conn,
        &env.covers_dir,
        &env.identified_dir,
        c_id,
        ConflictAction::ReplaceB,
    )
    .await
    .unwrap();

    // B took over A's path: the file at that location now has B's
    // bytes (so a_path.exists() is true; the original assertion
    // `!a_path.exists()` was checking the wrong invariant).
    assert!(a_path.exists(), "B's zip should be at A's former path");
    let bytes = std::fs::read(&a_path).unwrap();
    assert_eq!(
        blake_b64(&bytes),
        b_hash,
        "file at A's path should be B's bytes, not A's"
    );
    assert!(!env.inbox_dir.join("replace_b.zip").exists());
    // A row remains (history) and is marked physically_deleted
    let a = doujinshi_file::Entity::find_by_id(a_id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    assert!(a.physically_deleted, "A should be marked physically_deleted");
    // B got a new doujinshi_file row
    let rows = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Filename.eq("replace_b.zip"))
        .all(&env.conn)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "expected A + B rows, got {}", rows.len());
    let b_row = rows.iter().find(|r| !r.physically_deleted).unwrap();
    assert_ne!(b_row.hash, a.hash, "B's row should hold a different hash than A");
    assert_eq!(b_row.filename, "replace_b.zip");
    assert!(conflict_is_resolved(&env.conn, c_id).await);
}

#[tokio::test]
async fn resolve_keep_both_inserts_b_with_copy_suffix() {
    let env = make_env().await;
    let (_a_id, a_path, c_id, _b_hash) = seed_conflict(&env, "keep_both.zip").await;

    resolve_conflict_inner(
        &env.conn,
        &env.covers_dir,
        &env.identified_dir,
        c_id,
        ConflictAction::KeepBoth,
    )
    .await
    .unwrap();

    // A still on disk
    assert!(a_path.exists(), "A's zip should remain");
    // B moved under a " (copy)" suffix
    let suffixed = env.identified_dir.join("keep_both (copy).zip");
    assert!(suffixed.exists(), "B should land at {:?}", suffixed);
    // The plain "keep_both.zip" should no longer be in inbox
    assert!(!env.inbox_dir.join("keep_both.zip").exists());
    // Both rows exist
    let rows = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Filename.starts_with("keep_both"))
        .all(&env.conn)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "expected A + B(copy) rows");
    assert!(conflict_is_resolved(&env.conn, c_id).await);
}