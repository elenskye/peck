# 当前交接

更新：2026-11-26。**v1.2.1 做完、已提交、已装进 /Applications**（第一次打开会连弹几个钥匙串框）。没有在做的版本。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/design.md](./docs/design.md)，实现在 [docs/architecture.md](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。

## 这一版（1.2.1）

- **CLI 的后台进程一起停**：CLI 和它生出来的进程算一组，任务结束时把还活着的一起停掉（怎么认见 architecture「CLI 四家」的「进程」）。
- **AUTO 能提交**：只做 `git add` / `git commit` 的命令换一个能写 `.git` 的沙箱跑，不弹卡片；EDIT 和别的写 `.git` 照旧弹卡片。
- **WebKit 走 Peck 的代理**：`web_fetch` 渲染时每条连接由 Peck 解析、查过 IP 再连，WebRTC 关掉；能读回响应的请求不再只限这个网站自己。
- **Git Plan 1.2.0**：这一轮改到的每个 git 仓库（主文件夹和能改的其他文件夹）各提交一笔，仓库根目录 `CLAUDE.md` / `AGENTS.md` 写了 Git 规则的照它的；其他文件夹的说明文件路径开场带给模型。插件只装主文件夹，Peck 不写其他文件夹里的文件。
- 文档精简（去掉来历和重复，各放一处）；代码精简：Rust 合并重复的小函数、删只有测试在用的代码（净删约 130 行），前端删没人读的状态和字段、死样式、合并重复的小函数（净删约 70 行），行为不变。

数据库没变（schema 12）；没加依赖。

## 验证

- 全量 `cargo test` 386 个过（17 个要联网、真账号的 ignored）；`pnpm check`、`pnpm build`、`tsc --noUnusedLocals --noUnusedParameters` 过。
- 新加的测试：一组进程（同组的、换了会话的、被丢下的、两层 fork 带记号的 node 都停了）；AUTO 提交沙箱（真沙箱里 `Project` 写不了 `.git`、`Commit` 能提交、`config` / `hooks` / `info` 写不了）和 `plain_commit` 的认法（含几种绕过写法）；SOCKS 代理的拒绝和放行；WebKit 没有 WebRTC；Git Plan 多仓库（单元测试，加一整条 Peck Loop 任务：主文件夹照 Git Plan、其他文件夹照它的 `## Git`）；`<peck-folders>` 带说明文件。
- 真跑：Claude Code（Haiku）2 个小任务，让它在后台起 `sleep`：第一次任务结束后还留着（系统程序、新会话、父进程先退了），补上「被丢下的」那条后第二次没了。
- 联网：TodoMVC 经代理读到 JavaScript 生成的正文；`localtest.me`（解析到本机）的图片、fetch、WebSocket 一次都没到本机服务。
- `?mock`：精简后的前端开过任务页、任务板和子代理计数、设置 → AGENTS，没有脚本错误。
- **没验的**：
  - 正式版窗口里没点：Git Plan 多仓库的两行 recap、AUTO 下提交不弹卡片、WebKit 渲染。
  - 真模型：PLOOP 在 AUTO 下自己 `git commit`、Git Plan 多仓库的问法（真模型照不照编号回）、CLI 的多仓库提交。
  - 进程清理只在 Claude Code 上真跑过，Codex、Muse、Grok 没有。
  - 以前留下的：真中文输入法下的 `/` 弹层和占位、从访达拖图、`+` 的系统选窗；Gemini 原生搜索、Vertex、百炼新加坡、Mathpix、通知横幅。

## 留给站主的

- **push** 你做：`origin/main` 到 1.1.11，1.1.12、1.2.0、1.2.1 都没推。
- **Git Plan UPDATE**：装着 1.1.0 的项目在项目设置 → PLUGINS 点 UPDATE 才换成新的 Skill 和问法；不点也能用，多仓库提交照样按新规矩走。
- 任务 194 的记忆卡片（「Peck 给每条命令的 `$TMPDIR` 跑完就删」）还等你 SAVE / SKIP（没再查过）。
- **废纸篓**：装包换下来的旧 `Peck.app`，确认不要了自己清空。
- 仓库里没跟踪的 `.agents/skills`（Peck 认结构建的链接）：要不要提交你定。
- **Vertex**：要用先在终端跑 `gcloud auth application-default login`。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| CLI 跑在它自己的沙箱里：Codex 的沙箱里 WebKit 起不来，截图、渲染要出沙箱，每次弹卡片 | 让它一个脚本截完 |
| Muse、Grok 读不到订阅余量，用完了只能等 429 | 记着 |
| 句中 `/` 按标题包含筛：空格后打路径（` /src`）时标题里正好有这几个字会弹，这时 ↵ 是插提示词 | Esc 关掉再 ↵ |
| 占位只往后跳、不绕回；⇧Tab 是退缩进；撤销不找回占位；PROMPT 的箭头放进来的不选占位 | 用一阵看要不要 |
| App 一直开着、额度快照不变时，过了重置时间的提醒要等快照更新才消失 | 记着 |
| 关了轮数上限，一个任务（定时任务也是）能一直跑到每月花费上限才停 | 长任务前看一眼花费上限 |
| 在 Peck 里开发 Peck 时装不了正式版（换包会把自己换掉、签名要钥匙串） | 装包留给站主或 Claude Code，攒一批再装 |
| 真用完额度的 429 也会等满 5 次重发（约 2 分钟）才停 | 用一阵看要不要分开「限流」和「额度用完」 |
| 高危放宽后，项目里被 .gitignore 忽略的文件（`.env`、`node_modules`）用 `rm` 删了不问，任务前快照里也没有；`git clean -x` 照问 | 用一阵看要不要把删 `.env` 这类单独算高危 |
| 读文件的规则只按字面路径：临时目录里一个指向家目录的链接也算临时目录（EDIT / AUTO 下 `cat ~/...` 本来就不问，不算放宽） | 记着 |
| 命令的说明是模型写的，可能和命令对不上；CLI 里只有 Claude Code 带说明，别家卡片标题是命令第一行 | 卡片上点「看命令」看原文 |
| TRUST 卡片、Hook 卡片、没批过的命令：定时任务碰到会一直等 | 先手动发一次、点掉 |
| auditor 只读但不限在项目里，一组能跑十几轮 | 用一阵看要不要限死项目、少给轮数 |
| CLI 的花费是按价格表估的 API 价，不是订阅真花的钱；价格表里没有的模型记成未知（REFRESH 只刷新已有的，不加新模型） | 新模型出来补进 `defaults/prices.json` |
| 旧表 `api_calls`、`api_tasks`、`usage_calls`、`messages` 还在库里（几十行），删任务时那几行清理代码得留着（外键） | 要清就加一条迁移把四张表删掉 |
| 划文件只管文件工具和命令的沙箱：MCP 工具、主角色自己的命令管不到；`sed -i` 原地改划给它的单个文件会被沙箱拦 | 划文件夹，或用文件工具改 |
| 原地打转只认一模一样的调用和结果 | 用一阵再看 |
| 插件带的 MCP 服务器（`.mcp.json`）只列出来，Peck Loop 不起 | 有插件要用时再接 |
| Vertex 上的 Claude / Gemini 只有离线测试；Vertex 上的 Claude 没带 `block_binding` | 开了 GCP 项目再试 |
| 小窗打开的 0.2 秒动画里点下去会落到外面那层、把小窗关掉 | 记着 |
| 任务结束时认不出的 CLI 子进程：自己 `setsid` 当组长、又是系统自带程序（读不到记号）、父进程先退了的；被丢下的但工作目录不在 CLI 工作目录里的 | 真碰到再看 |
| 被丢下的进程按工作目录认：站主同时在别的终端里、同一个项目文件夹下用 `sh -c '… &'` 起的后台进程，任务结束时也会被停 | 记着 |
| 仓库的 Git 规则只认标题带 Git、commit、提交的那几节；写在别的标题下的认不到，就照 Git Plan | 规则放进这样的标题下 |
| AUTO 的提交沙箱放开了 `.git`（`config`、`hooks`、`info` 除外）：提交时仓库自己的 pre-commit 钩子也能写 `.git` | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步。按位置：

- **顶栏和左栏**：收起左右栏的按钮和图标（`.top-side`、`.pk-i--side-*`）、`[ NOW ]`、PROJECTS 头上的钟和放大图标、全部项目页、归档图标和归档条、菜单栏「在跑 N 个」。
- **中栏**：任务头的轮数和子代理小片（`.task-round--warn`、`.task-agents`）、「Thinking…」行、recap（含收尾提交那一行、按段花费表）、任务板（`.board-*`）、独立工作区条、`[ MISSED ]` / `[ TRUST ]` / `[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、审批卡片的原因标签和「看命令」（`.agent-card-reason`、`.out-cmd`）、ASK 的页签和 Other（`.ask-tab`、`.ask-q`、`.agent-other`）、YOU 下面的小图小片和追加的那一行（`.you-*`）、附图放大（`.lightbox`）、`/compact` 那一行、读网页的网址行（`.web-*`）、中栏 15px 正文和 Body 字体。
- **输入框**：`/` 和 `@` 弹层（含 `/ 提示词` 一组 `.slash-row`）、没填的占位（`.cm-slot`）、↵、不带箭头的 `+`、`+` 附的小片（`.composer-upload`）、拖放虚线（`.composer.is-drop`）、上下文环和额度环（`.quota-ring`、`.is-stale`）、排着的话的小片。
- **右栏**：PROMPT 页签图标（`.pk-i--prompt`，占位）、提示词列表和小窗（`.prompt-*`、`.prompt-form`）、TERM 头上的 dev server 小片、FILES 的 ALL 和变暗的行（`.pk-row--dim`）。
- **设置和 QUOTA**：统一排法（`.set-scope`、`.set-line`、`.set-head`、`.field-unit`）、小窗（`.modal`）、插件卡片和像素图标（`.plug-*`）、记忆小窗（`.memo-*`）、MCP、AGENTS 和 SUBAGENTS 几块、进度条、额度页的环和读数（`.quota-rows`）、ⓘ 图标（占位）和弹层、代码配色 `--code-*`。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`moon-lab`、`gateway-smoke`、`git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 联网搜索的协议各家常变：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

自定义子代理（站主之前定的）。其他候选在 roadmap「之后要做的」。
