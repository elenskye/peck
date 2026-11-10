# CLAUDE.md

Peck：只在站主自己的 Mac 上用的本地 Coding Agent（Tauri 2 + React + Rust + SQLite），调本机的 CLI（Muse Code、Grok Build）或 API 模型改代码。站主不是设计师、不是全职开发者，用自然语言指挥 Agent；规则只写在这一个文件，`AGENTS.md` 指向这里。

从 v0.4 起两个 Claude 账号分工：**云端账号**开发，**本地账号**验收（「两个账号」）。合并进 `main`、打 tag 都由站主做。

## 开工前

1. 读本文件、`docs/roadmap.md`（现在在哪个 Phase、定过什么）、`HANDOFF.md`（上一次做到哪、什么没验、有没有退回项）。定过的不重复问。
2. 先认清自己是哪一边：在 Claude Code 网页版里、仓库从 GitHub 克隆来的，是云端账号；在站主的 Mac 上（`~/Documents/GitHub/personal-cli-studio/peck`）是本地账号。
3. 要站主拍板的小事用提问功能问；没人答就按「默认取舍」做，记进 HANDOFF。只有两种事必须停下等站主：视觉方案或原型要批、v1.0 发布。
4. 文档和界面都用中文，专有名词保留英文（Muse、Grok、API、Key、Diff、MCP）。

## 两个账号

| | 云端账号 | 本地账号 |
| --- | --- | --- |
| 是谁 | 另一个 Claude 账号，在 Claude Code 网页版里 | 站主 Mac 上的 Claude Code（现用账号） |
| 做什么 | Phase 表里 v0.4 起的开发：代码、文档、`?mock` 假数据、测试（「云端开发」） | 验收云端的分支、出验收清单页、读回意见（「本地验收」）；视觉（visual-atelier、云端画布）和同步 `src/vendor/peck/` |
| Git | 只往自己的分支提交、推送 | 不提交，改的文件留给站主提交 |

Peck 仓库的版本 tag 叫 `v0.4.0` 这样，由站主打；visual-atelier 里的 `peck-v*` 是视觉方案的版本，是两回事。

## 云端的边界

云端账号只做下面允许的事；没写到的、拿不准的，写进 HANDOFF 交给站主，不自己扩。

- **只在自己的分支上**：每个版本一条分支，名字 `claude/peck-v0.4`、`claude/peck-v0.5`……开工先 `git fetch`：有这条分支就接着它做，没有就从最新的 `origin/main` 切出来；`main` 有新提交（站主合并了上一版、同步了视觉）就把 `main` 合进自己的分支。只提交、推送这一条分支。**不推 `main`、不合并进 `main`、不打 tag、不动别的分支、不开 PR、不 force push。**
- **不建任何云端页面**：验收清单页、问卷、画布、Design System 都不建、不发、不改，不用 claude-artifacts 这类发布工具。验收清单页由本地账号出。
- **只改这个仓库**：visual-atelier、Pomelo、站主的其他仓库都不碰。`src/vendor/peck/` 是同步来的，不改；视觉照它的 `DESIGN.md` 和 `docs/design.md`，要改视觉基线写进 HANDOFF 交给本地。
- **不发真实请求**：不跑真实的 Muse、Grok，不发真实 API 请求，不碰 Key；只用 `?mock`、本地夹具和假 Agent。验收点里要真 CLI、真 API、原生 App 的那一半留给本地验收，HANDOFF 里写清哪些没验。
- **不打包、不安装**：不跑 `pnpm app:install`、`pnpm app:package`。
- 额度监测器的目录不碰（「边界」）；本文件的规则不改，要改写进 HANDOFF 提给站主。
- 新加依赖写成 `^x.y.z`，在 HANDOFF 记一笔。

## 云端开发

云端每个会话照这个走。**状态只在 `docs/roadmap.md` 的 Phase 表「状态」列**（`未开始`、`进行中`、`等站主批`、`完成`），别处不另记。

