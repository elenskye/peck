# Peck 项目开发交接

更新日期：2026-11-06（Asia/Singapore）。长期方案见 [实施蓝图](./peck_architecture_plan.md)，开发约束见 [AGENTS.md](./AGENTS.md) 和 [CLAUDE.md](./CLAUDE.md)。

## 最新方向：v0.1 暂停，v0.2 方案讨论

- 用户明确要求暂放 v0.1，待订阅 Grok / Muse 后继续真实接入验收；不把暂停状态当成 v0.1 全部完成。
- v0.2 用户目标：多供应商 API、Supervisor / Worker 分工、API Key 与模型/思考配置、Grok / Muse CLI 委派 API Worker。
- 本轮已核对现有源码及官方工具调用、思考参数和 MCP 接口，整理 [v0.2 架构建议](./docs/v0.2-architecture.md)。该文档为设计草案，尚未开始 API 运行时代码开发，也未进行真实 API 调用。
- 核心建议：由 Peck 统一编排，角色与模型分开；复用现有界面、SQLite 与影子 Git，新增 API 工具循环和根 Run 内子任务。先做单 Worker 开发闭环，再接 Supervisor，CLI MCP 兼容后置，不阻塞 API 路线。
- 已核对的参数差异：Opus 5.5 思考始终开启，DeepSeek Flash 支持开关且档位不同；界面必须依据模型/端点能力展示设置。
- 本次仅修改开发文档。13 份文档相对链接检查通过，v0.1 暂停与 v0.2 尚未实现的草案状态已明确记录；未运行与文档修改无关的编译或测试。

## 接手约定

- 接手先读本文件。**每次完成开发（含代码、配置、开发文档）必须在汇报前更新本文件**，记录完成内容、实际验证、遗留问题和下一步；区分实现、自动测试和真实服务验收。
- 仅在 `personal-cli-studio/` 内开发；`~/Documents/GitHub/pomelo` 只读。使用 `pnpm`，Vite 端口固定 `1430`。
- `personal-cli-studio/` 与 `peck/` 当前均不是 Git 仓库；未初始化仓库，也未提交或推送。后续未经用户明确授权不执行 `git commit` / `git push`。

## v0.1 保留状态

Phase 0 ~ Phase 3 核心代码已落地；**Phase 4 无订阅可完成的接入配置、诊断与模拟链路，以及 Phase 5 的 SQLite 历史与本机打包，现已实现**。真实 Muse/Grok 调用与原生窗口交互仍单独待验，不能称为全部验收完成。

**无需同时订阅两家，也无需先订阅才能继续开发。** 可使用内置模拟 Agent；只拥有一家 CLI 的有效访问条件时，可在输入框选择该单 Agent 或输入 `@muse` / `@grok`，将完整任务交给它。默认双步流程是可选编排，不是两份订阅的要求。Muse Wonder 是模型系列，CLI 工具是 Muse Code。

## 本次完成：Phase 4

- 修复 `AgentsDialog`、顶栏和 Agent 菜单的状态提示，区分未安装、内置模拟和“已检测到 CLI · 认证未验证”；不再将外部 CLI 统一标成绿色就绪。
- 默认入口改为 `muse exec {{prompt}}` 与 `grok -p {{prompt}} --output-format plain --no-auto-update`；官方依据与权限要求见 [CLI 接入说明](./docs/cli-integration.md)。未安装真实 CLI，尚无实际 `--help` 输出或认证验收结果。
- 针对旧版内置参数的精确匹配进行迁移，保留其他配置值；自定义 command/args 不替换。设置中可编辑 TOML、校验并保存、重新检测；下次运行生效。
- 可选择模拟 Agent、任意单 Agent 或双步流水线；没有外部 CLI 时默认选择模拟。
- 新增 `StepExecution` 事件：真实调用或模拟执行在步骤看板、运行卡片中显示，且随历史保存。
- 找不到可执行文件时明确降级模拟；已安装 CLI 的认证、额度、权限或进程错误会失败暂停，不改为模拟成功。缺少 `expects` 产物时也会失败暂停。
- 显式 `@mock` 现在也包含 `--write-files`，会写交接规范和任务清单；模拟只验证 Peck，不执行真实 AI 推理。
- 新增首次打开项目页面、原生文件夹选择入口及路径输入，移除本机项目路径硬编码。路径支持 `~/`；运行按会话所属工作区执行，同一工作区不并发运行。

## 本次完成：Phase 5

