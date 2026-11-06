# Peck 业务需求与功能定义 (Requirements)

## 1. 产品背景与核心价值
在本地同时使用多个 AI CLI（例如擅长产品意图拆解的 **Muse Wonder** 与擅长代码实施的 **Grok Build**）时，传统的终端切换或复杂的 stdout/stdin 管道拼接既脆弱又难以直观审查文件改动。

Peck 的核心定位是：**「自然语言发令枪 + 文件变动看板」**。
- **不搬运内存管道**：CLI 之间通过工作区内的标准文件（`.peck/handoff/<run_id>/intent.md` 与 `.peck/tasks.md`）交接。
- **聚焦产物与变更**：用户发出一句自然语言指令后，右侧看板实时呈现步骤时间线、任务复选框完成度、产物 Markdown 预览与代码 Diff，而将嘈杂的终端 Stdout/Stderr 默认折叠收起。

---

## 2. 功能模块清单

### 2.1 工作区管理 (Workspace)
- 支持打开本地任意项目目录作为当前工作区，并保存最近打开列表。
- 打开工作区时自动检查 `.gitignore`，若未包含 `.peck/` 则自动追加，防止交接产物污染用户 Git 提交。

### 2.2 自然语言发令与路由 (Dispatch)
- **默认多步流水线 (`idea-to-code`)**：
  1. **Step 1 (`muse`)**：根据用户输入生成 `.peck/handoff/<run_id>/intent.md`（需求规范）与 `.peck/tasks.md`（Markdown 任务复选框清单）。
  2. **Step 2 (`grok`)**：读取上述两份文件，在工作区实施代码修改，并逐项勾选 `.peck/tasks.md` 中的 `- [x]`。
- **单步直达 (`@agent`)**：
  - 用户输入以 `@muse`、`@grok` 或 `@mock` 开头时，直接单步调用对应 Agent。
- **中断与重试**：
  - 支持随时点击「停止」终止当前正在运行的子进程组。
  - 若某一步骤非 0 退出或缺失预期文件，流水线自动暂停，支持「重试此步骤」。

### 2.3 右侧文件变动看板 (ArtifactBoard)
- **步骤时间线 (`StepTimeline`)**：展示每个步骤的状态（等待中/运行中/成功/失败/已取消）、执行耗时与预期产物检查结果。
- **任务清单进度 (`TaskChecklist`)**：实时解析 `.peck/tasks.md`，展示进度条与完成百分比（如 `3/5 (60%)`）。
- **变动文件清单 (`ChangeList`)**：展示当前 Run 或选中 Step 产生的所有新增/修改/删除文件及 `+added / -removed` 行数统计。
- **文件预览与 Diff (`FilePreview`)**：
  - `.md` 文件支持高保真 Markdown 渲染与源码/Diff 切换；
  - 代码文件支持 **Unified Diff（逐行）** 与 **Split Diff（双栏对比）** 切换。
- **原始输出抽屉 (`RawOutput`)**：默认收起，展开后支持按 Stdout / Stderr 过滤、复制与自动滚屏。
