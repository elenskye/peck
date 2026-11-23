# 当前交接

更新：2026-11-23。**v1.1.0（插件）在做：Phase 1–3 完成，还剩 Phase 4（Git Plan）、Phase 5（收尾钩子和收尾）**，表在 [docs/roadmap.md](./docs/roadmap.md)。正式版还是 v1.0.3，Phase 5 收尾时才装。规则在 [CLAUDE.md](./CLAUDE.md)，插件怎么设计的在 [docs/architecture.md「插件」](./docs/architecture.md)。

## 现在做到哪

- **Phase 1 插件库和自带插件**：插件格式的读取；库在数据目录 `library/`，源在 `src-tauri/defaults/library/`（**现在是空的**，Git Plan 在 Phase 4 放进去）；自带的三个（撤销、定时任务、写 Skill）按项目开关；`create_skill` 只写项目；全局 Skill、全局命令库没有了。
- **Phase 2 项目这边**（`src-tauri/src/project_plugins.rs`）：点开项目时认项目结构（补 `CLAUDE.md` / `AGENTS.md`、`.claude/skills` / `.agents/skills` 的软链接，一个项目只做一次）、装「每个项目都装」的插件；INSTALL / UPDATE / REMOVE；Skill 和 `/` 命令只看项目里的加自带的，插件的命令是 `/<插件名>:<命令>`。
- **Phase 3 界面**（`src/shell/Plugins.tsx`）：设置 → PLUGINS（BUILT-IN、LIBRARY、MCP SERVERS，原来的 MCP 页并进来）；项目设置 → PLUGINS（并掉了 COMMANDS，顺序是 PLUGINS、SKILLS、MCP）。每个插件点名字展开看它的 SKILL、COMMAND、TOOL、HOOK、MCP。
- 新加依赖：没有。数据库没动（schema 11）；项目设置多了 `pluginsOff`、`prepared`、`pluginsRemoved`，`app_settings` 多一行 `plugins`。

## 验证

- **自动测试**：全量 `cargo test` 287 过、0 失败、7 忽略；`pnpm check`、`pnpm build` 过。
- **`?mock`**（演示数据在 `src/dev/pluginMock.ts`，里面的 Git Plan 是假的）：设置 → PLUGINS 三块和展开的清单，明暗各看了；项目设置里 UPDATE、INSTALL、REMOVE（点两下）、自带插件的开关都点过，SKILLS 跟着变；输入框打 `/git` 列出三个带前缀的命令，选了、带参数发出去了。控制台没有报错。
- **没验的**：
  - 原生窗口一次没开过：点开真项目时往项目里写文件（软链接、装插件）只有单元测试。
  - 库是空的，所以真 App 里 LIBRARY 现在什么都没有，INSTALL / UPDATE 在真 App 里走不到。
  - 真模型、真 CLI 没跑；各家 CLI 是不是真的从 `.agents/skills` 读 Skill 没验（按站主给的方案做的）。
  - 这一版还没做收尾要的那几样：两个只读子代理审、升级测试（都在 Phase 5）。

## 下一步：Phase 4 Git Plan

内容照 roadmap 的 Phase 4 那一行。接手时要知道的：

- **站主的原文**（五条，Git Plan 的 Skill 照这个写；原文件在站主桌面，不在仓库里）：
  1. 版本号 `x.y.z`：小幅优化、修 Bug 升 z；新增功能、大幅优化升 y；升 x 由站主定。
  2. 升 y、升 x 的开发计划要分 Phase（比如 1.2.0 Phase 1–5）；升 z 不用。
  3. 升 z 做完自动更新正式版；升 y、升 x 只在最后一轮收尾时自动更新（站主可以手动更新，或在某个 Phase 要求更新）。
  4. Git：提交说明 `vx.y.z: dev description`，description 简约概括（可以直接写 `fix some problems`、`minor improvement`）；不署名；每一轮开发自动 commit，push 站主做，每轮告诉站主有哪些提交、各是什么。
  5. 每次开发前先问站主这次属于哪种程度。（原文写的是某个项目名，Skill 里写成通用的。）
- 「更新正式版」每个项目做法不一样：Skill 里写「按项目说明文件里写的步骤；没写就问站主」。
- **放哪**：`src-tauri/defaults/library/plugins/git-plan/`（`.claude-plugin/plugin.json` 带 `peck.title: "Git Plan"`、`peck.everyProject: true`、`peck.tools: [git_commit]`；`skills/git-plan/SKILL.md`；`commands/{plan,release,commits}.md`；`hooks/hooks.json` 的 `Stop` 那一条先写上，Phase 5 才跑）。放进去以后 `plugins::tests::bundled_library_items_all_parse` 会查它读不读得了。
- **`git_commit` 工具**：加进 `plugins.rs` 的 `KNOWN_TOOLS`（现在那里只有自带插件的五个，测试断言「每个工具归且只归一个自带插件」要跟着改）。它是库里插件要的工具，只在项目里装着声明了它的插件时才给——这条线现在没有：自带插件按 `plugins_off` 关，项目里装的插件的 `peck.tools` 还没人读。Peck Loop（`harness/loop.rs` 的 `drive`、`run_tool`）和本机 MCP（`gateway/mcp.rs` 的 `Offer`）两边都要接。
- 装了 Git Plan 的项目每轮会多一笔自动提交（Phase 5）：站主的规则里 Peck 自己这个仓库也是每轮提交，到时别提交两遍。

## 留给站主的

- **push**：本地 `main` 比 `origin/main` 多 1.0.2、1.0.3 和 1.1.0 的四笔。
- 以前留下、还没验的：1.0.3 的接力 SPEC 一行、轮数设置、旧任务的样子在正式版里走一遍；钥匙串里的 MCP 环境变量（第一次弹授权）、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、项目默认预设、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 只有界面点开项目时才认结构、装默认插件；定时任务在一个从没点开过的老项目里跑时不会先做 | Phase 5 看要不要在发任务前也做一遍 |
| 插件带的 MCP 服务器（`.mcp.json`）只列出来，Peck Loop 不起 | 有插件要用时再接 |
| 单独的 Skill 装没装是按「项目里有没有同名的 Skill」认的：项目自己写了一个和库里同名的，会显示成装着的 | 记着 |
| 自带插件的 Skill CLI 引擎读不到全文，只靠本机 MCP 的工具说明里那几句 | Phase 5：CLI 经本机 MCP 读自带 Skill |
| CLI 续接会话的花费按「会话号」取差：续接后会话号变了就整个算进去（不会少算，可能多算一次） | 真跑时顺便看 QUOTA |
| 模型只在被点名时才提议存 Skill，主动提的没见过 | 用一阵再看 |
| 定时任务的「谁来干」设的时候不查 Key 和 CLI 在不在，到点起不来记成错过 | 记着 |
| 回退只按整个文件放回，做不到一个文件里的某一段 | Skill 里让模型直接改那一段 |
| 预览的 RELOAD 回到最初的地址；`localhost` 只连 127.0.0.1 | 记着 |
| Vertex 上的 Claude / Gemini 只有离线测试，没真跑过 | 站主开了 GCP 项目再试 |
| PREVIEW 批注只在浏览器存储里；Esc 关设置页时没存的 MCP 编辑会丢 | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、合作每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。 1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke` 在 Peck 仓库外。
