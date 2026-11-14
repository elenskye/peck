# 技术架构

> 选型、一个任务怎么跑（项目即环境、独立工作区）、数据放哪、Model Gateway、Peck 循环（上下文、后台和验证）、审阅和预览、代码位置。界面的用词和样子在 [design.md](./design.md)，怎么测在 [testing.md](./testing.md)。

## 选型

| 层 | 用什么 | 为什么 |
|---|---|---|
| 外壳 | Tauri 2（macOS） | 原生窗口、钥匙串、菜单栏、通知；包小 |
| 界面 | React 19 + TypeScript，样式是 `src/vendor/peck/` 的变量和 `pk-` 组件加一份 `src/app.css`（不用 Tailwind），Vite 固定端口 1430 | 浏览器 `?mock` 能自检；视觉只有一个来源 |
| 后端 | Rust | 模型请求、工具、CLI 进程、快照、SQLite 都在这里，界面只经 IPC 调 |
| 存储 | SQLite（WAL）+ 影子 Git 仓库 | 记录和历史 Diff 分开存，重开都还在 |

不做：常驻辅助服务、通用插件框架、本地模型、云同步、开机自启。

## 一个任务怎么跑

一句话对应一个**任务**（代码里叫 Run）。任务开始前给项目拍一个**任务前快照**；跑完留着，撤销是站主主动点。同一个项目同时只跑一个任务（两个任务要写的文件夹——项目文件夹和能改的其他文件夹——一样或套着也算同一个，`ActiveRunContext::touches`），不同项目能同时跑（后端按项目锁，界面每个项目自己的「在跑」，菜单栏图标两个以上写个数）。失败、取消、重开都不自动重发：可能已经生效的请求和命令不重放。

「谁来干」是模型列表（IPC `gateway_models`）里的一个 id，决定走哪条路：

| id | 谁干 | 入口 |
|---|---|---|
| `api:<供应商>/<模型>` | Peck 循环 + 这个 API 模型（下面「Peck 循环」） | `harness_run_start` |
| `cli:<引擎>` | CLI 整包引擎，结构化接入（下面「CLI 五家」） | `gateway_run_start` |
| `flow:muse>grok`、`flow:grok>muse` | 旧的接力：前一个 CLI 写 `.peck/handoff/<任务号>/intent.md`，后一个读它实施；按 TOML 清单起纯文本子进程 | `run_dispatch` |
| `flow:plan-implement` | 旧的规划 + 实施（`api/worker.rs`、`supervisor.rs`）：规划的模型用 `delegate_task` 把子任务交给实施的模型、`review_task` 记通过 / 返工 / 接手 | `api_run_start` |
| ONE SHOT（右栏） | Peck 循环只给读的工具（最多 12 轮），或 CLI 引擎按 PLAN 档；不占任务号、不锁项目和终端，事件照样进事件日志；Antigravity 不进 | `harness_oneshot` |

两个旧入口（接力、规划 + 实施）保留到 v0.8 多引擎合作时并进新循环；假 Agent（`peck-mock`，设置的开发者选项）也走旧的 TOML 清单。

