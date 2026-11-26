// `?mock` 的假数据：项目列表、每个项目最近几次任务（在跑的、做完的、失败的、归档的），每次带一段事件日志。
// 只在浏览器里用，刷新重置。用词照 docs/design.md。
import type { ApiStatus } from "@/lib/api";
import type { AgentCaps, AgentEvent, FileDiff, ToolKind } from "@/lib/agentEvents";
import type { FileChangeKind, RunRecord, Session, StepDescriptor, Workspace } from "@/lib/types";
import type { ReplayEvent } from "@/dev/gatewayMock";
import type { PromptItem } from "@/lib/ipc";

const NOW = Date.now();
const MIN = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

// ---------- 项目 ----------

export const MOCK_WORKSPACES: Workspace[] = [
  { id: "ws-notebook", path: "/Users/me/Documents/GitHub/notebook", name: "notebook", lastOpenedAt: NOW - 2 * MIN, gitBranch: "feat/search" },
  { id: "ws-peck", path: "/Users/me/Documents/GitHub/personal-cli-studio/peck", name: "peck", lastOpenedAt: NOW - 1 * HOUR, gitBranch: "main" },
  { id: "ws-gateway", path: "/Users/me/Documents/GitHub/edge-gateway", name: "edge-gateway", lastOpenedAt: NOW - 1 * DAY, gitBranch: "release/v2.4" },
];

// ---------- 任务（一次对话就是一个任务） ----------

export const MOCK_SESSIONS: Session[] = [
  // notebook：在跑一个，做完两个
  { id: "task-12", workspaceId: "ws-notebook", title: "笔记列表加搜索框", createdAt: NOW - 6 * MIN, updatedAt: NOW - 20_000 },
  { id: "task-11", workspaceId: "ws-notebook", title: "日期统一成 YYYY-MM-DD", createdAt: NOW - 3 * HOUR, updatedAt: NOW - 3 * HOUR + 4 * MIN },
  { id: "task-10", workspaceId: "ws-notebook", title: "导出 Markdown 时保留标签", createdAt: NOW - 1 * DAY, updatedAt: NOW - 1 * DAY + 9 * MIN },
  // peck：一个 API 做完，一个 CLI 失败
  { id: "task-9", workspaceId: "ws-peck", title: "设置面板加通知开关", createdAt: NOW - 1 * HOUR - 30 * MIN, updatedAt: NOW - 1 * HOUR },
  { id: "task-8", workspaceId: "ws-peck", title: "菜单栏图标在跑时变样", createdAt: NOW - 5 * HOUR, updatedAt: NOW - 5 * HOUR + 2 * MIN },
  // edge-gateway：一个 CLI 做完
  { id: "task-7", workspaceId: "ws-gateway", title: "限流中间件按路由配置", createdAt: NOW - 2 * DAY, updatedAt: NOW - 2 * DAY + 11 * MIN },
  // edge-gateway：一个归档了的任务（左栏不列，设置 → ARCHIVE 里看）
  { id: "task-6", workspaceId: "ws-gateway", title: "健康检查接口带上版本号", createdAt: NOW - 6 * DAY, updatedAt: NOW - 6 * DAY + 4 * MIN, archivedAt: NOW - 3 * DAY },
];

// ---------- 任务记录和事件日志 ----------

const SEED_CAPS: AgentCaps = {
  streaming: true, reasoningReplay: true, tools: "peck", approvals: "live", askUser: true, steer: true, interrupt: true,
  resume: true, mcpInject: false, usage: "per_turn", cost: "priced", quota: false, switchModel: true, needsTty: false,
};

const started = (engine: string, model: string | null = null): ReplayEvent => ({ type: "session.started", engine, model, caps: SEED_CAPS, tools: [], mcp: [] });
const say = (id: string, text: string): ReplayEvent => ({ type: "message.completed", itemId: id, text });
const tool = (callId: string, name: string, kind: ToolKind, title: string, input: unknown = {}): ReplayEvent => ({ type: "tool_call.started", callId, name, kind, title, input });
const ok = (callId: string, output: string, extra: { exitCode?: number; diff?: FileDiff[] } = {}): ReplayEvent => ({ type: "tool_call.completed", callId, status: "ok", output, exitCode: extra.exitCode ?? null, diff: extra.diff ?? [], durationMs: 900 });
const edited = (path: string, additions: number, deletions: number, created = false): FileDiff => ({ path, change: created ? "added" : "modified", additions, deletions });
const usage = (usd: number, source: "priced" | "estimated" = "priced"): ReplayEvent => ({ type: "usage.updated", usage: { input: 9_200, output: 1_400, cacheRead: 4_100, cacheWrite: null, reasoning: null }, cost: { usd, source }, scope: "turn" });
const turnEnd = (reason: "done" | "error" = "done", error: string | null = null): ReplayEvent => ({ type: "turn.completed", turnId: "turn-1", reason, error });
/** 编上号、按时间排开（从 `from` 起，每条隔 `gap` 毫秒）。 */
const stamp = (events: ReplayEvent[], from: number, gap: number): AgentEvent[] => events.map((e, i) => ({ ...(e as object), seq: i, at: from + i * gap }) as AgentEvent);

