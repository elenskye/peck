# Peck

> **自然语言发令枪 + 文件变动看板** · macOS 原生桌面应用 (Tauri 2.0 + React 19 + Tailwind CSS v4)

Peck 是一个极简、现代、克制的本地多 CLI 协同桌面工作台。它**不充当复杂的双 CLI 管道搬运工**，而是让所有本地 Agent CLI（如 **Muse Code**、**Grok Build** 及自定义 CLI）直接围绕本地工作区文件（`.peck/handoff/<run_id>/intent.md`、`.peck/tasks.md` 与项目源码）进行协同。

---

## 核心特性

1. **自然语言发令 (Dispatch)**
   - 支持预设多步流水线（如默认 `idea-to-code`：Step 1 Muse 生成意图文档与任务清单 $\to$ Step 2 Grok 读取规范并实施代码构建）。
   - 支持 `@agent` 单步直达语法（如 `@muse`、`@grok`、`@mock`）。
   - 输入框可选择模拟 Agent、任意单 Agent 或双步流水线；不要求同时订阅两家。
2. **无侵入观察 (Observe)**
   - 自动维护工作区 `.gitignore`（追加 `.peck/` 防止污染用户 Git 仓库）。
   - 基于**影子 Git 仓库 (Shadow Git)** 在每步前后生成毫秒级文件树快照（`git write-tree`），绝不触碰用户仓库的 Index、Commit 或 Stash。
3. **高保真看板呈现 (Present)**
   - **步骤时间线 (`StepTimeline`)**：实时追踪多步状态、耗时与失败重试。
   - **任务复选框进度 (`TaskChecklist`)**：实时解析 `.peck/tasks.md` 中的 `- [ ]` / `- [x]` 任务进度。
   - **文件变动与 Diff (`ChangeList` / `FilePreview`)**：支持 Markdown 规范渲染与 Unified / Split 双模式高亮代码 Diff。
   - **底层原始输出 (`RawOutput`)**：默认折叠收起，需要排查时一键展开查看 Stdout / Stderr 终端流。

---

## 快速开始

### 环境要求
- **系统**：macOS (Apple Silicon M 系列推荐)
- **Node.js**：v24+
- **包管理器**：`pnpm` (`~/Library/pnpm/bin`)
- **Rust**：stable 工具链 (`~/.cargo/bin`)

### 常用命令

```bash
# 安装依赖
pnpm install

# 启动前端开发服务器 (固定端口 1430，浏览器可访问 http://localhost:1430/?mock)
pnpm dev

# 启动 Tauri 桌面端开发模式
pnpm app:dev

# TypeScript 类型检查
pnpm check

# 构建并安装到 /Applications/Peck.app
pnpm app:install

# 仅打包 Release .app（本机 ad-hoc 签名，无需自签名证书）
pnpm app:package

# 检查并清理超过 20GB 的 Rust 编译缓存
pnpm clean:build
```

---

## 浏览器 `?mock` 预览模式

在未启动 Tauri 桌面外壳或尚未安装真实 CLI 时，可直接运行 `pnpm dev` 并在浏览器访问：

```text
http://localhost:1430/?mock
```

该模式会自动加载 `src/dev/mockIpc.ts` 与高仿真测试数据，完整支持：
- 切换工作区与历史会话；
- 在底部输入框发送新指令，实时观看 Step 1 (Muse) $\to$ Step 2 (Grok) 的流式推进、任务勾选与文件 Diff 生成；
- 切换查看 `intent.md`、`.peck/tasks.md` 及代码文件的 Unified / Split Diff。

---

浏览器演示使用内存数据，刷新会重置；真实 SQLite 历史由桌面端保存。首次打开项目流程可用 `http://localhost:1430/?mock&empty` 预览。

## 文档导航

- [v0.2 API Supervisor / Worker 架构建议（草案）](./docs/v0.2-architecture.md)
- [最新开发交接与验证状态 (`HANDOFF.md`)](./HANDOFF.md)
- [架构设计与分阶段实施蓝图 (`peck_architecture_plan.md`)](./peck_architecture_plan.md)
- [编码规范与架构准则 (`CLAUDE.md`)](./CLAUDE.md)
- [开发里程碑路线图 (`docs/roadmap.md`)](./docs/roadmap.md)
- [业务需求与功能定义 (`docs/requirements.md`)](./docs/requirements.md)
- [架构设计与影子 Git (`docs/architecture.md`)](./docs/architecture.md)
- [视觉与交互设计规范 (`docs/design.md`)](./docs/design.md)
- [测试与验证指南 (`docs/testing.md`)](./docs/testing.md)
- [CLI 接入与无订阅开发 (`docs/cli-integration.md`)](./docs/cli-integration.md)
- [本机打包、安装与历史数据 (`docs/packaging.md`)](./docs/packaging.md)
