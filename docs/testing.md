# Peck 测试与验证指南 (Testing Guide)

## 1. 静态类型与构建校验

在 `peck/` 目录下执行：

```bash
# TypeScript 严格类型检查
pnpm check

# 前端生产包构建校验
pnpm build
```

---

## 2. 浏览器 `?mock` 模式全链路验证

无需启动 Rust 编译或安装真实 CLI，即可在浏览器中完整验证前端 UI 与流式事件状态机：

1. 启动开发服务器（固定端口 `1430`）：
   ```bash
   pnpm dev
   ```
2. 在浏览器中打开：
   ```text
   http://localhost:1430/?mock
   ```
3. 验证检查项：
   - **视觉与布局**：深灰阶暗色主题（`#0F1115` / `#14171C` / `#181B21` / `#262B34` / `#00E5FF`）准确应用；左右分栏分割线可顺滑拖拽调整宽度。
   - **顶栏交互**：点击左上角工作区选择器可切换示例项目；点击右上角「Agents 就绪」可弹出 Agent 清单探测对话框。
   - **预置 Run 审查**：
     - 左侧展示对话消息与 `Run #104` 摘要卡片；
     - 右侧展示 `StepTimeline`（Step 1 Muse 与 Step 2 Grok）、`TaskChecklist`（任务勾选进度）、`ChangeList`（`.peck/handoff/104/intent.md`、`src/components/Captcha.tsx`、`src/pages/Login.tsx` 等）；
     - 点击文件可在 **Markdown 渲染预览**、**Unified Diff** 与 **Split Diff** 之间切换；
     - 点击底部「底层 CLI 原始输出」可展开/收起 Stdout/Stderr 面板并切换过滤。
   - **交互式模拟发令**：
     - 在左侧底部输入框输入任意需求（或使用 `@muse` / `@grok` 前缀），按下 `⌘ + Enter` 发送；
     - 观察右侧看板实时流式推进 Step 1 $\to$ Step 2、任务复选框动态勾选、新增文件自动浮现。

---

## 3. Rust 后端与桌面端验证

```bash
# 检查 Rust 编译与单元测试
cd src-tauri && cargo check && cargo test

# 启动 Tauri 原生桌面窗口
pnpm app:dev
```

## 4. Phase 4 / 5 验证

```bash
# 使用临时目录隔离测试数据与影子仓库，不修改 HOME
PECK_DATA_DIR=/private/tmp/peck-test-data PECK_SHADOW_ROOT=/private/tmp/peck-test-shadow cargo test --offline --manifest-path src-tauri/Cargo.toml

# 安装脚本与缓存检查（不删除文件、不写入系统安装目录）
bash -n scripts/install-app.sh scripts/clean-build.sh
bash scripts/install-app.sh --check
bash scripts/clean-build.sh --check

# 本机 Release 打包与签名校验
pnpm app:package
```

新增自动测试覆盖旧模板升级保留自定义环境变量、缺失 CLI 的明确模拟事件、缺少预期产物时失败、SQLite 重开后的消息/任务/日志恢复、原始快照差异读取、异常退出识别，以及失败后从后续步骤恢复且不重跑已成功步骤。

浏览器可访问 `?mock&empty` 检查首次打开项目；普通 `?mock` 检查未安装/模拟状态、配置编辑、单步与双步模拟、停止/重试及文件预览。浏览器演示数据不写入真实 SQLite。

真实 CLI 认证、付费调用和 macOS 文件夹选择器/原生窗口交互需单独验收。历史恢复自动测试不等于已人工验证原生应用退出重开。
