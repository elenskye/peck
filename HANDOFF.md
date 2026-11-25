# 当前交接

更新：2026-11-25。**在做 v1.2.0**（分 Phase，表和状态在 roadmap「Phase」；版本号收尾才改，现在还写 1.1.12）：Phase 1–5 做完、已提交。正式版是 **v1.1.12**，已装进 /Applications（Claude Code 装的；Peck 在跑时自己换包会把自己换掉、签名也要钥匙串，所以不让 PLOOP 装）。第一次打开会连弹几个钥匙串框。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/design.md](./docs/design.md)，实现在 [docs/architecture.md](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。

## v1.2.0 在做

- **Phase 1、2**：跑完的「下一步」按钮去掉了；PREVIEW 去掉了，dev server 小片在 TERM 头上。
- **Phase 3 提示词库**（PLOOP）：
  - 数据库到 **schema 12**：`0012_prompts.sql` 加 `prompts` 表（全局一份、不分项目），`prompts.rs` 增删改查和 4 个 IPC；升级照旧先备份 `before-v12`。存法和规整见 architecture「数据存放」。
  - 右栏新页签 **PROMPT**（ONE SHOT 和 TERM 之间，⌘1–⌘4 是 FILES、ONE SHOT、PROMPT、TERM），`src/shell/PromptLibrary.tsx`，规整规则前端和 `?mock` 共用 `src/lib/prompts.ts`；样子见 design「右栏」。箭头接的是 ONE SHOT 那个 `onToTask`，另外会收回宽栏。
  - 顺手（auditor 提的）：小窗、大图开着时快捷键只认 ⌘.（以前 ⌘1–⌘4、⌘N 会把小窗连同没存的字卸掉）。
  - `{占位}` 在库里只存、照原样显示（插进输入框见 Phase 4）。没加依赖。
- **Phase 4 输入框里调提示词**（PLOOP）：
  - `/` 弹层按光标算（`slashAt(text, cursor)`）：整句只有「/几个字」时 `/ 命令` 在前、`/ 提示词` 在后，句中（空格、换行后）只列提示词；网址、路径、选了一段、输入法组字时不弹。样子和规则见 design「输入框」。
  - 选了提示词只换「/查询词」（`MarkdownEditor` 的 `insertPrompt`，先核对那几个字还在）；占位照 snippet 的做法用 CodeMirror 的 StateField 记：选中第一个、Tab 跳下一个（排在列表缩进前面），光标走出所有占位、Esc、撤销插入就整组退出（auditor 指出以前留着没填的会一直抢列表的 Tab）。字面的 `{` 写 `\{`，代码里的花括号不算占位；复制和 PROMPT 的箭头也把 `\{` 换成 `{`。占位规则在 `src/lib/prompts.ts`（`expandPrompt`）。
  - 顺手：弹层里说明太长会压到命令名上（`/compact` 那行），改成名字照全、说明省略；↑↓ 选到下面会滚出来（以前选到第 8 个以后看不见）；插件命令只在句首时读。
  - 没加依赖，数据库没动。
- **验证（Phase 3）**：沙箱里全量 `cargo test` 过（374 个，要套沙箱的跳过），新加的：新库建表不备份、从 schema 11 升到 12 先备份（备份还是 11、没有新表、旧数据都在）、再开不再备份；增删改查和排序、规整和上限、重开还在。`pnpm check`、`pnpm build` 过。`?mock` 明暗都看了：加（SAVE、⌘↵）、改（排到最上；只加末尾空白 SAVE 不亮）、删（点两下，焦点回到搜索框）、搜（`{`、正文里的词、两个词、搜不到）、复制（拿到的是正文、绿勾）、放进输入框（接在后面隔空行、宽栏收回、没项目时灰）、⌘1–⌘4（小窗开着时不切）、`&empty` 的空状态。
- **没验的（Phase 3）**：原生窗口里真走 Rust 的那一遍（只有 Rust 测试和 `?mock`）；笔悬停才露（离屏截图没有鼠标和焦点，只看了样式规则）；系统剪贴板（`?mock` 里换掉了 `navigator.clipboard`）；正式数据的升级测试（版本收尾做）。
- **验证（Phase 4）**：`pnpm check` 过；纯函数（`slashAt`、`expandPrompt` 含代码里的花括号、`matchPrompts`）用 node 跑了一个临时检查脚本，48 条都过（不进仓库）。auditor 审了一遍，提的真缺陷（占位抢 Tab）和几条建议改了。`?mock` 明暗：句首 `/` 两组小标题、↓ 走到提示词自己滚出来、`/compact` 那行不压字；`/补` ↵ 插进来选中 `{函数名}`、打字替换、Tab 选中 `{边界情况}`、填完再 Tab 不管；句中「改完以后 /补 再提交」只换「/补」、后面照留，句中只列提示词；网址、路径、`a/`、选一段不弹；Esc 关、接着打字再弹；列表里插一段、留着没填的点到上面一行，Tab 是缩进；Esc 退出占位；点进第二个占位再 Tab 不管；撤销插入占位一起没；列表 Tab / ⇧Tab 照旧；假的组字事件下组字时不弹、组完才弹；选 CLI（Claude Code）插一段发出去，YOU 就是那段字。
- **没验的（Phase 4）**：原生窗口里用真中文输入法打（只有假的 composition 事件；组字时的回车靠 CodeMirror 和原来的判断，没真按过；选中占位后直接用拼音替换也没真打过）；真鼠标点弹层（只用 JS 点过）；`\{` 在复制、箭头里换成 `{` 只有纯函数的检查（样例里没有 `\{`）。

- **Phase 5 读网页**（Codex）：`web_fetch` 的 HTML 正文太短时自动用离屏 WebKit，也可指定 `render: true`。结果标明 WebKit，失败返回原文和原因。抓取、渲染共用 30 秒，按停结束子进程。缓存、Markdown 和超长落盘见 architecture「联网」。
- **验证（Phase 5）**：Codex 沙箱外 17 个相关 Rust 测试通过。覆盖 JS 正文、长原文直读、强制渲染、缓存和落盘、跳转、内网子资源、取消后连接关闭、Peck Loop 入口和本机截图。公开的 TodoMVC React 原文是应用空壳，实测 `web_fetch` 返回 JavaScript 生成的 React 介绍文字。Codex 沙箱内 WebKit 进程起不来，带 `PECK_SANDBOX=seatbelt` 跳过的 4 项已在沙箱外补跑。`pnpm check` 通过。全量测试和正式版窗口留到版本收尾。[TodoMVC React](https://todomvc.com/examples/react/dist/)

## 这一版做了什么（1.1.12）

站主列的 6 条（另 2 条只汇报：看网页的能力、右栏开页面）加上中途追加的轮数。PLOOP（Claude，AUTO）做到第 86 轮断线（Broken pipe）停了；Claude Code 审了日志和改动，修了 Peck 自己的几处问题（下面 7–11），PLOOP 再接着做完。

1. **`read_pdf` 的页面图按窗口给**（`pdf::image_budget`）：每 5 万 token 一张、2–20 张，100 万窗口的一次 20 页给满 20 张，不知道窗口的 6 张。
2. **对话里的图有总预算**（`loop.rs` 的 `prune_old_images`）：超过 15 MB 或 80 张，一次去到一半（最新的一张总留），旧的换成一句话。各家上限是 searcher 查的（Gemini 内联、Vertex 上的 Gemini 一次 20 MB 最紧）。Claude Code 审时指出每轮去一张会每轮断缓存，改成了一次去一批。
3. **输入框**：`+` 去掉下箭头（`MenuChip` 的 `caret`）；列表里 Tab / ⇧Tab 缩进一级（缩到上一项正文那一格，有序的改编号，`MarkdownEditor.tsx`）。
4. **PROJECTS 收起**只剩模块头，不再写项目名。
5. **收起左栏、右栏**：顶栏两头各一个按钮（新像素图标），存在界面设置的 `fold.left` / `fold.right`；点输入框下面的「谁」展开左栏，点改过的文件、⌘1–⌘3 展开右栏；收右栏时宽栏一起收回。栏是藏起来不卸掉。
6. **轮数上限** 5–1000（以前 400；任务、ONE SHOT、子代理都是）。
7. （Claude Code）问模型时断线、超时、限流、5xx 自动重发 5 次（2/5/15/30/60 秒），不算轮数。
8. （Claude Code）子代理到了上限多给一轮写报告；设置里改轮数，跑着的任务下一轮就跟着变。
9. （Claude Code）命令的 `CARGO_HOME` 用站主的 `~/.cargo`（以前每次全量重编）；沙箱里的命令带 `PECK_SANDBOX=seatbelt`，要套第二层沙箱的 14 个测试在沙箱里跳过（以前全量测试卡住跑不完）。
10. （Claude Code）PLOOP 主角色选过的不再被悄悄换成第一个能用的（这一轮被换成了 Vertex 上的 Opus、令牌过期起不来）；Vertex 上的名字后面带 `· VERTEX`。
11. （Claude Code）Anthropic 的思考带 `block_binding: drop_block`：换了版本工具说明变了，旧思考块不再让请求整个 400。
12. （Claude Code，最后一轮）**任务能不限轮数**（站主问的）：设置 → GENERAL「每个任务最多几轮」左边一个开关，关了就不按轮数停（`roundsUnlimited`），任务头只写「第 N 轮」；靠每月花费上限和原地打转收尾。ONE SHOT、子代理照旧有上限。
13. （Claude Code，最后一轮）**收了栏中栏不再拉满**：对话的每一块和输入框最宽 900px、居中（两栏都收时以前一行字有 1300px）。
14. （Claude Code，最后一轮）**命令读得到站主的 git 身份**：带 `GIT_CONFIG_GLOBAL` 指到 `~/.gitconfig`（只读）；PLOOP 这一轮提交时只好自己在命令里带。被挡在 `.git` 上时卡片原因写「要写 .git」，以前写成「要写到项目外」。
- 修了三个测试：worker 的轮数（上限改了）；CLI 收尾 Hook（Peck 给的临时目录带空格，测试拼的命令没加引号）；追加的那句（测试的 `drive` 改成先处理事件，以前全量并发时偶尔先跑到下一轮）。
- 数据库没动（schema 11），没加依赖。

## 验证

- **自动测试**：在 Peck 的沙箱里全量 `cargo test` 连跑三遍都过（373 个；要套沙箱的 14 个在沙箱里跳过，这一版没在终端里跑）；`pnpm check`、`pnpm build` 过。新加：页面图按窗口、对话里的图一次去一批（没超不动、再超才去下一批、按张数、最新的一张总留）、轮数到 1000。
- **`?mock`**（明暗）：PROJECTS 收起只剩头；`+` 没箭头；收左栏、收右栏、两个都收（中栏占满）、设置页里收；点「谁」展开左栏、⌘1 展开右栏；列表 Tab、⇧Tab、有序列表改编号。
- **这一轮续跑**就是带着 `block_binding` 发的，没再 400。真 API 也试过：不带时改了工具说明再带回旧思考块是 400，带上 `drop_block` 照常答。
- **最后一轮（Claude Code）**：终端里全量 `cargo test` 374 个都过（沙箱外，要套沙箱的那 14 个也真跑了）；`pnpm check`、`pnpm build` 过；`?mock` 看了收两栏时对话居中 900px、设置里不限的开关（关了这一行变灰）。
- **没验的**：
  - 正式版窗口里的这一版：收起的栏重开 App 还收着没有；不限轮数真跑到 1000 轮以上（只有设置读法的测试）。
  - 真模型：一次读 20 页扫描件（手上没有扫描件）；真断线时的重发（只有假模型的测试）。
  - 1.1.11 留下的：从访达、截图浮窗拖图进输入框，`+` 的系统选窗；PLOOP 会不会每条命令写 `description`、一次问完、看界面用 `screenshot_page`；挡住钥匙串后真起 Chrome 不弹框；Codex 的多问题卡片（默认模式不给提问工具）。
  - 更早的：Gemini 原生搜索（Key 额度用完）、Vertex（ADC 过期）、百炼新加坡（要国际站 Key）、Muse 会不会自己跑项目 Hook、钥匙串里的 MCP 变量、额度真到 85% 的提醒、Mathpix（没 Key）、通知横幅。

## 留给站主的

- **push** 你做：`origin/main` 到 1.1.11，1.1.12 的两笔没推。
- **设置 → GENERAL「每个任务最多几轮」**：现在存的是 400（旧版把你填的 1000 压成了 400）；要不限就把左边的开关关掉。
- **废纸篓**：`agy-…`（Antigravity 的程序，223 MB）、`agy-link-…` 和以前的 `Peck.app.backup-*`，确认不要了自己清空。
- **测试项目 tester** 批过信任（Grok 记进了 `~/.grok/trusted_folders.toml`，项目设置里 `trusted` 能关），里面有验收时加的记忆和几个样例文件。**git-plan-smoke** 里 Git Plan 提交过一笔 `243787f`（没 push）。
- **Vertex**：要用先在终端跑 `gcloud auth application-default login`（ADC 过期）。
- **仓库里没跟踪的 `.agents/skills`**（指向 `.claude/skills` 的链接，Peck 认结构建的）：要不要提交你定。
- **Claude Code** 的 `/compact 重点` 还没核对重点有没有生效：压缩那一行写了「重点：…」就是生效了。
- 装了 Git Plan 1.0.0 的项目要点 UPDATE 才有 HANDOFF 检查；Mathpix 的 Key 在设置 → PROVIDERS → TOOL KEYS。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 句中 `/` 按标题包含筛：空格后打路径（` /src`）时标题里正好有这几个字会弹，这时 ↵ 是插提示词不是发出去（打下一个 `/` 就收起） | Esc 关掉再 ↵ |
| 占位 Tab 只往后跳、不绕回，⇧Tab 不回上一个（还是退缩进）；光标走出去就整组退出，回头要自己选那几个字；撤销、重做不找回占位；PROMPT 的箭头放进来的不选占位 | 用一阵看要不要 |
| 关了轮数上限，一个任务（定时任务也是）能一直跑到每月花费上限才停 | 长任务前看一眼花费上限 |
| 在 Peck 里开发 Peck 时装不了正式版（换包会把自己换掉、签名要钥匙串）；每装一次新包第一次打开都要点几个钥匙串框 | 装包留给站主或 Claude Code，攒一批再装 |
| 对话里的图超了预算，去掉的旧图模型要再看得自己重新读（落盘的还在） | 记着 |
| 真用完额度的 429 也会等满 5 次重发（约 2 分钟）才停 | 用一阵看要不要分开「限流」和「额度用完」 |
| Vertex 上的 Claude 没带 `block_binding`：换了版本后续跑旧对话可能还是 400 | 开了 Vertex 再试 |
| 列表 Tab 只认空格缩进、只看上面的项：有序列表缩进以后，下面的同级项编号不跟着改 | 记着 |
| Gemini 内联数据的上限官方文档自相矛盾（20 MB / 100 MB），图的预算按 20 MB 定 | 记着 |
| Peck 没有能看外网的浏览器；模型看外网只有 `web_fetch` 的文字，看本机页面用 `screenshot_page` | 记着 |
| 高危放宽后，项目里被 .gitignore 忽略的文件（`.env`、`node_modules`）用 `rm` 删了不问，任务前快照里也没有，救不回来；`git clean -x` 照问 | 用一阵看要不要把删 `.env` 这类单独算高危 |
| `screenshot_page` 每次是新开的页面：只在内存里的状态（`?mock`）要在 script 里点出来；离屏时页面的计时器变慢，等动画、轮询的要多给时间 | 记着 |
| 拖进来的文档先存在 `harness-output/staging/`，没发出去的也留着（14 天没动的随输出目录一起清） | 记着 |
| 命令的说明是模型写的，可能和命令对不上；卡片上点「看命令」能看原文 | 记着 |
| CLI 的审批卡片标题照各家转换器给的：Claude Code 的 Bash 带说明（英文），别家还是「跑 命令」 | 记着 |
| 排着的话（Grok）只在内存里，只在正看着那个任务时接着发；切到别的任务、重开 App 就不发了 | 记着 |
| auditor 只读但不限在项目里：验收时它翻了 `~/Library/Application Support` 里 Peck 的数据、想跑 `tmutil`（被拒），一组就跑了十几轮 | 用一阵看要不要在它的提示里限死项目、少给轮数 |
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
| Vertex 上的 Claude / Gemini 只有离线测试，没真跑过 | 站主开了 GCP 项目再试 |
| Esc 关设置页时没存的 MCP 编辑会丢 | 记着 |
| 小窗打开的 0.2 秒动画里点下去会落到外面那层、把小窗关掉（动画用 clip-path，没露出来的部分点不中） | 记着 |
| 设置 → PLUGINS 的小窗按项目一个个读状态（项目多了会慢一点）；点开时不在左栏的项目不列 | 记着 |
| 窗口最窄（980）时设置页的名字会被省略得很短（「Vertex AI」只剩几个字） | 记着 |
| 手动压缩不起外部 MCP 服务器：工具列表少了它们，写摘要那一次缓存可能不中 | 记着 |
| 手动压缩是另一次 run：放回的文件登记成「读过」只在那一次里，下一句（新的 run）要整个覆盖还得先读（和平时一样）；`write_patch` 不受影响 | 记着 |
| Grok 续上的会话不再报窗口：小环用这个任务里它以前报过的那个；从来没报过就只写用了多少 | 记着 |
| Codex 压缩不报前后多大；Claude Code 的 `post_tokens` 只算对话，不拿来画 | 等下一条用量 |
| 读网页：WebKit 渲染超时、站点需要登录或交互时仍可能读不全。子资源规则按 URL 拦本机和内网，无法检查 DNS 解析出的 IP 或阻止 DNS 重绑定，WebRTC 等连接也不受这层规则约束；原文抓取走系统代理时不钉 IP | 记着 |
| 读网页的 Markdown 会带转义（`pairs\.` 这类），模型看得懂 | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、按段花费表、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、TERM 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.1.0 新加的：recap 里收尾提交的那一行。 1.1.2 新加的：设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。**1.2.0 新加的**：右栏 PROMPT 的页签图标（`.pk-i--prompt`，占位：花括号夹一个点）、提示词列表（`.prompt-*`：复制后的绿勾和底色、悬停才露的笔、朝左的箭头是 `send` 翻过来）和它的小窗（`.prompt-form`）；`/` 弹层里 `/ 提示词` 一组（标题加正文第一行，`.slash-row`）和输入框里没填的占位（`.cm-slot`：琥珀字加点线）。1.1.12 新加的：顶栏收起左右栏的按钮和图标（`.top-side`、`.pk-i--side-l`、`.pk-i--side-r` 和 `-off`）、不带箭头的 `+`。1.1.11 新加的：审批卡片的原因标签（`.agent-card-reason`）和「看命令」、ASK 的页签（`.ask-tab`）和 Other 那一行（`.agent-other`）、YOU 下面的小图和小片（`.you-attached`、`.you-shot`）、输入框拖放时的虚线（`.composer.is-drop`）、任务头的轮数和子代理小片（`.task-round--warn`、`.task-agents`）、子代理那一步的「N/上限 轮」、命令那一步点开的 `$ 命令` 行（`.out-cmd`）。1.1.10 新加的：中栏 15px 正文、卡片和标题换 Body（方案里还是像素字）。1.1.9 新加的：额度环（`.quota-ring`、`.quota-rows`）、ASK 里一个问题一段（`.ask-q`）、追加的那一行 YOU（`.you--mid`）、排着的话的小片、附图放大（`.lightbox`）。1.1.8 新加的：`[ TRUST ]` 卡片、FILES 的 ALL 和变暗的行（`.pk-row--dim`）、`+` 附的小片（`.composer-upload`）。1.1.7 新加的：`/compact` 那一行（`.you-compact`）、CLI 的小环（待更新变暗 `.is-stale`、弹层里「压缩由 X 自己管」）、读网页那一步的网址行（`.web-head`、`.web-link`）。1.1.6 新加的：设置页的统一排法（`.set-scope` 的行高、控件宽、两行的块 `.set-line`、组头 `.set-head`、框里的单位 `.field-unit`）、小窗（`.modal`、`.modal-scrim`）、插件卡片和像素图标（`.plug-card`、`.plug-icon`、`.plug-hero`、小窗里的清单 `.plugin-detail`）、记忆小窗（`.memo-*`）。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- 设置迁移（`settings.ts` 的 `loadSettings`）别删：正式版的界面设置还是 v1.1.1 以前的样子（有 `relay`、单干选的 API 模型、没有 `subagents`），第一次打开新版才换。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。
- Mathpix 的接口：认证是两个请求头 `app_id`、`app_key`，错误大多是 HTTP 200 带 `error_info.id`；真跑的入口 `gateway::mathpix` 的 `live_mathpix`。
- 联网搜索的协议各家常变（Gemini 的工具组合还是 Preview、Anthropic 搜索工具按日期出新版本）：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

v1.2.0 接着做 Phase 6；收尾（版本号、审查、升级测试、装正式版）由 Claude Code 做。这一版之后：自定义子代理（站主之前定的）。其他候选在 roadmap「之后要做的」。
