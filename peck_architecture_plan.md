# Peck v0.1 — 架构设计与分阶段执行蓝图 (精简交接版)

> **自然语言发令枪 + 文件变动看板** · macOS 原生桌面应用 · Tauri 2.0 + React 19 + Tailwind CSS v4  
> **新会话快速交接说明**：Phase 0 ~ Phase 3 核心实现已落地。接手先读 [HANDOFF.md](./HANDOFF.md) 获取最新验证与遗留问题，再参考第 6 节继续开发。Phase 4 的真实服务联调与 Phase 5 的主体开发无需串行等待；没有 Muse/Grok 订阅也可推进配置诊断、模拟验证和持久化。

---

## 1. 核心定位与边界铁律

### 1.1 核心定位
Peck **绝不充当复杂的“双 CLI 内存管道搬运工”**。所有本地 Agent CLI（`Muse Wonder`、`Grok Build` 及未来扩展 CLI）均围绕本地工作区文件（`.peck/handoff/<run_id>/intent.md`、`.peck/tasks.md` 与项目源码）进行文件级协同：
1. **发令 (Dispatch)**：解析自然语言为默认多步流水线（`idea-to-code`：Muse $\to$ Grok）或 `@agent` 单步直达，顺序调度子进程。
2. **观察 (Observe)**：监听工作区变动（遵循 `.gitignore` 但强制包含 `.peck/`），每步前后通过**影子 Git 裸仓库**生成毫秒级快照与 Unified Diff。
3. **呈现 (Present)**：右侧看板聚焦展示步骤时间线、任务复选框进度、Markdown 预览与 Unified/Split Diff；底层 Stdout/Stderr 默认折叠收起。

### 1.2 严守边界与环境规范
- **工作目录**：仅允许在 `personal-cli-studio/` 及其子目录 `personal-cli-studio/peck/` 内读写。
- **隔离 Pomelo**：`~/Documents/GitHub/pomelo` **绝对只读，严禁任何修改**。
- **工具链与端口**：
  - 包管理器：`pnpm`（`~/Library/pnpm/bin`）；Node：`~/.nvm/versions/node/v24.x/bin`；Rust：`~/.cargo/bin`（沙箱内离线测试可使用 `cargo test --offline`）。
  - 开发端口：Vite 固定为 **`1430`**（避开 1420）。
- **Git 准则**：未经用户明确授权，不执行 `git commit` 或 `git push`。

---

## 2. 核心机制规范

### 2.1 Agent 清单 (`agents/*.toml`) 与 Flow 编排 (`flows/*.toml`)
- **内置/默认清单位置**：`peck/src-tauri/defaults/agents/{muse,grok,mock}.toml` 与 `defaults/flows/idea-to-code.toml`，启动时自动同步至 `<app_data_dir>/agents/` 与 `<app_data_dir>/flows/`。
- **模板变量**：
  - Flow 层替换 `{{input}}`、`{{handoff}}`、`{{workspace}}`；
  - Agent 层替换 `{{prompt}}`、`{{handoff}}`、`{{workspace}}`。
- **中断与恢复规则**：前序步骤退出码非 0 或超时，流水线立即暂停在 `Failed` 状态，不触发后续步骤；支持在左侧 `RunCard` 或右侧 `StepTimeline` 点击「重试此步骤」从断点继续执行。

### 2.2 影子 Git 快照与 Diff 算法 (Phase 3 核心)
Peck **绝不修改用户仓库的 `.git/index`、Commit 历史或 Stash**：
1. **影子仓库路径**：`~/Library/Application Support/Peck/shadow/<workspace_md5>/`（开发模式为 `Peck Dev/shadow/...`），通过 `git init --bare` 初始化。
2. **快照生成**：
   - 使用环境变量 `GIT_DIR=<shadow_path> GIT_WORK_TREE=<workspace_path>`
   - 执行 `git add -A`（并确保 `.peck/` 被包含）$\to$ `git write-tree` $\to$ `git commit-tree <tree_id> -m "snapshot"`。
