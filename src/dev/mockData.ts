// `?mock` 的假数据：项目列表、每个项目最近几次任务、在跑的和完成的、CLI 模式和 API 模式各有。
// 只在浏览器里用，刷新重置。用词照 docs/design.md。
import type { ApiStatus, ApiTask, ModelProfile, RunLimits } from "@/lib/api";
import type {
  AgentInfo,
  DiffResult,
  Message,
  RunRecord,
  Session,
  Workspace,
} from "@/lib/types";

const NOW = Date.now();
const MIN = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

// ---------- 项目 ----------

export const MOCK_WORKSPACES: Workspace[] = [
  {
    id: "ws-notebook",
    path: "/Users/me/Documents/GitHub/notebook",
    name: "notebook",
    lastOpenedAt: NOW - 2 * MIN,
    gitBranch: "feat/search",
    gitignoreConfigured: true,
  },
  {
    id: "ws-peck",
    path: "/Users/me/Documents/GitHub/personal-cli-studio/peck",
    name: "peck",
    lastOpenedAt: NOW - 1 * HOUR,
    gitBranch: "main",
    gitignoreConfigured: true,
  },
  {
    id: "ws-gateway",
    path: "/Users/me/Documents/GitHub/edge-gateway",
    name: "edge-gateway",
    lastOpenedAt: NOW - 1 * DAY,
    gitBranch: "release/v2.4",
    gitignoreConfigured: false,
  },
];

// ---------- CLI ----------

export const MOCK_AGENTS: AgentInfo[] = [
  {
    id: "muse",
    name: "Muse",
    description: "写规范（意图和任务清单）；也能单个跑完整个任务",
    command: "cli/muse/muse",
    args: ["exec", "--trust-workspace", "--max-model-steps", "40", "{{prompt}}"],
    timeout: "30m",
    available: true,
    version: "Muse Code 1.4.14 (演示)",
    diagnostic: "浏览器演示：没有检测本机 CLI，这里当作检测到了。",
    isMock: false,
  },
  {
    id: "grok",
    name: "Grok",
    description: "按规范实施，边改边勾任务清单；也能单个跑完整个任务",
    command: "cli/grok/grok",
    // 和站主正式清单一样长，看设置里一行放不放得下。
    args: ["-p", "{{prompt}}", "--output-format", "plain", "--no-auto-update", "--sandbox", "workspace", "--permission-mode", "acceptEdits", "--max-turns", "40", "--no-subagents", "--rules", "Use file edit tools for implementation.", "--allow", "Read", "--allow", "Grep", "--allow", "Edit", "--allow", "Bash(cargo check*)", "--deny", "Bash(git push*)"],
    timeout: "45m",
    available: true,
    version: "grok 1.0.77 (演示)",
    diagnostic: "浏览器演示：没有检测本机 CLI，这里当作检测到了。",
    isMock: false,
  },
  {
    id: "mock",
    name: "假 Agent",
    description: "开发者选项里的假 Agent：不调用 AI，几秒跑完并写演示文件",
    command: "peck-mock",
    args: ["--prompt", "{{prompt}}", "--handoff", "{{handoff}}", "--write-files"],
    timeout: "5m",
    available: true,
    resolvedPath: "builtin://mock-runner",
    version: "0.1.0-builtin",
    isMock: true,
  },
];

// ---------- 任务（一次对话就是一个任务） ----------

export const MOCK_SESSIONS: Session[] = [
  // notebook：在跑一个，完成两个
  { id: "task-12", workspaceId: "ws-notebook", title: "笔记列表加搜索框", createdAt: NOW - 6 * MIN, updatedAt: NOW - 20_000 },
  { id: "task-11", workspaceId: "ws-notebook", title: "日期统一成 YYYY-MM-DD", createdAt: NOW - 3 * HOUR, updatedAt: NOW - 3 * HOUR + 4 * MIN },
  { id: "task-10", workspaceId: "ws-notebook", title: "导出 Markdown 时保留标签", createdAt: NOW - 1 * DAY, updatedAt: NOW - 1 * DAY + 9 * MIN },
  // peck：一个 API 规划 + 实施完成，一个 CLI 失败
  { id: "task-9", workspaceId: "ws-peck", title: "设置面板加通知开关", createdAt: NOW - 1 * HOUR - 30 * MIN, updatedAt: NOW - 1 * HOUR },
  { id: "task-8", workspaceId: "ws-peck", title: "菜单栏图标在跑时变样", createdAt: NOW - 5 * HOUR, updatedAt: NOW - 5 * HOUR + 2 * MIN },
  // edge-gateway：一个旧的完成
  { id: "task-7", workspaceId: "ws-gateway", title: "限流中间件按路由配置", createdAt: NOW - 2 * DAY, updatedAt: NOW - 2 * DAY + 11 * MIN },
];

// ---------- 对话 ----------

export const MOCK_MESSAGES: Record<string, Message[]> = {
  "task-12": [
    { id: "m-12-1", sessionId: "task-12", role: "user", content: "给笔记列表顶部加一个搜索框，按标题和正文模糊匹配，输入时实时过滤，清空后恢复全部。", createdAt: NOW - 6 * MIN },
    { id: "m-12-2", sessionId: "task-12", role: "assistant", content: "交给 Muse → Grok：Muse 先写规范，Grok 按规范实施。改动在右栏。", runId: "run-12", createdAt: NOW - 6 * MIN + 2_000 },
  ],
  "task-11": [
    { id: "m-11-1", sessionId: "task-11", role: "user", content: "把界面里所有日期显示统一成 YYYY-MM-DD，用一个共用的格式化函数。", createdAt: NOW - 3 * HOUR },
    { id: "m-11-2", sessionId: "task-11", role: "assistant", content: "交给 Grok 单个跑完整个任务。", runId: "run-11", createdAt: NOW - 3 * HOUR + 2_000 },
  ],
  "task-10": [
    { id: "m-10-1", sessionId: "task-10", role: "user", content: "导出 Markdown 的时候把笔记的标签写进文件头（YAML front matter）。", createdAt: NOW - 1 * DAY },
    { id: "m-10-2", sessionId: "task-10", role: "assistant", content: "交给 API 模式 · 单模型干活（DeepSeek Flash）。", runId: "run-10", createdAt: NOW - 1 * DAY + 2_000 },
  ],
  "task-9": [
    { id: "m-9-1", sessionId: "task-9", role: "user", content: "设置里加一个「通知」开关，默认开，关了以后任务完成不发 macOS 通知。", createdAt: NOW - 1 * HOUR - 30 * MIN },
    { id: "m-9-2", sessionId: "task-9", role: "assistant", content: "交给 API 模式 · 规划 + 实施（规划 Claude Opus 5.5，实施 DeepSeek Flash）。", runId: "run-9", createdAt: NOW - 1 * HOUR - 30 * MIN + 2_000 },
  ],
  "task-8": [
    { id: "m-8-1", sessionId: "task-8", role: "user", content: "有任务在跑的时候把菜单栏小图标换成带点的那个。", createdAt: NOW - 5 * HOUR },
    { id: "m-8-2", sessionId: "task-8", role: "assistant", content: "交给 Grok 单个跑完整个任务。", runId: "run-8", createdAt: NOW - 5 * HOUR + 2_000 },
  ],
  "task-7": [
    { id: "m-7-1", sessionId: "task-7", role: "user", content: "限流中间件改成可以按路由单独配置阈值，没配的用全局默认。", createdAt: NOW - 2 * DAY },
    { id: "m-7-2", sessionId: "task-7", role: "assistant", content: "交给 Muse → Grok：Muse 先写规范，Grok 按规范实施。", runId: "run-7", createdAt: NOW - 2 * DAY + 2_000 },
  ],
};

