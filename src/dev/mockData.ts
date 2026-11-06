import type {
  AgentInfo,
  DiffResult,
  Message,
  RunRecord,
  Session,
  Workspace,
} from "@/lib/types";

export const MOCK_WORKSPACES: Workspace[] = [
  {
    id: "ws-my-project",
    path: "/Users/elenskye/Documents/GitHub/my-project",
    name: "my-project",
    lastOpenedAt: Date.now() - 120_000,
    gitBranch: "feat/auth-captcha",
    gitignoreConfigured: true,
  },
  {
    id: "ws-peck",
    path: "/Users/elenskye/Documents/GitHub/personal-cli-studio/peck",
    name: "peck",
    lastOpenedAt: Date.now() - 3_600_000,
    gitBranch: "main",
    gitignoreConfigured: true,
  },
  {
    id: "ws-api-gateway",
    path: "/Users/elenskye/Documents/GitHub/edge-gateway",
    name: "edge-gateway",
    lastOpenedAt: Date.now() - 86_400_000,
    gitBranch: "release/v2.4",
    gitignoreConfigured: true,
  },
];

export const MOCK_AGENTS: AgentInfo[] = [
  {
    id: "muse",
    name: "Muse Code",
    description: "根据自然语言生成产品意图文档与任务清单 (.peck/handoff/<run_id>/intent.md & .peck/tasks.md)",
    command: "muse",
    args: ["exec", "{{prompt}}"],
    timeout: "30m",
    available: false,
    diagnostic: "演示：CLI 未安装，可继续使用内置模拟 Agent。",
    isMock: false,
  },
  {
    id: "grok",
    name: "Grok Build",
    description: "读取意图与任务文档，在工作区实施代码变更并实时勾选 .peck/tasks.md",
    command: "grok",
    args: ["-p", "{{prompt}}", "--output-format", "plain", "--no-auto-update"],
    timeout: "45m",
    available: false,
    diagnostic: "演示：CLI 未安装。检测不包含认证与额度验证。",
    isMock: false,
  },
  {
    id: "mock",
    name: "Peck Mock Agent",
    description: "开发测试专用模拟 Agent，模拟 3 秒耗时、文件写入与复选框进度更新",
    command: "peck-mock",
    args: ["--prompt", "{{prompt}}", "--handoff", "{{handoff}}", "--write-files"],
    timeout: "5m",
    available: true,
    resolvedPath: "builtin://mock-runner",
    version: "0.1.0-builtin",
    isMock: true,
  },
];

export const MOCK_SESSIONS: Session[] = [
  {
    id: "sess-104",
    workspaceId: "ws-my-project",
    title: "登录页图形验证码功能",
    createdAt: Date.now() - 600_000,
    updatedAt: Date.now() - 45_000,
  },
  {
    id: "sess-103",
    workspaceId: "ws-my-project",
    title: "重构 JWT 刷新中间件与错误码提示",
    createdAt: Date.now() - 7_200_000,
    updatedAt: Date.now() - 6_800_000,
  },
];

export const MOCK_MESSAGES: Record<string, Message[]> = {
  "sess-104": [
    {
      id: "msg-1",
      sessionId: "sess-104",
      role: "user",
      content: "帮我给登录页加一个图形验证码功能，支持点击刷新、大小写不敏感校验，并在连续输错时自动刷新验证码。",
      createdAt: Date.now() - 320_000,
    },
    {
      id: "msg-2",
      sessionId: "sess-104",
      role: "assistant",
      content: "已启动「想法 → 代码」双步协同流水线（Muse Code → Grok Build）。所有交接文档与代码变更已由影子 Git 记录快照。",
      runId: "run-104",
      createdAt: Date.now() - 318_000,
    },
  ],
  "sess-103": [
    {
      id: "msg-103-1",
      sessionId: "sess-103",
      role: "user",
      content: "@grok 检查 src/utils/token.ts 的过期缓冲时间，改为提前 90 秒刷新。",
      createdAt: Date.now() - 7_200_000,
    },
    {
      id: "msg-103-2",
      sessionId: "sess-103",
      role: "assistant",
      content: "已完成单步执行（@grok），更新了 token 刷新阈值并补充了单元测试。",
      createdAt: Date.now() - 7_150_000,
    },
  ],
};