- **控制通道**：审批、回答、追加一句、打断都走 `gateway_control(runId, control)`；CLI 引擎、Peck 循环、ONE SHOT 的任务都登记在 `gateway/runner.rs` 的同一张表里。`run_cancel` 先发打断，等它收尾，超时再结束整个进程组。
- **项目和任务的管理**（`projects.rs`，左栏右键）：改名（项目改的是显示名，文件夹不动；任务改 `sessions.title`）；项目设置（`Workspace.settings`：在这个项目里默认谁来干、默认权限，进项目时换上）；「移出列表」只把 `Workspace.hidden` 设上，文件夹、任务、快照都不动，重新打开这个文件夹就回来；删任务是真删（任务记录、消息、事件日志、Peck 循环的对话），旧 API 层（规划 + 实施、假 Agent）的调用记录只断开和任务的关联、留着照算花费；在跑的不能删。拖动排的顺序存在 `app_settings` 的 `layout`（没拖过的新项目、新任务排在最上面）。
- **项目即环境**（项目设置）：项目除了主文件夹，还能加最多 8 个文件夹，每个选只读或能改（`ProjectSettings.folders`；不行的：和主文件夹或彼此套着的、根目录、家目录和包着家目录的、和 Peck 数据目录套着的、`.git` 里面的——`workspace::folder_allowed`，每次用之前再查一遍）；项目自己的环境变量最多 60 个（`ProjectSettings.env`，`HOME`、`TMPDIR`、`PWD`、`SHELL`、`CI`、`PAGER`、`TERM` 和 `PECK_*` 不让改）。`workspace_update` 规整、校验这两样。Peck 循环：工具用绝对路径读其他文件夹、写能改的那几个（只读的拒掉；`.git` 一样不碰），命令的沙箱放开能改的那几个，只读的明着不让写；项目文件夹和能改的文件夹下面任何一层的 `.git` 都不让写（正则，挡住 `~/code` 这类文件夹里各个仓库的钩子），环境块里列出文件夹和变量名（值不进上下文）；能改的文件夹各拍一份任务前快照（各自的影子仓库），改动进 CHANGES（写绝对路径）、撤销整个任务一起撤。环境变量带进 Peck 循环的命令、后台命令、验证命令、dev server 和 TERM（同名的盖过站主 shell 里的）。CLI 引擎还只认主文件夹、不带项目的环境变量。
- **独立工作区**（`worktree.rs`，输入框旁的 WORKTREE，一次一选）：只给 git 项目、不能是 PLAN，Peck 循环和 CLI 引擎都行；所有 git 调用都带 `-c core.hooksPath=/dev/null`（不跑仓库的钩子，它们是沙箱里就能改的项目文件）；任务里项目的其他文件夹一律只读（不在独立工作区里，改了就是真改）。发出去时 `git worktree add --detach <数据目录>/worktrees/<项目哈希>/<任务号> HEAD`（不建分支，主文件夹的 index、commit、stash 不动，只在用户仓库的 `.git/worktrees/` 多一份登记），再把主文件夹没提交的改动（改过的、新建没忽略的、删掉的、改名的）照搬进去；任务在它里面做，快照存在项目自己的影子仓库里，CHANGES 照常。做完在 CHANGES 顶上 APPLY / DISCARD：APPLY 先比对——这几个文件在主文件夹里要和任务开始时一样（按 git 的口径比，换行、过滤器照 `.gitattributes`），不一样列成冲突、什么都不动，站主可以 APPLY ANYWAY 覆盖；先全看一遍再动手，复制带上文件权限。搬之前、搬之后各给主文件夹拍一份快照，任务记录换成主文件夹和这两棵树（`RunRecord.worktree.appliedAt`）：撤销整个任务回到搬之前，照样搬时覆盖掉的站主的改动也回得来。DISCARD 删掉独立工作区，记录也换成主文件夹，改动在历史 Diff 里照样能看。没搬回、丢掉了的不能撤销（没有要撤的）；同一个任务上一次的独立工作区没搬回、没丢掉时，不能接着发（先 APPLY 或 DISCARD）；建到一半失败、删不掉时只删自己那一条登记，不 `git worktree prune`；`autostart` 的 dev server 在独立工作区的任务里不起。
- **文件夹不在了**：每次读项目列表都看一遍文件夹在不在（`ProjectOverview.missing`）；不在的项目不能发任务，中栏换成「重新找文件夹 / 移出列表」。重新找（`workspace_relocate`）：项目 id 和快照目录都按文件夹路径的哈希，所以换到新文件夹时任务、「总是允许」挪到新 id 下，快照目录改名到新哈希（新位置已经有快照就不动）。
- **撤销整个任务**（`run_undo`）：算出任务前快照到做完之间改过的文件，新建的删掉、改过和删掉的用 `git restore --source=<任务前的树> --worktree` 写回；只动这几个文件，用户仓库的 index、commit、stash 不碰。撤销前再存一份快照；记录上写 `undoneAt`，历史 Diff 照旧能看。
- **终端**（右栏 TERM，`term.rs`）：每条命令在 pty 里起一个 shell，输出当文字显示（`TERM=dumb`），跑完报退出码和用时；`cd` 记着，`export` 不带到下一条。环境照站主平时的终端：App 打开时在后台起一次交互式登录 shell（`$SHELL -l -i -c`，最多等 8 秒）读出环境变量，这次打开期间每条命令带着它跑；读不出来退回 `$SHELL -l -c`。Peck 循环的 `bash_run` 也用这份环境。这个项目有任务在跑时拒绝新命令。只在内存，退出就没了。
- **关窗口**（`lib.rs`）：红色关闭只藏窗口，菜单栏图标和在跑的任务都留着；点 Dock 图标或菜单栏图标叫回；⌘Q 或菜单栏「退出 Peck」才退出。
- **菜单栏图标和通知**（`tray.rs`）：图标跟着空闲、在跑（三帧轮流，每帧 300ms）、等你换，两个以上在跑时图标旁边写个数（`set_running_count`）；做完、失败、停下、等你批、等你回答时按设置发 macOS 通知（Peck 在最前面时不发）。模板图由 `scripts/menubar-icons.mjs` 从 `src/vendor/peck/marks/menubar/` 出到 `src-tauri/icons/menubar/`：托盘把高度压成 18pt，脚本先裁掉四周的空，一格出 3 像素，菜单栏里一格 1.5pt。
- **App 图标**（`app_icon.rs`）：打包用 J2；`app_icon_set(j2 | a3)` 用 AppKit 换 Dock 里的图标，选哪个存在界面设置里，打开 App 时再设一次。图标文件由 `scripts/app-icons.sh` 从 `src/vendor/peck/marks/` 出到 `src-tauri/icons/`。

## 数据存放

| 内容 | 位置 |
|---|---|
| 正式数据 / 开发数据（`pnpm app:dev`） | `~/Library/Application Support/Peck/` / `Peck Dev/` |
| 项目、任务、步骤、消息、调用、用量快照、界面设置、事件日志 | 数据目录的 `peck.db`（**schema 9**，迁移在 `src-tauri/migrations/`）：6 加 `app_settings`（界面设置整段一行 JSON；Gateway 设置、CLI 的模型列表、左栏拖过的顺序 `layout` 也在这里）；7 加 `agent_events`（统一事件，原件另存一列）和 `runs.engine`；8 加 `harness_conversations`（Peck 循环的对话，接着发一句就带上）、`harness_allow`（「这个项目总是允许」）；9 加 `memories`（站主批过的项目记忆） |
| 项目的验证命令 | 项目里的 `.peck/verify.toml`（`.peck/` 打开项目时已经加进 `.gitignore`）；项目设置里记站主确认过的那一份的 md5（`verifyApproved`）和不要猜（`verifyDismissed`），这两样只由 `verify_save` / `verify_dismiss` 改 |
| 任务前快照和历史 Diff | `shadow/<项目哈希>/`（项目的其他文件夹各有自己的 `shadow/<文件夹哈希>/`；独立工作区的快照存在项目自己的那一个里） |
| 独立工作区 | `worktrees/<项目哈希>/<任务号>/`（搬回或丢掉时删掉） |
| 项目的其他文件夹、环境变量 | 项目设置（`workspaces` 表里那一行的 JSON：`folders`、`env`） |
| Peck 循环命令用的 HOME / 落盘的长输出 | `harness-runtime/<项目>/` / `harness-output/<任务>/`（命令输出、压缩时落的 `ctx-*.log`、验证命令的输出、模型起的后台命令的日志；同一个任务接着发的几次共用；14 天没动过的文件夹打开 App 时删掉）；常驻 dev server 的日志在 `harness-output/servers-<项目哈希>/` |
| 旧循环（规划 + 实施）命令用的 HOME | `worker-runtime/<项目哈希>/home/` |
| CLI 清单（接力、假 Agent） | `agents/*.toml`、`flows/*.toml`（第一次从 `src-tauri/defaults/` 复制） |
| Key | macOS 钥匙串，服务名 `Peck API`、`Peck API Dev`、`Peck API Test` 分开；每个服务两项：AI 供应商的 Key 一项（账户 `keys`）、其他 Key 一项（`other-keys`） |
| 真实 API 授权账本（只管 Agent 的测试调用，界面里 Peck 循环的请求不经过它） | 正式数据目录的 `api-budget.db`，测试目录、角色、别名都不重置 |

