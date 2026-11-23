# 开发路线

## 现在

**v1.1.0 在做**（2026-11-23 开）：插件。Phase 1–3 完成，还剩 Phase 4（Git Plan）、Phase 5（收尾钩子和收尾）。正式版还是 **v1.0.3**，Phase 5 收尾时才更新。

## 定位

Peck 是只为站主、只在站主这台 Mac 上跑的个人 **Harness**：包在模型外面的那一层——把各家模型和 CLI 接进来（Model Gateway）、给模型手脚（工具箱）、管它看到什么和记住什么（上下文）、每个任务前后存快照、把这一切摆给站主看（界面）。能在对话里说清楚的事不做成界面：反悔、定时任务、起 dev server、改完跑什么测试都交给模型。不考虑多用户、跨平台、团队，放开用 macOS 本机的能力。不跑本地模型、不做云同步。

- **干活的是 Peck Loop**：通过 Gateway 调任何 API 模型，带自己的工具箱和上下文管理。
- **CLI 是可换的引擎**：Claude Code、Codex、Muse、Grok 当整包引擎也能选，Peck 用它们的结构化接口显示每一步、替它们审批、记花费。
- **视觉**照 `src/vendor/peck/`；新图标、新组件先在方案里画，同步办法见 architecture「视觉、打包和签名」。

## Phase

分 Phase 的版本（`1.x.0`、`2.0.0`）开工前在这里写表，站主看过再做。只做表里写的，不扩；状态只在「状态」列，四种：`未开始`、`进行中`、`等站主批`、`完成`；一个 Phase 做完改状态、提交。一个版本做完，表换成下一个版本的。

### v1.1.0 插件

项目只用项目里的插件和 Skill；Peck 管系统级的维护、更新和注入。三种来源、插件的写法、装进项目后放哪见 [architecture「插件」](./architecture.md#插件pluginsrsproject_pluginsrs)。

- **站主定的**（2026-11-23）：老项目在新版里第一次点开时也自动认结构、装默认插件；插件的命令一律带插件前缀（`/git-plan:plan`），Peck 自己的几个不带；收尾自动提交只提交这一轮改过的文件、不弹卡片；一个 Phase 做完停下汇报。
- **不做**：联网的插件市场、在界面里编辑库、插件里跑任意脚本的 Hook（`"type": "command"` 只列出来、不跑）、子代理（`agents/`）、自动 push、全局 Skill 和全局命令库。

| Phase | 内容 | 验收 | 状态 |
| --- | --- | --- | --- |
| 1 插件库和自带插件 | 插件格式的读取；库在数据目录、源打进程序、按版本写入；自带的三个做成自带插件、按项目开关；`create_skill` 只写项目 | 单元测试 | 完成 |
| 2 项目这边 | 认项目结构；INSTALL / UPDATE / REMOVE；默认注入；Skill 和 `/` 命令只看项目里的加自带的 | 单元测试（六种现状、装 / 更新 / 移除、默认注入） | 完成 |
| 3 界面 | 设置 → PLUGINS（BUILT-IN、LIBRARY、MCP SERVERS）；项目设置 → PLUGINS | `?mock` 明暗各看一遍 | 完成 |
| 4 Git Plan | 库里第一个插件（`src-tauri/defaults/library/plugins/git-plan/`），默认每个项目都装。**Skill**：版本号规则（`x.y.z`，修 Bug 升 z、新功能升 y、升 x 站主定）、`x.y.0` 先分 Phase、什么时候更新正式版、提交说明 `v<版本>: <description>`、不署名、不 push、开工前先问程度。**命令**：`/git-plan:plan` 写 Phase 计划、`/git-plan:release` 改版本号、`/git-plan:commits` 汇报还没推的提交。**工具** `git_commit`：只会提交（能指定文件），不 push、不改历史、不署名，说明不合格式的拒绝；PLAN 不给，EDIT 问一次、AUTO 不问；CLI 经本机 MCP 用同一个；只在装了要它的插件的项目里给 | 单元测试（临时 git 仓库）：合格的说明提交成功、用仓库自己的 `user.name`；带署名的、格式不对的、PLAN 下的被拒；没有改动时说明白不提交；本机 MCP 里看得到 `git_commit`；没装 Git Plan 的项目里没有这个工具 | 未开始 |
| 5 收尾钩子和收尾 | **Hook**（`Stop`：一轮做完）：任务做完、改了文件、是 git 项目、不是 PLAN，Peck 让刚干完的那个模型用一行写提交说明（Peck Loop 在原对话里多问一轮、不给工具；CLI 续上它的会话问），Peck 校验后自己跑 git 提交这一轮改过的文件，结果写成对话里一行（短哈希 + 说明）；没答、格式不对就不提交、写明原因。Antigravity、独立工作区的任务不跑。CLI 经本机 MCP 读自带 Skill 的全文。文档、全量测试、两个只读子代理审这一版、升级测试、按授权真跑一个小任务、装正式版 | 假模型、假引擎的测试：做完自动出一笔合格的提交、不 push；没改文件、失败、打断、PLAN 都不提交；真跑一次（最便宜的模型，`projects/tester`）看到提交；`cargo test`、`pnpm check`、`pnpm build` 全过；装进 /Applications | 未开始 |

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