/** 只有一步的任务记录（现在的任务都这样）。 */
function step(runId: string, agentId: string, label: string, status: StepDescriptor["status"], from: number, to?: number): StepDescriptor {
  return { id: `${runId}-step-1`, stepIndex: 0, agentId, agentName: label, title: label, status, startedAt: from, ...(to ? { finishedAt: to, durationMs: to - from } : {}) };
}
const file = (path: string, kind: FileChangeKind, additions: number, deletions: number) => ({ path, kind, additions, deletions });


const run12: RunRecord = {
  id: "run-12", number: 12, sessionId: "task-12",
  prompt: "给笔记列表顶部加一个搜索框，按标题和正文模糊匹配，输入时实时过滤，清空后恢复全部。",
  status: "running", startedAt: NOW - 6 * MIN,
  steps: [step("run-12", "peck", "Peck · Claude Fable 5.2", "running", NOW - 6 * MIN)],
  files: [file("src/components/SearchBox.tsx", "create", 12, 0)],
  who: "api:anthropic/claude-fable-5-2", permission: "edit",
};

const run11: RunRecord = {
  id: "run-11", number: 11, sessionId: "task-11",
  prompt: "把界面里所有日期显示统一成 YYYY-MM-DD，用一个共用的格式化函数。",
  status: "succeeded", startedAt: NOW - 3 * HOUR + 2_000, finishedAt: NOW - 3 * HOUR + 4 * MIN,
  summary: "新增 formatDate，替换了 2 处日期显示。pnpm check 通过。",
  steps: [step("run-11", "grok", "Grok", "succeeded", NOW - 3 * HOUR + 2_000, NOW - 3 * HOUR + 4 * MIN)],
  files: [file("src/lib/date.ts", "create", 9, 0), file("src/pages/NoteList.tsx", "modify", 2, 1), file("src/pages/NoteDetail.tsx", "modify", 2, 1)],
  who: "cli:grok", permission: "edit",
};

const run10: RunRecord = {
  id: "run-10", number: 10, sessionId: "task-10",
  prompt: "导出 Markdown 的时候把笔记的标签写进文件头（YAML front matter）。",
  status: "succeeded", startedAt: NOW - 1 * DAY + 2_000, finishedAt: NOW - 1 * DAY + 9 * MIN,
  summary: "导出时写入 tags 字段，补了一个单元测试。vitest 3 个用例通过。",
  steps: [step("run-10", "peck", "Peck · DeepSeek V4.1 Flash 1017", "succeeded", NOW - 1 * DAY + 2_000, NOW - 1 * DAY + 9 * MIN)],
  files: [file("src/lib/export.ts", "modify", 7, 1), file("src/lib/export.test.ts", "modify", 11, 0)],
  who: "api:deepseek/deepseek-flash", permission: "edit",
};

const run9: RunRecord = {
  id: "run-9", number: 9, sessionId: "task-9",
  prompt: "设置里加一个「通知」开关，默认开，关了以后任务完成不发 macOS 通知。",
  status: "succeeded", startedAt: NOW - 1 * HOUR - 30 * MIN + 2_000, finishedAt: NOW - 1 * HOUR,
  summary: "settings.rs 加了 notifications 字段（默认 true），notify.rs 发通知前读开关。cargo test 通过。",
  steps: [step("run-9", "peck", "Peck · DeepSeek V4.1 Flash 1017", "succeeded", NOW - 1 * HOUR - 30 * MIN + 2_000, NOW - 1 * HOUR)],
  files: [file("src-tauri/src/settings.rs", "modify", 6, 0), file("src-tauri/src/notify.rs", "modify", 3, 1)],
  who: "api:deepseek/deepseek-flash", permission: "edit",
};

