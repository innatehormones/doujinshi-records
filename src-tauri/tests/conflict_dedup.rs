//! Tests for `record_conflict` dedup behavior.
//!
//! 修复前：scanner 每重启一次都 INSERT 一条新 conflict 行——
//! 用户报"待处理冲突每次启动 +1"。修复后：(a_file_id, b_file_path)
//! 已存在则重置 resolved=false + bump created_at，不插新行。

use doujinshi_records::db::{self, entities::{conflict, doujinshi_file}};
use doujinshi_records::services::identifier::record_conflict;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::path::PathBuf;

struct TestEnv {
    conn: DatabaseConnection,
    _resources_dir: tempfile::TempDir,
}

async fn make_env() -> TestEnv {
    let resources_dir = tempfile::tempdir().unwrap();
    let db_path = resources_dir.path().join("data.db");
    let conn = db::connect(&db_path).await.expect("connect");
    db::migrations::init_schema_versioned(&conn).await.expect("init");
    TestEnv { conn, _resources_dir: resources_dir }
}

/// conflict.a_file_id 有 FK 引用 doujinshi_file.id——先插占位行，
/// 拿到自增 id 后再调 record_conflict。
async fn seed_a_row(conn: &DatabaseConnection, marker: &str) -> i64 {
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set(format!("A title {marker}")),
        filename: Set(format!("{marker}.zip")),
        hash: Set(format!("h{marker}")),
        ext: Set("zip".into()),
        size_bytes: Set(1),
        last_seen_path: Set(format!("/tmp/identified/{marker}.zip")),
        status: Set("in_library".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    am.insert(conn).await.unwrap().id
}

async fn count_conflicts(conn: &DatabaseConnection, a_id: i64) -> usize {
    conflict::Entity::find()
        .filter(conflict::Column::AFileId.eq(a_id))
        .all(conn)
        .await
        .unwrap()
        .len()
}

/// 当前 a_id 唯一对应的 conflict 行 id（dedup 后必为单行）。
async fn single_row_id(conn: &DatabaseConnection, a_id: i64) -> i64 {
    conflict::Entity::find()
        .filter(conflict::Column::AFileId.eq(a_id))
        .one(conn)
        .await
        .unwrap()
        .unwrap()
        .id
}

#[tokio::test]
async fn first_call_inserts_one_row() {
    let env = make_env().await;
    let a_id = seed_a_row(&env.conn, "first").await;
    let b_path = PathBuf::from("/tmp/inbox/foo.zip");

    record_conflict(&env.conn, a_id, &b_path, "foo.zip").await.unwrap();

    let rows = conflict::Entity::find()
        .filter(conflict::Column::AFileId.eq(a_id))
        .all(&env.conn)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "first call inserts exactly one row");
    assert!(!rows[0].resolved);
}

#[tokio::test]
async fn repeated_calls_with_same_pair_insert_one_row() {
    let env = make_env().await;
    let a_id = seed_a_row(&env.conn, "dup").await;
    let b_path = PathBuf::from("/tmp/inbox/dup.zip");

    record_conflict(&env.conn, a_id, &b_path, "dup.zip").await.unwrap();
    record_conflict(&env.conn, a_id, &b_path, "dup.zip").await.unwrap();
    record_conflict(&env.conn, a_id, &b_path, "dup.zip").await.unwrap();

    assert_eq!(
        count_conflicts(&env.conn, a_id).await,
        1,
        "同一对 (a_id, b_path) 多次调用只能产生一行"
    );
}

/// 这是用户实际报的 bug：Skip 之后 conflict 行被标 resolved=true
/// 但 B 还坐在 inbox，重启 scanner 又跑一遍——必须把旧行重新打回
/// resolved=false 并 bump created_at，让它重新浮到 InboxView 顶部。
#[tokio::test]
async fn re_flag_resets_resolved_and_bumps_created_at() {
    let env = make_env().await;
    let a_id = seed_a_row(&env.conn, "reflag").await;
    let b_path = PathBuf::from("/tmp/inbox/reflag.zip");

    record_conflict(&env.conn, a_id, &b_path, "reflag.zip").await.unwrap();
    let row_id = single_row_id(&env.conn, a_id).await;

    // 模拟 Skip：手动把这一行标 resolved=true 并把时间倒推 60 秒，
    // 这样下面 bump 后的 created_at 一定会 > 旧值。
    let row = conflict::Entity::find_by_id(row_id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    let original_ts = row.created_at;
    let mut am: conflict::ActiveModel = row.into();
    am.resolved = Set(true);
    am.created_at = Set(original_ts - chrono::Duration::seconds(60));
    am.update(&env.conn).await.unwrap();

    // 模拟重启后 scanner 再次跑同一文件——这时 DB 里这条应是 resolved=true，
    // 不能因此认为"已处理过"而什么都不做；必须重新打开 + bump 时间戳。
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    record_conflict(&env.conn, a_id, &b_path, "reflag.zip").await.unwrap();

    let row = conflict::Entity::find_by_id(row_id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !row.resolved,
        "Skip 后重启 scanner 必须把 resolved 重置回 false"
    );
    assert!(
        row.created_at > original_ts - chrono::Duration::seconds(60),
        "created_at 必须被 bump（实际值 = now() > 原值 - 60s）"
    );
    assert_eq!(count_conflicts(&env.conn, a_id).await, 1, "不能多出行");
}

/// `b_filename` 不参与 dedup key——同 A、同 path 即视为同一冲突。
#[tokio::test]
async fn dedup_key_ignores_b_filename_changes() {
    let env = make_env().await;
    let a_id = seed_a_row(&env.conn, "dupname").await;
    let b_path = PathBuf::from("/tmp/inbox/dup_filename.zip");

    record_conflict(&env.conn, a_id, &b_path, "old_name.zip").await.unwrap();
    record_conflict(&env.conn, a_id, &b_path, "new_name.zip").await.unwrap();

    let rows = conflict::Entity::find()
        .filter(conflict::Column::AFileId.eq(a_id))
        .all(&env.conn)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}

/// 不同 A 行（同 b_path）应分别记录——dedup key 是 (a, b) 不是单 b。
#[tokio::test]
async fn different_a_ids_create_separate_rows() {
    let env = make_env().await;
    let a1 = seed_a_row(&env.conn, "a1").await;
    let a2 = seed_a_row(&env.conn, "a2").await;
    let b_path = PathBuf::from("/tmp/inbox/shared.zip");

    record_conflict(&env.conn, a1, &b_path, "shared.zip").await.unwrap();
    record_conflict(&env.conn, a1, &b_path, "shared.zip").await.unwrap();
    record_conflict(&env.conn, a2, &b_path, "shared.zip").await.unwrap();

    assert_eq!(count_conflicts(&env.conn, a1).await, 1);
    assert_eq!(count_conflicts(&env.conn, a2).await, 1);
}