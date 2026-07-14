# V1.x Large-Library Performance Results

Measured by `cargo test --offline --test perf -- --nocapture` after sub-plan 4 indices were added. Debug-mode Rust (no codegen opts); Windows + NVMe; in-process axum Router via `tower::ServiceExt::oneshot` (no real socket, no CORS round trip).

> **v6 后续**：(physically_deleted, created_at) 索引在 2026-07-14 提交 `8e4e248` 后被替换为 `(current_location, created_at)`。本文件记录的 search / count 性能特征仍然成立（同样的 compound 索引、同样的查询形态），但 `physically_deleted` 这一列本身已被砍——"已删"语义改为通过 `current_location='permanently_deleted'` 表达。

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