1. **读、切分支**：读本文件、`docs/roadmap.md`、`HANDOFF.md`；照「云端的边界」切到这个版本的分支，合进 `main` 的新提交。
2. **先修退回项**：HANDOFF「退回项」里没写「已处理」的先修，修完在那一项后面写「已处理」。
3. **找 Phase**：表里 `进行中` 的那一行就是现在做的。没有 `进行中`：上一行是 `完成` 就把下一行改成 `进行中` 开工。当前行是 `等站主批`：不做它，也不做依赖它的后面的 Phase。**上一个版本还没合进 `main` 就不开下一个版本**（比如 `origin/claude/peck-v0.4` 还不是 `origin/main` 的祖先，就不开 v0.5 的 Phase），只修退回项。都没得做就在 HANDOFF 记一行「等站主批 Phase N」或「v0.x 等本地验收」，然后结束，不空转。
4. **做**：一次会话做一段（两小时内能收尾的量），只做这个 Phase 表里写的内容，不扩。验证照「验证」里云端能跑的那些。
5. **收尾**（没做到不算完成）：
   - Phase 做完：验收点里云端能验的都过了，「状态」改 `完成`，下一行改 `进行中`（要站主批的改 `等站主批`）；没验的那一半写进 HANDOFF 留给本地。
   - 一个版本的 Phase 都 `完成`：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的版本号改成那个版本；HANDOFF 写「v0.x 等本地验收」和给本地的验收清单草稿（每一项：验什么、在 App 里怎么做、云端已经验了什么）。不出页、不打包。
   - 更新 `HANDOFF.md`：只留当前状态、这次做了什么、实际验证了什么、没做完的、退回项、下一步；不按轮次追加旧历史。
   - 提交并推送这个版本的分支，提交说明用一句中文说做了什么。
6. **结束时说清楚**：最后一句写「Phase N 继续 / Phase N 完成，下一个 Phase M / 等站主批 Phase N / v0.x 等本地验收（分支 `claude/peck-v0.x`）」四种之一。

## 本地验收

站主说「验收 v0.x」时，本地账号这样做：

1. `git fetch`，在旁边开一个 worktree 检出云端的分支（`git worktree add ../peck-review origin/claude/peck-v0.x`），不动主工作区和站主没提交的改动。
2. 在那里装依赖，全跑「验证」：`pnpm check`、全量 `cargo test`、`?mock` 明暗各看一遍、升级测试；`pnpm app:install` 装进 /Applications（旧包备份）；验收点要的真 CLI 冒烟、真 API 照「额度」。模拟、自动测试、原生交互、真实服务的结果分开记。
3. 照 `docs/acceptance/README.md` 出这个版本的验收清单页（本地账号建，云端账号读不到别的账号的页），链接填进 roadmap「版本与验收清单」。
4. 站主填完，`db_get` 读回：退回的项写进 worktree 里 `HANDOFF.md` 的「退回项」（每项一句：哪里不对、站主的原话）；全部通过就告诉站主可以合并。
5. 本地改的文件（`docs/acceptance/v0.x.html`、roadmap 的链接、HANDOFF）都留在 worktree 里不提交，告诉站主：提交到云端的分支并推送，云端下一个会话就读得到退回项；全部通过后站主把分支合进 `main`、打 tag `v0.x.0`。
6. 本地不修功能上的退回项，交回云端修；只有打包、签名、钥匙串这类只有 Mac 上能做的事本地修，并在 HANDOFF 写明。

## 边界

