# 技术架构

> 界面在 [design.md](./design.md)，怎么测在 [testing.md](./testing.md)。

## 选型

| 层 | 用什么 | 为什么 |
|---|---|---|
| 外壳 | Tauri 2（macOS） | 原生窗口、钥匙串、菜单栏、通知；包小 |
| 界面 | React 19 + TypeScript；样式只用 `src/vendor/peck/` 的变量和 `pk-` 组件加 `src/app.css`；Vite 固定端口 1430 | 浏览器 `?mock` 能自检；视觉只有一个来源 |
| 后端 | Rust | 模型请求、工具、CLI 进程、快照、SQLite 都在这里，界面只经 IPC |
| 存储 | SQLite（WAL）+ 影子 Git 仓库 | 记录和历史 Diff 分开存 |

不做：常驻辅助服务、通用插件框架、开机自启。

## 一个任务怎么跑

一句话对应一个**任务**（代码里叫 Run），开始前拍**任务前快照**，跑完留着，撤销由站主点。一个项目同时只跑一个任务（要写的文件夹一样或套着也算，`ActiveRunContext::touches`），不同项目能同时跑。失败、取消、重开都不自动重发。

「谁来干」是模型列表（IPC `gateway_models`）里的一个 id：

| id | 谁干 | 入口 |
|---|---|---|
| `api:<供应商>/<模型>` | Peck 循环 + 这个 API 模型 | `harness_run_start` |
| `cli:<引擎>` | CLI 引擎 | `gateway_run_start` |
| 接力、审查 | 两三个合作，任务上记 `team:<写规范>\|<实施>\|<审查>` | `team_run_start` |
| ONE SHOT | Peck 循环只给读的工具，或 CLI 按 PLAN 档；不占任务号、不锁项目和终端 | `harness_oneshot` |

- **直接调用的引擎**（`caps.rs` 的 `direct_only`，现在是 Antigravity）挡在 ONE SHOT、接力、审查、委派、定时任务、目标模式、自动修之外。
- **发出去前**（`src/App.tsx` 的 `send`）：存着的接力、审查、委派只认 AGENTS 里现在能选的；GOAL 不能和接力、审查一起发，PLAN 不能开接力和审查，都提示一句、不悄悄丢掉。
- **斜杠命令**（`src/lib/slash.ts`、`skills.rs`）：Peck 自己的 `/plan` `/edit` `/auto` `/new` `/review` `/compact` `/undo` `/verify` `/cost`，加上项目和站主的 `.claude/commands/**/*.md`（同名项目的为准）。`slash_expand` 换掉 `$ARGUMENTS`、`$1`…`$9`，`!` 命令不跑，展开的全文当这句话发出去，哪家引擎都能用。CLI 自己的交互式命令不转发。
- **控制通道**：审批、回答、追加、打断都走 `gateway_control(runId, control)`（`gateway/runner.rs`）。`run_cancel` 先发打断，超时结束整个进程组。
- **撤销**（`run_undo`）：新建的删掉，其余 `git restore --source=<任务前的树> --worktree`；不碰用户仓库的 index、commit、stash。撤销前再存一份快照。
- **项目和任务**（`projects.rs`）：「移出列表」只设 `Workspace.hidden`，「归档」只设 `sessions.archived_at`（`search_tasks`、@ 引用照样读得到）；删任务真删；在跑的不能归档、不能删。
- **文件夹不在了**（`ProjectOverview.missing`）不能发任务；`workspace_relocate` 把任务、「总是允许」和快照挪到新位置。
- **旧记录**：v0.8 以前的任务（旧接力、规划 + 实施、假 Agent、旧 API 层）只读，照旧按步骤显示；接着做是用现在选的谁发一句新的。旧 API 调用记录在 QUOTA 页底。

### 项目即环境

项目设置里的其他文件夹（各选只读或能改）和环境变量（`ProjectSettings.folders` / `env`，`workspace_update` 校验）。

