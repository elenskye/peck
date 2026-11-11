# Peck

只在我自己的 Mac 上用的本地 Coding Agent：给它一个项目文件夹和一句话，它调本机装好的 CLI（Muse Code、Grok Build）或 API 模型（Anthropic、DeepSeek、OpenAI、Google Gemini、阿里云百炼）去改代码。改动看得见，整个任务能一键撤回。不跑本地模型，不联网同步。

Tauri 2 + React + TypeScript + Rust + SQLite。正在重构，做到哪了见[开发路线](docs/roadmap.md)。v0.4 起由另一个 Claude 账号在云端的分支上开发，我在本地验收、合并（规则在 [CLAUDE.md](CLAUDE.md)「两个账号」）。

## 模块

| 模块 | 用途 |
|---|---|
| 项目和任务 | 打开先看项目列表，每个项目带最近几次任务；点进去是这个项目的任务列表；下面是正在看的任务的记录 |
| 发任务 | 一句话 + 「谁来干」 + 权限（PLAN / EDIT / AUTO）：CLI 模式（Muse、Grok、Muse → Grok、Grok → Muse，可以让它调用 API 模型帮忙）或 API 模式（单模型干活、规划 + 实施） |
| 看任务 | 折叠的步骤（点开看输出）、提问和回答、跑完的总结和验证结果 |
| 右栏 | 四个页签：CHANGES（文件列表、Diff、任务前快照、「撤销整个任务」）、ONE SHOT（边干活边问，不改文件）、FILES（看项目文件）、TERM（我自己的终端，模型跑任务时锁着） |
| 设置 | 供应商和 Key、花费上限、通知、权限、CLI 检测，都在一个面板里 |
| 额度与花费 | 其他 AI 客户端的订阅额度、API 平台余额、本机花费。由我用 Peck 自己开发（v0.5 起） |

## 怎么用

- **安装或更新**：`pnpm app:install`，装进「应用程序」，旧包自动备份。新版第一次打开先备份、再升级数据库。
- **CLI**：Muse、Grok 装在仓库根目录的 `cli/`，用订阅账号登录（命令见[技术架构](docs/architecture.md#cli-接入)），在设置里检测一下就能选。没登录的那家只是不能选，不影响别的。
- **API**：在设置里填 Key（存在 macOS 钥匙串），只显示配了 Key 的供应商。真实请求有授权次数，见[开发路线](docs/roadmap.md#现行授权)。
- **开发**：`pnpm dev` 后开 `http://localhost:1430/?mock` 看假数据；`pnpm app:dev` 是原生窗口，用开发数据目录。怎么测见[验证方法](docs/testing.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [开发路线](docs/roadmap.md) | 现在在哪个 Phase、每个 Phase 做什么、验收清单、授权账本 |
| [技术架构](docs/architecture.md) | 选型、数据存放、CLI 和 API 的接入层、代码位置 |
| [界面](docs/design.md) | 用词对照、布局、各处的样子 |
| [验证方法](docs/testing.md) | `?mock`、原生、升级测试、真 CLI 和真接口的命令 |
| [验收清单页](docs/acceptance/README.md) | 每个版本出给我验收的页面怎么做 |
| [当前交接](HANDOFF.md) | 上一次做到哪、验了什么、下一步 |
| [CLAUDE.md](CLAUDE.md) | 给 AI 编程助手的规则（[AGENTS.md](AGENTS.md) 指向它） |

## 数据在哪里

- 正式数据：`~/Library/Application Support/Peck/`（`peck.db` 项目、任务和记录；`shadow/` 任务前快照；`api-budget.db` 真实 API 的授权账本；`worker-runtime/` 命令用的独立 HOME 和缓存）
- 开发数据：`~/Library/Application Support/Peck Dev/`
- Key：macOS 钥匙串，正式、开发、测试分开
- 视觉基线：visual-atelier 的 `schemes/peck/`（Design System「[Peck](https://claude.ai/artifact/KpUihg5AHn6cw1ggNTx6s1)」），同步进 `src/vendor/peck/`（不手改）
