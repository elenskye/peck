# 当前交接

更新：2026-11-12（本地账号）。**v0.5 进行中，本地做好了 Phase 5 的「契约」**：站主填完「[Peck Harness 问卷](https://claude.ai/artifact/1tXsLfFDZmfBB6WMCuVhF3)」，计划重写好了——Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，v0.5 的细节在 [docs/v0.5.md](./docs/v0.5.md)。v0.5 = Phase 5 Model Gateway（**本地开发**）+ Phase 6 工具箱和 Peck 循环（云端开发，等 Phase 5 的契约推到 `main` 才开工）。

规则在 [CLAUDE.md](./CLAUDE.md)（这次改了「两个账号」「云端的边界」「云端开发」，新加「本地开发」，「本地验收」改成先合并再验）。v0.5 两边并行时，本段以下分「本地（Phase 5）」「云端（Phase 6）」两段，各写各的。

## 问卷定了什么（站主 2026-11-12）

- **方向**：Peck 自己的循环为核心，CLI 是可换的引擎；主要借 Pi、Codex、Claude Code Desktop；竖切开发；旧计划重写；额度监测器作业保留、v0.6 起；App 图标换 J2、设置里能切回 A3。
- **Gateway**：API 七家（Anthropic、OpenAI、DeepSeek、阿里云百炼、Gemini 原生、Vertex 上的 Claude 和 Gemini，gcloud ADC）；CLI 五家结构化（Codex、Claude Code 用订阅、Muse、Grok、Antigravity）。**Antigravity 只做直接调用**，不进任何自动化。选模型用模型列表；花费显示在每轮、输入框旁、顶栏 QUOTA。
- **工具箱**：10 个工具（`memory` 放 v0.6）；改文件用「找原文 → 换新文」；PLAN / EDIT / AUTO + 高危卡片；**命令默认联网**，只限制写到项目外；MCP 两边都要。
- **其余**（v0.6–v0.8）：见 roadmap 的 Phase 7–12。
- **流程**：本地照旧不提交、站主提交；云端分支验收通过后本地合进 `main`，tag（`peck-v0.x.0`）和推送站主做；Gateway 本地开发；新授权 `peck-v0.5-2026-11-12`；v0.7 以后用 Peck 开发 Peck。

## 本地（Phase 5）

状态：进行中。**契约（M1）做好了**，等站主提交、推送（见「下一步」）。契约有哪些、怎么用写在 `docs/v0.5.md`「契约的约定」；接下来做 M2（七家 API、五家 CLI 的适配和接入，加上 CLI 多账号）。

这次做了：

- `src-tauri/src/gateway/`：`event.rs`（25 种统一事件）、`caps.rs`（能力标记和六个引擎的初值）、`mod.rs`（`ModelProvider`、`AgentBackend`、控制通道、错误、四个 IPC）、`catalog.rs`（模型目录、价格和花费、模型列表）、`config.rs`（Vertex 的 Project ID / Location、CLI 目录、ADC 在不在）、`log.rs`（事件日志和 `EventSink`）、`mock.rs`（假模型剧本、夹具回放引擎）、`convert/`（Codex app-server、Claude Code stream-json、Anthropic SSE 三个转换器和 unified diff 拆分；转换器出的是「事件 + 可选的子任务 id」）。
- 迁移 `0007_gateway.sql`（schema 7）；`src/lib/agentEvents.ts`；`ipc.ts` 加了 4 个调用（只追加）；`src/dev/gatewayMock.ts`、`mockIpc.ts` 加了 Gateway 的假数据。
- 设置：「供应商和 Key」下面多了 VERTEX AI（Project ID、Location、gcloud 登录状态）；「CLI 检测」第一行多了「CLI 目录」。这两处界面是站主点名要的，先由本地做了，Phase 6 可以重新摆。
- `defaults/models.json`（models.dev 2026-11-12 的快照，7 家 22 个模型、5 个引擎，脚本 `scripts/models-snapshot.mjs`）；`src-tauri/icons/` 的 J2、A3（脚本 `scripts/app-icons.sh`），打包图标换成 J2。
- CLI 统一装进仓库根目录的 `cli/`（Claude Code 2.1.322、Codex 0.167.2、Antigravity 1.5.6、gcloud 593.0.0；`cli/bin/` 统一入口，`cli/README.md`）。Claude Code、agy、gcloud ADC 都登好了；Gemini Key 导进了站主的 Key 目录（新格式 `AQ.`，还没发请求验过）。
- 查到 Claude Code 的审批能直接走 stdio（`--permission-prompt-tool stdio`），不用为它起 MCP；查到 `CLAUDE_CONFIG_DIR`、`CODEX_HOME` 能把账号隔开，Codex 的默认位置和 ChatGPT 客户端共用登录。计划里加了「CLI 账号」（`docs/v0.5.md`）和验收点 9；模型列表每项多了 `account`。
- 规则（CLAUDE.md）：「提交规范」（`main` 上只有站主的提交，说明写成 `v0.x: 小写英文`）；本地验收改成 `git merge --squash`、站主提交；云端看「上一版分支还在不在」判断合没合；「额度」加了省额度和测试项目放 `personal-cli-studio/projects/`。
- v0.4 那三次 Claude 署名的提交：本地另起了 `history-fix` 分支，合成一次站主名下的提交，后两次原样接上，文件和原来一字不差；`main`、tag 没动，换上去由站主做（「下一步」第 1 条）。

## 云端（Phase 6）

状态：进行中（roadmap 已改），`main` 上有契约就开工：从最新的 `origin/main` 切 `claude/peck-v0.5`，先读 `docs/v0.5.md` 的「文件归属」和「契约的约定」，只改云端那一栏的文件；提交说明照 CLAUDE.md「提交规范」。

## 实际验证了什么

- 自动测试：`cargo test --lib gateway` 30 个全过（事件序列化、TS 对照、回放规则、价格和分档、Vertex 地址、设置校验、事件日志续号、读不懂的行变 raw、假模型剧本和取消、回放引擎的审批 / 拒绝 / 打断、Codex、Claude Code、Anthropic 三份夹具、Claude Code 的审批和子任务）；全量 `cargo test` 108 个全过（含 schema 6 → 7 的升级和备份测试）；`pnpm check` 过。
- `?mock`（浏览器）：设置里填错 Project ID 报中文错、填对能存；「CLI 目录」显示；模型列表 30 项（三组、只有 Antigravity 标直接调用）、Codex 夹具 46 条事件都读得到。没看暗色。
- 真实调用（授权 `peck-v0.5-2026-11-12`）：Anthropic `claude-haiku-5-5` 4 次（第一对 `medium` 没出思考，换 `high` 重录一对）；Codex 1 个小任务（读、追加一行、跑一条命令，3 次审批都批准，过程正常）；Claude Code 1 个小任务（Haiku，同样的事，在 `projects/gateway-smoke`；`acceptEdits` 下没弹审批，stdio 审批这条路 M2 再验）。
- 原生 App：没跑；打包图标换成 J2 要下次装包才看得到。

## 退回项

（没有）

## 下一步

1. **站主**：照本地在对话里给的命令换上改好的 v0.4 历史、提交契约、推送、删掉 `claude/peck-v0.4`（这份 HANDOFF 跟着那次提交进 `main`，云端读到时这一步已经做完）。之后 `main` 上只有站主的提交。
2. **本地**：做 Phase 5 的 M2——七家 API 的适配、五家 CLI 的接入（审批、用量、额度）、CLI 多账号；真跑照 CLAUDE.md「额度」用便宜模型。
3. **云端**：照上面「云端（Phase 6）」开工。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 旧授权的累计留作记录：Opus 7/20、DeepSeek 46/100，不再新增。
- 额度监测器（`src/quota/`、`src-tauri/src/quota/`）Agent 不写；v0.6 起站主用 Peck 开发。
- v0.4 留给站主看的三条（新包里点关闭和 Dock 叫回、撤销动效、提示条的角）站主没退回，当作看过了。
- `src-tauri/icons/peck.icns`、`peck.png` 是旧的 A3 原图，打包已经不用了（换成 `j2.*`，A3 用 `a3.*`），站主同意就删。
- 视觉基线要补的（本地改方案时做）：`bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；`DESIGN.md` 菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式。v0.7 要画 ⓘ 图标。
