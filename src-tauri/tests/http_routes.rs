mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{authed_request, build_state, build_state_with_token, router};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok_json() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap().to_string();
    assert!(ct.starts_with("application/json"), "got {}", ct);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn search_empty_db_returns_zero_items() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/doujinshi/search?q=anything"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0);
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn by_hash_returns_null_when_missing() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/doujinshi/by-hash/deadbeef"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"null", "expected JSON null");
}

#[tokio::test]
async fn by_id_returns_404_when_missing() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/doujinshi/999999"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cover_returns_404_when_hash_unknown() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/covers/deadbeef"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cover_returns_404_when_row_exists_but_no_cover_path() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let am = doujinshi_file::ActiveModel {
        title: Set("no cover".into()),
        filename: Set("no_cover.zip".into()),
        hash: Set("abc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/no_cover.zip".into()),
        current_location: Set("identified".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap();
    let resp = router(h.state)
        .oneshot(authed_request(
            "GET",
            "/api/covers/abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cover_returns_jpeg_when_file_present() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let hash = "fff000fff000fff000fff000fff000fff000fff000fff000fff000fff000fff0";
    let cover_abs = h.covers_dir.join(format!("{}.jpg", hash));
    let img = image::RgbImage::from_fn(2, 2, |_, _| image::Rgb([255, 255, 255]));
    let mut f = std::fs::File::create(&cover_abs).unwrap();
    image::write_buffer_with_format(
        &mut f,
        img.as_raw(),
        2,
        2,
        image::ExtendedColorType::Rgb8,
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    let rel = format!("covers/{}.jpg", hash);
    let am = doujinshi_file::ActiveModel {
        title: Set("has cover".into()),
        filename: Set("has_cover.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/has_cover.zip".into()),
        current_location: Set("identified".into()),
        cover_path: Set(Some(rel)),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap();
    let resp = router(h.state)
        .oneshot(authed_request("GET", &format!("/api/covers/{}", hash)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("image/jpeg"));
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.len() > 100, "expected non-trivial jpeg, got {} bytes", bytes.len());
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
}

#[tokio::test]
async fn cover_serves_placeholder_when_disk_file_missing() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let hash = "ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc1";
    let rel = format!("covers/{}.jpg", hash);
    let am = doujinshi_file::ActiveModel {
        title: Set("ghost cover".into()),
        filename: Set("ghost_cover.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/ghost_cover.zip".into()),
        current_location: Set("identified".into()),
        cover_path: Set(Some(rel)),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap();
    let resp = router(h.state)
        .oneshot(authed_request("GET", &format!("/api/covers/{}", hash)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("image/png"));
}

#[tokio::test]
async fn search_filters_by_title_and_status() {
    use chrono::Utc;
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let now = Utc::now();
    for (i, (title, viewed, marked)) in [
        ("Hatsune Miku 2024", false, false),
        ("Hatsune Miku 2025", true, false),
        ("Kagamine Rin", false, true),
    ]
    .into_iter()
    .enumerate()
    {
        let hash = format!("row{:02x}{:063}", i, 0);
        let am = doujinshi_file::ActiveModel {
            title: Set(title.into()),
            filename: Set(format!("row_{}.zip", i)),
            hash: Set(hash),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            current_path: Set(format!("/tmp/row_{}.zip", i)),
            current_location: Set("identified".into()),
            viewed: Set(viewed),
            marked_for_delete: Set(marked),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        am.insert(&h.state.conn).await.unwrap();
    }
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/doujinshi/search?q=Hatsune"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 2, "expected 2 Hatsune rows");
}

// ===== Auth middleware tests =====

#[tokio::test]
async fn protected_route_returns_401_without_token() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/search")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_returns_401_with_wrong_token() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/search")
                .header("Authorization", "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_returns_200_with_correct_token() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/search")
                .header("Authorization", "Bearer test-token-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_route_is_exempt_from_auth() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state)
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn covers_route_is_exempt_from_auth() {
    // Cover URLs are baked into <img src="..."> tags by the
    // frontend, so they must work without an Authorization header.
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/covers/deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // 404 because the row doesn't exist — but it must NOT be 401.
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn probe_and_recover_moves_corrupt_db() {
    use doujinshi_records::db::recovery::{probe_and_recover, RecoveryAction};
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    std::fs::write(&db_path, b"this is not a sqlite file at all").unwrap();
    let result = probe_and_recover(&db_path).await.unwrap();
    match result {
        RecoveryAction::BackedUp { backup_path } => {
            assert!(backup_path.exists());
            assert!(!db_path.exists(), "corrupt file should be renamed");
        }
        RecoveryAction::Noop => panic!("expected BackedUp for non-sqlite bytes"),
    }
}

#[tokio::test]
async fn probe_and_recover_noop_when_db_is_valid() {
    use doujinshi_records::db::recovery::{probe_and_recover, RecoveryAction};
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    {
        let conn = doujinshi_records::db::connect(&db_path).await.unwrap();
        doujinshi_records::db::migrations::init_schema(&conn).await.unwrap();
    }
    let result = probe_and_recover(&db_path).await.unwrap();
    assert!(
        matches!(result, RecoveryAction::Noop),
        "valid db should not be backed up"
    );
}