// ---------- 任务记录 ----------

const DEMO_NOTICE = "浏览器演示数据，没有调用真实 AI。";

const run12: RunRecord = {
  id: "run-12",
  number: 12,
  sessionId: "task-12",
  flowId: "muse-grok",
  flowName: "Muse → Grok",
  prompt: MOCK_MESSAGES["task-12"][0].content,
  status: "running",
  startedAt: NOW - 6 * MIN,
  steps: [
    { id: "run-12-spec", stepIndex: 0, agentId: "muse", agentName: "Muse", title: "写规范", executionMode: "mock", notice: DEMO_NOTICE, status: "succeeded", expects: [".peck/handoff/12/intent.md", ".peck/tasks.md"], exitCode: 0, durationMs: 38_000, startedAt: NOW - 6 * MIN, finishedAt: NOW - 6 * MIN + 38_000 },
    { id: "run-12-build", stepIndex: 1, agentId: "grok", agentName: "Grok", title: "实施", executionMode: "mock", notice: DEMO_NOTICE, status: "running", expects: [], startedAt: NOW - 5 * MIN },
  ],
  tasks: [
    { id: "run-12-t1", text: "写规范（.peck/handoff/12/intent.md）", completed: true },
    { id: "run-12-t2", text: "新建 SearchBox 组件（src/components/SearchBox.tsx）", completed: true },
    { id: "run-12-t3", text: "笔记列表接上搜索框，按标题和正文过滤", completed: false },
    { id: "run-12-t4", text: "清空输入恢复全部，补一个用例", completed: false },
  ],
  files: [
    { path: ".peck/handoff/12/intent.md", kind: "create", additions: 31, deletions: 0, stepId: "run-12-spec" },
    { path: ".peck/tasks.md", kind: "create", additions: 6, deletions: 0, stepId: "run-12-spec" },
    { path: "src/components/SearchBox.tsx", kind: "create", additions: 42, deletions: 0, stepId: "run-12-build" },
    { path: "src/pages/NoteList.tsx", kind: "modify", additions: 0, deletions: 0, stepId: "run-12-build" },
  ],
  logs: [
    { id: "l-12-1", runId: "run-12", stepId: "run-12-spec", agentId: "muse", stream: "stdout", chunk: "[muse] 读项目结构：src/pages/NoteList.tsx、src/store/notes.ts", timestamp: NOW - 6 * MIN + 3_000 },
    { id: "l-12-2", runId: "run-12", stepId: "run-12-spec", agentId: "muse", stream: "stdout", chunk: "[muse] 写规范 → .peck/handoff/12/intent.md（31 行）", timestamp: NOW - 6 * MIN + 30_000 },
    { id: "l-12-3", runId: "run-12", stepId: "run-12-spec", agentId: "muse", stream: "stdout", chunk: "[muse] 写任务清单 → .peck/tasks.md（4 条）", timestamp: NOW - 6 * MIN + 36_000 },
    { id: "l-12-4", runId: "run-12", stepId: "run-12-build", agentId: "grok", stream: "stdout", chunk: "[grok] 读规范 .peck/handoff/12/intent.md", timestamp: NOW - 5 * MIN },
    { id: "l-12-5", runId: "run-12", stepId: "run-12-build", agentId: "grok", stream: "stdout", chunk: "[grok] 新建 src/components/SearchBox.tsx（+42）", timestamp: NOW - 3 * MIN },
    { id: "l-12-6", runId: "run-12", stepId: "run-12-build", agentId: "grok", stream: "stdout", chunk: "[grok] 正在改 src/pages/NoteList.tsx …", timestamp: NOW - 20_000 },
  ],
};

const run11: RunRecord = {
  id: "run-11",
  number: 11,
  sessionId: "task-11",
  flowId: "single-grok",
  flowName: "Grok",
  prompt: MOCK_MESSAGES["task-11"][0].content,
  status: "succeeded",
  startedAt: NOW - 3 * HOUR + 2_000,
  finishedAt: NOW - 3 * HOUR + 4 * MIN,
  summary: "新增 formatDate，替换了 2 处日期显示。验证：pnpm check 通过。",
  steps: [
    { id: "run-11-build", stepIndex: 0, agentId: "grok", agentName: "Grok", title: "跑完整个任务", executionMode: "mock", notice: DEMO_NOTICE, status: "succeeded", expects: [], exitCode: 0, durationMs: 226_000, startedAt: NOW - 3 * HOUR + 2_000, finishedAt: NOW - 3 * HOUR + 4 * MIN },
  ],
  tasks: [
    { id: "run-11-t1", text: "新增 formatDate(date) → YYYY-MM-DD（src/lib/date.ts）", completed: true },
    { id: "run-11-t2", text: "笔记列表和详情页的日期改用 formatDate", completed: true },
    { id: "run-11-t3", text: "pnpm check 通过", completed: true },
  ],
  files: [
    { path: "src/lib/date.ts", kind: "create", additions: 9, deletions: 0, stepId: "run-11-build" },
    { path: "src/pages/NoteList.tsx", kind: "modify", additions: 2, deletions: 1, stepId: "run-11-build" },
    { path: "src/pages/NoteDetail.tsx", kind: "modify", additions: 2, deletions: 1, stepId: "run-11-build" },
  ],
  logs: [
    { id: "l-11-1", runId: "run-11", stepId: "run-11-build", agentId: "grok", stream: "stdout", chunk: "[grok] 找到 2 处 toLocaleDateString()", timestamp: NOW - 3 * HOUR + 10_000 },
    { id: "l-11-2", runId: "run-11", stepId: "run-11-build", agentId: "grok", stream: "stdout", chunk: "[grok] 新建 src/lib/date.ts（+9）", timestamp: NOW - 3 * HOUR + 60_000 },
    { id: "l-11-3", runId: "run-11", stepId: "run-11-build", agentId: "grok", stream: "stdout", chunk: "[grok] 改 src/pages/NoteList.tsx（+2 -1）、src/pages/NoteDetail.tsx（+2 -1）", timestamp: NOW - 3 * HOUR + 150_000 },
    { id: "l-11-4", runId: "run-11", stepId: "run-11-build", agentId: "grok", stream: "stdout", chunk: "[grok] pnpm check … 通过", timestamp: NOW - 3 * HOUR + 220_000 },
  ],
};

