# 当前交接

更新：2026-11-24。**v1.1.4 做完了，已装进 /Applications**。这是逐步优化 Peck Loop 的第四轮，站主写明了版本号、不分 Phase。规则在 [CLAUDE.md](./CLAUDE.md)，设计在 [docs/architecture.md](./docs/architecture.md)「脚本 Hook」「工具」「外部 MCP 服务器」「Git Plan」，定位和之后要做的在 [docs/roadmap.md](./docs/roadmap.md)。没有在做的版本。

## 这一版做了什么（1.1.4）

1. **设置 → MEMORY**：全部项目的记忆一个项目一块（移出列表的有记忆才列），每条能 EDIT、DELETE，最下面自己记一条（ADD）。项目设置里的 MEMORY 用同一块，也能加、改了。
2. **读 PDF、Notebook**：`read_pdf`（macOS 自带的 PDFKit，经 `osascript` 在本机抽文字，扫描页给能看图的模型附 JPEG）、`ocr_pdf`（Mathpix：本机先拆出要的几页、只送这几页，识别完删掉它那边的，每页 $0.005 记在 `mathpix` 名下；设置 → PROVIDERS → PDF 两样 Key 都填了才给），两个都写进工具说明、模型自己挑（站主在提问里定的）。`read_notebook` 读 `.ipynb` 的单元和输出（图附给能看图的模型）。子代理也有。
3. **MCP 工具按需加载**：外部 MCP 工具开头只在 `tool_search` 的说明里列名字；`select:` 或关键词加载，加载的从下一次请求起进工具列表（站主在提问里选的「加进工具列表直接调」），一个对话里一直留着。CLI 的 MCP 说明是 CLI 自己的事（Claude Code 本来就按需加载），Peck 给 CLI 的本机 MCP 只有几样工具。
4. **写进 roadmap「不做」**：改完代码自动拿到编译错误、会话树和分叉浏览、轨迹记录。
5. **脚本 Hook 的 `SessionStart`**：对话第一句发出去之前跑一次（`startup`），压成交接摘要之后再跑一次（`compact`，站主在提问里定的「只在换成交接摘要之后」）；stdout 当补充上下文，退出码 2 不拦、只记一笔，子代理不跑。收尾时的 stdin 多了 `peck_changed_files`（这一轮改了的文件）。
6. **Git Plan 1.1.0**：多一条 `type: command` 的收尾 Hook（`hooks/handoff.sh`）：项目里有 HANDOFF.md、这一轮改了别的文件、HANDOFF.md 没改，就退出码 2 让模型先更新；一回合只提醒一次。Skill 里也写了一段。插件库自带、一字没改的 Hook 不弹卡片（我加的，不然每个项目第一次收尾都要点一次）。
7. **Peck 替 CLI 跑三个 Hook**：开场时、发话时在这句发出去之前跑（stdout 放在这句前面，发话时退出码 2 不发）；收尾时 CLI 做完再跑，退出码 2 就续上它的会话把原因发过去接着干，一回合最多 3 次，排在 Git Plan 自动提交之前。Claude Code 只跑插件里的；Antigravity 不跑。Hook 卡片走 Peck 自己的通道，不会被「替 CLI 批命令」的规则自动批掉（写测试时发现的，已挡住）。
8. **顺手修的**：插件详情里 `command` 类 Hook 还写着「Peck 不跑」（v1.1.2 起早就跑了）；交接摘要里会把 Hook 补充的话当成站主说的话。

- 新加依赖：没有（PDF 走系统的 PDFKit，Mathpix 的上传自己拼 multipart）。数据库没动（schema 11）。钥匙串「其他」那一包多 `mathpix_id`、`mathpix_key`。IPC 多 `memory_overview`、`memory_add`、`memory_update`。

## 验证

- **自动测试**：全量 `cargo test` 过（330 个；新加：SessionStart 只在第一句跑、退出码 2 不拦、压缩后补回；CLI 的三个 Hook 用假 CLI 跑一整个任务（开场、发话补充在这句前面，收尾拦一次续上会话，脚本拿到 `peck_changed_files`，卡片三张）；控制通道的转发；Git Plan 的 HANDOFF 脚本（装进项目没改过不问、改过要问、没 HANDOFF.md 放行）；MCP 按需加载（开头只有名单、加载了才有、下一句从记录认回来）；读 Notebook、PDF 一批同时跑、图跟在结果后面；PDFKit 真读一份 `cupsfilter` 做的 PDF、拆页；Mathpix 的错误和 multipart；记忆改字）；`pnpm check`、`pnpm build` 过。
- **`?mock`**：设置 → MEMORY（加一条、改一条、明暗都看了）、PROVIDERS 的 PDF 一块。
- **真跑**（账在 roadmap）：Mathpix 两次各 1 页（$0.01）：公式出 LaTeX、表格出 tabular；第二次确认它那边删掉了。
- **正式版**：装好了（`pnpm app:install`），版本号 1.1.4、签名校验过；窗口里没点过。
- **没验的**：
  - Peck Loop 里真模型用 `read_pdf` / `ocr_pdf` / `read_notebook`、`tool_search` 没真跑过（只有假模型）；扫描页出图只在格式化里测过，没拿真扫描件跑。
  - CLI 的三个 Hook 没在真 CLI 上跑过（只有假 CLI）；续上会话那一步各家的表现没看过。
  - 正式版窗口里一下都没点过：设置 → MEMORY、PROVIDERS 的 Mathpix、Hook 卡片。

