# 当前交接

更新：2026-11-25。**v1.1.10、v1.1.11 做完、已提交、已装进 /Applications**（旧包进了废纸篓；第一次打开可能连弹几个钥匙串框）。站主写明了版本号（「Peck v1.1.10 & 1.1.11」）、不分 Phase。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/design.md](./docs/design.md)「权限」「过程」「输入框」和 [docs/architecture.md](./docs/architecture.md)「Peck Loop」，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 1.1.10（站主用 PLOOP 开发 Peck 的那一轮）

PLOOP（Claude Opus 5.5 当主角色，AUTO）做的：NEW TASK 去掉 WHO；QUOTA 撑满居中、额度卡片两列；中栏正文 15px；要读的换 Body、装饰的留像素字（规则进 design.md「字」，方案 `peck` 里还是旧的，`app.css` 盖掉）。它跑满 100 轮停在写 HANDOFF 前，我审过、补完文档和检查后提交。

**那一轮的经过（日志在正式库任务 184）**：四个决策点一个一个问（四张 ASK 卡）；改完前端以后约 40 轮花在「怎么截 `?mock` 的图」上：截 App 窗口没有屏幕录制权限 → 起无头 Chrome（命令的 HOME 是 Peck 给的，Chrome 找不到钥匙串，系统连弹「Keychain Not Found」，就是站主截图里点不掉的那一串；渲染进程在沙箱里也崩了）→ 最后自己写了个 Swift 小程序用 WebKit 截、转成 PDF 用 `read_pdf` 看。中间弹了 6 张高危卡片（`rmdir` 空文件夹、删 Peck 给的 HOME 里的临时目录、写一个 heredoc 脚本都算高危），有一张等了 76 分钟。站主拖进去的第二张截图没进来（下面第 2 条）。这些都在 1.1.11 修了。

## 这一版做了什么（1.1.11）

站主列的 11 条加上从日志里看出来的，站主在提问里定了四件事（高危学 Claude Code 的 Auto、EDIT 一起放宽；命令照 Claude Code Desktop 那样显示；加截网页的工具；Other 里写的字算补充），中途又加了一条（轮数和子代理）。

1. **附件**：`+` 挑到的图当附图（进小图那排、能放大、发给能看图的模型；大于 4 MB 先缩，`upload_read_image`）。拖放整个输入框都收（以前只认打字那一行，落在附件那几排上就没了；浏览器没给类型的按扩展名认），拖进来的文档也能附（先存进 `harness-output/staging/`，`upload_stage`）；附不上写一句原因。
2. **对话里的附件**：YOU 下面画小图（点了放大）和文档、文件夹小片，不再显示「（站主附了 N 个文档，拷在这些本机文件里…）」。后端照旧把路径写在这一句后面（模型照它读），前端按格式拆（`src/lib/attachments.ts`）；PLOOP 的附图也存成文件、写进这一句。
3. **PLOOP 能看图**：`read_file` 读到 png、jpg、gif、webp 把图附给模型（以前回「二进制文件」，`+` 附的截图它其实看不到）。
4. **ASK 卡片**：一次问几个时一张卡片翻着答（头上一排短名当页签、← →、单选点了自动翻）；PLOOP 的 `ask_user` 收 `questions`（一次最多 4 个），系统提示让它一次问完。Other 有自己的圆圈或方框；选着一项时写的字算补充（「选的那项、补充：…」），点 Other 才是只送写的，一项没选时写字自动选上；点来点去字都留着。
5. **高危放宽**（EDIT、AUTO 都一样）：项目里、临时目录里、Peck 给的 HOME 里删东西不问，看不出会做什么的不问，补丁删文件不问。只问删项目外的、删整个项目或 `.git` 或 `git clean -x`、推送和改历史、改系统设置、`curl … | sh`、出沙箱。heredoc 写进文件的内容和注释不再当命令看。CLI 的命令不在 Peck 的沙箱里，另走 `assess_cli`（多看写到项目外、拼出来的命令）。
6. **审批卡片**：`bash_run`、`bash_background` 必填一句 `description`，卡片标题就是它，下面一个小标签写为什么要批（事件多了 `reason`），命令收在「看命令」后面；过程里那一步的标题也是这句（点开先一行 `$ 命令`）。
7. **钥匙串**：命令的沙箱挡掉 `com.apple.SecurityServer`（以前只挡了新的两个），出沙箱跑的那次也挡。原因见上面那一轮的经过。
8. **截网页 `screenshot_page`**：Peck 用 `osascript` 在屏幕外开一个 WebKit 窗口截本机网页（能先跑一段 JS、指定明暗、默认关动画），不用 Chrome、不要屏幕录制权限。工具说明和 `bash_run` 的说明里都写了别自己起浏览器。
9. **轮数和子代理**（站主中途加的）：PLOOP 在跑时任务头「第 37/100 轮」（八成起琥珀），子代理在跑时一个小片「子代理 2 在跑 · 1 排队」点了滚过去，子代理那一步右边「12/30 轮」。新事件 `round.started`。
10. **QUOTA** 订阅额度卡片的折线去掉。
- 数据库没动（schema 11），没加依赖。事件多了 `round.started`，`approval.requested` 多了 `reason`；`EngineRun` 去掉了 `attachments`（附图说明并进了这一句）。

