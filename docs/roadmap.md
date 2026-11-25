# 开发路线

## 现在

**v1.1.11**（2026-11-25）：修站主用 PLOOP 开发 Peck 时碰到的问题——附图（`+` 选的图、拖放、对话里画成小图和小片、`read_file` 能看图）；ASK 一次问几个翻着答、Other 有圆圈、写的字算补充；高危学 Claude Code 的 Auto（项目里删东西不问）、卡片标题是命令那句说明、命令收起来；命令碰不到钥匙串；新工具 `screenshot_page` 截本机网页；任务头的轮数和子代理；QUOTA 去掉折线。v1.1.10（PLOOP 做的）：NEW TASK 去掉 WHO、QUOTA 撑满居中、中栏正文 15px、要读的用 Body。

## 定位

Peck 是只为站主、只在站主这台 Mac 上跑的个人 **Harness**：包在模型外面的那一层——把各家模型和 CLI 接进来（Model Gateway）、给模型手脚（工具箱）、管它看到什么和记住什么（上下文）、每个任务前后存快照、把这一切摆给站主看（界面）。能在对话里说清楚的事不做成界面：反悔、定时任务、起 dev server、改完跑什么测试都交给模型。不考虑多用户、跨平台、团队，放开用 macOS 本机的能力。不跑本地模型、不做云同步。

- **Peck Loop：Peck 深度参与**。通过 Gateway 调任何 API 模型，带自己的工具箱、上下文管理和联网搜索；PLOOP 是它的工作方式（主角色 + 子代理）。优化的重点在这里。
- **CLI 引擎：Peck 只当外壳**。Claude Code、Codex、Muse、Grok 整包干，Peck 用它们官方的原生接口显示每一步、替它们审批、记花费，不把它们放进工作流；CLI 之间交接靠换引擎接着发一句或 `/review`。
- **视觉**照 `src/vendor/peck/`；新图标、新组件先在方案里画，同步办法见 architecture「视觉、打包和签名」。

## Phase

分 Phase 的版本（`1.x.0`、`2.0.0`）开工前在这里写表，站主看过再做。只做表里写的，不扩；状态只在「状态」列，四种：`未开始`、`进行中`、`等站主批`、`完成`；一个 Phase 做完改状态、提交。一个版本做完，表换成下一个版本的。

| Phase | 内容 | 验收 | 状态 |
| --- | --- | --- | --- |
| （现在没有） | | | |

## 之后要做的

候选，没排期，要做时站主定进哪个版本：

- **自定义子代理**（下一步）：在五类以外自己加类型（名字、做什么、给哪些工具、模型）；插件带的子代理（`agents/`）一起接。
- **自动审查**：先不做（v1.1.2 定的）；现在审查是主角色自己派 auditor，或 `/review`。
- 用 Peck 开发 Peck 一整轮；Peck 接手站主的其他项目。
- 多家 CLI 和 Peck Loop 的 Benchmark（Atelier MCP 那一轮没跑满的一起补）。
- xAI 直连、GitHub Copilot CLI、Amp、Qwen Code。
- 快照回退、定时任务在真 CLI 上各走一遍（现在只有假模型和假 MCP 客户端的测试）。
- 插件：带自己的 MCP 服务器（现在只列出来）；联网的插件市场、在界面里编辑库都没做，要做再定。
- 收尾提交在 Claude Code、Muse、Grok 上各真跑一遍（现在只有 Codex 真跑过）；定时任务在从没点开过的老项目里跑之前先认结构。

## 不做

站主定了不要的，不再当候选提：

- GitHub PR 和 CI 回路；同一个项目里并行跑几个任务。
- 通用的 ACP 接入层：每家 CLI 用它官方的原生接口（Grok 官方给的就是 ACP，照它的来）。
- Antigravity（Google）：条款风险，v1.1.5 整个去掉（引擎、额度卡片、夹具、`cli/` 里的安装），不再接。
- CLI 进工作流（接力、CLI 审查、CLI 委派）：v1.1.1 删了。
- 跨任务的任务板：任务板只管这一个任务（v1.1.3 定的）。
- 改完代码自动拿到编译错误（LSP 诊断）、会话树和分叉浏览、轨迹记录（v1.1.4 定的）：改完跑什么写在项目说明里由模型自己跑；换引擎接着发、`@` 旧任务、事件日志已经够用。

## 现行授权

只管 Claude 自己的真跑，每个版本重新算；站主自己在 App 里发的任务不算，只受设置里的花费上限管。

| 对象 | 每个版本的上限 | 说明 |
| --- | ---: | --- |
| 每家 API（Anthropic、OpenAI、DeepSeek、Aliyun、Gemini、Vertex 上的 Claude、Vertex 上的 Gemini） | 5 次尝试 | 一次 HTTP 发送尝试计一次，失败、取消、重试都计 |
| 每家 CLI（Codex、Claude Code、Muse、Grok） | 3 个小任务 | 用站主已登录的订阅；每个任务不超过 12 轮；Claude Code 尽量不用（站主平时要用） |