- `store.rs` 接入 rusqlite，建立 `workspaces`、`sessions`、`runs`、`steps`、`messages` 五张表。
- 实际保存最近工作区、会话、用户/调度消息、运行编号、任务、文件列表、原始日志、步骤状态与耗时、执行计划及影子 Git 快照索引。
- IPC 与 React 历史加载贯通，`session_get_runs` 不再返回空数组；消息时间戳使用实际毫秒时间。运行完成和重试后重新加载历史。
- 数据库与影子 Git 对象共同支持历史 Diff 恢复，不用当前文件冒充历史产出。失败或中止的运行可在重开后从未完成步骤重试。
- 启动时将上次退出时仍在运行的任务标记为中断失败，供人工重试；不自动重新调用 AI。
- `PECK_DATA_DIR` 可隔离应用数据，`PECK_SHADOW_ROOT` 可隔离影子 Git；无需重设 HOME。默认 Debug 数据在 `Peck Dev`，Release 数据在 `Peck`。
- 本机签名默认使用 ad-hoc（`-`），不再强制创建 `Peck Self-Signed` 证书；仍可用 `APPLE_SIGNING_IDENTITY` 指定已有证书。
- `pnpm app:package` 只打包；`pnpm app:install` 才安装，替换前保留旧版本备份。打包不自动删除 Rust 缓存，两个脚本均支持只读 `--check`。
- 最终 Release 产物：`src-tauri/target/release/bundle/macos/Peck.app`。Debug 为较早的验证产物，后续使用 Debug 时请重新构建。详见 [打包与数据说明](./docs/packaging.md)。

## 关键实现位置

- 后端：`agent.rs` / `commands.rs`（配置与探测）、`run.rs` / `events.rs`（执行模式与事件）、`store.rs` / `snapshot.rs`（历史与差异）、`state.rs` / `lib.rs`（运行上下文与初始化）。
- 前端：`App.tsx`、`useWorkspace.ts`、`useSession.ts`、`useRunEvents.ts`、`AgentsDialog.tsx`、`Composer.tsx`、`StepTimeline.tsx`、`RunCard.tsx`。
- 浏览器演示：`?mock` 为示例历史；`?mock&empty` 为首次打开工作区。浏览器模拟只存内存，刷新重置；真实 SQLite 在桌面端运行。

## 实际验证

- `pnpm check`、`pnpm build`：通过；Release 打包过程也包含前端生产构建。
- `PECK_DATA_DIR=/private/tmp/peck-phase45-data PECK_SHADOW_ROOT=/private/tmp/peck-phase45-shadow cargo test --offline --manifest-path src-tauri/Cargo.toml`：30 项测试通过，退出码 0。
- 新增测试验证：旧模板升级保留自定义环境变量、自定义参数不覆盖、缺失 CLI 的模拟标记、缺产物失败、数据库重开后历史/原始 Diff 恢复、中断任务识别、失败后重开续跑且不重跑成功步骤。
- `bash -n scripts/install-app.sh scripts/clean-build.sh`、两份脚本的 `--check`：通过；未清理缓存。
- Debug 与 Release `.app` 均已实际生成，`codesign --verify --deep --strict` 通过。Release 包约 4.5 MiB。
- 安装流程已在 `/private/tmp/peck-phase45-install/` 实测：保留旧包备份并安装，原测试文件仍在备份中；未安装或覆盖 `/Applications/Peck.app`。
- 原生 Debug 应用已用独立临时数据目录启动并生成数据库；电脑使用工具报 ScreenCaptureKit 错误 `-3811`，无法完成原生窗口交互验收。此限制不影响上述编译、打包和后端自动测试。
- 使用 Playwright 对浏览器生产构建实测：首次路径打开项目、未安装/模拟状态、Grok 配置编辑保存、单步模拟完成至 3/3、双步模拟完成至 2/2、Markdown 与分栏 Diff、停止后重试均通过；生产页面控制台 0 errors / 0 warnings。截图保存在 `output/playwright/`。
- 12 份开发文档的相对链接与计划文档位置检查通过。验证中遇到的热更新临时错误已修复；最终检查使用生产预览，无热更新依赖。

## v0.1 尚未验收（暂停期间不继续实施）

1. 在本机人工检查首次选目录、原生发令、停止/重试、退出重开后的历史与 Diff。自动测试已经覆盖后端恢复，但不替代原生交互验收。
2. 具备任一服务访问条件后，先分别验收单 Agent：版本/帮助、认证、非交互权限、文件产出、错误提示；需要双步协作时再验收双 Agent。具体账户权限与费用由服务渠道决定，不要求同时订阅。
3. 向其他用户公开分发前，再配置 Developer ID 签名、公证凭据并验收；当前是本机 ad-hoc 签名，未公证，不宣称公开发布完成。
4. 备份历史时需同时保留应用数据目录中的数据库和 `shadow/`。不在真实源码项目中用模拟运行探测能力，以免改写 `.peck/tasks.md`。

前一轮已完成的计划文档迁移与强制交接规则继续有效。本次未安装外部 CLI、购买订阅、发起付费模型调用或修改钥匙串。
