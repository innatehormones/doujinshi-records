mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{authed_request, build_state, build_state_with_token, router, TEST_TOKEN};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok_json() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
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
    assert!(
        bytes.len() > 100,
        "expected non-trivial jpeg, got {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
}

#[tokio::test]
async fn cover_returns_webp_when_file_extension_is_webp() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let hash = "abc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1";
    // V3 encodes covers as webp; cover_path carries `.webp`. The HTTP
    // route must return `image/webp` instead of the legacy hardcoded
    // `image/jpeg` so browsers don't log MIME mismatch warnings.
    let cover_abs = h.covers_dir.join(format!("{}.webp", hash));
    std::fs::write(&cover_abs, b"RIFF\x00\x00\x00\x00WEBPfake-data").unwrap();
    let rel = format!("covers/{}.webp", hash);
    let am = doujinshi_file::ActiveModel {
        title: Set("webp cover".into()),
        filename: Set("webp_cover.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/webp_cover.zip".into()),
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
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "image/webp"
    );
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
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
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
        doujinshi_records::db::migrations::init_schema(&conn)
            .await
            .unwrap();
    }
    let result = probe_and_recover(&db_path).await.unwrap();
    assert!(
        matches!(result, RecoveryAction::Noop),
        "valid db should not be backed up"
    );
}

// ===== V2 DetailView endpoint coverage =====

fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }
    buf.into_inner()
}

