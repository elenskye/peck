# CLAUDE.md

Peck：只为站主、只在站主这台 Mac 上跑的个人 Harness（Tauri 2 + React + Rust + SQLite）：Model Gateway 接各家 API 和 CLI，Peck 自己的循环带工具箱和上下文管理，后台替模型盯验证命令（定位见 `docs/roadmap.md`）。站主不是设计师、不是全职开发者，用自然语言指挥 Agent；规则只写在这一个文件，`AGENTS.md` 指向这里。

两个 Claude 账号分工：**云端账号**做 Phase 表里「谁做」是云端的 Phase，**本地账号**做「谁做」是本地的 Phase（站主指定，比如 v0.5 的 Gateway），并负责验收和合并（「两个账号」）。提交、推送、打 tag 由站主做。

## 开工前

1. 读本文件、`docs/roadmap.md`（现在在哪个 Phase、定过什么）、`HANDOFF.md`（上一次做到哪、什么没验、有没有退回项）。定过的不重复问。
2. 先认清自己是哪一边：在 Claude Code 网页版里、仓库从 GitHub 克隆来的，是云端账号；在站主的 Mac 上（`~/Documents/GitHub/personal-cli-studio/peck`）是本地账号。
3. 要站主拍板的小事用提问功能问；没人答就按「默认取舍」做，记进 HANDOFF。只有两种事必须停下等站主：视觉方案或原型要批、v1.0 发布。
4. 文档和界面都用中文，专有名词保留英文（Muse、Grok、API、Key、Diff、MCP）。

## 两个账号

| | 云端账号 | 本地账号 |
| --- | --- | --- |
| 是谁 | 另一个 Claude 账号，在 Claude Code 网页版里 | 站主 Mac 上的 Claude Code（现用账号） |
| 做什么 | 「谁做」是云端的 Phase：代码、文档、`?mock` 假数据、测试（「云端开发」） | 「谁做」是本地的 Phase（「本地开发」）；验收云端的分支、合进 `main`、出验收清单页、读回意见（「本地验收」）；真实 CLI 和 API 调用；视觉（visual-atelier、云端画布）和同步 `src/vendor/peck/` |
| Git | 只往这个版本的云端分支提交、只推这一条分支 | 在 `main` 上改、不提交、不推送；「本地验收」时把云端分支压成一份改动（`git merge --squash`）放进暂存区，由站主提交 |

Peck 仓库的版本 tag 照站主打的样子叫 `peck-v0.4.0`；visual-atelier 里的 `peck-v0.1.0` 这类是视觉方案的版本，在另一个仓库，是两回事。

**提交规范**（站主 2026-11-12 定，起因是 v0.4 有三次 Claude 署名的提交被快进进了 `main`）：

- `main` 上只有站主的提交。两个账号都不在 `main` 上提交、不推 `main`、不打 tag、不 force push；云端的提交只留在它自己的分支上，合进 `main` 一律压成一份改动、由站主提交，从不快进、不留合并提交。
- 说明照站主的写法：`v<版本>: <一句小写英文>`，比如 `v0.5: gateway contract, fake gateway and fixtures`。不写中文、不用全角冒号、不加 `Co-Authored-By` 之类的尾注。云端在自己分支上的提交也照这个写。
- 到了要提交的节点，本地把起草好的说明给站主（对话里和 HANDOFF「下一步」），站主自己提交、推送。

**文件归属**：同一个版本两边并行时（比如 v0.5：本地做 Gateway，云端做工具箱和循环），版本计划（`docs/v0.x.md`）里有「文件归属」表，各改各的；两边都要动的登记点（命令注册、`ipc.ts`、类型、`Cargo.toml`）只追加、不改别人写的，合并时本地解决冲突。

## 云端的边界

云端账号只做下面允许的事；没写到的、拿不准的，写进 HANDOFF 交给站主，不自己扩。

