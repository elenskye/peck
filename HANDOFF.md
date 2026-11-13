# 当前交接

更新：2026-11-13 晚（本地账号）。**v0.6 本地验收做完，等站主提交**：云端分支 `claude/nice-goodall-usgzn5`（Phase 7 上下文、Phase 8 后台和 Monitor）已压成一份改动合进 `main`，本地审查修了 15 处，正式包 0.6.0 装进了 /Applications。站主让本地验收、清理分支（删远端分支被自动权限检查拦了，留给站主，命令在「下一步」），这一版没出验收页，没验的列在下面「验证」。规则在 [CLAUDE.md](./CLAUDE.md)，Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，结论在 [architecture.md](./docs/architecture.md)（「上下文」「后台和验证」）、[design.md](./docs/design.md)。

## 云端开 v0.7 前要知道的

- **分支名**：云端会话只许推它自己定的名字（v0.6 是 `claude/nice-goodall-usgzn5`），CLAUDE.md 已改成「以会话给的为准，HANDOFF 开头写明」。
- **`.peck/verify.toml` 要站主确认过才用**（本地加的）：项目设置里记着确认过的那一份的 md5（`ProjectSettings.verify_approved`）；文件是项目自带的、或者模型 / 别人改过，Peck 不替模型跑验证、不起 dev server、不开 GOAL，NEW TASK 上换成「还没确认的验证命令」让站主确认。`verify_save` 就是确认。
- **PLAN 不起 dev server**；`autostart` 的 server 在任务前快照之后才起。
- 项目设置里改别的（默认谁来干、权限）不会冲掉 `verify_dismissed` / `verify_approved`（`workspace_update` 保留这两样）。

## 本地验收修了什么

审查（Rust、前端各一个子代理）挑出来、核实过的：

| 哪里 | 原来的问题 | 改成 |
| --- | --- | --- |
| `process.rs`、`tools/bash.rs` | 验证命令作废、任务被打断时直接丢掉 future，只杀了直接子进程，`cargo test` 的 rustc、`npm test` 的 worker 留在后台 | `GroupGuard`：丢掉时整个进程组 SIGKILL；加了测试 |
| `verify.rs`、`run.rs`、`background.rs` | `.peck/verify.toml` 不经站主就自动跑（clone 来的仓库、模型自己写的）；PLAN 也起能写项目的 dev server；server 在快照前起，写的文件算进任务改动 | 只认确认过的那一份（md5）；PLAN 不起；快照后起 |
| `loop.rs` | `bash_background` 起装依赖的命令，「起常驻进程」总是允许过以后就不问装依赖 | 两样分别问 |
| `loop.rs` | 交接摘要写不出来或被截断时，拿一行「改过哪些文件」换掉整个对话 | 对话原样留着、提示一行，下一轮又满就停；摘要的输出上限 4096 → 16000 |
| `compact.rs` | 同一任务接着发时 `ctx-1.log` 被覆盖，前一次的对话指着错的文件 | 已有的不覆盖 |
| `compact.rs` | 清旧思考时连这一回合的也清了（DeepSeek 工具回合要原样传回） | 站主最近一句以后的思考留着 |
| `monitor.rs` | 任务结束时已经跑完的验证命令也记成「停了」 | 先收下结果再停 |
| `background.rs` | dev server 12 小时被杀；DEV 和自动起同时点会起两份；起不来的原因模型读不到 | server 不限时；起之前加锁；原因记进新输出 |
| `loop.rs` | PLAN 的环境块还写着「改了文件 Peck 替你跑验证」 | PLAN 不提验证命令 |
| `MiddleColumn.tsx` | 切项目时验证状态串到另一个项目（CONFIRM 会把 A 猜的写进 B）；DEV 列表也串 | 按项目记；切项目清空 |
| `MiddleColumn.tsx` | @ 弹层开着时 SEND 发不出原话；没有 Esc、方向键；键盘选不了 | ↵ 插选中的、SEND 照原样发、↑↓ 选、Esc 关 |
| `MiddleColumn.tsx` | GOAL 换项目还开着；没有 server 的项目在跑时也每 2.5 秒读后台、窗口藏着也读 | 换项目清掉；只在有 server 且窗口在前时读 |
| `MiddleColumn.tsx`、`projects.rs` | NOT NOW 以后在项目设置里改默认谁来干，猜的验证命令又冒出来 | 后端保留，前端点完重读项目 |
| `AgentView.tsx`、`agentView.ts` | 记录还没收尾就能点「想记住这几条」的 SAVE（报错）；跑时没过的验证不自动展开；任务结束后 dev server 那一行一直转圈 | 收尾后才出；没过就展开；结束后写「起过」 |
| `App.tsx`、`ProjectPage.tsx` | 读事件日志和批记忆撞上时卡片又出来；项目设置页切项目后旧回调写进新项目；读不到 verify 时原文框一直灰着 | 按 `seq` 合并；按项目重建；框能写 |

