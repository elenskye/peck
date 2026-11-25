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
4. `pnpm app:install` 装进 /Applications（打包、校验签名，旧包和以前的备份都挪进废纸篓）。
5. 更新 HANDOFF，提交。

## 界面：`?mock`

- `pnpm dev`（固定端口 1430），开 `http://localhost:1430/?mock`；内置浏览器里是 `personal-cli-studio/.claude/launch.json` 的 `peck-mock`。`&empty` 看没有项目的空状态。
- 浏览器里 `src/lib/ipc.ts` 把调用转给 `src/dev/mockIpc.ts`（数据在 `mockData.ts`，只在内存，刷新重置）；原生窗口永远走 Rust。
- 假数据有 3 个项目（notebook、peck、edge-gateway）加一个文件夹不在了的，开局的任务都带事件日志（`mockData.ts` 的 `MOCK_EVENTS`）：notebook 有一个一直在跑的任务（那个项目的 AGENTS 锁着）、Grok 和 DeepSeek 各做完一个；peck 有一个 PLOOP 做完（DeepSeek 当主角色）、一个 Grok 没登录失败；edge-gateway 有一个 Grok 做完的和一个归档的。**试新东西用 peck 项目**。
- 打开是全部项目页（中栏列项目，左栏只有 AGENTS）；点一个项目进去，PROJECTS 头上的放大图标回来。
- 发任务：选项目 → NEW TASK → 左栏 AGENTS 选谁来干 → 一句话 → ↵。模型列表全部当能用。按这句话里的字挑场景：