const run10: RunRecord = {
  id: "run-10",
  number: 10,
  sessionId: "task-10",
  flowId: "api-worker",
  flowName: "API 模式 · 单模型干活 · DeepSeek Flash",
  prompt: MOCK_MESSAGES["task-10"][0].content,
  status: "succeeded",
  startedAt: NOW - 1 * DAY + 2_000,
  finishedAt: NOW - 1 * DAY + 9 * MIN,
  summary: "导出时写入 tags 字段，补了一个单元测试。验证：vitest 3 个用例通过。",
  steps: [
    { id: "run-10-worker", stepIndex: 0, agentId: "api-worker", agentName: "DeepSeek Flash", title: "单模型干活", executionMode: "api-mock", notice: DEMO_NOTICE, status: "succeeded", expects: [], exitCode: 0, durationMs: 7 * MIN, startedAt: NOW - 1 * DAY + 2_000, finishedAt: NOW - 1 * DAY + 9 * MIN },
  ],
  tasks: [
    { id: "run-10-t1", text: "exportMarkdown 写入 front matter 的 tags", completed: true },
    { id: "run-10-t2", text: "补 export.test.ts 用例", completed: true },
  ],
  files: [
    { path: "src/lib/export.ts", kind: "modify", additions: 7, deletions: 1, stepId: "run-10-worker" },
    { path: "src/lib/export.test.ts", kind: "modify", additions: 11, deletions: 0, stepId: "run-10-worker" },
  ],
  logs: [
    { id: "l-10-1", runId: "run-10", stepId: "run-10-worker", agentId: "api-worker", stream: "stdout", chunk: "第 1 轮 · read_file src/lib/export.ts", timestamp: NOW - 1 * DAY + 5_000 },
    { id: "l-10-2", runId: "run-10", stepId: "run-10-worker", agentId: "api-worker", stream: "stdout", chunk: "第 2 轮 · write_file src/lib/export.ts", timestamp: NOW - 1 * DAY + 90_000 },
    { id: "l-10-3", runId: "run-10", stepId: "run-10-worker", agentId: "api-worker", stream: "stdout", chunk: "第 3 轮 · write_file src/lib/export.test.ts", timestamp: NOW - 1 * DAY + 200_000 },
    { id: "l-10-4", runId: "run-10", stepId: "run-10-worker", agentId: "api-worker", stream: "stdout", chunk: "第 4 轮 · run_command pnpm vitest run src/lib/export.test.ts → 3 passed", timestamp: NOW - 1 * DAY + 400_000 },
  ],
};

const run9: RunRecord = {
  id: "run-9",
  number: 9,
  sessionId: "task-9",
  flowId: "api-supervisor",
  flowName: "API 模式 · 规划 + 实施 · Claude Opus 5.5 / DeepSeek Flash",
  prompt: MOCK_MESSAGES["task-9"][0].content,
  status: "succeeded",
  startedAt: NOW - 1 * HOUR - 30 * MIN + 2_000,
  finishedAt: NOW - 1 * HOUR,
  summary: "规划拆成 1 个子任务，实施一次通过。改了设置存储和通知发送两处。验证：cargo test 通过。",
  steps: [
    { id: "run-9-plan", stepIndex: 0, agentId: "api-supervisor", agentName: "Claude Opus 5.5", title: "规划和审查", executionMode: "api-mock", notice: DEMO_NOTICE, status: "succeeded", expects: [], exitCode: 0, durationMs: 28 * MIN, startedAt: NOW - 1 * HOUR - 30 * MIN + 2_000, finishedAt: NOW - 1 * HOUR },
    { id: "run-9-impl", stepIndex: 1, agentId: "api-worker", agentName: "DeepSeek Flash", title: "实施：通知开关", executionMode: "api-mock", notice: DEMO_NOTICE, status: "succeeded", expects: [], exitCode: 0, durationMs: 12 * MIN, startedAt: NOW - 1 * HOUR - 20 * MIN, finishedAt: NOW - 1 * HOUR - 8 * MIN },
  ],
  tasks: [
    { id: "run-9-t1", text: "设置里加 notifications 字段，默认 true", completed: true },
    { id: "run-9-t2", text: "发通知前读这个开关", completed: true },
    { id: "run-9-t3", text: "cargo test 通过", completed: true },
  ],
  files: [
    { path: "src-tauri/src/settings.rs", kind: "modify", additions: 6, deletions: 0, stepId: "run-9-impl" },
    { path: "src-tauri/src/notify.rs", kind: "modify", additions: 3, deletions: 1, stepId: "run-9-impl" },
  ],
  logs: [
    { id: "l-9-1", runId: "run-9", stepId: "run-9-plan", agentId: "api-supervisor", stream: "stdout", chunk: "规划：1 个子任务「通知开关」，写入范围 src-tauri/src/settings.rs、src-tauri/src/notify.rs", timestamp: NOW - 1 * HOUR - 25 * MIN },
    { id: "l-9-2", runId: "run-9", stepId: "run-9-impl", agentId: "api-worker", stream: "stdout", chunk: "第 1 轮 · read_file src-tauri/src/settings.rs", timestamp: NOW - 1 * HOUR - 19 * MIN },
    { id: "l-9-3", runId: "run-9", stepId: "run-9-impl", agentId: "api-worker", stream: "stdout", chunk: "第 3 轮 · run_command cargo test → ok", timestamp: NOW - 1 * HOUR - 9 * MIN },
    { id: "l-9-4", runId: "run-9", stepId: "run-9-plan", agentId: "api-supervisor", stream: "stdout", chunk: "审查：通过。开关默认开，关了不发通知，测试覆盖两种情况。", timestamp: NOW - 1 * HOUR - 2 * MIN },
  ],
};

