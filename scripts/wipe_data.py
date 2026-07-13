#!/usr/bin/env python3
"""
清空 doujinshi-records 运行时数据。

用法（在仓库根目录）：
    python scripts/wipe_data.py
    python scripts/wipe_data.py --yes         # 跳过确认
    python scripts/wipe_data.py --include-files  # 额外删除文件目录

默认行为（--include-files=False）：只清
    data.db（SQLite 主库）

加 --include-files 额外清：
    covers/、_preview_cache/、doujinshi/、doujinshi-identified/、
    doujinshi-will-delete/、doujinshi-archived/
    —— 等于完整重置到首次安装状态。

注意：
- 应用必须先关掉，否则 SQLite 文件锁住删不掉（Windows 上）。
- 默认 dry run 会先列出要删的东西，等用户 y 才执行。
"""

import argparse
import shutil
import sys
from pathlib import Path

# 项目根 = 脚本所在目录的上两级（scripts/ → 根）
ROOT = Path(__file__).resolve().parent.parent
RESOURCES = ROOT / "resources"

DB = RESOURCES / "data.db"
FILE_DIRS = [
    RESOURCES / "covers",
    RESOURCES / "_preview_cache",
    RESOURCES / "doujinshi",
    RESOURCES / "doujinshi-identified",
    RESOURCES / "doujinshi-will-delete",
    RESOURCES / "doujinshi-archived",
]


def list_targets(include_files: bool) -> list[Path]:
    targets: list[Path] = []
    if DB.exists():
        targets.append(DB)
    if include_files:
        for d in FILE_DIRS:
            if d.exists():
                targets.append(d)
    return targets


def is_app_running() -> bool:
    """检测 doujinshi-records.exe 是否在跑。Windows-only。"""
    if sys.platform != "win32":
        return False
    try:
        import subprocess  # noqa: PLC0415

        out = subprocess.check_output(
            ["tasklist", "/FI", "IMAGENAME eq doujinshi-records.exe"],
            text=True,
            stderr=subprocess.DEVNULL,
        )
        return "doujinshi-records.exe" in out
    except Exception:
        return False


def main() -> int:
    parser = argparse.ArgumentParser(description="清空 doujinshi-records 运行时数据。")
    parser.add_argument(
        "--include-files",
        action="store_true",
        help="额外删除文件目录（covers、缓存、4 个文件状态目录）。",
    )
    parser.add_argument(
        "--yes",
        "-y",
        action="store_true",
        help="跳过确认直接执行。",
    )
    args = parser.parse_args()

    if not RESOURCES.exists():
        print(f"resources/ 不存在：{RESOURCES}", file=sys.stderr)
        return 1

    targets = list_targets(args.include_files)
    if not targets:
        print("没有要清的东西（resources/ 已是空的或内容不在）。")
        return 0

    print("即将删除：")
    for t in targets:
        size = sum(p.stat().st_size for p in t.rglob("*") if p.is_file())
        kind = "目录" if t.is_dir() else "文件"
        print(f"  [{kind}] {t.relative_to(ROOT)}  ({_fmt_size(size)})")

    if is_app_running():
        print(
            "\n⚠️  检测到 doujinshi-records.exe 正在运行。\n"
            "   SQLite 文件可能被锁，先关掉应用再重跑。",
            file=sys.stderr,
        )
        return 2

    if not args.yes:
        print("\n这会丢全部入库数据，确认？(y/N) ", end="", flush=True)
        ans = input().strip().lower()
        if ans not in ("y", "yes"):
            print("已取消。")
            return 0

    for t in targets:
        if t.is_dir():
            shutil.rmtree(t)
        else:
            t.unlink()
        print(f"  ✓ 删 {t.relative_to(ROOT)}")

    print(f"\n清空完成。下次启动应用会自动建空 schema。")
    return 0


def _fmt_size(n: int) -> str:
    if n < 1024:
        return f"{n} B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f} KB"
    if n < 1024 * 1024 * 1024:
        return f"{n / 1024 / 1024:.1f} MB"
    return f"{n / 1024 / 1024 / 1024:.2f} GB"


if __name__ == "__main__":
    sys.exit(main())