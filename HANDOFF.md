# 当前交接

更新：2026-11-25。**v1.1.9 做完、在正式版窗口里验收了两轮，两轮修的都装进 /Applications 了**（连同 1.1.8）。站主写明了版本号、不分 Phase。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md](./docs/architecture.md)「CLI 四家」「额度与花费」和 [docs/design.md](./docs/design.md)「输入框」，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.9）

1. **任务跑着时照样能发话**：正看着的任务在跑，输入框不锁，↵ 是接着说一句。PLOOP、Codex、Claude Code、Muse 马上送进去（后端本来就留着 `Control::Append`，这次接了界面、把这句话记进过程 `user.appended`）；Grok 的 ACP 一轮里插不了话，排在输入框上面，这一轮做完自动接着发、没做完放回输入框。这时不能用斜杠命令、不能带附件。
2. **输入框右下角两个环**：上下文（12 个点，原来的；弹层里的说明文字都去掉了）和额度（一段实心的弧）。CLI 点开是这家订阅的几个窗口，环取用得最多的；读不到的画虚线，点开一句原因加 Peck 自己记的近 5 小时、近 7 天用量。PLOOP 是本月 API 花费 / 每月上限。
3. **一次问几个问题**：Muse、Codex、Claude Code 一次问几个时，卡片一个问题一段、各选各的（能多选的是方框），都答了才能 SUBMIT，每个问题的答案各自送回去。以前只画第一个问题的选项、一个回答填给全部。
4. **花费不再「未知」**：Muse、Codex 不报花费，现在按会话的模型查价格表估（标「估」）；以前存下的旧任务读的时候也补上。输入框旁真算不出时不写字。
5. **Muse 的后台命令**：回合说完时问一次 `task/list`，有后台命令在跑就写一行「等 Muse 的 N 条后台命令跑完」、不关进程，等它跑完自己开下一回合接着收（最多 30 分钟，STOP 是不等了）。等的时候追加的话直接开成下一回合。
6. **输入框**长到半个窗口才开始滚；**附图**点开放大。
7. **右栏 LOG 去掉**（站主定的）：页签剩 FILES / ONE SHOT / PREVIEW / TERM（⌘1–⌘4）；对话里每次记录头上的按钮改叫 FILES。PLOOP 的成员名单不再显示（事件里还记着）。
- 数据库没动（schema 11）。事件多了 `user.appended`，`user_input.requested` 多了 `questions`，`Control::Answer` 多了 `answers`。没加依赖。

## 验收时修的（站主让我接管屏幕、在正式版窗口里真点真跑）

1. **Claude Code 跑着时追加一句会卡住**：它把那句并进这一回合（两步之间吃进去）、只回一个 `result`，Peck 按「写几行等几个 result」一直等，任务停在「在跑」（STOP 能收）。现在带 `--replay-user-messages`，按它吃进去时的回显核对；不回显的旧版本 `result` 后 15 秒没动静也收。探针走协议核实过（2.1.322），复测：追加后 0.6 秒自己收尾。
2. **Codex 任务开头那行「没读到这个项目配的 MCP：dw，去 Codex 里点信任」是错的**：Codex 不读 `.mcp.json`（信任了也不读，`codex mcp list` 核实），只读 `.codex/config.toml`。现在 Codex 只拿后者比。
3. **附图放大超出窗口**：大图按原尺寸画、下半截跑到窗口外。现在按窗口缩。
4. **`+` 的选窗**：从当前项目文件夹开始（以前停在上次去过的地方）；附文件夹的那个不再写「选一个文件夹加进项目」（那是项目设置的提示）。
5. **额度弹层**：读数过了重置时间的窗口只写「已重置」、不画旧百分比（Muse 的读数还是 11-16 的），都过了就当读不到、显示 Peck 自己记的近 5 小时 / 7 天；「每周 Fable」「估 $0.181」不再折成两行。

## 验收第二轮修的（11-25，正式版窗口真跑 Muse、Codex、Claude Code、PLOOP 和几家 API）

