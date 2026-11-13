# Peck

只为我、只在我这台 Mac 上跑的个人 Harness：Model Gateway 把各家 API（Anthropic、OpenAI、DeepSeek、阿里云百炼、Gemini、Vertex AI）和各家 Coding CLI（Claude Code、Codex、Muse、Grok、Antigravity）接进来；Peck 自己的循环带工具箱和上下文管理去改代码；后台替模型盯着测试和检查。改动看得见，整个任务能一键撤回。不跑本地模型，不联网同步。

Tauri 2 + React + TypeScript + Rust + SQLite。做到哪了见[开发路线](docs/roadmap.md)。另一个 Claude 账号在云端的分支上开发，我指定的 Phase 由本地账号在 `main` 上开发；本地验收、合并（规则在 [CLAUDE.md](CLAUDE.md)「两个账号」）。

## 模块

| 模块 | 用途 |
|---|---|
| 项目和任务 | 打开先看项目列表，每个项目带最近几次任务；点进去是这个项目的任务列表；下面是正在看的任务的记录 |
| 发任务 | 一句话（↵ 发送，`@` 引用旧任务）+ 权限（PLAN / EDIT / AUTO）+ 目标模式；谁来干在左栏 AGENTS 里选（FLOW：API、CLI、接力、规划 + 实施；CLI 能选模型）+ 思考强度 |
| 看任务 | 回答按 Markdown 显示，思考和工具调用折叠成一行（点开看每一步），要批的事、要问的事弹卡片，下面一行花费；验证卡片（Peck 替模型跑的验证命令）、压缩上下文、「想记住这几条」 |
| 上下文 | 项目说明（AGENTS.md / CLAUDE.md）和记忆、每次带上的环境和 git；输入框旁的小环看用了多少，到压缩线自动压缩；`@` 旧任务 |
| 后台和验证 | `.peck/verify.toml`（第一次打开项目 Peck 猜；项目自带的、改过的都要我确认才用）：改了文件后台跑，没过交回模型修（最多 3 次）；目标模式 GOAL；常驻 dev server（DEV） |
| 右栏 | 四个页签：CHANGES（文件列表、Diff、任务前快照、「撤销整个任务」）、ONE SHOT（边干活边问，不改文件）、FILES（看项目文件）、TERM（我自己的终端，模型跑任务时锁着） |
| 设置 | 供应商和 Key、花费上限、权限、CLI 检测（引擎版本、登录）、通知、外观（明暗、App 图标），都在一个面板里；项目自己的（默认谁来干、验证命令、记忆）在右键 → 项目设置 |
| 额度与花费 | 各家订阅还剩多少（Claude 用 Claude Code 自带的 /usage 读，桌面 App 开着也算）、API 平台余额、本机花费 |

## 怎么用

- **安装或更新**：`pnpm app:install`，装进「应用程序」，旧包自动备份一份（更早的挪进废纸篓）。新版第一次打开先备份、再升级数据库。
- **CLI**：Claude Code、Codex、Antigravity、Muse、Grok 和 gcloud 都装在仓库根目录的 `cli/`（`cli/bin/` 是统一入口，怎么更新看 `cli/README.md`），用订阅账号登录；Peck 打开时会自己找到 `cli/bin` 存成「CLI 目录」（设置 →「CLI 检测」里能看、能改），也会找终端 PATH 里装的。没登录的那家只是不能选，不影响别的。
- **API**：在设置里填 Key（存在 macOS 钥匙串），只显示配了 Key 的供应商。真实请求有授权次数，见[开发路线](docs/roadmap.md#现行授权)。
- **开发**：`pnpm dev` 后开 `http://localhost:1430/?mock` 看假数据；`pnpm app:dev` 是原生窗口，用开发数据目录。怎么测见[验证方法](docs/testing.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [开发路线](docs/roadmap.md) | 现在在哪个 Phase、每个 Phase 做什么、验收清单、授权账本 |
| [技术架构](docs/architecture.md) | 选型、一个任务怎么跑、数据存放、Model Gateway、Peck 循环、代码位置 |
| [界面](docs/design.md) | 用词对照、布局、各处的样子 |
| [验证方法](docs/testing.md) | `?mock`、后端测试、原生、升级测试、夹具、真 CLI 和真 API |
| [验收清单页](docs/acceptance/README.md) | 每个版本出给我验收的页面怎么做 |
| [当前交接](HANDOFF.md) | 上一次做到哪、验了什么、下一步 |
| [CLAUDE.md](CLAUDE.md) | 给 AI 编程助手的规则（[AGENTS.md](AGENTS.md) 指向它） |

## 数据在哪里

- 正式数据：`~/Library/Application Support/Peck/`（`peck.db` 项目、任务、记录和事件日志；`shadow/` 任务前快照；`harness-runtime/`、`harness-output/` Peck 循环命令用的 HOME 和长输出；`api-budget.db` Agent 真跑的授权账本）。细节见[技术架构](docs/architecture.md#数据存放)
- 开发数据：`~/Library/Application Support/Peck Dev/`
- Key：macOS 钥匙串，正式、开发、测试分开
- 视觉基线：visual-atelier 的 `schemes/peck/`（Design System「[Peck](https://claude.ai/artifact/KpUihg5AHn6cw1ggNTx6s1)」），同步进 `src/vendor/peck/`（不手改）
