# Peck

只为我、只在我这台 Mac 上跑的个人 Harness：Model Gateway 把各家 API（Anthropic、OpenAI、DeepSeek、Aliyun、Gemini、Vertex AI）和各家 Coding CLI（Claude Code、Codex、Muse、Grok，各用官方原生接口）接进来。两种干法分开：CLI 引擎整包干，Peck 只当外壳；Peck Loop 是 Peck 自己的循环，带工具箱、上下文管理和联网搜索去改代码，PLOOP 是它的工作方式（一个 API 模型当主角色，能派子代理）。每个任务前后都有快照，说一声就能回到任何一次之前；定时任务也是说一声就设好。

Tauri 2 + React + TypeScript + Rust + SQLite。当前版本见[开发路线](docs/roadmap.md)，规则和文档分工见 [CLAUDE.md](CLAUDE.md)。

## 模块

| 模块 | 用途 |
|---|---|
| 项目和任务 | 没选项目时中间是全部项目页，左栏一整块是 AGENTS；选了项目，左栏是 NOW（跨项目的待你处理、在跑）和 PROJECTS（这个项目的任务）；不同项目的任务能同时跑；任务能归档（设置 → ARCHIVE） |
| 发任务 | 一句话（↵ 发送，`@` 引用旧任务，`/` 斜杠命令）+ 权限（PLAN / EDIT / AUTO）+ 独立工作区（WORKTREE，做完在对话里 APPLY / DISCARD）；谁来干在左栏 AGENTS 里选（CLI、PLOOP；思考强度），输入框下一行短的写现在是谁；贴图、拖图进输入框 |
| PLOOP 和换引擎 | PLOOP 是 Peck Loop 的工作方式：选一个 API 模型当主角色，它需要时在后台派子代理——explorer 读项目、searcher 联网调研、worker 做简单的改动、member 做复杂的活（权限和主角色一样，能再派 explorer、searcher）、auditor 审改动，同一次派的一批做完报告一起交回；worker、member 派的时候划好能改哪些文件，不重叠的同时跑、重叠的排队，只能改自己那块；对话里的任务板显示它们的进度，点开看每个的步骤；每类的模型、思考强度、轮数、同时几个在设置 → AGENTS。只读的工具一次要几个就同时跑；同一步原地打转会提醒、再转就停。项目 `.claude/settings.json` 和插件里照 Claude Code 格式写的 Hook（开场时、发话时、调工具前后、压缩前、收尾时）会跑，第一次先问；CLI 引擎 Peck 替它跑开场时、发话时、收尾时三个（Claude Code 只跑插件里的）。CLI 之间交接：换引擎接着发一句（带上前面的经过）或 `/review`。Peck Loop 接外部 MCP 服务器（设置里的和项目 `.mcp.json` 里的），工具按需加载（开头只给名单），能读 PDF（本机 PDFKit，或 Mathpix 认扫描件、公式、表格）和 Jupyter Notebook |
| 联网搜索 | Peck Loop 的模型要查网上的东西时用：Anthropic、OpenAI、Gemini、Aliyun 用它们自己的搜索；DeepSeek 用它家的 Search API；别的用 Tavily（设置 → PROVIDERS → SEARCH 填 Key）。按次记花费 |
| 看任务 | 任务清单在最上面（各家 CLI 的也转过来），回答按 Markdown 显示，连着的工具调用折成一行英文（像 Claude Code），在想时一行 Thinking；要批的事、要问的事弹卡片，下面一行花费；压缩上下文、「想记住这几条」；每轮末尾一块 recap，带下一步按钮；进任务停在最新一轮、回来回到上次的位置 |
| 上下文 | 项目说明（AGENTS.md / CLAUDE.md，改完跑什么测试也写在这里，模型自己跑）和记忆（设置 → MEMORY 能自己记、改、删）、Skills（按需读全文；模型能提议存成 Skill，批了才写进这个项目）、每次带上的环境和 git；输入框旁的小环看用了多少，到压缩线自动压缩 |
| 插件 | 一包 Skill、斜杠命令、工具、Hook。自带的（撤销、定时任务、写 Skill）按项目开关；插件库里的在项目设置里 INSTALL，拷进项目、跟着项目的 git 走，库里有新版出 UPDATE、点了才覆盖。库里第一个是 **Git Plan**（默认每个项目都装）：版本和提交的规矩、`/git-plan:plan` 这几个命令，每一轮做完 Peck 问模型要一行提交说明、自己提交这一轮改过的文件，不 push；项目里有 HANDOFF.md 时，改了别的文件没更新它就先让模型更新。点开项目时 Peck 补齐 CLAUDE.md / AGENTS.md 和 Skill 文件夹的软链接，哪家 CLI 都读得到同一份 |
| 在对话里做的 | **反悔**：「撤掉刚才的改动」，模型列出快照、复述，弹高危卡片，批了才放回去。**定时任务**：「每周一早上九点帮我检查依赖更新」，模型复述确认后设好，PROJECTS 头上的钟里看；App 关着错过的，重开时弹卡片 RUN NOW / SKIP。**dev server**：模型起的会留着，右栏 PREVIEW 里看、点一下停 |
| 右栏 | 五个页签：FILES（标出这次改了哪些）、LOG、ONE SHOT（边干活边问，不改文件）、PREVIEW（开 dev server 的页面，流式的也行；点元素写批注）、TERM（我自己的终端） |
| 本机 | 截 App 窗口给模型看、剪贴板、键盘（⌘N、⌘L、⌘.、⌘1–⌘5……）、菜单栏图标和通知 |
| 设置 | GENERAL、PROVIDERS、AGENTS、MEMORY、QUOTA、ACCESS、CLI、LOOK、PLUGINS、ARCHIVE、KEYS；项目自己的在右键 → 项目设置 |
| 额度与花费 | 顶栏 QUOTA：各家订阅还剩多少、API 平台余额、本机花费（按 API 价格表算，CLI 折成 API 价；国内的模型按人民币） |

## 怎么用

- **安装或更新**：`pnpm app:install`，装进「应用程序」，旧包自动备份一份。新版改了数据库结构时先备份、再升级。
- **CLI**：Claude Code、Codex、Muse、Grok 和 gcloud 都装在仓库外的 `personal-cli-studio/cli/`（`cli/bin/` 是统一入口，怎么更新看 `cli/README.md`），用订阅账号登录；Peck 打开时自己找到 `cli/bin`（设置 → CLI 里能看、能改）。没登录的那家只是不能选。
- **API**：在设置 → PROVIDERS 填 Key（存在 macOS 钥匙串），只显示配了 Key 的供应商。
- **开发**：`pnpm dev` 后开 `http://localhost:1430/?mock` 看假数据；`pnpm app:dev` 是原生窗口，用开发数据目录。怎么测见 [docs/testing.md](docs/testing.md)。

## 数据在哪里

- 正式数据：`~/Library/Application Support/Peck/`（`peck.db`、`shadow/` 任务前快照、`worktrees/` 独立工作区、`library/` 插件库等，细节见[技术架构](docs/architecture.md#数据存放)）；开发数据在 `Peck Dev/`。
- Key：macOS 钥匙串，正式、开发、测试分开。
- 视觉基线：visual-atelier 的 `schemes/peck/`，同步进 `src/vendor/peck/`。
