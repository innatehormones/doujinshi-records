# V1.x Large-Library Performance Results

Measured by `cargo test --offline --test perf -- --nocapture` after sub-plan 4 indices were added. Debug-mode Rust (no codegen opts); Windows + NVMe; in-process axum Router via `tower::ServiceExt::oneshot` (no real socket, no CORS round trip).

## 1 000 rows

| Bench | p50 | p95 | p99 |
|---|---|---|---|
| `search_empty_query` (limit=50) | 11.9 ms | 16.1 ms | — |
| `search_with_like` (q=`bench title 42`) | — | 13.9 ms | 15.3 ms |

## 10 000 rows

| Bench | p50 | p95 | Notes |
|---|---|---|---|
| `search_empty_query` (limit=50) | 11.3 ms | 14.7 ms | supported by the new `(physically_deleted, created_at)` compound index |
| `search_with_like` (q=`bench title 42`) | — | 55.3 ms | informational only — `LIKE '%...%'` cannot use a B-tree index |

## Conclusion

`/api/doujinshi/search` well under the 50 ms p95 target at both 1k and 10k rows for the indexed paths. The leading-wildcard LIKE search is bounded by a sequential scan and is expected — adding FTS5 trigrams would help but is a V1.x-y feature, not this rollout.

Hand-written `q.count()` swap (from `q.clone().all().len()`) and the new compound index are what brought the 10k empty-query path from ~200 ms to ~15 ms.
