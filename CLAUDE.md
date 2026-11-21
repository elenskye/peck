# CLAUDE.md

Peck：只为站主、只在站主这台 Mac 上跑的个人 Harness（Tauri 2 + React + Rust + SQLite）。站主不是设计师、不是全职开发者，用自然语言指挥 Agent。规则只写在这一个文件，`AGENTS.md` 指向这里。开发、验证、打包、提交全在本地做。

## 开工前

1. 读本文件、`docs/roadmap.md`（现在哪个版本、哪个 Phase）、`HANDOFF.md`（没验的、已知问题、下一步）。定过的不重复问。
2. `git status`、`git log --oneline -3`。有不认识的改动不动、不还原，先问。
3. **每次开发前先问站主这次属于哪种程度**（见「版本」），用提问功能问；答了再动手。
4. 文档和界面用中文，专有名词保留英文（Muse、Grok、API、Key、Diff、MCP）。

## 版本

版本号 `x.y.z`，从 `1.0.0` 起。

| 程度 | 例子 | 要不要分 Phase |
| --- | --- | --- |
| 小幅优化、修 Bug | `1.0.0 → 1.0.1` | 不用，直接做 |
| 新增功能、大幅优化 | `1.0.0 → 1.1.0` | 要：先在 roadmap 写 Phase 计划（如 1.2.0 Phase 1–5），站主看过再做 |
| 大版本 | `1.0.0 → 2.0.0` | 要；升不升大版本只由站主定 |

版本号写在 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`（和 `Cargo.lock` 里 peck 那一项），四处一起改。

## Git

Claude 替站主做 Git 操作：在 `main` 上改，做完一段就提交、推到 `origin/main`。

- 说明写成 `v<版本>: <description>`，小写英文，**简约、概括**：`v1.0.1: fix some problems`、`v1.1.0: minor improvement` 这样就行，不罗列细节。
- **不署名**：不加 `Co-Authored-By`、「Generated with」之类的尾注；用仓库里配好的 `user.name` / `user.email`，不改 git 配置。
- 不 force push、不改已推的历史、不开 PR。tag 站主说打才打，叫 `peck-v<版本>`。

## 开发

- **分 Phase 的版本**：状态只在 `docs/roadmap.md` 的 Phase 表「状态」列（`未开始`、`进行中`、`等站主批`、`完成`）。只做表里写的，不扩；一个 Phase 做完改状态、提交。
- **不分 Phase 的版本**：直接做，做完提交。
- 做完更新 `HANDOFF.md`：只留当前状态、没验的、已知问题、下一步，不追加历史。
- **收尾**（站主说「更新正式版」，或一个分 Phase 的版本做完）：改版本号 → `pnpm check`、全量 `cargo test`、`?mock` 明暗各看一遍 → `pnpm app:install` 装进 /Applications → 提交。分 Phase 的版本另加：两个只读子代理分头审这一版的改动（Rust、前端各一个），真缺陷修掉；升级测试。没验过的不写「通过」。
- 只有两种事必须停下等站主：视觉方案要批、升大版本。其余小事问了没人答就按「默认取舍」做，记进 HANDOFF。

## 边界

- 只在 `personal-cli-studio/` 里开发。`~/Documents/GitHub/pomelo` 只读。
- visual-atelier（`~/Documents/GitHub/visual-atelier`）：只碰 `schemes/peck/`、`canvases/peck-*/` 和两份索引里 Peck 那一行；它的规矩读它的 `CLAUDE.md`。`src/vendor/peck/` 是从那里同步来的，不手改：改方案 → 站主打 tag → `VA_BROWSER_CHANNEL=chrome npm run sync -- --scheme peck --to peck --tag peck-v<版本>`。
- **Antigravity 只做直接调用**：不放进接力、委派、审查、定时任务、目标模式这类自动化（避开条款风险）。
- **CLI 保持原厂设置**：不改它们的配置、不替它们点信任，Peck 只在外面编排；不读、不复用任何 CLI 的登录凭据。
- 不碰 Keychain 里的 Key 内容，不把 Key、绝对路径、提示词写进文档。不新增付费服务。
- 用户的仓库只读写文件，不动它的 index、commit、stash；任务前快照在应用数据目录的影子仓库里。
- 新加依赖写成 `^x.y.z`，在 HANDOFF 记一笔。

## 额度

- 自检优先 `?mock`（`http://localhost:1430/?mock`）和本地夹具，不发真请求。
- 真跑照 `docs/roadmap.md`「现行授权」：用完先停下问站主，失败不静默重试。一律用这家最便宜、又测得到要测功能的模型，做无关紧要的小任务；测试项目放仓库外的 `personal-cli-studio/projects/`。
- CLI 都装在仓库外的 `personal-cli-studio/cli/`（`cli/bin/` 是统一入口），用站主已登录的订阅。夹具录下来去掉 Key、账号、绝对路径再进仓库。
- 不用「你好」探针测额度，不用模型 turn 查余额。

## 默认取舍

- 先保留功能、再收界面：入口一个不少，旧组件能删就删。
- 界面只留必要的字：设置页只写英文分组名、不放说明文字；文案照 `docs/design.md` 的用词对照，不写开发者口径（Worker、Run、Step、影子、台账）。
- 看不准的视觉细节照 `src/vendor/peck/DESIGN.md`；没写到的取沉静的那一边。
- 要改数据库结构就加迁移、旧库先备份；不改旧记录的含义。
- 任务跑完默认留着，撤销是站主主动点；通知默认开，能关。

## 验证

- 前端：`pnpm check`；改了界面开 `?mock` 看一遍（明暗各一次），截图只放会话临时目录。
- 后端：改到的模块跑 `cargo test --manifest-path src-tauri/Cargo.toml <过滤>`（`PECK_DATA_DIR`、`PECK_SHADOW_ROOT` 指到新的临时目录），带 `set -o pipefail`，不用 `| tail` 吞退出码。
- 原生、升级测试的办法和坑见 `docs/testing.md`。

## 文档

| 文件 | 内容 |
| --- | --- |
| `CLAUDE.md` | 规则 |
| `README.md` | 是什么、怎么用、模块表 |
| `HANDOFF.md` | 当前状态、没验的、已知问题、下一步 |
| `docs/roadmap.md` | 定位、当前版本和 Phase 表、之后要做的、现行授权 |
| `docs/architecture.md` | 选型、一个任务怎么跑、数据存放、Gateway、Peck 循环、代码位置 |
| `docs/design.md` | 用词对照、布局、各处的样子（定值在 `src/vendor/peck/`） |
| `docs/testing.md` | 怎么测 |

文档只写结论，不写过程和流水；旧的阶段总结、历史日志不另建。
