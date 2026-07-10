# V1.x Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the eight V1.x / V2 candidate gaps that the V1 plan hygiene audit (Task 27 step 10 backend verification) identified, in priority order, without regressing the eight spec acceptance criteria.

**Architecture:** Each candidate is an independent subsystem with its own bite-sized sub-plan. Sub-plans live alongside this file in the same directory. Shared dependencies (temp DB helper, integration-test harness) are introduced by the first sub-plan that needs them; later sub-plans import rather than redefine.

**Tech Stack:** Tauri 2, Rust (sea-orm + axum 0.7), Vue 3 + Pinia + Naive UI, SQLite, GitHub Actions (later).

---

## Candidate summary

| # | Candidate | Sub-plan | Priority | Depends on | Effort | Risk |
|---|---|---|---|---|---|---|
| 1 | HTTP integration test (cover route + search + health + 404 paths) | `2026-07-10-v1x-http-integration-test.md` | High | none | M | Low |
| 2 | Error-recovery paths (DB corrupt -> backup, missing resources/, cover 404 fallback) | `2026-07-10-v1x-error-recovery.md` | High | #1 | M | Med |
| 3 | GUI smoke pass (sidebar grid, inbox conflict card, recycle two-zone, settings api_url) | `2026-07-10-v1x-gui-smoke.md` | High | none | S | Low |
| 4 | Large-library performance (1k+ files; SQL indices; identify throughput) | `2026-07-10-v1x-perf-large-library.md` | Med | #5 | M | Med |
| 5 | Versioned schema migrations (introduce `schema_version` table; add a sample V1.1 column) | `2026-07-10-v1x-schema-migrations.md` | Med | none | M | Med |
| 6 | i18n audit (settings page, context menus, toast fallbacks) | `2026-07-10-v1x-i18n-audit.md` | Low | none | S | Low |
| 7 | Plan hygiene (mark implemented steps in 2026-07-09 plan) | `2026-07-10-v1x-plan-hygiene.md` | Low | none | XS | None |
| 8 | CI (lint + test + build matrix for windows-latest) | `2026-07-10-v1x-ci.md` | Low | #1, #2 | M | Low |

Recommended execution order: **#1 -> #3 -> #2 -> #5 -> #4 -> #6 -> #7 -> #8**. #3 can run in parallel with #1 since they touch disjoint subsystems (frontend view-only smoke vs backend http harness).

---

## Acceptance criteria (post-rollout)

Rollout is complete when **all** of these are true:

- [ ] `cargo test --offline --lib` passes 7/7 (existing) AND `cargo test --offline --test '*'` passes all new integration tests.
- [ ] `pnpm tauri dev` was launched at least once during this rollout; the four V1 views render without console errors; a screenshot or curl-driven proof of `api_url` in SettingsView is committed under `docs/superpowers/evidence/`.
- [ ] `resources/schema_version` table exists; row `version=1`; up/down migrations are unit-tested.
- [ ] Stress run with 1000 fake files: identify throughput >= 5 files/sec end-to-end on the same Windows host; SQL search stays under 50 ms p95 with 10k rows.
- [ ] `grep -RIn -E "[A-Z][a-z]+( [A-Z][a-z]+){1,}" src/ --include="*.vue"` returns only English strings already justified in code comments (i.e. no UI-facing English string remains).
- [ ] 2026-07-09 plan's 107 unimplemented checkboxes have been re-evaluated; new checkboxes either filled or noted as future-V1.x work in this umbrella.
- [ ] A `.github/workflows/ci.yml` runs `cargo test --offline` + `pnpm install && pnpm lint && pnpm build` on windows-latest and is green.

---

## Out of scope

- V1.1+ features beyond the eight candidates (multi-user, cloud sync, cover preview animations, etc.)
- Migrating `tempfile` dev-dep to a vendored fixture loader (deferred unless a perf test demands it).
- Replacing axum 0.7 / tower-http 0.6 (already on the latest minor; defer major bumps to a separate plan).