const run8: RunRecord = {
  id: "run-8",
  number: 8,
  sessionId: "task-8",
  flowId: "single-grok",
  flowName: "Grok",
  prompt: MOCK_MESSAGES["task-8"][0].content,
  status: "failed",
  startedAt: NOW - 5 * HOUR + 2_000,
  finishedAt: NOW - 5 * HOUR + 2 * MIN,
  summary: "Grok 登录过期，没改任何文件。重新登录后可以重跑这一步。",
  steps: [
    { id: "run-8-build", stepIndex: 0, agentId: "grok", agentName: "Grok", title: "跑完整个任务", executionMode: "mock", notice: DEMO_NOTICE, status: "failed", expects: [], exitCode: 1, durationMs: 4_000, startedAt: NOW - 5 * HOUR + 2_000, finishedAt: NOW - 5 * HOUR + 6_000 },
  ],
  tasks: [],
  files: [],
  logs: [
    { id: "l-8-1", runId: "run-8", stepId: "run-8-build", agentId: "grok", stream: "stderr", chunk: "[grok] error: not logged in. Run `grok login` and try again.", timestamp: NOW - 5 * HOUR + 5_000 },
  ],
};

const run7: RunRecord = {
  id: "run-7",
  number: 7,
  sessionId: "task-7",
  flowId: "muse-grok",
  flowName: "Muse → Grok",
  prompt: MOCK_MESSAGES["task-7"][0].content,
  status: "succeeded",
  startedAt: NOW - 2 * DAY + 2_000,
  finishedAt: NOW - 2 * DAY + 11 * MIN,
  summary: "Muse 写了规范和 4 条任务，Grok 改了 3 个文件。验证：go test ./... 通过。",
  steps: [
    { id: "run-7-spec", stepIndex: 0, agentId: "muse", agentName: "Muse", title: "写规范", executionMode: "mock", notice: DEMO_NOTICE, status: "succeeded", expects: [".peck/handoff/7/intent.md", ".peck/tasks.md"], exitCode: 0, durationMs: 50_000, startedAt: NOW - 2 * DAY + 2_000, finishedAt: NOW - 2 * DAY + 52_000 },
    { id: "run-7-build", stepIndex: 1, agentId: "grok", agentName: "Grok", title: "实施", executionMode: "mock", notice: DEMO_NOTICE, status: "succeeded", expects: [], exitCode: 0, durationMs: 9 * MIN, startedAt: NOW - 2 * DAY + 60_000, finishedAt: NOW - 2 * DAY + 11 * MIN },
  ],
  tasks: [
    { id: "run-7-t1", text: "写规范（.peck/handoff/7/intent.md）", completed: true },
    { id: "run-7-t2", text: "RateLimit 配置加 per_route 表", completed: true },
    { id: "run-7-t3", text: "中间件先查路由阈值，没有再用全局", completed: true },
    { id: "run-7-t4", text: "go test ./... 通过", completed: true },
  ],
  files: [
    { path: ".peck/handoff/7/intent.md", kind: "create", additions: 24, deletions: 0, stepId: "run-7-spec" },
    { path: ".peck/tasks.md", kind: "create", additions: 6, deletions: 0, stepId: "run-7-spec" },
    { path: "internal/ratelimit/config.go", kind: "modify", additions: 14, deletions: 2, stepId: "run-7-build" },
    { path: "internal/ratelimit/middleware.go", kind: "modify", additions: 9, deletions: 3, stepId: "run-7-build" },
    { path: "internal/ratelimit/middleware_test.go", kind: "modify", additions: 22, deletions: 0, stepId: "run-7-build" },
  ],
  logs: [
    { id: "l-7-1", runId: "run-7", stepId: "run-7-spec", agentId: "muse", stream: "stdout", chunk: "[muse] 写规范 → .peck/handoff/7/intent.md（24 行）、任务清单 4 条", timestamp: NOW - 2 * DAY + 50_000 },
    { id: "l-7-2", runId: "run-7", stepId: "run-7-build", agentId: "grok", stream: "stdout", chunk: "[grok] 改 internal/ratelimit/config.go、middleware.go，补 middleware_test.go", timestamp: NOW - 2 * DAY + 8 * MIN },
    { id: "l-7-3", runId: "run-7", stepId: "run-7-build", agentId: "grok", stream: "stdout", chunk: "[grok] go test ./... → ok", timestamp: NOW - 2 * DAY + 10 * MIN },
  ],
};

/** 每个任务的记录，键是任务 id。 */
export const MOCK_RUNS: Record<string, RunRecord[]> = {
  "task-12": [run12],
  "task-11": [run11],
  "task-10": [run10],
  "task-9": [run9],
  "task-8": [run8],
  "task-7": [run7],
};

// ---------- Diff（键 `${runId}:${path}`） ----------

function created(runId: string, stepId: string, path: string, body: string, language = "markdown"): DiffResult {
  const lines = body.replace(/\n$/, "").split("\n");
  return {
    runId,
    stepId,
    filePath: path,
    kind: "create",
    language,
    additions: lines.length,
    deletions: 0,
    contentAfter: body,
    unifiedDiff: `diff --git a/${path} b/${path}\nnew file mode 100644\n--- /dev/null\n+++ b/${path}\n@@ -0,0 +1,${lines.length} @@\n${lines.map((l) => `+${l}`).join("\n")}`,
  };
}

const INTENT_12 = `# 笔记列表搜索框 · 规范

> 任务 12 · Muse 写 · 项目 notebook

## 目标
笔记列表顶部放一个搜索框，按标题和正文模糊匹配，输入时实时过滤，清空后恢复全部。

## 要点
- 匹配不分大小写；中文直接按子串。
- 过滤只在前端做，不改存储。
- 空结果显示「没有匹配的笔记」。

## 文件
| 文件 | 改法 |
|---|---|
| src/components/SearchBox.tsx | 新建：受控输入框，带清空按钮 |
| src/pages/NoteList.tsx | 接上搜索框，列表按关键字过滤 |
`;

