#!/bin/sh
# Git Plan 的收尾检查：项目里有 HANDOFF.md、这一轮改了别的文件、HANDOFF.md 却没改，就用退出码 2 让模型先更新它。
# Peck 在 stdin 的 JSON 里给 peck_changed_files（这一轮改了的文件，相对项目根）；在项目根跑。
# 一回合只提醒一次（stop_hook_active 为真时放行），读不懂、没有 jq 都放行。
[ -f HANDOFF.md ] || exit 0
command -v jq >/dev/null 2>&1 || exit 0
input=$(cat)
[ "$(printf '%s' "$input" | jq -r '.stop_hook_active // false' 2>/dev/null)" = "true" ] && exit 0
files=$(printf '%s' "$input" | jq -r '.peck_changed_files // [] | .[]' 2>/dev/null) || exit 0
[ -n "$files" ] || exit 0
printf '%s\n' "$files" | grep -qx 'HANDOFF.md' && exit 0
echo "这一轮改了文件，但 HANDOFF.md 没更新。先照项目的规矩更新 HANDOFF.md（当前状态、没验的、已知问题、下一步），再收尾；这一轮确实不用更新的话，说明原因再收尾。" >&2
exit 2
