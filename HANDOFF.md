# 当前交接

更新：2026-11-23。**v1.1.1 做完了，已装进 /Applications**。这是逐步优化 Peck Loop 的第一轮。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md「PLOOP」「联网搜索」](./docs/architecture.md)，定位和不做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.1）

1. **分清两种干法**：CLI 引擎整包干，Peck 只当外壳（显示、审批、花费、本机 MCP 的几样工具），不进工作流；Peck Loop 是 Peck 深度参与的那一边。CLI 之间交接靠换引擎接着发一句或 `/review`（`/review` 用现在选的那一个按 PLAN 看最近一次改动）。
2. **接力换成 PLOOP**：AGENTS 的 FLOW 是 API / CLI / PLOOP。PLOOP 三行「写规范」（OFF）、「实施」、「审查」（OFF）只列 API 模型，委派留在设置 → AGENTS、只在 PLOOP 里用。API、CLI 单干没有审查和委派。后端 `team.rs` 只认 API 模型（CLI 的那几段代码删了）；CLI 委派（本机 MCP 的 `delegate_task`、`harness/delegate.rs`）删了。
3. **工作流预设去掉了**：左栏的「预设」、设置 → AGENTS 的 PRESETS、项目设置的「默认预设」都没了。存着的设置读的时候换过来：接力 → PLOOP（只留 API 模型那几格），预设丢掉。
4. **联网搜索**（Peck Loop）：Anthropic、OpenAI、Gemini（3 起）接各家原生搜索；DeepSeek 用它家的 Search API（同一个 Key）；百炼、Vertex 上的 Gemini 走 Tavily（设置 → PROVIDERS → SEARCH 填 Key）；都没有就不给。供应商那边搜的每一次显示成一步「Search the web: …」（Anthropic 新版在它那边跑代码筛结果，那几步写「筛搜索结果」），按次的费用算进花费和花费上限，花费行多「搜 N 次」。
5. 文档：roadmap 加了「不做」（PR / CI 回路、同项目并行、通用 ACP 接入层、CLI 进工作流），每家 CLI 用官方原生接口写进了 architecture。

- 新加依赖：没有。数据库没动（schema 11）。`app_settings` 的 `ui` 那一行：`relay` → `ploop`，`presets`、`preset` 不再存；项目设置不再有 `preset`。钥匙串「其他」那一包多一项 `tavily`。事件里 `usage` 多一个 `webSearches`。

## 验证

- **自动测试**：全量 `cargo test` 过（新加：三家转换器各一组原生搜索、Peck Loop 的原生搜索 + `pause_turn`、Peck 的 `web_search`、搜索路由和参数）；`pnpm check`、`pnpm build` 过。
- **`?mock`**：AGENTS 三个 FLOW、PLOOP 下拉里只有 API 模型、PLOOP 写规范 → 实施 → 审查跑通；「上网搜一下」两步搜索折成一组、花费行「搜 2 次」；设置 → PROVIDERS 的 SEARCH、设置 → AGENTS 只剩委派（明暗都看了）。
- **真跑**（账在 roadmap）：Anthropic Haiku 5.5（基础版）、Sonnet 5.5（动态过滤）、OpenAI gpt-6-luna 各两次请求都过——带着 Peck 的工具一起搜、搜索原件回放被接受；DeepSeek Search API、Tavily 各搜一次都过。**Gemini 没验到**：Key 目录里那把 Key 额度用完（429）。
- **正式版**：装好了，版本号 1.1.1、签名校验过；窗口里没点过。
- **没验的**：
  - 正式版窗口里一下都没点过：AGENTS 的 PLOOP、设置 → PROVIDERS → SEARCH 存 Tavily 的 Key、整个 App 里 Peck Loop 真搜（上面的真跑只到 Gateway 一层）。
  - Gemini 的原生搜索（官方写着和函数调用一起用还是 Preview）、Vertex 上的 Claude（基础版）和 Gemini（走 Tavily）都没真跑过。
  - Anthropic 搜到一半停下（`pause_turn`）再发一次，只有假模型的测试。