- **只在自己的分支上**：每个版本一条分支，想叫 `claude/peck-v0.x`；会话环境只许推它自己定的名字（比如 v0.6 的 `claude/nice-goodall-usgzn5`）时就用那个，并在 HANDOFF 开头写明分支名。开工先 `git fetch`：HANDOFF 里写着这个版本的分支就接着它做，没有就从最新的 `origin/main` 切出来；`main` 有新提交（站主合并了上一版、本地开发的 Phase 推上来了、同步了视觉）就把 `main` 合进自己的分支。只提交、推送这一条分支，提交说明照「提交规范」。**不推 `main`、不合并进 `main`、不打 tag、不动别的分支、不开 PR、不 force push。**
- **只做「谁做」是云端的 Phase**；本地正在做的 Phase 的文件（版本计划的「文件归属」）不改，要它改什么写进 HANDOFF。
- **不建任何云端页面**：验收清单页、问卷、画布、Design System 都不建、不发、不改，不用 claude-artifacts 这类发布工具。验收清单页由本地账号出。
- **只改这个仓库**：visual-atelier、Pomelo、站主的其他仓库都不碰。`src/vendor/peck/` 是同步来的，不改；视觉照它的 `DESIGN.md` 和 `docs/design.md`，要改视觉基线写进 HANDOFF 交给本地。
- **不发真实请求**：不跑真实的 CLI（Claude Code、Codex、Muse、Grok、Antigravity），不发真实 API 请求，不碰 Key 和登录凭据；只用 `?mock`、仓库里的夹具（`src-tauri/tests/fixtures/`）和假 Agent、假 Gateway。验收点里要真 CLI、真 API、原生 App 的那一半留给本地，HANDOFF 里写清哪些没验。
- **Antigravity 只做直接调用**：站主选它、发一句话、它自己做完。不把它放进接力、委派、交叉审查、定时任务、目标模式、自动续跑这类自动化（站主 2026-11-12 定）。
- **不打包、不安装**：不跑 `pnpm app:install`、`pnpm app:package`。
- 本文件的规则不改，要改写进 HANDOFF 提给站主。
- 新加依赖写成 `^x.y.z`，在 HANDOFF 记一笔。

## 云端开发

云端每个会话照这个走。**状态只在 `docs/roadmap.md` 的 Phase 表「状态」列**（`未开始`、`进行中`、`等站主批`、`完成`），别处不另记。

1. **读、切分支**：读本文件、`docs/roadmap.md`、`HANDOFF.md`；照「云端的边界」切到这个版本的分支，合进 `main` 的新提交。
2. **先修退回项**：HANDOFF「退回项」里没写「已处理」的先修，修完在那一项后面写「已处理」。
3. **找 Phase**：只看「谁做」是云端的行。其中 `进行中` 的那一行就是现在做的。没有 `进行中`：上一个云端行是 `完成` 就把下一个云端行改成 `进行中` 开工。当前行是 `等站主批`：不做它，也不做依赖它的后面的 Phase（比如要等本地开发的契约推到 `main` 才能开工的，站主或本地会把它改成 `进行中`）。**上一个版本还没合进 `main` 就不开下一个版本**：合并是压成一次提交，所以看上一版的云端分支（名字在 HANDOFF）还在不在 GitHub 上（站主合并、推送后会删掉它），还在就不开下一版的 Phase，只修退回项。都没得做就在 HANDOFF 记一行「等站主批 Phase N」或「v0.x 等本地验收」，然后结束，不空转。
4. **做**：一次会话做一段（两小时内能收尾的量），只做这个 Phase 表里写的内容，不扩。验证照「验证」里云端能跑的那些。
5. **收尾**（没做到不算完成）：
   - Phase 做完：验收点里云端能验的都过了，「状态」改 `完成`，下一行改 `进行中`（要站主批的改 `等站主批`）；没验的那一半写进 HANDOFF 留给本地。
   - 一个版本的 Phase 都 `完成`：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的版本号改成那个版本；HANDOFF 写「v0.x 等本地验收」和给本地的验收清单草稿（每一项：验什么、在 App 里怎么做、云端已经验了什么）。不出页、不打包。
   - 更新 `HANDOFF.md`：只留当前状态、这次做了什么、实际验证了什么、没做完的、退回项、下一步；不按轮次追加旧历史。
   - 提交并推送这个版本的分支，提交说明照「提交规范」。
6. **结束时说清楚**：最后一句写「Phase N 继续 / Phase N 完成，下一个 Phase M / 等站主批 Phase N / v0.x 等本地验收（分支 `claude/peck-v0.x`）」四种之一。

## 本地开发

「谁做」是本地的 Phase（站主指定），本地账号在 `main` 上照「云端开发」的 2–6 步做，差别只有：

- 直接在主工作区的 `main` 上改，**不提交**；到了版本计划里写的节点（比如 v0.5 的「契约」做完），把起草好的提交说明给站主，站主提交、推送，云端才接得上。
- 真实 CLI、API 可以用，但只用「额度」里的授权，账本写进 roadmap。
- 本地开发的 Phase 也要过验收：和云端的 Phase 一起出在这个版本的验收清单里。

## 本地验收

站主说「验收 v0.x」时，本地账号这样做：