3. **Diff 计算**：
   - Step $k$ 的改动为 `git diff <tree_{k-1}> <tree_k>`，输出标准 Unified Diff 供前端解析染色。

---

## 3. 数据模型与 IPC 契约

### 3.1 SQLite Schema (`peck.db`, 已落地)
包含 5 张核心表：`workspaces`、`sessions`、`runs`、`steps`、`messages`。

### 3.2 统一流式事件 (`RunEvent`, 已在 `events.rs` 实现)
通过 `tauri::ipc::Channel<RunEvent>` 下发：`RunStarted`、`StepStarted`、`StepOutput`、`TasksUpdated`、`FileTouched`、`StepFinished`、`RunFinished`。

### 3.3 IPC Command 清单 (`commands.rs` & `src/lib/ipc.ts`)
| Command | 参数 | 返回值 | 当前状态 |
|---|---|---|---|
| `workspace_open` | `{ path }` | `Workspace` | ✅ Phase 3 已实现 Git 分支探测与 `.gitignore` 自动注入 `.peck/` |
| `workspace_list_recent` | `{}` | `Workspace[]` | ✅ 已接入 SQLite 最近工作区列表 |
| `session_create` / `session_list` / `session_get_messages` | `{ workspaceId / sessionId }` | `Session / Message[]` | ✅ 已接入 SQLite |
| `session_get_runs` | `{ sessionId }` | `RunRecord[]` | ✅ 恢复历史运行、步骤、任务、文件列表与日志 |
| `workspace_choose` | `{}` | `Workspace?` | ✅ 原生文件夹选择入口；交互待验 |
| `agent_get_config` / `agent_save_config` | `{ agentId, content? }` | `string / void` | ✅ 清单编辑与校验，保存后下次运行生效 |
| `run_dispatch` | `{ sessionId, prompt, channel }` | `string (runId)` | ✅ Phase 2/3 已实现流式多步调度 + 影子快照 + 文件监听 |
| `run_cancel` | `{ runId }` | `void` | ✅ Phase 1/2 已实现进程组终止 |
| `run_retry_step` | `{ runId, stepId, channel }` | `void` | ✅ Phase 2/3 已实现断点重试继续与快照更新 |
| `changes_get_diff` | `{ runId, stepId?, filePath? }` | `DiffResult` | ✅ Phase 3 已接入真实 Shadow Git 快照与 Unified Diff |
| `agents_list` | `{}` | `AgentInfo[]` | ✅ Phase 1 已实现 PATH 探测与版本读取 |

---

## 4. 视觉令牌 (Antigravity / Codex 暗色规范)
- **深灰底色层级**：主背景 `#0F1115` · 对话侧栏 `#14171C` · 卡片面板 `#181B21` · 浮层 `#1E222A` · 边框 `#262B34`
- **极客强调色**：Accent `#00E5FF` · Hover `#33EBFF` · Muted `rgba(0,229,255,0.12)`
- **状态色**：成功 `#10B981` · 运行中 `#3B82F6` · 警告 `#F59E0B` · 失败 `#EF4444`
- **窗口融合**：macOS `Overlay` 顶栏，红绿灯坐标 `(x: 18, y: 27.5)`。

---

## 5. 已完成阶段回顾 (Phase 0 ~ Phase 3)

- **✅ Phase 0：项目脚手架与双栏暗色框架**
  - 完成 `peck/` 下 Vite + React 19 + TypeScript + Tailwind v4 初始化（固定端口 `1430`）。
  - 完成 `TitleBar`、`SplitPane`、`ChatPanel`、`ArtifactBoard`（含 `StepTimeline`、`TaskChecklist`、`ChangeList`、`FilePreview`、`MarkdownView`、`DiffView`、`RawOutput`）及浏览器 `?mock` 模式。
