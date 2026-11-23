# 开发路线

## 现在

**v1.1.2**（2026-11-23）：逐步优化 Peck Loop 的第二轮，把 API 的工作流做好。PLOOP 变成一个 API 模型当主角色、在后台派四类子代理（explorer、searcher、worker、auditor；设置 → AGENTS 配），API 单干和写规范、自动审查那几段去掉；一次回答里全是只读的工具同时跑；原地打转检测；照 Claude Code 格式的脚本 Hook（项目 `.claude/settings.json` 和插件）；百炼换到 Responses、接上它的 `web_search`。设计见 [architecture「PLOOP」「脚本 Hook」](./architecture.md)。

## 定位

Peck 是只为站主、只在站主这台 Mac 上跑的个人 **Harness**：包在模型外面的那一层——把各家模型和 CLI 接进来（Model Gateway）、给模型手脚（工具箱）、管它看到什么和记住什么（上下文）、每个任务前后存快照、把这一切摆给站主看（界面）。能在对话里说清楚的事不做成界面：反悔、定时任务、起 dev server、改完跑什么测试都交给模型。不考虑多用户、跨平台、团队，放开用 macOS 本机的能力。不跑本地模型、不做云同步。

- **Peck Loop：Peck 深度参与**。通过 Gateway 调任何 API 模型，带自己的工具箱、上下文管理和联网搜索；PLOOP 是它的工作方式（主角色 + 子代理）。优化的重点在这里。
- **CLI 引擎：Peck 只当外壳**。Claude Code、Codex、Muse、Grok、Antigravity 整包干，Peck 用它们官方的原生接口显示每一步、替它们审批、记花费，不把它们放进工作流；CLI 之间交接靠换引擎接着发一句或 `/review`。
- **视觉**照 `src/vendor/peck/`；新图标、新组件先在方案里画，同步办法见 architecture「视觉、打包和签名」。

## Phase

分 Phase 的版本（`1.x.0`、`2.0.0`）开工前在这里写表，站主看过再做。只做表里写的，不扩；状态只在「状态」列，四种：`未开始`、`进行中`、`等站主批`、`完成`；一个 Phase 做完改状态、提交。一个版本做完，表换成下一个版本的。

| Phase | 内容 | 验收 | 状态 |
| --- | --- | --- | --- |
| （现在没有） | | | |

## 之后要做的

候选，没排期，要做时站主定进哪个版本：

- **自定义子代理**（下一步）：在四类以外自己加类型（名字、做什么、给哪些工具、模型）；插件带的子代理（`agents/`）一起接。
- **PLOOP 的成员和任务板**：v1.1.2 的子代理是铺垫（名单已在 LOG 里）。
- **自动审查**：先不做（v1.1.2 定的）；现在审查是主角色自己派 auditor，或 `/review`。
- 用 Peck 开发 Peck 一整轮；Peck 接手站主的其他项目。
- 多家 CLI 和 Peck Loop 的 Benchmark（Atelier MCP 那一轮没跑满的一起补）。
- xAI 直连、GitHub Copilot CLI、Amp、Qwen Code。
- 会话树和分叉浏览。
- 快照回退、定时任务在真 CLI 上各走一遍（现在只有假模型和假 MCP 客户端的测试）。
- 插件：带自己的 MCP 服务器（现在只列出来）；联网的插件市场、在界面里编辑库都没做，要做再定。
- 收尾提交在 Claude Code、Muse、Grok 上各真跑一遍（现在只有 Codex 真跑过）；定时任务在从没点开过的老项目里跑之前先认结构。

## 不做

站主定了不要的，不再当候选提：

- GitHub PR 和 CI 回路；同一个项目里并行跑几个任务。
- 通用的 ACP 接入层：每家 CLI 用它官方的原生接口（Grok、Antigravity 官方给的就是 ACP，照它们的来）。
- CLI 进工作流（接力、CLI 审查、CLI 委派）：v1.1.1 删了。

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
| v1.1.2 | Aliyun | qwen3.8-flash | 4 次尝试 | 换到 Responses：`live_two_requests` 两次（思考 + 工具回传）、`live_web_search` 两次（它的 `web_search` 和函数工具一起用、回放被接受）都过 |
| v1.1.2 | OpenAI | gpt-6-luna（low） | 3 次尝试 | PLOOP 主角色（`live_ploop_spawns_explorers`）：一次回答派两个 explorer、等这一批、拿报告收尾，都过 |
| v1.1.2 | DeepSeek | deepseek-flash | 5 次尝试 | 同上那次里的两个 explorer（同时跑、只读），都过；整次约 $0.003 |
