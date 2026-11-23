# 开发路线

## 现在

**v1.1.0 在做**（2026-11-23 开）：插件。Peck 有一个插件库，项目里点 INSTALL 把插件或 Skill 拷进项目、跟着项目的 git 走；第一个库里的插件是 Git Plan。Phase 表在下面。正式版还是 **v1.0.3**，1.1.0 最后一个 Phase 收尾时才更新。

## 定位

Peck 是只为站主、只在站主这台 Mac 上跑的个人 **Harness**：包在模型外面的那一层——把各家模型和 CLI 接进来（Model Gateway）、给模型手脚（工具箱）、管它看到什么和记住什么（上下文）、每个任务前后存快照、把这一切摆给站主看（界面）。能在对话里说清楚的事不做成界面：反悔、定时任务、起 dev server、改完跑什么测试都交给模型。不考虑多用户、跨平台、团队，放开用 macOS 本机的能力。不跑本地模型、不做云同步。

- **干活的是 Peck Loop**：通过 Gateway 调任何 API 模型，带自己的工具箱和上下文管理。
- **CLI 是可换的引擎**：Claude Code、Codex、Muse、Grok 当整包引擎也能选，Peck 用它们的结构化接口显示每一步、替它们审批、记花费。
- **视觉**照 `src/vendor/peck/`；新图标、新组件先在方案里画，同步办法见 architecture「视觉、打包和签名」。

## Phase

分 Phase 的版本（`1.x.0`、`2.0.0`）开工前在这里写表，站主看过再做。只做表里写的，不扩；状态只在「状态」列，四种：`未开始`、`进行中`、`等站主批`、`完成`；一个 Phase 做完改状态、提交。一个版本做完，表换成下一个版本的。

### v1.1.0 插件

**三种东西，各放各的**：

| 是什么 | 例子 | 放哪 | 谁能改 | 怎么到项目里 |
| --- | --- | --- | --- | --- |
| 自带插件 | 撤销（`peck-snapshots`）、定时任务（`peck-schedules`）、写 Skill（`peck-skills`） | 嵌在程序里 | 只有开发 Peck 时 | 不写进项目：跑任务时现给，项目设置里能一个个关；跟着 Peck 的版本自动更新 |
| 插件库 | Git Plan；单独的 `SKILL.md` | 数据目录 `library/plugins/<名字>/`、`library/skills/<名字>/`；源在 Peck 仓库 `src-tauri/defaults/library/`，打进程序，打开 App 时库里没有或版本旧就写进去 | 只有开发 Peck 时 | 项目设置里 INSTALL：拷进项目，之后是项目自己的文件。库里版本新了出 UPDATE，点了才覆盖 |
| 项目自己的 | 模型用 `create_skill` 提的、站主自己写的 | 项目里 | 模型（站主 SAVE 才写）、站主 | 本来就在 |

- **插件的写法**照 Claude Code：`.claude-plugin/plugin.json`（名字、版本、说明）、`skills/<名字>/SKILL.md`、`commands/*.md`、`hooks/hooks.json`、`.mcp.json`。Peck 多认两样：`plugin.json` 里的 `tools`（这个插件要用的 Peck 工具，比如 `git_commit`）、Hook 的 `"type": "peck"`（问模型一句 `ask`，再由 Peck 跑一个自己的动作 `run`）。
- **装进项目后在哪**：Skill 进项目的 Skill 文件夹（默认 `.claude/skills/<名字>/`，哪家 CLI 都读得到）；插件其余的（`plugin.json`、命令、Hook、MCP）进它旁边的 `plugins/<插件名>/`（默认 `.claude/plugins/`）。版本记在 `plugin.json` / `SKILL.md` 开头的 `version`，UPDATE 就是比这个。
- **各家怎么用**：`SKILL.md` 各家自己读；命令由 Peck 展开成一句话再发；工具 Peck Loop 直接调、CLI 走本机 MCP；Hook 由 Peck 自己跑。
- **站主定的**（2026-11-23）：老项目在新版里第一次点开时也自动认结构、装默认插件；插件的命令一律带插件前缀（`/git-plan:plan`），Peck 自己的几个（`/plan`、`/undo`…）不带；收尾自动提交只提交这一轮改过的文件、不弹卡片；一个 Phase 做完停下汇报。
- **不做**：联网的插件市场、在界面里编辑库、插件里跑任意脚本的 Hook（`"type": "command"` 只列出来、不跑）、子代理（`agents/`）、自动 push。数据目录的全局 Skill（`skills/`）和命令库（`commands/`）不再读：要用的做成库里的 Skill / 插件再装进项目。

