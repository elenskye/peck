#!/bin/bash
# App 图标：J2（默认）和 A3 两套，从 src/vendor/peck/marks/ 出，放进 src-tauri/icons/。
# 视觉换了版本再跑一次：bash scripts/app-icons.sh（要本机装 Google Chrome，用它无头渲染 SVG）。
#
# J2 的 .icns 10 张按尺寸取底稿，不从一张 1024 缩（src/vendor/peck/DESIGN.md「App 图标 J2」）：
#   16 → j2-16.svg；16@2x、32 → j2-32.svg；32@2x → j2-64.svg；128 → j2-128.svg；256 以上 → j2.svg。
# A3 是位图（marks/raster/a3.png，1024），只出 Dock 用的 128 以上。
# 出来的：j2.icns、j2.png（1024，打包和 Dock 用）、a3.icns、a3.png（1024，设置里切回 A3 时 Dock 用）。
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
marks="$root/src/vendor/peck/marks"
out="$root/src-tauri/icons"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

[ -x "$chrome" ] || { echo "要先装 Google Chrome（用来渲染 SVG）" >&2; exit 1; }

# render <svg> <px> <png>：按原尺寸渲染，透明底。
render() {
  local html="$work/render-$2.html"
  printf '<!doctype html><html><body style="margin:0;background:transparent"><img src="file://%s" width="%s" height="%s" style="display:block"></body></html>' "$1" "$2" "$2" > "$html"
  "$chrome" --headless --disable-gpu --hide-scrollbars --default-background-color=00000000 \
    --force-device-scale-factor=1 --window-size="$2,$2" --screenshot="$3" "file://$html" >/dev/null 2>&1
  [ -f "$3" ] || { echo "渲染失败：$1 @ $2" >&2; exit 1; }
}

j2="$work/j2.iconset"
mkdir -p "$j2"
render "$marks/app-icon/j2-16.svg" 16 "$j2/icon_16x16.png"
render "$marks/app-icon/j2-32.svg" 32 "$j2/icon_16x16@2x.png"
render "$marks/app-icon/j2-32.svg" 32 "$j2/icon_32x32.png"
render "$marks/app-icon/j2-64.svg" 64 "$j2/icon_32x32@2x.png"
render "$marks/app-icon/j2-128.svg" 128 "$j2/icon_128x128.png"
render "$marks/app-icon/j2.svg" 256 "$j2/icon_128x128@2x.png"
render "$marks/app-icon/j2.svg" 256 "$j2/icon_256x256.png"
render "$marks/app-icon/j2.svg" 512 "$j2/icon_256x256@2x.png"
render "$marks/app-icon/j2.svg" 512 "$j2/icon_512x512.png"
render "$marks/app-icon/j2.svg" 1024 "$j2/icon_512x512@2x.png"
iconutil -c icns "$j2" -o "$out/j2.icns"
cp "$j2/icon_512x512@2x.png" "$out/j2.png"

a3="$work/a3.iconset"
mkdir -p "$a3"
for spec in 128:icon_128x128 256:icon_128x128@2x 256:icon_256x256 512:icon_256x256@2x 512:icon_512x512 1024:icon_512x512@2x; do
  sips -z "${spec%%:*}" "${spec%%:*}" "$marks/raster/a3.png" --out "$a3/${spec#*:}.png" >/dev/null
done
iconutil -c icns "$a3" -o "$out/a3.icns"
cp "$marks/raster/a3.png" "$out/a3.png"

echo "写好了 src-tauri/icons/：j2.icns、j2.png、a3.icns、a3.png"
