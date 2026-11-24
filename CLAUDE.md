# CLAUDE.md

Peck：只为站主、只在站主这台 Mac 上跑的个人 Harness（Tauri 2 + React + Rust + SQLite）。站主不是设计师、不是全职开发者，用自然语言指挥 Agent。这里只放核心规则，细则在 `docs/`（分工见「文档」），同一条规则只写在一处。`AGENTS.md` 指向这里。

## 开工前

1. 读本文件、`docs/roadmap.md`、`HANDOFF.md`；定过的不重复问。
2. `git status`、`git log --oneline -3`：有不认识的改动不动、不还原，先问。
3. 用提问功能问站主这次属于哪种程度（见「版本」），答了再动手；站主已经写明版本号就不用再问。
4. 开发中拿不准的决策点也用提问功能问；问了没人答，按 `docs/` 里写的默认做法做，记进 HANDOFF。
5. 改完自己验：前端 `pnpm check`，后端跑改到的模块，界面开 `?mock` 看（命令见 `docs/testing.md`「每次改完」）。Peck 不替 Agent 跑检查。

## 版本

版本号 `x.y.z`，写在 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 和 `Cargo.lock` 里 peck 那一项，四处一起改。

| 程度 | 例子 | 分 Phase | 更新正式版 |
| --- | --- | --- | --- |
| 小幅优化、修 Bug | `1.0.0 → 1.0.1` | 不分，直接做 | 做完自动更新 |
| 新增功能、大幅优化 | `1.0.0 → 1.1.0` | 先在 roadmap 写 Phase 计划，站主看过再做 | 只在最后一轮收尾时自动更新；中间站主自己更新，或在某个 Phase 里要求更新 |
| 大版本 | `1.0.0 → 2.0.0` | 同上；升不升大版本只由站主定 | 同上 |

更新正式版的步骤在 `docs/testing.md`「更新正式版」。

## Git

- 在 `main` 上改。每一轮开发做完自动提交，**push 由站主做**；每轮结束告诉站主这一轮有哪几笔提交（短哈希、说明、一句话做了什么）。
- 说明写 `v<版本>: <description>`，小写英文、简约概括：`v1.0.1: fix some problems`、`v1.1.0: minor improvement`。
- 不署名（不加 `Co-Authored-By`、「Generated with」）；用仓库配好的 `user.name` / `user.email`，不改 git 配置。
- 不 force push、不改已推的历史、不开 PR。tag 站主说打才打，叫 `peck-v<版本>`。

## 边界

- 只在 `personal-cli-studio/` 里开发；`~/Documents/GitHub/pomelo` 只读。
- `src/vendor/peck/` 是 visual-atelier 方案 `peck` 同步来的，不手改；视觉方案要站主批。visual-atelier 里只碰 `schemes/peck/`、`canvases/peck-*/` 和两份索引里 Peck 那一行，守它自己的 `CLAUDE.md`。
- 不接 Google 的 Antigravity（条款风险，v1.1.5 整个去掉了），以后也不加回来。
- CLI 保持原厂设置：不改它们的配置、不替它们点信任，不读、不复用任何 CLI 的登录凭据；Peck 只在外面编排。
- 不碰 Keychain 里的 Key 内容；Key、账号、绝对路径、提示词不进文档和夹具。不新增付费服务；新加依赖写成 `^x.y.z`，在 HANDOFF 记一笔。
- 真跑（花 API 的钱、用 CLI 的订阅额度）只按 `docs/roadmap.md`「现行授权」。
- 没验过的不写「通过」。

## 插件

- 自带插件嵌在程序里（`src-tauri/defaults/plugins/`），插件库的源在 `src-tauri/defaults/library/`；系统级的插件和 Skill 只在开发 Peck 时新建、更新，改了就升它自己的 `version`。跑着的模型（`create_skill`）只写项目里的。
- 插件的命令一律带插件前缀：`/<插件名>:<命令>`（`/git-plan:plan`）。不带前缀的只有 Peck 自己的那几个。
- 装进项目的是项目自己的文件：Peck 不自己改，库里有新版只提示 UPDATE、点了才覆盖。
- 格式、放哪、怎么装进项目见 `docs/architecture.md`「插件」。

## 文档

文档用中文（专有名词保留英文），只写结论，不写过程和流水，不另建阶段总结、历史日志。规则按下表各放一处，别处要用就写「见 …」。

| 文件 | 放什么 |
| --- | --- |
| `CLAUDE.md` | 核心规则 |
| `README.md` | 是什么、模块、怎么装和用 |
| `HANDOFF.md` | 每轮做完更新：只留当前状态、没验的、已知问题、下一步，不追加历史 |
| `docs/roadmap.md` | 定位、当前版本、Phase 表、之后要做的、现行授权和账本 |
| `docs/architecture.md` | 选型、一个任务怎么跑、数据和迁移、Gateway、 Peck Loop 、视觉同步和打包、代码位置 |
| `docs/design.md` | 界面的用词、默认取舍、布局、各处的样子 |
| `docs/testing.md` | 怎么测、更新正式版的步骤、原生和真跑的办法 |
