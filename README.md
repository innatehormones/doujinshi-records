# doujinshi-records

A local Tauri desktop app for managing a personal doujinshi library. The app
watches an inbox directory for new archives, computes hashes, extracts covers,
and tracks each file through identified -> will-delete -> permanently-deleted
stages. A local HTTP API is exposed on 127.0.0.1 so browser extensions and
other tools can query the library.

> Personal use only. Content stays on your machine. This project does not
> distribute or download doujinshi - it only organizes files you already have.

## Features

- Watch `resources/doujinshi/` for new ZIP/RAR archives
- BLAKE3 hash + filename parsing (title / circle / series / translator / version)
- Auto-extract a cover image (~100 KB) and store it locally
- Name+ext collision detection (parked in Inbox for manual decision)
- Two-step delete (Dialog A then Dialog B) to prevent misclicks
- Recycle bin view with Restore and permanent-delete
- Local HTTP API for browser extensions (`/api/health`, `/api/doujinshi/...`)
- Live updates: scanner emits `library-updated` events that refresh the UI

## Tech stack

- Rust + Tauri 2 (backend + window)
- SQLite via SeaORM 1.1
- Axum 0.7 (HTTP API)
- notify-debouncer-full 0.3 (file system watcher)
- BLAKE3 (hashing)
- image 0.25 + zip/rar (cover extraction)
- Vue 3 + TypeScript + Naive UI (frontend)
- Pinia (state), Vue Router

## Project layout

```
doujinshi-records/
  resources/
    doujinshi/          # inbox: drop new archives here
    doujinshi-identified/   # auto-moved here after identification
    doujinshi-will-delete/  # user-marked for deletion
    covers/             # extracted cover images (~100 KB each)
  src/                  # Vue frontend
  src-tauri/
    src/
      commands/         # Tauri commands (frontend <-> backend)
      db/               # SeaORM entities + raw SQL migration
      http/             # Axum router + handlers
      services/         # scanner, identifier, hasher, parser, archive, cover
  docs/superpowers/     # spec + implementation plan
```

## Getting started

### Prerequisites

- Rust 1.77+
- Node.js 20+
- pnpm 9+
- Windows 10/11 (Tauri 2 WebView2 runtime is bundled)

### Install

```bash
pnpm install
```

### Run (development)

```bash
pnpm tauri dev
```

The first run creates `resources/`, applies the SQLite schema, and prints
`http api listening on http://127.0.0.1:<port>` to stdout. The Settings view
shows the actual port.

### Build (release)

```bash
pnpm tauri build
```

## HTTP API

All endpoints are served on `http://127.0.0.1:<random-port>` (see Settings
view for the exact URL). CORS is open for any origin.

| Method | Path | Description |
|---|---|---|
| GET | `/api/health` | Liveness probe |
| GET | `/api/doujinshi/search?q=<query>` | Search by title / circle / filename |
| GET | `/api/doujinshi/by-hash/<hash>` | Look up by BLAKE3 hash |
| GET | `/api/doujinshi/<id>` | Get a single record |
| GET | `/api/covers/<file_id>` | Cover JPEG (~100 KB) |

Example (PowerShell):

```powershell
$port = (Get-Content resources/.api-port)  # see Settings for actual port
Invoke-RestMethod "http://127.0.0.1:$port/api/health"
Invoke-RestMethod "http://127.0.0.1:$port/api/doujinshi/search?q=sample"
```

Use cases for browser extensions:

- "Have I downloaded this doujinshi before?" - search by title or hash.
- "Did I view it and decide to keep/delete?" - check `viewed` and
  `marked_for_delete` fields in the response.

## Data model

See `src-tauri/src/db/migrations.rs` for the canonical schema. The main table
is `doujinshi_file`; supporting tables are `filename_alias`, `conflict`, and
`scan_event`. Settings live in `app_setting`.

## Development notes

- `pnpm.onlyBuiltDependencies` is configured for `esbuild` and `vue-demi`.
  If pnpm complains, run `pnpm install` again or check that the list is
  present in `package.json`.
- The watcher debounces file events with a 2-second window, so a freshly
  dropped archive appears in Library within ~2-3 seconds.
- The scanner only processes `.zip` and `.rar` files in the top level of
  `resources/doujinshi/`. Subdirectories are ignored.
- Database is stored at `<resources>/doujinshi.db` (created on first run).

## Spec and plan

- Design spec: `docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- Implementation plan: `docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