- 不能加（`workspace::folder_allowed`）：和主文件夹或彼此套着的、根目录、家目录、数据目录、`.git` 里的。`HOME`、`SHELL`、`PECK_*` 这类变量不能设。
- Peck 循环只写能改的；能改的各有影子仓库和快照，改动进 CHANGES、一起撤销。环境变量带进它的命令、后台命令、验证命令、dev server 和 TERM。
- CLI 引擎：环境变量带进 CLI 进程；能改的文件夹只对这一次进程挂上（Claude Code `--add-dir`、Codex `writableRoots`），并在这一句后面写一段 `<peck-folders>`。

### 独立工作区（`worktree.rs`）

- git 项目、不是 PLAN，一次一选。其他文件夹一律只读。
- 建：`git worktree add --detach <数据目录>/worktrees/<项目哈希>/<任务号> HEAD`（不建分支），再把主文件夹没提交的改动搬进去。
- APPLY：要搬的文件在主文件夹里和任务开始时不一样就列成冲突、不动，可 APPLY ANYWAY；撤销回到搬之前。
- DISCARD：删掉，历史 Diff 还在。上一次的没 APPLY 或 DISCARD 不能接着发。

### 定时任务（`schedule.rs`）

项目设置的 SCHEDULE，一个项目最多 12 个，各用一个 API 模型或 CLI；到点开一个新任务（标题前 ⏰）。App 开着才跑；关着错过的 12 小时以内补一次；项目有任务在跑就等，等 2 小时跳过。受花费上限管。

## 数据存放

| 内容 | 位置 |
|---|---|
| 数据目录 | 正式 `~/Library/Application Support/Peck/`；开发（`pnpm app:dev`）`Peck Dev/` |
| 项目、任务、消息、事件日志、设置、记忆、定时任务 | `peck.db`（schema 11，迁移在 `src-tauri/migrations/`）；设置在 `app_settings`，一样一行 JSON |
| 项目设置 | `workspaces` 表那一行的 JSON（加字段不加迁移） |
| 验证命令 | 项目里的 `.peck/verify.toml`（打开项目时把 `.peck/` 加进 `.gitignore`） |
| 快照、历史 Diff | `shadow/<项目哈希>/`，其他文件夹各自 `shadow/<文件夹哈希>/` |
| 独立工作区 | `worktrees/<项目哈希>/<任务号>/` |
| Peck 循环命令的 HOME | `harness-runtime/<项目>/` |
| 落盘的输出 | `harness-output/<任务>/`（长输出、验证输出、后台日志、附图、截图），14 天没动的打开 App 时删 |
| 其他 | `skill-history/`（Skill 旧版）、`login-shim/`（CLI 登录时只记链接的小脚本）、`cli-probe/`（跑 Claude `/usage`）、`agents/*.toml`（旧清单，只用来找 CLI 目录） |
| Key | 钥匙串服务 `Peck API` / `Peck API Dev`（开发包）/ `Peck API Test`（设了 `PECK_DATA_DIR`） |

- `PECK_DATA_DIR`、`PECK_SHADOW_ROOT` 把数据和快照指到别处；`PECK_API_MODE=mock|real` 强制模拟或真实，开发包和设了 `PECK_DATA_DIR` 的默认模拟。
- **迁移和升级**：改数据库结构就在 `src-tauri/migrations/` 加迁移，不改旧记录的含义；升级前先用 SQLite Backup API 备份；比自己新的 schema 拒绝写入。
- **影子仓库**：独立的 `GIT_DIR`、`GIT_WORK_TREE`，每个任务存前后两棵树。`peck.db` 和 `shadow/` 要一起备份。

## Model Gateway（`src-tauri/src/gateway/`）

各家 API 和 CLI 转成同一套事件。契约：`mod.rs`（`ModelProvider` 裸模型、`AgentBackend` 整包引擎、控制通道）、`event.rs`、`caps.rs`、`catalog.rs`；TS 对照 `src/lib/agentEvents.ts`。

