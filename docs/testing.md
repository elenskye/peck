# 验证方法

> 怎么测、用什么命令。原则在 [CLAUDE.md](../CLAUDE.md)「验证」「额度」，云端能跑的只有 `?mock`、类型和构建、后端测试（编得过的话）；平时只用 `?mock` 和本地夹具；真 CLI 只做验收点要求的冒烟；真 API 只用 [roadmap](./roadmap.md#现行授权) 里剩下的授权；不碰正式数据目录。模拟、自动测试、原生交互、真实服务的结果分开记，没验过的不写「通过」。

## 界面：`?mock`

- `pnpm dev`（固定端口 1430），开 `http://localhost:1430/?mock`。本地：内置浏览器，`personal-cli-studio/.claude/launch.json` 里的 `peck-mock` 就是它；云端：用无头浏览器开同一个地址截图看。
- 浏览器里 `src/lib/ipc.ts` 把所有调用转给 `src/dev/mockIpc.ts`，数据在 `src/dev/mockData.ts`，只在内存，刷新重置；原生窗口永远走 Rust。
- 参数：`&empty` 没有项目（看空状态）。
- 假数据里有 3 个项目（notebook、peck、edge-gateway），6 个任务：在跑的一个（Muse → Grok，第二步在跑）、完成的四个（Grok 单个、Muse → Grok、API 模式单模型干活、API 模式规划 + 实施）、失败的一个（Grok 登录过期）。每个完成的任务都有改动列表和 Diff，API 模式的有调用与审查历史。
- 在 `?mock` 里发任务：选项目 → NEW TASK → 左栏 AGENTS 里选谁来干（FLOW 选干法，再选 CLI / API 和模型；模型列表全部当能用、CLI 的模型是演示数据；设置的开发者选项开了 FLOW 里多一个「假」）→ 一句话 → ↵。
  - **Peck 循环**（任何一个 PECK 组的模型）：`src/dev/peckMock.ts` 的剧本——看目录、读、搜、改（进 CHANGES）、跑一条输出很长的命令（点开能看完整日志）、`rm -rf dist`（高危卡片，ALLOW / DENY 都能走完）、清单、ASK（选项或自己写）、收尾；每轮报用量，任务下面一行花费，输入框旁累计。设置里花费上限设得比 0.42（演示里这个月已花的）低，发出去就停、写明原因。
  - **CLI 引擎**：回放 `src-tauri/tests/fixtures/gateway/` 的录音（Codex 3 次审批、Grok 1 次……），碰到审批停下等卡片。
  - **接力、规划 + 实施、假 Agent**：照旧的步骤显示，能停、能重跑步骤、能撤销。
  - **ONE SHOT**：右栏页签，选一个（Antigravity 不在里面）问一句，读一个文件再答，COPY、TO TASK。
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
- 云端（Linux）：装上 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libsoup-3.0-dev`、`librsvg2-dev`、`libayatana-appindicator3-dev`、`libxdo-dev` 就编得过（没有离线的 crate 缓存，去掉 `--offline`）。`api::tools` 的 3 项和 `api::worker` 的 5 项要 macOS 的 `sandbox-exec`，在 Linux 上一定失败，留给本地。`harness::tools::bash` 的沙箱测试（项目外写被拒、联网能通、`.git` 只读）只在 macOS 上编进来；Linux 上命令不进沙箱直接跑。`app_icon.rs` 的 AppKit 部分只在 macOS 上编。
- Peck 循环的测试（`-- harness::`）用假模型（`ScriptedModel`）跑完整的循环：工具顺序、PLAN 不给改的工具、高危必弹、EDIT 装依赖问一次和「总是允许」、轮数 / 上下文 / 花费上限 / 用量未知停下、打断命令、追加一句、续接对话、ONE SHOT 只读，以及发任务到收尾（任务记录、CHANGES、事件日志、花费）。
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

评估额度监测器（站主 v0.6 起用 Peck 开发，每个版本的验收页评估）时对照这张表。这是 2026-11-06 读官方文档的结论，接入前按本机安装的版本再核对；没有的那家只影响那一家。

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
