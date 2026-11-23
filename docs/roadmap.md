# 开发路线

## 现在

**v1.1.0**（2026-11-23）：插件。Peck 有插件库，项目设置里 INSTALL 把插件或 Skill 拷进项目、跟着项目的 git 走，库里有新版出 UPDATE；自带的（撤销、定时任务、写 Skill）按项目开关；点开项目时补齐 `CLAUDE.md` / `AGENTS.md` 和 Skill 文件夹的软链接；库里第一个插件 Git Plan 默认每个项目都装，每一轮做完自动提交（不 push）。设计见 [architecture「插件」](./architecture.md)。没有分 Phase 的版本在做。

## 定位

Peck 是只为站主、只在站主这台 Mac 上跑的个人 **Harness**：包在模型外面的那一层——把各家模型和 CLI 接进来（Model Gateway）、给模型手脚（工具箱）、管它看到什么和记住什么（上下文）、每个任务前后存快照、把这一切摆给站主看（界面）。能在对话里说清楚的事不做成界面：反悔、定时任务、起 dev server、改完跑什么测试都交给模型。不考虑多用户、跨平台、团队，放开用 macOS 本机的能力。不跑本地模型、不做云同步。

- **干活的是 Peck Loop**：通过 Gateway 调任何 API 模型，带自己的工具箱和上下文管理。
- **CLI 是可换的引擎**：Claude Code、Codex、Muse、Grok 当整包引擎也能选，Peck 用它们的结构化接口显示每一步、替它们审批、记花费。
- **视觉**照 `src/vendor/peck/`；新图标、新组件先在方案里画，同步办法见 architecture「视觉、打包和签名」。

## Phase

分 Phase 的版本（`1.x.0`、`2.0.0`）开工前在这里写表，站主看过再做。只做表里写的，不扩；状态只在「状态」列，四种：`未开始`、`进行中`、`等站主批`、`完成`；一个 Phase 做完改状态、提交。一个版本做完，表换成下一个版本的。

| Phase | 内容 | 验收 | 状态 |
| --- | --- | --- | --- |
| （现在没有） | | | |

## 之后要做的

候选，没排期，要做时站主定进哪个版本：

- 用 Peck 开发 Peck 一整轮；Peck 接手站主的其他项目。
- 多家 CLI 和 Peck Loop 的 Benchmark（Atelier MCP 那一轮没跑满的一起补）。
- xAI 直连、GitHub Copilot CLI、Amp、Qwen Code。
- 会话树和分叉浏览；只读探索 subagent。
- 快照回退、定时任务在真 CLI 上各走一遍（现在只有假模型和假 MCP 客户端的测试）。
- 插件：带自己的 MCP 服务器（现在只列出来）；联网的插件市场、在界面里编辑库、任意脚本的 Hook、子代理（`agents/`）都没做，要做再定。
- 收尾提交在 Claude Code、Muse、Grok 上各真跑一遍（现在只有 Codex 真跑过）；定时任务在从没点开过的老项目里跑之前先认结构。

## 现行授权

只管 Claude 自己的真跑，每个版本重新算；站主自己在 App 里发的任务不算，只受设置里的花费上限管。

| 对象 | 每个版本的上限 | 说明 |
| --- | ---: | --- |
| 每家 API（Anthropic、OpenAI、DeepSeek、Aliyun、Gemini、Vertex 上的 Claude、Vertex 上的 Gemini） | 5 次尝试 | 一次 HTTP 发送尝试计一次，失败、取消、重试都计 |
| 每家 CLI（Codex、Claude Code、Muse、Grok、Antigravity） | 3 个小任务 | 用站主已登录的订阅；每个任务不超过 12 轮；Claude Code 尽量不用（站主平时要用） |

- 一律用这家最便宜、又测得到要测功能的模型，做无关紧要的小任务；测试项目放仓库外的 `personal-cli-studio/projects/`。
- 用之前和之后都记账；失败不静默重试；用完先停下问站主。
- 站主另给的额度（比如某一周按钱算）以站主当时说的为准，记在下面。

**账本**（只留当前版本）：

| 版本 | 对象 | 模型 | 用了 | 做了什么 |
| --- | --- | --- | ---: | --- |
| v1.1.0 | Codex | gpt-6-luna（low） | 1 个小任务（两轮） | 收尾 Hook：Codex 改一个文件，续上会话问到一行 `v0.1.0: add hook smoke note`，Peck 提交了（`projects/git-plan-smoke`，引擎一层的 `live_stop_hook_commits_the_round`） |
