# 当前交接

更新：2026-11-23。**v1.1.2 做完了，已装进 /Applications**。这是逐步优化 Peck Loop 的第二轮，专注 API 的工作流。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md「PLOOP」「脚本 Hook」](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.2）

1. **PLOOP = 主角色 + 四类子代理**：AGENTS 的 FLOW 只剩 CLI / PLOOP（API 单干去掉）；PLOOP 只选一个 API 模型当主角色。它用 `spawn_agent` 在后台派 explorer（读、搜、只读命令）、searcher（联网检索和调研）——这两类能同时派几个；worker（以前的委派）、auditor（以前的审查）一个一个排队。同一次派的一批全做完才把报告一起交回、叫醒主角色一次；没有查进度的工具。写规范、自动审查那几段去掉。
2. **设置 → AGENTS**：主角色一行 + SUBAGENTS 四块（开关、模型（空着跟主角色）、思考强度、最多几轮、explorer / searcher 同时几个）。思考强度按模型记，这里和 AGENTS 面板调的是同一档。GENERAL 的「子任务最多几轮」去掉（挪到每类里）。
3. **缓存不失效**：系统提示和工具说明里不写子代理用哪个模型；成员名单（`agents.roster`）发一次、显示在右栏 LOG 最上面。
4. **一批只读工具同时跑**：一次回答里全是只读的（读、看目录、搜、找文件、读 Skill、查旧任务、`web_search`、派子代理）就同时跑，结果按调用顺序放回；混进改文件、命令、MCP 就按顺序。
5. **原地打转**：同一个工具、同样的参数、同样的结果，第 3 次提醒（琥珀提示），第 5 次要它收尾，再调工具就停下。
6. **脚本 Hook**：照 Claude Code 格式读项目 `.claude/settings.json` / `settings.local.json` 和装进项目的插件的 `hooks/hooks.json`，在发话时、调工具前后、压缩前、收尾时跑命令；exit 2 拦下、JSON 决定，和 Claude Code 一样。每条命令第一次跑前弹卡片（ALWAYS 按指纹记）。只在 Peck Loop 里跑，ONE SHOT 不跑。
7. **百炼换到 Responses**（Qwen 方言），接上它自己的 `web_search`（不再走 Tavily）。
8. 文档：roadmap 写了「自定义子代理（下一步）」「PLOOP 的成员和任务板」「自动审查先不做」。

- 新加依赖：Rust `regex = "^1.13.1"`（Hook 的 matcher；已经在锁文件里，是间接依赖）。数据库没动（schema 11）。`app_settings` 的 `ui` 那一行：`ploop` 只剩 `main`，多了 `subagents`，`subRounds`、`review`、`delegate` 不再存（读的时候换过去并马上存回：选着单干的那个当主角色，不然是接力 / 旧 PLOOP 实施的那个；委派 → worker、审查 → auditor 的模型）。事件：多了 `agents.roster`，`subagent.started` 多 `kind`，`hook.ran` 多 `event`。旧的定时任务里记的写规范、审查、委派读的时候丢掉，到点按 PLOOP 跑。

## 验证

- **自动测试**：全量 `cargo test` 过（308 个；新加：子代理一批叫醒一次、worker + auditor 拿到 Diff、只读一批同时跑且顺序不乱、原地打转收尾、项目里的脚本 Hook 拦下和补充、Hook 的解析和退出码、百炼 Responses 的方言）；`pnpm check`、`pnpm build` 过。
- **`?mock`**：AGENTS 只剩 CLI / PLOOP、主角色下拉；NEW TASK 两行（主角色、子代理）；「派两个子代理」两个 explorer 同时跑、挂在派的那一步下面、一批做完主线才接着；LOG 的成员名单和时间线；设置 → AGENTS 四块（明暗都看了）。
- **真跑**（账在 roadmap）：百炼 qwen3.8-flash 在 Responses 上两次请求、原生搜索两次都过；PLOOP 主角色 gpt-6-luna 一次派两个 DeepSeek Flash 的 explorer、等报告、收尾，过（约 $0.003）。
- **正式版**：装好了，版本号 1.1.2、签名校验过；窗口里没点过。
- **没验的**：
  - 正式版窗口里一下都没点过：AGENTS、设置 → AGENTS、整个 App 里真派子代理。
  - searcher、worker、auditor 没真跑过（只有假模型的测试）；主角色是 Anthropic、Gemini 时派子代理没真跑过。
  - 脚本 Hook 只在测试里用 sh 脚本跑过，App 里没挂过真的 `.claude/settings.json`；插件 `hooks.json` 里的 `command` 也是。
  - 百炼新加坡地域在 Responses 上没真跑；Gemini 的原生搜索、Vertex 上的 Claude 和 Gemini 还是没真跑过（Gemini 那把 Key 额度用完）。

## 留给站主的

- **第一次打开新版**：可能弹钥匙串授权框。
- **AGENTS 变了**：没有 API 单干了。你正式版里现在选的是单干 Sonnet 5.5，打开新版就是 PLOOP · 主角色 Sonnet 5.5；委派 DeepSeek Flash 换成 worker 的模型，别的几类跟主角色；接力里的写规范 Sonnet、实施 DeepSeek Flash 不再用。到设置 → AGENTS 看一眼，想让 explorer / searcher 用便宜的模型就在那里选。
- **走一遍**：选 PLOOP 发「派两个子代理分别看看 X 和 Y，然后告诉我…」，看过程里的「Spawn explorer」和 LOG 的成员名单；改完东西让它「派 auditor 审一下」。
- **Hook**：想试的话在测试项目的 `.claude/settings.json` 写一条（格式和 Claude Code 一样），第一次会弹卡片。
- **Tavily**：还没存进钥匙串（v1.1.1 真跑用的是剪贴板那把）。现在百炼不用它了，只有 Vertex 上的 Gemini、Gemini 2.x 用，DeepSeek 搜不了时也是退路。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.2 的全部提交。
- 以前留下、还没验的：1.1.1 在正式版里存 Tavily 的 Key、真搜一次；1.1.0 的插件界面和收尾提交在正式版里走一遍；1.0.3 的轮数设置、旧任务的样子；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 搜索按次的费用按收费价记，Gemini 每月头 5000 次、Tavily 免费档每月 1000 次不扣（会多算） | 宁可多算；在意再加个月度计数 |
| 子代理报告回来以前站主停下任务，派出去的一起叫停，报告不进对话 | 记着 |
| 原地打转只认一模一样的调用和结果；参数稍微不同、结果一样的绕圈认不出来 | 用一阵再看 |
| 脚本 Hook 不进沙箱，跑的是站主 shell 的环境（第一次弹卡片就是为这个） | 记着 |
| OpenAI 的来源没有标题，过程里标题写网址 | 记着 |
| Gemini 的来源挂在第一条搜索上（它只在最后一起给） | 记着 |
| 只有界面点开项目时才认结构、装默认插件；定时任务在一个从没点开过的老项目里跑时不会先做 | 记在 roadmap |
| 单独的 Skill 装没装是按「项目里有没有同名的 Skill」认的：项目自己写了一个和库里同名的，会显示成装着的，点 UPDATE / REMOVE 会动到它（库里现在没有单独的 Skill） | 往库里放单独的 Skill 之前先加归属记号 |
| 收尾提交失败（仓库的 pre-commit 钩子不过、没配 `user.name`）时，这一轮的文件已经 `git add` 了，留在暂存区 | 记着 |
| CLI 任务每轮做完多用一轮订阅额度问提交说明；Antigravity、续不了会话的不问，写「模型没给出提交说明」 | 用一阵看烦不烦 |
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

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、PLOOP 每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。 1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字、recap 里收尾提交的那一行。 1.1.2 新加的：LOG 上面的成员名单（`.log-roster`）、设置 → AGENTS 的 SUBAGENTS 四块（开关、「跟主角色」、「同时 N 个」）。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。
- 联网搜索的协议各家常变（Gemini 的工具组合还是 Preview、Anthropic 搜索工具按日期出新版本）：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

自定义子代理（站主定了下一步做）；之后是 PLOOP 的成员和任务板。其他候选在 roadmap「之后要做的」。