export const MOCK_RUN_104: RunRecord = {
  id: "run-104",
  number: 104,
  sessionId: "sess-104",
  flowId: "idea-to-code",
  flowName: "想法 → 代码",
  prompt: "帮我给登录页加一个图形验证码功能，支持点击刷新、大小写不敏感校验，并在连续输错时自动刷新验证码。",
  status: "succeeded",
  startedAt: Date.now() - 318_000,
  finishedAt: Date.now() - 305_000,
  summary: "Muse 生成了意图文档与 5 项任务清单；Grok 完成了 Captcha 组件与 Login 页面集成（共变动 4 个文件，+160 -3）。",
  steps: [
    {
      id: "step-intent",
      stepIndex: 0,
      agentId: "muse",
      agentName: "Muse Code",
      title: "意图与任务生成",
      executionMode: "mock",
      notice: "浏览器演示数据，未调用真实 AI。",
      status: "succeeded",
      expects: [".peck/handoff/104/intent.md", ".peck/tasks.md"],
      exitCode: 0,
      durationMs: 4200,
      startedAt: Date.now() - 318_000,
      finishedAt: Date.now() - 313_800,
    },
    {
      id: "step-build",
      stepIndex: 1,
      agentId: "grok",
      agentName: "Grok Build",
      title: "代码构建与实施",
      executionMode: "mock",
      notice: "浏览器演示数据，未调用真实 AI。",
      status: "succeeded",
      expects: [],
      exitCode: 0,
      durationMs: 8600,
      startedAt: Date.now() - 313_800,
      finishedAt: Date.now() - 305_200,
    },
  ],
  tasks: [
    {
      id: "task-1",
      text: "产生物品规格与交互边界说明文档 (.peck/handoff/104/intent.md)",
      completed: true,
    },
    {
      id: "task-2",
      text: "实现 SVG 噪点与随机旋转字符图形验证码组件 (src/components/Captcha.tsx)",
      completed: true,
    },
    {
      id: "task-3",
      text: "在登录页表单中集成 Captcha 组件与验证码输入框 (src/pages/Login.tsx)",
      completed: true,
    },
    {
      id: "task-4",
      text: "支持忽略大小写比对，并在校验失败或提交报错时自动触发验证码刷新",
      completed: true,
    },
    {
      id: "task-5",
      text: "通过 TypeScript 严格模式类型检查与无障碍键盘聚焦校验",
      completed: true,
    },
  ],
  files: [
    {
      path: ".peck/handoff/104/intent.md",
      kind: "create",
      additions: 52,
      deletions: 0,
      stepId: "step-intent",
    },
    {
      path: ".peck/tasks.md",
      kind: "create",
      additions: 12,
      deletions: 0,
      stepId: "step-intent",
    },
    {
      path: "src/components/Captcha.tsx",
      kind: "create",
      additions: 84,
      deletions: 0,
      stepId: "step-build",
    },
    {
      path: "src/pages/Login.tsx",
      kind: "modify",
      additions: 12,
      deletions: 3,
      stepId: "step-build",
    },
  ],
  logs: [
    {
      id: "log-1",
      runId: "run-104",
      stepId: "step-intent",
      agentId: "muse",
      stream: "stdout",
      chunk: "[muse] Initializing workspace context at /Users/elenskye/Documents/GitHub/my-project",
      timestamp: Date.now() - 317_800,
    },
    {
      id: "log-2",
      runId: "run-104",
      stepId: "step-intent",
      agentId: "muse",
      stream: "stdout",
      chunk: "[muse] Analyzing existing auth flow in src/pages/Login.tsx ...",
      timestamp: Date.now() - 316_500,
    },
    {
      id: "log-3",
      runId: "run-104",
      stepId: "step-intent",
      agentId: "muse",
      stream: "stdout",
      chunk: "[muse] Wrote product specification -> .peck/handoff/104/intent.md (52 lines)",
      timestamp: Date.now() - 314_500,
    },
    {
      id: "log-4",
      runId: "run-104",
      stepId: "step-intent",
      agentId: "muse",
      stream: "stdout",
      chunk: "[muse] Wrote implementation checklist -> .peck/tasks.md (5 tasks)",
      timestamp: Date.now() - 313_900,
    },
    {
      id: "log-5",
      runId: "run-104",
      stepId: "step-build",
      agentId: "grok",
      stream: "stdout",
      chunk: "[grok] Reading handoff spec .peck/handoff/104/intent.md and .peck/tasks.md",
      timestamp: Date.now() - 313_400,
    },
    {
      id: "log-6",
      runId: "run-104",
      stepId: "step-build",
      agentId: "grok",
      stream: "stdout",
      chunk: "[grok] Created src/components/Captcha.tsx (+84 -0)",
      timestamp: Date.now() - 309_500,
    },
    {
      id: "log-7",
      runId: "run-104",
      stepId: "step-build",
      agentId: "grok",
      stream: "stderr",
      chunk: "[grok:warn] Detected legacy autofill attribute on Login form, normalized to autoComplete=\"off\" for captcha input",
      timestamp: Date.now() - 307_800,
    },
    {
      id: "log-8",
      runId: "run-104",
      stepId: "step-build",
      agentId: "grok",
      stream: "stdout",
      chunk: "[grok] Patched src/pages/Login.tsx (+12 -3) and marked 5/5 tasks completed in .peck/tasks.md",
      timestamp: Date.now() - 305_300,
    },
  ],
};

