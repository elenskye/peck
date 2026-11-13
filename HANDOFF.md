# 当前交接

更新：2026-11-13 晚（本地账号）。**v0.5 完成**（tag `peck-v0.5.0`），R4、R5 站主已提交（`201ef85`、`4ee69db`）。这一轮 **Refine R6** 在工作区，没提交，等站主提交。规则在 [CLAUDE.md](./CLAUDE.md)，Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，结论都并进了 [architecture.md](./docs/architecture.md)、[design.md](./docs/design.md)。

## 云端开 v0.6 前要知道的

本地改了不少云端写的文件（`harness/`、`src/` 下的界面），合进 `main` 后以 `main` 为准：

- **「谁来干」在左栏下面的【AGENTS】**（`src/shell/AgentsPanel.tsx`，R4）：FLOW 分段（API、CLI、接力、规划、假）；CLI 五家点选 + 这家的模型和强度；API 模型下拉 + 强度；规划两个下拉。输入框下只剩一行「谁」（点它跳到 AGENTS）、权限、SEND。设置里 `picks` 记每种干法上次选的、`cliModels` 记每家 CLI 的模型、`fold` 记左栏两块收没收起。ONE SHOT 和项目设置的「默认谁来干」还用 `ModelPicker.tsx` 的 `WhoPicker` 三个按钮。
- **CLI 能选模型**（R4）：`gateway/cli_models.rs` 从 CLI 自己读模型列表（Codex、Muse `model/list`；Grok、Antigravity ACP `session/new` 的 `configOptions`；Claude Code 用别名），存在 `app_settings.cli_models`；发任务、ONE SHOT 都带 `model`、`effort`；Claude Code 引擎加了 `--effort`。
- **CLI 只用客户端那一个账号**（R6，站主定）：Claude Code、Codex 不再有多个账号，都用 CLI 自己的默认位置（`~/.claude`、`~/.codex`），和 Claude 客户端、ChatGPT 客户端同一个账号；`accounts.rs` 只剩看状态和登录 / 重新登录，加账号、切账号、删账号的 IPC 删了，发任务、模型列表、QUOTA 都不再加 `CLAUDE_CONFIG_DIR` / `CODEX_HOME`。
- **额度与花费改由本地账号做**（R5，站主定）：不再另建 `src/quota/`，CLAUDE.md、roadmap 已改；Claude 的额度改用 Claude Code 自带的 `/usage` 读（`usage.rs` 的 `query_claude`），桌面 App 开着也算在跑。
- **PROJECTS 底下的 OPEN FOLDER / PATH / 提示去掉了**（R5）：[ PROJECTS ] 旁边一个文件夹图标是新增环境的入口（浏览器演示里点它出一行路径）。
- **右栏五个页签**（R4）：CHANGES、LOG（原来在左栏）、ONE SHOT、FILES、TERM，只有像素图标、选中的带名字；四个新图标在 `src/app.css`（`.pk-i--changes/log/oneshot/files`）。
- **输入框 ↵ 发送**，⇧↵ / ⌘↵ 换行（`MarkdownEditor.tsx`，输入法组字的回车不算）。
- **Key 两包、打开 App 就读**（`api/keys.rs`）：AI 供应商一包（账户 `keys`）、其他 Key 一包（`other-keys`，`OTHER_KEYS` 现在是空的，以后加的都放这里）；`Keys::load_unlocked` 在启动时后台读，读完发 `keys-loaded`。没读到 Key 时模型列表里没有 API 模型，所以这一步要紧。
- **中文进像素字**：Doto 里混的中文落到缝合像素 12（`src/app.css` 的 `Peck Pixel CJK 12/13/14`，按 12 号画）；菜单动作、右键菜单也是像素字。
- **左栏和项目**（`projects.rs`、`LeftColumn.tsx`、`ProjectPage.tsx`）：右键菜单、就地改名、拖动排序（`app_settings.layout`）、移出列表（`Workspace.hidden`）、删任务（旧 API 层的记录只断开关联）、文件夹不在了（`ProjectOverview.missing` + FIND FOLDER）、项目设置（默认谁来干 / 权限、「总是允许」能撤）。roadmap Phase 9 写了这些已经做了，接着扩。
- **QUOTA 页**（`UsagePage.tsx` + `spend.rs`）：从事件日志画图；订阅额度的刻度、数字、折线、手工记都是「还剩多少」。
- **设置页**重排过：CLI 检测一份五家清单（CLI 目录空着时自己找）、供应商只留 Key / 百炼地域 / Vertex、权限能撤「总是允许」、旧东西归开发者选项 LEGACY。五家 CLI 各一色（`engineDot`，三个新颜色在 `src/app.css`）。
- **安全和正确性**（本地验收审查出来修的）：文件工具不能经大小写或符号链接写进 `.git`；高危规则多了「看不出会做什么的命令」一类；沙箱 `(deny lsopen)`、`(deny appleevent-send)`；环境变量去掉 `SSH_AUTH_SOCK` 和更多凭据名；Claude Code 流式回答不再显示两遍；同一任务接着发给 CLI 会续接；花费上限每次请求前现算；长输出按任务落盘；超过 8MB 的文件流式读；事件攒 40ms 一起放；前端包拆成四块。

