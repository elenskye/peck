# 当前交接

更新：2026-11-26。**v1.2.0 做完、已提交、已装进 /Applications**（Claude Code 装的；每装一次新包，第一次打开会连弹几个钥匙串框）。没有在做的版本。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/design.md](./docs/design.md)，实现在 [docs/architecture.md](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。

## 这一版（1.2.0）

站主定的做法：Claude Code 接管电脑，用正式版 Peck 开发，6 个 Phase 各交给一个引擎；真跑里暴露的 Peck 问题由 Claude Code 当场修，各一笔提交。

| Phase | 谁来做 | 做了什么 |
| --- | --- | --- |
| 1 | Claude Code | 去掉跑完的「下一步」按钮（系统提示那条、recap 的按钮、`?mock`、文档） |
| 2 | Grok | 右栏去掉 PREVIEW 和本机预览代理；留着的 dev server 小片在 TERM 头上 |
| 3 | PLOOP | 提示词库：全局一份存在库里（schema 12）；右栏 PROMPT 页签：搜索、复制、放进输入框、小窗增改、删除点两下 |
| 4 | PLOOP | 输入框打 `/` 插提示词（句首命令在前、提示词在后，句中只列提示词）；`{占位}` 选中、Tab 跳，`\{` 是字面花括号 |
| 5 | Codex | `web_fetch` 正文太少或 `render: true` 时用离屏 WebKit 跑完 JavaScript 再取正文 |
| 6 | Muse → Codex | 界面对齐：`+`、↵ 居中，各栏控件高度和基线，设置、小窗、QUOTA 的对齐（Muse 量到一半订阅额度用完，站主改给 Codex） |

真跑里修的 Peck 问题：

- **崩溃**：发任务那一下 App 被 macOS 杀掉。菜单栏图标在后台线程上被并发取放，计数乱掉、在后台线程上被移除。托盘现在只在主线程上碰。
- **无人值守会卡住的卡片**：CLI 读临时目录里自己截的图也要站主点；`T=$(mktemp -d) … rm -rf "$T"` 被当成删项目外。前者在项目、临时目录里替 CLI 批；后者只认最死板的写法（审查找到过绕过，见 architecture「权限和高危」），顺带补上了 `do`、`then` 后面的 `rm` 认不出的老漏洞。
- **卡片藏在视线外**：长提示词发出去以后中栏停在 YOU 顶上，审批卡片在下面看不见；现在卡片出来就滚到它。
- **CLI 的几处显示**：Grok 搜索没搜到被标成 FAIL；Codex 搜网页那一步没有搜索词；额度提醒用的是过了重置时间的旧快照；CLI 模型清单停在 11-13 选不到新模型（现在存了 3 天自动重读）；价格表补上 gpt-6.2 Sol / Astra。
- **WebKit 读网页的拦截**（收尾审查找到）：页面脚本能经「解析到本机的域名」请求本机服务。现在能读回响应的请求只准发给这个网站自己；剩下的见「已知问题」。

数据库到 schema 12（升级前自动备份 `before-v12`）；没加依赖。

## 验证

- 全量 `cargo test` 379 个过（终端里，要套沙箱的、WebKit 的都真跑了）；`pnpm check`、`pnpm build` 过。
- 两个只读子代理分头审了这一版的 Rust 和前端，真缺陷都修了：Rust 那份是 mktemp 规则能绕过、WebKit 拦截、托盘动画；前端那份是切项目时 TERM 的 dev server 小片串到别的项目、后台重读的模型清单冲掉新引擎的、`/` 插提示词打得快时留尾巴、最后一个占位按 Tab 把列表项缩进、清空的占位旁边删字后 Tab 跳不了、PROMPT 小窗拼音上屏的 Enter，还有 TERM 头比别的页签高 6px。
- 升级测试：正式数据副本用新版打开，升到 schema 12、留了备份、项目 / 任务 / 事件行数不变，崩溃时卡在「在跑」的任务收成失败。
- `?mock` 明暗：PROMPT 页签、`/` 弹层两组、句中插入和占位（真按键：清空一个占位再删旁边的字，Tab 照样跳；列表里最后一个占位 Tab 不缩进）、`+` 居中（32px 框里 12px 图形正中）、四个右栏页签头一样高、等人点的卡片自己滚进视线。
- 实测 WebKit 拦截：本地页面的 fetch、WebSocket、sendBeacon 经 `localtest.me` 打本机服务，改前三种都到了、改后都没到。Codex 在公开的 TodoMVC React 空壳页上读到了 JavaScript 生成的正文。
- **没验的**：
  - 正式版窗口里：真中文输入法下的 `/` 弹层和占位（只有模拟的组字事件）、真鼠标点弹层、提示词复制到系统剪贴板、悬停才露的笔。
  - 真模型用 `web_fetch` 的 `render: true`（只有测试入口和 Codex 的实测）。
  - 以前留下的：从访达拖图、`+` 的系统选窗；Gemini 原生搜索（Key 额度用完）、Vertex（ADC 过期）、百炼新加坡、Mathpix（没 Key）、通知横幅。

## 留给站主的

- **push** 你做：`origin/main` 到 1.1.11，1.1.12 两笔和 1.2.0 这一版没推。
- **Muse** 的订阅额度用完了（北京时间 11-26 04:17 重置）；Peck 读不到 Muse 的余量，事前不提醒。
- **Claude Code** 的每周额度剩得不多（11-25 晚看是 16%）。
- 任务 194 的记忆卡片（「Peck 给每条命令的 `$TMPDIR` 跑完就删」）还等你 SAVE / SKIP。任务 193（崩溃）、196（Muse 额度用完）是失败记录，留着当记录。
- **废纸篓**：两次装包换下来的 `Peck.app.backup-*`，确认不要了自己清空。
- 仓库里没跟踪的 `.agents/skills`（Peck 认结构建的链接）：要不要提交你定。
- **Vertex**：要用先在终端跑 `gcloud auth application-default login`。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| WebKit 读网页：图片、脚本、样式还能发往解析到本机、内网的域名（只能盲发、读不回）；同一个主机的 DNS 重绑仍能让页面读到本机服务；WebRTC 不受内容规则管 | 根治要给 WebKit 配 Peck 自己的代理、每条连接查解析出来的 IP（roadmap 候选） |
| CLI 的后台进程（`pnpm dev &`、无头浏览器）任务结束后不一定停：Claude Code 起的 vite 用 `pkill -f 'vite --port 1430'` 没杀掉（进程名是 `vite.js`） | 提示词里叫它自己停；Peck 不管 CLI 进程以外的 |
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

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步。按位置：

- **顶栏和左栏**：收起左右栏的按钮和图标（`.top-side`、`.pk-i--side-*`）、`[ NOW ]`、PROJECTS 头上的钟和放大图标、全部项目页、归档图标和归档条、菜单栏「在跑 N 个」。
- **中栏**：任务头的轮数和子代理小片（`.task-round--warn`、`.task-agents`）、「Thinking…」行、recap（含收尾提交那一行、按段花费表）、任务板（`.board-*`）、独立工作区条、`[ MISSED ]` / `[ TRUST ]` / `[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、审批卡片的原因标签和「看命令」（`.agent-card-reason`、`.out-cmd`）、ASK 的页签和 Other（`.ask-tab`、`.ask-q`、`.agent-other`）、YOU 下面的小图小片和追加的那一行（`.you-*`）、附图放大（`.lightbox`）、`/compact` 那一行、读网页的网址行（`.web-*`）、中栏 15px 正文和 Body 字体。
- **输入框**：`/` 和 `@` 弹层（含 `/ 提示词` 一组 `.slash-row`）、没填的占位（`.cm-slot`）、↵、不带箭头的 `+`、`+` 附的小片（`.composer-upload`）、拖放虚线（`.composer.is-drop`）、上下文环和额度环（`.quota-ring`、`.is-stale`）、排着的话的小片。
- **右栏**：PROMPT 页签图标（`.pk-i--prompt`，占位）、提示词列表和小窗（`.prompt-*`、`.prompt-form`）、TERM 头上的 dev server 小片、FILES 的 ALL 和变暗的行（`.pk-row--dim`）。
- **设置和 QUOTA**：统一排法（`.set-scope`、`.set-line`、`.set-head`、`.field-unit`）、小窗（`.modal`）、插件卡片和像素图标（`.plug-*`）、记忆小窗（`.memo-*`）、MCP、AGENTS 和 SUBAGENTS 几块、进度条、额度页的环和读数（`.quota-rows`）、ⓘ 图标（占位）和弹层、代码配色 `--code-*`。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- 设置迁移（`settings.ts` 的 `loadSettings`）别删：正式版的界面设置还是 v1.1.1 以前的样子，第一次打开新版才换。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`moon-lab`、`gateway-smoke`、`git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`。
- 联网搜索的协议各家常变：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

自定义子代理（站主之前定的）。其他候选在 roadmap「之后要做的」。
