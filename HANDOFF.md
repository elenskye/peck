# 当前交接

更新：2026-11-24。**v1.1.6 做完了，已装进 /Applications**。站主写明了版本号、不分 Phase，只改界面（前端），后端只改了一句提示里的分组名（PDF → TOOL KEYS），数据库没动。规则在 [CLAUDE.md](./CLAUDE.md)，界面在 [docs/design.md](./docs/design.md)「设置」，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.6）

1. **设置页统一排法**（站主给的三条准则，写进 design.md「设置 → 统一排法」）：每行一样高；右边的主控件一样宽、右边对齐，控件一样高，单位写进框里；小按钮一样宽，每行都有的放最右；放不下一行的做成一块两行（头一行 + 下面固定的格子，几块的格子对齐）；名字、状态不换行、放不下省略；一组的按钮挪到组头右边。窗口窄时 AGENTS 的格子换成两行两列，归档、MCP、价格表每行变成两行——整组一起变。设置、项目设置、小窗共用 `.set-scope`。「这个月花了」那一行不再特殊：「花了 / 上限」写在名字下面，进度和输入框一样大。
2. **PROVIDERS 只分两组**：API KEYS（各家 + 最后一块 Vertex AI）、TOOL KEYS（Tavily、Mathpix 两样）。Vertex 和别的 Key 一样一块：头一行 Location 和一个状态方框（NO GCLOUD / NO PROJECT ID / SAVED，原来是两个方框）、下面 Project ID + SAVE + REMOVE（新加的：清掉 Project ID）。Aliyun 的地域挪到头一行、点了就存（原来要另点 SAVE）。
3. **MEMORY**：一个项目一行 + EDIT，开那个项目的记忆小窗（字照原样换行，EDIT 就地变多行的框，⌘↵ 存、Esc 取消；DELETE 改成点两下；最下面多行的框记一条）。项目设置的 MEMORY 也改成一行 + 同一个小窗。
4. **PLUGINS**：学 Claude Desktop，两栏卡片（图标、名字、两行简介），点开小窗看全部；小窗里 PROJECTS 列左栏的全部项目，**能一个个项目装、更新、移除（库里的）或开关（自带的）**，库里的还有「每个项目都装」。图标按名字算出 5×5 像素（插件青色、单独的 Skill 淡紫）。项目设置的 PLUGINS 也是卡片 + 同一个小窗（只列这个项目）。
- 新零件（`ui.tsx`）：`Modal`（小窗）、`SetHead`（组头）、`Field` 的 `unit`。删了 `MemoryList`、`PluginRow`、旧的 `.set-keys`、`.set-num`、`.memory-*`、`.plugin-name` 这些样式。
- 新加依赖：没有。数据库没动（schema 11）。

## 验证

- **自动测试**：`pnpm check`、`tsc --noUnusedLocals --noUnusedParameters`、`pnpm build` 过；全量 `cargo test` 过（326 个）。
- **`?mock`**（内置浏览器，1360×860 和 1024×768 两种窗口，明暗各一次）：十一组量过行高一样、主控件左右边一样、没有横向滚动；AGENTS 窄时两行两列；Aliyun 地域和 Vertex Location 对齐；记忆小窗改一条 ⌘↵ 存上、Esc 只关小窗不关设置页；Git Plan 小窗里给 edge-gateway INSTALL 装上、Snapshots 在 notebook 里关掉后项目设置的卡片变暗。
- **真跑**：没有。
- **正式版**：装好了（`pnpm app:install`）；窗口里没点过。
- **没验的**：正式版窗口（WKWebView）里的样子——`cqw` 单位、小窗里的放大倍数、`-webkit-line-clamp` 都只在内置浏览器里看过。

## 留给站主的

- **废纸篓**里有 `agy-…`（223 MB，Antigravity 的程序）和 `agy-link-…`（链接）：确认不要了自己清空。它的登录信息在你家目录里它自己的位置，我没碰。
- **第一次打开新版**：可能弹钥匙串授权框；界面设置会在这时从 v1.1.1 以前的样子迁移过来（单干选的那个当 PLOOP 主角色）。
- **1.1.4 留下的**：装了 Git Plan 1.0.0 的项目要点 UPDATE 才有 HANDOFF 检查（设置 → PLUGINS 点开 Git Plan 能一次看全部项目）；Mathpix 的 Key 在设置 → PROVIDERS → TOOL KEYS 填。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.6 的全部提交。
- 以前留下、还没验的：1.1.5 的 CLI 带记忆在真 CLI 上、正式版窗口；1.1.4 的真模型读 PDF / Notebook、MCP 按需加载、CLI 的三个 Hook 在真 CLI 上；1.1.3 的 member 真跑、任务板和 Claude Code 清单在正式版里；1.1.2 的正式版窗口（AGENTS、真派子代理、脚本 Hook 挂真的 `.claude/settings.json`）、searcher / auditor 真跑、百炼新加坡地域；1.1.1 存 Tavily 的 Key、真搜一次；1.1.0 的插件界面和收尾提交；1.0.3 的轮数设置；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。Gemini 原生搜索、Vertex 上的 Claude 和 Gemini 还没真跑过。

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

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.1.0 新加的：recap 里收尾提交的那一行。 1.1.2 新加的：LOG 上面的成员名单（`.log-roster`）、设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。1.1.6 新加的：设置页的统一排法（`.set-scope` 的行高、控件宽、两行的块 `.set-line`、组头 `.set-head`、框里的单位 `.field-unit`）、小窗（`.modal`、`.modal-scrim`）、插件卡片和像素图标（`.plug-card`、`.plug-icon`、`.plug-hero`、小窗里的清单 `.plugin-detail`）、记忆小窗（`.memo-*`）。

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
