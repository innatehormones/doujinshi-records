# V2 Sub-Plan 4 — CI 加 tauri build job

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`。
> Implements umbrella candidate **#4**。

**Goal:** 在 `.github/workflows/ci.yml` 新增第三个 job `tauri-build`，跑 `pnpm tauri build --bundles nsis`，上传 NSIS 安装包作为 artifact。MSI bundle 留作 V2.x 后续（NSIS 一个先稳）。

**Architecture:** 复用现有 `rust` / `frontend` job 的工具链步骤（Swatinem/rust-cache、pnpm cache），新 job 串起来再跑一次 `pnpm tauri build`。

**Tech Stack:** GitHub Actions `windows-latest`，Tauri 2 工具链。

**依赖：** 独立。可在任何时候塞进 CI。

---

## Task 1: workflow 文件加 tauri-build job

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: 追加第三个 job**

在现有 `frontend` job 之后追加：

```yaml
  tauri-build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri -> target
      - uses: pnpm/action-setup@v4
        with:
          version: 9
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - name: cache pnpm
        uses: actions/cache@v4
        with:
          path: ~/AppData/Local/pnpm/store
          key: pnpm-${{ runner.os }}-${{ hashFiles('pnpm-lock.yaml') }}
      - run: pnpm install --frozen-lockfile
      - run: cd src-tauri && cargo test
      - run: pnpm tauri build --bundles nsis
      - uses: actions/upload-artifact@v4
        with:
          name: tauri-nsis
          path: src-tauri/target/release/bundle/nsis/*.exe
          if-no-files-found: error
```

注意点：
- `cargo test` 必须在 `pnpm tauri build` 之前（避免 release build 时才发现单测失败）
- `--bundles nsis` 限定只出 NSIS 一个（省时间，5–8 min）。MSI 留给 V2.x
- `if-no-files-found: error` 防止 release bundle 没产出来时 artifact 静默成功
- `tauri-nsis` artifact 名固定，方便 release workflow 引用

- [ ] **Step 2: 本地 dry-run 验证 YAML 语法**

```bash
# 用 act 或纯 YAML lint 都行；这里用 yq 简单检查
yq '.jobs.tauri-build.steps | length' .github/workflows/ci.yml
# 期望输出 11
```

- [ ] **Step 3: 提交**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add tauri build (nsis) job"
```

---

## Task 2: 验证 workflow 跑通

- [ ] **Step 1: 推到 main 触发**

```bash
git push origin main
```

- [ ] **Step 2: 在 GitHub Actions 页确认三个 job 全绿**

打开 `https://github.com/innatehormones/doujinshi-records/actions`，看 `rust` / `frontend` / `tauri-build` 三个 job 都成功。

- [ ] **Step 3: 下载 artifact 验证**

在 workflow run 页底部 `Artifacts` 区下载 `tauri-nsis`，解压得到 `.exe`，本地双击能开安装向导（不必真装）。

---

## Task 3: 已知失败模式 + 修复

- [ ] **pnpm 11 cache miss**：CI 第一次跑会去 download esbuild 等 binary 包（~30 s），后续有 cache 就快了。
- [ ] **WiX 下载超时**：Tauri build 内部会拉 WiX 3.x。如果 github.com flaky，可能 5+ min 卡住。**当前 bundle 只指定 `nsis`，不会触发 WiX 下载**。如果未来要加 MSI，再处理。
- [ ] **`actions/upload-artifact` 报路径不存在**：确认 `src-tauri/target/release/bundle/nsis/*.exe` 实际有产出。可以用 `ls` 在 step 里 echo 调试。
- [ ] **`cargo test` 在 release build 前失败**：把 `cargo test` 放在 `pnpm tauri build` 之前的 step（Task 1 已经这么排）。

---

## Task 4: 回归

- [ ] 跑完后 `pnpm tauri dev` 本地启动正常（确保 workflow 没改 `tauri.conf.json` 等）

---

## Self-review

- [ ] workflow 文件 lint 过（GitHub Actions 页右上角无报错提示）
- [ ] 三个 job 都跑过且成功
- [ ] `tauri-nsis` artifact 可下载、文件大小 > 3 MB（NSIS 自解压大约 5 MB 左右）
- [ ] 总耗时 < 15 min（cold cache），< 8 min（warm cache）
- [ ] 没改 `tauri.conf.json` 或 `package.json`（避免污染其他 plan）