1. **CLI 的收尾 Hook 卡片看不见、任务卡在「等你」**：CLI 的收尾 Hook 在 CLI 说完以后才问，界面一见回合结束就把还没批的卡片全藏了。现在回合结束时没批的照旧作废，之后才问的照样画，任务真停了才藏。
2. **Claude Code 在 Peck 里没有清单**：`claude -p` 默认不给 `TaskCreate` / `TaskUpdate`（工具列表里只有 `Task`），站主定了每次带官方的 `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`（只对那一次进程；CLAUDE.md「边界」写了这个例外）。
3. **PREVIEW 认不出 `python3 -m http.server` 起的 dev server**：它把地址写成 `http://[::]:8765/`，而且输出不在终端时会攒着不写。后台命令带 `PYTHONUNBUFFERED=1`、`[::]` 也认；认出的端口和存着的地址不一样就换过去（以前只在没存过地址时用）。
4. **提问卡片里模型写的 Markdown 原样露出来**（`**时间**`、列表挤成一段）：第一行当问题，后面按 Markdown 画成小字；过程里的 ASK 记录一样。
5. **失败时同一句错画两遍**（过程里一条、底下「没做完：…」又一条）：一样就只画一次。
6. **设置 → KEYS 还写着 ⌘1 – ⌘5 和 LOG**：改成 ⌘1 – ⌘4。
7. **Grok 的项目 Hook 跑两遍**：Grok 在它信任过的文件夹里自己也跑项目 `.claude/settings.json` 的 Hook（它的 Claude 兼容），Peck 又替它跑一遍。现在按 `grok inspect` 的 `projectTrusted`：信任了就只替它跑插件带的（和 Claude Code 一样）。
8. **「连不上供应商」**：任务板那组主角色等 auditor 86 秒后，下一次请求 7 毫秒就断了，原因查不出（直连 DeepSeek 空闲 95 秒也不断）。报错现在带上最里层的原因（不含地址和 Key），连接池的空闲连接 30 秒就丢。

## 上一版（1.1.8，提交 `7a7fbec`）

说明文字的准则进 CLAUDE.md；输入框 `+`（附文档、文件夹）；TRUST 卡片（四家都管，批了记在项目设置里、以后每次带各家官方的信任开关；Grok 用它自己的 `--trust`）；FILES 的 ALL；Muse、Grok 读不到额度时写明原因。项目设置多了 `trusted`，事件多了 `trust.prompt`、`trust.resolved`。

## Muse 那一轮的情况（站主问的）

- **全量测试是谁在跑、跑完了吗**：Muse 自己的命令工具在跑。它等了 5 分钟没出结果，把命令放到后台、接着改文档，说完「测完我就写 HANDOFF」这一回合就结束了。照 Muse 的设计，后台命令跑完它会自己再开一回合接着干（今晚真跑核实过）——但 Peck 以前在回合一结束就关掉它的进程，后台的测试跟着没了，它也没机会回来。**这是 Peck 的问题，1.1.9 修了**（第 5 条）。
- **它留下的**：前端一处语法错（`MiddleColumn.tsx` 多了一个 `}`，它那边跑不了前端检查）、HANDOFF 没写、没装、没提交。都补上了。
- **三个问题只弹出第一个**、**花费未知**：1.1.9 的第 3、4 条。
- **额度**：今晚 Muse 的额度又读得到了（任务里报的、命令行读的都有：5 小时 14%、每周 10%）。11-16 到今天下午读不到时，它的服务器回的是「没有在计的窗口」（`server_sent_no_usage`），那 3 小时的任务里它也一次没报——原因在它那边，我只能猜（可能那段时间用的 Spark 1.3 没开计量窗口；今晚窗口是我用默认模型试的时候开的）。Grok 在这台机器的线路上官方就不给，只能在它里面 `/usage` 看。

## 验证