const run8: RunRecord = {
  id: "run-8", number: 8, sessionId: "task-8",
  prompt: "有任务在跑的时候把菜单栏小图标换成带点的那个。",
  status: "failed", startedAt: NOW - 5 * HOUR + 2_000, finishedAt: NOW - 5 * HOUR + 2 * MIN,
  summary: "没做完：Grok 没登录：在终端跑 grok login 再试。",
  steps: [step("run-8", "grok", "Grok", "failed", NOW - 5 * HOUR + 2_000, NOW - 5 * HOUR + 2 * MIN)],
  files: [],
  who: "cli:grok", permission: "edit",
};

const run7: RunRecord = {
  id: "run-7", number: 7, sessionId: "task-7",
  prompt: "限流中间件改成可以按路由单独配置阈值，没配的用全局默认。",
  status: "succeeded", startedAt: NOW - 2 * DAY + 2_000, finishedAt: NOW - 2 * DAY + 11 * MIN,
  summary: "Grok 改了 3 个文件。go test ./... 通过。",
  steps: [step("run-7", "grok", "Grok", "succeeded", NOW - 2 * DAY + 2_000, NOW - 2 * DAY + 11 * MIN)],
  files: [file("internal/ratelimit/config.go", "modify", 14, 2), file("internal/ratelimit/middleware.go", "modify", 9, 3), file("internal/ratelimit/middleware_test.go", "modify", 22, 0)],
  who: "cli:grok", permission: "edit",
};

const run6: RunRecord = {
  id: "run-6", number: 6, sessionId: "task-6",
  prompt: "/healthz 的返回里带上构建版本号和提交号。",
  status: "succeeded", startedAt: NOW - 6 * DAY + 2_000, finishedAt: NOW - 6 * DAY + 4 * MIN,
  summary: "/healthz 加了 version、commit 两个字段，构建时注入。go test ./... 通过。",
  steps: [step("run-6", "grok", "Grok", "succeeded", NOW - 6 * DAY + 2_000, NOW - 6 * DAY + 4 * MIN)],
  files: [],
  who: "cli:grok", permission: "edit",
};

/** 每个任务的记录，键是任务 id。 */
export const MOCK_RUNS: Record<string, RunRecord[]> = {
  "task-12": [run12],
  "task-11": [run11],
  "task-10": [run10],
  "task-9": [run9],
  "task-8": [run8],
  "task-7": [run7],
  "task-6": [run6],
};