| Phase | 内容 | 验收 | 状态 |
| --- | --- | --- | --- |
| 1 插件库和自带插件 | 插件格式的读取（`plugin.json`、Skill、命令、Hook、MCP）；库在数据目录，源打进程序、打开 App 时按版本写进去；自带的三个做成自带插件（Skill + 工具归在一起），按项目开关；`create_skill` 只写项目（去掉 `global`）；不再读数据目录的 `skills/`、`commands/` | 单元测试：读一个完整插件和一个单独的 Skill；库按版本写入、不降级；自带插件关了以后它的 Skill 和工具都不给；`create_skill` 传 `global` 被拒 | 完成 |
| 2 项目这边 | **认项目结构**：都没有就建 `CLAUDE.md`、`AGENTS.md` 软链接过去，建 `.claude/skills/`、`.agents/skills` 软链接过去；已有一边就只补另一边的软链接；两边都有不动（说明文件、Skill 文件夹各判各的）。**INSTALL / UPDATE / REMOVE**。**默认注入**：库里标了「每个项目都装」的（Git Plan）打开项目时装上，站主在这个项目里 REMOVE 过就不再装。Peck Loop 的 Skill 列表和 `/` 命令改成只看项目里的加自带的；插件的命令写成 `/<插件名>:<命令>` | 单元测试：六种现状（全无、只有 CLAUDE.md、只有 AGENTS.md、两个都有、只有 `.agents/skills`、软链接已在）各得到该有的结果，跑第二遍什么都不动；INSTALL 后文件在项目里、UPDATE 只在点了才覆盖、REMOVE 后不再默认装；`/` 只列项目里装的命令、都带前缀 | 未开始 |
| 3 界面 | **设置 → PLUGINS**（换掉 MCP 那一页）：BUILT-IN、LIBRARY、MCP SERVERS 三块；每个插件点开列出它的 SKILL、COMMAND（怎么打、参数、做什么）、TOOL、HOOK、MCP，版本，「每个项目都装」的开关。**项目设置 → PLUGINS**（并掉 MCP、SKILLS、COMMANDS 三节）：自带的 ON / OFF，库里的 INSTALL / UPDATE / REMOVE，项目自己的 Skill，MCP | `?mock` 明暗各看一遍：设置页三块、插件展开、项目设置里装 / 更新 / 移除三种状态；`pnpm check`、`pnpm build` 过 | 未开始 |
| 4 Git Plan | 库里第一个插件，默认每个项目都装。**Skill**：版本号规则（`x.y.z`，修 Bug 升 z、新功能升 y、升 x 站主定）、`x.y.0` 先分 Phase、什么时候更新正式版、提交说明 `v<版本>: <description>`、不署名、不 push、开工前先问程度。**命令**：`/git-plan:plan` 写 Phase 计划、`/git-plan:release` 改版本号、`/git-plan:commits` 汇报还没推的提交。**工具** `git_commit`：只会提交（能指定文件），不 push、不改历史、不署名，说明不合格式的拒绝；PLAN 不给，EDIT 问一次、AUTO 不问；CLI 经本机 MCP 用同一个 | 单元测试（临时 git 仓库）：合格的说明提交成功、用仓库自己的 `user.name`；带署名的、格式不对的、PLAN 下的被拒；没有改动时说明白不提交；本机 MCP 里看得到 `git_commit` | 未开始 |
| 5 收尾钩子和收尾 | **Hook**（`Stop`：一轮做完）：任务做完、改了文件、是 git 项目、不是 PLAN，Peck 让刚干完的那个模型用一行写提交说明（Peck Loop 在原对话里多问一轮、不给工具；CLI 续上它的会话问），Peck 校验后自己跑 git 提交这一轮改过的文件，结果写成对话里一行（短哈希 + 说明）；没答、格式不对就不提交、写明原因。Antigravity、独立工作区的任务不跑。CLI 经本机 MCP 读自带 Skill 的全文。文档、全量测试、按授权真跑一个小任务、装正式版 | 假模型、假引擎的测试：做完自动出一笔合格的提交、不 push；没改文件、失败、打断、PLAN 都不提交；真跑一次（最便宜的模型，`projects/tester`）看到提交；`cargo test`、`pnpm check`、`pnpm build` 全过；装进 /Applications | 未开始 |

## 之后要做的

候选，没排期，要做时站主定进哪个版本：

- 用 Peck 开发 Peck 一整轮；Peck 接手站主的其他项目。
- 多家 CLI 和 Peck Loop 的 Benchmark（Atelier MCP 那一轮没跑满的一起补）。
- xAI 直连、GitHub Copilot CLI、Amp、Qwen Code。
- 会话树和分叉浏览；只读探索 subagent。
- 快照回退、定时任务在真 CLI 上各走一遍（现在只有假模型和假 MCP 客户端的测试）。

## 现行授权

只管 Claude 自己的真跑，每个版本重新算；站主自己在 App 里发的任务不算，只受设置里的花费上限管。

| 对象 | 每个版本的上限 | 说明 |
| --- | ---: | --- |
| 每家 API（Anthropic、OpenAI、DeepSeek、Aliyun、Gemini、Vertex 上的 Claude、Vertex 上的 Gemini） | 5 次尝试 | 一次 HTTP 发送尝试计一次，失败、取消、重试都计 |
| 每家 CLI（Codex、Claude Code、Muse、Grok、Antigravity） | 3 个小任务 | 用站主已登录的订阅；每个任务不超过 12 轮；Claude Code 尽量不用（站主平时要用） |

- 一律用这家最便宜、又测得到要测功能的模型，做无关紧要的小任务；测试项目放仓库外的 `personal-cli-studio/projects/`。
- 用之前和之后都记账；失败不静默重试；用完先停下问站主。
- 站主另给的额度（比如某一周按钱算）以站主当时说的为准，记在下面。

**账本**（只留当前版本）：

| 版本 | 对象 | 模型 | 用了 | 做了什么 |
| --- | --- | --- | ---: | --- |
| v1.1.0 | — | — | 0 | 没有真跑 |
