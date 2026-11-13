# 当前交接

更新：2026-11-13 下午（本地账号）。**v0.5 做完，等站主提交**：云端分支 `claude/peck-v0.5` 已压成一份改动合进 `main`，本地验收时修了审查找出的问题，又按站主的意见做了三轮优化（Refine 1–3），正式包装好了。验收页「[Peck v0.5 验收](https://claude.ai/artifact/7udRbMmzo6C3NRLTwpE7iJ)」没交，站主 11-13 让直接提交、推送。规则在 [CLAUDE.md](./CLAUDE.md)，Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，结论都并进了 [architecture.md](./docs/architecture.md)、[design.md](./docs/design.md)。

## 云端开 v0.6 前要知道的

本地改了不少云端写的文件（`harness/`、`src/` 下的界面），合进 `main` 后以 `main` 为准：

- **「谁来干」是三个按钮**（`src/shell/ModelPicker.tsx` 的 `WhoPicker`）：FLOW（API、CLI、接力、规划 + 实施、假 Agent）决定后面出 CLI 还是 API、怎么选；设置里 `picks` 记每种干法上次选的。输入框、ONE SHOT（只有 API、CLI）、项目设置的「默认谁来干」都用它。旧的单菜单 `ModelPicker` 删了。
- **输入框 ↵ 发送**，⇧↵ / ⌘↵ 换行（`MarkdownEditor.tsx`，输入法组字的回车不算）。
- **Key 两包、打开 App 就读**（`api/keys.rs`）：AI 供应商一包（账户 `keys`）、其他 Key 一包（`other-keys`，`OTHER_KEYS` 现在是空的，以后加的都放这里）；`Keys::load_unlocked` 在启动时后台读，读完发 `keys-loaded`。没读到 Key 时模型列表里没有 API 模型，所以这一步要紧。
- **中文进像素字**：Doto 里混的中文落到缝合像素 12（`src/app.css` 的 `Peck Pixel CJK 12/13/14`，按 12 号画）；菜单动作、右键菜单也是像素字。
- **左栏和项目**（`projects.rs`、`LeftColumn.tsx`、`ProjectPage.tsx`）：右键菜单、就地改名、拖动排序（`app_settings.layout`）、移出列表（`Workspace.hidden`）、删任务（旧 API 层的记录只断开关联）、文件夹不在了（`ProjectOverview.missing` + FIND FOLDER）、项目设置（默认谁来干 / 权限、「总是允许」能撤）。roadmap Phase 9 写了这些已经做了，接着扩。
- **QUOTA 页**（`UsagePage.tsx` + `spend.rs`）：从事件日志画图；订阅额度的刻度、数字、折线、手工记都是「还剩多少」。
- **设置页**重排过：CLI 检测一份五家清单（CLI 目录空着时自己找）、供应商只留 Key / 百炼地域 / Vertex、权限能撤「总是允许」、旧东西归开发者选项 LEGACY。五家 CLI 各一色（`engineDot`，三个新颜色在 `src/app.css`）。
- **安全和正确性**（本地验收审查出来修的）：文件工具不能经大小写或符号链接写进 `.git`；高危规则多了「看不出会做什么的命令」一类；沙箱 `(deny lsopen)`、`(deny appleevent-send)`；环境变量去掉 `SSH_AUTH_SOCK` 和更多凭据名；Claude Code 流式回答不再显示两遍；同一任务接着发给 CLI 会续接；花费上限每次请求前现算；长输出按任务落盘；超过 8MB 的文件流式读；事件攒 40ms 一起放；前端包拆成四块。

## 这一轮（Refine 3，站主 11-13 下午）做了什么

1. **删任务报 FOREIGN KEY constraint failed**：旧 API 层（规划 + 实施、假 Agent 走过的）的 `api_tasks.run_id` 指着任务。删任务时先把它断开（调用记录留着、花费照算）。正式数据库的副本上复现并验证过修法，加了回归测试。
2. **输入框下三个按钮** FLOW / CLI / API（见上）。
3. **菜单里的中文用像素字**（说明除外）。
4. **CLI 额度显示还剩多少**，不再是用了多少（剩 25% 起琥珀、10% 起红）。
5. **↵ 发送，⇧↵ / ⌘↵ 换行**。
6. **Peck 每次更新后打开就弹授权窗口**：启动时后台读钥匙串；设置里的说明改成「打开 App 时自动读，拒了点 READ KEYS 再问」。
7. **Key 授权两包**：AI 一包、其他一包，每次更新最多问两次（现在其他 Key 还没有，所以实际只问一次）。

**验证**（分开记）：
- 自动测试：`pnpm check` 过；全量 `cargo test` 211 过、0 失败、6 忽略（新加 Key 两包、启动读一次、删任务带旧 API 记录三项）。
- 数据库：正式库复制一份，原来的删法照样报外键错，新删法过（回滚，没动正式库）。
- `?mock`（内置浏览器，深浅各一遍，控制台没报错）：FLOW 切 API / CLI / 接力 / 规划 + 实施，后面的按钮跟着换、菜单内容对；菜单里中文的字体是缝合像素；⇧↵、⌘↵ 换行接着列表，↵ 发出去；QUOTA 卡片显示「剩 X%」、颜色档对。
- 正式包：`pnpm app:install` 装进 /Applications（旧包备份一份）。
- **没验的**：原生 App 里一项都没点；更新后的钥匙串授权弹窗（要站主退出旧的 Peck 再打开，在弹窗里点「始终允许」）；微信输入法下回车选字不误发（逻辑和 ASK 卡片、改名框同一套判断，没在原生里试）。

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

1. **站主**：提交、推送（合并的改动在暂存区，本地改的在工作区，一起提交）：
   ```bash
   git add -A && git commit -m "v0.5: toolbox, peck loop, unified process view, one shot, app icon and refinements" && git push
   ```
   再打 tag、删云端分支：`git tag peck-v0.5.0 && git push origin peck-v0.5.0 && git push origin --delete claude/peck-v0.5`。
2. **站主**：退出正在跑的旧 Peck，再打开新装的：系统会弹钥匙串授权，点「始终允许」；设置 → 供应商和 Key 应该都是 SAVED，输入框 FLOW 里 API 能用。
3. **云端**：分支删了、`main` 上有 v0.5 以后开 Phase 7（roadmap 已是「进行中」），开工先看上面「云端开 v0.6 前要知道的」。

## 视觉基线要补的（本地改方案时做）

- 方案里没有、云端取了沉静一边的样子：折叠的工具调用行、思考行、花费行、模型菜单（分组、长名字截断、固定定位）、输入框下的 FLOW / CLI / API 三个按钮、Markdown 正文（标题、表格、代码块）、输入框编辑器、`[ RISK ]` 卡片的红边、Doto 里中文落到缝合像素。
- `bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；`DESIGN.md` 菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式。v0.7 要画 ⓘ 图标。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 额度监测器（`src/quota/`、`src-tauri/src/quota/`）Agent 不写；v0.6 起站主用 Peck 开发，目录还没建。
- Codex 暂时和 ChatGPT 客户端共用登录（站主 11-12 同意），要独立时再说。
