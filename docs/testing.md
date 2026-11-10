# 验证方法

> 怎么测、用什么命令。原则在 [CLAUDE.md](../CLAUDE.md)「验证」「额度」，云端能跑的只有 `?mock`、类型和构建、后端测试（编得过的话）；平时只用 `?mock` 和本地夹具；真 CLI 只做验收点要求的冒烟；真 API 只用 [roadmap](./roadmap.md#现行授权) 里剩下的授权；不碰正式数据目录。模拟、自动测试、原生交互、真实服务的结果分开记，没验过的不写「通过」。

## 界面：`?mock`

- `pnpm dev`（固定端口 1430），开 `http://localhost:1430/?mock`。本地：内置浏览器，`personal-cli-studio/.claude/launch.json` 里的 `peck-mock` 就是它；云端：用无头浏览器开同一个地址截图看。
- 浏览器里 `src/lib/ipc.ts` 把所有调用转给 `src/dev/mockIpc.ts`，数据在 `src/dev/mockData.ts`，只在内存，刷新重置；原生窗口永远走 Rust。
- 参数：`&empty` 没有项目（看空状态）。
- 假数据里有 3 个项目（notebook、peck、edge-gateway），6 个任务：在跑的一个（Muse → Grok，第二步在跑）、完成的四个（Grok 单个、Muse → Grok、API 模式单模型干活、API 模式规划 + 实施）、失败的一个（Grok 登录过期）。每个完成的任务都有改动列表和 Diff，API 模式的有调用与审查历史。
- 在 `?mock` 里发任务：不带前缀走 Muse → Grok，`@muse`、`@grok`、`@mock` 开头走单个；几秒跑完，能停、能重跑步骤。
- 改了界面明暗各看一次（内置浏览器的 `resize_window` 切 `colorScheme`），截图只放会话临时目录。截图常慢一拍，关键结果用 `read_page` 或 JS 读页面确认。
- `?mock` 不代替原生、钥匙串、真实 API 的验收。

## 类型和构建

```bash
pnpm check     # tsc --noEmit
pnpm build     # 前端生产构建
```

## 后端

```bash
set -o pipefail
T=$(mktemp -d)
PECK_DATA_DIR=$T/data PECK_SHADOW_ROOT=$T/shadow \
  cargo test --offline --manifest-path src-tauri/Cargo.toml
```

- 日常只跑改到的模块：在命令末尾加模块名过滤，如 `-- snapshot::`。一个版本收尾才全跑。
- 不用 `| tail` 截输出（会吞掉退出码）；要短就加 `-q`。
- 有些测试要本机允许回环 HTTP 夹具和 `sandbox-exec`。API 协议的测试用本地夹具，不发真请求；夹具通过不等于真实账号通过。

## 原生（只有本地）

- `pnpm app:dev`：原生开发窗口，数据在 `~/Library/Application Support/Peck Dev/`，API 默认模拟。
- 装好的正式包隔离着跑（不碰正式数据和钥匙串里的正式 Key）：
  ```bash
  PECK_DATA_DIR=/private/tmp/peck-demo PECK_API_MODE=mock \
    /Applications/Peck.app/Contents/MacOS/peck
  ```
- 假 Agent（设置的开发者选项，清单 `src-tauri/defaults/agents/mock.toml`）会在项目里写演示文件，用临时目录当项目。
- 验收点要「原生」的，就在 App 里真的点一遍：发任务、看步骤、看 Diff、撤销、重开 App 后记录和 Diff 还在。

## 升级测试（只有本地）

1. 退出 Peck，把正式数据目录整个复制到临时目录（`peck.db`、`peck.db-wal`、`shadow/` 都要）。
2. 用新版指过去打开：`PECK_DATA_DIR=<副本> /Applications/Peck.app/Contents/MacOS/peck`（或 `pnpm app:dev` 加同样的变量）。
3. 看：数据目录里生成了升级前的备份；项目、任务、消息、调用数和升级前一样；旧任务的 Diff 能打开；用旧版打开新库会被拒绝写入。

## 真 CLI（冒烟，只有本地验收时）

每个 Phase 最多 3 个任务、每个不超过 12 轮；在 HANDOFF 记次数和结果，失败不静默重试。

1. 设置 → CLI 检测，Muse、Grok 都显示版本（只说明装好了，不说明登录了）。
2. 用一个临时 Git 项目当目标，发一个小任务（比如「在 README 末尾加一行今天的日期」）。
3. 看：步骤输出、改动列表、Diff 都对；任务前快照能撤销；认证或额度失败时任务停下并显示原因，不显示成功。
4. 先写规范再实施：两个方向各一次，看 `.peck/handoff/<任务号>/intent.md` 生成、后一段读到了它。

## 真 API（只有本地验收时）

- 只用 roadmap「现行授权」剩下的次数，每个 Phase 不超过 5 次尝试；发之前和之后把账本写进 roadmap。账本在正式数据目录的 `api-budget.db`，测试目录不重置它。
- 用正式包、设置里选真实，在临时项目里发一个任务；看调用记录里的模型、思考、usage、次数和账本一致。
- 不用「你好」探针测额度，不用模型 turn 查余额。

## 额度与花费的真实来源

评估额度监测器（v0.5 起，Phase 8 和每个版本的验收页）时对照这张表。这是 2026-11-06 读官方文档的结论，接入前按本机安装的版本再核对；没有的那家只影响那一家。

| 来源 | 官方入口 | 注意 |
|---|---|---|
| Codex | [App Server](https://learn.chatgpt.com/docs/app-server)：`account/rateLimits/read`、`account/rateLimits/updated` | 不用 API Key 推订阅额度；同一池的兼容字段和 map 项不重复加；credits 不当 USD |
| Claude Code | [status line JSON](https://code.claude.com/docs/en/statusline)：`rate_limits` | 支持的账号、有过模型响应后才可能有；事件驱动，空闲不刷新；缺字段不是 0 |
| Antigravity | [CLI statusline](https://antigravity.google/docs/cli/statusline)：`quota` | 只是 CLI 的路径，不代表桌面 App；上下文比例不是订阅额度 |
| Muse CLI | `muse usage --json`（[更新记录](https://dev.meta.ai/docs/muse-code/changelog)） | 用 `allowance.asOfMs` 作数据时间；没有账户标识，旧快照不归到新登录的账号 |
| Grok CLI | [`/usage`](https://docs.x.ai/build/modes-and-commands) | 1.0.77 的 `usage --json` 还不给套餐数据，只能手工快照 |
| DeepSeek API | [GET `/user/balance`](https://api-docs.deepseek.com/api/get-user-balance/) | 用已配置的 Key；换 Key 清旧余额 |
| Anthropic API | [Usage/Cost Analytics](https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api) | 要 Admin 权限，普通 Key 没有余额接口，显示未知 |

看的要点：只在对应客户端运行时监测，退出就停并留最后一次快照；未装、未登录、缺字段、失败分开显示；未知不填 0，币种不直接相加，估算标出来；不采集提示词、会话正文、环境变量；不为查额度启动已关闭的客户端或发模型 turn。合成数据只验启停和显示，不算真实接入。