## 验证（分开记）

- **自动测试**：`pnpm check` 过；全量 `cargo test`（Mac，有沙箱）247 过、0 失败、7 忽略（云端在 Linux 上挂的 8 个这里都过）。
- **`?mock`**（内置浏览器，明暗各一次）：「修好」交回修一次后挂 ✓；「长任务」压缩三步后做完；「反复」同一失败两次停下；上下文小环和 `[ CONTEXT ]` 弹层；notebook 猜的验证命令 CONFIRM / NOT NOW；@ 弹层 ↵、↑↓、Esc、筛选。
- **升级**：正式数据复制一份用 0.6.0 打开：schema 8 → 9，生成 `before-v9` 备份，16 个任务、231 条事件都在。
- **正式包**：`pnpm app:install` 装进 /Applications（0.6.0）。
- **没验的**（站主用的时候留意）：原生 App 里一项都没点；真实 API 的长任务压缩（摘要写得好不好、Anthropic 第三个断点命中缓存）；真实 Rust 项目 check 失败到修好；dev server 退出 Peck 时真的停。这一版没有真跑，账本没变。

## 已知问题（没修，记着）

| 问题 | 建议谁、什么时候 |
| --- | --- |
| CLI 引擎不验证、不自动修（只给 Peck 循环） | 问站主要不要；要的话 Gateway 的 runner 收尾后接 `Monitor`（Antigravity 照旧不进） |
| 验证命令的 HOME 是 Peck 给的，第一次 `cargo check` 会重拉依赖 | 记着；慢的话改成带上站主的 `CARGO_HOME` |
| 收尾验证不重新比文件：最后一轮工具以后 dev server、站主改的文件不算；文件超过 5 万个时只认工具报的 Diff | v0.8 Phase 12 |
| 验证卡片「都过了」那一张可能只列了重跑的几条 | v0.8 Phase 12 |
| `git_summary` 没有超时，很大的仓库可能拖慢开头 | v0.8 Phase 12 |
| ONE SHOT 不带环境块；`?mock` 的上下文大小是编的；后台命令表只在内存 | 记着 |
| 高危规则是黑名单（站主 gitconfig 的别名、项目里的脚本看不出来） | 记着；要更严得换白名单，问站主 |
| 长任务里每批事件整个任务重算一遍 | 云端，v0.8 Phase 12 |
| 规划 + 实施、接力、假 Agent 还在旧代码上 | 云端，v0.8 Phase 11 |
| 删任务会连事件日志一起删，QUOTA 的图和这个月的花费跟着少 | 记着；要留得把花费单独记，问站主 |

## 退回项

（没有）

## 下一步

1. **站主**：提交、推送、打 tag，再删云端分支：
   ```bash
   git add -A && git commit -m "v0.6: context layers, auto-compaction, memory, verify monitor, background jobs and goal mode" && git push && git tag peck-v0.6.0 && git push origin peck-v0.6.0
   git push origin --delete claude/nice-goodall-usgzn5 && git fetch --prune
   ```
2. **云端**：开 Phase 9（会话和项目），先看上面「云端开 v0.7 前要知道的」。

## 视觉基线要补的（本地改方案时做）

方案里没有、云端取了沉静一边的样子（样式都在 `src/app.css`）：用量小环和 `[ CONTEXT ]` 弹层、`[ VERIFY ]` 卡片、「压缩上下文」暗行和交接摘要、`[ MEMORY ]` 卡片、GOAL / DEV 选项片、@ 弹层和正文里的小标记、NEW TASK 上的验证命令、项目设置的 verify.toml 原文框；折叠的工具调用行、思考行、花费行、模型菜单、左栏 AGENTS 和收起的小三角、右栏四个新图标、Markdown 正文、输入框编辑器、`[ RISK ]` 红边、Doto 里中文落到缝合像素、身份色三个新颜色。另外：`bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式；v0.7 要画 ⓘ 图标。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 额度与花费（QUOTA 页）由本地账号做；Peck 做好以后接手站主的其他项目。
- Codex 暂时和 ChatGPT 客户端共用登录（站主 11-12 同意）。Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到（设置 → CLI 检测 → 登录）。
