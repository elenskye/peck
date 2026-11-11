# 技术架构

> 选型、数据存放、CLI 和 API 的接入层、代码位置。界面的用词和样子在 [design.md](./design.md)，怎么测在 [testing.md](./testing.md)。

## 选型

| 层 | 用什么 | 为什么 |
|---|---|---|
| 外壳 | Tauri 2（macOS） | 原生窗口、钥匙串、菜单栏、通知；包小 |
| 界面 | React 19 + TypeScript，样式是 `src/vendor/peck/` 的变量和 `pk-` 组件加一份 `src/app.css`（不用 Tailwind），Vite 固定端口 1430 | 浏览器 `?mock` 能自检；视觉只有一个来源 |
| 后端 | Rust | 模型请求、工具、CLI 进程、快照、SQLite 都在这里，界面只经 IPC 调 |
| 存储 | SQLite（WAL）+ 影子 Git 仓库 | 记录和历史 Diff 分开存，重开都还在 |

不做：常驻辅助服务、通用插件框架、多层 Agent 网络、本地模型、云同步、开机自启。

## 一个任务怎么跑

一句话对应一个**任务**（代码里叫 Run）。任务开始前给项目拍一个**任务前快照**；任务里的每一段是一个**步骤**（Step / Task）；跑完留着，撤销是站主主动点。同一个项目同时只跑一个任务（v0.7 起不同项目能同时跑）。

| 谁来干 | 怎么执行 |
|---|---|
| 单个 CLI | 按清单起子进程，直接传参不经 shell，流式读输出；超时、停止都终止整个进程组 |
| 先写规范再实施 | 前一个 CLI 写 `.peck/handoff/<任务号>/intent.md` 和 `.peck/tasks.md`，后一个读它们实施；界面里是一个任务两段。两个方向（Muse → Grok、Grok → Muse）用同一份流程清单的两步提示词，只换谁来跑 |
| 单模型干活 | 请求 → 原生工具调用 → 串行执行 → 回传结果 → 下一轮，直到模型收尾或到上限 |
| 规划 + 实施 | 规划的模型多两个工具：`delegate_task` 把边界明确的子任务交给实施的模型，`review_task` 记「通过 / 返工 / 接手」。子任务只拿到目标、必要上下文、写入范围和验收条件，不复制父对话 |
| 单次请求（v0.6 起是右栏 ONE SHOT） | 现在不带工具，问一句答一句；挪进 ONE SHOT 后只给读文件的工具，CLI 用 PLAN 档答 |

失败、取消、重开都不自动重发：可能已经生效的请求和命令不重放，继续由站主新发一个任务。

- **发任务**（`run_dispatch`）带「谁来干」`who`（`muse`、`grok`、`muse>grok`、`grok>muse`、`mock`）和权限档 `permission`（`plan` / `edit` / `auto`），都记在任务记录上。v0.4 权限档只记不管：CLI 的权限参数 v0.5 接，API 模式按档位给工具 v0.6 接。
- **跑完的总结**：CLI 是最后一步收尾时的最后一段输出（最多 24 行），API 模式是模型自己的总结。
- **撤销整个任务**（`run_undo`）：算出任务前快照到任务做完之间改过的文件，新建的删掉、改过和删掉的用 `git restore --source=<任务前的树> --worktree` 写回；只动这几个文件，用户仓库的 index、commit、stash 不碰。撤销前再存一份快照。记录上写 `undoneAt`，历史 Diff 照旧能看。
- **终端**（右栏 TERM，`term.rs`）：每条命令在 pty 里起一个 shell，输出当文字显示（`TERM=dumb`），跑完报退出码和用时；`cd` 到哪儿记着，`export` 不带到下一条。环境照站主平时的终端：App 打开时在后台起一次交互式登录 shell（`$SHELL -l -i -c`，`.zprofile`、`.zshrc` 都过一遍，最多等 8 秒）把环境变量读出来，这次打开期间每条命令带着它跑（`$SHELL -c`）；从访达打开的 App 只有系统给的最小 PATH，不这样就找不到 pnpm、node。读不出来时退回登录 shell（`$SHELL -l -c`）。改了 shell 的启动脚本要重开 App 才生效。这个项目有任务在跑时后端拒绝新命令；已经在跑的不管，STOP 结束整个进程组。只在内存，退出就没了。
- **关窗口**（`lib.rs`）：点红色关闭只把窗口藏起来，菜单栏图标和在跑的任务都留着；点 Dock 图标（Reopen 事件）或菜单栏图标叫回窗口；⌘Q 或菜单栏的「退出 Peck」才退出。
- **菜单栏图标和通知**（`tray.rs`）：界面告诉后端现在是空闲、在跑还是等你，图标跟着换（在跑时三帧轮流，每帧 300ms）；任务做完、失败、停下时界面按设置决定发不发 macOS 通知（Peck 在最前面时不发）。模板图是 `scripts/menubar-icons.mjs` 从 `src/vendor/peck/marks/menubar/` 出的 @2x PNG，放在 `src-tauri/icons/menubar/`：托盘把图片高度一律压成 18pt，所以脚本先裁掉四周的空（几个状态同一个裁法），一格出 3 像素，菜单栏里一格 1.5pt。

