# Peck 视觉与交互设计规范 (Design System)

## 1. 设计语言与气质
Peck 视觉风格对齐 **Antigravity / Codex** 现代极客桌面工具：
- **克制深灰阶暗色**：不使用纯黑 (`#000000`)，通过细腻的深灰明度差区分「主画布 - 侧栏 - 卡片 - 浮层」四个层级。
- **高信噪比极客强调色**：使用 `#00E5FF` (Cyan/Teal) 作为核心交互焦点与运行态微光点缀。
- **信息分层**：将用户最关心的「步骤进度、任务完成度、文档与代码 Diff」置于右侧主视野，将底层 CLI 原始终端日志默认收起在底部抽屉。

---

## 2. 色彩令牌 (Color Tokens)

| 语义角色 | CSS 变量 / Token | 色值 | 用途说明 |
|---|---|---|---|
| 主背景 | `--bg-app` | `#0F1115` | 顶栏、右侧看板底层画布、终端区底色 |
| 侧边栏/对话面板 | `--bg-panel` | `#14171C` | 左侧自然语言交互区背景 |
| 卡片/容器面板 | `--bg-card` | `#181B21` | 步骤卡片、Run 摘要卡片、文件预览区背景 |
| 浮层/下拉菜单 | `--bg-elevated` | `#1E222A` | 工作区切换下拉框、弹窗、悬浮按钮 |
| 边框与分割线 | `--border-subtle` | `#262B34` | 面板分割线、卡片描边 |
| 品牌强调色 | `--accent` | `#00E5FF` | 主按钮、激活态指示条、焦点高亮 |
| 强调色悬停 | `--accent-hover` | `#33EBFF` | 强调按钮 Hover 状态 |
| 强调色微透底 | `--accent-muted` | `rgba(0, 229, 255, 0.12)` | 选中项背景、徽章底色 |
| 状态：成功 | `--status-success` | `#10B981` | 步骤完成、新增代码行 (`+`) |
| 状态：运行中 | `--status-running` | `#3B82F6` | 正在执行的步骤、呼吸指示灯 |
| 状态：警告 | `--status-warning` | `#F59E0B` | 预期产物缺失提醒、Mock 模式标识 |
| 状态：失败 | `--status-danger` | `#EF4444` | 步骤异常退出、删除代码行 (`-`) |

---

## 3. 字体与排版 (Typography)

- **界面字体 (`--font-sans`)**：`system-ui, -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif`
- **代码/Diff/终端字体 (`--font-mono`)**：`"JetBrains Mono", "SF Mono", Menlo, Monaco, Consolas, monospace`

---

## 4. 窗口与布局尺寸

- **macOS Overlay 顶栏**：高度 `44px`，红绿灯位于 `(x: 18, y: 27.5)`，左侧留出 `78px` 安全区内边距，顶栏空白处支持 `-webkit-app-region: drag` 拖拽窗口。
- **双栏可拖拽分割 (`SplitPane`)**：
  - 左侧 `ChatPanel` 默认宽度 `420px`（最小 `320px`，最大 `640px`）。
  - 右侧 `ArtifactBoard` 自适应剩余全部宽度，内部划分为：顶部步骤时间线 + 任务清单进度，中部「文件变动列表 + 产物预览/Diff 视图」，底部可折叠原始输出抽屉。
