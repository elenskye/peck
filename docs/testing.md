# 验证方法

> 怎么测、用什么命令、怎么更新正式版。平时只用 `?mock` 和本地夹具，不发真请求、不碰正式数据目录；真跑见 [roadmap「现行授权」](./roadmap.md#现行授权)。模拟、自动测试、原生交互、真实服务的结果分开记。

## 每次改完

- 前端：`pnpm check`；改了界面开 `?mock` 看改到的地方，明暗各一次。
- 后端：只跑改到的模块（见「后端」）。

## 更新正式版

什么时候做见 [CLAUDE.md「版本」](../CLAUDE.md)。

1. 版本号四处一起改。
2. `pnpm check`、`pnpm build`、全量 `cargo test`；`?mock` 明暗各看一遍这一版改的地方。
3. 分 Phase 的版本另加：两个只读子代理分头审这一版的改动（Rust、前端各一个），真缺陷修掉；做一次「升级测试」。
4. `pnpm app:install` 装进 /Applications（打包、校验签名，旧包自动备份一份）。
5. 更新 HANDOFF，提交。

## 界面：`?mock`

- `pnpm dev`（固定端口 1430），开 `http://localhost:1430/?mock`；内置浏览器里是 `personal-cli-studio/.claude/launch.json` 的 `peck-mock`。`&empty` 看没有项目的空状态。
- 浏览器里 `src/lib/ipc.ts` 把调用转给 `src/dev/mockIpc.ts`（数据在 `mockData.ts`，只在内存，刷新重置）；原生窗口永远走 Rust。
- 假数据有 3 个项目（notebook、peck、edge-gateway）加一个文件夹不在了的，开局的任务都带事件日志（`mockData.ts` 的 `MOCK_EVENTS`）：notebook 有一个一直在跑的任务（那个项目的 AGENTS 锁着）、Grok 和 DeepSeek 各做完一个；peck 有一个接力做完（Opus 写规范 → DeepSeek 实施）、一个 Grok 没登录失败；edge-gateway 有一个接力（Muse → Grok）和一个归档的。**试新东西用 peck 项目**。
- 打开是全部项目页（中栏列项目，左栏只有 AGENTS）；点一个项目进去，PROJECTS 头上的放大图标回来。
- 发任务：选项目 → NEW TASK → 左栏 AGENTS 选谁来干 → 一句话 → ↵。模型列表全部当能用。按这句话里的字挑场景：

| 场景 | 怎么触发 | 看什么 |
| --- | --- | --- |
| Peck Loop 主线（`peckMock.ts`） | 任何一个 API 模型 | 看目录、读、搜、改（FILES 里标出来）、长输出命令、`rm -rf dist` 高危卡片、清单、ASK、recap 和下一步按钮、花费、「想记住这几条」 |
| 上下文 | 「长任务」 | 自动压缩三步 |
| 快照回退 | 「撤销」「撤掉」「回退」；`/undo` | 读 Skill → 列快照 → 复述 → `[ RISK ]`「回到任务 N 开始前（2 个文件）」→ ALLOW / DENY |
| 定时任务 | 「定时」「每周」「每天」「每隔」 | 读 Skill → 复述、ASK 确认 → 设好，PROJECTS 头上的钟多一个 |
| 错过的定时任务 | 打开就有两个（notebook） | 一打开跳到 notebook 的新任务页：`[ MISSED ]` RUN NOW / SKIP；另一张在「日期统一成 YYYY-MM-DD」那个对话末尾；钟里看详情、REMOVE |
| 留着的 dev server | 「dev server」 | 过程里一行「留着在跑」；右栏 PREVIEW 头上一个 DEV 小片，点一下停 |
| 花费上限 | 设置里上限设得比 0.42 低 | 发出去就停、写明原因 |
| CLI 引擎 | 选一个 CLI | 回放 `src-tauri/tests/fixtures/gateway/` 的录音，碰到审批停下 |
| 接力 + 审查 + 委派（`teamMock.ts`） | FLOW 选接力，写规范选 CLI、实施选 API、审查另选一个、委派选 FLASH | SPEC 一行（在跑时下面露 Thinking、Read …；做完「写好了规范」，点开是规范全文和读过的 2 个文件）→ BUILD（中间「交给子任务」）→ REVIEW 逐条清单「1 条没过 · 交回去修」→ FIX → recap 和按段花费表；句子带「通过」就都过、没有 FIX |
| 预设、额度提醒 | AGENTS 最上面「预设」选「稳妥」；QUOTA 卡片里 Claude Code 每周 90% | 输入框下那行写预设名；输入框上方「Claude Code 每周已用 90%，换成 Codex？」 |
| 换引擎接着发 | 合作做完的任务里换成 CLI、关审查再发 | 一行「换成 X 接着做：前面 1 次……带过去了」 |
| 独立工作区 | peck 项目点亮 WORKTREE；带「冲突」 | 对话里 recap 下面 APPLY / DISCARD；冲突时 APPLY ANYWAY |
| 截图、贴图 | 带「截图」；输入框粘贴或拖进图 | 演示图；小图，发出去一行「这一句带了 1 张图」 |
| 斜杠命令 | 输入框打 `/` | Peck 自己的八个 + 命令库里演示的 `/fix-todo`、`/changelog`；`/undo` 发一句话给模型、`/model` 提示不转发 |

- 其他入口：`@` 旧任务；ONE SHOT；PREVIEW（GO → NOTE 点元素写批注 → TO TASK）；项目设置（FOLDERS、ENV、MCP、SKILLS、COMMANDS、记忆）；设置各组；FILES（peck 的 `docs/`、`public/` 里有各种文件；对话里点改过的文件名在这里打开）；AGENTS 头上的 ⓘ。
- 终端只认 `ls`、`cd`、`pwd`、`git status`、`git diff`、`pnpm check`、`pnpm test`（失败）、`pnpm dev`（一直跑）。
- 切明暗：内置浏览器 `resize_window` 的 `colorScheme`，或在控制台改 `document.documentElement.dataset.theme`；截图只放会话临时目录。
- **内置浏览器的坑**：面板隐藏时 CSS 动画停在第一帧（菜单、弹层截图里看不见，用 JS 读 DOM、点按钮）；按 ref 点会点偏，用截图坐标或 JS 点；关键结果用 `read_page` 或 JS 读页面确认；控制台记录跨刷新还在，看时间戳。
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
  cargo test --manifest-path src-tauri/Cargo.toml
```

- 日常只跑改到的模块：末尾加过滤，如 `-- snapshot::`。收尾才全跑。
- **每次用新的临时目录**，不碰正式数据（模拟授权次数记在数据目录里，重复用会失败）。
- **测试撞号**：任务前快照的登记表、Gateway 的控制表都是整个进程共用、按任务号存的，每个临时库的任务号都从 `run-105` 开始：会发任务的测试先 `create_run` 占一个大号，各测试不一样（现在用了 700105、700106、700201–700205、905001、910_001、920_001、930_001、930_002、940_001、950_001），新测试另挑一个。
- Peck Loop 的测试用假模型（`ScriptedModel`）跑完整的循环，外部 MCP 用 sh 写的假服务器，预览代理起一个假 dev server（一段一段出的 SSE、分块的 HTML、POST）。快照回退、定时任务的工具用 `harness::extras::testing` 搭的带快照的假项目，Peck Loop 和本机 MCP 两边各走一遍。沙箱、Quick Look、AppKit 那几项只在 macOS 上编进来。
- 不用 `| tail` 截输出（会吞掉退出码）；要短就加 `-q`。
- 终端的测试会真的起一次站主的交互式登录 shell 读环境，慢几秒。夹具通过不等于真实账号通过。

## 原生

- `pnpm app:dev`：原生开发窗口，数据在 `~/Library/Application Support/Peck Dev/`，API 默认模拟。裸二进制没有 bundle id，computer-use 认不到。
- 装好的正式包隔离着跑（不碰正式数据和正式 Key）：`PECK_DATA_DIR=<临时目录> PECK_SHADOW_ROOT=<临时目录> PECK_API_MODE=mock /Applications/Peck.app/Contents/MacOS/peck`，后台起、`kill $!` 只停自己起的那个。
- 调试包（带 Web Inspector）：`PECK_BUILD_PROFILE=debug pnpm app:package`，再 `open -n <包> --env PECK_DATA_DIR=<临时目录> --env PECK_SHADOW_ROOT=<临时目录> --env PECK_API_MODE=mock`。
- 不花钱地走一遍：设置 → GENERAL → API 请求发到哪选 MOCK：Peck Loop 用假模型照剧本做，CLI 引擎回放夹具；用临时目录当项目。
- **只截 Peck 的窗口**不用接管屏幕：按 pid 找窗口号，`screencapture -x -o -l<窗口号>`。
- **点窗口**：bundle id `com.personalclistudio.peck`。WKWebView 不把控件给辅助功能，要站主批 computer-use（下拉、右键菜单、打字要全屏控制）；站主拒了或人不在就把没验的写进 HANDOFF，别反复要。
  - 每装一次新包，第一次打开可能弹钥匙串授权框，要站主输密码点「始终允许」；框开着时点击都被挡——要修的攒在一起再换包。
  - 开了很久的窗口后台截图常截不到，退出重开。拖动用 `left_click_drag`。右键菜单的项先移过去停 0.3 秒再点。
  - 站主用微信输入法：输入框有焦点时模拟点击别的按钮会被吞，回车会变成换行。多行的话先放剪贴板再 ⌘V。
  - 通知横幅会抢到最前，之后的点击被拦：把 Peck 重新拉到前面。站主开着台前调度：点到桌面会把窗口全收起来，跨 App 拖文件做不成。
  - 长任务别一直截图：只读查正式库（`runs.record` 的状态、`agent_events` 里的阶段、卡片、`turn.completed`），有卡片再回去点。
  - 退出 Peck 会停掉在跑的任务：换包要等真跑都完了。
- **Muse** 只在它信任过的文件夹里才连 Peck 挂在会话上的 MCP（提 Skill、委派）：验之前站主先在那个文件夹里开一次 `muse` 点信任。

## 升级测试

1. 把正式数据目录复制到临时目录（`cp -Rc ~/Library/Application\ Support/Peck <副本>`；`peck.db`、`-wal`、`shadow/` 都要）。
2. 用新版指过去打开：`PECK_DATA_DIR=<副本> PECK_SHADOW_ROOT=<副本>/shadow /Applications/Peck.app/Contents/MacOS/peck`（后台起，看完 `kill $!`）。
3. 看：改了 schema 的版本生成 `before-v<新 schema>` 备份；项目、任务、事件和升级前一样（`sqlite3` 比行数，不比文件 md5）；旧任务的 Diff 能打开。旧任务快照记录里是正式影子仓库的路径，只看不撤销。用完删掉副本。

**备份恢复**：把正式数据 `backups/` 里的一份 `peck-…-before-v<N>.db` 复制到临时目录改名 `peck.db`，照第 2 步打开：它照常升到现在的版本、再留一份备份。

## 夹具（Gateway）

- `src-tauri/tests/fixtures/gateway/<名字>/`：`raw.jsonl`（原始输出，去掉了 Key、账号、邮箱，路径换成 `{{WORKSPACE}}`、`{{HOME}}`）和 `events.jsonl`（转成的统一事件）。五家 CLI 各一个小任务，四家 API 各一次带工具的请求和回放；`models-mock.json` 是 `?mock` 的模型列表。
- 改了转换器、确认新结果对：`PECK_REGEN_FIXTURES=1 cargo test gateway` 重写期望文件，再看 `git diff`。

## 真 CLI、真 API

- 规矩和次数见 roadmap「现行授权」；项目用仓库外的 `personal-cli-studio/projects/gateway-smoke`。
- 入口是各模块 `#[ignore]` 的 `live_*` 测试，按环境变量拿参数（Key 从 Key 目录的文件读，不打印；列 Key 目录会被权限检查拦，真 API 要站主给路径或自己跑）：
  - API：`gateway::providers` 的 `live_two_requests`。`PECK_LIVE_MODEL`、`PECK_LIVE_KEYS`（Key 目录）、可选 `PECK_LIVE_REGION`、`PECK_LIVE_EFFORT`。
  - CLI：`gateway::engines::{codex,claude_code,muse,acp}` 的 `live_small_task`。`PECK_LIVE_CODEX` / `PECK_LIVE_CLAUDE` / `PECK_LIVE_MUSE` / `PECK_LIVE_ACP`（`cli/bin/…`）、`PECK_LIVE_CWD`、`PECK_LIVE_PROMPT`，可选 `PECK_LIVE_MODE`、`PECK_LIVE_CONFIG_DIR`、`PECK_LIVE_MCP`、`PECK_LIVE_MODEL`、`PECK_LIVE_EFFORT`、`PECK_LIVE_LOG=<文件>`。审批一律批准一次，提问答「好」。
  - CLI 的模型列表（不花额度）：`PECK_LIVE_CLI_DIR=<cli/bin> PECK_LIVE_ENGINE=codex cargo test --manifest-path src-tauri/Cargo.toml live_list_models -- --ignored --nocapture`。
  - 跑法：`cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture <测试路径>`，`PECK_DATA_DIR` 照样指到新的临时目录。

## 额度与花费的真实来源

| 来源 | 官方入口 | 注意 |
|---|---|---|
| Codex | [App Server](https://learn.chatgpt.com/docs/app-server)：`account/rateLimits/read`、`account/rateLimits/updated` | 不用 API Key 推订阅额度；credits 不当 USD |
| Claude Code | [status line JSON](https://code.claude.com/docs/en/statusline)：`rate_limits`；Peck 现在跑 `claude -p /usage` | 空闲不刷新；缺字段不是 0；`cli/bin` 的 Claude Code 要先登录 |
| Antigravity | [CLI statusline](https://antigravity.google/docs/cli/statusline)：`quota` | 只是 CLI 的路径，不代表桌面 App |
| Muse CLI | `muse usage --json`（[更新记录](https://dev.meta.ai/docs/muse-code/changelog)） | 用 `allowance.asOfMs` 作数据时间 |
| Grok CLI | [`/usage`](https://docs.x.ai/build/modes-and-commands) | `usage --json` 还不给套餐数据 |
| DeepSeek API | [GET `/user/balance`](https://api-docs.deepseek.com/api/get-user-balance/) | 用已配置的 Key；换 Key 清旧余额 |
| Anthropic API | [Usage/Cost Analytics](https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api) | 要 Admin 权限，普通 Key 显示未知 |

要点：只在对应客户端运行时监测，退出就停并留最后一次快照；未知不填 0，币种不直接相加，估算标出来；不采集提示词、会话正文、环境变量；不为查额度启动已关闭的客户端，不用「你好」探针或模型 turn 查额度、余额。