const TASKS_12 = `# 任务清单

- [x] 写规范（.peck/handoff/12/intent.md）
- [x] 新建 SearchBox 组件（src/components/SearchBox.tsx）
- [ ] 笔记列表接上搜索框，按标题和正文过滤
- [ ] 清空输入恢复全部，补一个用例
`;

const INTENT_7 = `# 限流按路由配置 · 规范

> 任务 7 · Muse 写 · 项目 edge-gateway

## 目标
限流中间件支持按路由单独配置阈值，没配的用全局默认。

## 要点
- 配置里加 per_route: map[string]Limit。
- 中间件先查路由，查不到再用全局。
- 补两个用例：命中路由阈值、回落全局。
`;

const TASKS_7 = `# 任务清单

- [x] 写规范（.peck/handoff/7/intent.md）
- [x] RateLimit 配置加 per_route 表
- [x] 中间件先查路由阈值，没有再用全局
- [x] go test ./... 通过
`;

export const MOCK_DIFFS: Record<string, DiffResult> = {
  "run-12:.peck/handoff/12/intent.md": created("run-12", "run-12-spec", ".peck/handoff/12/intent.md", INTENT_12),
  "run-12:.peck/tasks.md": created("run-12", "run-12-spec", ".peck/tasks.md", TASKS_12),
  "run-12:src/components/SearchBox.tsx": created(
    "run-12",
    "run-12-build",
    "src/components/SearchBox.tsx",
    `import React from "react";

export interface SearchBoxProps {
  value: string;
  onChange: (next: string) => void;
}

export const SearchBox: React.FC<SearchBoxProps> = ({ value, onChange }) => (
  <div className="search-box">
    <input
      type="search"
      value={value}
      placeholder="搜索标题或正文"
      onChange={(e) => onChange(e.target.value)}
    />
    {value && (
      <button type="button" aria-label="清空" onClick={() => onChange("")}>
        ×
      </button>
    )}
  </div>
);
`,
    "typescript",
  ),
  "run-12:src/pages/NoteList.tsx": {
    runId: "run-12",
    stepId: "run-12-build",
    filePath: "src/pages/NoteList.tsx",
    kind: "modify",
    language: "typescript",
    additions: 0,
    deletions: 0,
    unifiedDiff: "",
  },

  "run-11:src/lib/date.ts": created(
    "run-11",
    "run-11-build",
    "src/lib/date.ts",
    `/** 日期统一显示成 YYYY-MM-DD。 */
export function formatDate(input: Date | number | string): string {
  const d = new Date(input);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return \`\${y}-\${m}-\${day}\`;
}
`,
    "typescript",
  ),
  "run-11:src/pages/NoteList.tsx": {
    runId: "run-11",
    stepId: "run-11-build",
    filePath: "src/pages/NoteList.tsx",
    kind: "modify",
    language: "typescript",
    additions: 2,
    deletions: 1,
    unifiedDiff: `diff --git a/src/pages/NoteList.tsx b/src/pages/NoteList.tsx
--- a/src/pages/NoteList.tsx
+++ b/src/pages/NoteList.tsx
@@ -1,4 +1,5 @@
 import React from "react";
+import { formatDate } from "../lib/date";
 import { useNotes } from "../store/notes";

@@ -18,7 +19,7 @@ export const NoteList: React.FC = () => {
         <li key={note.id}>
           <span>{note.title}</span>
-          <time>{new Date(note.updatedAt).toLocaleDateString()}</time>
+          <time>{formatDate(note.updatedAt)}</time>
         </li>`,
  },
  "run-11:src/pages/NoteDetail.tsx": {
    runId: "run-11",
    stepId: "run-11-build",
    filePath: "src/pages/NoteDetail.tsx",
    kind: "modify",
    language: "typescript",
    additions: 2,
    deletions: 1,
    unifiedDiff: `diff --git a/src/pages/NoteDetail.tsx b/src/pages/NoteDetail.tsx
--- a/src/pages/NoteDetail.tsx
+++ b/src/pages/NoteDetail.tsx
@@ -1,3 +1,4 @@
 import React from "react";
+import { formatDate } from "../lib/date";

@@ -12,5 +13,5 @@ export const NoteDetail: React.FC<{ id: string }> = ({ id }) => {
-      <p className="meta">更新于 {new Date(note.updatedAt).toLocaleDateString()}</p>
+      <p className="meta">更新于 {formatDate(note.updatedAt)}</p>`,
  },

  "run-10:src/lib/export.ts": {
    runId: "run-10",
    stepId: "run-10-worker",
    filePath: "src/lib/export.ts",
    kind: "modify",
    language: "typescript",
    additions: 7,
    deletions: 1,
    unifiedDiff: `diff --git a/src/lib/export.ts b/src/lib/export.ts
--- a/src/lib/export.ts
+++ b/src/lib/export.ts
@@ -3,6 +3,12 @@ import type { Note } from "./types";
 export function exportMarkdown(note: Note): string {
-  return \`# \${note.title}\\n\\n\${note.body}\`;
+  const frontMatter = [
+    "---",
+    \`title: \${JSON.stringify(note.title)}\`,
+    \`tags: [\${note.tags.map((t) => JSON.stringify(t)).join(", ")}]\`,
+    "---",
+  ].join("\\n");
+  return \`\${frontMatter}\\n\\n# \${note.title}\\n\\n\${note.body}\`;
 }`,
  },
  "run-10:src/lib/export.test.ts": {
    runId: "run-10",
    stepId: "run-10-worker",
    filePath: "src/lib/export.test.ts",
    kind: "modify",
    language: "typescript",
    additions: 11,
    deletions: 0,
    unifiedDiff: `diff --git a/src/lib/export.test.ts b/src/lib/export.test.ts
--- a/src/lib/export.test.ts
+++ b/src/lib/export.test.ts
@@ -14,3 +14,14 @@ describe("exportMarkdown", () => {
   });
+
+  it("写入 tags", () => {
+    const md = exportMarkdown({
+      id: "n1",
+      title: "买菜",
+      body: "鸡蛋",
+      tags: ["家务", "周末"],
+      updatedAt: 0,
+    });
+    expect(md).toContain('tags: ["家务", "周末"]');
+  });
 });`,
  },

  "run-9:src-tauri/src/settings.rs": {
    runId: "run-9",
    stepId: "run-9-impl",
    filePath: "src-tauri/src/settings.rs",
    kind: "modify",
    language: "rust",
    additions: 6,
    deletions: 0,
    unifiedDiff: `diff --git a/src-tauri/src/settings.rs b/src-tauri/src/settings.rs
--- a/src-tauri/src/settings.rs
+++ b/src-tauri/src/settings.rs
@@ -8,6 +8,12 @@ pub struct Settings {
     pub permission: Permission,
+    /// 任务完成、要回答时发 macOS 通知。
+    #[serde(default = "default_true")]
+    pub notifications: bool,
 }
+
+fn default_true() -> bool {
+    true
+}`,
  },
  "run-9:src-tauri/src/notify.rs": {
    runId: "run-9",
    stepId: "run-9-impl",
    filePath: "src-tauri/src/notify.rs",
    kind: "modify",
    language: "rust",
    additions: 3,
    deletions: 1,
    unifiedDiff: `diff --git a/src-tauri/src/notify.rs b/src-tauri/src/notify.rs
--- a/src-tauri/src/notify.rs
+++ b/src-tauri/src/notify.rs
@@ -5,5 +5,7 @@ use crate::settings::Settings;
-pub fn task_finished(app: &AppHandle, title: &str) {
+pub fn task_finished(app: &AppHandle, settings: &Settings, title: &str) {
+    if !settings.notifications {
+        return;
+    }
     send(app, "任务完成", title);
 }`,
  },

  "run-7:.peck/handoff/7/intent.md": created("run-7", "run-7-spec", ".peck/handoff/7/intent.md", INTENT_7),
  "run-7:.peck/tasks.md": created("run-7", "run-7-spec", ".peck/tasks.md", TASKS_7),
  "run-7:internal/ratelimit/config.go": {
    runId: "run-7",
    stepId: "run-7-build",
    filePath: "internal/ratelimit/config.go",
    kind: "modify",
    language: "plaintext",
    additions: 14,
    deletions: 2,
    unifiedDiff: `diff --git a/internal/ratelimit/config.go b/internal/ratelimit/config.go
--- a/internal/ratelimit/config.go
+++ b/internal/ratelimit/config.go
@@ -4,6 +4,18 @@ type Limit struct {
 	Burst int
 }

-type Config struct {
-	Global Limit
+type Config struct {
+	Global   Limit
+	PerRoute map[string]Limit
+}
+
+// For 返回路由的阈值，没配的用全局。
+func (c Config) For(route string) Limit {
+	if c.PerRoute != nil {
+		if l, ok := c.PerRoute[route]; ok {
+			return l
+		}
+	}
+	return c.Global
 }`,
  },
  "run-7:internal/ratelimit/middleware.go": {
    runId: "run-7",
    stepId: "run-7-build",
    filePath: "internal/ratelimit/middleware.go",
    kind: "modify",
    language: "plaintext",
    additions: 9,
    deletions: 3,
    unifiedDiff: `diff --git a/internal/ratelimit/middleware.go b/internal/ratelimit/middleware.go
--- a/internal/ratelimit/middleware.go
+++ b/internal/ratelimit/middleware.go
@@ -12,9 +12,15 @@ func Middleware(cfg Config) func(http.Handler) http.Handler {
 	return func(next http.Handler) http.Handler {
 		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
-			if !global.Allow() {
-				http.Error(w, "too many requests", http.StatusTooManyRequests)
-				return
+			limit := cfg.For(r.URL.Path)
+			bucket := buckets.get(r.URL.Path, limit)
+			if !bucket.Allow() {
+				http.Error(w, "too many requests", http.StatusTooManyRequests)
+				return
+			}
+			if limit != cfg.Global {
+				w.Header().Set("X-RateLimit-Scope", "route")
 			}
 			next.ServeHTTP(w, r)
 		})`,
  },
  "run-7:internal/ratelimit/middleware_test.go": {
    runId: "run-7",
    stepId: "run-7-build",
    filePath: "internal/ratelimit/middleware_test.go",
    kind: "modify",
    language: "plaintext",
    additions: 22,
    deletions: 0,
    unifiedDiff: `diff --git a/internal/ratelimit/middleware_test.go b/internal/ratelimit/middleware_test.go
--- a/internal/ratelimit/middleware_test.go
+++ b/internal/ratelimit/middleware_test.go
@@ -30,3 +30,25 @@ func TestGlobalLimit(t *testing.T) {
 	}
 }
+
+func TestPerRouteLimit(t *testing.T) {
+	cfg := Config{
+		Global:   Limit{Rate: 100, Burst: 100},
+		PerRoute: map[string]Limit{"/login": {Rate: 1, Burst: 1}},
+	}
+	h := Middleware(cfg)(okHandler)
+	if code := hit(h, "/login"); code != 200 {
+		t.Fatalf("first /login: %d", code)
+	}
+	if code := hit(h, "/login"); code != 429 {
+		t.Fatalf("second /login should be limited: %d", code)
+	}
+}
+
+func TestFallbackToGlobal(t *testing.T) {
+	cfg := Config{Global: Limit{Rate: 1, Burst: 2}}
+	h := Middleware(cfg)(okHandler)
+	if cfg.For("/anything") != cfg.Global {
+		t.Fatal("expected global limit")
+	}
+}`,
  },
};