## 留给站主的

- **Git Plan 要点 UPDATE**：已经装了 1.0.0 的项目，在项目设置 → PLUGINS 点 UPDATE 才有 HANDOFF 检查（Peck 不自己改装进项目的文件）；新装的直接是 1.1.0。
- **Mathpix 的 Key**：在设置 → PROVIDERS → PDF 填 App ID、App Key（我真跑用的是 Key 目录里的 `mathpix`，没存进钥匙串）。
- **第一次打开新版**：可能弹钥匙串授权框。
- **走一遍**：在测试项目里放一份 PDF，PLOOP 发「读一下 x.pdf 第 1 页」；配了 MCP 服务器的项目看过程里先「加载 MCP 工具」再调；测试项目的 `.claude/settings.json` 写一条 `SessionStart`，换 Codex 发一句看补充有没有带上。
- **push**：本地 `main` 比 `origin/main` 多 1.0.2 到 1.1.4 的全部提交。
- 以前留下、还没验的：1.1.3 的 member 真跑、任务板和 Claude Code 清单在正式版里；1.1.2 的正式版窗口（AGENTS、真派子代理、脚本 Hook 挂真的 `.claude/settings.json`）、searcher / auditor 真跑、百炼新加坡地域；1.1.1 存 Tavily 的 Key、真搜一次；1.1.0 的插件界面和收尾提交；1.0.3 的轮数设置、旧任务的样子；钥匙串里的 MCP 环境变量、截图要的屏幕录制权限、「减弱动态效果」、从访达拖图进输入框、额度真到 85% 时的提醒、Muse 在要用的项目文件夹里点一次信任；1.0.2 的反悔、定时任务、dev server 在正式版和真 CLI 上走一遍。Gemini 原生搜索、Vertex 上的 Claude 和 Gemini 还没真跑过。

## 已知问题

| 问题 | 建议 |
| --- | --- |
| 加载一批 MCP 工具，下一次请求的缓存整个重写一次（工具列表变了） | 站主选的这种；常用的话一个对话里只付一次 |
| 没加载就直接调 MCP 工具也照样跑（模型不该这样，供应商一般也不让） | 记着 |
| CLI 收尾 Hook 的卡片等着时按了停：这一轮照样算做完，Git Plan 照样提交 | 记着 |
| CLI 的 Hook 卡片、Peck Loop 的一样：定时任务里碰到没批过的命令会一直等 | 先在对话里跑一次点 ALWAYS |
| `read_pdf` 抽的文字按 PDFKit 的顺序，分栏、表格会乱；`ocr_pdf` 的 `.mmd` 不标页边界 | 模型看说明自己挑 |
| Mathpix 的费用按送去的页数照官网价记（v3/pdf 每页 $0.005），不读它的账单 | 宁可按价算 |
| HANDOFF 检查要 `jq`（macOS 15 起系统自带）；没有就放行 | 记着 |
| 搜索按次的费用按收费价记，Gemini 每月头 5000 次、Tavily 免费档每月 1000 次不扣（会多算） | 宁可多算；在意再加个月度计数 |
| 子代理报告回来以前站主停下任务，派出去的一起叫停，报告不进对话 | 记着 |
| 划文件只管文件工具和命令的沙箱：MCP 工具、主角色自己的命令管不到；项目在临时目录里时沙箱那一层不起作用 | 记着 |
| 命令原地改划给它的单个文件（`sed -i` 先在同一个文件夹建临时文件）会被沙箱拦；划文件夹、或用文件工具改就没事 | 记着 |
| 主角色可能一次回答只派一个、分几次派（真跑时 gpt-6-luna 就这样），同一批的说法就变成几批 | 用一阵看要不要在提示里再强调 |
| 原地打转只认一模一样的调用和结果；参数稍微不同、结果一样的绕圈认不出来 | 用一阵再看 |
| 脚本 Hook 不进沙箱，跑的是站主 shell 的环境（第一次弹卡片就是为这个） | 记着 |
| OpenAI 的来源没有标题，过程里标题写网址 | 记着 |
| Gemini 的来源挂在第一条搜索上（它只在最后一起给） | 记着 |
| 只有界面点开项目时才认结构、装默认插件；定时任务在一个从没点开过的老项目里跑时不会先做 | 记在 roadmap |
| 单独的 Skill 装没装是按「项目里有没有同名的 Skill」认的：项目自己写了一个和库里同名的，会显示成装着的，点 UPDATE / REMOVE 会动到它（库里现在没有单独的 Skill） | 往库里放单独的 Skill 之前先加归属记号 |
| 收尾提交失败（仓库的 pre-commit 钩子不过、没配 `user.name`）时，这一轮的文件已经 `git add` 了，留在暂存区 | 记着 |
| CLI 任务每轮做完多用一轮订阅额度问提交说明；Antigravity、续不了会话的不问，写「模型没给出提交说明」 | 用一阵看烦不烦 |
| Peck Loop 用命令（不是自己的工具）改了一个开头就没提交的文件、别的都没动时，收尾不问、也不提交 | 记着 |
| 插件带的 MCP 服务器（`.mcp.json`）只列出来，Peck Loop 不起 | 有插件要用时再接 |
| CLI 续接会话的花费按「会话号」取差：续接后会话号变了就整个算进去（不会少算，可能多算一次） | 真跑时顺便看 QUOTA |
| 模型只在被点名时才提议存 Skill，主动提的没见过 | 用一阵再看 |
| 定时任务的「谁来干」设的时候不查 Key 和 CLI 在不在，到点起不来记成错过 | 记着 |
| 回退只按整个文件放回，做不到一个文件里的某一段 | Skill 里让模型直接改那一段 |
| 预览的 RELOAD 回到最初的地址；`localhost` 只连 127.0.0.1 | 记着 |
| Vertex 上的 Claude / Gemini 只有离线测试，没真跑过 | 站主开了 GCP 项目再试 |
| PREVIEW 批注只在浏览器存储里；Esc 关设置页时没存的 MCP 编辑会丢 | 记着 |