## 验证

- **自动测试**：全量 `cargo test` 过（366 个；新加：高危新规则（PLOOP 这一轮那几条都不再算高危、删项目外 / 整个项目 / 推送 / 下载脚本照问、CLI 另一套）、钥匙串挡住了（真起 `security list-keychains`）、截网页（真起一个本机小服务、跑 JS、截图）、一次问几个问题 + 读图 + 每轮一条 `round.started`；改了：卡片标题和原因、补丁删文件不问）；`pnpm check`、`pnpm build` 过。
- **原型**：`page.js` 截 `?mock` 四种情况（点 QUOTA 后截、JS 抛错、地址打不开、语法错）都对，明暗都截过。
- **`?mock`**（明暗）：ASK 三个问题翻页、自动翻、Other 写字自动选上、点别的字留着、多选加补充、SUBMIT 后过程里的回答；推送卡片（标题、红标签、看命令、ALLOW 以后那一步点开 `$ git push origin main`）；`+` 选到的图进小图、拖一张没类型的 png 和一个 md 到附件那排都收下；发出去 YOU 下面小图加小片、没有路径那段话、小图点开放大；任务头的轮数；派子代理时的小片和「1/30 轮」；QUOTA 没有折线。
- **没验的**：
  - **正式版窗口**：从访达、截图浮窗拖图进输入框（站主说的「第二次拖就没了」没复现，修的是两个最可能的原因：落点不在打字那一行、浏览器没给图的类型）；`+` 选图的系统选窗；装包后第一次打开的钥匙串框。
  - **真模型**：PLOOP 会不会照做——每条命令写 `description`、一次问完几个问题、要看界面时用 `screenshot_page` 不起 Chrome。都在工具说明和系统提示里写了，没花额度真跑。
  - 挡住 `com.apple.SecurityServer` 以后真起 Chrome 不再弹框：只验了 `security` 连不上钥匙串（没敢在站主屏幕上再起一次 Chrome）。
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
| 高危放宽后，项目里被 .gitignore 忽略的文件（`.env`、`node_modules`）用 `rm` 删了不问，任务前快照里也没有，救不回来；`git clean -x` 照问 | 用一阵看要不要把删 `.env` 这类单独算高危 |
| `screenshot_page` 每次是新开的页面：只在内存里的状态（`?mock`）要在 script 里点出来；离屏时页面的计时器变慢，等动画、轮询的要多给时间 | 记着 |
| 拖进来的文档先存在 `harness-output/staging/`，没发出去的也留着（14 天没动的随输出目录一起清） | 记着 |
| 命令的说明是模型写的，可能和命令对不上；卡片上点「看命令」能看原文 | 记着 |
| CLI 的审批卡片标题照各家转换器给的：Claude Code 的 Bash 带说明（英文），别家还是「跑 命令」 | 记着 |
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

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.1.0 新加的：recap 里收尾提交的那一行。 1.1.2 新加的：设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。1.1.11 新加的：审批卡片的原因标签（`.agent-card-reason`）和「看命令」、ASK 的页签（`.ask-tab`）和 Other 那一行（`.agent-other`）、YOU 下面的小图和小片（`.you-attached`、`.you-shot`）、输入框拖放时的虚线（`.composer.is-drop`）、任务头的轮数和子代理小片（`.task-round--warn`、`.task-agents`）、子代理那一步的「N/上限 轮」、命令那一步点开的 `$ 命令` 行（`.out-cmd`）。1.1.10 新加的：中栏 15px 正文、卡片和标题换 Body（方案里还是像素字）。1.1.9 新加的：额度环（`.quota-ring`、`.quota-rows`）、ASK 里一个问题一段（`.ask-q`）、追加的那一行 YOU（`.you--mid`）、排着的话的小片、附图放大（`.lightbox`）。1.1.8 新加的：`[ TRUST ]` 卡片、FILES 的 ALL 和变暗的行（`.pk-row--dim`）、`+` 附的小片（`.composer-upload`）。1.1.7 新加的：`/compact` 那一行（`.you-compact`）、CLI 的小环（待更新变暗 `.is-stale`、弹层里「压缩由 X 自己管」）、读网页那一步的网址行（`.web-head`、`.web-link`）。1.1.6 新加的：设置页的统一排法（`.set-scope` 的行高、控件宽、两行的块 `.set-line`、组头 `.set-head`、框里的单位 `.field-unit`）、小窗（`.modal`、`.modal-scrim`）、插件卡片和像素图标（`.plug-card`、`.plug-icon`、`.plug-hero`、小窗里的清单 `.plugin-detail`）、记忆小窗（`.memo-*`）。

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