| 场景 | 怎么触发 | 看什么 |
| --- | --- | --- |
| Peck Loop 主线（`peckMock.ts`） | 任何一个 API 模型 | 看目录、读、搜、改（FILES 里标出来）、长输出命令（标题是那句说明「跑一遍测试」，点开先一行 `$ pnpm test`）、推上去的 `[ RISK ]` 卡片（标题「把这次的改动推到 GitHub」、红标签「推送或改 Git 历史」、「看命令」点了才露出命令）、清单、ASK、recap、花费、「想记住这几条」；任务头「第 N/40 轮」 |
| 上下文 | 「长任务」；`/compact 重点`（PLOOP 做过的任务，比如 peck 的「设置面板加通知开关」） | 自动压缩三步；手动的那一次是一行「COMPACT 压缩上下文 · 重点：…」，下面三步、花费、「上下文压缩好了」 |
| CLI 的用量和 `/compact` | 选一个 CLI 在 edge-gateway 的任务里发一句（回放带用量），再 `/compact 重点` | 输入框旁小环亮起，点开「压缩由 Codex 自己管」；`/compact` 那一次「Codex 不接收重点」一行、「CLI 自己压：Codex 自己压了一次上下文（你让它压的）」 |
| 快照回退 | 「撤销」「撤掉」「回退」；`/undo` | 读 Skill → 列快照 → 复述 → `[ RISK ]`「回到任务 N 开始前（2 个文件）」→ ALLOW / DENY |
| 定时任务 | 「定时」「每周」「每天」「每隔」 | 读 Skill → 复述、ASK 确认 → 设好，PROJECTS 头上的钟多一个 |
| 错过的定时任务 | 打开就有两个（notebook） | 一打开跳到 notebook 的新任务页：`[ MISSED ]` RUN NOW / SKIP；另一张在「日期统一成 YYYY-MM-DD」那个对话末尾；钟里看详情、REMOVE |
| 留着的 dev server | 「dev server」 | 过程里一行「留着在跑」；右栏 TERM 头上一个 DEV 小片，点一下停 |
| 花费上限 | 设置里上限设得比 0.42 低 | 发出去就停、写明原因 |
| CLI 引擎 | 选一个 CLI | 回放 `src-tauri/tests/fixtures/gateway/` 的录音，碰到审批停下 |
| 任务板（`agentsMock.ts`） | PLOOP 下句子带「任务板」或「member」（在没有任务在跑的项目里，比如 peck） | 过程最上面「任务板」：worker 1、2 和 member 1 在跑、worker 3（和 worker 1 划的同一个文件）排队，worker 1 做完它才跑；点 member 1 展开它的几步，它派的 explorer 再挂一层；设置 → AGENTS 五块、member 默认 40 轮同时 2 个 |
| 子代理（`agentsMock.ts`） | PLOOP 下句子带「派」「子代理」 | 两步「Spawn explorer: …」同时在跑，各自的几步挂在下面（头上「explorer · 模型名」，右边「1/30 轮」），两个都完了主线才接着改；任务头上「子代理 2 在跑」，点了滚到它们 |
| 联网搜索、读网页 | 句子带「搜一下」「上网」「最新版」 | 「Searched the web 2 times, fetched 2 pages」一组：两步「Search the web: …」带来源，两步「Fetch …」（一步读到正文，点开头上的网址能用浏览器打开；一步跳到别的网站、失败）；回答写出处；花费行「搜 2 次」 |
| 额度提醒 | 选一个 CLI；QUOTA 卡片里 Claude Code 每周 90% | 输入框上方「Claude Code 每周已用 90%，换成 Codex？」 |
| 换引擎接着发 | PLOOP 做完的任务里换成 CLI 再发 | 一行「换成 X 接着做：前面 1 次……带过去了」 |
| 独立工作区 | peck 项目点亮 WORKTREE；带「冲突」 | 对话里 recap 下面 APPLY / DISCARD；冲突时 APPLY ANYWAY |
| 截图、贴图 | 带「截图」；输入框粘贴或拖进图（整个输入框都收，拖着时描一圈虚线） | 演示图；小图，发出去一行「这一句带了 1 张图」，YOU 下面一张小图，点了放大 |
| `+` 附文档 | 输入框 `+` → 文件 / 文件夹（浏览器里给两个文档和一张图的演示路径） | 文档、文件夹在上面排成小片（图标不同）、能叉掉；图进上面那排小图；发出去清掉，YOU 下面照样画成小图和小片（不显示路径那段话）；拖进来的文档也进小片 |
| TRUST 卡片 | peck 项目里选一个 CLI 发一句（刷新后头一次） | 任务开头 `[ TRUST ]`：点 TRUST 留一行「信任了这个文件夹」再接着回放，项目设置里「CLI 信任这个文件夹」亮了；点 NOT NOW 直接回放，这次打开不再问 |
| FILES 的 ALL | 右栏 FILES 头上点 ALL | 藏着的 `dist`、`node_modules` 变暗列出来，点开 `node_modules` 读出一层 |
| 一次问几个问题 | PLOOP 下句子带「几个问题」 | `[ ASK ]` 一张卡片一次一个问题：头上 VER、TRUST、FILES 三个页签（答了带勾）、← → 翻，单选的点了自动翻到下一个；最后一行 Other 有自己的圆圈，选着一项时写的字算补充；都答了 SUBMIT，过程里一个问题一行 |
| 跑着时发话 | 任务在跑（比如停在卡片上）时在输入框打一句 ↵ | PLOOP、Codex、Claude Code、Muse：过程里多一行 YOU；Grok：输入框上面排一片，这一轮做完自动发成下一句 |
| 两个环 | 输入框右下角 | 左边 12 个点是上下文，右边实心弧是额度：PLOOP 点开「本月 $0.42 / $20」；Claude Code、Codex 点开两个窗口；Muse、Grok 读不到，虚线、点开原因加近 5 小时 / 7 天 |
| 附图放大 | 输入框贴一张图，点小图 | 盖住窗口的大图，点一下或 Esc 关 |
| 斜杠命令 | 输入框打 `/` | Peck 自己的八个 + 项目里装着的插件带的（peck、notebook 装着 Git Plan：`/git-plan:plan` 这几个，选了填成「/名字 」等参数）；`/undo` 发一句话给模型、`/model` 提示不转发 |
| 插件（`pluginMock.ts`） | 设置 → PLUGINS；右键项目 → 项目设置 → PLUGINS | 设置页两组卡片 + MCP SERVERS，点卡片开小窗（每个项目都装、按项目装和开关、它带的东西）；三个项目三种状态：notebook 装着旧版（卡片上青色版本号、小窗里 UPDATE）、peck 装着最新的（REMOVE 点两下）、edge-gateway 没装（INSTALL）；装了以后项目设置的 SKILLS 多一行。在装着 Git Plan 的 peck 项目里发一个任务，做完后 recap 里多一行「Git Plan 提交了 …」 |

