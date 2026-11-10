# 当前交接

更新：2026-11-10 17:50（站主开的会话，本地账号）。**Phase 1 完成；Phase 2 只差站主在 visual-atelier 打 tag `peck-v0.1.0`、本地账号同步。v0.4 起由云端账号开发，开工前要先做完下面「下一步」1–4。** /Applications/Peck.app 还是 11-08 装的 `0.2.0-alpha.4`，代码里的版本号是 `0.3.0`（v0.3 不打包、不出验收页）。

规则在 [CLAUDE.md](./CLAUDE.md)（「两个账号」「云端的边界」「云端开发」「本地验收」），Phase 和状态在 [docs/roadmap.md](./docs/roadmap.md)。

## 现在的状态

- **Phase 2 视觉基线**：四步站主都验收了（11-10 认了 Design System「Peck」，说菜单栏模板图换成新的、开始收尾）。方案 `peck` 0.1.0（approved）和画布导入了 visual-atelier。roadmap 里 Phase 2 仍是 `等站主批`（等 tag 和同步），云端看到它会停在这里，不会提前开 Phase 3。
- **v0.4 起的分工**：另一个 Claude 账号在 Claude Code 网页版、`claude/peck-v0.4` 分支上做 Phase 3、4；只推自己的分支、不建任何云端页面；做完一个版本停下等本地验收。合并进 `main`、打 tag 由站主做。

## 这次做了什么

- **菜单栏模板图**换成 14 格的鸟（和 16 像素 App 图标同一只，一格 1pt，@1x、@2x 都整像素），状态和鸟隔 3 格。云端 Design System「Peck」重传了 6 个资产、改了说明（版本 `1794302976-fdb8`）；画布 Peck Explore 的「图标」板跟着换，加了第一版 / 换上的对照（版本 `1794303553-b12d`）。
- **Design System 收尾**：方案和 12 个资产改 approved，DESIGN.md「待确认」清空，A3 收进方案的 `marks/raster/`；画布简报记了收尾这一轮。
- **导入 visual-atelier**（都没提交）：
  - `schemes/peck/`：画布第 4 步生成器出的全部文件 + `CLAUDE.md`、`AGENTS.md`、外框规则 `peck-rules.md`、`sync.json`（59 个文件同步进 `src/vendor/peck/`）、两个 OFL 字体和许可原文。
  - `canvases/peck-explore/`：生成器、55 张画板、附件 `blobs/`、README；`canvases/peck-refactor-questionnaire/` 的答案和结论放回去了。
  - 站主同意碰的：`tooling/design-system/`（build:ds 认 themes、fonts、scaleGroups，没圆角不出 radius；check:ds 认随包的字体）、`tooling/sync/sync.mjs`（方案自己的说明、字体、Design System 产物跟着 scheme.json 的状态）、`docs/licensing.md`（四个字体）、`docs/adoption.md`（Peck 消费方）、`CHANGELOG.md`；两份索引里 Peck 那一行。
- **Peck 文档**：CLAUDE.md 加了两个账号的分工和云端的边界；roadmap 精简（Phase 表里写明哪些验收点在本地）；`docs/design.md` 补上「样子」；architecture、testing、acceptance、README 跟着改。
- `src/index.css`：旧组件的变量映射改成占位 token 和正式 token 的名字都认，同步后旧界面不会掉色。
- Peck 里的暂存 `design/visual-atelier/` 已导入，按站主说的移到了废纸篓；`.gitignore` 里它的两行删了。

## 实际验证了什么

- 画布：71 个源文件从云端拉回，逐个 sha256 一致；第 1–4 步生成器在本地重新生成的画板、方案、Design System 和云端逐字节一致（Design System 的索引只差云端页面重存时的键顺序）。改完发回后 `dw verify` 191/191。
- 「图标」板 `dw check` 通过，截图看过。
- Design System：云端 47 个文件回读和本地一致；visual-atelier 自己的 build:ds 构建出的 47 个文件和它一样。
- visual-atelier：`npm run qa -- --scheme peck` 91 项全过；`npm test` 20 个全过；改 build:ds 前后 Pomelo、Vermilion 构建出的 Design System 逐字节一致；Pomelo 按 `pomelo-v0.5.1` 的同步核对一致；Peck 的同步在临时仓库里打临时 tag 走过 `--dry-run`（62 个文件）。
- Peck：`pnpm check` 通过，前端构建（输出到会话临时目录）通过。没跑 `cargo test`（没改 Rust）。
- 没有真实 CLI、API 调用。

## 下一步

1. **站主**：在 visual-atelier 看一遍改动、提交，打 tag `peck-v0.1.0`（打在含 `schemes/peck/` 和 `tooling/` 改动的那个提交上）。
2. **本地账号**：核对 tag，再 `VA_BROWSER_CHANNEL=chrome npm run sync -- --scheme peck --to peck --tag peck-v0.1.0`（要用系统 Chrome）；Peck 里 `pnpm check`、`?mock` 明暗各看一眼旧界面颜色还在；roadmap 的 Phase 2 改 `完成`、Phase 3 改 `进行中`；更新本文件。
3. **站主**：Peck 仓库提交并推到 GitHub 的 `main`。现在 `main` 只有 initial commit，几十个文件的改动（Phase 1、文档、同步来的视觉）都在本地，云端账号只看得到推上去的。
4. **站主**：用另一个账号在 Claude Code 网页版连上 `elenskye/peck` 开会话，说「照 CLAUDE.md 开始 v0.4」。它做完 v0.4 会停在「v0.4 等本地验收」，那时在本地说「验收 v0.4」。

## 留意

- 画布第 1 步的三张位图（现在的图标原图、A1、A2）云端读不回，本地预览 `S1b-Icon`、`S1c-Icon` 缺图，云端照常；所以画布发回要照它 README 的办法（`dw publish` 会停下）。
- 真实 API 账本：Opus 7/20、DeepSeek 46/100（roadmap「现行授权」），这次没有新调用。
- Claude Code 状态栏脚本的接入没动（备份在 `~/.claude/settings.json.before-peck-usage-20261107-234334`）。
- 额度监测器（`src/quota/`、`src-tauri/src/quota/`）从 v0.5 起由站主用 Peck 做，Agent 不写。