## 数据存放

| 内容 | 位置 |
|---|---|
| 正式数据 | `~/Library/Application Support/Peck/` |
| 开发数据（`pnpm app:dev`） | `~/Library/Application Support/Peck Dev/` |
| 项目、任务、步骤、消息、调用、工具记录、用量快照 | 数据目录里的 `peck.db`（schema v6，迁移在 `src-tauri/migrations/`）；v6 加了 `app_settings`，界面设置（明暗、模式、谁来干、默认权限、通知、花费上限、开发者选项）整段存成一行 JSON |
| 任务前快照和历史 Diff | 数据目录里的 `shadow/<项目哈希>/` |
| 真实 API 授权账本 | 固定在正式数据目录的 `api-budget.db`，测试目录、角色、别名都不重置 |
| 命令用的 HOME 和缓存 | 数据目录里的 `worker-runtime/<项目哈希>/home/` |
| CLI 清单 | 数据目录里的 `agents/*.toml`、`flows/*.toml`（第一次从 `src-tauri/defaults/` 复制） |
| Key | macOS 钥匙串，服务名 `Peck API`、`Peck API Dev`、`Peck API Test` 分开，account `keys` |

- `PECK_DATA_DIR`、`PECK_SHADOW_ROOT` 把数据和快照指到别处（测试用）；`PECK_API_MODE=mock|real` 强制 API 模拟或真实。开发和临时目录默认 API 模拟。
- **升级**：旧库升级前用 SQLite Backup API 做一致性备份（含 WAL 里已提交的数据）；遇到比自己新的 schema 拒绝写入。改结构就加迁移、升版本号，不改旧记录的含义。
- **任务前快照**：影子仓库用独立的 `GIT_DIR` 和 `GIT_WORK_TREE` 跑 `add`、`write-tree`、`commit-tree`、`diff`，不碰用户仓库的 index、commit、stash。每个任务和步骤存前后两棵树，重开还能看当时的 Diff。监听项目文件按忽略规则走，保留 `.peck/`。
- **备份**：要保住历史 Diff，`peck.db` 和 `shadow/` 一起备份，退出 App 后再复制。

## CLI 接入

一家 CLI 一份 TOML 清单，加一家只加清单（v1.0 只接 Muse、Grok）。字段：`id`、`name`、`description`、`command`、`args`、`cwd`、`stdin`（`none` / `prompt`）、`timeout`（如 `30m`）、`path`（补 PATH，`~/` 展开）、`env`。参数里能用 `{{prompt}}`、`{{handoff}}`、`{{workspace}}`。设置里能改、会校验，下次任务生效；旧的内置参数精确匹配时自动迁移，保留自己改过的 `command`、`args`、`env`。