- **事件**：`AgentEvent` = `seq` + `at` + 可选 `subagentId` + 本体（`session.*`、`turn.*`、`message.*`、`tool_call.*`、`approval.*`、`stage.*`、`verify.*`、`usage.updated` 等，全的在 `event.rs`）。`EventSink`（`log.rs`）编号、存 `agent_events`、交给界面；`gateway_events(runId)` 读。
- **用量**：`input` 不含缓存，`cacheRead`、`cacheWrite` 另算；`output` 含思考。花费分实报 / 按价格表估 / CLI 估 / 未知，没有就 `usd: null`，不填 0。
- **模型目录**：`src-tauri/defaults/models.json`，`node scripts/models-snapshot.mjs` 从 models.dev 重取。
- **价格表**（`prices.rs`）：花费都按「用量 × 价格表」算。内置 `src-tauri/defaults/prices.json`（美元价取 models.dev，DeepSeek、Aliyun 是官网人民币价），站主改的、刷新来的、汇率存 `app_settings`，盖在内置的上面。`prices_refresh` 只更新美元价，站主改过的行不动。
- **CLI 的模型列表**（`cli_models.rs`）：读各家自己的列表，不花额度；空着用 CLI 默认。
- **假 Gateway**（`mock.rs`）：照剧本出工具调用的假模型、回放夹具的假引擎（`?mock` 读同一份夹具）。
- **找 CLI**（`config.rs` 的 `search_path`）：「CLI 目录」→ 站主登录 shell 的 PATH → `~/.local/bin`、Homebrew 等；「CLI 目录」空着时自己找引擎最多的目录存下。

### API 七家（`providers.rs` + `convert/{anthropic,responses,chat,gemini}.rs`）

Key 从钥匙串取。不自动重发；错误是一句中文加供应商的说明，不带 Key。思考块只在供应商、线协议、模型都一样时回放。

| 供应商 | 线协议 | 认证 | 要点 |
|---|---|---|---|
| Anthropic | Messages | Key | 思考和签名块原样回传；缓存读写进用量 |
| OpenAI | Responses | Key | `store:false`；加密思考和 function_call 原样回传 |
| DeepSeek | Chat Completions | Key | `reasoning_content` 回传 |
| 阿里云百炼（界面写 Aliyun） | Chat Completions（Qwen 方言） | Key + 地域 | 只开关思考 |
| Gemini | `streamGenerateContent` | Key | thought signature 回放 |
| Vertex 上的 Claude / Gemini | Messages / 原生 | gcloud ADC | 令牌用 `gcloud auth application-default print-access-token` 取 |

### CLI 五家（`engines/` + `convert/`）

一次 `run` 第一条 `session.started`，最后一条总是 `turn.completed`（中途退出也补）。录夹具时的版本在 `catalog.rs` 的 `TESTED_VERSIONS`。

| CLI | 接口 | PLAN / EDIT / AUTO | 审批、ASK、追加、打断 | 用量和额度 |
|---|---|---|---|---|
| Codex | `codex app-server`（stdio JSON-RPC） | `read-only` / `workspace-write` / 再加联网 | `requestApproval`、`requestUserInput`、`turn/steer`、`turn/interrupt` | 用量；5 小时、每周 |
| Claude Code | `claude -p` stream-json 双向 | `plan` / `acceptEdits` / `acceptEdits`（放宽靠 Peck 自动批） | `--permission-prompt-tool stdio`；追加写一行 `user`；`interrupt` | 花费（估）、用量、额度 |
| Muse | `muse serve` | `approvalMode`：`denyUnmatched` / `promptUnmatched` / `onRequest` | `approval/request`、`userInput/request` | 用量；5 小时、每周 |
| Grok | ACP（`grok agent stdio`） | 没有模式，Peck 回答审批 | `request_permission`；ASK 走本机 MCP | 用量和花费；读不到余量 |
| Antigravity | ACP（`agy acp`） | `plan` / `accept-edits` / `auto` | 同 Grok | 用量；没有花费、余量 |

