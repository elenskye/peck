# 当前交接

更新：2026-11-24。**v1.1.5 做完了，已装进 /Applications**。站主写明了版本号、不分 Phase。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.5）

1. **CLI 带项目记忆**：CLI 新会话（不是续上的）的第一句前面放一块 `<peck-memory>`（和 Peck Loop 系统提示里的同一份），排在开场时的 Hook 前面；续上的会话不重复带。
2. **去掉 Antigravity**（Google 条款风险）：引擎、「直接调用」这个概念（`direct_only`）、模型目录和生成脚本里的条目、夹具、QUOTA 的额度卡片和检测、状态栏脚本里那一支、ACP 转换器里只给它用的分支、界面上的色点和图表色、各处文档；CLAUDE.md 写明不再接，roadmap「不做」也写了。仓库外 `cli/agy`、`cli/bin/agy` 移到了废纸篓（站主定的），`cli/README.md` 和 `briefs/peck-compaction-brief.md` 里的那几行也删了。
3. **精简代码**（站主定的：删旧记录兼容、删死代码、合并重复，不拆文件）：
   - 旧记录兼容：接力、审查的 `stage.*` 事件和界面上那几段（SPEC 一行、审查结论框、验收清单、按段花费里的几段）、`team:` 的「谁来干」、旧的 `delegate_task` 显示、`<peck-verify>`、定时任务的 `weekdays`、项目默认里存的 `relay` / `api:`。删之前只读查了正式库：没有「工作日」的定时任务、没有项目存过默认「谁来干」。
   - 死代码：合作时代几段共用 MCP 服务器的 `mcp_slot`、永远是默认账号的 `default_account` / `without_quota_sync`、没人调的 `shortName`、没人用的样式（`.agents-row--team`、`.agent-stage*` 等）。用编译器、更严的类型检查、脚本扫了没人用的导出、样式、IPC、事件类型；只读子代理列了旧兼容的清单，我逐条核对后删。
   - 合并：Peck Loop（`harness/run.rs`）和 CLI（`gateway/runner.rs`）发任务的占号、前后快照、收尾、停止都改用 `run.rs` 的 `claim_run`、`Lifecycle`、`abandon`、`watch_cancel`。
   - 顺手修的：Peck Loop 占了任务号以后读库出错会提前返回，任务一直标着「在跑」、项目被占到重启（现在记成失败、放开项目）；项目设置里默认「谁来干」的列表把 PLOOP 滤掉了、PLOOP 选不了，却列着现在已经不能单独选的 API 模型（现在是 PLOOP 和各家 CLI）。
   - 留着的：界面设置的迁移（正式版的设置还没迁移过，见「留意」）、数据库迁移、旧表和删任务时清旧表的那几行（外键）。
   - 改动合计约 −1430 / +600 行。

- 新加依赖：没有。数据库没动（schema 11）。

## 验证

- **自动测试**：全量 `cargo test` 过（326 个；新加：CLI 新会话先带记忆、续上的不带；占号一次一个、`abandon` 放开项目；删掉的旧兼容带走了它们的测试）；`pnpm check`、`pnpm build`、`tsc --noUnusedLocals --noUnusedParameters` 过。
- **`?mock`**：改过的两条假任务（原来的接力换成 PLOOP、Grok）显示正常；项目设置里默认「谁来干」能选 PLOOP、存得上；左栏 CLI 只剩四家；QUOTA 的图五种颜色、没有 Antigravity。
- **真跑**：没有。
- **正式版**：装好了（`pnpm app:install`），版本号 1.1.5、签名校验过；窗口里没点过。
- **没验的**：
  - CLI 带记忆没在真 CLI 上跑过（只有假 CLI 的测试）。
  - 正式版窗口里没点过：这一版和 1.1.4 的都没有。

## 留给站主的

- **废纸篓**里有 `agy-…`（223 MB，Antigravity 的程序）和 `agy-link-…`（链接）：确认不要了自己清空。它的登录信息在你家目录里它自己的位置，我没碰。
- **第一次打开新版**：可能弹钥匙串授权框；界面设置会在这时从 v1.1.1 以前的样子迁移过来（单干选的那个当 PLOOP 主角色）。
- **1.1.4 留下的**：装了 Git Plan 1.0.0 的项目在项目设置 → PLUGINS 点 UPDATE 才有 HANDOFF 检查；Mathpix 的 Key 要在设置 → PROVIDERS → PDF 填。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.5 的全部提交。
- 以前留下、还没验的：1.1.4 的真模型读 PDF / Notebook、MCP 按需加载、CLI 的三个 Hook 在真 CLI 上；1.1.3 的 member 真跑、任务板和 Claude Code 清单在正式版里；1.1.2 的正式版窗口（AGENTS、真派子代理、脚本 Hook 挂真的 `.claude/settings.json`）、searcher / auditor 真跑、百炼新加坡地域；1.1.1 存 Tavily 的 Key、真搜一次；1.1.0 的插件界面和收尾提交；1.0.3 的轮数设置；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。Gemini 原生搜索、Vertex 上的 Claude 和 Gemini 还没真跑过。

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

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字、recap 里收尾提交的那一行。 1.1.2 新加的：LOG 上面的成员名单（`.log-roster`）、设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。1.1.4 新加的：设置 → MEMORY 的项目头（`.memory-group`、`.memory-path`）、改字和自己记一条那一行（`.memory-edit`）。

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
