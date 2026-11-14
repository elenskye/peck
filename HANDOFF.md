# 当前交接

更新：2026-11-14（本地账号）。**v0.7 本地验收做完，等站主提交**：云端分支 `claude/nice-goodall-usgzn5`（Phase 9 会话和项目、Phase 10 审阅和预览）压成一份改动合进了 `main`，本地审查修了 34 处（合成下面 15 行），正式包 0.7.0 装进了 /Applications。这一版没出验收页，没验的列在「验证」。规则在 [CLAUDE.md](./CLAUDE.md)，Phase 表在 [docs/roadmap.md](./docs/roadmap.md)，结论在 [architecture.md](./docs/architecture.md)（「项目即环境」「独立工作区」「审阅和预览」）、[design.md](./docs/design.md)。

## 云端开 v0.8 前要知道的

- **同时跑**：两个任务要写的文件夹（项目文件夹 + 能改的其他文件夹）一样或套着就算同一个项目，不让同时跑（`ActiveRunContext::touches`，`ActiveRunContext` 多了 `writable`）。新加的发任务入口照这个查。
- **独立工作区**：git 调用一律 `-c core.hooksPath=/dev/null`；任务里其他文件夹只读；APPLY 前后各拍主文件夹一份快照，撤销回到搬之前；同一个任务上一次的独立工作区没搬回、没丢掉时不能接着发（`worktree::pending_in_session`）；DISCARD 后记录换成主文件夹，历史 Diff 照样能看。
- **其他文件夹**：`workspace::folder_allowed` 挡住家目录和它的上层、Peck 数据目录、`.git` 里面的、彼此套着的；沙箱明着不让写只读的，项目和能改的文件夹下面任何一层 `.git` 都不让写（SBPL 正则）。
- **测试**：会发任务的测试先 `create_run` 占一个大号（testing.md 写了为什么），不然和 `api::worker` 撞号。

## 本地验收修了什么

审查（Rust、前端各一个子代理）挑出来、核实过的：

| 哪里 | 原来的问题 | 改成 |
| --- | --- | --- |
| `harness/run.rs`（测试） | 两个新测试和 `api::worker` 的测试撞任务号（都是 `run-105`），并行时 5 个失败 | 先占一个大号 |
| `harness/run.rs` | 独立工作区的任务照样真改「能改」的其他文件夹，DISCARD 以后撤不回来 | 独立工作区里其他文件夹只读 |
| `worktree.rs` | APPLY ANYWAY 覆盖站主冲突的改动，没有备份，撤销也回不来 | 搬之前、之后各拍一份，撤销回到搬之前；加了测试 |
| `worktree.rs` | `git worktree add` 会在沙箱外跑仓库的 post-checkout 钩子（husky 的钩子是模型能改的项目文件） | 所有 git 调用关掉钩子 |
| `run.rs`、`commands.rs`、`runner.rs`、`worker.rs` | 「一个项目一次一个」只比路径相等：套着的项目、共用的其他文件夹能同时跑，共用影子仓库、互相算改动 | 按写的文件夹重叠判断 |
| `worktree.rs` | APPLY 丢可执行位；中途出错搬一半；CRLF / LFS 文件总报冲突；建到一半失败留下登记；删不掉时 `prune` 会清掉站主自己的 worktree；DISCARD 后 Diff 打不开；接着发的话看不到上一次的改动 | 复制带权限、先查后写、按 git 口径比；失败就删；只删自己那条登记；记录换主文件夹；先 APPLY / DISCARD 才能接着发 |
| `snapshot.rs` | 拒一块会把 CRLF 文件整份改成 LF；任务删的文件站主又建了，拒掉会把旧内容插进去 | 换行照原来；重建的不动；加了测试 |
| `workspace.rs`、`tools/mod.rs`、`tools/bash.rs` | 其他文件夹能加 `/Users`、`~/Library`（含 Peck 的影子仓库和数据库）、`.git` 里面的、彼此套着的；`~/code` 这类文件夹里各仓库的 `.git/hooks` 能写；只读文件夹在 `/private/tmp` 下照样能写 | 校验挡住、每次用前再查；沙箱加正则和只读 deny；Mac 上真跑了测试 |
| `background.rs`、`loop.rs` | 模型起的后台命令不认其他文件夹 | 和 `bash_run` 一样 |
| `files.rs`、`tray.rs` | `qlmanage -t` 卡住就一直占着线程；`qlmanage -p` 留僵尸进程；菜单栏提示在跑的数掉下来以后不更新 | 20 秒超时；等它退出；每次重写提示 |
| `agentView.ts`、`AgentView.tsx`、`MiddleColumn.tsx` | 「→ 下一步」在 ONE SHOT、旧的轮、CLI 引擎的回答里被藏掉，第 4 条以后丢了；点下一步冲掉输入框里写了一半的话；没读到事件日志时回答哪都看不到 | 只藏最新一轮的 Peck 循环、最多 3 条；接在后面；recap 里照写 |
| `App.tsx` | NOW 的「看过」只留 300 条，挤掉的旧任务又冒出来；每批事件把所有在跑任务的事件整个折一遍 | 抬高 `seenSince`；只扫审批和提问 |
| `RightColumn.tsx` | 独立工作区没搬回时 FILES 在主文件夹上标那个任务的改动；html 预览能从网上取东西、`meta refresh` 跳走；任务一动就重读图片、PDF、重起 Quick Look；块状态无限增长、删任务批注不清；换任务时露一下旧的批注数 | 不标、写一句；加 CSP、去掉 refresh；只重读任务改到的文件；最多 2000 条、删任务清掉；换 key 当场读 |
| `ui.tsx` | ⓘ 读屏读不到内容；悬停打开的 Esc 关不掉；弹层里的字选不了、点穿 | `aria-describedby`、Esc 都能关、弹层能悬停 |
| `ProjectPage.tsx`、`highlight.ts` | 选文件夹出错什么都不说；语言包加载失败一次以后一直不上色 | 报出来；下次再试 |

