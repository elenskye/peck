# 当前交接

更新：2026-11-23。**v1.0.3**（站主标的版本号，不分 Phase），已装进 /Applications。规则在 [CLAUDE.md](./CLAUDE.md)，定位和 Phase 在 [docs/roadmap.md](./docs/roadmap.md)。

## 这一版做了什么（1.0.3）

1. **接力的对话只露实施的输出**：写规范那一段收成一行「SPEC · Claude Code 写好了规范 · 1:20 ▸」，点开看规范全文和它读过的文件；在跑时下面只露一行（在调工具就是那一步，在想就是 Thinking）。审查、按审查改那两段照旧。代码在 `agentView.ts`（写规范那一段的条目收进 `StageView.items`）和 `AgentView.tsx` 的 `SpecStage`。
2. **Peck 循环统一叫 Peck Loop**：界面、错误提示、文档、注释一起改。
3. **ONE SHOT 和子任务的轮数进设置**：设置 → GENERAL 多两行「ONE SHOT 最多几轮」（默认 12）、「子任务最多几轮」（默认 24，Peck Loop 和 CLI 委派的都按它），5–400。
4. **不读 `~/.claude/`**：Skill 不读 `~/.claude/skills/`，全局 Skill 暂存 Peck 数据目录的 `skills/`（只有 Peck 读）；斜杠命令不读 `~/.claude/commands/` 和项目的 `.claude/commands/`，改读 Peck 自己的命令库（数据目录的 `commands/**/*.md`，所有项目共用）。
5. **清掉 `.peck/` 的照顾**：快照不再强制收 `.peck/`，文件监听和 FILES 不再强制显示它（照 `.gitignore`）。
6. **清死代码**（这一版一共删 3,993 行、加 1,098 行，加的多是新功能和重做的演示数据）：
   - v0.8 以前旧记录的兼容：按步骤显示、重跑旧步骤、旧 API 层（调用表、规划 + 实施记录、启动时的恢复）、QUOTA 页底的旧调用表和 JSON 导入 / CSV 导出、旧「谁来干」的迁移、旧 Agent 清单（`agents/*.toml`）、对话消息表的读写。旧任务现在头上写「旧记录」，过程写「这次没有记下过程」，下面照样有状态和结果那一句；库里的行没删。
   - 只写不读的：任务记录里的流程 id 和名字、按步骤记的文件和快照、旧的输出日志和任务清单、改动合计、步骤上的说明和预期文件、工具上下文里的权限档。
   - 从来不发的：`retry` 事件、「引擎自己批的」、「实报」花费、`cancelled` 结束原因（停下都是 `interrupted`）。
   - CHANGES、按块接受、撤销预览的残留：图标、样式、类型，`changes_get_diff` 只回 Diff 文本。
   - 没人传的参数、用不到的样式和导出、过时的注释；依赖 `rfd` 删了。
   - `?mock` 的开局任务重做成现在的格式，每个带一段事件日志（含两个接力，开局就看得到 SPEC 那一行）。

- 站主在提问里定的：全局 Skill 先暂存 Peck 自己的目录，下一轮站主给方向；两处 `.claude/commands/` 都不读，Peck 要有自己独立的命令库；项目里的 `.claude/skills/`、`.mcp.json`、`CLAUDE.md` 兜底是正常的，只要不和 Claude Code Desktop 绑定；旧记录的兼容代码删掉。
- 排查时顺手修的：
  - **DeepSeek 余额自动查**：以前靠旧调用表判断「DeepSeek 请求在跑」，v0.8 以后那张表不写了，所以从没触发过。现在改成看有没有在跑的、用到 DeepSeek 的任务（真实模式）。
  - **Aliyun「业务空间 ID」**：设置里能填，但 Gateway 发请求从来不读它（只用地域），删了这个框；你的正式库里也没填过。
  - **QUOTA 查 Muse、Grok 的额度**：以前靠旧清单找程序，现在和发任务一样走「CLI 目录」加 shell 的 PATH。
- 新加依赖：没有（删了 `rfd`）。数据库没动（schema 11）；旧记录里多出来的字段读的时候忽略。

## 验证

- **自动测试**：`pnpm check`、`pnpm build` 过；全量 `cargo test` 275 过、0 失败、7 忽略（删了旧记录那几项测试，加了轮数设置、命令库、DeepSeek 在跑这几项）。
- **`?mock`**：
  - 接力（暗色）：在跑时 SPEC 下面依次露 Thinking…、Read …；做完收成「写好了规范」；点开是规范全文和读过的文件。
  - 开局的演示任务：接力 SPEC 一行、在跑的露当前一步、Grok 没登录的红条、按段花费表。
  - 设置 GENERAL 的三行轮数，PROVIDERS 里 Aliyun 的 SAVE 和上面对齐；项目设置的 SKILLS 写「Peck/skills（全局）」，COMMANDS 列命令库里的。
  - 亮色看了 SPEC 展开；刷新以后控制台没有报错。
- **没验的**：
  - 正式版里没点。
  - 真模型、真 CLI 的接力没跑过新的 SPEC 一行（CLI 的读文件步骤标题各家不一样：Codex 只给文件名、太长会截）。
  - 你正式库里那 13 个旧任务在新版里的样子没在原生窗口看过。
  - DeepSeek 余额自动查只有单元测试。

## 留给站主的

- **在正式版里走一遍**：发一次接力（写规范 + 实施），看 SPEC 那一行；设置 → GENERAL 调一下 ONE SHOT、子任务的轮数；打开一个旧任务看「旧记录」的样子。第一次打开可能弹钥匙串授权框。
- **命令库、全局 Skill 放在哪**：`~/Library/Application Support/Peck/commands/`、`…/Peck/skills/`，现在要自己往里放文件（以前放在 `~/.claude/commands/`、`~/.claude/skills/` 的要用就搬过来）。下一轮等你给方向。
- **可以手动删的旧东西**（Peck 不再读，我没动你的数据）：正式数据目录里的 `agents/`、`flows/`、`worker-runtime/`、`api-budget.db`、`api-budget-mock.db`。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2、1.0.3 的提交，等你推。
- 以前留下、还没验的：钥匙串里的 MCP 环境变量（第一次弹授权）、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、项目默认预设、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 还看着 Claude 桌面 App 的只有一处：QUOTA 在「Claude 桌面 App 或 Claude Code 在跑」时才去读 Claude 订阅额度（只是决定什么时候读，不读它的文件） | 要不要也断开，等你说 |
| 全局 Skill、命令库只有 Peck 读，CLI 引擎读不到（Peck 自带的两个 Skill 也是），只靠工具说明里那几句 | 下一轮定方向时一起看 |
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

等站主给全局 Skill 和命令库的方向（放哪、怎么管）。其他候选在 roadmap「之后要做的」。
