# Peck v0.1 开发阶段路线图 (Roadmap)

> **版本安排（2026-11-06）**：用户要求暂放 v0.1，待订阅 Grok / Muse 后继续验收。v0.2 优先讨论 API Supervisor / Worker 方案，实施阶段建议见 [v0.2 架构文档](./v0.2-architecture.md)；以下仍保留 v0.1 的阶段状态，不代表 v0.2 已实现。

> **当前进度**：Phase 0 ~ Phase 3 核心实现已落地；Phase 4 的离线接入与配置、Phase 5 的持久化与本机打包已实现。真实服务调用、原生交互与公开分发另行验收，详见 [HANDOFF.md](../HANDOFF.md)。无需同时订阅 Muse 和 Grok。

| 阶段 | 阶段名称 | 核心交付模块 | 状态 |
|---|---|---|---|
| **Phase 0** | 项目脚手架与双栏暗色框架 | Vite + React 19 + Tailwind v4（端口 1430）、Tauri Overlay 顶栏、`TitleBar` + `SplitPane` + 双栏看板、`?mock` 预览模式 | ✅ 已完成 |
| **Phase 1** | Agent 清单体系与进程调度核心 | `agent.rs` (TOML + PATH `which`/`--version` 探测)、`process.rs` (流式捕获 + 超时 + `pgid` 进程组终止)、`mock.rs` (3s 模拟 Agent) | ✅ 已完成 |
| **Phase 2** | Flow 编排引擎与统一事件流 | `flow.rs` (`idea-to-code` + `@agent` 路由)、`run.rs` (多步顺序执行 + 失败自动暂停 + 步骤重试)、`events.rs` (`Channel<RunEvent>` 桥接) | ✅ 已完成 |
| **Phase 3** | 文件监听、影子 Git 与变动看板 | `workspace.rs` (`.gitignore` 自动注入 `.peck/`)、`snapshot.rs` (Shadow Git `write-tree` 快照与 Unified Diff)、`watch.rs`、`tasks.rs` | ✅ 已完成 |
| **Phase 4** | CLI 接入配置与端到端验证 | 官方非交互参数、旧模板升级、配置编辑/重新检测、单 Agent 入口、模拟标记与失败暂停 | ✅ 离线部分完成；真实服务联调待验 |
| **Phase 5** | 持久化与本机打包 | SQLite 五张表、历史/快照/重试恢复、ad-hoc 打包、备份旧包后安装、缓存只读检查 | ✅ 核心实现完成；原生交互与对外公证待验 |

---

## 常用验证命令速查

```bash
# 1. 前端 TypeScript 严格检查 (在 peck/ 下)
pnpm check

# 2. Rust 后端全量单元测试 (支持离线沙箱执行)
cargo test --offline --manifest-path src-tauri/Cargo.toml

# 3. 浏览器 ?mock 预览模式 (固定端口 1430)
pnpm dev
# 访问 http://localhost:1430/?mock
```