- 其他入口：`@` 旧任务；ONE SHOT；项目设置（FOLDERS、ENV、PLUGINS、SKILLS、MCP、记忆）；设置各组；FILES（peck 的 `docs/`、`public/` 里有各种文件；对话里点改过的文件名在这里打开）；AGENTS 头上的 ⓘ。TERM 头上的 dev server 小片见上表。
- 终端只认 `ls`、`cd`、`pwd`、`git status`、`git diff`、`pnpm check`、`pnpm test`（失败）、`pnpm dev`（一直跑）。
- 切明暗：内置浏览器 `resize_window` 的 `colorScheme`，或在控制台改 `document.documentElement.dataset.theme`；截图只放会话临时目录。
- **内置浏览器的坑**：面板隐藏时 CSS 动画停在第一帧（菜单、弹层截图里看不见，用 JS 读 DOM、点按钮）；按 ref 点会点偏，用截图坐标或 JS 点；关键结果用 `read_page` 或 JS 读页面确认；控制台记录跨刷新还在，看时间戳。
- **截清楚的图**（v1.1.11）：内置浏览器的面板常被缩小、截图糊。PLOOP 里用 `screenshot_page`；别处直接跑它的脚本：`osascript -l JavaScript -e "$(cat src-tauri/src/harness/tools/page.js)" <url> <out.png> 1360 860 <light|dark|空> 1 "<一段 JS，async 函数体>" 300`（参数顺序见脚本头）。每次是新开的页面，`?mock` 的状态要在那段 JS 里点出来；离屏时计时器变慢，JS 最多跑 30 秒。
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

- 日常只跑改到的模块：末尾加过滤，如 `-- snapshot::`；`loop` 模块要写 `'harness::r#loop'`（写 `harness::loop` 一个也匹配不到）。收尾才全跑。
- **在 Peck 的沙箱里跑**（PLOOP 开发 Peck 时）：命令带 `PECK_SANDBOX=seatbelt`，要再套一层沙箱的那十几个测试（沙箱、高危、后台命令里的）写一句「跳过」、算通过，全量 20 多秒；要验它们在终端里跑。Peck 给的临时目录在「Application Support」下面、带空格，测试里拼进 shell 命令的路径要加引号。
- **每次用新的临时目录**，不碰正式数据（模拟授权次数记在数据目录里，重复用会失败）。
- **测试撞号**：任务前快照的登记表、Gateway 的控制表都是整个进程共用、按任务号存的，每个临时库的任务号都从 `run-105` 开始：会发任务的测试先 `create_run` 占一个大号，各测试不一样（现在用了 700105、700106、700201–700205、905001、910_001、920_001、930_001、930_002、940_001、960_001、970_001），新测试另挑一个。
- 要 git 仓库的测试（`git_commit`、收尾 Hook）用 `harness::git::testing` 起临时仓库（自己配 `user.name`，不碰站主的 git 配置）。
- Peck Loop 的测试用假模型（`ScriptedModel`）跑完整的循环（子代理一批叫醒一次、worker 和 auditor、划文件的 worker 同时跑和排队、member 再派 explorer、只读的一批同时跑、原地打转、项目里的脚本 Hook 和开场时、MCP 按需加载、读 Notebook 和 PDF 都在 `harness::r#loop` 里；CLI 的脚本 Hook 用 `gateway::runner` 里的假 CLI 跑一整个任务；读 PDF 的测试用 `cupsfilter` 现做一份 PDF；会派子代理的测试各用自己的 `run_id`，子代理的控制通道按任务号登记），外部 MCP 用 sh 写的假服务器。快照回退、定时任务的工具用 `harness::extras::testing` 搭的带快照的假项目，Peck Loop 和本机 MCP 两边各走一遍。沙箱、Quick Look、AppKit 那几项只在 macOS 上编进来。
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
- **Muse** 只在信任的文件夹里才连 Peck 挂在会话上的 MCP（提 Skill 这些）：TRUST 卡片点过的项目带 `--trust-workspace`（v1.1.8；`live_small_task` 里用 `PECK_LIVE_TRUST=1`）。不花额度地验：起 `muse serve [--trust-workspace]`，只发 `initialize`、`session/start`（`config.mcpServers` 指到一个本地假 MCP），看它连不连。