- 一律用这家最便宜、又测得到要测功能的模型，做无关紧要的小任务；测试项目放仓库外的 `personal-cli-studio/projects/`。
- 用之前和之后都记账；失败不静默重试；用完先停下问站主。
- 站主另给的额度（比如某一周按钱算）以站主当时说的为准，记在下面。
- **站主另给（2026-11-25，v1.1.9 验收第二轮）**：Muse 10、Claude Code 3、Codex 5、Grok 10 个小任务，DeepSeek 50 次尝试；百炼（Aliyun）、Vertex、Gemini、Tavily 各 5 次尝试。都从 0 算（不和下面 v1.1.9 前一轮的混算）。DeepSeek 的 50 次用完（超了 1 次）后站主又给了 100 次。

**账本**（只留当前版本）：

| 版本 | 对象 | 模型 | 用了 | 做了什么 |
| --- | --- | --- | ---: | --- |
| v1.1.9 | Muse | 默认、muse-spark-1.3 | 3 / 3 | 两次直接走协议看后台命令：回合结束时 `task/list` 还有一条在跑，跑完它自己开下一回合；一次走 Peck 的引擎（Spark、带 `--trust-workspace`）：等到了那一回合，`quota.updated` 也到了（5 小时 14%、每周 10%） |
| v1.1.9 | Codex | gpt-6-luna low | 2 / 3（估 $0.005） | 带信任开关起 app-server；第一步工具开始时追加一句（`turn/steer`），最后的回答照追加的那句做了；验收时正式版窗口里再追加一次，照做、花费标「估」 |
| v1.1.9 | DeepSeek | V4.1 Flash | 3 / 5（$0.0014） | 验收：正式版窗口里 PLOOP 跑着时追加一句，YOU 进过程，回答照那句改了 |
| v1.1.9 | Grok | grok-4.7-build-fast | 1 / 3（两轮，估 $0.26） | 验收：TRUST 卡片点了，`grok --trust` 记进它的信任表；跑着时发的话排着，这一轮做完自动发成下一句 |
| v1.1.9 | Claude Code | haiku-5-5 | 3 / 3（估 $0.012） | 验收：一次问两个问题各自答回去、带信任开关；跑着时追加卡住（它把那句并进这一回合、只回一个 `result`），按 STOP 收；一次探针走协议看回显（`--replay-user-messages`）；修完复测追加，自己收尾 |
| v1.1.9 验收二 | Muse | muse-spark-1.3 xhigh | 2 / 10（估 $0.25） | 记忆 + 开场 Hook + 一次问两个 + 跑着时插话（收尾 Hook 卡片被藏，STOP 收；已修）；后台命令等到 Muse 自己开下一轮 |
| v1.1.9 验收二 | Claude Code | haiku-5-5 | 3 / 3（估 $0.02） | 记忆 + Hook + 清单（`-p` 下没有清单工具）；一次探针看开场工具列表（发一句「hi」、一出 init 就停，三次）；带上 `CLAUDE_CODE_ENABLE_TODO_TOOLS` 后清单 2/2 画出来了 |
| v1.1.9 验收二 | Codex | gpt-6-luna low | 2 / 5（估 $0.005） | 记忆 + 开场 Hook；那行 MCP 提示没了；默认模式没有提问工具；收尾 Hook 卡片看得见、批了以后续上会话接着一轮 |
| v1.1.9 验收二 | Grok | grok-4.7 high | 2 / 10（估 $0.17） | 记忆 + 开场 Hook；发现信任过的文件夹里 Grok 自己也跑项目 Hook（两遍）；修完复测 Peck 不再替它跑 |
| v1.1.9 验收二 | DeepSeek | V4.1 Flash | 51 / 50（$0.03） | 派两个 explorer + 一个 searcher（DeepSeek 搜索 2 次）17 次；任务板（2 worker + member + auditor）34 次：auditor 跑了十几轮、翻到项目外、要跑 `tmutil` 被拒，主角色最后一次「连不上供应商」失败（计 1 次） |
| v1.1.9 验收二 | DeepSeek（追加） | V4.1 Flash | 25 / 100（$0.014） | 反悔 + 定时任务 + 留着的 dev server 7 次；读 PDF / Notebook + 按需加载 dw + 收尾 Hook 拦一次 7 次；Git Plan 收尾提交（含问提交说明）6 次；新端口的 dev server（PREVIEW 自己换过去）5 次 |
| v1.1.9 验收二 | 百炼（Aliyun） | qwen3.8-flash | 3 / 5（$0.002） | 新加坡地域：地址通、北京的 Key 被拒（Invalid API-key）；切回北京问一个要联网的问题（2 次） |
| v1.1.9 验收二 | Vertex | Claude Haiku 5.5 | 0 / 5 | 填了项目 ID 后模型列出来了；gcloud 换不到令牌（ADC 过期），请求没发出去 |
| v1.1.9 验收二 | Gemini | gemini-3.8-flash | 1 / 5 | 这把 Key 额度用完（429 quota），原生搜索没测到 |
| v1.1.9 验收二 | Tavily | — | 1 / 5 | Qwen 当主角色时 `web_search` 走 Tavily 搜一次，回答带出处 |
| v1.1.8 | Muse、Codex | — | 0 / 3 | 没花额度：`muse serve [--trust-workspace]` 只建会话，用本地假 MCP 看它连不连（带了才连）；`codex -c projects={…} mcp list` 看项目 `.codex/config.toml` 读没读进来（内联表的写法才认） |