- **Peck 替 CLI 批命令**（`runner.rs` 的 `rule_allows`）：EDIT、AUTO 里 CLI 要跑的命令先过 Peck 循环同一套 `permission::check_command`，这一档 Peck 自己也不会问的直接批、不弹卡片；装依赖、起常驻进程、高危的照旧给站主。PLAN、改文件、MCP 的审批不替它批。
- **项目里的 MCP**（`project_mcp.rs`）：CLI 自己读项目的 `.mcp.json`，要站主先在它的客户端里信任过这个文件夹。任务开头查一次，配了却没读到的写一行提示；Antigravity 不查。
- **本机 MCP**（`mcp.rs`）：只听 127.0.0.1，每个任务一个随机令牌。工具：`create_skill`（五家都给）、`ask_user`（Grok、Antigravity）、`delegate_task`（选了委派才给）。只对这一次进程挂上、不改 CLI 的配置；Muse 要那个文件夹在它那里信任过才连，没信任 Peck 在任务开头提示。
- **登录**（`accounts.rs`）：Claude Code、Codex 只用和客户端同一个账号。LOGIN / RELOGIN 在伪终端里跑它们自己的登录命令，`open` 换成只记链接的小脚本，站主自己挑浏览器登。

## Peck 循环（`src-tauri/src/harness/`）

- **循环**（`loop.rs`，`PeckBackend`，引擎 id `peck`）：问模型 → 跑工具 → 结果放回，直到不再要工具。对话存 `harness_conversations`。
- **上限**：默认 40 轮，设置 → GENERAL 能调（手动、合作、定时任务都按它）；ONE SHOT 12 轮、委派的子任务 24 轮。
- **花费上限**：这个月（UTC）事件日志里 Peck 循环和 ONE SHOT 的 `usage.updated` 合计，每次请求前现算；设了上限时没价格的模型不发，供应商没报用量也停；CLI 估的不算。停下是 `turn.completed` 的 `limit` + 原因。

### 工具（`tools/`）

读改只在项目和它的其他文件夹里，不碰 `.git`。

| 工具 | 要点 |
|---|---|
| `list_directory`、`search_text`、`find_files`、`read_file` | 照 .gitignore；`read_file` 记下哈希，能读本任务落盘的输出 |
| `write_file` | 覆盖前要在本任务读过且没变 |
| `write_patch` | 找原文换新文，整批校验，失败写回；原文要唯一 |
| `apply_patch`（`v4a.rs`） | OpenAI 线协议（GPT）代替 `write_patch`：V4A 补丁；删文件当高危 |
| `update_plan`、`ask_user` | 任务清单；停下来问站主 |
| `screenshot_app`（`shot.rs`） | Mac、能看图的模型，PLAN 也有：截这个 App 最前面的窗口 |

其余（`bash_run`、`bash_background`、`memory`、`search_tasks`、`delegate_task`、`load_skill`、`create_skill`、`mcp__*`）见下面各节。**附图**最多 6 张：Peck 循环放进这一句（看不了图的模型不发、写一句），CLI 引擎存成文件、在句末写路径。

### 命令（`tools/bash.rs`）

- `sandbox-exec`，默认联网；EDIT / AUTO 只能写项目、临时目录和 Peck 给的 HOME，`.git` 只读；PLAN 只写临时目录；不能开别的 App、发 Apple Events。沙箱拒了写项目外就弹高危卡片，批了出沙箱再跑。
- 环境是站主 shell 的，去掉名字像 Key、令牌、密码、Cookie、Session 的变量和 `SSH_AUTH_SOCK`。
- 默认 120 秒、最长 600 秒，结束整个进程组。输出太长整份落盘（`spill.rs`），上下文留开头、结尾和路径。

### 权限和高危（`permission.rs`、`risk.rs`）

- **PLAN** 只给读的工具、`update_plan`、`ask_user`，命令在只读沙箱。**EDIT** 全部工具，装依赖、起常驻进程前问一次（「这个项目总是允许」存 `harness_allow`）。**AUTO** 除高危都不问。
- **高危**（哪档都弹，只有批准、不行）：删文件、推送和改历史、写到项目外、改系统设置、看不出会做什么的命令。`sh -c`、`$(…)` 里的也看。黑名单只挡常见写法，兜底靠沙箱和撤销。

### 外部 MCP 服务器（`mcp_client.rs`）