## 升级测试

1. 把正式数据目录复制到临时目录（`cp -Rc ~/Library/Application\ Support/Peck <副本>`；`peck.db`、`-wal`、`shadow/` 都要）。
2. 用新版指过去打开：`PECK_DATA_DIR=<副本> PECK_SHADOW_ROOT=<副本>/shadow /Applications/Peck.app/Contents/MacOS/peck`（后台起，看完 `kill $!`）。
3. 看：改了 schema 的版本生成 `before-v<新 schema>` 备份；项目、任务、事件和升级前一样（`sqlite3` 比行数，不比文件 md5）；旧任务的 Diff 能打开。旧任务快照记录里是正式影子仓库的路径，只看不撤销。用完删掉副本。

**备份恢复**：把正式数据 `backups/` 里的一份 `peck-…-before-v<N>.db` 复制到临时目录改名 `peck.db`，照第 2 步打开：它照常升到现在的版本、再留一份备份。

## 夹具（Gateway）

- `src-tauri/tests/fixtures/gateway/<名字>/`：`raw.jsonl`（原始输出，去掉了 Key、账号、邮箱，路径换成 `{{WORKSPACE}}`、`{{HOME}}`）和 `events.jsonl`（转成的统一事件）。四家 CLI 各一个小任务，四家 API 各一次带工具的请求和回放；`models-mock.json` 是 `?mock` 的模型列表。
- 改了转换器、确认新结果对：`PECK_REGEN_FIXTURES=1 cargo test gateway` 重写期望文件，再看 `git diff`。

## 真 CLI、真 API

