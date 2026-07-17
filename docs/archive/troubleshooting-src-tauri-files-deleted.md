# src-tauri/ 根 4 文件反复被删

## 现象

启动 `pnpm tauri dev` 或 `cargo test` 时报：

```
error: could not find `Cargo.toml` in `...src-tauri` or any parent directory
```

或

```
unable to read Tauri config file at .../tauri.conf.json because entity not found
```

`git status` 看到 4 个文件 `D`：

- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/build.rs`
- `src-tauri/tauri.conf.json`

仅这 4 个会消失，`src-tauri/src/`、`src-tauri/tests/` 子目录的 .rs 文件不受影响。

## 立刻恢复

```bash
git checkout HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs src-tauri/tauri.conf.json
```

## 根因

**未定位**。2026-07-14 排查 4 个 session 的完整 JSONL log（共约 30MB），找不到任何 `rm` / `git rm` / `Remove-Item` 直接删这 4 个文件的命令。git reflog 干净，`git log --diff-filter=D` 也无记录。

删除源在 session 外部。最可能：

1. Windows Defender / 第三方杀毒启发式隔离
2. `D:\NewCode` 若在 OneDrive 路径下，同步冲突丢本地
3. IDE / 编辑器 cleanup 插件

## 已观察规律

- 多次发生在 `cargo test` 长跑后（`bash.exe.stackdump` 同时出现）
- 2026-07-11 至今至少 11 次跨 session 复发
- 2026-07-15 V4 实施期再次复发（cargo test 长跑 + cargo clean 触发的两次 `git checkout --` 恢复）

## 长期方案

待定。