- stdio，设置里最多 8 台；要保密的变量只记名字，值在钥匙串。Peck 循环发任务时起（ONE SHOT 不起），任务结束停；起不来的写一行、照常干活。在沙箱外跑。CLI 引擎用自己配的 MCP。
- **项目的 `.mcp.json`**：Claude Code 的格式，只接 stdio 的，和设置里的合并（同名以项目的为准，项目设置里能关）。第一次起之前弹一张卡片，「总是允许」按这一项原文的指纹记，配置改了再问。
- **权限**：PLAN 只给 `readOnlyHint` 的；EDIT 用其他的前问一次（能总是允许这台）；AUTO 不问；会删改的（标了 `destructiveHint`，或只读、删改都没标）哪档都当高危。
- **工具标注**（`mcp_hints`）：Peck 起过或 TEST 过的服务器记下每个工具是只读、会改还是会删改。CLI 经 ACP 问要不要调 MCP 工具时照它替 CLI 批（`runner::mcp_hint`）：只读的批；会改的 AUTO 批、EDIT 下这台「总是允许」过才批；会删改的、查不到的给站主。

### 上下文（`context.rs`、`compact.rs`、`history.rs`）

- **系统提示**两块，都打缓存断点：Peck 的说明 + 工具定义；项目层 = 项目根的 `AGENTS.md`（没有就 `CLAUDE.md`）+ 站主批过的记忆 + Skills 的名字和描述。
- **环境块** `<peck-context>` 放在这一句前面：系统和项目根、git、任务清单、之前几次改过的文件、验证命令、目标模式、在跑的后台命令。
- **自动压缩**：到窗口 80% 时，下一次请求前依次做，降到 60% 以下就停：① 长的工具输出落盘；② 最近两轮以外的输出换成一句「做过什么」；③ 让模型写交接摘要，对话换成「摘要 + 站主最近三句」。压完还满就停下写明原因。
- **记忆**：`memory` 工具只提议；站主 SAVE 才存 `memories`（按项目）。
- **Skills**（`skills.rs`）：项目的 `.claude/skills/*/SKILL.md`、和 `AGENTS.md` 同级的 `skills/*/SKILL.md`、站主的 `~/.claude/skills/*/SKILL.md`；`load_skill` 按需读全文。CLI 原厂会读的自己读。
- **自己写 Skill**（`skill_propose.rs`）：模型用 `create_skill` 提议，站主 SAVE 才写。像 Key 的存不进去，同名的是替换那一份（旧版留着能退回）。任务还在跑、要写进项目里的排到收尾快照之后，所以不算进这次的改动、撤销也撤不掉。
- **@ 旧任务**：`[@标题](peck:task/<任务 id>)`，`history::task_refs` 把那个任务最近几次的请求、结果、改的文件放在这句前面（只认本项目），CLI 也带。`search_tasks` 让模型自己按关键词或任务号查。

### 后台和验证（`verify.rs`、`monitor.rs`、`cli_verify.rs`、`background.rs`）

- **`.peck/verify.toml`**：`[[check]]`（name、run、when：glob，空着都跑；timeout）最多 8 条；`[[server]]`（name、run、ready、autostart）最多 4 个。没有这份文件时按 `package.json`、`Cargo.toml` 这类猜（不跑命令）。
- **只认站主确认过的那一份**（`verify::load_approved` 比 md5）：项目自带的、被改过的，不跑、不起 dev server、不开目标模式。
- **改了就跑**：Peck 循环（不是 PLAN、ONE SHOT）每轮改过东西后，改到哪条的 `when` 就在后台起它；跑着时文件又改了就记 `stale`、重跑。
- **模型停下时**：要看的几条（目标模式是选的那几条；平时是改过的文件管得到的）都过就收尾；没过包成 `<peck-verify>` 交回修，最多 3 次（目标模式 8 次）。同一个失败连续两次、或两次之间没改文件就停（`monitor::FixGate`）。
- **CLI 也一样**：CLI 做完一轮，Peck 跑管得到的那几条，没过续接同一个会话交回去；项目设置能关。Antigravity 和续接不了的只跑一遍、不交回。
- **后台命令**（`bash_background`）：EDIT 下当「起常驻进程」问一次；一个任务最多 4 个，任务结束一起停。
- **dev server**（`[[server]]`）：在右栏 PREVIEW 头上起停（`server_start` / `background_stop`），跨任务一直在，退出 Peck 才停。`autostart = true` 的发任务时先起（PLAN 不起）；模型能读输出、不能停。PREVIEW 页签开着、窗口看得见时每 3 秒读一次状态（`background_list`）。