- 规矩和次数见 roadmap「现行授权」；项目用仓库外的 `personal-cli-studio/projects/gateway-smoke`（要 git 仓库的用 `projects/git-plan-smoke`）。
- 入口是各模块 `#[ignore]` 的 `live_*` 测试，按环境变量拿参数（Key 从 Key 目录的文件读，不打印；列 Key 目录会被权限检查拦，真 API 要站主给路径或自己跑）：
  - API：`gateway::providers` 的 `live_two_requests`。`PECK_LIVE_MODEL`、`PECK_LIVE_KEYS`（Key 目录）、可选 `PECK_LIVE_REGION`、`PECK_LIVE_EFFORT`。
  - 原生联网搜索：同一个模块的 `live_web_search`（参数同上，只认有原生搜索的模型）：一次带着 Peck 的工具让它搜，一次把带搜索原件的回答放回去再问一句（验回放）。
  - Peck 的 `web_search`：`gateway::search` 的 `live_tavily`（`PECK_TAVILY_KEY`）、`live_deepseek`（`PECK_DEEPSEEK_KEY`），各搜一次。
  - Mathpix（`ocr_pdf`）：`gateway::mathpix` 的 `live_mathpix`，`PECK_MATHPIX_ID`、`PECK_MATHPIX_KEY`（Key 目录的 `mathpix` 第一行、第二行）、`PECK_LIVE_PDF`（一两页的 PDF，按页收费）：识别一次、确认它那边删掉了。
  - 划文件（v1.1.3）：同一个模块的 `live_ploop_workers_split_files`，参数同下（`PECK_LIVE_SUB_MODEL` 是 worker）：主角色派两个 worker 各建一个文件，主角色最多 5 轮、worker 各 2 轮。
  - PLOOP 子代理：`harness::r#loop` 的 `live_ploop_spawns_explorers`。`PECK_LIVE_KEYS`、`PECK_LIVE_MODEL`（主角色）、`PECK_LIVE_SUB_MODEL`（explorer）：在临时项目里让主角色一次派两个 explorer、等报告、收尾（主角色约 3 次请求，explorer 各 2–3 次）。
  - CLI：`gateway::engines::{codex,claude_code,muse,acp}` 的 `live_small_task`。`PECK_LIVE_CODEX` / `PECK_LIVE_CLAUDE` / `PECK_LIVE_MUSE` / `PECK_LIVE_ACP`（`cli/bin/…`）、`PECK_LIVE_CWD`、`PECK_LIVE_PROMPT`，可选 `PECK_LIVE_MODE`、`PECK_LIVE_CONFIG_DIR`、`PECK_LIVE_MCP`、`PECK_LIVE_MODEL`、`PECK_LIVE_EFFORT`、`PECK_LIVE_LOG=<文件>`、`PECK_LIVE_TRUST=1`（带信任开关）、`PECK_LIVE_APPEND=<一句话>`（第一步工具开始时追加这一句，验中途插话）。审批一律批准一次，提问答「好」。
  - 收尾 Hook（Codex，花一个小任务、两轮）：`gateway::engines::codex` 的 `live_stop_hook_commits_the_round`，`PECK_LIVE_CODEX`、`PECK_LIVE_CWD=<一个 git 仓库>`（用仓库外的 `projects/git-plan-smoke`）。
  - CLI 的模型列表（不花额度）：`PECK_LIVE_CLI_DIR=<cli/bin> PECK_LIVE_ENGINE=codex cargo test --manifest-path src-tauri/Cargo.toml live_list_models -- --ignored --nocapture`。
  - 跑法：`cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture <测试路径>`，`PECK_DATA_DIR` 照样指到新的临时目录。

## 额度与花费的真实来源

| 来源 | 官方入口 | 注意 |
|---|---|---|
| Codex | [App Server](https://learn.chatgpt.com/docs/app-server)：`account/rateLimits/read`、`account/rateLimits/updated` | 不用 API Key 推订阅额度；credits 不当 USD |
| Claude Code | [status line JSON](https://code.claude.com/docs/en/statusline)：`rate_limits`；Peck 现在跑 `claude -p /usage` | 空闲不刷新；缺字段不是 0；`cli/bin` 的 Claude Code 要先登录 |
| Muse CLI | `muse usage --json`（[更新记录](https://dev.meta.ai/docs/muse-code/changelog)）；会话里是 `usage/read`、`usage/changed` | 用 `allowance.asOfMs` 作数据时间；`allowance: null`、`reason: server_sent_no_usage` 是服务器没有在计的窗口、不给（2026-11-16 到 11-24 下午一直这样，11-24 晚上又有了） |
| Grok CLI | [`/usage`](https://docs.x.ai/build/modes-and-commands) | `grok usage` 只在 daemon 线路上给套餐数据，shell 线路报「Run /usage inside grok」 |
| DeepSeek API | [GET `/user/balance`](https://api-docs.deepseek.com/api/get-user-balance/) | 用已配置的 Key；换 Key 清旧余额 |
| Anthropic API | [Usage/Cost Analytics](https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api) | 要 Admin 权限，普通 Key 显示未知 |

要点：只在对应客户端运行时监测，退出就停并留最后一次快照；未知不填 0，币种不直接相加，估算标出来；不采集提示词、会话正文、环境变量；不为查额度启动已关闭的客户端，不用「你好」探针或模型 turn 查额度、余额。
