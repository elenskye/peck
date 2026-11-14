# 验证方法

> 怎么测、用什么命令。原则在 [CLAUDE.md](../CLAUDE.md)「验证」「额度」，云端能跑的只有 `?mock`、类型和构建、后端测试（编得过的话）；平时只用 `?mock` 和本地夹具；真 CLI 只做验收点要求的冒烟；真 API 只用 [roadmap](./roadmap.md#现行授权) 里剩下的授权；不碰正式数据目录。模拟、自动测试、原生交互、真实服务的结果分开记，没验过的不写「通过」。

## 界面：`?mock`

- `pnpm dev`（固定端口 1430），开 `http://localhost:1430/?mock`。本地：内置浏览器，`personal-cli-studio/.claude/launch.json` 里的 `peck-mock` 就是它；云端：用无头浏览器开同一个地址截图看。
- 浏览器里 `src/lib/ipc.ts` 把所有调用转给 `src/dev/mockIpc.ts`，数据在 `src/dev/mockData.ts`，只在内存，刷新重置；原生窗口永远走 Rust。
- 参数：`&empty` 没有项目（看空状态）。
- 假数据里有 3 个项目（notebook、peck、edge-gateway），6 个任务：在跑的一个（Muse → Grok，第二步在跑）、完成的四个（Grok 单个、Muse → Grok、API 模式单模型干活、API 模式规划 + 实施）、失败的一个（Grok 登录过期）。每个完成的任务都有改动列表和 Diff，API 模式的有调用与审查历史。
- 在 `?mock` 里发任务：选项目 → NEW TASK → 左栏 AGENTS 里选谁来干（FLOW 选干法，再选 CLI / API 和模型；模型列表全部当能用、CLI 的模型是演示数据；设置的开发者选项开了 FLOW 里多一个「假」）→ 一句话 → ↵。
  - **Peck 循环**（任何一个 PECK 组的模型）：`src/dev/peckMock.ts` 的剧本——看目录、读、搜、改（进 CHANGES）、跑一条输出很长的命令（点开能看完整日志）、`rm -rf dist`（高危卡片，ALLOW / DENY 都能走完）、清单、ASK（选项或自己写）、收尾；每轮报用量，任务下面一行花费，输入框旁累计。设置里花费上限设得比 0.42（演示里这个月已花的）低，发出去就停、写明原因。
  - **上下文和验证的场景**（Peck 循环，按这句话里的字挑，`peckMock.ts` 开头写着）：「长任务」上下文过了压缩线、自动压缩三步以后接着做完；「修好」改完验证没过、交回模型修一次、再验证过了挂 ✓；「反复」同一个失败连续两次停下（任务记成失败、写原因）；「并发」后台验证跑到一半文件又改了、这次的结果过期、按新文件再跑；输入框旁开了 GOAL 就是目标模式。不带这些字的主线在 peck 项目里（演示有验证命令）改完文件会在后台跑一遍、收尾挂 ✓，结束时弹「想记住这几条」。输入框旁的小环点开看上下文分成哪几块。
  - **验证命令**：notebook 项目没有 `.peck/verify.toml`，NEW TASK 上有 Peck 猜的三条（CONFIRM 写进去、NOT NOW 不再问；演示里都当站主确认过）；peck 项目有，输入框旁有 GOAL 和 DEV（DEV 里起 dev server，1.5 秒后算起好了）；项目设置里能改 verify.toml、看和删记忆（peck 演示有两条）。
  - **@ 旧任务**：在一个任务里打 `接着 @设置`，上方弹出旧任务，↵ 插进去。
  - **CLI 引擎**：回放 `src-tauri/tests/fixtures/gateway/` 的录音（Codex 3 次审批、Grok 1 次……），碰到审批停下等卡片。
  - **接力、规划 + 实施、假 Agent**：照旧的步骤显示，能停、能重跑步骤、能撤销。
  - **ONE SHOT**：右栏页签，选一个（Antigravity 不在里面）问一句，读一个文件再答，COPY、TO TASK。
- **会话和项目、审阅和预览**：
  - **两个项目同时跑**：在 notebook 发一个 Peck 循环的任务，没等它停就切到 peck 再发一个；左栏最上面 `[ NOW ]` 列着两个在跑的，碰到审批的那个排到最前面（琥珀问号），点一行直接到那个任务。做完了、在别的任务里看不到的那个留着绿勾，打开过就从 NOW 里下去。
  - **recap**：Peck 循环的主线和「修好」做完，回答末尾的 `→ ` 那几行变成 recap 下面的按钮，点了放进输入框（不发）。
  - **项目设置**：FOLDERS 里填一个绝对路径点 ADD 加进来（空着点 ADD 是原生的选文件夹窗，`?mock` 里没有），READ / WRITE 切；ENV 写 `NAME=value`，写 `HOME=x` 会报错。
  - **FILES**：peck 项目的 `docs/` 和 `public/` 里有几种演示文件——`guide.md`、`index.html`、`bird.svg`（VIEW / SRC）、`screenshot.png`、`spec.pdf`、`plan.docx`、`budget.xlsx`（Quick Look 的缩略图在 `?mock` 里是一张演示图）；`src-tauri/src/*.rs`、notebook 的 `.tsx` 看代码上色。
  - **独立工作区**：peck 项目（演示里是 git 项目）输入框下点亮 WORKTREE 再发，做完 CHANGES 顶上是 APPLY / DISCARD；这句话里带「冲突」两个字，APPLY 会报一个冲突、再给 APPLY ANYWAY。
  - **按块接受 / 拒绝、批注**：主线任务做完，CHANGES 里点 `src-tauri/src/settings.rs` 这类改过的文件，块头上 REJECT / ACCEPT；点行号写批注，顶上 TO TASK 放进输入框。批注和块的状态只在浏览器里存（`localStorage`），刷新 `?mock` 数据重置、它们还在。
- `?mock` 里的终端只认几条命令：`ls`、`cd`、`pwd`、`git status`、`git diff`、`pnpm check`、`pnpm test`（失败，退出码 1）、`pnpm dev`（一直跑，点 STOP 才停）。任务在跑时终端锁着。
- 改了界面明暗各看一次（内置浏览器的 `resize_window` 切 `colorScheme`），截图只放会话临时目录。截图常慢一拍，关键结果用 `read_page` 或 JS 读页面确认。
- `?mock` 不代替原生、钥匙串、真实 API 的验收。

## 类型和构建

```bash
pnpm check     # tsc --noEmit
pnpm build     # 前端生产构建
```

## 后端

```bash
set -o pipefail
T=$(mktemp -d)
PECK_DATA_DIR=$T/data PECK_SHADOW_ROOT=$T/shadow \
  cargo test --offline --manifest-path src-tauri/Cargo.toml
```

- 日常只跑改到的模块：在命令末尾加模块名过滤，如 `-- snapshot::`。一个版本收尾才全跑。
- 任务前快照的登记表是整个进程共用、按任务号存的，每个临时库的任务号都从 `run-105` 开始：会发任务的测试先 `create_run` 占一个大号（9xx_001，各测试不一样），不然并行时互相覆盖（`api::worker` 会报「任务快照基线缺失」）。
- 云端（Linux）：装上 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libsoup-3.0-dev`、`librsvg2-dev`、`libayatana-appindicator3-dev`、`libxdo-dev` 就编得过（没有离线的 crate 缓存，去掉 `--offline`）。`api::tools` 的 3 项和 `api::worker` 的 5 项要 macOS 的 `sandbox-exec`，在 Linux 上一定失败，留给本地。`harness::tools::bash` 的沙箱测试（项目外写被拒、联网能通、`.git` 只读）只在 macOS 上编进来；Linux 上命令不进沙箱直接跑。`app_icon.rs` 的 AppKit 部分只在 macOS 上编。
- Peck 循环的测试（`-- harness::`）用假模型（`ScriptedModel`）跑完整的循环：工具顺序、PLAN 不给改的工具、高危必弹、EDIT 装依赖问一次和「总是允许」、轮数 / 花费上限 / 用量未知停下、打断命令、追加一句、续接对话、ONE SHOT 只读，以及发任务到收尾（任务记录、CHANGES、事件日志、花费）。上下文和验证：项目层（AGENTS.md / CLAUDE.md + 记忆）、环境块（git 不动 index）、引用和 `search_tasks`、长任务压缩以后接着做、压完还太满停下、三步压缩各自的单测；后台命令不阻塞、任务结束就停、EDIT 起后台命令问一次、常驻 server 不跟任务停；验证命令的猜、校验、写，没确认过的、改过的不用；后台验证和并发过期；没过交回去修好不再叫模型、同一个失败连续两次停、两次之间没改文件停、修够 3 次停、目标模式；记忆存、去重、批过不能再批。命令的 future 被丢掉时整个进程组都停（`harness::tools::bash`）。会话和项目、审阅和预览：项目的其他文件夹（只读的写不进、能改的进 CHANGES、和任务一起撤销，`harness::run`；沙箱里嵌套的 `.git`、只读文件夹写不进，`harness::tools::bash`，只在 Mac 上）、项目设置的校验（`workspace::`）、独立工作区（建、做完主文件夹不动、APPLY 搬回、冲突、照样搬以后撤销回到搬之前、可执行位、DISCARD，`worktree::` 和 `harness::run`）、按块拒绝（CRLF、重建的文件不动，`snapshot::hunk_tests`）、FILES 的图片和 PDF（`files::`）。Quick Look（`qlmanage`）只在 macOS 上有，Linux 上 Office 文件只回「看不了」。验证和后台命令的测试真的起 `sh`（`sleep`、`test -f` 这类），在 Linux 上没有沙箱照样跑。
- 不用 `| tail` 截输出（会吞掉退出码）；要短就加 `-q`。
- 每次用新的临时目录（上面的 `mktemp -d`）：模拟授权的次数记在数据目录里，重复用同一个目录会把次数用完，`api::worker` 的测试就失败。
- 终端的测试会真的起一次站主的交互式登录 shell 读环境（`term.rs`），启动脚本特别慢的机器上会多等几秒。
- 有些测试要本机允许回环 HTTP 夹具和 `sandbox-exec`。API 协议的测试用本地夹具，不发真请求；夹具通过不等于真实账号通过。

## 原生（只有本地）

- `pnpm app:dev`：原生开发窗口，数据在 `~/Library/Application Support/Peck Dev/`，API 默认模拟。
- 装好的正式包隔离着跑（不碰正式数据和钥匙串里的正式 Key）：
  ```bash
  PECK_DATA_DIR=/private/tmp/peck-demo PECK_API_MODE=mock \
    /Applications/Peck.app/Contents/MacOS/peck
  ```
- 调试包（带 Web Inspector，右键 → Inspect Element 看控制台）：`PECK_BUILD_PROFILE=debug pnpm app:package`，再 `open -n <包> --env PECK_DATA_DIR=<临时目录> --env PECK_SHADOW_ROOT=<临时目录> --env PECK_API_MODE=mock`：和从访达打开一样的最小环境，钥匙串用测试分区，不碰正式数据。
- 假 Agent（设置的开发者选项，清单 `src-tauri/defaults/agents/mock.toml`）会在项目里写演示文件，用临时目录当项目。
- 验收点要「原生」的，就在 App 里真的点一遍：发任务、看步骤、看 Diff、撤销、重开 App 后记录和 Diff 还在。

## 升级测试（只有本地）

1. 退出 Peck，把正式数据目录整个复制到临时目录（`peck.db`、`peck.db-wal`、`shadow/` 都要）。
2. 用新版指过去打开：`PECK_DATA_DIR=<副本> /Applications/Peck.app/Contents/MacOS/peck`（或 `pnpm app:dev` 加同样的变量）。
3. 看：数据目录里生成了升级前的备份；项目、任务、消息、调用数和升级前一样；旧任务的 Diff 能打开；用旧版打开新库会被拒绝写入。

## 夹具（Gateway）

- `src-tauri/tests/fixtures/gateway/<名字>/`：`raw.jsonl`（引擎或供应商的原始输出，去掉了 Key、账号、邮箱，项目路径换成 `{{WORKSPACE}}`、家目录换成 `{{HOME}}`）和 `events.jsonl`（转成的统一事件）。五家 CLI 各一个小任务，四家 API 各一次带工具的请求和回放；`models-mock.json` 是 `?mock` 的模型列表。
- 改了转换器、确认新结果对：`PECK_REGEN_FIXTURES=1 cargo test gateway` 重写期望文件，再看 `git diff`。
- 夹具通过不等于真实账号通过；Claude Code 的夹具录的时候没开 `--include-partial-messages`，流式增量只有单测（`streamed_pieces_and_the_finished_message_share_ids`），2026-11-12 真跑核对过一次。

## 真 CLI、真 API（只有本地：本地开发的 Phase 和本地验收）

- 次数照 [roadmap「现行授权」](./roadmap.md#现行授权)：每个版本每家 API 5 次尝试、每家 CLI 3 个小任务（每个不超过 12 轮）；用之前和之后都记账（roadmap 的表，CLI 的结果另记 HANDOFF），失败不静默重试，用完先停下问站主。
- 一律用这家最便宜的模型、做无关紧要的小任务，项目用仓库外的 `personal-cli-studio/projects/gateway-smoke`。
- 各模块里 `#[ignore]` 的 `live_*` 测试就是真跑的入口，按环境变量拿参数（Key 从 Key 目录的文件读，不打印、不进日志）：
  - API：`gateway::providers` 的 `live_two_requests`（一次带工具、一次回放）。`PECK_LIVE_MODEL`（目录里的模型 id）、`PECK_LIVE_KEYS`（Key 目录）、可选 `PECK_LIVE_REGION`、`PECK_LIVE_EFFORT`。
  - CLI：`gateway::engines::{codex,claude_code,muse,acp}` 的 `live_small_task`。`PECK_LIVE_CODEX` / `PECK_LIVE_CLAUDE` / `PECK_LIVE_MUSE` / `PECK_LIVE_ACP`（可执行文件，`cli/bin/…`）、`PECK_LIVE_CWD`、`PECK_LIVE_PROMPT`，可选 `PECK_LIVE_MODE`、`PECK_LIVE_CONFIG_DIR`（另一个账号的配置目录）、`PECK_LIVE_MCP`。审批一律批准一次，提问答「好」，过程逐条打出来。
  - 跑法：`cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture <测试路径>`，`PECK_DATA_DIR` 照样指到新的临时目录；输出里的家目录路径去掉再贴进文档。
- App 里的真实请求（站主自己发的任务）不经过授权账本，只受设置里的花费上限管。验收点要「在 App 里」的，装正式包后在 App 里走，结果和上面的离线、真跑分开记。

## 额度与花费的真实来源

改额度与花费时对照这张表。这是 2026-11-06 读官方文档的结论，接入前按本机安装的版本再核对；没有的那家只影响那一家。

| 来源 | 官方入口 | 注意 |
|---|---|---|
| Codex | [App Server](https://learn.chatgpt.com/docs/app-server)：`account/rateLimits/read`、`account/rateLimits/updated` | 不用 API Key 推订阅额度；同一池的兼容字段和 map 项不重复加；credits 不当 USD |
| Claude Code | [status line JSON](https://code.claude.com/docs/en/statusline)：`rate_limits` | 支持的账号、有过模型响应后才可能有；事件驱动，空闲不刷新；缺字段不是 0 |
| Antigravity | [CLI statusline](https://antigravity.google/docs/cli/statusline)：`quota` | 只是 CLI 的路径，不代表桌面 App；上下文比例不是订阅额度 |
| Muse CLI | `muse usage --json`（[更新记录](https://dev.meta.ai/docs/muse-code/changelog)） | 用 `allowance.asOfMs` 作数据时间；没有账户标识，旧快照不归到新登录的账号 |
| Grok CLI | [`/usage`](https://docs.x.ai/build/modes-and-commands) | 1.0.77 的 `usage --json` 还不给套餐数据，只能手工快照 |
| DeepSeek API | [GET `/user/balance`](https://api-docs.deepseek.com/api/get-user-balance/) | 用已配置的 Key；换 Key 清旧余额 |
| Anthropic API | [Usage/Cost Analytics](https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api) | 要 Admin 权限，普通 Key 没有余额接口，显示未知 |

看的要点：只在对应客户端运行时监测，退出就停并留最后一次快照；未装、未登录、缺字段、失败分开显示；未知不填 0，币种不直接相加，估算标出来；不采集提示词、会话正文、环境变量；不为查额度启动已关闭的客户端或发模型 turn。合成数据只验启停和显示，不算真实接入。

- **CLI 的模型列表**（不发任务、不花额度，只握手和列模型）：`PECK_LIVE_CLI_DIR=<cli/bin 的完整路径> PECK_LIVE_ENGINE=codex cargo test --manifest-path src-tauri/Cargo.toml live_list_models -- --ignored --nocapture`（engine 换成 muse、grok、antigravity）。