// ---------- API 模式的调用历史（右栏「调用与审查历史」） ----------

const LIMITS: RunLimits = { maxRounds: 12, maxToolCalls: 80, maxContextBytes: 180000, maxTotalTokens: 500000, maxDelegations: 4, maxReworks: 1, workerMaxRounds: 6 };

const DEEPSEEK: ModelProfile = { id: "deepseek-flash", name: "DeepSeek Flash", providerId: "deepseek", apiModel: "deepseek-flash", contextWindow: 128000, maxOutputTokens: 8192, thinking: "switch", efforts: [], tools: true };
const OPUS: ModelProfile = { id: "claude-opus-5-5", name: "Claude Opus 5.5", providerId: "anthropic", apiModel: "claude-opus-5-5", contextWindow: 200000, maxOutputTokens: 16000, thinking: "switch", efforts: ["low", "medium", "high"], tools: true };

export const MOCK_API_TASKS: Record<string, ApiTask[]> = {
  "run-10": [
    {
      id: "apitask-10", runId: "run-10", parentId: null, previousTaskId: null, stepId: "run-10-worker",
      title: "导出 Markdown 时保留标签", paths: [], acceptance: "", attempt: 1, review: null,
      changeset: { files: run10.files },
      mode: "mock", model: DEEPSEEK, settings: { role: "worker", modelId: "deepseek-flash", thinking: false, effort: null, maxOutputTokens: 8192 },
      status: "completed", round: 4, limits: LIMITS, callIds: ["call-10-1", "call-10-2", "call-10-3", "call-10-4"],
      summary: run10.summary ?? null, inputTokens: 18_420, outputTokens: 2_310,
      tools: [
        { callId: "call-10-1", tool: { id: "t1", name: "read_file", arguments: { path: "src/lib/export.ts" } }, status: "completed", output: "export function exportMarkdown(note: Note): string { … }", startedAt: NOW - 1 * DAY + 5_000, finishedAt: NOW - 1 * DAY + 5_200 },
        { callId: "call-10-2", tool: { id: "t2", name: "write_file", arguments: { path: "src/lib/export.ts" } }, status: "completed", output: "写入 18 行", startedAt: NOW - 1 * DAY + 90_000, finishedAt: NOW - 1 * DAY + 90_300 },
        { callId: "call-10-3", tool: { id: "t3", name: "write_file", arguments: { path: "src/lib/export.test.ts" } }, status: "completed", output: "写入 27 行", startedAt: NOW - 1 * DAY + 200_000, finishedAt: NOW - 1 * DAY + 200_300 },
        { callId: "call-10-4", tool: { id: "t4", name: "run_command", arguments: { command: "pnpm vitest run src/lib/export.test.ts" } }, status: "completed", output: "Test Files  1 passed (1)\nTests  3 passed (3)", startedAt: NOW - 1 * DAY + 400_000, finishedAt: NOW - 1 * DAY + 412_000 },
      ],
    },
  ],
  "run-9": [
    {
      id: "apitask-9", runId: "run-9", parentId: null, previousTaskId: null, stepId: "run-9-plan",
      title: "设置面板加通知开关", paths: [], acceptance: "", attempt: 1, review: null,
      changeset: { files: [] },
      mode: "mock", model: OPUS, settings: { role: "supervisor", modelId: "claude-opus-5-5", thinking: true, effort: "medium", maxOutputTokens: 16000 },
      status: "completed", round: 3, limits: LIMITS, callIds: ["call-9-1", "call-9-2", "call-9-3"],
      summary: "拆成 1 个子任务交给实施的模型；审查通过。", inputTokens: 9_800, outputTokens: 1_450,
      tools: [
        { callId: "call-9-1", tool: { id: "t1", name: "list_files", arguments: { path: "src-tauri/src" } }, status: "completed", output: "settings.rs\nnotify.rs\n…", startedAt: NOW - 1 * HOUR - 28 * MIN, finishedAt: NOW - 1 * HOUR - 28 * MIN + 300 },
        { callId: "call-9-2", tool: { id: "t2", name: "delegate_task", arguments: { title: "通知开关", paths: ["src-tauri/src/settings.rs", "src-tauri/src/notify.rs"] } }, status: "completed", output: "子任务完成，改了 2 个文件", startedAt: NOW - 1 * HOUR - 20 * MIN, finishedAt: NOW - 1 * HOUR - 8 * MIN },
      ],
    },
    {
      id: "apitask-9-1", runId: "run-9", parentId: "apitask-9", previousTaskId: null, stepId: "run-9-impl",
      title: "通知开关", paths: ["src-tauri/src/settings.rs", "src-tauri/src/notify.rs"], acceptance: "设置里有 notifications 字段默认 true；关了不发通知；cargo test 通过", attempt: 1,
      review: { decision: "accept", feedback: "开关默认开，关了不发通知，测试覆盖两种情况。", reviewedAt: NOW - 1 * HOUR - 2 * MIN },
      changeset: { files: run9.files },
      mode: "mock", model: DEEPSEEK, settings: { role: "worker", modelId: "deepseek-flash", thinking: false, effort: null, maxOutputTokens: 8192 },
      status: "completed", round: 3, limits: LIMITS, callIds: ["call-9-w1", "call-9-w2", "call-9-w3"],
      summary: "settings.rs 加 notifications 字段（默认 true），notify.rs 发通知前读开关。cargo test 通过。", inputTokens: 12_300, outputTokens: 1_900,
      tools: [
        { callId: "call-9-w1", tool: { id: "t1", name: "read_file", arguments: { path: "src-tauri/src/settings.rs" } }, status: "completed", output: "pub struct Settings { … }", startedAt: NOW - 1 * HOUR - 19 * MIN, finishedAt: NOW - 1 * HOUR - 19 * MIN + 200 },
        { callId: "call-9-w2", tool: { id: "t2", name: "write_file", arguments: { path: "src-tauri/src/notify.rs" } }, status: "completed", output: "写入 11 行", startedAt: NOW - 1 * HOUR - 14 * MIN, finishedAt: NOW - 1 * HOUR - 14 * MIN + 300 },
        { callId: "call-9-w3", tool: { id: "t3", name: "run_command", arguments: { command: "cargo test" } }, status: "completed", output: "test result: ok. 41 passed; 0 failed", startedAt: NOW - 1 * HOUR - 9 * MIN, finishedAt: NOW - 1 * HOUR - 8 * MIN },
      ],
    },
  ],
};