## 这一轮（Refine R6，站主 11-13 晚）做了什么

1. **Claude Code、Codex 只用和客户端一样的账号**：不再有多个账号；设置 → CLI 检测底下的「登录」只剩两行（Claude Code、Codex），能看登录状态、LOGIN / RELOGIN（切换登录）、CHECK。切了以后让客户端也是这个账号，站主自己统一。
2. 发任务、CLI 的模型列表、Claude 的额度（`/usage`）都直接用默认位置，不再加环境变量。站主的数据里本来就没有加过的账号，不用搬。

**验证**：`pnpm check` 过；全量 `cargo test` 217 过、0 失败、7 忽略（多账号的测试换成「只列客户端那一个」）；`?mock` 里登录一块只有两行、CHECK ALL 后显示已登录和邮箱，没有 ADD、USE、DELETE。原生 App 没点；真登录没跑（LOGIN 会真去登，留给站主）。

## 上一轮（Refine R5，已提交）

1. **Claude Code 选模型带版本号**：读 Claude Code 自己缓存的模型目录（配置目录下 `cache/model-catalog/`）：Opus 5.5、Fable 5.2、Sonnet 5.5、Haiku 5.5 在前，Opus 4.8 这些旧版本在后，各带强度档；没有缓存才退回别名。输入框下那一行「谁」写模型名。
2. **PROJECTS**：去掉底下的 OPEN FOLDER、PATH 和右键提示；[ PROJECTS ] 旁边加文件夹图标（新增环境）。
3. **多文件夹环境**：问了站主——照原计划 v0.7 Phase 9（云端）做；每个文件夹两档权限「能改 / 只读」，记进了 roadmap。
4. **QUOTA 交给本地 Claude**，这一轮的优化：
   - Claude 读不到额度的原因：status line 只在终端里跑 Claude Code 时才触发，桌面 App 不跑它；`cli/bin` 这份 Claude Code 也没登录。改成桌面 App 或 Claude Code 在跑时，三分钟一次跑 Claude Code 自带的 `/usage`（本地命令，0 轮、不进模型、不花额度），读 5 小时、每周、按模型分的周窗口和重置时间；没登录时卡片上写怎么登（设置 → CLI 检测 → Claude Code 的账号点 LOGIN，或终端 `cli/bin/claude auth login`）。
   - 找 Claude Code 照「CLI 检测」的目录（原来只看 PATH，找不到就当没装）。
   - 这次没读到时，写原因、照旧显示上次的读数；按模型分的窗口名字照写（「每周 Sonnet」）。