| CLI | 命令 | 说明 |
|---|---|---|
| Muse Code | `muse exec {{prompt}}` | 设 `MUSE_NO_AUTO_UPDATE=1`；[官方非交互入口](https://dev.meta.ai/docs/muse-code/extending) |
| Grok Build | `grok -p {{prompt}} --output-format plain --no-auto-update` | [官方说明](https://docs.x.ai/build/cli/headless-scripting) |
| 假 Agent | `peck-mock --prompt {{prompt}} --handoff {{handoff}} --write-files` | 内置，不推理，开发和验收用 |

- 两家 CLI 装在仓库根目录 `cli/muse/`、`cli/grok/`（官方发布文件，关自动更新）。正式数据目录里的清单写绝对 `command`，内置清单不写机器路径；Finder 启动不继承终端的 PATH。
- 登录（在仓库根目录跑，按提示用订阅账号授权）：`./cli/muse/muse login`、`./cli/grok/grok --no-auto-update login`（要浏览器回调加 `--oauth`）。已有 `META_API_KEY` 时 Muse 优先用它。
- 检测只跑 `version`，看退出码；「检测到」不等于登录了、有额度。找不到可执行文件就不能选；认证、额度、权限、超时错误让任务停下，不当成成功。Peck 不默认加 `--yolo` 或关权限检查。
- v0.5 起：权限三档 PLAN / EDIT / AUTO（`design.md`「权限」）、高危操作卡片、ASK 卡片、「让它调用 API 模型帮忙」都经 Peck 给 CLI 的本机 MCP 入口，清单里多一组能力字段。

## API 接入

| 供应商 | 协议 | 要点 |
|---|---|---|
| Anthropic | Messages | 内容、思考、签名块按顺序原样回传；运行中系统指令不变，剩余预算作为工具结果后的新消息追加，不破坏签名前缀 |
| DeepSeek | Chat Completions | 原始 assistant 消息和 `reasoning_content` 原样回传 |
| OpenAI | Responses | `store:false`，回传原生 output（含加密 reasoning）和 function_call |
| Google Gemini | 官方 OpenAI 兼容接口 | 保留签名扩展 |
| 阿里云百炼 | 兼容接口 | `enable_thinking`；北京 / 新加坡地域要和 Key 一致，可填业务空间 ID |

- 内置模型 Claude Opus 5.5 和 DeepSeek Flash；其他模型在设置里加，思考能力、档位、工具支持要明确填，不按名字猜。
- **每次 HTTP 发送**都走同一个 `request_once`，发送前在授权账本里占一次；没授权的模型拒绝发送；不隐式重试、降档、重定向。超时：连接 20 秒、读取 60 秒、整体 180 秒。
- **Key**：Rust 读钥匙串，IPC 不返回 Key，不进配置、历史和模型上下文；看状态不加载 Key，真要用时才读。保存成功和认证成功分开显示。模拟模式的 Key 只在内存。
- **文件工具**：列目录、读文件、搜索只接受相对路径，拒绝绝对路径、`..`、`.git` 和越界的符号链接；补丁先整批校验再写。
- **命令工具**：`sandbox-exec` + 系统 `dyld-support.sb`，清空继承的环境；只读项目外，能写项目、专用 HOME 和临时目录；默认禁网（装依赖要显式开），默认 60 秒、最长 180 秒。沙箱起不来就不执行，不退回无限制。
- **上限**（v0.6 起界面上只留一个花费上限，其他照存）：单模型默认 12 轮，规划 + 实施 24 轮；整个任务 80 次工具、180,000 上下文字节、500,000 tokens；最多委派 4 项，每项返工 1 次。所有角色共用一份；usage 未知就停下。
- **导出**：调用记录能导出成脱敏 JSON（`schemaVersion=1`、`calls` 数组），不含 Key、绝对路径、提示词、回复、思考、工具参数；未知费用填 `null`。

## 额度与花费

- 现在的 `usage.rs` 和用量页照旧可用：API 记录（按 `source + requestId` 去重，缺 ID 的行无效）、DeepSeek 余额（只在真实 DeepSeek 请求进行中且 Key 已加载时自动查）、五家客户端额度卡（每 15 秒看进程，按来源约 60 秒查一次，客户端退出就停、留最后一次的快照，不推算满额、缺数据不填 0）。
- Claude Code、Antigravity CLI 的额度靠官方状态栏事件：脚本随 App 打包在 `Peck.app/Contents/Resources/integrations/statusline.py`，只存额度、来源进程、账户标识和时间。接入是往 Claude Code 设置里**合并**（不替换整个文件）：
  ```json
  {"statusLine":{"type":"command","command":"/usr/bin/python3 '/Applications/Peck.app/Contents/Resources/integrations/statusline.py' --client claude-code"}}
  ```
  已有状态栏就在命令末尾加 `--forward-original '原有命令'`。
- v0.5 起站主用 Peck 自己开发新的「额度与花费」（`src/quota/`、`src-tauri/src/quota/`），Agent 只留入口和数据接口、不写实现；接上以后旧的 `usage.rs` 和用量页退役，状态栏脚本搬到新模块名下。

## 视觉

- 视觉基线在 visual-atelier 的方案 `schemes/peck/`，按 tag 单向同步进 `src/vendor/peck/`（现在是 `peck-v0.1.0`，`VERSION` 记着提交，`SHA256SUMS` 记校验和），不手改；同步怎么跑在 `CLAUDE.md`「边界」。
- 读取：`src/main.tsx` 依次引 `vendor/peck/peck.css`（变量、两个本地字体）、`vendor/peck/bundle.css`（`pk-` 组件）、`@fontsource/doto` 和 `@fontsource/ibm-plex-sans`（点阵字和说明字，打进包里，App 不联网取字体）、`src/app.css`（外壳怎么摆，只用变量）。根元素的 `data-theme` 由 `src/lib/theme.ts` 写，默认跟随系统。
- 同步来的：`peck.css`（Design System 的变量、字体样式类、两个字体的 @font-face）、`bundle.css`（`pk-` 组件）、`fonts/`（缝合像素 12、Departure Mono 和 OFL 原文）、`tokens.ts`、`marks/`（App 图标、A3、菜单栏模板图）、`DESIGN.md`、`peck-rules.md`、`DESIGN-SYSTEM.md` 和 `components/`（组件说明和示例）。云端开发看不到 visual-atelier，照这些做。

## 打包和签名

- `pnpm app:package` 只打包、校验签名；`pnpm app:install` 打包并装进 /Applications，旧包先备份。`PECK_INSTALL_DIR` 改安装目标，`PECK_BUILD_PROFILE=debug` 打开发包。产物在 `src-tauri/target/release/bundle/macos/Peck.app`，`pnpm clean:build` 会删掉它，Dock 只固定 /Applications 里那份。
- 默认用本机自签名证书 **Peck Self-Signed**（要在钥匙串里、信任代码签名）；`APPLE_SIGNING_IDENTITY` 换别的，设 `-` 是 ad-hoc。只给本机用，不是 Developer ID、没公证。
- 版本号三处一起改：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。

## 代码位置

| 位置 | 职责 |
|---|---|
| `src-tauri/src/{lib,commands,state,events}.rs` | 启动和恢复、IPC、共享状态、推给界面的事件 |
| `src-tauri/src/{run,flow,agent,process,mock,mode}.rs` | 任务、先写规范再实施、CLI 清单、子进程、假 Agent、模式 |
| `src-tauri/src/api/{mod,provider,profiles,keys,budget}.rs` | 统一请求、SSE、模型能力、Key、授权账本 |
| `src-tauri/src/api/{worker,supervisor,tools,storage}.rs` | 单模型循环、规划 + 实施、文件和命令工具、API 记录 |
| `src-tauri/src/{store,migrations}.rs`、`src-tauri/migrations/` | 历史存储和追加迁移 |
| `src-tauri/src/{workspace,snapshot,watch,tasks}.rs` | 项目、任务前快照和撤销、文件监听、`.peck/tasks.md` |
| `src-tauri/src/{files,term,tray}.rs` | 右栏 FILES 的列表和预览、TERM 的 pty、菜单栏图标 |
| `src-tauri/src/{usage,export}.rs`、`src-tauri/integrations/` | 旧的用量监控、原生保存面板、状态栏脚本 |
| `src-tauri/defaults/` | 内置 CLI 清单和流程 |
| `src/App.tsx`、`src/lib/` | 界面编排和状态、类型化 IPC（`lib/ipc.ts` 在浏览器里转给 `?mock`）、设置、事件并进记录 |
| `src/shell/`、`src/ui.tsx`、`src/app.css` | 三栏外壳：顶栏、左栏、中栏、右栏四个页签、设置、额度与花费；`pk-` 组件的小零件；布局样式 |
| `src/dev/` | `?mock` 的假数据和假 IPC |
| `src/vendor/peck/` | 同步来的视觉：样式、字体、图标、设计说明（不手改） |
| `src/quota/`、`src-tauri/src/quota/` | 额度与花费，站主用 Peck 写（v0.5 起） |
