# 当前交接

更新：2026-11-24。**v1.1.7 做完了，已装进 /Applications**。站主写明了版本号、不分 Phase，照两份 brief（上下文压缩、读网页）做，先压缩后读网页。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md](./docs/architecture.md)「上下文」「联网」「CLI 四家」，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.7）

1. **`/compact [重点]`**（站主在提问里定的：落成这个任务的一次新记录）：
   - PLOOP：三步都做（落盘、清旧输出、叫一次模型写交接摘要），重点接在写摘要的要求后面、进 `PreCompact` Hook 的 `custom_instructions`（`trigger: manual`）。用这个对话存的模型，用不了报原因不换；花费照记、算进上限。原来那个免费、只做前两步的 `/compact` 去掉了。任务在跑时提示「跑完再压」。
   - CLI：续上它在这个任务里的会话转过去（`gateway_compact`）：Claude Code、Grok 发文字命令，Codex `thread/compact/start`，Muse `session/compact`；Codex、Muse、Grok 不收重点，写一行说明。不带引用、记忆、文件夹说明，Peck 替它跑的 Hook 不跑。
   - 界面：那一次不画 YOU，一行「COMPACT 压缩上下文 · 重点：…」。
2. **压完放回**（第 3 步之后，自动、手动都做）：交接摘要 → 站主说过的每一句（超过窗口 3% 省中间）→ 新的环境块 → 加载过的 MCP 工具（`<peck-loaded-tools>`，下一句认回来、工具列表不变）→ 最近读过和改过的文件（最多 5 个、窗口的 10%，现读、带行号、放不下留头尾、登记成读过）→ 接着干。清旧输出时 `tool_search` 留着「已加载：…」那一行。
3. **交接摘要长度跟窗口**：目标窗口 × 1.5%（1500–8000 tokens），输出上限 min(模型上限, max(16000, 目标 × 2))。
4. **CLI 的用量小环和压缩那一行**：四家报的「用了 / 窗口」转成 `context.updated`（带 `managedBy`），CLI 任务也画小环（点开「压缩由 X 自己管」，不画压缩线，Muse 说快满了变琥珀，CLI 压过以后变暗等下一条）；它自己压了一次显示成「CLI 自己压」一行。Codex 的上下文用量改成 `last.totalTokens`。
5. **读网页 `web_fetch`**（`harness/tools/web.rs`）：所有模型、哪一档、ONE SHOT 都有，不弹卡片、不花钱；searcher（替掉了 curl）、member 有。地址检查（本机、内网、钉 IP）、同站重定向最多 5 次（跨站停下）、HTML 抽正文转 Markdown（`dom_smoothie`）、文字原样、PDF 存下读前 20 页、只放开头 32 KB 其余落盘、15 分钟缓存（子代理共用）、能停。过程里一步「Fetch …」，点开头上的网址用系统浏览器打开（新命令 `open_url`，只认 http / https）。
6. 顺手修的：压缩那几行、一组工具的摘要在窄栏里换成两行时会压住下一行（`.fold-head` 写死了高度），现在跟着变高；Codex 的 `thread/goal/cleared` 不再冒成一条 raw。
7. **装新版不再留备份**（站主定的）：`install-app.sh` 把刚换下来的和以前留的 `Peck.app.backup-*` 都挪进废纸篓，「应用程序」里只有 Peck.app。
- **新加依赖**：`dom_smoothie ^0.18.2`（Readability；故意用 0.18，和 tauri 共用 `dom_query 0.28`、`html5ever 0.39`，锁文件里没有第二份）、`dom_query ^0.28.0`（开 `markdown`，整个 `<body>` 转 Markdown 用）、`encoding_rs ^0.8.44`（锁文件里本来就有，改成直接依赖）。
- 数据库没动（schema 11）。`AgentTask` 多了 `compact`，事件 `context.updated` 多了 `managedBy`、`warn`，`context.compacted` 的 `step` 多了 `engine`。
- brief 里「接力每段都是新会话」那条没做：接力在 v1.1.1 就去掉了。ACP 初始化时声明 `session.compaction` 没做（字段名查不准，怕 Grok 拒）；收到 `compaction_update`、`usage_update` 照样认。

## 验证

- **自动测试**：全量 `cargo test` 过（350 个，新加：摘要长度、站主的话省中间、放回文件的挑法和预算、`tool_search` 那一行、手动压缩三步和重点进 Hook、压缩后不用再读就能整个覆盖、四家 CLI 的转发和用量、runner 转发不带记忆和 Hook、读网页的地址 / 重定向 / 类型 / 截断落盘 / 缓存 / 停止 / PDF、`web_fetch` 和搜索一起跑）；四份 CLI 夹具重新生成（只多了 `context.updated`）；`pnpm check`、`pnpm build` 过。
- **`?mock`**：PLOOP 任务 `/compact 数据库迁移` 一行 COMPACT 和三步、花费；Codex 任务小环 22.7k / 475k、`/compact` 一行「不接收重点」和「CLI 自己压」；「搜一下」两步 Search + 两步 Fetch（一步失败），点开网址那一行；明暗各看了一次。
- **真跑**（账本在 roadmap）：DeepSeek 3 次（≈ $0.0062）读 docs.rs 一页正文干净、手动压缩的摘要照重点写细；Codex、Muse、Grok 各 2 个小任务，`/compact` 都正常结束、压缩那一行和用量都到了；Grok 的窗口在模型列表里有（500,000）。
- **正式版**：装好了（`pnpm app:install`，旧包都进了废纸篓）；窗口里没点过。
- **没验的**：
  - **Claude Code**：`cli/bin` 的登录过期了（OAuth 刷新不了），没跑成，没花额度；`/compact 重点` 的重点在 stream-json 下生不生效还不知道。
  - 正式版窗口：这一版和 1.1.6 的都没点过。