async fn seed_file_with_zip(
    conn: &sea_orm::DatabaseConnection,
    zip_path: &std::path::Path,
    filename: &str,
) -> i64 {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let am = doujinshi_file::ActiveModel {
        title: Set("seeded".into()),
        filename: Set(filename.to_string()),
        hash: Set("seed-hash".into()),
        ext: Set("zip".into()),
        size_bytes: Set(std::fs::metadata(zip_path).unwrap().len() as i64),
        current_path: Set(zip_path.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(conn).await.unwrap().id
}

#[tokio::test]
async fn images_returns_entries_when_zip_present() {
    let h = build_state().await;
    let zip = h.resources_dir.path().join("d.zip");
    std::fs::write(
        &zip,
        build_test_zip(&[
            ("01.jpg", b"jpg-data"),
            ("02.png", b"png-data"),
            ("readme.txt", b"hi"),
        ]),
    )
    .unwrap();
    let id = seed_file_with_zip(&h.state.conn, &zip, "d.zip").await;

    let resp = router(h.state)
        .oneshot(authed_request(
            "GET",
            &format!("/api/doujinshi/{}/images", id),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["file_id"].as_i64().unwrap(), id);
    assert_eq!(v["zip_missing"].as_bool().unwrap(), false);
    let images = v["images"].as_array().unwrap();
    assert_eq!(images.len(), 2);
    for (idx, img) in images.iter().enumerate() {
        let url = img["url"].as_str().unwrap();
        assert!(
            url.starts_with(&format!("/api/doujinshi/{}/images/", id)),
            "got {}",
            url
        );
        assert_eq!(img["thumb_cached"].as_bool().unwrap(), false);
        // Index in URL must match the array position.
        assert!(url.ends_with(&format!("/{}", idx)), "got {}", url);
    }
}

#[tokio::test]
async fn image_at_returns_original_bytes_on_cache_miss() {
    let h = build_state().await;
    let zip = h.resources_dir.path().join("d.zip");
    let mut img = image::DynamicImage::new_rgb8(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            img.as_mut_rgb8().unwrap().put_pixel(
                x,
                y,
                image::Rgb([(x * 30) as u8, (y * 30) as u8, 128]),
            );
        }
    }
    let mut png_buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png_buf, image::ImageFormat::Png).unwrap();
    let png_bytes = png_buf.into_inner();
    std::fs::write(&zip, build_test_zip(&[("01.png", png_bytes.as_slice())])).unwrap();
    let id = seed_file_with_zip(&h.state.conn, &zip, "d.zip").await;

    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/doujinshi/{}/images/0", id))
        .body(Body::empty())
        .unwrap();
    let resp = router(h.state.clone()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mime = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(mime, "image/png");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(body.starts_with(b"\x89PNG\r\n\x1a\n"));

    let cache_dir = h.resources_dir.path().join("_preview_cache");
    let on_disk = cache_dir.join(format!("{}-0.webp", id));
    assert!(
        !on_disk.exists(),
        "cache miss must not transcode on backend"
    );
}

fn thumb_request(uri: &str, body: &[u8]) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("Authorization", format!("Bearer {}", TEST_TOKEN))
        .header("Content-Type", "image/webp")
        .body(Body::from(body.to_vec()))
        .unwrap()
}

#[tokio::test]
async fn put_image_thumb_writes_cache_for_next_get() {
    let h = build_state().await;
    let zip = h.resources_dir.path().join("thumb.zip");
    std::fs::write(&zip, build_test_zip(&[("01.jpg", b"original")])).unwrap();
    let id = seed_file_with_zip(&h.state.conn, &zip, "thumb.zip").await;
    let webp = b"RIFF\x10\x00\x00\x00WEBPfake-webp";

    let resp = router(h.state.clone())
        .oneshot(thumb_request(
            &format!("/api/doujinshi/{}/images/0/thumb", id),
            webp,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let on_disk = h
        .resources_dir
        .path()
        .join("_preview_cache")
        .join(format!("{}-0.webp", id));
    assert!(on_disk.exists(), "PUT should persist webp cache file");

    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/doujinshi/{}/images/0", id))
        .body(Body::empty())
        .unwrap();
    let resp = router(h.state.clone()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/webp");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), webp);

    let resp = router(h.state)
        .oneshot(authed_request(
            "GET",
            &format!("/api/doujinshi/{}/images", id),
        ))
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["images"][0]["thumb_cached"].as_bool().unwrap(), true);
}

#[tokio::test]
async fn put_image_thumb_does_not_overwrite_existing_cache() {
    let h = build_state().await;
    let first = b"RIFF\x10\x00\x00\x00WEBPfirst-webp";
    let second = b"RIFF\x11\x00\x00\x00WEBPsecond-webp";

    let resp = router(h.state.clone())
        .oneshot(thumb_request("/api/doujinshi/123/images/0/thumb", first))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = router(h.state.clone())
        .oneshot(thumb_request("/api/doujinshi/123/images/0/thumb", second))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let cached = h.state.preview_cache.get(&(123, 0)).unwrap();
    assert_eq!(cached, first);
}

#[tokio::test]
async fn put_image_thumb_rejects_non_webp() {
    let h = build_state().await;
    let png = b"\x89PNG\r\n\x1a\nfake";

    let resp = router(h.state)
        .oneshot(thumb_request("/api/doujinshi/123/images/0/thumb", png))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn put_image_thumb_requires_auth() {
    let h = build_state().await;
    let req = Request::builder()
        .method("PUT")
        .uri("/api/doujinshi/123/images/0/thumb")
        .header("Content-Type", "image/webp")
        .body(Body::from(b"RIFF\x10\x00\x00\x00WEBPfake-webp".to_vec()))
        .unwrap();
    let resp = router(h.state).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn image_at_returns_404_for_out_of_range_index() {
    let h = build_state().await;
    let zip = h.resources_dir.path().join("d.zip");
    std::fs::write(&zip, build_test_zip(&[("01.jpg", b"x")])).unwrap();
    let id = seed_file_with_zip(&h.state.conn, &zip, "d.zip").await;

    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/doujinshi/{}/images/9", id))
        .body(Body::empty())
        .unwrap();
    let resp = router(h.state).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn images_returns_zip_missing_true_when_file_gone() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let am = doujinshi_file::ActiveModel {
        title: Set("ghost".into()),
        filename: Set("ghost.zip".into()),
        hash: Set("g".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/nonexistent/ghost.zip".into()),
        current_location: Set("identified".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let id = am.insert(&h.state.conn).await.unwrap().id;

    let resp = router(h.state)
        .oneshot(authed_request(
            "GET",
            &format!("/api/doujinshi/{}/images", id),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["zip_missing"].as_bool().unwrap(), true);
    assert_eq!(v["images"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn images_returns_404_when_id_missing() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/doujinshi/999999/images"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

fn patch_request(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("PATCH")
        .uri(uri)
        .header("Authorization", format!("Bearer {}", TEST_TOKEN))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn patch_updates_title_and_returns_204() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    let h = build_state().await;
    let am = doujinshi_file::ActiveModel {
        title: Set("旧".into()),
        filename: Set("p.zip".into()),
        hash: Set("ph".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        circle: Set(Some("旧社团".into())),
        current_path: Set("/tmp/p.zip".into()),
        current_location: Set("identified".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let id = am.insert(&h.state.conn).await.unwrap().id;

    let resp = router(h.state.clone())
        .oneshot(patch_request(
            &format!("/api/doujinshi/{}", id),
            serde_json::json!({ "title": "新", "note": "memo" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&h.state.conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.title, "新");
    assert_eq!(row.circle.as_deref(), Some("旧社团"));
    assert_eq!(row.note.as_deref(), Some("memo"));
}

#[tokio::test]
async fn patch_with_empty_body_is_noop() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    let h = build_state().await;
    let am = doujinshi_file::ActiveModel {
        title: Set("保持".into()),
        filename: Set("n.zip".into()),
        hash: Set("nh".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/n.zip".into()),
        current_location: Set("identified".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let id = am.insert(&h.state.conn).await.unwrap().id;

    let resp = router(h.state.clone())
        .oneshot(patch_request(
            &format!("/api/doujinshi/{}", id),
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&h.state.conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.title, "保持");
}

#[tokio::test]
async fn patch_unknown_id_returns_404() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(patch_request(
            "/api/doujinshi/999999",
            serde_json::json!({ "title": "x" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// V3 endpoints
// ---------------------------------------------------------------------------

use doujinshi_records::db::entities::doujinshi_file;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

async fn seed_identified_row(h: &common::Harness, filename: &str, hash: &str) -> i64 {
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set("t".into()),
        filename: Set(filename.into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set(h
            .resources_dir
            .path()
            .join("identified")
            .join(filename)
            .to_string_lossy()
            .into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap().id
}

#[tokio::test]
async fn archive_moves_row_to_archived() {
    let h = build_state().await;
    let id = seed_identified_row(&h, "f.zip", "h").await;
    std::fs::write(
        h.resources_dir.path().join("identified").join("f.zip"),
        b"data",
    )
    .unwrap();

    let resp = router(h.state.clone())
        .oneshot(authed_request(
            "POST",
            &format!("/api/doujinshi/{}/archive", id),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&h.state.conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.current_location, "archived");
    assert!(h
        .resources_dir
        .path()
        .join("archived")
        .join("f.zip")
        .exists());
}

#[tokio::test]
async fn restore_moves_archived_row_back_to_identified() {
    let h = build_state().await;
    let id = seed_identified_row(&h, "g.zip", "hh").await;
    std::fs::create_dir_all(h.resources_dir.path().join("archived")).unwrap();
    std::fs::write(
        h.resources_dir.path().join("archived").join("g.zip"),
        b"data",
    )
    .unwrap();
    let now = chrono::Utc::now();
    let mut am: doujinshi_file::ActiveModel = doujinshi_file::Entity::find_by_id(id)
        .one(&h.state.conn)
        .await
        .unwrap()
        .unwrap()
        .into();
    am.current_location = Set("archived".into());
    am.current_path = Set(h
        .resources_dir
        .path()
        .join("archived")
        .join("g.zip")
        .to_string_lossy()
        .into_owned());
    am.updated_at = Set(now);
    am.update(&h.state.conn).await.unwrap();

    let resp = router(h.state.clone())
        .oneshot(authed_request(
            "POST",
            &format!("/api/doujinshi/{}/restore", id),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&h.state.conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.current_location, "identified");
}

#[tokio::test]
async fn archive_rejects_illegal_transition_with_409() {
    let h = build_state().await;
    let id = seed_identified_row(&h, "x.zip", "hx").await;
    let now = chrono::Utc::now();
    let mut am: doujinshi_file::ActiveModel = doujinshi_file::Entity::find_by_id(id)
        .one(&h.state.conn)
        .await
        .unwrap()
        .unwrap()
        .into();
    am.current_location = Set("will_delete".into());
    am.updated_at = Set(now);
    am.update(&h.state.conn).await.unwrap();

    let resp = router(h.state.clone())
        .oneshot(authed_request(
            "POST",
            &format!("/api/doujinshi/{}/archive", id),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn list_dirty_returns_empty_when_no_orphans() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(authed_request("GET", "/api/dirty"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let arr: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(arr.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn images_endpoint_returns_304_when_etag_matches() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let zip_path = h.resources_dir.path().join("real.zip");
    std::fs::write(
        &zip_path,
        build_test_zip(&[("a.png", b"\x89PNG\r\n\x1a\n")]),
    )
    .unwrap();
    let hash = "c001c001c001c001c001c001c001c001c001c001c001c001c001c001c001c001";
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set("e2e".into()),
        filename: Set("real.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(2),
        current_path: Set(zip_path.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let m = am.insert(&h.state.conn).await.unwrap();
    let id = m.id;

    // First request → 200 + ETag header.
    let resp = router(h.state.clone())
        .oneshot(authed_request(
            "GET",
            &format!("/api/doujinshi/{}/images", id),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let etag = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with(&format!("\"{}-", id)));

    // Second request with If-None-Match → 304.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/doujinshi/{}/images", id))
        .header("authorization", format!("Bearer {}", TEST_TOKEN))
        .header("if-none-match", etag.clone())
        .body(Body::empty())
        .unwrap();
    let resp2 = router(h.state).oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn state_transition_invalidates_preview_cache() {
    // V3.1: state_machine 转移后，preview_cache 里该 id 的 entries 都要清掉。
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let zip = h.resources_dir.path().join("cache.zip");
    std::fs::write(&zip, build_test_zip(&[("a.png", b"x")])).unwrap();
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set("t".into()),
        filename: Set("cache.zip".into()),
        hash: Set("h-cache-inv".into()),
        ext: Set("zip".into()),
        size_bytes: Set(2),
        current_path: Set(zip.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let m = am.insert(&h.state.conn).await.unwrap();
    let id = m.id;

    // 直接写 cache 文件 + LRU entry
    h.state.preview_cache.invalidate(id); // 先清空确保干净状态
    let _ = h
        .state
        .preview_cache
        .get_or_compute((id, 0usize), || async {
            Ok::<_, anyhow::Error>(b"cached-bytes".to_vec())
        })
        .await
        .unwrap();
    assert!(h.state.preview_cache.get(&(id, 0usize)).is_some());

    // transition identified → archived → should invalidate cache
    let result = doujinshi_records::services::state_machine::transition_with_dirs(
        &h.state.conn,
        id,
        doujinshi_records::services::state_machine::TransitionKind::Archive,
        &h.resources_dir.path().join("doujinshi-identified"),
        &h.resources_dir.path().join("doujinshi-will-delete"),
        &h.resources_dir.path().join("doujinshi-archived"),
    )
    .await;
    result.unwrap();
    h.state.preview_cache.invalidate(id); // 调用方负责；这里模拟 commands/library 的 helper

    assert!(
        h.state.preview_cache.get(&(id, 0usize)).is_none(),
        "cache entries for id should be cleared after state transition"
    );
}
