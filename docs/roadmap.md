# 开发路线

## 现在

**v0.5 进行中**（2026-11-12）：v0.4 验收通过、合进 `main`（tag `peck-v0.4.0`）。站主填完「[Peck Harness 问卷](https://claude.ai/artifact/1tXsLfFDZmfBB6WMCuVhF3)」，Peck 从 GUI 转成个人 Harness，计划按问卷重写成下面的 Phase 5–14。v0.5 = Model Gateway（**本地开发**）+ 工具箱和 Peck 循环（云端开发），细节在 [v0.5 开发计划](./v0.5.md)。本地做好了 Phase 5 的「契约」（站主已提交，推送以后云端开工 Phase 6：表里已经是 `进行中`，云端看到 `main` 上有契约就开工）；**Phase 5 本地做完了**（七家 API、五家 CLI、CLI 账号、本机 MCP、额度、Gateway 版发任务；Vertex 两家等站主开项目再真跑），等站主提交、推送，云端接着把界面换上真 Gateway。

## 定位

Peck 是只为站主、只在站主这台 Mac 上跑的个人 **Harness**：包在模型外面的那一层——把各家模型和 CLI 接进来（Model Gateway）、给模型手脚（工具箱）、管它看到什么和记住什么（上下文）、替它盯着验证命令（后台和 Monitor）、把这一切摆给站主看和审（界面）。不考虑多用户、跨平台、团队和云端 runner，放开用 macOS 本机的能力。不跑本地模型、不做云同步。

- **干活的是 Peck 自己的循环**：Peck 通过 Gateway 调任何 API 模型，带自己的工具箱和上下文管理。
- **CLI 是可换的引擎**：Claude Code、Codex、Muse、Grok 当「整包引擎」也能选，Peck 用它们的结构化接口显示每一步、替它们审批、记花费。**Antigravity 只做直接调用**：站主选它、发一句话、它自己做完，Peck 显示进度和 Diff；它不进接力、委派、审查、定时任务、目标模式这些自动化（站主 2026-11-12 定，避开条款风险）。
- 主要借鉴 Pi（极简工具、钩子、会话树）、Codex（收件箱、按块审阅、独立工作区）、Claude Code Desktop（回合 recap、行内批注、后台任务）。调研在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。

## 怎么走

- **竖切**：每个版本做一条从头到尾能用的链路，装上就能用、能验。一个版本一到两个 Phase。
- **谁做**：Phase 表里「谁做」一列。默认云端账号开发、本地账号验收；站主指定的 Phase（比如 v0.5 的 Gateway）由本地账号在 `main` 上开发。同一个版本两边并行时，按版本计划里的「文件归属」各改各的。流程细节在 `CLAUDE.md`。
- **Git**：云端在自己的分支 `claude/peck-v0.x` 上提交、推到 GitHub；本地在 `main` 上改、不提交，站主提交和推送；验收通过后本地把云端分支合进 `main`，站主打 tag（`peck-v0.x.0`）、推送。
- **停下来问站主的只有两种**：视觉方案或原型要批；v1.0 发布。其余小事用提问功能问，没人答按 `CLAUDE.md`「默认取舍」做并记进 HANDOFF。
- **额度监测器是 Peck 自己的作业**：v0.6 起（Peck 有了自己的工具箱和上下文以后），站主用 Peck 真实开发「额度与花费」。Agent **不写**它（`src/quota/`、`src-tauri/src/quota/`），只在每个版本的验收清单里评估。现在的 `usage.rs` 和用量页在它接上以前照旧可用；Gateway 读到的订阅余量也先写进那里。
- **用 Peck 开发 Peck**：v0.7 以后。
- **视觉**：照 `src/vendor/peck/`（visual-atelier 方案 `peck` 同步来的，不手改）。新图标、新组件要进方案的，本地账号在方案里画、站主批、发新版本再同步。

## Phase

状态只有四种：`未开始`、`进行中`、`等站主批`、`完成`。云端只认「谁做」是云端的行。

| Phase | 版本 | 谁做 | 内容 | 验收点 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 1 文档与底座 | v0.3 | 云端 | 文档重写、界面用词定稿、`?mock` 假数据按新流程重做、`src/vendor/peck/` 的读取方式 | 已验 | 完成 |
| 2 视觉基线 | v0.3 | 本地 | 云端画布「[Peck Explore](https://claude.ai/artifact/2YJB1WKiiC6q7s6VmryCaR)」四步定稿，Design System「[Peck](https://claude.ai/artifact/KpUihg5AHn6cw1ggNTx6s1)」；方案 `peck` 0.1.0 同步进 `src/vendor/peck/` | 已验 | 完成 |
| 3 三栏外壳 | v0.4 | 云端 | 三栏新外壳、右栏四个页签、一个设置面板、跟随系统明暗 | 已验（19/19） | 完成 |
| 4 发任务与看任务 | v0.4 | 云端 | 发任务、看任务、撤销整个任务、TERM、菜单栏图标和通知 | 已验（19/19） | 完成 |
| 5 Model Gateway | v0.5 | **本地** | 统一事件模型和事件日志、能力标记、模型目录和价格表；API 七家：Anthropic、OpenAI（Responses）、DeepSeek、阿里云百炼、Gemini（原生接口）、Vertex 上的 Claude 和 Gemini（gcloud ADC）；CLI 五家结构化接入：Codex（app-server）、Claude Code（stream-json，站主的订阅）、Muse（--json）、Grok（ACP）、Antigravity（ACP，只直接调用）；CLI 的权限三档和审批；Peck 给 CLI 的本机 MCP 入口（审批、ASK）；订阅余量写进顶栏 QUOTA 的数据；CLI 多账号（设置里切换登录、重新验权，和客户端、浏览器的登录互不干扰）。第一步先出「契约」（事件类型、接口、夹具、假的 Gateway）给云端用。细节见 [v0.5 开发计划](./v0.5.md) | 夹具回放测试全过；本地真跑：每家 API 一次带工具的请求，每家 CLI 一个小任务，事件、Diff、审批卡片、花费都对 | 完成 |
| 6 工具箱和 Peck 循环 | v0.5 | 云端 | Peck 循环接 Gateway；9 个工具（`list_directory`、`read_file`、`search_text`、`find_files`、`write_file`、`write_patch`、`bash_run`、`update_plan`、`ask_user`），补丁原子、大输出落盘；权限三档真正生效（命令默认联网、只限制写到项目外）、高危和 ASK 卡片；「谁来干」改成一个模型列表（去掉顶栏 CLI / API）；Peck 循环和 CLI 的过程用同一套显示，工具调用折叠成一行；Markdown（回答渲染、输入框边打边渲染）；花费（每轮一行、输入框旁、顶栏 QUOTA）和花费上限生效；ONE SHOT；App 图标换 J2（设置里能切回 A3）。版本号 `0.5.0`。**等 Phase 5 的契约推到 `main` 以后**才开工 | `?mock` 走完发、看、批、问、撤；本地验收：真实 DeepSeek 一个任务、真实 Claude Code 和 Codex 各一个任务 | 进行中 |
| 7 上下文 | v0.6 | 云端 | 项目说明 AGENTS.md（没有就读 CLAUDE.md）；分层和缓存断点；每次自动带上环境、git、任务清单、改过的文件；输入框旁的用量小环（点开看分类和压缩线）；自动压缩三步（落盘、清旧工具输出、交接摘要）；`memory` 工具 + 任务结束「想记住这几条」由站主批；@ 旧任务、模型能搜历史任务 | `?mock` 长任务压缩后接着做；本地：真实长任务一次 | 未开始 |
| 8 后台和 Monitor | v0.6 | 云端 | `.peck/verify.toml`（第一次打开项目自动猜、站主确认）；规则为主 + 模型自己起后台命令；后台命令通道（不再 180 秒一刀切）；失败自动修 3 次（同一失败连续 2 次、两次之间没改文件就停）；通过不再叫模型、挂 ✓；结果只显示成对话卡片；常驻 dev server；目标模式。版本号 `0.6.0`。**额度监测器作业从这个版本开始** | `?mock` 失败修好、反复失败停下、并发过期三个场景；本地：真实 Rust 项目 check 失败到修好 | 未开始 |
| 9 会话和项目 | v0.7 | 云端 | 左栏上面「待你处理、在跑」、下面项目和任务；项目即环境：多个文件夹、每个自己选能改还是只读，右键和悬停编辑、删除；项目里记默认引擎、验证命令、权限和「总是允许」、环境变量、记忆；每轮 recap 卡片（带下一步按钮）；界面里的提示文字全删、必要的放 ⓘ（ⓘ 本地在方案里画、站主批）；不同项目同时跑 | `?mock` 两个项目同时跑；本地：一个真实 + 一个假的同时跑 | 未开始 |
| 10 审阅和预览 | v0.7 | 云端 | FILES：代码上色（Peck 的颜色）、md 和 html 切源码 / 预览、图片、PDF、Word 和 Excel（Quick Look）；CHANGES：Diff 行内批注攒着一起发、按块接受 / 拒绝；任务可选独立工作区（git worktree），做完一键搬回。版本号 `0.7.0`。之后站主开始用 Peck 开发 Peck | `?mock` 走完；本地：五种文件预览、独立工作区搬回 | 未开始 |
| 11 多引擎合作 | v0.8 | 云端 | 接力（任意两个引擎，前一个写规范后一个实施）、委派（子任务交给便宜的模型）、交叉审查；中途换引擎按分叉；Peck 接外部 MCP 服务器；GPT 模型加 V4A 补丁。Antigravity 不参与 | 本地：真实接力、委派、审查各一次 | 未开始 |
| 12 本机和打磨 | v0.8 | 云端 | 剪贴板、用编辑器 / 访达打开、内置预览面板（起 dev server、点元素写批注）、模型看 App 窗口截图；定时任务（结果进「待你处理」）；动效、键盘、文案全文过一遍。版本号 `0.8.0` | 本地：原生过一遍 | 未开始 |
| 13 收口 | v0.9 | 本地 + 云端 | 文档终稿；升级测试、备份恢复真实走一遍；用 Peck 开发 Peck 一整轮；额度监测器终评。版本号 `0.9.0`，**停下等站主** | 站主验收 v0.9 | 未开始 |
| 14 发布 | v1.0 | 本地 | 站主批了 v0.9 以后：版本号 `1.0.0`，打包、装；roadmap 精简到只剩 v1.0 之后的事 | 站主说发布 | 未开始 |

**v1.0 之后**：xAI 直连、GitHub Copilot CLI、Amp、Qwen Code；Muse 的会话协议（MSP）；会话树和分叉浏览；只读探索 subagent；Skills；本地模型和云同步不做。

## 版本与验收清单

| 版本 | 内容 | 验收清单页 |
| --- | --- | --- |
| v0.3 | 文档与底座、视觉基线（完成） | 不出页，站主在画布上批的视觉 |
| v0.4 | 新外壳、发任务与看任务（通过，19/19；tag `peck-v0.4.0`） | [Peck v0.4 验收](https://claude.ai/artifact/65eT7MNmNNFbn343WTX6BK) |
| v0.5 | Model Gateway、工具箱和 Peck 循环 | 本地验收时填 |
| v0.6 | 上下文、后台和 Monitor | 本地验收时填 |
| v0.7 | 会话和项目、审阅和预览 | 本地验收时填 |
| v0.8 | 多引擎合作、本机和打磨 | 本地验收时填 |
| v0.9 | 收口 | 本地验收时填 |

验收清单页由本地账号出（`docs/acceptance/README.md`），站主在页上每一项勾「通过」或「退回」并写意见；本地账号读回，把退回项写进 HANDOFF。

## 现行授权

**授权 `peck-v0.5-2026-11-12`**（站主在问卷里定「开一份新授权」）。只有本地账号用：本地开发的 Phase（v0.5 的 Gateway）和每个版本的本地验收。每个版本重新算：

| 对象 | 每个版本的上限 | 说明 |
| --- | ---: | --- |
| 每家 API（Anthropic、OpenAI、DeepSeek、阿里云百炼、Gemini、Vertex 上的 Claude、Vertex 上的 Gemini） | 5 次尝试 | 一次 HTTP 发送尝试计一次，失败、取消、重试都计；默认用这家最便宜、又测得到要测功能的模型，第一次用到某个模型就把它的 API ID 记进下表 |
| 每家 CLI（Codex、Claude Code、Muse、Grok、Antigravity） | 3 个小任务 | 用站主已登录的订阅；每个任务不超过 12 轮；在 HANDOFF 记次数和结果 |

- 账本照旧在 `~/Library/Application Support/Peck/api-budget.db`，新目录、角色、别名不重置；上限用完先停下问站主，不静默重试。
- 旧授权 `peck-v0.2-2026-11-06` 的累计：Claude Opus 5.5（`claude-opus-5-5`）7/20、DeepSeek Flash（`deepseek-flash`）46/100，留作记录，不再新增。

| 版本 | 对象 | 模型 API ID | 用了 |
| --- | --- | --- | ---: |
| v0.5 | Anthropic（契约的夹具：一次带工具、一次带思考；第一对 `medium` 没出思考，换 `high` 重录） | `claude-haiku-5-5` | 4/5 |
| v0.5 | Codex（契约的夹具：读、追加一行、跑一条命令，3 次审批；M2 真引擎：跑一条命令、1 次审批，`gpt-6-luna`） | 订阅（`gpt-6.1-sol`、`gpt-6-luna`） | 2/3 |
| v0.5 | Claude Code（契约的夹具：同一个小任务，在 `projects/gateway-smoke`；M2 真引擎：跑一条命令，审批走 stdio） | 订阅（`claude-haiku-5-5`） | 2/3 |
| v0.5 | Muse（M2 的夹具：会话协议 `muse serve`，读一个文件、跑一条命令、1 次审批） | 订阅（默认模型，`low`） | 1/3 |
| v0.5 | Grok（M2 的夹具：ACP，读一个文件、跑一条命令；第 2 个：经 Peck 的本机 MCP 调 `ask_user`） | 订阅（`grok-4.5`，`low`） | 2/3 |
| v0.5 | Antigravity（M2 的夹具：ACP，直接调用，读一个文件、跑一条命令） | 个人订阅（`gemini-3.8-flash`，`low`） | 1/3 |
| v0.5 | OpenAI（M2 的夹具：一次带工具和思考、一次回放；第一对 `low` 没出思考，换 `high` 也没出，停下；思考的回放只有离线单测） | `gpt-6-luna` | 4/5 |
| v0.5 | DeepSeek（同上；第一次没出思考，第二次出了） | `deepseek-flash` | 2/5 |
| v0.5 | 阿里云百炼（同上，北京） | `qwen3.8-flash` | 2/5 |
| v0.5 | Gemini（同上；新格式 Key 能用） | `gemini-3.5-flash-lite` | 2/5 |