- 只在 `personal-cli-studio/` 里开发（云端只有 Peck 这一个仓库）。`~/Documents/GitHub/pomelo` 只读。
- visual-atelier（`~/Documents/GitHub/visual-atelier`，只有本地账号碰）：只碰 `schemes/peck/`、`canvases/peck-*/` 和两份索引里 Peck 那一行（动索引前先 `git status`，有别人没提交的改动就不动）；别的地方要站主针对这一次同意。站主打了 `peck-v*` 的 tag 才同步：`VA_BROWSER_CHANNEL=chrome npm run sync -- --scheme peck --to peck --tag peck-v<版本>`。它的规矩读它的 `CLAUDE.md`，命令前先 `source ~/.nvm/nvm.sh && nvm use`（Node 22）。
- **额度监测器是 Peck 自己的作业**：`src/quota/`、`src-tauri/src/quota/` 由站主用 Peck 开发，Agent 不写、不改、不「顺手修」；只读、评估、把 Peck 的改进项记下来。Peck 其他地方给它留的接口（导航入口、数据读取点）可以改。
- 本地账号改任何文件但不提交，保留接手时已有的改动；`git status` 里有你不认识的改动，不动也不还原。
- 不碰 Keychain 里的 Key 内容，不把 Key、绝对路径、提示词写进文档和验收页。不新增付费服务。
- 用户的仓库只读写文件，不动它的 index、commit、stash；任务前快照在应用数据目录的影子仓库里。

## 额度

- 自检优先 `?mock`（浏览器 `http://localhost:1430/?mock`，假数据）和本地夹具，不发真请求。
- 真实 CLI、API 调用只有本地账号在验收时做。
- CLI（Muse、Grok）用站主已登录的订阅额度。每个版本的真实 CLI 调用只做验收点要求的冒烟：最多 3 个任务、每个不超过 12 轮，在 HANDOFF 记次数和结果；失败不静默重试。
- API 的真实请求只用 `docs/roadmap.md`「现行授权」的剩余额度，每个 Phase 不超过 5 次尝试；其他模型没有授权就不发。调用前后把账本写进 roadmap。
- 不用「你好」探针测额度，不用模型 turn 查余额。

## 默认取舍

问了没人答时这样做，并记进 HANDOFF：

- 先保留功能、再收界面：入口一个不少，但从新外壳能到就行，旧组件能删就删。
- 文案照 `docs/design.md` 的用词对照；没对照的按「站主看得懂的中文 + 英文专有名词」写，不写开发者口径（Worker、Run、Step、影子、台账）。
- 看不准的视觉细节照 `src/vendor/peck/DESIGN.md`；没写到的取沉静的那一边（少一层颜色、短一点的动效）。
- 要改数据库结构就加迁移、升版本号、旧库先备份；不改旧记录的含义。
- 任务跑完默认留着，撤销是站主主动点；通知默认开，能关。

## 验证

- 前端：`pnpm check`；改了界面开 `?mock` 看一遍（明暗各一次），截图只放会话临时目录。
- 后端：改到的模块跑 `cargo test --offline --manifest-path src-tauri/Cargo.toml`（`PECK_DATA_DIR`、`PECK_SHADOW_ROOT` 指到临时目录），带 `set -o pipefail`，不用 `| tail` 吞退出码。云端缺 Linux 的系统库编不过时，在 HANDOFF 写明没跑，留给本地。
- 原生（只有本地）：`pnpm app:dev` 或装正式包后在 App 里走一遍验收点；模拟、自动测试、原生交互、真实服务的结果分开记，没验过的不写「通过」。
- 升级（只有本地）：旧数据目录复制一份再用新版打开，数据都在、备份生成。
- 日常只跑和改动相关的最小验证；一个版本收尾才全跑。

## 文档职责

| 文件 | 内容 |
| --- | --- |
| `CLAUDE.md` | 规则（`AGENTS.md` 指向它） |
| `README.md` | 是什么、怎么用、模块表、文档表 |
| `HANDOFF.md` | 当前交接：做到哪、验了什么、没做完的、退回项、下一步 |
| `docs/roadmap.md` | 现在、定位、Phase 表（唯一的状态来源）、验收清单链接、现行授权 |
| `docs/architecture.md` | 选型、数据存放、CLI 和 API 接入层、代码位置 |
| `docs/design.md` | 用词对照、布局、各处的样子（定值在 `src/vendor/peck/`） |
| `docs/testing.md` | 怎么测 |
| `docs/acceptance/` | 验收清单页的模板和出页办法 |

文档只写结论，不写过程、不写验证报告式的流水；旧的阶段总结、历史日志不另建。
