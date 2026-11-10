#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/Library/pnpm/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
for d in "$HOME"/.nvm/versions/node/v24*/bin; do
  [ -d "$d" ] && export PATH="$d:$PATH"
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MODE="${1:---build-only}"
PROFILE="${PECK_BUILD_PROFILE:-release}"
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-Peck Self-Signed}"

case "$MODE" in
  --check|--build-only|--install) ;;
  *) echo "用法: $0 [--check|--build-only|--install]" >&2; exit 2 ;;
esac
case "$PROFILE" in
  debug|release) ;;
  *) echo "PECK_BUILD_PROFILE 仅支持 debug 或 release" >&2; exit 2 ;;
esac

for tool in pnpm cargo codesign ditto; do
  command -v "$tool" >/dev/null || { echo "缺少工具: $tool" >&2; exit 1; }
done
if [ "$APPLE_SIGNING_IDENTITY" != "-" ]; then
  if ! security find-identity -v -p codesigning | grep -Fq -- "$APPLE_SIGNING_IDENTITY"; then
    echo "找不到受信任的代码签名身份: $APPLE_SIGNING_IDENTITY；请检查证书、私钥和代码签名信任。" >&2
    exit 1
  fi
fi

echo "[Peck] 构建类型: ${PROFILE}; 签名: ${APPLE_SIGNING_IDENTITY}（- 表示本机 ad-hoc）"
if [ "$MODE" = "--check" ]; then
  echo "[Peck] 工具与签名配置检查通过；未构建、安装或清理文件。"
  exit 0
fi

cd "$PROJECT_ROOT"
BUILD_ARGS=(build --bundles app)
if [ "$PROFILE" = "debug" ]; then BUILD_ARGS+=(--debug); fi
pnpm tauri "${BUILD_ARGS[@]}"
BUNDLE_PATH="$PROJECT_ROOT/src-tauri/target/$PROFILE/bundle/macos/Peck.app"
[ -d "$BUNDLE_PATH" ] || { echo "未找到构建产物: $BUNDLE_PATH" >&2; exit 1; }
codesign --verify --deep --strict "$BUNDLE_PATH"
echo "[Peck] 已生成并验证: $BUNDLE_PATH"
if [ "$MODE" = "--build-only" ]; then exit 0; fi

INSTALL_DIR="${PECK_INSTALL_DIR:-/Applications}"
mkdir -p "$INSTALL_DIR"
STAGING_DIR="$(mktemp -d "$INSTALL_DIR/.peck-install.XXXXXX")"
trap 'rm -rf "$STAGING_DIR"' EXIT
ditto "$BUNDLE_PATH" "$STAGING_DIR/Peck.app"
codesign --verify --deep --strict "$STAGING_DIR/Peck.app"
if [ -e "$INSTALL_DIR/Peck.app" ]; then
  BACKUP_PATH="$INSTALL_DIR/Peck.app.backup-$(date +%Y%m%d-%H%M%S)"
  mv "$INSTALL_DIR/Peck.app" "$BACKUP_PATH"
  echo "[Peck] 原版本已保留: $BACKUP_PATH"
fi
mv "$STAGING_DIR/Peck.app" "$INSTALL_DIR/Peck.app"
echo "[Peck] 已安装: $INSTALL_DIR/Peck.app"