/** 每次的事件日志（中栏的过程）。run-12 一直在跑：最后一步没收尾，也没有 turn.completed。 */
export const MOCK_EVENTS: Record<string, AgentEvent[]> = {
  "run-12": stamp([
    started("peck", "anthropic/claude-fable-5-2"),
    { type: "turn.started", turnId: "turn-1" },
    { type: "plan.updated", steps: [{ text: "看笔记列表和数据从哪来", status: "completed" }, { text: "新建 SearchBox 组件", status: "completed" }, { text: "列表接上搜索框、按标题和正文过滤", status: "in_progress" }] },
    say("m1", "我先看一下笔记列表和数据是怎么来的。"),
    tool("c1", "read_file", "read", "读 src/pages/NoteList.tsx"), ok("c1", "export function NoteList() { … }"),
    tool("c2", "read_file", "read", "读 src/store/notes.ts"), ok("c2", "export function useNotes() { … }"),
    tool("c3", "write_file", "edit", "写 src/components/SearchBox.tsx"), ok("c3", "写好了", { diff: [edited("src/components/SearchBox.tsx", 12, 0, true)] }),
    say("m2", "组件好了，接到列表上。"),
    tool("c4", "write_patch", "edit", "改 src/pages/NoteList.tsx"),
  ], NOW - 6 * MIN, 20_000),
  "run-11": stamp([
    started("grok"),
    say("m1", "找到 2 处 toLocaleDateString()，先加一个共用的 formatDate。"),
    tool("c1", "Grep", "search", "搜「toLocaleDateString」"), ok("c1", "src/pages/NoteList.tsx:21\nsrc/pages/NoteDetail.tsx:8"),
    tool("c2", "Write", "edit", "写 src/lib/date.ts"), ok("c2", "", { diff: [edited("src/lib/date.ts", 9, 0, true)] }),
    tool("c3", "Edit", "edit", "改 src/pages/NoteList.tsx"), ok("c3", "", { diff: [edited("src/pages/NoteList.tsx", 2, 1)] }),
    tool("c4", "Edit", "edit", "改 src/pages/NoteDetail.tsx"), ok("c4", "", { diff: [edited("src/pages/NoteDetail.tsx", 2, 1)] }),
    tool("c5", "Bash", "run", "跑 pnpm check"), ok("c5", "$ tsc --noEmit", { exitCode: 0 }),
    say("m2", "新增 formatDate，替换了 2 处日期显示。pnpm check 通过。"),
    usage(0.031, "estimated"),
    turnEnd(),
  ], NOW - 3 * HOUR + 2_000, 15_000),
  "run-10": stamp([
    started("peck", "deepseek/deepseek-flash"),
    tool("c1", "read_file", "read", "读 src/lib/export.ts"), ok("c1", "export function exportMarkdown(note: Note): string { … }"),
    tool("c2", "write_patch", "edit", "改 src/lib/export.ts"), ok("c2", "改好了", { diff: [edited("src/lib/export.ts", 7, 1)] }),
    tool("c3", "write_patch", "edit", "改 src/lib/export.test.ts"), ok("c3", "改好了", { diff: [edited("src/lib/export.test.ts", 11, 0)] }),
    tool("c4", "bash_run", "run", "跑 pnpm vitest run src/lib/export.test.ts"), ok("c4", "Test Files  1 passed (1)\nTests  3 passed (3)", { exitCode: 0 }),
    say("m1", "导出时把 tags 写进 front matter，补了一个用例。vitest 3 个用例通过。"),
    usage(0.0042),
    turnEnd(),
  ], NOW - 1 * DAY + 2_000, 60_000),
  "run-9": stamp([
    started("peck", "deepseek/deepseek-flash"),
    { type: "turn.started", turnId: "turn-1" },
    ...[
      tool("s1", "read_file", "read", "读 src-tauri/src/settings.rs"), ok("s1", "pub struct Settings { … }"),
      tool("s2", "read_file", "read", "读 src-tauri/src/notify.rs"), ok("s2", "pub fn notify(title: &str) { … }"),
      say("b1", "改两处。"),
      tool("b2", "write_patch", "edit", "改 src-tauri/src/settings.rs"), ok("b2", "改好了", { diff: [edited("src-tauri/src/settings.rs", 6, 0)] }),
      tool("b3", "write_patch", "edit", "改 src-tauri/src/notify.rs"), ok("b3", "改好了", { diff: [edited("src-tauri/src/notify.rs", 3, 1)] }),
      tool("b4", "bash_run", "run", "跑 cargo test"), ok("b4", "test result: ok. 41 passed; 0 failed", { exitCode: 0 }),
      say("b5", "settings.rs 加了 notifications 字段（默认 true），notify.rs 发通知前读开关。cargo test 通过。"),
      usage(0.0036),
    ],
    turnEnd(),
  ], NOW - 1 * HOUR - 30 * MIN + 2_000, 90_000),
  "run-8": stamp([
    started("grok"),
    { type: "error", message: "Grok 没登录：在终端跑 grok login 再试。", fatal: true },
    turnEnd("error", "Grok 没登录：在终端跑 grok login 再试。"),
  ], NOW - 5 * HOUR + 2_000, 4_000),
  "run-7": stamp([
    started("grok"),
    { type: "turn.started", turnId: "turn-1" },
    ...[
      tool("b1", "edit", "edit", "改 internal/ratelimit/config.go"), ok("b1", "", { diff: [edited("internal/ratelimit/config.go", 14, 2)] }),
      tool("b2", "edit", "edit", "改 internal/ratelimit/middleware.go"), ok("b2", "", { diff: [edited("internal/ratelimit/middleware.go", 9, 3)] }),
      tool("b3", "edit", "edit", "改 internal/ratelimit/middleware_test.go"), ok("b3", "", { diff: [edited("internal/ratelimit/middleware_test.go", 22, 0)] }),
      tool("b4", "execute", "run", "跑 go test ./..."), ok("b4", "ok  edge-gateway/internal/ratelimit  0.41s", { exitCode: 0 }),
      say("b5", "改了 3 个文件：配置加 PerRoute，中间件先查路由再回落全局，补了两个用例。go test ./... 通过。"),
      usage(0.024, "estimated"),
    ],
    turnEnd(),
  ], NOW - 2 * DAY + 2_000, 50_000),
  "run-6": stamp([
    started("grok"),
    tool("c1", "Bash", "run", "跑 go test ./..."), ok("c1", "ok  edge-gateway/internal/health  0.12s", { exitCode: 0 }),
    say("m1", "/healthz 加了 version、commit 两个字段，构建时用 -ldflags 注入。go test ./... 通过。"),
    usage(0.009, "estimated"),
    turnEnd(),
  ], NOW - 6 * DAY + 2_000, 30_000),
};