- `PECK_DATA_DIR`、`PECK_SHADOW_ROOT` 把数据和快照指到别处（测试用）；`PECK_API_MODE=mock|real` 强制模拟或真实。开发和临时目录默认模拟。
- **升级**：旧库升级前用 SQLite Backup API 做一致性备份；遇到比自己新的 schema 拒绝写入。改结构就加迁移、升版本号，不改旧记录的含义。
- **任务前快照**：影子仓库用独立的 `GIT_DIR`、`GIT_WORK_TREE` 跑 `add`、`write-tree`、`commit-tree`、`diff`，不碰用户仓库。每个任务和步骤存前后两棵树。要保住历史 Diff，`peck.db` 和 `shadow/` 一起备份。

## Model Gateway（`src-tauri/src/gateway/`）

把各家 API 和 CLI 都转成同一套事件，界面只认这一套。契约：`mod.rs`（两个接口：裸模型 `ModelProvider`、整包引擎 `AgentBackend`；对话用和供应商无关的 `Message` / `Part`；控制通道：审批、回答、追加、打断）、`event.rs`（统一事件）、`caps.rs`（能力标记）、`catalog.rs`（模型目录、价格、模型列表），TS 对照在 `src/lib/agentEvents.ts`。

- **事件**：`AgentEvent` = `seq`（任务里从 0 递增）+ `at`（毫秒）+ 可选 `subagentId` + 事件本体（`type` 是 `session.started` 这样带点的名字，字段 camelCase）。种类：会话、回合（结束原因：做完、到上限、拒绝、取消、打断、出错）、内容和思考的 delta / completed、工具调用（读、改、跑、搜、MCP……，改文件的带 Diff）、`diff.updated`、`plan.updated`、审批和提问的 requested / resolved、子任务、`usage.updated`、`quota.updated`、`notice`、`error`、`raw`。引擎只经 `EventSink`（`log.rs`）发：编号、存进 `agent_events`（原件另存一列，不发给界面）、再交给界面。读：`gateway_events(runId)`。
- **用量口径**：`input` 是没走缓存的输入，`cacheRead`、`cacheWrite` 另算；`output` 包括思考，`reasoning` 是其中的思考。花费来源分实报 / 按价格表估 / CLI 估 / 未知；没有就是 `usd: null`，不填 0。Claude Code 续接后报的是累计，取差值。
- **审批**：转换器只出 `approval.requested`；`approval.resolved` 由引擎那一层在站主回答以后发。选项统一成 `allow_once`、`allow_always`、`deny_once`、`deny_always`。`highRisk` 由 Peck 的 `harness/risk.rs` 定。
- **CLI 的模型列表**（`cli_models.rs`，IPC `cli_models`）：从 CLI 自己那里读，不起模型、不花额度——Codex `app-server` 的 `model/list`、Muse `serve` 的 `model/list`、Grok / Antigravity（ACP）`session/new` 回应里的 `configOptions`（模型和强度；会在它们那边留一个空会话）。Claude Code 没有列模型的接口，读它自己缓存的模型目录（配置目录下 `cache/model-catalog/*.json`，带版本号的模型名、强度档、现在选的哪个；常用的在前、旧版本在后），没有缓存才给 `--model` 认的别名；强度用 `--effort`。读到的存在 `app_settings` 的 `cli_models`，AGENTS 里点「重新读一遍」才重读；读不到照旧给上次的、带原因。发任务（`gateway_run_start`）和 ONE SHOT（`harness_oneshot`）都带 `model`、`effort`，不填用 CLI 自己的默认。
- **模型目录和价格**：`src-tauri/defaults/models.json`（ID、供应商、线协议、上下文窗口、最大输出、思考档位、工具和看图、每百万 tokens 的价格和日期），`node scripts/models-snapshot.mjs` 从 models.dev 重取。模型列表里 CLI 引擎带 `account`（当前账号名）和 `testedVersion`（录夹具时的版本，`catalog.rs` 的 `TESTED_VERSIONS`）；`directOnly` 为真的（Antigravity）不进 ONE SHOT 和任何自动化。
- **假 Gateway**（`mock.rs`）：`ScriptedModel` 照剧本出工具调用，`ReplayBackend::fixture(引擎)` 回放录好的夹具，碰到审批、提问停下等控制通道。浏览器 `?mock` 用 `src/dev/gatewayMock.ts`，读同一份夹具。
- **设置**：`gateway_config_get` / `_set` 存 Vertex 的 Project ID、Location 和「CLI 目录」。
- **找 CLI**（`config.rs` 的 `search_path`，检测、发任务、登录、找 gcloud 都用它）：「CLI 目录」→ 站主登录 shell 的 PATH（`term.rs` 打开 App 时读的那份；从访达打开的 App 自己的 PATH 几乎是空的）→ 常见的安装位置（`~/.local/bin`、Homebrew……）。「CLI 目录」空着时，打开 App 先同步找一遍旧清单（`agents/*.toml`）里写的 CLI 旁边的 `bin/`，找不到再在后台带上 shell 的 PATH 找，哪个目录里引擎最多（至少两家）就存成「CLI 目录」（`fill_cli_dir`）。检测结果带版本和位置。

