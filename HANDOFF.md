# 当前交接

更新：2026-11-27。正式版 **v1.2.1**（已装、已推）。**v1.3.0 在做**（Phase 0、1 做完）：PLOOP 评测基线，Phase 表在 [docs/roadmap.md](./docs/roadmap.md#phase)，各 Phase 状态只看那张表。

## 这一版（1.3.0，进行中）

- **Phase 0 交接摘要**：写摘要的要求改成固定五栏（`## 目标` `## 已定的决策` `## 踩过的坑` `## 文件现在的状态` `## 下一步`，栏名照写、内容用站主的语言）；站主说过的话（含 ASK 的回答）压几次都带着（见 architecture「上下文」）。测试：`harness::compact` 10 个、`harness::r#loop` 44 个过。**没验的**：真模型照不照五栏写。
- 需求原文是站主桌面的 `brief.md`（不在仓库）。要点：Phase 表 + 下面「Phase 1 怎么做」+「评测要守的」。

### Phase 1 框架（做完）

用法、题目格式、结果在哪都在 [evals/README.md](./evals/README.md)；代码位置见 architecture「代码位置」。

- **一次尝试**：`peck eval <attempt.json>`（`src-tauri/src/eval.rs`，`lib.rs` 的 `run()` 起 Tauri 前看参数）：临时数据目录里建 `AppState`、存臂的 `ui`、打开项目、`project_plugins::prepare`（认结构、装 Git Plan）、把这些记成一笔（判分的起点）、新建任务、`start_peck_run`；审批卡片一律拒、ASK 回固定的话、都计数；到时间打断（`timeout`）；导出 `events.jsonl` 和 `outcome.json`。真跑只用内存里的 Key（从 Key 目录按文件名读）。不用 `#[test]` 入口：测试构建里一律 Mock。
- **一组**：`pnpm eval run | check | report`（`evals/run.ts`，Node 24 直接跑 TS，没加依赖；读 TOML 是自己写的一小段，只认 README 里的写法）。
- 验过的：`smoke` 组（2 题 × 2 臂 × 2 次，假模型照参考答案改 / 什么都不改）11 秒跑完，参考答案 4/4 过、空改动 0/4；12 次的组跑到第 4 次时杀掉，重跑跳过做完的 3 次、被杀的那次重做；卡片（`git push` 高危被拒）、ASK 自动回答、到时间打断都单独跑过；`git archive` 和带历史的两种工作区里都看不到起点以后的提交、没有远端和 reflog；`pnpm eval check` 两题都合格；`node --test evals/test/*.test.ts` 5 个、`eval::` 2 个过。
- **没验的**：真 API 的臂（Key 目录、`PECK_API_MODE=real`、子代理）、并发大于 2、infra 重跑、花到预算停下、噪声底和 `--baseline`（逻辑有单元测试，没有真数据）、预热缓存的拷贝（`projects/eval/cache/` 还是空的）。
- 假模型剧本每一步都要带 `usage`：Peck Loop 碰到没报用量的回答会停下（本来就这样）。

### Phase 2 要知道的

- 挖题照 brief：一笔真实提交 → 起点是它的上一笔 → 站主当时的原话（正式库任务记录里找，只读；拷一份 `cp -c` 再用 `sqlite3 -readonly` 读）→ 那笔提交里的测试当隐藏测试。优先 v1.1.x–v1.2.x 真跑时踩过的坑：无头 Chrome 连弹钥匙串框、全量测试在沙箱里卡死、删 `mktemp` 临时目录被当成高危、CLI 的后台进程收不干净、旧的界面设置迁移不能删、托盘在后台线程上被碰导致崩溃。
- Peck 的题要预热缓存：`projects/eval/cache/peck/src-tauri/target`、`…/node_modules`（在起点附近编一次）。
- 换写法的解用 Codex 写（站主批了，每题 1 个小任务，我审过再用）。

### 评测要守的

- 评测代码不改发任务、权限、沙箱的正常路径；正式版界面和行为不变。
- 每次尝试全新：数据目录、影子仓库、工作区、记忆、设置、对话都是空的；工作区里不能有起点以后的提交（远端、reflog、stash 也不行）。Pomelo、visual-atelier 只 `git archive` / `git clone` 拷出来用。
- 给模型的那句话前面不加别的话；ASK 自动回「评测中：按你的判断做，不要再问」，卡片和 ASK 各记次数。
- Key、账号不进任何结果文件；trace 有代码和提示词，只放 `projects/eval/`，不进 Peck 仓库。
- 看结果不看过程，代码判分优先（模型判分只给代码判不了的，先和人判的对一遍）；失败的那几次要逐个读过程，确认是模型没做对还是题坏了，没读过的分数不算数。
- 报 pass@1（带区间）和 pass^3、按题配对比较；先测噪声底。
- 总预算估法：最便宜的主角色跑 1–2 题量单次花费 × 题数 × 次数 × 臂数；用完就停；Opus 只放一个臂（批和记账见 roadmap「现行授权」）。
- 收尾告诉站主：报告路径、一句话结论、一共花了多少钱、提交列表。

## 没验的

- 正式版窗口里没点：Git Plan 多仓库的两行 recap、AUTO 下提交不弹卡片、WebKit 渲染；真中文输入法下的 `/` 弹层和占位、从访达拖图、`+` 的系统选窗。
- 真模型：PLOOP 在 AUTO 下自己 `git commit`、Git Plan 多仓库的问法、CLI 的多仓库提交。
- 进程清理只在 Claude Code 上真跑过，Codex、Muse、Grok 没有。
- Gemini 原生搜索、Vertex、百炼新加坡、Mathpix、通知横幅。

## 留给站主的

- **push** 你做：v1.3.0 的提交都还没推。
- **Git Plan UPDATE**：装着 1.1.0 的项目在项目设置 → PLUGINS 点 UPDATE 才换成新的 Skill 和问法；不点也能用。
- 任务 194 的记忆卡片（「Peck 给每条命令的 `$TMPDIR` 跑完就删」）还等你 SAVE / SKIP（没再查过）。
- **废纸篓**：装包换下来的旧 `Peck.app`，确认不要了自己清空。
- 仓库里没跟踪的 `.agents/skills`（Peck 认结构建的链接）：要不要提交你定。
- **Vertex**：要用先在终端跑 `gcloud auth application-default login`。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| CLI 跑在它自己的沙箱里：Codex 的沙箱里 WebKit 起不来，截图、渲染要出沙箱，每次弹卡片 | 让它一个脚本截完 |
| Muse、Grok 读不到订阅余量，用完了只能等 429 | 记着 |
| 句中 `/` 按标题包含筛：空格后打路径（` /src`）时标题里正好有这几个字会弹，这时 ↵ 是插提示词 | Esc 关掉再 ↵ |
| 占位的跳法和撤销（见 design「插提示词」） | 用一阵看要不要 |
| App 一直开着、额度快照不变时，过了重置时间的提醒要等快照更新才消失 | 记着 |
| 关了轮数上限，一个任务（定时任务也是）能一直跑到每月花费上限才停 | 长任务前看一眼花费上限 |
| 在 Peck 里开发 Peck 时装不了正式版（换包会把自己换掉、签名要钥匙串） | 装包留给站主或 Claude Code，攒一批再装 |
| 真用完额度的 429 也会等满 5 次重发（约 2 分钟）才停 | 用一阵看要不要分开「限流」和「额度用完」 |
| 高危放宽后，项目里被 .gitignore 忽略的文件（`.env`、`node_modules`）用 `rm` 删了不问，任务前快照里也没有；`git clean -x` 照问 | 用一阵看要不要把删 `.env` 这类单独算高危 |
| 读文件的规则只按字面路径：临时目录里一个指向家目录的链接也算临时目录（EDIT / AUTO 下 `cat ~/...` 本来就不问，不算放宽） | 记着 |
| 命令的说明是模型写的，可能和命令对不上；CLI 里只有 Claude Code 带说明，别家卡片标题是命令第一行 | 卡片上点「看命令」看原文 |
| TRUST 卡片、Hook 卡片、没批过的命令：定时任务碰到会一直等 | 先手动发一次、点掉 |
| auditor 只读但不限在项目里，一组能跑十几轮 | 用一阵看要不要限死项目、少给轮数 |
| CLI 的花费是按价格表估的 API 价，不是订阅真花的钱；价格表里没有的模型记成未知（REFRESH 只刷新已有的，不加新模型） | 新模型出来补进 `defaults/prices.json` |
| 旧表 `api_calls`、`api_tasks`、`usage_calls`、`messages` 还在库里（几十行），删任务时那几行清理代码得留着（外键） | 要清就加一条迁移把四张表删掉 |
| 划文件只管文件工具和命令的沙箱：MCP 工具、主角色自己的命令管不到；`sed -i` 原地改划给它的单个文件会被沙箱拦 | 划文件夹，或用文件工具改 |
| 原地打转只认一模一样的调用和结果 | 用一阵再看 |
| Vertex 上的 Claude / Gemini 只有离线测试；Vertex 上的 Claude 没带 `block_binding` | 开了 GCP 项目再试 |
| 小窗打开的 0.2 秒动画里点下去会落到外面那层、把小窗关掉 | 记着 |
| 任务结束时认不出的 CLI 子进程：自己 `setsid` 当组长、又是系统自带程序（读不到记号）、父进程先退了的；被丢下的但工作目录不在 CLI 工作目录里的 | 真碰到再看 |
| 被丢下的进程按工作目录认：站主同时在别的终端里、同一个项目文件夹下用 `sh -c '… &'` 起的后台进程，任务结束时也会被停 | 记着 |
| 仓库的 Git 规则只认标题带 Git、commit、提交的那几节；写在别的标题下的认不到，就照 Git Plan | 规则放进这样的标题下 |
| AUTO 的提交沙箱放开了 `.git`（`config`、`hooks`、`info` 除外）：提交时仓库自己的 pre-commit 钩子也能写 `.git` | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步。按位置：

- **顶栏和左栏**：收起左右栏的按钮和图标（`.top-side`、`.pk-i--side-*`）、`[ NOW ]`、PROJECTS 头上的钟和放大图标、全部项目页、归档图标和归档条、菜单栏「在跑 N 个」。
- **中栏**：任务头的轮数和子代理小片（`.task-round--warn`、`.task-agents`）、「Thinking…」行、recap（含收尾提交那一行、按段花费表）、任务板（`.board-*`）、独立工作区条、`[ MISSED ]` / `[ TRUST ]` / `[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、审批卡片的原因标签和「看命令」（`.agent-card-reason`、`.out-cmd`）、ASK 的页签和 Other（`.ask-tab`、`.ask-q`、`.agent-other`）、YOU 下面的小图小片和追加的那一行（`.you-*`）、附图放大（`.lightbox`）、`/compact` 那一行、读网页的网址行（`.web-*`）、中栏 15px 正文和 Body 字体。
- **输入框**：`/` 和 `@` 弹层（含 `/ 提示词` 一组 `.slash-row`）、没填的占位（`.cm-slot`）、↵、不带箭头的 `+`、`+` 附的小片（`.composer-upload`）、拖放虚线（`.composer.is-drop`）、上下文环和额度环（`.quota-ring`、`.is-stale`）、排着的话的小片。
- **右栏**：PROMPT 页签图标（`.pk-i--prompt`，占位）、提示词列表和小窗（`.prompt-*`、`.prompt-form`）、TERM 头上的 dev server 小片、FILES 的 ALL 和变暗的行（`.pk-row--dim`）。
- **设置和 QUOTA**：统一排法（`.set-scope`、`.set-line`、`.set-head`、`.field-unit`）、小窗（`.modal`）、插件卡片和像素图标（`.plug-*`）、记忆小窗（`.memo-*`）、MCP、AGENTS 和 SUBAGENTS 几块、进度条、额度页的环和读数（`.quota-rows`）、ⓘ 图标（占位）和弹层、代码配色 `--code-*`。

## 留意

- **visual-atelier 的权限**（2026-11-27 定）：这边的会话对 Atelier 有全部权限（本体也能直接改、照那边的规矩提交和打 tag），用到前先读它的 `CLAUDE.md`，只有改别的项目的方案、给 Atelier 加新功能先问，见 `CLAUDE.md`「边界」。
- **改用到的引擎**（2026-11-27 定）：Peck 用到的引擎（含间接用到的）可以直接改、照那边的规矩提交和打 tag，见 `CLAUDE.md`「边界」和总仓库 `personal-engine-studio/docs/decisions/0009`。
- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- 测试项目 `projects/tester`、`moon-lab`、`gateway-smoke`、`git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 联网搜索的协议各家常变：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

v1.3.0 Phase 2 题库（站主说开工再做）。