const INTENT_MD_CONTENT = `# 登录页图形验证码 (Captcha) 设计与实现规范

> **Run ID**: \`#104\` · **生成引擎**: \`Muse Code\` · **目标工作区**: \`my-project\`

## 1. 背景与目标
为防止针对 \`/api/auth/login\` 的自动化撞库请求，需要在前端登录表单中引入轻量、高辨识度的 SVG 图形验证码组件，在不增加外部第三方闭源 SDK 依赖的前提下完成人机校验闭环。

## 2. 交互与视觉要求
- **字符集规避歧义**：仅使用 \`23456789ABCDEFGHJKLMNPQRSTUVWXYZ\`（剔除易混淆的 \`0/O\`、\`1/I/l\`）。
- **防 OCR 干扰层**：
  - 4 位随机字符独立旋转（角度范围 \`-18° ~ +18°\`）；
  - 3 条贝塞尔干扰曲线 + 18 个微透噪点圆斑；
  - 适配暗色主题（背景 \`#14171C\`，描边 \`#262B34\`，高亮字符使用青蓝渐变）。
- **刷新机制**：
  1. 用户点击验证码画布或「换一张」按钮立即重新生成；
  2. 提交时若验证码不匹配（忽略大小写），阻止表单请求并立即刷新验证码。

## 3. 涉及文件清单
| 文件路径 | 变更类型 | 说明 |
|---|---|---|
| \`src/components/Captcha.tsx\` | 新增 | 封装 SVG 验证码渲染、刷新回调与受控校验 |
| \`src/pages/Login.tsx\` | 修改 | 接入 \`Captcha\` 组件，增加 \`captchaInput\` 状态与提交拦截 |
`;

const TASKS_MD_CONTENT = `# Peck 任务执行清单 (.peck/tasks.md)

- [x] 产生物品规格与交互边界说明文档 (.peck/handoff/104/intent.md)
- [x] 实现 SVG 噪点与随机旋转字符图形验证码组件 (src/components/Captcha.tsx)
- [x] 在登录页表单中集成 Captcha 组件与验证码输入框 (src/pages/Login.tsx)
- [x] 支持忽略大小写比对，并在校验失败或提交报错时自动触发验证码刷新
- [x] 通过 TypeScript 严格模式类型检查与无障碍键盘聚焦校验
`;