5. CLAUDE.md、roadmap：额度与花费不再是 Peck 自己的作业；Peck 做好以后接手站主的其他项目。

**验证**（分开记）：
- 自动测试：`pnpm check` 过；全量 `cargo test` 217 过、0 失败、7 忽略（新加：Claude Code 模型目录、`/usage` 的几种行和重置时间、桌面 App 算在跑）。`target` 目录中途被清空过一次（不是这边删的），重新编译、联网拉了一次依赖。
- 真 Claude Code（本机，不花额度）：`claude -p /usage` 在没登录时 0 轮、0 花费，只出花费几行，Peck 认成「读不到」；`auth status` 是没登录。**登录以后的输出没在真机上见过**，格式照 Claude Code 2.1.322 程序里的写法解析（`Current session: N% used · resets …`），站主登录以后要看一眼 QUOTA。
- `?mock`：PROJECTS 头上的图标、点它出路径框；Claude Code 模型列表带版本、选了以后「谁」那一行写名字；QUOTA 页正常。
- 正式包：`pnpm app:install` 装进 /Applications。
- **没验的**：原生 App 里一项都没点；Claude 额度要站主先给 CLI 登录才看得到。

## 授权账本（v0.5，`peck-v0.5-2026-11-12`）

见 roadmap「现行授权」。DeepSeek 超了 1 次（6/5，原生验收那次模型多搜了几轮）。这一轮没有真跑。

## 退回项

（验收页没交，没有退回项）

## 已知问题（没修，记着）

| 问题 | 建议谁、什么时候 |
| --- | --- |
| 高危规则是黑名单：站主 gitconfig 里自己的别名（比如 `git p` = push）、跑项目里的脚本文件都看不出来；兜底是沙箱和撤销 | 记着；要更严得换成白名单，问站主 |
| 长任务里每批事件还是整个任务重算一遍（现在 40ms 一批，够用）；要更快得改成增量 | 云端，v0.8 Phase 12（打磨） |
| 规划 + 实施、接力、假 Agent 还在旧代码上（设置的 LEGACY 一块）；接力只有 Muse ↔ Grok 两对 | 云端，v0.8 Phase 11（roadmap 已写：任意两个引擎） |
| 身份色三个新颜色、中文像素字的 `@font-face` 只在 `src/app.css` | 本地改方案时并进 tokens，发新版本再同步 |
| 删任务会连 Peck 循环的事件日志一起删，QUOTA 的图和这个月的花费跟着少 | 记着；要留花费得把花费单独记，问站主 |

## 下一步

1. **站主**：提交、推送 R6：
   ```bash
   git add -A && git commit -m "v0.5: cli uses the same account as the client" && git push
   ```
2. **站主**：给 `cli/bin` 的 Claude Code 登录一次（设置 → CLI 检测 → 登录 → Claude Code 点 LOGIN），和 Claude 客户端同一个订阅；退出旧的 Peck 再打开，几分钟后 QUOTA 的 Claude 卡片应该有 5 小时、每周。
3. **云端**：开 Phase 7。开工前合进 `main`，先看上面「云端开 v0.6 前要知道的」。

## 视觉基线要补的（本地改方案时做）

- 方案里没有、云端取了沉静一边的样子：折叠的工具调用行、思考行、花费行、模型菜单（分组、长名字截断、固定定位）、左栏的 AGENTS 和模块收起的小三角、右栏页签的四个新图标、Markdown 正文（标题、表格、代码块）、输入框编辑器、`[ RISK ]` 卡片的红边、Doto 里中文落到缝合像素。
- `bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；`DESIGN.md` 菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式。v0.7 要画 ⓘ 图标。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 额度与花费（QUOTA 页）站主 11-13 改由本地账号做，不再另建 `src/quota/`；Peck 做好以后接手站主的其他项目。
- Codex 暂时和 ChatGPT 客户端共用登录（站主 11-12 同意），要独立时再说。