## 多引擎合作（`harness/team.rs`）

FLOW 选「接力」，或任何干法下开「审查」，走 `team_run_start`：一个任务号、一份快照、一条事件日志，一段一段来：

1. **写规范**（接力才有，只读）：按固定的五个标题写（目标、改哪些文件、步骤、验收标准、不做什么），验收标准是编号的、能查的条目；没写让它补一次。
2. **实施**（照选的档）：这句话 + `<peck-spec>`。
3. **审查**（开了才有，只读；没改文件不审）：看 Diff + 原话（+ 规范）。有验收标准时逐条判 `CHECK n: PASS / FAIL`，结论由没过的条目定；没有就看最后一行 `VERDICT: PASS / CHANGES`。
4. **按审查改**（要改才有，只一次）：交回实施的那一个，接着原对话；逐条判过的只给没过的那几条。

- 每段都是新会话，带上这个任务前面几次的经过。每段 `stage.started` → 这段引擎的事件 → `stage.completed`；一段没做完后面不开；打断停整个任务。
- **委派**（`delegate_task`，选了「委派」才给）：子任务交给那个 API 模型，同项目、同工具和权限（不能再委派、不问站主），审批照样弹给站主；事件带 `subagentId`，用量算进任务和花费上限。**一个任务最多 6 次**。实施的是 API 模型，或 Claude Code、Codex、Grok、Muse（经本机 MCP，`harness/delegate.rs`；PLAN 不委派）时能用。
- **换引擎接着发**（`history::fork_context`）：Peck 循环所有模型算一家，每个 CLI 一家，合作一家。这一家上次以后别家做的几次做成 `<peck-fork>` 放在这句前面；原来的会话留着，换回去接得上。

## 审阅和预览

- **FILES**（`files.rs`、`src/lib/highlight.ts`）：CodeMirror 语言包按需加载；html 在 `sandbox=""` 的 iframe 里看；Office 这类用 `qlmanage` 出缩略图。
- **按块接受 / 拒绝**（`snapshot.rs` 的 `revert_hunk`）：拒绝是把现在文件里的这一块换回任务前的，对不上不动；接受只是标记。标记和行内批注存浏览器存储。
- **PREVIEW**（`preview.rs` + `preview.js`）：地址默认是在跑的 dev server 输出里头一个本机地址，也能自己写（只认本机 http）。iframe 开的是 Peck 在 `127.0.0.1` 上起的代理：只认发给自己的 `Host`（挡 DNS rebinding），HTML 里插 `preview.js`（批注用）；iframe 带 `sandbox`，不能导走顶层窗口。
- **OPEN / FINDER**（IPC `open_path`）：用设置 → LOOK 的「编辑器」（空着用系统默认；会运行的文件不直接打开）或 `open -R`；只认项目和其他文件夹里的。

## 本机

- **终端**（TERM，`term.rs`）：每条命令在 pty 里起一个 shell；`cd` 记着，`export` 不带到下一条。用打开 App 时读的登录 shell 环境（`bash_run` 也是）。项目有任务在跑时拒绝新命令。只在内存。
- **关窗口**（`lib.rs`）：红色关闭只藏窗口，任务照跑；⌘Q 或「退出 Peck」才退出。
- **菜单栏和通知**（`tray.rs`）：空闲、在跑、等你；做完、失败、停下、等你批或答发通知（Peck 在最前面时不发）。
- **图标**：`scripts/menubar-icons.mjs`、`scripts/app-icons.sh` 从 `src/vendor/peck/marks/` 出图到 `src-tauri/icons/`。
- **贴图、拖图**：`tauri.conf.json` 设 `dragDropEnabled: false`，HTML5 拖放才能把图拖进输入框。
- **键盘**（`src/App.tsx`）：⌘N、⌘L、⌘.、⌘[ ⌘]（上一个 / 下一个任务）、⌘1–⌘6、⌘,、Esc。