export const MOCK_DIFFS: Record<string, DiffResult> = {
  ".peck/handoff/104/intent.md": {
    runId: "run-104",
    stepId: "step-intent",
    filePath: ".peck/handoff/104/intent.md",
    kind: "create",
    language: "markdown",
    additions: 52,
    deletions: 0,
    contentAfter: INTENT_MD_CONTENT,
    unifiedDiff: `diff --git a/.peck/handoff/104/intent.md b/.peck/handoff/104/intent.md
new file mode 100644
index 0000000..8f4e2a1
--- /dev/null
+++ b/.peck/handoff/104/intent.md
@@ -0,0 +1,24 @@
+# 登录页图形验证码 (Captcha) 设计与实现规范
+
+> **Run ID**: \`#104\` · **生成引擎**: \`Muse Code\` · **目标工作区**: \`my-project\`
+
+## 1. 背景与目标
+为防止针对 \`/api/auth/login\` 的自动化撞库请求，需要在前端登录表单中引入轻量、高辨识度的 SVG 图形验证码组件。
+
+## 2. 交互与视觉要求
+- **字符集规避歧义**：仅使用 \`23456789ABCDEFGHJKLMNPQRSTUVWXYZ\`（剔除易混淆的 \`0/O\`、\`1/I/l\`）。
+- **防 OCR 干扰层**：
+  - 4 位随机字符独立旋转（角度范围 \`-18° ~ +18°\`）；
+  - 3 条贝塞尔干扰曲线 + 18 个微透噪点圆斑；
+  - 适配暗色主题（背景 \`#14171C\`，描边 \`#262B34\`）。
+- **刷新机制**：
+  1. 用户点击验证码画布或「换一张」按钮立即重新生成；
+  2. 提交时若验证码不匹配（忽略大小写），阻止表单请求并立即刷新验证码。
+
+## 3. 涉及文件清单
+| 文件路径 | 变更类型 | 说明 |
+|---|---|---|
+| \`src/components/Captcha.tsx\` | 新增 | 封装 SVG 验证码渲染、刷新回调与受控校验 |
+| \`src/pages/Login.tsx\` | 修改 | 接入 \`Captcha\` 组件，增加 \`captchaInput\` 状态与提交拦截 |`,
  },
  ".peck/tasks.md": {
    runId: "run-104",
    stepId: "step-intent",
    filePath: ".peck/tasks.md",
    kind: "create",
    language: "markdown",
    additions: 12,
    deletions: 0,
    contentAfter: TASKS_MD_CONTENT,
    unifiedDiff: `diff --git a/.peck/tasks.md b/.peck/tasks.md
new file mode 100644
index 0000000..41c920b
--- /dev/null
+++ b/.peck/tasks.md
@@ -0,0 +1,7 @@
+# Peck 任务执行清单 (.peck/tasks.md)
+
+- [x] 产生物品规格与交互边界说明文档 (.peck/handoff/104/intent.md)
+- [x] 实现 SVG 噪点与随机旋转字符图形验证码组件 (src/components/Captcha.tsx)
+- [x] 在登录页表单中集成 Captcha 组件与验证码输入框 (src/pages/Login.tsx)
+- [x] 支持忽略大小写比对，并在校验失败或提交报错时自动触发验证码刷新
+- [x] 通过 TypeScript 严格模式类型检查与无障碍键盘聚焦校验`,
  },
  "src/components/Captcha.tsx": {
    runId: "run-104",
    stepId: "step-build",
    filePath: "src/components/Captcha.tsx",
    kind: "create",
    language: "typescript",
    additions: 84,
    deletions: 0,
    unifiedDiff: `diff --git a/src/components/Captcha.tsx b/src/components/Captcha.tsx
new file mode 100644
index 0000000..c92b71e
--- /dev/null
+++ b/src/components/Captcha.tsx
@@ -0,0 +1,46 @@
+import React, { useCallback, useEffect, useState } from "react";
+
+const CHARSET = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
+
+export interface CaptchaProps {
+  length?: number;
+  refreshSeed?: number;
+  onCodeChange: (code: string) => void;
+}
+
+export const Captcha: React.FC<CaptchaProps> = ({
+  length = 4,
+  refreshSeed = 0,
+  onCodeChange,
+}) => {
+  const [chars, setChars] = useState<string[]>([]);
+
+  const regenerate = useCallback(() => {
+    const next = Array.from({ length }, () => {
+      const idx = Math.floor(Math.random() * CHARSET.length);
+      return CHARSET[idx];
+    });
+    setChars(next);
+    onCodeChange(next.join(""));
+  }, [length, onCodeChange]);
+
+  useEffect(() => {
+    regenerate();
+  }, [regenerate, refreshSeed]);
+
+  return (
+    <button
+      type="button"
+      onClick={regenerate}
+      title="点击刷新验证码"
+      className="inline-flex items-center justify-center rounded-md border border-[#262B34] bg-[#14171C] px-3 py-1.5 hover:border-[#00E5FF]/60 transition-colors"
+    >
+      <svg width="96" height="32" viewBox="0 0 96 32" aria-label="图形验证码">
+        {chars.map((ch, idx) => (
+          <text
+            key={idx}
+            x={14 + idx * 20}
+            y={22}
+            fill="#00E5FF"
+            fontSize="18"
+            fontFamily="JetBrains Mono, monospace"
+          >
+            {ch}
+          </text>
+        ))}
+      </svg>
+    </button>
+  );
+};`,
  },
  "src/pages/Login.tsx": {
    runId: "run-104",
    stepId: "step-build",
    filePath: "src/pages/Login.tsx",
    kind: "modify",
    language: "typescript",
    additions: 12,
    deletions: 3,
    unifiedDiff: `diff --git a/src/pages/Login.tsx b/src/pages/Login.tsx
index 51a9d02..9e84b11 100644
--- a/src/pages/Login.tsx
+++ b/src/pages/Login.tsx
@@ -1,8 +1,11 @@
 import React, { useState } from "react";
 import { loginWithCredentials } from "../api/auth";
+import { Captcha } from "../components/Captcha";
 
 export const Login: React.FC = () => {
   const [email, setEmail] = useState("");
   const [password, setPassword] = useState("");
+  const [captchaExpected, setCaptchaExpected] = useState("");
+  const [captchaInput, setCaptchaInput] = useState("");
+  const [captchaSeed, setCaptchaSeed] = useState(0);
   const [error, setError] = useState<string | null>(null);
 
   const handleSubmit = async (e: React.FormEvent) => {
     e.preventDefault();
-    setError(null);
-    await loginWithCredentials({ email, password });
+    if (captchaInput.trim().toUpperCase() !== captchaExpected.toUpperCase()) {
+      setError("图形验证码不正确，请重新输入");
+      setCaptchaSeed((s) => s + 1);
+      return;
+    }
+    setError(null);
+    await loginWithCredentials({ email, password, captcha: captchaInput });
   };
 
   return (
@@ -26,6 +29,12 @@ export const Login: React.FC = () => {
         onChange={(e) => setPassword(e.target.value)}
         placeholder="登录密码"
       />
+      <div className="flex items-center gap-2">
+        <input
+          value={captchaInput}
+          onChange={(e) => setCaptchaInput(e.target.value)}
+          placeholder="输入 4 位验证码"
+          autoComplete="off"
+        />
+        <Captcha refreshSeed={captchaSeed} onCodeChange={setCaptchaExpected} />
+      </div>
       {error && <p className="text-red-400 text-xs">{error}</p>}
       <button type="submit">安全登录</button>
     </form>`,
  },
};