## 视觉基线要补的

方案 `peck` 里还没有、现在取了沉静一边的样子（样式都在 `src/app.css`），要在方案里画、站主批、发新版本再同步：ⓘ 图标（现在是占位）和弹层、`[ NOW ]`、recap 和下一步按钮、`[ SKILL ]` / `[ MEMORY ]` / `[ CONTEXT ]` 卡片、`/` 和 `@` 弹层、归档图标和归档条、PLOOP 每一段的头和审查结论框、按段花费表、PREVIEW 的地址栏和批注列、设置里的 MCP、QUOTA、AGENTS 几组、项目设置的各节、代码配色 `--code-*`、菜单栏「在跑 N 个」、全部项目页、PROJECTS 头上的放大图标、输入框右下角的 ↵、设置里的进度条、PREVIEW 头上的 dev server 小片、「Thinking…」行、PROJECTS 头上的钟和它的详情弹层、`[ MISSED ]` 卡片、对话里的独立工作区条。1.0.3 新加的：SPEC 那一行（整行能点、最右三角）和展开后的规范框、「读过 N 个文件」列表。 1.1.0 新加的：插件那一行（名字前的小三角、版本、标签）和展开的清单（`.plugin-detail`）、EVERY PROJECT 开关旁的字、recap 里收尾提交的那一行。 1.1.2 新加的：LOG 上面的成员名单（`.log-roster`）、设置 → AGENTS 的 SUBAGENTS 几块（开关、「跟主角色」、「同时 N 个」）。1.1.3 新加的：任务板（`.board-*`：状态字的颜色、`WORKER 2` 标签、划的文件那一行、展开的步骤）。1.1.4 新加的：设置 → MEMORY 的项目头（`.memory-group`、`.memory-path`）、改字和自己记一条那一行（`.memory-edit`）。

## 留意

- Gemini 4 Carbon / Argon 12 月 1 日起涨价（Google 官网），到时在设置 → QUOTA 点 REFRESH。
- Codex 和 ChatGPT 客户端共用登录；Claude 额度要 `cli/bin` 的 Claude Code 登录一次才读得到。
- 测试项目 `projects/tester`、`projects/moon-lab`、`projects/gateway-smoke`、`projects/git-plan-smoke`（git 仓库，收尾提交用）在 Peck 仓库外。
- 改系统级的插件（`src-tauri/defaults/plugins/`、`defaults/library/`）要升它自己的 `version`，不然装着旧版的数据目录和项目收不到。
- Mathpix 的接口：认证是两个请求头 `app_id`、`app_key`，错误大多是 HTTP 200 带 `error_info.id`；真跑的入口 `gateway::mathpix` 的 `live_mathpix`。
- 联网搜索的协议各家常变（Gemini 的工具组合还是 Preview、Anthropic 搜索工具按日期出新版本）：出 400 先查 `gateway/search.rs` 和各家转换器，`live_web_search` 能单独真跑一家。

## 下一步

自定义子代理（站主之前定的下一步）。其他候选在 roadmap「之后要做的」。
