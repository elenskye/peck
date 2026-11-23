# 当前交接

更新：2026-11-23。**v1.1.0 在做，Phase 1 完成**（一共 5 个 Phase，表在 [docs/roadmap.md](./docs/roadmap.md)）。正式版还是 v1.0.3，1.1.0 最后一个 Phase 收尾时才装。规则在 [CLAUDE.md](./CLAUDE.md)。

## 1.1.0 要做什么

插件：Peck 有一个插件库，项目里点 INSTALL 把插件或 Skill 拷进项目、跟着项目的 git 走；自带的（撤销、定时任务、写 Skill）嵌在程序里按项目开关；第一个库里的插件是 Git Plan（版本和提交的规矩、三个命令、`git_commit` 工具、收尾自动提交）。

站主定的（2026-11-23）：

- 老项目在新版里第一次点开时也自动认结构、装默认插件。
- 插件的命令一律带插件前缀（`/git-plan:plan`），已写进 CLAUDE.md「插件」。
- 收尾自动提交只提交这一轮改过的文件，不弹卡片。
- 一个 Phase 做完停下汇报，站主说继续再做下一个。
- QUOTA 等「Claude 桌面 App 或 Claude Code 在跑」时才读 Claude 订阅额度：没问题，留着。

## Phase 1 做了什么

1. **插件格式的读取**（`src-tauri/src/plugins.rs`）：照 Claude Code 的写法读 `plugin.json`、Skill、命令、Hook、MCP；`plugin.json` 的 `peck` 一节放 Peck 多认的（界面名字、要用的 Peck 工具、默认每个项目都装）。
2. **插件库**：数据目录 `library/`。源在 `src-tauri/defaults/library/`（现在是空的，Git Plan 在 Phase 4 放进去），`build.rs` 打进程序，打开 App 时按版本写进数据目录，不降级。
3. **自带插件**：`src-tauri/defaults/plugins/` 三个——`peck-snapshots`、`peck-schedules`、新加的 `peck-skills`（`create_skill` 加一份讲怎么写 Skill 的 Skill）。按项目关（`ProjectSettings.pluginsOff`），关了它的 Skill、工具、系统提示里那一句都不给，Peck Loop 和 CLI 的本机 MCP 一样。**开关的界面在 Phase 3**，现在只有后端。
4. **`create_skill` 只写项目**：去掉 `global`（传了被拒，回一句「请站主从插件库里装」）；和自带插件的 Skill 同名的提议也拒。Skill 卡片上「项目 / 全局」的选择去掉了。
5. **不再读数据目录的 `skills/`、`commands/`**：全局 Skill、全局命令库没有了。`/` 现在只列 Peck 自己的八个，插件带的命令等 Phase 2 项目里装了插件才有。
6. IPC `plugin_list`（自带的和库里的），Phase 3 的设置页用。

- 新加依赖：没有。数据库没动（schema 11）；项目设置多一个 `pluginsOff`。
- 正式数据目录里的 `agents/`、`flows/`、`worker-runtime/`、`api-budget.db`、`api-budget-mock.db` 按站主说的删了（724 MB）。

## 验证

- **自动测试**：全量 `cargo test` 283 过、0 失败、7 忽略；`pnpm check`、`pnpm build` 过。新加的：读完整插件和单独的 Skill、读不了的报原因、库按版本写入不降级、自带插件关了工具和 Skill 都不给（Peck Loop、本机 MCP 各一项）、`create_skill` 传 `global` 被拒、和自带 Skill 同名被拒。
- **`?mock`**（暗色）：项目设置 SKILLS 列出自带的三个、COMMANDS 写「没有插件带的命令」；Skill 卡片写「存进这个项目 .claude/skills/…」，没有全局的选择；控制台没有报错。
- **没验的**：亮色没看；打开 App 时写插件库那一步只有单元测试（库的源现在是空的）；真模型、真 CLI 没跑。

## 留给站主的

- **push**：本地 `main` 比 `origin/main` 多 1.0.2、1.0.3 和 1.1.0 Phase 1 的提交。
- 以前留下、还没验的：1.0.3 的接力 SPEC 一行、轮数设置、旧任务的样子在正式版里走一遍；钥匙串里的 MCP 环境变量（第一次弹授权）、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、项目默认预设、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 自带插件的 Skill CLI 引擎读不到全文，只靠本机 MCP 的工具说明里那几句 | Phase 5：CLI 经本机 MCP 读自带 Skill |
| CLI 续接会话的花费按「会话号」取差：续接后会话号变了就整个算进去（不会少算，可能多算一次） | 真跑时顺便看 QUOTA |
| 模型只在被点名时才提议存 Skill，主动提的没见过 | 用一阵看要不要在系统提示里说得更具体 |
| 点名「DeepSeek」这种对得上好几个模型的，工具会让模型回头问站主要哪一个 | 用一阵看烦不烦 |
| 定时任务的「谁来干」设的时候不查 Key 和 CLI 在不在，到点起不来记成错过 | 记着 |
| 回退只按整个文件放回，做不到一个文件里的某一段 | Skill 里让模型直接改那一段 |
| `/review` 只把上一次的请求和改了的文件写给审查的那一个，不出逐条清单 | 用一阵再看 |
| 预览的 RELOAD 回到最初的地址；`localhost` 只连 127.0.0.1 | 记着 |
| Vertex 上的 Claude / Gemini 只有离线测试，没真跑过 | 站主开了 GCP 项目再试 |
| PREVIEW 批注只在浏览器存储里；Esc 关设置页时没存的 MCP 编辑会丢 | 记着 |
| 这次排查没删、有意留着的：`AgentCaps` 里 12 个没人读的能力标记（各引擎的契约，牵扯面大）；测试用的回放管道 `Pipe::Script`、快照的 `undo_*` 测试工具；压缩时认旧 `<peck-verify>` 消息的那一处（1.0.1 的对话还有）；只在自己文件里用、多写了 `export` 的类型 | 要删再说 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、合作每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke` 在 Peck 仓库外。

## 下一步

站主说继续，做 **Phase 2 项目这边**：认项目结构（说明文件、Skill 文件夹的软链接）、INSTALL / UPDATE / REMOVE、默认注入、`/` 命令从项目里装的插件来。架构文档里「打开项目不改项目里的文件」那一句到时要改。
