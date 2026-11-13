# 当前交接

更新：2026-11-13 傍晚（本地账号）。**v0.5 完成**（站主提交了 `a6858c1`、打了 tag `peck-v0.5.0`、删了云端分支）。之后本地按站主的意见又做了一轮界面优化 **Refine R4**（在工作区，没提交，等站主提交）。规则在 [CLAUDE.md](./CLAUDE.md)，Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，结论都并进了 [architecture.md](./docs/architecture.md)、[design.md](./docs/design.md)。

## 云端开 v0.6 前要知道的

本地改了不少云端写的文件（`harness/`、`src/` 下的界面），合进 `main` 后以 `main` 为准：

- **「谁来干」在左栏下面的【AGENTS】**（`src/shell/AgentsPanel.tsx`，R4）：FLOW 分段（API、CLI、接力、规划、假）；CLI 五家点选 + 这家的模型和强度；API 模型下拉 + 强度；规划两个下拉。输入框下只剩一行「谁」（点它跳到 AGENTS）、权限、SEND。设置里 `picks` 记每种干法上次选的、`cliModels` 记每家 CLI 的模型、`fold` 记左栏两块收没收起。ONE SHOT 和项目设置的「默认谁来干」还用 `ModelPicker.tsx` 的 `WhoPicker` 三个按钮。
- **CLI 能选模型**（R4）：`gateway/cli_models.rs` 从 CLI 自己读模型列表（Codex、Muse `model/list`；Grok、Antigravity ACP `session/new` 的 `configOptions`；Claude Code 用别名），存在 `app_settings.cli_models`；发任务、ONE SHOT 都带 `model`、`effort`；Claude Code 引擎加了 `--effort`。
- **右栏五个页签**（R4）：CHANGES、LOG（原来在左栏）、ONE SHOT、FILES、TERM，只有像素图标、选中的带名字；四个新图标在 `src/app.css`（`.pk-i--changes/log/oneshot/files`）。
- **输入框 ↵ 发送**，⇧↵ / ⌘↵ 换行（`MarkdownEditor.tsx`，输入法组字的回车不算）。
- **Key 两包、打开 App 就读**（`api/keys.rs`）：AI 供应商一包（账户 `keys`）、其他 Key 一包（`other-keys`，`OTHER_KEYS` 现在是空的，以后加的都放这里）；`Keys::load_unlocked` 在启动时后台读，读完发 `keys-loaded`。没读到 Key 时模型列表里没有 API 模型，所以这一步要紧。
- **中文进像素字**：Doto 里混的中文落到缝合像素 12（`src/app.css` 的 `Peck Pixel CJK 12/13/14`，按 12 号画）；菜单动作、右键菜单也是像素字。
- **左栏和项目**（`projects.rs`、`LeftColumn.tsx`、`ProjectPage.tsx`）：右键菜单、就地改名、拖动排序（`app_settings.layout`）、移出列表（`Workspace.hidden`）、删任务（旧 API 层的记录只断开关联）、文件夹不在了（`ProjectOverview.missing` + FIND FOLDER）、项目设置（默认谁来干 / 权限、「总是允许」能撤）。roadmap Phase 9 写了这些已经做了，接着扩。
- **QUOTA 页**（`UsagePage.tsx` + `spend.rs`）：从事件日志画图；订阅额度的刻度、数字、折线、手工记都是「还剩多少」。
- **设置页**重排过：CLI 检测一份五家清单（CLI 目录空着时自己找）、供应商只留 Key / 百炼地域 / Vertex、权限能撤「总是允许」、旧东西归开发者选项 LEGACY。五家 CLI 各一色（`engineDot`，三个新颜色在 `src/app.css`）。
- **安全和正确性**（本地验收审查出来修的）：文件工具不能经大小写或符号链接写进 `.git`；高危规则多了「看不出会做什么的命令」一类；沙箱 `(deny lsopen)`、`(deny appleevent-send)`；环境变量去掉 `SSH_AUTH_SOCK` 和更多凭据名；Claude Code 流式回答不再显示两遍；同一任务接着发给 CLI 会续接；花费上限每次请求前现算；长输出按任务落盘；超过 8MB 的文件流式读；事件攒 40ms 一起放；前端包拆成四块。

## 这一轮（Refine R4，站主 11-13 傍晚）做了什么

1. **LOG 挪到右栏**，五个页签换成像素图标（± 改动、时间线、闪电、两张纸、终端），选中的带名字、别的悬停看名字。
2. **左栏下面是【AGENTS】**（原来 LOG 的位置）：布局三处问了站主，都选了推荐的——页签选中的带字；输入框下留一行「谁」和权限；AGENTS 用「分段 + 列表」。
3. **CLI 能选模型**：从 CLI 自己读模型列表（不发任务、不花额度），AGENTS 里选；强度也跟着这个模型能选的来，「默认」是 CLI 自己定。
4. **PROJECTS 和 AGENTS 都能收起**：模块头最右边的小三角；收起的只剩头（PROJECTS 写当前项目、AGENTS 写现在是谁），另一块占满整栏。
5. 站主问 CLI 的专属命令（斜杠命令）要不要用：回答在对话里，结论是平时自然语言就够，没做界面。

**验证**（分开记）：
- 自动测试：`pnpm check` 过；全量 `cargo test` 215 过、0 失败、7 忽略（新加：四家模型列表的解析，用夹具里录到的回应；Claude Code 带 `--effort`）。
- 真 CLI（只握手、列模型，没发任务、不算授权次数）：Codex 读到 8 个、Muse 4 个、Grok 2 个、Antigravity 7 个，强度都读到了。Grok、Antigravity 那边会留一个空会话。
- `?mock`（深浅各一遍）：FLOW 四种切换、CLI 选模型和强度（四档以内分段、多了下拉）、接力、规划；输入框下那一行「谁」跟着变、点它 AGENTS 展开并闪一下；两块收起 / 展开两个方向；LOG 页签显示任务的记录；控制台没有界面报错。
- 正式包：`pnpm app:install` 装进 /Applications。
- **没验的**：原生 App 里一项都没点；CLI 选了模型以后真发一个任务（要真跑，留给站主在 App 里用；我这边没花授权）。

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

1. **站主**：提交、推送 R4（都在工作区）：
   ```bash
   git add -A && git commit -m "v0.5: agents panel, cli model choice, log tab with icons and foldable left column" && git push
   ```
2. **站主**：退出旧的 Peck 再打开新装的。
3. **云端**：开 Phase 7（roadmap 已是「进行中」）。开工前合进 `main` 的新提交，先看上面「云端开 v0.6 前要知道的」。

## 视觉基线要补的（本地改方案时做）

- 方案里没有、云端取了沉静一边的样子：折叠的工具调用行、思考行、花费行、模型菜单（分组、长名字截断、固定定位）、左栏的 AGENTS 和模块收起的小三角、右栏页签的四个新图标、Markdown 正文（标题、表格、代码块）、输入框编辑器、`[ RISK ]` 卡片的红边、Doto 里中文落到缝合像素。
- `bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；`DESIGN.md` 菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式。v0.7 要画 ⓘ 图标。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 额度监测器（`src/quota/`、`src-tauri/src/quota/`）Agent 不写；v0.6 起站主用 Peck 开发，目录还没建。
- Codex 暂时和 ChatGPT 客户端共用登录（站主 11-12 同意），要独立时再说。