## 留给站主的

- **废纸篓**里有 `agy-…`（223 MB，Antigravity 的程序）和 `agy-link-…`（链接）：确认不要了自己清空。它的登录信息在你家目录里它自己的位置，我没碰。
- **第一次打开新版**：可能弹钥匙串授权框；界面设置会在这时从 v1.1.1 以前的样子迁移过来（单干选的那个当 PLOOP 主角色）。
- **1.1.4 留下的**：装了 Git Plan 1.0.0 的项目要点 UPDATE 才有 HANDOFF 检查（设置 → PLUGINS 点开 Git Plan 能一次看全部项目）；Mathpix 的 Key 在设置 → PROVIDERS → TOOL KEYS 填。
- **Claude Code 重新登录**：设置 → CLI → LOGIN 里的 Claude Code（`cli/bin` 那份）登录过期了。登好以后要核对 `/compact 重点` 的重点有没有生效：在一个 Claude Code 任务里 `/compact 某个重点`，压缩那一行写了「重点：…」就是生效了。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.7 的全部提交。
- 以前留下、还没验的：1.1.6 的设置页在正式版窗口里；1.1.5 的 CLI 带记忆在真 CLI 上、正式版窗口；1.1.4 的真模型读 PDF / Notebook、MCP 按需加载、CLI 的三个 Hook 在真 CLI 上；1.1.3 的 member 真跑、任务板和 Claude Code 清单在正式版里；1.1.2 的正式版窗口（AGENTS、真派子代理、脚本 Hook 挂真的 `.claude/settings.json`）、searcher / auditor 真跑、百炼新加坡地域；1.1.1 存 Tavily 的 Key、真搜一次；1.1.0 的插件界面和收尾提交；1.0.3 的轮数设置；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。Gemini 原生搜索、Vertex 上的 Claude 和 Gemini 还没真跑过。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 以前版本的任务（v1.1.1 以前的接力、审查，v0.8 以前没记过程的）打开只剩状态和结果那一句（v1.1.5 删了兼容，站主定的） | 记着 |
| 以前 Antigravity 跑过的任务，QUOTA 的图里算进 Peck Loop 那一色（它的用量大多没有花费） | 记着 |
| 旧表 `api_calls`、`api_tasks`、`usage_calls`、`messages` 还在库里（几十行），删任务时那几行清理代码得留着（外键） | 要清就加一条迁移把四张表删掉 |
| 加载一批 MCP 工具，下一次请求的缓存整个重写一次（工具列表变了） | 站主选的这种；常用的话一个对话里只付一次 |
| 没加载就直接调 MCP 工具也照样跑（模型不该这样，供应商一般也不让） | 记着 |
| CLI 收尾 Hook 的卡片等着时按了停：这一轮照样算做完，Git Plan 照样提交 | 记着 |
| CLI 的 Hook 卡片、Peck Loop 的一样：定时任务里碰到没批过的命令会一直等 | 先在对话里跑一次点 ALWAYS |
| `read_pdf` 抽的文字按 PDFKit 的顺序，分栏、表格会乱；`ocr_pdf` 的 `.mmd` 不标页边界 | 模型看说明自己挑 |
| Mathpix 的费用按送去的页数照官网价记（v3/pdf 每页 $0.005），不读它的账单 | 宁可按价算 |
| HANDOFF 检查要 `jq`（macOS 15 起系统自带）；没有就放行 | 记着 |
| 搜索按次的费用按收费价记，Gemini 每月头 5000 次、Tavily 免费档每月 1000 次不扣（会多算） | 宁可多算；在意再加个月度计数 |
| 子代理报告回来以前站主停下任务，派出去的一起叫停，报告不进对话 | 记着 |
| 划文件只管文件工具和命令的沙箱：MCP 工具、主角色自己的命令管不到；项目在临时目录里时沙箱那一层不起作用 | 记着 |
| 命令原地改划给它的单个文件（`sed -i` 先在同一个文件夹建临时文件）会被沙箱拦；划文件夹、或用文件工具改就没事 | 记着 |
| 主角色可能一次回答只派一个、分几次派（真跑时 gpt-6-luna 就这样），同一批的说法就变成几批 | 用一阵看要不要在提示里再强调 |
| 原地打转只认一模一样的调用和结果；参数稍微不同、结果一样的绕圈认不出来 | 用一阵再看 |
| 脚本 Hook 不进沙箱，跑的是站主 shell 的环境（第一次弹卡片就是为这个） | 记着 |
| OpenAI 的来源没有标题，过程里标题写网址 | 记着 |
| Gemini 的来源挂在第一条搜索上（它只在最后一起给） | 记着 |
| 只有界面点开项目时才认结构、装默认插件；定时任务在一个从没点开过的老项目里跑时不会先做 | 记在 roadmap |
| 单独的 Skill 装没装是按「项目里有没有同名的 Skill」认的：项目自己写了一个和库里同名的，会显示成装着的，点 UPDATE / REMOVE 会动到它（库里现在没有单独的 Skill） | 往库里放单独的 Skill 之前先加归属记号 |
| 收尾提交失败（仓库的 pre-commit 钩子不过、没配 `user.name`）时，这一轮的文件已经 `git add` 了，留在暂存区 | 记着 |
| CLI 任务每轮做完多用一轮订阅额度问提交说明；续不了会话的不问，写「模型没给出提交说明」 | 用一阵看烦不烦 |
| Peck Loop 用命令（不是自己的工具）改了一个开头就没提交的文件、别的都没动时，收尾不问、也不提交 | 记着 |
| 插件带的 MCP 服务器（`.mcp.json`）只列出来，Peck Loop 不起 | 有插件要用时再接 |
| CLI 续接会话的花费按「会话号」取差：续接后会话号变了就整个算进去（不会少算，可能多算一次） | 真跑时顺便看 QUOTA |
| 模型只在被点名时才提议存 Skill，主动提的没见过 | 用一阵再看 |
| 定时任务的「谁来干」设的时候不查 Key 和 CLI 在不在，到点起不来记成错过 | 记着 |
| 回退只按整个文件放回，做不到一个文件里的某一段 | Skill 里让模型直接改那一段 |
| 预览的 RELOAD 回到最初的地址；`localhost` 只连 127.0.0.1 | 记着 |
| Vertex 上的 Claude / Gemini 只有离线测试，没真跑过 | 站主开了 GCP 项目再试 |
| PREVIEW 批注只在浏览器存储里；Esc 关设置页时没存的 MCP 编辑会丢 | 记着 |
| 小窗打开的 0.2 秒动画里点下去会落到外面那层、把小窗关掉（动画用 clip-path，没露出来的部分点不中） | 记着 |
| 设置 → PLUGINS 的小窗按项目一个个读状态（项目多了会慢一点）；点开时不在左栏的项目不列 | 记着 |
| 窗口最窄（980）时设置页的名字会被省略得很短（「Vertex AI」只剩几个字） | 记着 |
| 手动压缩不起外部 MCP 服务器：工具列表少了它们，写摘要那一次缓存可能不中 | 记着 |
| 手动压缩是另一次 run：放回的文件登记成「读过」只在那一次里，下一句（新的 run）要整个覆盖还得先读（和平时一样）；`write_patch` 不受影响 | 记着 |
| Grok 续上的会话不再报窗口：小环用这个任务里它以前报过的那个；从来没报过就只写用了多少 | 记着 |
| Codex 压缩不报前后多大；Claude Code 的 `post_tokens` 只算对话，不拿来画 | 等下一条用量 |
| 读网页：要 JavaScript 才出内容的页读不全（会写一句）；走系统代理时只按主机名查地址、不钉 IP | 记着 |
| 读网页的 Markdown 会带转义（`pairs\.` 这类），模型看得懂 | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.1.0 新加的：recap 里收尾提交的那一行。 1.1.2 新加的：LOG 上面的成员名单（`.log-roster`）、设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。1.1.7 新加的：`/compact` 那一行（`.you-compact`）、CLI 的小环（待更新变暗 `.is-stale`、弹层里「压缩由 X 自己管」）、读网页那一步的网址行（`.web-head`、`.web-link`）。1.1.6 新加的：设置页的统一排法（`.set-scope` 的行高、控件宽、两行的块 `.set-line`、组头 `.set-head`、框里的单位 `.field-unit`）、小窗（`.modal`、`.modal-scrim`）、插件卡片和像素图标（`.plug-card`、`.plug-icon`、`.plug-hero`、小窗里的清单 `.plugin-detail`）、记忆小窗（`.memo-*`）。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- 设置迁移（`settings.ts` 的 `loadSettings`）别删：正式版的界面设置还是 v1.1.1 以前的样子（有 `relay`、单干选的 API 模型、没有 `subagents`），第一次打开新版才换。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。
- Mathpix 的接口：认证是两个请求头 `app_id`、`app_key`，错误大多是 HTTP 200 带 `error_info.id`；真跑的入口 `gateway::mathpix` 的 `live_mathpix`。
- 联网搜索的协议各家常变（Gemini 的工具组合还是 Preview、Anthropic 搜索工具按日期出新版本）：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

自定义子代理（站主之前定的下一步）。其他候选在 roadmap「之后要做的」。
