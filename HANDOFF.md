# 当前交接

更新：2026-11-23。**v1.1.3 做完了，已装进 /Applications**。这是逐步优化 Peck Loop 的第三轮。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md「PLOOP」](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.3）

1. **Claude Code 的任务清单**：旧的 `TodoWrite`（整份）和 2.1 起默认的 `TaskCreate` / `TaskUpdate`（一次一项，Peck 拼成整份）都转成任务清单，和 Codex、Muse、Grok、Antigravity 一样显示在过程最上面；子任务里的不转。
2. **子代理 member**：复杂、多步的活。工具和权限和主角色一样（改、命令、截图、读 Skill、联网搜索、MCP），只能再派 explorer、searcher；不问站主、不写清单和记忆。设置 → AGENTS 多一块，默认 40 轮、同时 2 个（花费上限照样管）。worker 也能同时几个了（默认 3）。主角色的提示里写明：简单的给 worker、复杂的给 member、互不干扰的一起派。
3. **划文件**：派 worker、member 必须写 `paths`（能改的文件、文件夹）。不重叠的同时跑，和前面的重叠就按派的顺序排队；Peck 强制它只改自己那块——文件工具改别处直接拒，命令的沙箱只放开它那几块和构建、缓存目录（`target`、`node_modules`、git 忽略着的文件夹等）。主角色用文件工具改划出去的地方也拒，告诉它等报告。
4. **任务板**：派了 worker、member 时，过程最上面的任务清单换成任务板：主角色自己那几步 + 每个 worker / member 一行（排队、在跑、等你批、做完，Peck 照子代理的事件填；做什么、谁、用时、划的文件）。点一项展开它自己的步骤。只管这一个任务。
5. **顺手修的**：
   - 子代理的审批按最长的前缀转（member 派的 explorer 的审批不会被 member 截走）。
   - `subagent.started` / `completed` 存进库以后读不回来（变成 `raw`），重开任务时 LOG 里「派了谁」、子任务那一行的头都没了——v0.9 起就有，这版改成手写读回，旧记录也读得回来。

- 新加依赖：没有。数据库没动（schema 11）。`ui` 的 `subagents` 多 `member`。事件：多了 `board.updated`；`spawn_agent` 多 `paths`、`title`。

## 验证

- **自动测试**：全量 `cargo test` 过（317 个；新加：Claude Code 清单转换、划文件的规范和重叠、排队先派先跑、任务板状态、两个 worker 同时跑 + 重叠的排队 + 改别人那块被拒 + 主角色被拒、member 在自己那块里改并再派 explorer、沙箱规则只放开那几块、嵌套审批的转发、事件读回）；`pnpm check`、`pnpm build` 过。
- **`?mock`**：任务板（三个在跑、一个排队、做完一个接着跑下一个）、点开 member 看到它的步骤和它派的 explorer、设置 → AGENTS 五块（明暗都看了）。
- **真跑**（账在 roadmap）：主角色 gpt-6-luna 派两个 DeepSeek Flash 的 worker，`paths` 各写对一个文件，两个文件都建好，任务板两项做完；它是分两次回答派的（两个有一段同时在跑），测试把主角色卡在 3 轮，收到报告后的收尾那一轮没发（测试已改成 5 轮，这一版的次数用得差不多了，没再跑）。
- **正式版**：装好了，版本号 1.1.3、签名校验过；窗口里没点过。
- **没验的**：
  - member 没真跑过（只有假模型的测试）；命令被划文件的沙箱拦下、弹卡片那条路没真跑过。
  - Claude Code 的清单只用夹具式的测试验过，没真跑一次 Claude Code（站主平时要用）。
  - 正式版窗口里一下都没点过：任务板、设置 → AGENTS 的 member。

## 留给站主的

- **第一次打开新版**：可能弹钥匙串授权框。
- **AGENTS 变了**（1.1.2 起，正式版还没打开过）：没有 API 单干了。你现在选的单干 Sonnet 5.5 打开新版就是 PLOOP · 主角色 Sonnet 5.5；委派 DeepSeek Flash 换成 worker 的模型，别的几类（含 member）跟主角色。想让 member 用更贵更聪明的、explorer 用便宜的，到设置 → AGENTS 选。
- **走一遍**：选 PLOOP，在测试项目里发「把 X 和 Y 分给两个 worker 同时改，Z 比较复杂交给 member」，看任务板、点开一项看它的步骤。
- **Hook**：想试的话在测试项目的 `.claude/settings.json` 写一条（格式和 Claude Code 一样），第一次会弹卡片。
- **Tavily**：还没存进钥匙串。只有 Vertex 上的 Gemini、Gemini 2.x 用，DeepSeek 搜不了时也是退路。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.3 的全部提交。
- 以前留下、还没验的：1.1.2 的正式版窗口（AGENTS、真派子代理、脚本 Hook 挂真的 `.claude/settings.json`）、searcher / auditor 真跑、百炼新加坡地域；1.1.1 存 Tavily 的 Key、真搜一次；1.1.0 的插件界面和收尾提交；1.0.3 的轮数设置、旧任务的样子；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。Gemini 原生搜索、Vertex 上的 Claude 和 Gemini 还没真跑过。

## 已知问题

| 问题 | 建议 |
| --- | --- |
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

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、PLOOP 每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。 1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字、recap 里收尾提交的那一行。 1.1.2 新加的：LOG 上面的成员名单（`.log-roster`）、设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。
- 联网搜索的协议各家常变（Gemini 的工具组合还是 Preview、Anthropic 搜索工具按日期出新版本）：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

自定义子代理（站主定了下一步做）。其他候选在 roadmap「之后要做的」。
