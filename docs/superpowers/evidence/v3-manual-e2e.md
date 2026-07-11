# V3 手动 E2E 矩阵

> V3 plan §15 step 3 的 GUI 验证清单。V3.0 全部 13 个 commit 推送后，沙箱无桌面环境，需在本地 `pnpm tauri dev` 实跑。

## 起步

启动前 `resources/` 应为空。第一次运行会从空 schema 通过 `init_schema_versioned` 自动建立 v5 schema，所以新 `data.db` 不需要手动初始化。

测试样本准备（任选其一）：

- 复制任意 zip，改名丢进 `resources/doujinshi/`，文件名建议含 `[社团] 标题` 格式让 `filename_parser` 有戏：
  - `resources/doujinshi/[TestCircle] TestTitle1.zip`
  - `resources/doujinshi/[TestCircle] TestTitle2.zip`
- 也可以用 PowerShell 临时生成空 zip：
  ```powershell
  "[Content_Types].xml" | Compress-Archive -DestinationPath resources/doujinshi/test1.zip
  ```

防抖窗口 2 秒，新文件 ~3s 内出现在 Library。

## 矩阵

| # | 场景 | 前置 | 操作 | 预期 | 异常信号 |
|---|---|---|---|---|---|
| E1 | 启动建库 | 空 resources | `pnpm tauri dev` | 终端打印 `http api listening on http://127.0.0.1:<port>`；前端 Library 页文案「还没有文件...」；左侧菜单显示「脏数据」入口；Settings 页显示真实端口与 token | 启动崩溃 / 终端没端口 / 菜单缺项 |
| E2 | 新 zip 入库 | E1 之后 | 拖一个测试 zip 进 `resources/doujinshi/` | ~3s 后 Library 出现卡片（封面应为 webp 图）；`resources/doujinshi-identified/` 出现对应 zip；`resources/covers/<hash>.webp` 多一个文件 | 卡片不来 / 封面 404 / zip 没移动 |
| E3 | 归档操作 | E2 之后 | Library 卡片点「归档」 | zip 移到 `doujinshi-archived/`；Library 筛选改「归档」能看到；「全部」视图筛掉该卡片；DB `current_location = 'archived'`；HTTP 状态 200 | 文件还在 identified/；按钮无反应；后端报错 |
| E4 | 移到回收站 | E2 之后 | 卡片点「移到回收站」 | zip 移到 `doujinshi-will-delete/`；Library 「回收站」筛选可见；DB location 切到 will_delete | 文件还在；location 标签未切 |
| E5 | 回收站取回 | E4 之后 | RecycleBin 页点「取回」 | zip 移回 `doujinshi-identified/`；Library 全部 / 已入库可见 | 文件还在 will-delete；location 没切 |
| E6 | 彻底清理 | E4 之后 | RecycleBin 页点「彻底清理」 | zip 从 will-delete/ 删；DB 行 `has_physical_file = 0`；卡片显示「文件丢失」红色「!」角标 | 文件没删；角标没出 |
| E7 | 启动脏数据扫描 | E6 之后 | 重启 Tauri；手动拷一个任意 zip 进 `doujinshi-archived/`；再重启 | 二次启动后 `/dirty` 页能看到该孤儿（`detected_dir = archived`，`reason` 表明"DB 无对应行"） | 重启后 /dirty 为空；或者扫描导致崩溃 |
| E8 | 同 hash 复活 | E6 之后 | 把 `doujinshi-archived/` 那个 zip 复制到 `doujinshi/`（文件名任意） | ~3s 后 Library 看到的是**同一行**（id 复用、`current_location = identified`、hash 一致）；旧的 `has_physical_file` 翻回 1 | 新建了另一行（id 重复）；location 卡在 will_delete |
| E9 | HTTP 端点 | 任意 | `curl -H "Authorization: Bearer <token>" http://127.0.0.1:<port>/api/doujinshi/search?q=Test` + `/api/dirty` | search 返回 JSON 数组；dirty 返回数组；不带 token 应返回 401 | 应有端点 404；鉴权明明有 token 还 401 |

## 通过标准

- 9 行全 ✓ 才算 V3 上线
- 任意一格 × → 记下：
  - 哪一步（包括 E#）
  - 实际看到什么（截图 / 终端日志 / 文件夹内容）
  - 触发操作（如有歧义）
- 上报后挂起代码工作，等定位修复方案

## 已知不在矩阵里

- **跨设备 rename 兜底**：需要把 `doujinshi-identified/` 挂成网络盘才能触发；本机 SSD 单分区基本走不到 `EXDEV`，跳过
- **封面格式兜底**：仅有 webp 编码路径，jpg 输入若出现会失败；当前 scanner 只接受 zip/rar，jpg 不是合法 inbox 输入，跳过
- **V3.1**：LRU preview cache + gallery detail page 在 V3 scope 外，不在本矩阵