## Key 和模拟 / 真实（`src-tauri/src/api/`）

IPC 不返回 Key，Key 不进配置、历史和模型上下文。分两包（AI 供应商、其他），一包一个钥匙串项。打开 App 在后台读，读完发 `keys-loaded`；拒了在设置里点 READ KEYS 再问。模拟模式的 Key 只在内存。`api_set_mode` 切模拟 / 真实。

## 额度与花费

顶栏 QUOTA（`usage.rs`、`spend.rs`、`UsagePage.tsx`）。官方来源见 [testing.md](./testing.md#额度与花费的真实来源)。

- **Claude 订阅额度**：Claude 桌面 App 或 Claude Code 在跑时，定时在 `cli-probe/` 里跑 `claude -p /usage`（本地命令，不进模型、不花额度）。要 Claude Code 命令行按订阅登录；status line 的数据作补充（`integrations/statusline.py`）。
- **别家**：CLI 报的 `quota.updated` 写进额度卡，缺数据不填 0；DeepSeek 余额只在真实 DeepSeek 请求进行中查。
- **图**（`spend_overview`）：事件日志里的 `usage.updated`，按模型记。CLI 按它会话的模型用价格表折成 API 价、标「估」（订阅不真花）。
- **额度提醒**（`src/lib/quota.ts`）：发任务前看这次要用到的 CLI 的 5 小时 / 每周窗口，用到阈值（默认 85%，能关）就提示换成另一家还够的 CLI。不自动换、不拦着发；读不到的不提示。

## 视觉、打包和签名

- 视觉基线是 visual-atelier 的 `schemes/peck/`，按 tag 单向同步进 `src/vendor/peck/`：改方案 → 站主批、打 tag → 在 visual-atelier 里跑 `VA_BROWSER_CHANNEL=chrome npm run sync -- --scheme peck --to peck --tag peck-v<版本>`。现在是那边的 `peck-v0.1.0`，是方案自己的版本号；Peck 自己的 tag 也叫 `peck-v<版本>`，两边各算各的。方案里还没有的样式先写在 `src/app.css`（清单在 HANDOFF「视觉基线要补的」）。字体打进包里，不联网。
- `pnpm app:package` 打包、校验签名；`pnpm app:install` 再装进 /Applications。`PECK_INSTALL_DIR` 改目标。
- 签名默认用本机自签名证书 **Peck Self-Signed**；`APPLE_SIGNING_IDENTITY` 换别的，`-` 是 ad-hoc。不是 Developer ID、没公证。

## 代码位置

各节标题已写模块，这里只列其余的。

| 位置 | 职责 |
|---|---|
| `src-tauri/src/{lib,commands,state,events}.rs` | 启动和恢复、IPC、共享状态、推给界面的事件 |
| `src-tauri/src/{run,flow,agent,process,mode}.rs` | 在跑的登记、任务记录里的计划、找 CLI 和旧清单、进程组、数据目录和模拟 / 真实 |
| `src-tauri/src/{store,migrations,workspace,snapshot,watch}.rs` | 存储和迁移、项目和校验、快照和撤销、忽略规则 |
| `src-tauri/{defaults,integrations,tests/fixtures/gateway}/` | 模型目录、价格表和旧清单；状态栏脚本；Gateway 夹具 |
| `src/App.tsx`、`src/lib/` | 编排和状态；IPC（`ipc.ts`，浏览器里转给 `?mock`）、设置、事件 → 界面（`agentView.ts`） |
| `src/shell/`、`src/ui.tsx`、`src/app.css` | 三栏外壳和各页；方案里还没有的样式和身份色 |
| `src/dev/`、`src/vendor/peck/` | `?mock` 的假数据和剧本；同步来的视觉 |
