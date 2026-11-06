#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TARGET_DIR="$PROJECT_ROOT/src-tauri/target"
MAX_GB=20

case "${1:-}" in
  ""|--force|--check) ;;
  *) echo "用法: $0 [--check|--force]" >&2; exit 2 ;;
esac

if [ ! -d "$TARGET_DIR" ]; then
  echo "[Peck] src-tauri/target 目录不存在，无需清理。"
  exit 0
fi

SIZE_KB=$(du -sk "$TARGET_DIR" | awk '{print $1}')
SIZE_GB=$((SIZE_KB / 1024 / 1024))

echo "[Peck] 当前 src-tauri/target 大小约为 ${SIZE_GB}GB (阈值: ${MAX_GB}GB)"
if [ "${1:-}" = "--check" ]; then
  echo "[Peck] 仅检查大小，未清理文件。"
  exit 0
fi

if [ "$SIZE_GB" -ge "$MAX_GB" ] || [ "${1:-}" = "--force" ]; then
  echo "[Peck] 正在清理 Rust 编译缓存..."
  rm -rf "$TARGET_DIR"
  echo "[Peck] 清理完成。"
else
  echo "[Peck] 未超过阈值，跳过清理（传入 --force 可强制清理）。"
fi