// ---------- 项目里的文件（右栏 FILES） ----------

/** 每个项目的文件：路径 → 内容。任务新建的文件不在这里，从 Diff 里取。 */
export const MOCK_FILES: Record<string, Record<string, string>> = {
  "ws-notebook": {
    "README.md": "# notebook\n\n本地笔记：Markdown、标签、导出。\n\n## 开发\n\npnpm install\npnpm dev\n",
    "package.json": '{\n  "name": "notebook",\n  "version": "0.6.0",\n  "scripts": {\n    "dev": "vite",\n    "check": "tsc --noEmit",\n    "test": "vitest run"\n  }\n}\n',
    "src/main.tsx": 'import { createRoot } from "react-dom/client";\nimport { App } from "./App";\n\ncreateRoot(document.getElementById("root")!).render(<App />);\n',
    "src/App.tsx": 'import { NoteList } from "./pages/NoteList";\n\nexport function App() {\n  return <NoteList />;\n}\n',
    "src/pages/NoteList.tsx": 'import { useNotes } from "../store/notes";\nimport { formatDate } from "../lib/date";\n\nexport function NoteList() {\n  const notes = useNotes();\n  return (\n    <ul>\n      {notes.map((note) => (\n        <li key={note.id}>\n          <b>{note.title}</b>\n          <span>{formatDate(note.updatedAt)}</span>\n        </li>\n      ))}\n    </ul>\n  );\n}\n',
    "src/pages/NoteDetail.tsx": 'import { formatDate } from "../lib/date";\n\nexport function NoteDetail({ note }) {\n  return (\n    <article>\n      <h1>{note.title}</h1>\n      <time>{formatDate(note.updatedAt)}</time>\n      <p>{note.body}</p>\n    </article>\n  );\n}\n',
    "src/lib/date.ts": "// 日期统一成 YYYY-MM-DD\nexport function formatDate(input: Date | number | string): string {\n  const d = new Date(input);\n  const pad = (n: number) => String(n).padStart(2, \"0\");\n  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;\n}\n",
    "src/lib/export.ts": "export function exportMarkdown(note) {\n  const tags = note.tags.length ? `tags: [${note.tags.join(\", \")}]\\n` : \"\";\n  return `---\\n${tags}---\\n\\n# ${note.title}\\n\\n${note.body}\\n`;\n}\n",
    "src/lib/export.test.ts": 'import { exportMarkdown } from "./export";\n\ntest("写入 tags", () => {\n  expect(exportMarkdown({ title: "a", body: "b", tags: ["x"] })).toContain("tags: [x]");\n});\n',
    "src/store/notes.ts": "export function useNotes() {\n  return [];\n}\n",
    "src/legacy/old-banner.ts": "// 旧的首页横幅，已经没人用了\nexport const banner = \"notebook 0.5\";\n",
    "public/logo.png": "",
  },
  "ws-peck": {
    "README.md": "# Peck\n\n只在我自己的 Mac 上用的本地 Coding Agent。\n",
    "src-tauri/src/settings.rs": "pub struct Settings {\n    pub notifications: bool,\n}\n\nimpl Default for Settings {\n    fn default() -> Self {\n        Self { notifications: true }\n    }\n}\n",
    "src-tauri/src/notify.rs": "pub fn notify(settings: &Settings, title: &str) {\n    if !settings.notifications {\n        return;\n    }\n    send(title);\n}\n",
    // v0.7 FILES 的几种预览（图片、PDF、Word、Excel 的内容在 mockIpc 里给）
    "docs/screenshot.png": "",
    "docs/spec.pdf": "",
    "docs/plan.docx": "",
    "docs/budget.xlsx": "",
    "docs/guide.md": "# 使用说明\n\n- **FILES** 能给代码上色\n- md、html 能切源码和预览\n\n| 文件 | 怎么看 |\n| --- | --- |\n| 图片、PDF | 直接显示 |\n| Word、Excel | Quick Look |\n",
    "public/index.html": "<!doctype html>\n<html lang=\"zh\">\n<head><meta charset=\"utf-8\"><title>Peck</title>\n<style>body{font-family:sans-serif;background:#06121F;color:#E6EEF8;padding:24px}h1{color:#00E5FF}</style></head>\n<body>\n  <h1>Peck</h1>\n  <p>只在我自己的 Mac 上用的本地 Coding Agent。</p>\n</body>\n</html>\n",
    "public/bird.svg": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\" viewBox=\"0 0 8 8\">\n  <rect width=\"8\" height=\"8\" fill=\"#06121F\"/>\n  <rect x=\"2\" y=\"2\" width=\"4\" height=\"3\" fill=\"#00E5FF\"/>\n  <rect x=\"5\" y=\"3\" width=\"2\" height=\"1\" fill=\"#FFC857\"/>\n</svg>\n",
  },
  "ws-gateway": {
    "README.md": "# edge-gateway\n",
    "go.mod": "module edge-gateway\n\ngo 1.23\n",
    "internal/ratelimit/config.go": "package ratelimit\n\ntype Config struct {\n\tDefault  int\n\tPerRoute map[string]int\n}\n",
    "internal/ratelimit/middleware.go": "package ratelimit\n\nfunc (c Config) limitFor(route string) int {\n\tif n, ok := c.PerRoute[route]; ok {\n\t\treturn n\n\t}\n\treturn c.Default\n}\n",
    "internal/ratelimit/middleware_test.go": "package ratelimit\n",
  },
};
/** 照忽略规则藏起来的文件夹（演示）。 */
export const MOCK_HIDDEN: Record<string, number> = { "ws-notebook": 2, "ws-peck": 3, "ws-gateway": 1 };

