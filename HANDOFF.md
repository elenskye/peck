# 当前交接

更新：2026-11-23。**v1.1.0（插件）做完了，已装进 /Applications**。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md「插件」](./docs/architecture.md)，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.0，五个 Phase）

1. **插件库和自带插件**：插件照 Claude Code 的写法（`plugin.json`、Skill、命令、Hook、MCP）。自带的三个（撤销、定时任务、写 Skill）嵌在程序里，按项目开关。插件库在数据目录 `library/`，源在 `src-tauri/defaults/library/`，打开 App 时按版本写进去。`create_skill` 只写项目；全局 Skill、全局命令库没有了。
2. **项目这边**：点开项目时认结构（补 `CLAUDE.md` / `AGENTS.md`、`.claude/skills` / `.agents/skills` 的软链接，一个项目只做一次）、装「每个项目都装」的插件；项目设置里 INSTALL / UPDATE / REMOVE，装进去的是项目自己的文件。插件的命令是 `/<插件名>:<命令>`。
3. **界面**：设置 → PLUGINS（BUILT-IN、LIBRARY、MCP SERVERS）；项目设置 → PLUGINS、SKILLS、MCP。每个插件点名字展开看它的 SKILL、COMMAND、TOOL、HOOK、MCP。
4. **Git Plan**（库里第一个，默认每个项目都装）：版本和提交规矩的 Skill；`/git-plan:plan`、`/git-plan:release`、`/git-plan:commits`；工具 `git_commit`（只会提交，不 push、不改历史、不署名；说明必须是 `v<版本>: <description>`）。
5. **收尾 Hook**：一轮做完、这一轮改的文件还没提交，Peck 问刚干完的模型要一行提交说明、自己提交这一轮改过的文件，结果写在 recap 里；不弹卡片、不 push。CLI 经本机 MCP 的 `load_skill` 读自带 Skill 的全文。

- 新加依赖：没有。数据库没动（schema 11）；项目设置多了 `pluginsOff`、`prepared`、`pluginsRemoved`，`app_settings` 多一行 `plugins`，事件多一种 `hook.ran`。
- 收尾时两个只读子代理审了这一版（Rust、前端各一个），找到的 16 条修了 15 条（其中一条高危：项目里的 `plugin.json` 能让 REMOVE 删到 Skill 文件夹外面），剩的一条在已知问题里。
- 正式数据目录里旧版本留下的 `agents/`、`flows/`、`worker-runtime/`、`api-budget*.db` 按站主说的删了（`agents/` 被当时还开着的旧版 Peck 重新写出来过一次，又删了）。

## 验证

- **自动测试**：全量 `cargo test` 296 过、0 失败、8 忽略；`pnpm check`、`pnpm build` 过。
- **`?mock`**：设置 → PLUGINS 三块和展开的清单（明暗）；项目设置里 UPDATE、INSTALL、REMOVE、自带插件的开关；`/git-plan:plan` 带参数发出去；装着 Git Plan 的项目里跑一个任务，recap 里出「Git Plan 提交了 …」。
- **真跑**（Codex，一个小任务两轮，账在 roadmap）：Codex 改一个文件，续上它的会话问到一行 `v0.1.0: add hook smoke note`，Peck 提交了、只有那一个文件、没有署名。走的是引擎一层的测试（`live_stop_hook_commits_the_round`），不是整个 App。
- **升级测试**：新版打开正式数据的副本，项目、任务、事件的行数和升级前一样，插件库里写进了 Git Plan。
- **正式版**：装好了，版本号 1.1.0、签名校验过；用空的临时数据目录隔离启动过一次。
- **没验的**：
  - **正式版的窗口里一下都没点过**：设置 → PLUGINS、项目设置里的 INSTALL / UPDATE / REMOVE、点开真项目时往项目里写文件（软链接、装 Git Plan）都只有单元测试和 `?mock`。
  - 整个 App 里的收尾提交没真跑过：Peck Loop（真 API 模型）在原对话末尾多问的那一轮、CLI 任务做完后在界面上的那一行。Claude Code、Muse、Grok 续会话问那一句没跑过。
  - 各家 CLI 是不是真的从 `.agents/skills` 读 Skill 没验（按站主给的方案做的）；CLI 用 `load_skill`、`git_commit` 这两个本机 MCP 工具只有假客户端的测试。

## 留给站主的

- **第一次打开新版**：可能弹钥匙串授权框。之后点开每个项目，Peck 会往里补软链接、装 Git Plan，git 项目里会看到没提交的新文件（`.claude/`、`.agents/`，没有说明文件的还有 `CLAUDE.md`）。不想要 Git Plan 的项目，在项目设置 → PLUGINS 里 REMOVE（以后不再自动装）；不想它默认装，设置 → PLUGINS 里关 EVERY PROJECT。
- **装了 Git Plan 的项目每轮做完会自动提交**。在 Peck 里开发 Peck 自己时也一样。
- **走一遍**：设置 → PLUGINS 看 Git Plan 的清单；找一个 git 项目发一个小任务，看 recap 里那一行和 `git log`。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2、1.0.3 和 1.1.0 的全部提交。
- 以前留下、还没验的：1.0.3 的接力 SPEC 一行、轮数设置、旧任务的样子在正式版里走一遍；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、项目默认预设、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。

## 已知问题

| 问题 | 建议 |
| --- | --- |
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

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、合作每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。 1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字、recap 里收尾提交的那一行。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。

## 下一步

等站主说下一个版本做什么。候选在 roadmap「之后要做的」。
