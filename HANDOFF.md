# 当前交接

更新：2026-11-12（本地账号）。**v0.5 进行中，本地的 Phase 5 做完了**：站主填完「[Peck Harness 问卷](https://claude.ai/artifact/1tXsLfFDZmfBB6WMCuVhF3)」，计划重写好了——Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，v0.5 的细节在 [docs/v0.5.md](./docs/v0.5.md)。v0.5 = Phase 5 Model Gateway（**本地开发**）+ Phase 6 工具箱和 Peck 循环（云端开发，等 Phase 5 的契约推到 `main` 才开工）。

规则在 [CLAUDE.md](./CLAUDE.md)（这次改了「两个账号」「云端的边界」「云端开发」，新加「本地开发」，「本地验收」改成先合并再验）。v0.5 两边并行时，本段以下分「本地（Phase 5）」「云端（Phase 6）」两段，各写各的。

## 问卷定了什么（站主 2026-11-12）

- **方向**：Peck 自己的循环为核心，CLI 是可换的引擎；主要借 Pi、Codex、Claude Code Desktop；竖切开发；旧计划重写；额度监测器作业保留、v0.6 起；App 图标换 J2、设置里能切回 A3。
- **Gateway**：API 七家（Anthropic、OpenAI、DeepSeek、阿里云百炼、Gemini 原生、Vertex 上的 Claude 和 Gemini，gcloud ADC）；CLI 五家结构化（Codex、Claude Code 用订阅、Muse、Grok、Antigravity）。**Antigravity 只做直接调用**，不进任何自动化。选模型用模型列表；花费显示在每轮、输入框旁、顶栏 QUOTA。
- **工具箱**：10 个工具（`memory` 放 v0.6）；改文件用「找原文 → 换新文」；PLAN / EDIT / AUTO + 高危卡片；**命令默认联网**，只限制写到项目外；MCP 两边都要。
- **其余**（v0.6–v0.8）：见 roadmap 的 Phase 7–12。
- **流程**：本地照旧不提交、站主提交；云端分支验收通过后本地合进 `main`，tag（`peck-v0.x.0`）和推送站主做；Gateway 本地开发；新授权 `peck-v0.5-2026-11-12`；v0.7 以后用 Peck 开发 Peck。

## 本地（Phase 5）

状态：**完成**（本地开发做完了，等站主提交、推送；Vertex 两家等站主开项目再真跑）。约定写在 `docs/v0.5.md`「契约的约定」，各家实测结论在「API 七家」「CLI 五家」「CLI 账号」「本机 MCP」「额度数据」。

M1 契约已提交（`v0.5: gateway contract …`）。M2 这次做了（没提交）：

- **API 七家**：`gateway/providers.rs`（`HttpProvider`、`for_model`）+ `convert/{anthropic,responses,chat,gemini}.rs`；四份真实夹具 `api-*-tools`。
- **CLI 五家**：`gateway/engines/{codex,claude_code,muse,acp}.rs` + `convert/{muse,acp}.rs`；Muse 改用会话协议 `muse serve`（审批能交给 Peck，计划里的 `exec --json` 不用了）；Grok、Antigravity 走 ACP。三份新夹具 `muse-small-task`、`grok-small-task`、`antigravity-small-task`。
- **CLI 账号**：`gateway/accounts.rs` + 设置 →「CLI 检测 → CLI 账号」。站主 11-12 定：Codex 暂时用默认账号、和 ChatGPT 客户端共用登录。
- **本机 MCP**：`gateway/mcp.rs`（`ask_user`，只给 Grok、Antigravity）。
- **额度**：引擎报的 `quota.updated` 写进 `usage.rs` 的额度卡片（只加了写入口 `usage_record_gateway_quota`）。
- **Gateway 版发任务**：`gateway/runner.rs`，IPC `gateway_run_start` / `gateway_control`（`ipc.ts`、`mockIpc.ts`、`agentEvents.ts` 只追加）；旧的 `run_dispatch` 没动。
- **版本提示**：模型列表带 `testedVersion`；设置 →「CLI 检测 → 引擎」列出五家，版本和录夹具时不一样标 CHANGED。
- `src/dev/gatewayMock.ts`、`mockIpc.ts`、`SettingsPage.tsx`、`app.css` 都只在末尾追加（CLI 账号和引擎这两块界面计划里归本地）。

**给云端（Phase 6）**：界面发 CLI 引擎的任务用 `ipc.gatewayRunStart`，审批卡片和 ASK 卡片回答用 `ipc.gatewayControl`；Peck 循环要真 API 用 `providers::for_model`，要自己出事件用 `log::EventSink`。`api:` 的发任务入口由云端在 `harness/` 里加（`gateway_run_start` 现在只认 `cli:`，可以照它的样子写一个或扩它，扩的话写进 HANDOFF 让本地合并时看）。

## 云端（Phase 6）

状态：进行中（roadmap 已改），`main` 上有契约就开工：从最新的 `origin/main` 切 `claude/peck-v0.5`，先读 `docs/v0.5.md` 的「文件归属」和「契约的约定」，只改云端那一栏的文件；提交说明照 CLAUDE.md「提交规范」。

## 实际验证了什么

- 自动测试：全量 `cargo test` 154 个全过（连跑三次；另有 6 个真跑用的默认不跑），没有编译警告；`pnpm check` 过。夹具测试覆盖：四家 API「回放拼出的第二次请求和真发的一字不差」；五家 CLI 的录音都能经各自的引擎完整回放（审批从控制通道回答）；Muse 升级审批、Claude Code 的提问、引擎中途退出补收尾；本机 MCP 的令牌、等回答、任务结束作废；Gateway 版发任务从头到尾（占号、一个项目一次一个、3 次审批、任务记录、事件日志、结束后不再收控制）；额度卡片只写默认账号的。
- `?mock`（浏览器）：设置 →「CLI 检测」的引擎和 CLI 账号，明暗各看一遍；`ipc.gatewayRunStart` 回放 Grok（39 条，1 次审批从 `gatewayControl` 批准）、Muse（14 条），`api:` 的被拒；控制台没报错。
- 真实调用（授权 `peck-v0.5-2026-11-12`，账本在 roadmap）：API——OpenAI 4/5、DeepSeek 2/5、百炼 2/5、Gemini 2/5（Anthropic 4/5 是契约时用的）。CLI——Codex 2/3、Claude Code 2/3（第 2 个验了 stdio 审批）、Muse 1/3、Grok 2/3（第 2 个验了 `ask_user` 经 MCP）、Antigravity 1/3（直接调用）。都在 `projects/gateway-smoke`。CLI 账号的登录用临时账号目录真起过一次（拿到链接、浏览器没开、取消）。
- 没验的：原生 App 里没走（新入口还没有界面调用，界面是 Phase 6）；Vertex 两家没真跑；OpenAI 加密思考的回放、Muse 的升级审批、Claude Code 的 `AskUserQuestion` 回答格式只有离线测试；第二个 Claude 订阅的真登录要站主自己登。

## 退回项

（没有）

## 下一步

1. **站主**：提交、推送 Phase 5（说明见对话，草稿：`v0.5: model gateway adapters for seven apis, five cli engines, cli accounts and local mcp`）。推上去以后云端把 `main` 合进 `claude/peck-v0.5`，换上真 Gateway。
2. **云端**：照上面「给云端」和「云端（Phase 6）」做；Phase 6 完成后写「v0.5 等本地验收」。
3. **本地**：等 v0.5 验收（合并、装包、出验收页；Phase 5 真跑剩下的额度：OpenAI 1、Anthropic 1、DeepSeek 3、百炼 3、Gemini 3，Codex、Claude Code、Grok 各 1，Muse、Antigravity 各 2）。站主开了 Vertex 项目就补验收点 3。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 旧授权的累计留作记录：Opus 7/20、DeepSeek 46/100，不再新增。
- 额度监测器（`src/quota/`、`src-tauri/src/quota/`）Agent 不写；v0.6 起站主用 Peck 开发。
- v0.4 留给站主看的三条（新包里点关闭和 Dock 叫回、撤销动效、提示条的角）站主没退回，当作看过了。
- `src-tauri/icons/peck.icns`、`peck.png` 是旧的 A3 原图，打包已经不用了（换成 `j2.*`，A3 用 `a3.*`），站主同意就删。
- 视觉基线要补的（本地改方案时做）：`bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；`DESIGN.md` 菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式。v0.7 要画 ⓘ 图标。