- **自动测试**：全量 `cargo test` 过（361 个，验收时加了 Claude Code 追加并进回合 / 下一回合 / 不回显三种收尾、Codex 只读 `.codex/config.toml`；新加：追加的话到引擎也进过程、几个问题各自的选项和答案、Claude Code 的多问题回答、CLI 花费估算和读旧事件时补、Muse 等后台命令回来和等的时候按 STOP）；`pnpm check`、`pnpm build` 过。
- **`?mock`**（明暗各看过）：三问题卡片；跑着时追加（PLOOP 进过程、Grok 排队后自动发）；两个环和各自的弹层；附图放大；输入框 40 行时长到 413px 开始滚；LOG 页签没了；TRUST 卡片；FILES 的 ALL。
- **真跑**（账本在 roadmap）：Muse 3 次——后台命令跑完它自己开下一回合（走协议看的），Peck 的引擎等到了那一回合（Spark、带 `--trust-workspace`），额度事件也到了；Codex 1 次——带信任开关，干活中途追加一句（`turn/steer`），最后照那句做了，花费估出来了。
- **正式版窗口（验收，正式数据）**：QUOTA 页、三家 CLI 和 PLOOP 的额度环、上下文环；⌘1–⌘4 四个页签（LOG 没了、⌘5 不动）；FILES 的 ALL（`.peck` 变暗列出；链接不列是故意的）；`+` 附文件和文件夹、附图放大、40 行输入框长高后滚；真跑——PLOOP（DeepSeek Flash）和 Codex 跑着时追加、照做了，花费标「估」；Grok 的 TRUST 卡片（`grok --trust` 记进它的信任表、项目设置 `trusted` 记上）和排队自动发；Claude Code 一次问两个问题各自答回去、带信任开关、追加（修完复测过）。界面设置在第一次打开新版时已经迁移好（有 PLOOP、subagents）。测试时换过的模型都改回去了。
- **正式版窗口（验收第二轮，11-25）**：设置页各组（统一排法、GENERAL 的轮数、AGENTS 五块、MEMORY 小窗加一条、PLUGINS 卡片和 Git Plan 小窗、PROVIDERS、ACCESS、LOOK、ARCHIVE、KEYS）；真跑——Muse：项目记忆、开场 Hook、一次问两个、跑着时插话、后台命令等到它自己开下一轮；Codex：记忆、Hook、那行 MCP 提示没了、收尾 Hook 卡片看得见且批了能续上会话；Claude Code：记忆、Hook、清单（带开关后 2/2）；Grok：记忆、Hook、不再跑两遍；PLOOP（DeepSeek）：同时派两个 explorer + 一个 searcher（DeepSeek 搜索）、任务板（2 worker + member + auditor）、反悔（`[RISK]` 卡片）、定时任务（确认 ASK、钟上多一个、删掉）、留着的 dev server 和 PREVIEW（自己换到新端口、能打开、点小片停）、`read_pdf`、`read_notebook`、`tool_search` 按需加载 dw、Peck Loop 的开场 / 收尾 Hook（拦一次）、Git Plan 收尾提交（只提交这一轮的文件）；Qwen + Tavily 搜一次；百炼新加坡地址通（Key 不对）。账本在 roadmap。
- **没验的**：
  - Codex 的多问题卡片：Codex 默认模式下不给提问工具（真跑问过，它说没有），触发不到。
  - Gemini 原生搜索：这把 Key 额度用完（429）。Vertex 上的 Claude / Gemini：gcloud 的 ADC 过期、换不到令牌，请求没发出去。百炼新加坡：要国际站的 Key。
  - Muse 会不会也自己跑项目 `.claude/settings.json` 的 Hook（Grok 会）：没查到它的文档，这次 Muse 任务里看不出跑没跑两遍。
  - 钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」（这两样要改系统设置）、从访达拖图进输入框（台前调度下做不成）、额度真到 85% 时的提醒、Mathpix（没 Key）、通知横幅（截不到）。

## 留给站主的