### API 七家（`providers.rs` + `convert/{anthropic,responses,chat,gemini}.rs`）

`providers::for_model(state, 模型 id)` 拿一个 `HttpProvider`（Key 从钥匙串；模拟模式给 `ScriptedModel`）。不自动重发；错误是一句中文，带供应商自己的说明，不带 Key。思考块只在供应商、线协议、模型三样都一样时回放，换了就丢。

| 供应商 | 线协议 | 认证 | 要点（2026-11-12 实测） |
|---|---|---|---|
| Anthropic | Messages | Key | `anthropic-version: 2026-10-21`；思考和签名块原样回传；缓存读写进用量 |
| OpenAI | Responses | Key | `store:false`；加密思考和 function_call 原样回传，回放的项不带 `id`；用量里的 `cache_write_tokens` 算缓存写 |
| DeepSeek | Chat Completions | Key | `reasoning_content` 回传 |
| 阿里云百炼 | Chat Completions（Qwen 方言） | Key + 地域 | 只开关思考（`enable_thinking`），强度没接；历史里的 `reasoning_content` 不放回 |
| Gemini | 原生 `streamGenerateContent` | Key | 工具参数用 `parametersJsonSchema`；thought signature 挂在工具调用上，挂在文字上的跟一个空文字的 `Part::Reasoning` 回放；没给工具调用 id 就编 `peck-call-<n>`，回放时不发 |
| Vertex 上的 Claude / Gemini | Messages / 原生 | gcloud ADC | 令牌用 `gcloud auth application-default print-access-token` 换（45 分钟换一次，被拒就重换）；Claude 的 body 带 `anthropic_version: vertex-2023-10-16`；默认 `global`。**只有离线测试**，等站主开项目 |

### CLI 五家（`engines/` + `convert/`）

`engines::from_state(state, 引擎 id)` 拿（CLI 目录；账号就是客户端那一个；模拟模式回放夹具）。一次 `run` 第一条是 `session.started`、最后一条总是 `turn.completed`（引擎中途退出也补一条出错的）。Peck 不认识的引擎请求马上回错误，免得它干等。

| CLI（实测版本） | 接口 | PLAN / EDIT / AUTO | 审批和 ASK | 用量和额度 |
|---|---|---|---|---|
| Codex（0.167.2） | `codex app-server`（stdio JSON-RPC） | `read-only` + `untrusted` / `workspace-write` + `untrusted` / `workspace-write` + 联网 + `on-request` | `requestApproval` → 卡片；`requestUserInput` → ASK；追加 `turn/steer`、打断 `turn/interrupt`（10 秒没收尾结束进程） | `thread/tokenUsage`；额度 5 小时、每周 |
| Claude Code（2.1.322） | `claude -p` stream-json 双向 + `--include-partial-messages`；用站主的订阅，不加 `--bare`（照读项目的 CLAUDE.md、hooks、MCP） | `plan` / `acceptEdits` / `acceptEdits`（放宽靠 Peck 自动批） | `--permission-prompt-tool stdio`：审批和 `AskUserQuestion` 都是 `control_request`，Peck 回 `control_response`；追加一句再写一行 `user`；打断发 `interrupt`，10 秒后 SIGINT | `result` 里的花费（估）、用量、额度 |
| Muse（1.4.15 录的夹具，1.4.18 真跑核对过） | 会话协议 `muse serve`（每个命令一个 UUIDv7 的 `commandId`，`initialize` 后发 `initialized`） | `approvalMode`：`denyUnmatched` / `promptUnmatched` / `onRequest` | 它的审查员先看，拿不准的发 `approval/request`；自己放行的 Peck 记一条提示；ASK 是 `userInput/request` | `session/tokenUsage`、`usage/changed`（5 小时、每周） |
| Grok（1.0.77） | ACP（`grok agent stdio`）；模型、`reasoning_effort` 用 `session/set_config_option` | 没有模式，Peck 回答审批 | `request_permission` → 卡片；ASK 走 Peck 的本机 MCP | 回应里的用量和花费（`costUsdTicks`，1 美元 = 10¹⁰）；订阅余量命令行读不到 |
| Antigravity（1.5.6） | ACP（`agy acp`）；不需要终端 | `plan` / `accept-edits` / `auto` | 同 Grok | 用量；没有花费、没有余量 |

