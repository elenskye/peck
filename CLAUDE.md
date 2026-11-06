# CLAUDE.md — Peck v0.1 架构准则与交接地图

> **当前任务方向（2026-11-06）**：按用户要求暂停 v0.1，待订阅后继续真实 CLI 验收。v0.2 的 API Supervisor / Worker 方案见 [架构建议](./docs/v0.2-architecture.md)，目前为设计草案，尚未实现；本轮不修改下述 v0.1 运行代码。后续实施 v0.2 时需同步更新旧架构边界与接口说明。

> **当前开发进度 (Peck v0.1)**：
> - ✅ **Phase 0**（脚手架、双栏暗色 UI、`?mock` 模式）已完成
> - ✅ **Phase 1**（`agent.rs` 清单探测、`process.rs` 流式子进程与进程组终止、`mock.rs`）已完成
> - ✅ **Phase 2**（`flow.rs` 路由、`run.rs` 多步状态机与失败暂停/重试、`events.rs` Channel 桥接）已完成
> - ✅ **Phase 3**（`workspace.rs` `.gitignore` 注入、`snapshot.rs` 影子 Git 快照与 Diff、`watch.rs` 文件监听、`tasks.rs` 复选框解析）已完成
> - ✅ **Phase 4 离线部分**：官方非交互参数、旧模板升级、配置编辑与诊断、单 Agent 入口、明确模拟状态已实现；真实 Muse/Grok 服务联调待验。
> - ✅ **Phase 5 核心部分**：SQLite 历史保存、快照索引恢复、重开后重试、本机 ad-hoc 打包已实现；原生交互与对外签名公证单独待验。
> - 最新交接与验证状态以 [HANDOFF.md](./HANDOFF.md) 为入口；实施蓝图见 [peck_architecture_plan.md](./peck_architecture_plan.md)。

---

## 1. 铁律与边界原则 (Strict Rules)

1. **目录边界**：
   - 仅在 `personal-cli-studio/` 及其子目录 `personal-cli-studio/peck/` 内开发。
   - **绝对禁止修改** `~/Documents/GitHub/pomelo`（仅只读参考）。
2. **构建工具与端口**：
   - 前端包管理器固定使用 `pnpm`；Vite 端口固定为 **`1430`**（严禁使用 `1420`）。
   - Rust 测试在沙箱内可直接运行：`/Users/elenskye/.cargo/bin/cargo test --offline --manifest-path /Users/elenskye/Documents/GitHub/personal-cli-studio/peck/src-tauri/Cargo.toml`。
3. **架构初心**：
   - Peck 是**「自然语言发令枪 + 文件变动看板」**，通过本地工作区文件（`.peck/handoff/<run_id>/intent.md` 与 `.peck/tasks.md`）完成多 CLI 交接。
   - 影子 Git 只在 `<app_data_dir>/shadow/<workspace_md5>/` 中使用底层 plumbing 命令生成快照，绝不污染用户仓库的 Index、Commit 或 Stash。
4. **强制交接**：每次完成开发（含代码、配置与开发文档）必须在汇报前更新根目录 `HANDOFF.md`，记录完成内容、实际验证、遗留问题与下一步；未更新不得视为完成。

---

## 2. 视觉设计令牌 (Antigravity / Codex Dark)

- **背景色阶**：`--bg-app: #0F1115` · `--bg-panel: #14171C` · `--bg-card: #181B21` · `--bg-elevated: #1E222A` · `--border-subtle: #262B34`
- **强调色与状态色**：`--accent: #00E5FF` (`#33EBFF`) · 成功 `#10B981` · 运行中 `#3B82F6` · 警告 `#F59E0B` · 失败 `#EF4444`
- **窗口融合**：macOS `titleBarStyle: "Overlay"`，红绿灯坐标 `(x: 18, y: 27.5)`。

---

## 3. 代码地图

### Rust 后端 (`src-tauri/src/`)
- `lib.rs` / `commands.rs` / `state.rs`：Tauri 入口、工作区/会话/运行/Agent 配置 IPC；SQLite Store 与运行中任务上下文。
- `agent.rs`：解析 `agents/*.toml`，补全系统 `PATH`，执行 `which` 与 `--version` 探测（含 4 项单测）。
- `process.rs`：`tokio::process` 异步流式捕获 Stdout/Stderr、超时控制、Unix `process_group(0)` 整组终止（含 5 项单测）。
- `mock.rs`：内置 `peck-mock` 模拟执行器（默认 3 秒三阶段流式日志输出与产物写入）。
- `flow.rs`：解析 `flows/idea-to-code.toml`，支持 `@agent` 单步直达路由与多步模板渲染（含 3 项单测）。
- `run.rs` / `events.rs`：多步顺序执行引擎、失败自动暂停、断点步骤重试与 `EventSink`（含 2 项单测）。
- `workspace.rs` / `snapshot.rs` / `watch.rs` / `tasks.rs`：Phase 3 的工作区、影子 Git、文件监听与任务解析实现。
- `store.rs`：SQLite 五张核心表、运行事件保存、消息/日志/任务/步骤恢复、异常退出识别与快照索引保存。

### React 前端 (`src/`)
- `App.tsx`：主界面编排，连接 `useWorkspace`、`useSession`、`useRunEvents` 与 `ipc.ts`。
- `lib/ipc.ts` & `dev/mockIpc.ts`：自动识别 Tauri 环境与浏览器 `?mock` 模式。
- `components/layout/`：`TitleBar.tsx`、`SplitPane.tsx`（默认左侧 420px）。
- `components/chat/`：`ChatPanel.tsx`、`MessageList.tsx`、`RunCard.tsx`（含失败步骤重试按钮）、`Composer.tsx`（`⌘↩` 发送、`@agent` 切换）。
- `components/board/`：`ArtifactBoard.tsx`、`StepTimeline.tsx`、`TaskChecklist.tsx`、`ChangeList.tsx`、`FilePreview.tsx`、`MarkdownView.tsx`、`DiffView.tsx`、`RawOutput.tsx`。