// ---------- 设置里的供应商、模型（演示） ----------

export const MOCK_API_STATUS: ApiStatus = {
  mode: "mock",
  providers: [
    { id: "anthropic", name: "Anthropic", baseUrl: "https://api.anthropic.com", protocol: "anthropic" },
    { id: "deepseek", name: "DeepSeek", baseUrl: "https://api.deepseek.com", protocol: "chatCompletions" },
    { id: "openai", name: "OpenAI", baseUrl: "https://api.openai.com", protocol: "responses" },
    { id: "gemini", name: "Google Gemini", baseUrl: "https://generativelanguage.googleapis.com", protocol: "gemini" },
    { id: "aliyun", name: "阿里云百炼", baseUrl: "https://dashscope.aliyuncs.com", protocol: "qwen" },
  ],
  config: {
    models: [OPUS, DEEPSEEK],
    agents: [
      { role: "supervisor", modelId: "claude-opus-5-5", thinking: true, effort: "medium", maxOutputTokens: 16000 },
      { role: "worker", modelId: "deepseek-flash", thinking: false, effort: null, maxOutputTokens: 8192 },
    ],
    aliyunRegion: "beijing",
    aliyunWorkspaceId: "",
  },
  keys: [
    { providerId: "anthropic", state: "saved" },
    { providerId: "deepseek", state: "saved" },
    { providerId: "openai", state: "missing" },
    { providerId: "gemini", state: "missing" },
    { providerId: "aliyun", state: "missing" },
  ],
  allowances: [
    { providerId: "anthropic", apiModel: "claude-opus-5-5", limit: 20, used: 7 },
    { providerId: "deepseek", apiModel: "deepseek-flash", limit: 100, used: 46 },
  ],
  calls: [],
};