## 验证（分开记）

- **自动测试**：`pnpm check` 过；全量 `cargo test`（Mac，有沙箱）261 过、0 失败、7 忽略（撞号修好后连跑两次都过，改完全部再跑一次也过）。
- **`?mock`**（内置浏览器，明暗各一次）：NOW 块；WORKTREE 发任务 → recap（下一步按钮、「在独立工作区，还没搬回」）→ APPLY 搬回；FILES 的 html（沙箱、渲染正常）、`.rs` 上色；项目设置的 FOLDERS、ENV、ⓘ。前端修完再看一遍：回答里不显示 `→` 行、点下一步接在草稿后面、ⓘ 读屏能读到、Esc 能关。
- **升级**：正式数据（还在 schema 8）复制一份用 0.7.0 打开：升到 9，生成 `before-v9` 备份，16 个任务、231 条事件都在。
- **正式包**：`pnpm app:install` 装进 /Applications（0.7.0）。
- **没验的**（站主用的时候留意）：原生 App 里一项都没点；真 git 项目上 worktree 的建、搬回、丢掉（只有临时仓库的测试）；Quick Look 的缩略图和窗口、WKWebView 里的 PDF 框、选文件夹的系统窗；菜单栏的数字；两个真任务同时跑。没有真跑，账本没变。

## 已知问题（没修，记着）

| 问题 | 建议谁、什么时候 |
| --- | --- |
| CLI 引擎不认项目的其他文件夹、不带项目环境变量 | 问站主要不要；Gateway 起引擎时带 env 和 `--add-dir` 这类参数（各家不一样） |
| 独立工作区在用户仓库 `.git/worktrees/` 里登记一条（站主点了 WORKTREE 才有，搬回、丢掉时删）；App 中途退出留下的不会自己清；删任务、删项目不清没搬回的独立工作区 | 站主看行不行；v0.8 Phase 12 打开 App 时清 |
| 项目环境变量：`PATH=...:$PATH` 不展开（整个换掉）；`GIT_PAGER`、`NO_COLOR`、`CARGO_HOME` 这几个 Peck 自己会盖掉；值明文存在数据库里 | 记着；要放 Key 的话放钥匙串，问站主 |
| 块的状态、批注只在这台机器的浏览器存储里 | 记着；要留就进数据库，问站主 |
| SPLIT 的 Diff 没有 ACCEPT / REJECT 和批注 | v0.8 Phase 12 |
| 图片、PDF 最多 24MB，整份经 IPC 传 | 记着 |
| v0.6 记着的那些（CLI 不验证、验证的 HOME、收尾不重新比文件、`git_summary` 没超时、高危规则是黑名单、每批事件重算、旧代码……） | 见 v0.8 Phase 11、12 |

## 退回项

（没有）

## 下一步

1. **站主**：提交、推送、打 tag，再删云端分支：
   ```bash
   git add -A && git commit -m "v0.7: inbox, project folders and env, recap, previews, hunk review and worktrees" && git push && git tag peck-v0.7.0 && git push origin peck-v0.7.0
   git push origin --delete claude/nice-goodall-usgzn5 && git fetch --prune
   ```
2. **站主**：照 roadmap，v0.7 以后开始用 Peck 开发 Peck。
3. **云端**：开 v0.8 Phase 11（多引擎合作），先看上面「云端开 v0.8 前要知道的」。

## 视觉基线要补的（本地改方案时做）

方案里没有、云端取了沉静一边的样子（样式都在 `src/app.css`）：ⓘ 图标（现在是占位，要站主批）和弹层、`[ NOW ]` 和四种状态、recap 和下一步按钮、FOLDERS 行和 READ / WRITE、ENV 原文框、WORKTREE 选项片、代码配色 `--code-*`、图片棋盘格底和 PDF 框、Quick Look 缩略图、Diff 块头的 REJECT / ACCEPT 和划掉的块、批注、搬回条、菜单栏「在跑 N 个」的图（现在是字）；用量小环和 `[ CONTEXT ]`、`[ VERIFY ]`、`[ MEMORY ]`、压缩上下文、GOAL / DEV、@ 弹层、NEW TASK 的验证命令、verify.toml 原文框；折叠的工具调用行、思考行、花费行、模型菜单、左栏 AGENTS、右栏四个图标、Markdown 正文、输入框编辑器、`[ RISK ]` 红边、Doto 里的中文、身份色三个新颜色。另外：`bundle.css` 的露出动画会盖掉像素角；提示条换大一号像素角；菜单栏图标一格 1.5pt；在跑时权限那一格的选中样式。

## 留意

- 调研全文在 visual-atelier 的 `canvases/peck-harness-questionnaire/research.md`。
- 额度与花费（QUOTA 页）由本地账号做；Peck 做好以后接手站主的其他项目。
- Codex 暂时和 ChatGPT 客户端共用登录（站主 11-12 同意）。Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到（设置 → CLI 检测 → 登录）。