// ---------- Diff（键 `${runId}:${path}`） ----------

/** 演示的 Diff：FILES 用它标出这次加的行；新建的文件内容也从这里取。 */
export interface MockDiff { kind: FileChangeKind; additions: number; deletions: number; unifiedDiff: string; contentAfter?: string }

function created(path: string, body: string): MockDiff {
  const lines = body.replace(/\n$/, "").split("\n");
  return {
    kind: "create",
    additions: lines.length,
    deletions: 0,
    contentAfter: body,
    unifiedDiff: `diff --git a/${path} b/${path}\nnew file mode 100644\n--- /dev/null\n+++ b/${path}\n@@ -0,0 +1,${lines.length} @@\n${lines.map((l) => `+${l}`).join("\n")}`,
  };
}

export const MOCK_DIFFS: Record<string, MockDiff> = {
  "run-12:src/components/SearchBox.tsx": created("src/components/SearchBox.tsx",
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
`),
  "run-12:src/pages/NoteList.tsx": {
    kind: "modify",
    additions: 0,
    deletions: 0,
    unifiedDiff: "",
  },

  "run-11:src/lib/date.ts": created("src/lib/date.ts",
    `/** 日期统一显示成 YYYY-MM-DD。 */
export function formatDate(input: Date | number | string): string {
  const d = new Date(input);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return \`\${y}-\${m}-\${day}\`;
}
`),
  "run-11:src/pages/NoteList.tsx": {
    kind: "modify",
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
    kind: "modify",
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
    kind: "modify",
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
    kind: "modify",
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
    kind: "modify",
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
    kind: "modify",
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

  "run-7:internal/ratelimit/config.go": {
    kind: "modify",
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
    kind: "modify",
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
    kind: "modify",
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

// ---------- 提示词库（v1.2.0，全局一份；演示用的样例，最近改的在上） ----------

export const MOCK_PROMPTS: PromptItem[] = [
  { id: "prompt-review", title: "审一下这次的改动", body: "看一下这次的改动：\n- 有没有明显的 Bug\n- 命名和结构清不清楚\n- 测试够不够\n最后列出要改的地方，先别动手改。", createdAt: NOW - 9 * DAY, updatedAt: NOW - 20 * MIN },
  { id: "prompt-explain", title: "讲清楚一个文件", body: "读 {文件名}，用几句话说清楚它做什么、被谁调用、有哪些坑。\n不用改代码。", createdAt: NOW - 6 * DAY, updatedAt: NOW - 3 * HOUR },
  { id: "prompt-tests", title: "补测试", body: "给 {函数名} 补单元测试，覆盖 {边界情况}。\n跑一遍确认都过，失败的贴出来。", createdAt: NOW - 4 * DAY, updatedAt: NOW - 1 * DAY },
  { id: "prompt-commit", title: "写提交说明", body: "照项目说明里「Git」那节给这次的改动写一句提交说明，只给说明，不提交。", createdAt: NOW - 8 * DAY, updatedAt: NOW - 2 * DAY },
  { id: "prompt-handoff", title: "收尾更新 HANDOFF", body: "这一轮做完了。更新 HANDOFF.md：\n1. 只留当前状态、没验的、已知问题、下一步\n2. 不追加历史\n3. 没验过的不写「通过」", createdAt: NOW - 12 * DAY, updatedAt: NOW - 5 * DAY },
];

// ---------- 设置里的供应商（演示） ----------

export const MOCK_API_STATUS: ApiStatus = {
  mode: "mock",
  providers: [
    { id: "anthropic", name: "Anthropic" },
    { id: "deepseek", name: "DeepSeek" },
    { id: "openai", name: "OpenAI" },
    { id: "google", name: "Google Gemini" },
    { id: "aliyun", name: "Aliyun" },
  ],
  config: { aliyunRegion: "beijing" },
  keys: [
    { providerId: "anthropic", state: "saved" },
    { providerId: "deepseek", state: "saved" },
    { providerId: "openai", state: "missing" },
    { providerId: "google", state: "missing" },
    { providerId: "aliyun", state: "missing" },
    { providerId: "tavily", state: "missing" },
    { providerId: "mathpix_id", state: "missing" },
    { providerId: "mathpix_key", state: "missing" },
  ],
};