- **废纸篓**里有 `agy-…`（223 MB，Antigravity 的程序）和 `agy-link-…`（链接）：确认不要了自己清空。它的登录信息在你家目录里它自己的位置，我没碰。验收时换了五次包，旧的 `Peck.app.backup-*` 都在废纸篓。
- **测试项目 tester** 被批了信任（验收时在 Grok 的 TRUST 卡片上点的）：Grok 自己记进了 `~/.grok/trusted_folders.toml`，Peck 的项目设置 `trusted` 也亮着（项目设置里能关）。验收时往里加了一条项目记忆（口令 MAPLE-42）、`notes.txt`、`bg.txt`、`docs/sample.pdf`、`docs/sample.ipynb`；测 Hook 的 `.claude/settings.json` 用完删了（「总是允许」里留着那几条命令的记录）；dw MCP 开过、已关回去。
- **Vertex**：设置 → PROVIDERS 填了 gcloud 配的项目 ID（`gen-lang-client-…`），模型列表里出来了；要用得先在终端跑 `gcloud auth application-default login`（ADC 过期）。
- **git-plan-smoke** 加进了项目列表、装了 Git Plan，Git Plan 在那个测试仓库里提交了一笔 `243787f`（没 push）；Peck 认结构补的 `CLAUDE.md` 链接等没跟踪。
- **第一次打开新版**：可能弹钥匙串授权框；界面设置会在这时从 v1.1.1 以前的样子迁移过来（单干选的那个当 PLOOP 主角色）。
- **1.1.4 留下的**：装了 Git Plan 1.0.0 的项目要点 UPDATE 才有 HANDOFF 检查（设置 → PLUGINS 点开 Git Plan 能一次看全部项目）；Mathpix 的 Key 在设置 → PROVIDERS → TOOL KEYS 填。
- **仓库里多了 `.agents/skills`**（指向 `.claude/skills` 的链接）：站主在 Peck 里第一次点开 peck 项目时 Peck 认结构建的，没跟踪、我没动，要不要提交站主定。
- **Claude Code**：`cli/bin` 那份现在能用了（验收时跑了 3 次）。还没核对 `/compact 重点` 的重点有没有生效：在一个 Claude Code 任务里 `/compact 某个重点`，压缩那一行写了「重点：…」就是生效了。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.9 的全部提交。
- 以前版本留下的没验项，验收第二轮验掉了大部分，剩下的在上面「没验的」。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 排着的话（Grok）只在内存里，只在正看着那个任务时接着发；切到别的任务、重开 App 就不发了 | 记着 |
| auditor 只读但不限在项目里：验收时它翻了 `~/Library/Application Support` 里 Peck 的数据、想跑 `tmutil`（被拒），一组就跑了十几轮 | 用一阵看要不要在它的提示里限死项目、少给轮数 |
| Vertex 和 Anthropic 的同名模型（Claude Haiku 5.5）在主角色那一栏看起来一样 | 记着 |
| dev server 认出新地址后 PREVIEW 只换地址，要点 GO 才打开 | 记着 |
| 主角色问提交说明、任务板 auditor 这类多出来的请求也算真跑次数，按次数给的额度要留余量 | 记着 |
| 任务跑着时追加的话带不了图和附件、用不了斜杠命令 | 记着 |
| Claude Code 收到追加的话在两步之间并进这一回合，或者说完了再开一回合；它已经在收尾时可能送不进去（会写一行）。不回显的旧版本要多等 15 秒才收 | 记着 |
| TRUST 卡片等着时任务不往下走：定时任务碰到第一次要问的项目会一直等 | 先手动发一次、点掉 |
| Muse 留着不停的后台命令（它起的 dev server）会让任务多等到 30 分钟才收 | 按 STOP 就收（照做完算） |
| Claude Code 的 TRUST 卡片按「项目里有没有 `.mcp.json`」问：已经在 Claude Code 里批过的也会问一次 | 记着 |
| CLI 的花费是按价格表估的 API 价，不是订阅真花的钱；旧任务按现在的价格补 | 记着 |
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

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.1.0 新加的：recap 里收尾提交的那一行。 1.1.2 新加的：设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。1.1.9 新加的：额度环（`.quota-ring`、`.quota-rows`）、ASK 里一个问题一段（`.ask-q`）、追加的那一行 YOU（`.you--mid`）、排着的话的小片、附图放大（`.lightbox`）。1.1.8 新加的：`[ TRUST ]` 卡片、FILES 的 ALL 和变暗的行（`.pk-row--dim`）、`+` 附的小片（`.composer-upload`）。1.1.7 新加的：`/compact` 那一行（`.you-compact`）、CLI 的小环（待更新变暗 `.is-stale`、弹层里「压缩由 X 自己管」）、读网页那一步的网址行（`.web-head`、`.web-link`）。1.1.6 新加的：设置页的统一排法（`.set-scope` 的行高、控件宽、两行的块 `.set-line`、组头 `.set-head`、框里的单位 `.field-unit`）、小窗（`.modal`、`.modal-scrim`）、插件卡片和像素图标（`.plug-card`、`.plug-icon`、`.plug-hero`、小窗里的清单 `.plugin-detail`）、记忆小窗（`.memo-*`）。

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
