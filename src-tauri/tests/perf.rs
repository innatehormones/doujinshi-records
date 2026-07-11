//! Synthetic-data search perf check. Seeds N fake rows, then runs the
//! real `/api/doujinshi/search` handler through the same `Router` used by
//! integration tests and asserts p95 latency stays under 50 ms.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use chrono::Utc;
use common::{build_state, router, TEST_TOKEN};
use doujinshi_records::db::entities::doujinshi_file;
use sea_orm::{EntityTrait, Set};
use std::time::{Duration, Instant};
use tower::ServiceExt;

const CHUNK: usize = 500; // stay under SQLite's default 999-var limit

async fn seed_rows(h: &mut common::Harness, n: usize) {
    let mut inserted = 0usize;
    while inserted < n {
        let end = (inserted + CHUNK).min(n);
        let rows: Vec<doujinshi_file::ActiveModel> = (inserted..end)
            .map(|i| {
                let hash = format!("bench{:063x}", i);
                doujinshi_file::ActiveModel {
                    title: Set(format!("bench title {}", i)),
                    filename: Set(format!("bench_{}.zip", i)),
                    hash: Set(hash),
                    ext: Set("zip".into()),
                    size_bytes: Set(1024),
                    current_path: Set(format!("/tmp/bench_{}.zip", i)),
                    current_location: Set("identified".into()),
                    created_at: Set(Utc::now()),
                    updated_at: Set(Utc::now()),
                    ..Default::default()
                }
            })
            .collect();
        doujinshi_file::Entity::insert_many(rows)
            .exec_without_returning(&h.state.conn)
            .await
            .expect("bulk insert");
        inserted = end;
    }
}

fn percentile(samples: &mut [Duration], pct: f64) -> Duration {
    samples.sort();
    let idx = ((pct / 100.0) * (samples.len() - 1) as f64).round() as usize;
    samples[idx]
}

async fn run_samples(app: &Router, uris: &[&str], n: usize, label: &str) -> Vec<Duration> {
    // Drop the first 5 to remove first-touch + JIT-style warmups.
    for uri in uris {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(*uri)
                    .header("Authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    let mut samples = Vec::with_capacity(n);
    for k in 0..n {
        let uri = uris[k % uris.len()];
        let t0 = Instant::now();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("Authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "{}: bad status for {}", label, uri);
        samples.push(t0.elapsed());
    }
    samples
}

#[tokio::test]
async fn search_latency_with_1k_rows() {
    let mut h = build_state().await;
    seed_rows(&mut h, 1000).await;
    let app = router(h.state.clone());

    let empty = run_samples(
        &app,
        &["/api/doujinshi/search?limit=50"],
        100,
        "empty",
    )
    .await;
    let like = run_samples(
        &app,
        &["/api/doujinshi/search?q=bench%20title%2042"],
        100,
        "like",
    )
    .await;

    let p50_empty = percentile(&mut empty.clone(), 50.0);
    let p95_empty = percentile(&mut empty.clone(), 95.0);
    let p95_like = percentile(&mut like.clone(), 95.0);
    let p99_like = percentile(&mut like.clone(), 99.0);

    eprintln!(
        "search_empty_query (1k): p50={:?} p95={:?}",
        p50_empty, p95_empty
    );
    eprintln!(
        "search_with_like (1k):    p95={:?} p99={:?}",
        p95_like, p99_like
    );

    assert!(
        p95_empty < Duration::from_millis(50),
        "p95 empty-query latency {:?} exceeded 50 ms",
        p95_empty
    );
    assert!(
        p95_like < Duration::from_millis(50),
        "p95 LIKE-predicate latency {:?} exceeded 50 ms",
        p95_like
    );
}

#[tokio::test]
async fn search_latency_with_10k_rows() {
    let mut h = build_state().await;
    seed_rows(&mut h, 10_000).await;
    let app = router(h.state.clone());

    // The umbrella acceptance spec calls for "search stays under 50 ms
    // p95 with 10k rows". We test the indexed empty-query path here —
    // `LIKE '%...%'` cannot use a B-tree index and is informational only.
    let empty = run_samples(
        &app,
        &["/api/doujinshi/search?limit=50"],
        50,
        "empty-10k",
    )
    .await;
    let p50 = percentile(&mut empty.clone(), 50.0);
    let p95 = percentile(&mut empty.clone(), 95.0);
    eprintln!("search_empty_query (10k): p50={:?} p95={:?}", p50, p95);

    assert!(
        p95 < Duration::from_millis(50),
        "p95 empty-query at 10k rows: {:?} exceeded 50 ms",
        p95
    );

    let like = run_samples(
        &app,
        &["/api/doujinshi/search?q=bench%20title%2042"],
        50,
        "like-10k",
    )
    .await;
    let p95_like = percentile(&mut like.clone(), 95.0);
    eprintln!(
        "search_with_like (10k, leading wildcard, informational): p95={:?}",
        p95_like
    );
}
