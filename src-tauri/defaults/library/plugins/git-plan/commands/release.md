---
description: 把项目里写版本号的几处一起改掉
argument-hint: <x.y.z，或 patch / minor>
---
按 git-plan 这个 Skill 的规矩改这个项目的版本号。

目标：$ARGUMENTS

1. 找出版本号写在哪几处（`package.json`、`Cargo.toml` 和它的 lock、`tauri.conf.json`、`pyproject.toml`、`Info.plist` 这类；项目说明文件里写了就照它的），说出现在的版本。
2. 目标写的是 `patch` 就升 z，`minor` 就升 y、z 归零；写的是具体版本号就用它；什么都没写就问站主。升 x 只有站主明说才做。
3. 几处一起改成新版本，不动别的内容。
4. 列出改了哪几处、从多少到多少。不要 push、不要打 tag。
