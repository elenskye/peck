# 当前交接

更新：2026-11-24。**v1.1.8 做完了**（Muse 在 Peck 里开了头，Claude Code 审过、补完）；正式版还是 1.1.7，等 1.1.9 做完一起装。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md](./docs/architecture.md)「CLI 四家」「额度与花费」，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。在做的版本：1.1.9。

## 这一版做了什么（1.1.8）

1. **开发准则**（站主定，优先级最高，进了 CLAUDE.md「界面」）：说明文字能不写就不写，必须写就写短。
2. **输入框 `+`**（Muse 做的，收拾过）：附文档（多选）和文件夹（一次一个），最多 10 个、每个文件 10MB 以内；文件拷进这个任务的附件目录、文件夹只带路径，写在这一句后面，CLI 和 Peck Loop 都看得到。
3. **TRUST 卡片**（站主在提问里定的：卡片上点 TRUST 就行、四家都管）：起 CLI 之前看它信没信任项目文件夹，没信任、站主也没批过就弹卡片，点了才起。批了记在项目设置里（项目设置多一个开关「CLI 信任这个文件夹」），以后每次带各家官方的开关：Muse、Codex、Claude Code 只对这一次进程生效，Grok 用它自己的 `--trust` 记进它的信任表。Muse 原来做的「打开终端自己点」去掉了。CLAUDE.md「边界」那句跟着改了。
4. **FILES 的 ALL**（站主在提问里定的：加开关、默认藏）：有照 `.gitignore` 藏着的就多一片 ALL，点亮了变暗列出来，藏着的文件夹点开才读一层。底下那行「照忽略规则藏了 N 个」去掉了。
5. **Muse、Grok 的订阅额度**：两家现在都读不到，不是 Peck 读错了——Muse 的官方接口对这个订阅回 `server_sent_no_usage`（命令行和会话里都一样，11-16 那次是最后一次有数）；Grok 的 `grok usage` 在这台机器的线路上不给套餐数据，只能在它里面 `/usage` 看。卡片上的原因改短改准了，手工记一笔还在。Muse 原来写的「跑一次 Grok 任务会自动带回」不对，去掉了。
- 数据库没动（schema 11）。项目设置多了 `trusted`；事件多了 `trust.prompt`、`trust.resolved`；`AgentTask` 多了 `trusted`；`FileListing` 多了 `ignored`，新命令 `files_children`、`project_trust`、`dialog_pick_uploads`。没加依赖。

## Muse 那一轮的情况（站主问的）

- **全量测试是谁在跑、跑完了吗**：是 Muse 自己的命令工具在跑（`cargo test`，它给自己设了等 5 分钟）。5 分钟没出结果它就把命令放到后台、接着改文档，然后说「测完我就写 HANDOFF」并结束了这一回合。CLI 的回合一结束 Peck 就收尾、关掉它的进程，后台那条命令跟着没了，也没有谁再叫醒它——所以 HANDOFF、装正式版、提交都没做。那 5 分钟大多花在编译上，测试本身只要几秒。
- **留下的问题**：前端有一处语法错（`MiddleColumn.tsx` 多了一个 `}`），`pnpm check` 过不了——它那边跑不了前端检查，没发现。已修。
- **三个问题只弹出第一个**：Muse 一次问了三个问题，Peck 的卡片只画了第一个问题的选项，站主的回答被原样填给了三个问题。1.1.9 修。

## 验证

- **自动测试**：全量 `cargo test` 过（355 个；新加：TRUST 卡片等回答和收着别的控制、各家的信任开关、Codex 的信任表、NOT NOW 记着、藏着的文件清单和按层读、Muse 没给数据时的原因）；`pnpm check`、`pnpm build` 过。
- **`?mock`**：peck 项目里选 CLI 发一句 → `[ TRUST ]` → 点 TRUST 留一行、接着回放；FILES 的 ALL 点亮、`node_modules` 点开读出一层；`+` 的小片。
- **真 CLI（没花额度）**：`muse serve --trust-workspace` 建会话、用本地假 MCP 看——带了开关它才连 Peck 挂的 MCP；`codex -c projects={…} mcp list` 带了才读项目的 `.codex/config.toml`（点分的写法它不认）。
- **没验的**：Grok 的 `--trust`（会往它的信任表里写一条，没替站主试；头一次在 Grok 任务的 TRUST 卡片上点了就知道，没记上会写一行）；Claude Code 的 `--settings`（`cli/bin` 的登录过期）；TRUST 卡片在正式版窗口里。

## 留给站主的

- **废纸篓**里有 `agy-…`（223 MB，Antigravity 的程序）和 `agy-link-…`（链接）：确认不要了自己清空。它的登录信息在你家目录里它自己的位置，我没碰。
- **第一次打开新版**：可能弹钥匙串授权框；界面设置会在这时从 v1.1.1 以前的样子迁移过来（单干选的那个当 PLOOP 主角色）。
- **1.1.4 留下的**：装了 Git Plan 1.0.0 的项目要点 UPDATE 才有 HANDOFF 检查（设置 → PLUGINS 点开 Git Plan 能一次看全部项目）；Mathpix 的 Key 在设置 → PROVIDERS → TOOL KEYS 填。
- **仓库里多了 `.agents/skills`**（指向 `.claude/skills` 的链接）：站主在 Peck 里第一次点开 peck 项目时 Peck 认结构建的，没跟踪、我没动，要不要提交站主定。
- **Claude Code 重新登录**：设置 → CLI → LOGIN 里的 Claude Code（`cli/bin` 那份）登录过期了。登好以后要核对 `/compact 重点` 的重点有没有生效：在一个 Claude Code 任务里 `/compact 某个重点`，压缩那一行写了「重点：…」就是生效了。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.8 的全部提交。
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