## 留给站主的

- **第一次打开新版**：可能弹钥匙串授权框。
- **AGENTS 变了**：你以前用的「省钱」「稳妥」「Shot」「Daily」预设都没了。「Shot」那一套现在就是 PLOOP：写规范 Sonnet 5.5、实施 DeepSeek Flash（旧的接力设置已经换过去）；委派 DeepSeek Flash 留着，只在 PLOOP 里用。
- **Tavily**：在设置 → PROVIDERS → SEARCH 填一次 Key（这次真跑用的是剪贴板里那把，没存进钥匙串）。不填的话百炼、Vertex 上的 Gemini 没有联网搜索，DeepSeek 搜不了时也没有退路。
- **走一遍**：选 Sonnet 或 GPT 发「上网查一下 Tauri 最新版本」，看过程里的「Search the web」和花费行；有 Gemini 额度时也试一次。
- **Key 目录**：`anthropic` 那个文件按你说的换成了剪贴板里的新 Key；Gemini 那把额度用完了。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2、1.0.3、1.1.0 和 1.1.1 的全部提交。
- 以前留下、还没验的：1.1.0 的插件界面和收尾提交在正式版里走一遍；1.0.3 的轮数设置、旧任务的样子；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 搜索按次的费用按收费价记，Gemini 每月头 5000 次、Tavily 免费档每月 1000 次不扣（会多算） | 宁可多算；在意再加个月度计数 |
| 旧的定时任务里记了 CLI 写规范或审查的，到点起不来、记成错过（正式数据里现在没有这样的） | 记着 |
| OpenAI 的来源没有标题，过程里标题写网址 | 记着 |
| Gemini 的来源挂在第一条搜索上（它只在最后一起给） | 记着 |
| 只有界面点开项目时才认结构、装默认插件；定时任务在一个从没点开过的老项目里跑时不会先做 | 记在 roadmap |
| 单独的 Skill 装没装是按「项目里有没有同名的 Skill」认的：项目自己写了一个和库里同名的，会显示成装着的，点 UPDATE / REMOVE 会动到它（库里现在没有单独的 Skill） | 往库里放单独的 Skill 之前先加归属记号 |
| 收尾提交失败（仓库的 pre-commit 钩子不过、没配 `user.name`）时，这一轮的文件已经 `git add` 了，留在暂存区 | 记着 |
| CLI 任务每轮做完多用一轮订阅额度问提交说明；Antigravity、续不了会话的不问，写「模型没给出提交说明」 | 用一阵看烦不烦 |
| Peck Loop 用命令（不是自己的工具）改了一个开头就没提交的文件、别的都没动时，收尾不问、也不提交 | 记着 |
| 插件带的 MCP 服务器（`.mcp.json`）只列出来，Peck Loop 不起 | 有插件要用时再接 |
| CLI 续接会话的花费按「会话号」取差：续接后会话号变了就整个算进去（不会少算，可能多算一次） | 真跑时顺便看 QUOTA |
| 模型只在被点名时才提议存 Skill，主动提的没见过 | 用一阵再看 |
| 定时任务的「谁来干」设的时候不查 Key 和 CLI 在不在，到点起不来记成错过 | 记着 |
| 回退只按整个文件放回，做不到一个文件里的某一段 | Skill 里让模型直接改那一段 |
| 预览的 RELOAD 回到最初的地址；`localhost` 只连 127.0.0.1 | 记着 |
| Vertex 上的 Claude / Gemini 只有离线测试，没真跑过 | 站主开了 GCP 项目再试 |
| PREVIEW 批注只在浏览器存储里；Esc 关设置页时没存的 MCP 编辑会丢 | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、PLOOP 每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。 1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字、recap 里收尾提交的那一行。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。
- 联网搜索的协议各家常变（Gemini 的工具组合还是 Preview、Anthropic 搜索工具按日期出新版本）：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

等站主给 PLOOP 的新工作流方案和设计（下一轮）。其他候选在 roadmap「之后要做的」。