- **Antigravity 只做直接调用**（站主 2026-11-12 定，避开条款风险）：只在 NEW TASK 和接着发的那一句里能选；不进 ONE SHOT、接力、委派、审查、定时任务、目标模式、自动修。
- **本机 MCP**（`mcp.rs`，手写的最小服务）：只听 127.0.0.1，每个任务一个随机令牌，任务结束令牌作废、还在等的提问回「结束了」。工具只有 `ask_user`，只注入给 Grok、Antigravity（别家有自己的提问通道）。
- **CLI 的登录**（`accounts.rs`）：Claude Code、Codex 只用和客户端同一个账号——CLI 自己的默认位置（`~/.claude`、`~/.codex`），不加 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`（站主 2026-11-13 定，原来的多账号去掉了）。设置里能看登录状态、登录 / 重新登录（切换登录）：在 Peck 的伪终端里跑 CLI 自己的命令（`claude auth login --claudeai`、`codex login --device-auth`），`open` 和 `BROWSER` 换成只记链接的小脚本（数据目录的 `login-shim/`），站主自己挑浏览器去登；切了以后让客户端也是这个账号，站主自己统一。看状态只跑状态命令。Peck 不读、不复用任何凭据。
- **额度**：`EventSink` 收到 `quota.updated` 时写进 `usage.rs` 的额度卡片（`usage_record_gateway_quota`），顶栏 QUOTA 就能看到 Codex、Claude Code、Muse 的余量。
- **版本**：检测到的版本和 `testedVersion` 不一样时，设置的「CLI 检测 → 引擎」标 CHANGED、写「格式可能变了」。

## Peck 循环（`src-tauri/src/harness/`）

- **循环**（`loop.rs`，`PeckBackend` 实现 `AgentBackend`，引擎 id `peck`）：问模型 → 跑它要的工具 → 结果放回去，直到模型不再要工具。思考的私有数据原样放回下一轮。一个任务的对话存 `harness_conversations`，一问一答交替（连着两条用户消息并成一条）。
- **上限**：默认 40 轮；上下文到了压缩线先自动压缩（下面「上下文」），压完还超过窗口的 95% 才停；**花费上限**按这个月（UTC）Peck 循环和 ONE SHOT 在事件日志里的 `usage.updated` 加起来算，每次请求前现算（同时跑的任务花的也算进来）；设了上限时价格表里没有价格的模型不发；供应商没报用量也停。停下都是 `turn.completed` 的 `limit` + 一句原因。旧的 API 记录和 CLI 估的花费不算进上限。
- **十二个工具**（`tools/`；`bash_background`、`memory`、`search_tasks` 见下面两节）：`list_directory`（遵守 .gitignore，最多 300 条）、`read_file`（6 位行号 + tab，2000 行 / 50KB，记下读到时的哈希，能读这个任务落盘的命令输出；超过 8MB 的文件边读边数行、只留要的那段，也不记「读过」）、`search_text`（进程内 `ignore` + `grep-searcher`）、`find_files`（按修改时间，最多 100 条）、`write_file`（覆盖前要在这个任务里读过且没变）、`write_patch`（找原文 → 换新文：整批校验，临时文件 + 改名，中途失败写回原样；找不到给最像的几行和空白差异，多处列行号，宽松匹配行尾空白和中英文标点但仍要唯一）、`bash_run`、`update_plan`（同时一步在做）、`ask_user`。读改都只在项目里、不碰 `.git`。
- **命令**（`tools/bash.rs`）：`sandbox-exec`，默认联网；EDIT / AUTO 只能写项目、临时目录和 Peck 给的 HOME，`.git` 只读；PLAN 只能写临时目录；不能开别的 App（`lsopen`）、不能发 Apple Events。环境是站主 shell 的那份，去掉名字像 Key、令牌、密码、Cookie、Session 的变量和 `SSH_AUTH_SOCK`（站主 2026-11-13 定）；stdin 空，`CI=1`、`PAGER=cat`、`NO_COLOR=1`；默认 120 秒、最长 600 秒，跑完或超时结束整个进程组；跑命令的 future 被丢掉（验证命令作废、任务被打断）时 `process::GroupGuard` 也把整组杀掉。被沙箱拒了写项目外，弹高危卡片，批了出沙箱再跑一次。输出超过 16KB 整份落盘（`spill.rs`），上下文只留开头 2KB、结尾 6KB（失败时 1KB、7KB）和路径，界面能点开（`harness_output_read`）。grep、rg、diff 的退出码 1 不算失败。
- **权限三档**（`permission.rs`）：PLAN 只给读的工具、`update_plan`、`ask_user`，命令在只读沙箱；EDIT 全部工具，装依赖、起常驻进程前问一次（能选「这个项目总是允许」，存 `harness_allow`；设置 → 权限里列出来、能撤：`harness_allow_list` / `_remove`）；AUTO 除了高危都不问。
- **高危**（`risk.rs`，不管哪档都弹，只有批准、不行）：删文件（`rm`、`git clean`、`find -delete` / `-exec(dir) rm`、丢掉没提交的改动……）、推送和改历史（`git push`、`git -c alias.*`、`reset --hard`、`branch -D`、`rebase`、`commit --amend`……）、写到项目外（重定向、`cp`、`mv`、`tee` 的目标在项目外）、改系统设置（`sudo`、`defaults write`、`launchctl`……）、看不出会做什么的命令（命令名是展开出来的 `$CMD` / `$(…)`、`eval` 带展开、`… | sh`、`find -exec sh`、解释器代码里删文件或起进程）。命令先按 shell 规矩拆段，`sh -c '…'`（`-ec`、`-xc` 这类也算）、`$(…)` 里的也看。规则是黑名单，挡常见写法；兜底的是沙箱和撤销。
- **任务记录**（`run.rs`）：和 CLI 引擎一样占任务号、一个项目一次一个、任务前快照；`runs.engine` 记 `peck`。`harness_spend` 给这个月花了多少。

### 上下文（`context.rs`、`compact.rs`、`history.rs`）

- **分层和缓存断点**：系统提示两块，都打缓存断点：Peck 自己的说明（档位、怎么干活）+ 工具定义；项目层 = 项目说明（项目根的 `AGENTS.md`，没有就 `CLAUDE.md`；空文件当没有；指到项目外的符号链接不认；最多 32KB，多了截掉写明）+ 站主批过的记忆。Anthropic 再在对话最后一块打一个断点（`convert/anthropic.rs`：系统提示打了断点才打，一共 3 个）；OpenAI、DeepSeek、Gemini 是自动前缀缓存，不用打。
- **每次发一句话带上的环境**（`<peck-context>`，放在这句话前面、不进系统提示，免得每次打穿缓存）：系统和项目根、git（分支、没提交的改动最多 20 条、最近 5 个提交；`GIT_OPTIONAL_LOCKS=0`，不碰用户仓库的 index）、最近一份任务清单、这个任务之前几次改过的文件（撤销了的不算）、验证命令、目标模式、在跑的后台命令。ONE SHOT 不带环境块，但有项目层。
- **大小**：每次请求前粗估一遍（中日韩字一个算一个 token，其他四个字符一个），按上一次实报的提示大小校准，发 `context.updated`（四块：Peck 的说明和工具、项目说明和记忆、对话和思考、工具的输出；窗口和压缩线），回合结束再报一次。
- **自动压缩**：估到窗口的 80%（压缩线）时，下一次请求前按顺序做，降到 60% 以下就停：① **落盘**：除了最后一条，比 3KB 长的工具输出整份写进这个任务的输出目录（`ctx-<n>.log`，已有的不覆盖），上下文里留开头结尾各 800 字节和路径（`read_file` 读得回来）；② **清旧工具输出**：最近两轮以外的工具输出换成一句「做过什么」，站主最近一句以前的思考、旧的环境块去掉（工具调用都留着，对话接得上；这一回合的思考留着，DeepSeek 要原样传回）；③ **交接摘要**：同样的系统提示、工具、对话（命中缓存）后面接一段要求，让模型写交接摘要（不显示成回答，花费照算，输出上限 16000），对话换成「摘要 + 站主最近说的三句」；没写出来或被截断就不换，对话原样留着、提示一行。每一步发 `context.compacted`。压完还超过 95%、或者刚压过一轮又满了，停下写明原因。
- **记忆**：`memory` 工具只发 `memory.proposed`；任务结束时中栏一张「想记住这几条」，站主勾了 SAVE（IPC `memory_review`）才存进 `memories`（按项目，一字不差的不重复存），事件日志里追加 `memory.reviewed`（卡片据此收起）。项目设置里能看、能删（`memory_list` / `memory_delete`）；换文件夹时跟着项目走。
- **@ 旧任务**：输入框里打 `@` 列出这个项目的旧任务，点一个插进 `[@标题](peck:task/<任务 id>)`；也认手写的 `@任务 12`。发出去时 `history::task_refs` 把那个任务（最近 5 次）问了什么、结果、改了哪些文件做成 `<referenced-task>` 放在这句话前面（最多 4 个，只认这个项目的）；CLI 引擎看到的这一句前面也带上，任务记录里照旧是站主的原话。
- **搜历史任务**：`search_tasks`（关键词都要出现在标题、请求、结果或文件路径里，最新的 10 个；或者按任务号看一次的详情），ONE SHOT 也有。

### 后台和验证（`verify.rs`、`background.rs`、`monitor.rs`）

- **`.peck/verify.toml`**：`[[check]]`（name、run、when：改了哪些文件才跑的 glob，空着是都跑、timeout：秒，默认 300、最长 3600）最多 8 条；`[[server]]`（name、run、ready：输出里出现这段算起好了、autostart）最多 4 个。写错了照常跑任务、不看验证，开头记一条提示。第一次打开项目（没有这份文件、没说过不要）时按 `package.json` 的 scripts（check / typecheck、lint、test、dev；照锁文件认 pnpm / yarn / bun / npm）、根目录或下一层的 `Cargo.toml`、`go.mod`、`pyproject.toml` / `setup.py`、`Makefile` 的 test 猜几条（不跑任何命令），站主在 NEW TASK 上确认（能改）才写（`verify_get` / `verify_save` / `verify_dismiss`）。**只认站主确认过的那一份**（`verify::load_approved` 比 md5）：项目自带的、模型或别人改过的，不替模型跑、不起 dev server、不开目标模式，NEW TASK 上换成「还没确认的验证命令」。
- **规则为主**：Peck 循环（不是 PLAN、不是 ONE SHOT）每轮跑过会改东西的工具后，比一遍项目文件（大小 + 修改时间，照 `.gitignore`，最多 5 万个；再多就只认工具报的 Diff），改到了哪条的 `when` 就在后台起它（项目沙箱、命令的环境同 `bash_run`），模型接着干活。
- **并发过期**：一条验证命令跑的时候它管的文件又改了：停掉，记一条 `stale`，按新文件再跑。
- **模型停下时**：要看的那几条（目标模式是目标那几条；平时是这个任务改过的文件管得到的）——结果是新的直接用，正在按最新文件跑的等它，没跑过的现在跑。都过了：发 `verify.round` passed、收尾，不再叫模型（中栏挂 ✓）。没过：把失败的命令、退出码、输出结尾 3KB 和完整输出的路径包成 `<peck-verify>` 交回模型修，最多 3 次（目标模式 8 次）；**同一个失败连续两次**（命令名 + 退出码 + 像错误的那几行，去掉耗时和地址）或者**两次之间没改文件**都停下（`limit`，写明原因）。没有要看的就照常收尾。
- **后台命令通道**（`bash_background`，`start` / `read` / `stop` / `list`）：最长 12 小时；起的时候照 `bash_run` 的规矩（高危弹卡片；EDIT 下这条命令自己要问的，比如装依赖，照问，再当「起常驻进程」问一次，都能「总是允许」）；起完等 1.5 秒把开头的输出一起给模型；`read` 给上次读过以后的新输出（多了留开头结尾，完整的写在日志文件，`read_file` 读得到），能等最多 60 秒；一个任务最多同时 4 个；任务结束一起停。
- **常驻 dev server**（`[[server]]`）：站主在输入框旁的 DEV 里起停（`server_start` / `background_stop`），不限时、跨任务一直在，退出 Peck 才停（`RunEvent::Exit` 停整个进程组）；起之前加锁，同时点不会起两份。`autostart = true` 的发任务时没在跑就先起（PLAN 不起；拍完任务前快照再起，它写的文件不算任务的改动）；模型能读它的输出，不能停它。项目有 server 时界面每 2.5 秒读一次状态（`background_list`，窗口藏着不读）。
- **目标模式**：发任务时开 GOAL 选一条（或全部），只给 Peck 循环、不能是 PLAN；每次模型停下都跑目标那几条，过了才收尾。Antigravity 只做直接调用，不进目标模式和自动修。
- **CLI 引擎**：不替 CLI 引擎跑验证、不自动修（它们自己会跑命令；要做的话在 Gateway 的 runner 收尾后接上，记在 HANDOFF）。PLAN 也不跑验证，环境块里不提验证命令。

## 审阅和预览

- **FILES**（`files.rs`、`src/lib/highlight.ts`）：代码上色用 CodeMirror 的语言包（JS / TS / JSX、Rust、Python、CSS、HTML、JSON、YAML、Markdown，TOML、Shell、Go、Swift 走 legacy-modes），按 `@lezer/highlight` 的 `classHighlighter` 出 `tok-*` 类名，颜色在 `src/app.css`（`--code-*`，云端先定的 Peck 配色）；语言包按需加载，超过 20 万字符不上色。md、html、svg 能切 VIEW / SRC（html 在没有脚本的 sandbox iframe 里，前面加一条 CSP，不从网上取东西、`meta refresh` 去掉）。图片和 PDF 读成 data URL（最多 24MB；PDF 换成 blob 地址给 iframe）；Word、Excel、PowerPoint、iWork、RTF、OpenDocument 交给 Quick Look：`qlmanage -t` 出一张缩略图（最多等 20 秒），QUICK LOOK 按钮 `qlmanage -p` 叫出系统窗口（只在 Mac 上）。独立工作区没搬回时 FILES 列的是主文件夹，不标那个任务的改动。
- **CHANGES 按块接受 / 拒绝**（`snapshot.rs` 的 `parse_hunks` / `revert_hunk`，IPC `changes_hunk_revert`）：拒绝一块是在现在的文件里找到这一块任务后的样子（带上下文；好几处对得上时照行号挑），换回任务前的；后来又改过、对不上就不动；任务新建的文件整块拒掉就删掉，删掉的文件拒掉就放回来（站主后来又建了同名文件就不动）；换行照文件原来的（CRLF 不变）。只认这个任务改过的文件，在跑、撤销了、独立工作区丢掉了都不行；没搬回的独立工作区改的是它里面的文件。接受只是标一下。接受 / 拒绝的标记存在界面这台机器的浏览器存储里（最多 2000 条），批注按任务存、删任务时清掉。
- **行内批注**：点 Diff 的行号写一条，攒在这个任务下（浏览器存储），TO TASK 把它们整理成一段（文件、第几行、那一行写的什么、批了什么）放进中栏输入框，站主再发。

## 旧的 API 层（`src-tauri/src/api/`）

规划 + 实施和授权账本还在这里，v0.8 并进 Gateway 和 Peck 循环。

- **Key**（`keys.rs`，Gateway 也用）：Rust 读钥匙串，IPC 不返回 Key，不进配置、历史和模型上下文。按「包」存：AI 供应商的一包、其他的（`OTHER_KEYS`，以后新加的都放这里）一包，一包一个钥匙串项，所以 Peck 每次更新（签名变了）系统最多问两次授权。打开 App 就在后台读两包（`Keys::load_unlocked`，读的时候不占锁，授权窗口这时弹出），读完发 `keys-loaded` 让界面重读模型列表和 Key 状态；拒了可以在设置里点 READ KEYS 再问。模拟模式的 Key 只在内存。
- **请求**（`provider.rs`）：每次 HTTP 发送都走 `request_once`，发送前在授权账本里占一次，没授权的模型拒绝发送；不隐式重试、降档、重定向。Gemini 在这里还是 OpenAI 兼容接口。
- **工具和上限**（`tools.rs`、`worker.rs`）：只接受相对路径；命令在 `sandbox-exec` 里、默认禁网、最长 180 秒。单模型 12 轮、规划 + 实施 24 轮，整个任务 80 次工具、18 万上下文字节、50 万 tokens；最多委派 4 项，每项返工 1 次。
- **导出**：调用记录能导出成脱敏 JSON（`schemaVersion=1`），不含 Key、绝对路径、提示词、回复、思考、工具参数。

## 额度与花费

- **Claude 的订阅额度**（2026-11-13 改）：Claude 桌面 App 或 Claude Code 在跑时，三分钟一次跑 Claude Code 自带的 `/usage`（`claude -p /usage --output-format json --no-session-persistence`，`TZ=UTC`，在 Peck 数据目录的 `cli-probe/` 里跑）：它是本地命令，0 轮、不进模型、不花额度，由 Claude Code 自己去问订阅用了多少，Peck 不读它的登录；解析 `Current session / Current week …: N% used · resets …` 那几行。要 Claude Code 命令行按订阅登录（桌面 App 的登录和它不共用；没登录时卡片上写怎么登）。status line 送来的（终端里跑 Claude Code 才有）作补充。
- 现在的 `usage.rs` 和用量页照旧可用：API 记录、DeepSeek 余额（只在真实 DeepSeek 请求进行中且 Key 已加载时查）、五家客户端额度卡（每 15 秒看进程，约 60 秒查一次，客户端退出就停、留最后一次的快照，缺数据不填 0），加上 Gateway 写进来的 CLI 额度。
- Claude Code、Antigravity 的额度另有官方状态栏脚本：随 App 打包在 `Peck.app/Contents/Resources/integrations/statusline.py`，往 Claude Code 设置里**合并**（已有状态栏就在命令末尾加 `--forward-original '原有命令'`）：
  ```json
  {"statusLine":{"type":"command","command":"/usr/bin/python3 '/Applications/Peck.app/Contents/Resources/integrations/statusline.py' --client claude-code"}}
  ```
- **QUOTA 页的图**（`spend.rs` 的 `spend_overview`）：从事件日志取近 7 / 30 / 90 天的 `usage.updated`（Peck 循环按任务的「谁来干」认模型，ONE SHOT 单算，CLI 引擎的标「估」，累计口径的取差值）和 `quota.updated`（订阅额度的历史读数），按天、按谁汇总在界面里做（界面知道本地时区）。旧的规划 + 实施的调用记录照旧来自 `usage.rs`，收在页底。
- 「额度与花费」就是这一页（`usage.rs`、`spend.rs`、`UsagePage.tsx`），本地账号接着做（站主 2026-11-13 改，不再另建 `src/quota/`）。各家额度的官方来源见 [testing.md](./testing.md#额度与花费的真实来源)。

## 视觉

- 视觉基线在 visual-atelier 的方案 `schemes/peck/`，按 tag 单向同步进 `src/vendor/peck/`（现在是 `peck-v0.1.0`，`VERSION` 记着提交，`SHA256SUMS` 记校验和），不手改；同步命令在 `CLAUDE.md`「边界」。
- 读取：`src/main.tsx` 依次引 `vendor/peck/peck.css`（变量、两个本地字体）、`vendor/peck/bundle.css`（`pk-` 组件）、`@fontsource/doto` 和 `@fontsource/ibm-plex-sans`（打进包里，App 不联网取字体）、`src/app.css`（外壳怎么摆，只用变量）。根元素的 `data-theme` 由 `src/lib/theme.ts` 写，默认跟随系统。
- 同步来的还有 `tokens.ts`、`marks/`（App 图标 J2 和 A3、菜单栏模板图）、`DESIGN.md`、`peck-rules.md`、`DESIGN-SYSTEM.md` 和 `components/`。云端开发看不到 visual-atelier，照这些做。

## 打包和签名

- `pnpm app:package` 只打包、校验签名；`pnpm app:install` 打包并装进 /Applications，旧包备份成 `Peck.app.backup-<时间>`，只留这一份，更早的挪进废纸篓。`PECK_INSTALL_DIR` 改安装目标，`PECK_BUILD_PROFILE=debug` 打开发包（带 Web Inspector）。产物在 `src-tauri/target/release/bundle/macos/Peck.app`。
- 默认用本机自签名证书 **Peck Self-Signed**；`APPLE_SIGNING_IDENTITY` 换别的，设 `-` 是 ad-hoc。只给本机用，不是 Developer ID、没公证。
- 版本号三处一起改：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。

## 代码位置

| 位置 | 职责 |
|---|---|
| `src-tauri/src/{lib,commands,state,events}.rs` | 启动和恢复、IPC、共享状态、推给界面的事件 |
| `src-tauri/src/gateway/` | Model Gateway：契约（`mod`、`event`、`caps`、`catalog`）、事件日志（`log`）、七家 API（`providers` + `convert/`）、五家 CLI（`engines/` + `convert/`）、CLI 的登录和模型列表、本机 MCP、Gateway 版发任务（`runner`）、假 Gateway（`mock`） |
| `src-tauri/src/harness/` | Peck 循环：`loop`、`tools/`、`permission`、`risk`、`spill`、`run`（发任务、ONE SHOT、花费）；上下文 `context`、`compact`、`history`（记忆、@ 旧任务、搜历史）；后台和验证 `verify`、`background`、`monitor` |
| `src-tauri/src/{run,flow,agent,process,mock,mode}.rs` | 旧的任务派发、接力、CLI 清单、子进程、假 Agent、模式 |
| `src-tauri/src/api/` | Key、授权账本、旧的请求层、规划 + 实施和它的工具、API 记录 |
| `src-tauri/src/{store,migrations}.rs`、`src-tauri/migrations/` | 历史存储和追加迁移 |
| `src-tauri/src/{workspace,snapshot,watch,tasks,worktree}.rs` | 项目（含其他文件夹、环境变量的校验）、任务前快照和撤销（含其他文件夹、按块拒绝）、文件监听、`.peck/tasks.md`、独立工作区 |
| `src-tauri/src/{projects,spend}.rs` | 左栏的改名、项目设置、移出列表、重新找文件夹、删任务、顺序；QUOTA 页的图表数据 |
| `src-tauri/src/{files,term,tray,app_icon}.rs` | 右栏 FILES（预览、Quick Look）、TERM 的 pty、菜单栏图标（在跑的个数）、Dock 图标 |
| `src-tauri/src/{usage,export}.rs`、`src-tauri/integrations/` | 旧的用量监控、导出、状态栏脚本 |
| `src-tauri/defaults/`、`src-tauri/tests/fixtures/gateway/` | 内置 CLI 清单、流程、模型目录；Gateway 的夹具 |
| `src/App.tsx`、`src/lib/` | 界面编排和状态（左栏「待你处理、在跑」从这里算）、类型化 IPC（`ipc.ts` 在浏览器里转给 `?mock`）、设置、统一事件 → 界面（`agentView.ts`）、代码上色（`highlight.ts`） |
| `src/shell/`、`src/ui.tsx`、`src/app.css` | 三栏外壳：顶栏、左栏、左栏下面的 `AgentsPanel`（谁来干）、中栏（`AgentView`、`ModelPicker`（FLOW 的判断、发出去带什么；ONE SHOT 和项目设置还用它的三个按钮）、`Markdown`、`MarkdownEditor`）、右栏四个页签、设置、用量页；布局样式和方案里还没有的身份色 |
| `src/dev/` | `?mock` 的假数据和假 IPC：`peckMock.ts` 是 Peck 循环的剧本，`gatewayMock.ts` 读 CLI 引擎的夹具 |
| `src/vendor/peck/` | 同步来的视觉（不手改） |