- **✅ Phase 1：Agent 清单体系与进程调度核心**
  - 完成 `src-tauri/src/agent.rs`（TOML 解析、系统 PATH 补全、`which` 与 `--version` 探测）。
  - 完成 `src-tauri/src/process.rs`（`tokio::process` 流式 Stdout/Stderr 捕获、超时控制、Unix `process_group(0)` 进程组安全终止）与 `src-tauri/src/mock.rs`（内置 3 秒模拟 Agent）。
- **✅ Phase 2：Flow 编排引擎与统一事件流**
  - 完成 `src-tauri/src/flow.rs`（`idea-to-code.toml` 加载、`@agent` 单步路由与多步模板渲染）。
  - 完成 `src-tauri/src/run.rs` 与 `events.rs`（多步顺序状态机、失败自动暂停、断点步骤重试、预期产物检查、未安装外部 CLI 时自动降级至 `peck-mock`）。
- **✅ Phase 3：文件监听、影子 Git 与变动看板**
  - 完成 `src-tauri/src/workspace.rs`（打开工作区时探测 Git 分支，幂等将 `.peck/` 追加写入工作区 `.gitignore`）。
  - 完成 `src-tauri/src/snapshot.rs`（在 `<app_data_dir>/shadow/<workspace_md5>/` 初始化裸仓库 `git init --bare`，在 Run 与每个 Step 前后调用 `git add -A` + 强制包含 `.peck/` + `git write-tree` 生成快照，并通过 `git diff <tree_a> <tree_b>` 实现 `changes_get_diff` 真实返回）。
  - 完成 `src-tauri/src/watch.rs`（Step 运行期间实时监听工作区文件变动，遵循 `.gitignore` 但强制包含 `.peck/`，实时推送 `FileTouched`）。
  - 完成 `src-tauri/src/tasks.rs`（独立封装 `.peck/tasks.md` 复选框解析器，忽略代码块干扰，实时推送 `TasksUpdated`）。

---

## 6. Phase 4 ~ Phase 5 实施与验收

### Phase 4：CLI 接入配置与端到端验证
- **目标**：提供 Muse Wonder 与 Grok Build 真实 CLI 接入规范与降级容错。
- **任务清单**：
  1. 在 `peck/docs/cli-integration.md` 建立 CLI 接入说明文档，记录官方接口依据、`--help` 输出与二进制路径配置；项目文档统一收在 `peck/` 内。
  2. 完善 `muse.toml` 和 `grok.toml` 参数映射与环境变量透传，支持在前端 `AgentsDialog` 展示详细探测诊断信息。
  3. 无订阅时先完成模拟执行、配置与错误提示验证；真实 CLI 的认证、额度、非交互调用与文件产出另行验收，不将模拟成功计为真实接入完成。

**2026-11-06 状态**：上述离线部分已实现；新增单 Agent 选择、TOML 编辑、旧模板升级与步骤模拟标记。真实服务验收尚未完成，具体接入依据见 [CLI 接入说明](./docs/cli-integration.md)。

### Phase 5：持久化存储与本机打包
- **目标**：SQLite 会话历史持久化与 macOS 应用签名打包。
- **任务清单**：
  1. 实现 `src-tauri/src/store.rs`：集成 SQLite 保存 `workspaces`、`sessions`、`runs`、`steps`、`messages`。
  2. 验证 `scripts/install-app.sh` 的本机 ad-hoc 签名、只打包/显式安装模式与旧包备份；自定义证书通过 `APPLE_SIGNING_IDENTITY` 指定，不强制创建自签名证书。
  3. 验证 `scripts/clean-build.sh --check` 的只读检查；公开分发签名与公证具备条件后单独验收。

**2026-11-06 状态**：SQLite 五张核心表、事件记录、历史差异索引、重开恢复与重试已实现；本机 Debug/Release .app 可打包并验证。签名安装范围与后续验收见 [打包说明](./docs/packaging.md)，最新结果见 [HANDOFF.md](./HANDOFF.md)。