1. **先合并**：`main` 上有没提交的改动，先请站主提交（合并要干净的工作区）。`git fetch`，`git merge --squash origin/<云端分支>`（名字在云端写的 HANDOFF 开头）：云端分支的全部改动压成一份放进暂存区，**不提交**；冲突本地解决（按「文件归属」判断该留哪边）。下面的验收都在这份没提交的改动上做。
2. **先审查**：两个子代理分头审云端的改动（Rust、前端各一个，只读），真缺陷本地修、写进 HANDOFF；要改设计或扩功能的不修，记进「已知问题」。
3. 在 `main` 上装依赖，全跑「验证」：`pnpm check`、全量 `cargo test`、`?mock` 明暗各看一遍、升级测试；`pnpm app:install` 装进 /Applications（旧包备份）；验收点要的真 CLI 冒烟、真 API 照「额度」。模拟、自动测试、原生交互、真实服务的结果分开记。
4. 照 `docs/acceptance/README.md` 出这个版本的验收清单页（本地账号建，云端账号读不到别的账号的页），链接填进 roadmap「版本与验收清单」。站主说直接收（不出页）就跳过 4、5，没验的写进 HANDOFF。
5. 站主填完，`db_get` 读回：退回的项写进 `HANDOFF.md` 的「退回项」（每项一句：哪里不对、站主的原话、该哪边修）。
6. 把起草好的提交说明（照「提交规范」）给站主，站主提交、推送 `main`：云端下一个会话合进 `main`、读到退回项再修；全部通过后站主打 tag `peck-v0.x.0`、推送，再删掉云端分支（`git push origin --delete <云端分支>`；站主让本地清理时本地删）。
7. 云端开发的 Phase 的功能退回项交回云端修；本地开发的 Phase 的退回项、打包、签名、钥匙串这类只有 Mac 上能做的事本地修，并在 HANDOFF 写明。

## 边界

- 只在 `personal-cli-studio/` 里开发（云端只有 Peck 这一个仓库）。`~/Documents/GitHub/pomelo` 只读。
- visual-atelier（`~/Documents/GitHub/visual-atelier`，只有本地账号碰）：只碰 `schemes/peck/`、`canvases/peck-*/` 和两份索引里 Peck 那一行（动索引前先 `git status`，有别人没提交的改动就不动）；别的地方要站主针对这一次同意。站主打了 `peck-v*` 的 tag 才同步：`VA_BROWSER_CHANNEL=chrome npm run sync -- --scheme peck --to peck --tag peck-v<版本>`。它的规矩读它的 `CLAUDE.md`，命令前先 `source ~/.nvm/nvm.sh && nvm use`（Node 22）。
- **额度与花费（QUOTA 页）**：站主 2026-11-13 改了主意，不再留作 Peck 自己的作业，由本地账号接着做（`usage.rs`、`spend.rs`、`UsagePage.tsx`），云端照常能改。Peck 做好以后接手站主的其他项目。
- 本地账号改任何文件但不提交（「本地验收」第 1 步的合并除外），保留接手时已有的改动；`git status` 里有你不认识的改动，不动也不还原。
- 不碰 Keychain 里的 Key 内容，不把 Key、绝对路径、提示词写进文档和验收页。不新增付费服务。
- 用户的仓库只读写文件，不动它的 index、commit、stash；任务前快照在应用数据目录的影子仓库里。

## 额度

- 自检优先 `?mock`（浏览器 `http://localhost:1430/?mock`，假数据）和本地夹具，不发真请求。
- 真实 CLI、API 调用只有本地账号做：本地开发的 Phase 里，和每个版本的本地验收。
- 上限照 `docs/roadmap.md`「现行授权」：每个版本每家 API 最多 5 次尝试，每家 CLI 最多 3 个小任务（每个不超过 12 轮）；用完先停下问站主，失败不静默重试。调用前后把账本写进 roadmap，CLI 的次数和结果记进 HANDOFF。
- **省额度**（站主 2026-11-12 定）：真跑一律用这家最便宜、又测得到要测功能的模型（Claude Code `--model haiku`，Codex、Grok、Muse 选它们最便宜的那档），做无关紧要的小任务（读一个文件、改一行、跑一条命令）。测试用的小项目统一放仓库外的 `personal-cli-studio/projects/`（现在有 `projects/gateway-smoke`），不在 Peck 仓库里、也不另建零散目录。
- CLI 都装在仓库外的 `personal-cli-studio/cli/`（`cli/bin/` 是统一入口，来源和更新见 `cli/README.md`）。
- CLI 用站主已登录的订阅；Claude Code 用站主登录的、未改动的官方程序，Peck 不读、不复用任何 CLI 的登录凭据。夹具录下来以后去掉 Key、账号、绝对路径再进仓库。
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
| `docs/v0.x.md` | 这个版本的开发计划：范围、分工和文件归属、设计、验收点（版本做完以后结论并进 `architecture.md`、`design.md`，这份删掉） |
| `docs/architecture.md` | 选型、数据存放、CLI 和 API 接入层、代码位置 |
| `docs/design.md` | 用词对照、布局、各处的样子（定值在 `src/vendor/peck/`） |
| `docs/testing.md` | 怎么测 |
| `docs/acceptance/` | 验收清单页的模板、出页办法、当前版本的页（上一版的源文件删掉，页面链接在 roadmap） |

文档只写结论，不写过程、不写验证报告式的流水；旧的阶段总结、历史日志不另建。
