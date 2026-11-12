// 浏览器 `?mock` 里替代 Rust IPC：数据来自 mockData.ts，只在内存，刷新重置。
import {
  MOCK_AGENTS,
  MOCK_API_STATUS,
  MOCK_API_TASKS,
  MOCK_DIFFS,
  MOCK_FILES,
  MOCK_HIDDEN,
  MOCK_MESSAGES,
  MOCK_RUNS,
  MOCK_SESSIONS,
  MOCK_WORKSPACES,
} from "@/dev/mockData";
import { MOCK_MODEL_ENTRIES } from "@/dev/gatewayMock";
import type { AgentEvent, GatewayConfig, GatewaySettings, ModelEntry } from "@/lib/agentEvents";
import type { ApiConfig, ApiRole, ApiStatus, ApiTask } from "@/lib/api";
import type {
  AgentInfo,
  DiffResult,
  FileChangeItem,
  FileListing,
  FilePreview,
  Message,
  Permission,
  ProjectOverview,
  RunEvent,
  RunRecord,
  Session,
  StepDescriptor,
  TaskItem,
  TermEvent,
  TermState,
  UndoPreview,
  Workspace,
} from "@/lib/types";

const DEMO_NOTICE = "浏览器演示数据，没有调用真实 AI。";
const emptyPreview = new URLSearchParams(window.location.search).has("empty");
let workspacesState: Workspace[] = emptyPreview ? [] : [...MOCK_WORKSPACES];
let sessionsState: Session[] = emptyPreview ? [] : [...MOCK_SESSIONS];
const messagesState: Record<string, Message[]> = structuredClone(MOCK_MESSAGES);
const runsState: Record<string, RunRecord[]> = structuredClone(MOCK_RUNS);
// 键是 `${runId}:${path}`；新发的任务也按这个键存。
const diffsState: Record<string, DiffResult> = structuredClone(MOCK_DIFFS);
const apiTasksState: Record<string, ApiTask[]> = structuredClone(MOCK_API_TASKS);
const filesState: Record<string, Record<string, string>> = structuredClone(MOCK_FILES);
const apiStatusState: ApiStatus = structuredClone(MOCK_API_STATUS);
let settingsState: string | null = null;
let runCounter = 13;

const activeTimers = new Map<string, number[]>();
const activeCallbacks = new Map<string, (event: RunEvent) => void>();
const configState: Record<string, string> = {};

const allRuns = () => Object.values(runsState).flat();
const findRun = (runId: string) => allRuns().find((r) => r.id === runId);
const workspaceOfSession = (sessionId: string) => sessionsState.find((s) => s.id === sessionId)?.workspaceId ?? "";
const workspaceIsBusy = (workspaceId: string) =>
  allRuns().some((r) => r.status === "running" && workspaceOfSession(r.sessionId) === workspaceId);

function clearRunTimers(runId: string) {
  activeTimers.get(runId)?.forEach((id) => window.clearTimeout(id));
  activeTimers.delete(runId);
}

function addedDiff(runId: string, stepId: string, path: string, body: string): DiffResult {
  const lines = body.replace(/\n$/, "").split("\n");
  return {
    runId, stepId, filePath: path, kind: "create", language: /\.md$/.test(path) ? "markdown" : "typescript",
    additions: lines.length, deletions: 0, contentAfter: body,
    unifiedDiff: `diff --git a/${path} b/${path}\nnew file mode 100644\n--- /dev/null\n+++ b/${path}\n@@ -0,0 +1,${lines.length} @@\n${lines.map((l) => `+${l}`).join("\n")}`,
  };
}

interface ScriptStep {
  step: StepDescriptor;
  lines: string[];
  files: FileChangeItem[];
  /** 这一步写下的 Diff 和项目文件（撤销时照着放回去）。 */
  apply: () => void;
}

/** 假的任务：一步一步发事件，几秒跑完；能停。 */
function simulate(record: RunRecord, script: ScriptStep[], summary: string, listener: (event: RunEvent) => void) {
  const runId = record.id;
  const onEvent = (event: RunEvent) => {
    if (event.type === "StepStarted") record.steps = record.steps.map((s) => s.id === event.payload.stepId ? { ...s, status: "running", startedAt: Date.now(), executionMode: "mock", notice: DEMO_NOTICE } : s);
    if (event.type === "StepOutput") record.logs.push({ id: `log-${runId}-${record.logs.length}`, runId, stepId: event.payload.stepId, agentId: "mock", stream: event.payload.stream, chunk: event.payload.chunk, timestamp: Date.now() });
    if (event.type === "TasksUpdated") record.tasks = event.payload.items;
    if (event.type === "StepFinished") {
      record.steps = record.steps.map((s) => s.id === event.payload.stepId ? { ...s, status: event.payload.status, durationMs: event.payload.durationMs, exitCode: event.payload.exitCode, finishedAt: Date.now() } : s);
      for (const file of event.payload.changeset.files) {
        const index = record.files.findIndex((f) => f.path === file.path);
        if (index < 0) record.files.push(file); else record.files[index] = file;
      }
    }
    if (event.type === "RunFinished") {
      record.status = event.payload.status; record.summary = event.payload.summary; record.finishedAt = Date.now();
      if (event.payload.status === "cancelled") record.steps = record.steps.map((s) => s.status === "running" || s.status === "pending" ? { ...s, status: "cancelled" } : s);
      activeCallbacks.delete(runId);
    }
    listener(event);
    if (event.type === "StepStarted") listener({ type: "StepExecution", payload: { runId, stepId: event.payload.stepId, mode: "mock", notice: DEMO_NOTICE } });
  };
  activeCallbacks.set(runId, onEvent);
  const timers: number[] = [];
  const at = (ms: number, fn: () => void) => timers.push(window.setTimeout(fn, ms));
  activeTimers.set(runId, timers);

  const tasks: TaskItem[] = script.flatMap((s, i) => s.files.map((f, j) => ({ id: `${runId}-t${i}-${j}`, text: `${f.kind === "create" ? "新建" : f.kind === "delete" ? "删掉" : "改"} ${f.path}`, completed: false })));
  at(40, () => onEvent({ type: "RunStarted", payload: { runId, flowId: record.flowId, flowName: record.flowName, steps: structuredClone(record.steps) } }));
  let t = 200;
  let done = 0;
  script.forEach((part, index) => {
    const started = t;
    at(t, () => onEvent({ type: "StepStarted", payload: { runId, stepId: part.step.id, stepIndex: index, agentId: part.step.agentId } }));
    part.lines.forEach((line) => {
      t += 450;
      at(t, () => onEvent({ type: "StepOutput", payload: { runId, stepId: part.step.id, stream: "stdout", chunk: line } }));
    });
    part.files.forEach((file) => {
      t += 500;
      at(t, () => {
        onEvent({ type: "FileTouched", payload: { runId, stepId: part.step.id, path: file.path, kind: file.kind } });
        done += 1;
        const items = tasks.map((task, i) => ({ ...task, completed: i < done }));
        onEvent({ type: "TasksUpdated", payload: { runId, stepId: part.step.id, total: items.length, completed: done, items } });
      });
    });
    t += 500;
    const finished = t;
    at(t, () => {
      part.apply();
      onEvent({
        type: "StepFinished",
        payload: {
          runId, stepId: part.step.id, status: "succeeded", exitCode: 0, durationMs: finished - started,
          changeset: { files: part.files, totalAdditions: part.files.reduce((n, f) => n + f.additions, 0), totalDeletions: part.files.reduce((n, f) => n + f.deletions, 0) },
        },
      });
    });
    t += 200;
  });
  at(t, () => {
    onEvent({ type: "RunFinished", payload: { runId, status: "succeeded", summary } });
    clearRunTimers(runId);
  });
}

function startRun(
  sessionId: string, prompt: string, permission: Permission,
  flowId: string, flowName: string, who: string | undefined,
  plan: Array<{ agentId: string; agentName: string; title: string; spec?: boolean }>,
  listener: (event: RunEvent) => void,
): string {
  const workspaceId = workspaceOfSession(sessionId);
  if (workspaceIsBusy(workspaceId)) throw new Error("同一个项目一次跑一个任务。");
  const number = runCounter++;
  const runId = `run-${number}`;
  const steps: StepDescriptor[] = plan.map((p, i) => ({
    id: `${runId}-step-${i + 1}`, stepIndex: i, agentId: p.agentId, agentName: p.agentName, title: p.title, status: "pending",
    expects: p.spec ? [`.peck/handoff/${number}/intent.md`, ".peck/tasks.md"] : [],
  }));
  const record: RunRecord = { id: runId, number, sessionId, flowId, flowName, prompt, status: "running", startedAt: Date.now(), steps, tasks: [], files: [], logs: [], who, permission };
  runsState[sessionId] = [...(runsState[sessionId] ?? []), record];
  messagesState[sessionId] = [
    ...(messagesState[sessionId] ?? []),
    { id: `msg-u-${number}`, sessionId, role: "user", content: prompt, createdAt: Date.now() },
    { id: `msg-a-${number}`, sessionId, role: "assistant", content: `交给 ${flowName}。改动在右栏。`, runId, createdAt: Date.now() + 1 },
  ];
  const session = sessionsState.find((s) => s.id === sessionId);
  if (session) session.updatedAt = Date.now();

  const files = (filesState[workspaceId] ??= {});
  const intentPath = `.peck/handoff/${number}/intent.md`;
  const hintPath = "src/components/TaskHint.tsx";
  const editPath = Object.keys(files).find((p) => /\.(tsx?|rs|go)$/.test(p) && !p.includes("legacy")) ?? "README.md";
  const gonePath = Object.keys(files).find((p) => p.includes("legacy"));
  const specBody = `# 规范（任务 ${number}）\n\n> ${prompt}\n\n## 要点\n1. 先看项目结构，定改法；\n2. 任务清单写在 \`.peck/tasks.md\`，做一条勾一条；\n3. 只改项目里的文件，任务前有快照。\n`;
  const hintBody = `// 任务 ${number} 新建（演示数据）\nexport function TaskHint() {\n  return <p>${prompt.slice(0, 24)}</p>;\n}\n`;

  const specStep = (step: StepDescriptor): ScriptStep => ({
    step,
    lines: [`[${step.agentId}] 读项目结构，找到 ${Object.keys(files).length} 个文件`, `[${step.agentId}] 写规范 → ${intentPath}`],
    files: [
      { path: intentPath, kind: "create", additions: 8, deletions: 0, stepId: step.id },
      { path: ".peck/tasks.md", kind: "create", additions: 3, deletions: 0, stepId: step.id },
    ],
    apply: () => {
      diffsState[`${runId}:${intentPath}`] = addedDiff(runId, step.id, intentPath, specBody);
      diffsState[`${runId}:.peck/tasks.md`] = addedDiff(runId, step.id, ".peck/tasks.md", `# 任务清单\n\n- [x] 改 ${editPath}\n`);
      files[intentPath] = specBody;
    },
  });
  const buildStep = (step: StepDescriptor): ScriptStep => {
    const before = files[editPath] ?? "";
    const firstLine = before.split("\n")[0] ?? "";
    const changed: FileChangeItem[] = [
      { path: editPath, kind: "modify", additions: 1, deletions: 0, stepId: step.id },
      { path: hintPath, kind: "create", additions: 4, deletions: 0, stepId: step.id },
    ];
    if (gonePath) changed.push({ path: gonePath, kind: "delete", additions: 0, deletions: (files[gonePath] ?? "").trimEnd().split("\n").length, stepId: step.id });
    return {
      step,
      lines: [`[${step.agentId}] 读 ${editPath}`, `[${step.agentId}] 改 ${editPath}，新建 ${hintPath}`, `[${step.agentId}] pnpm check … 通过`],
      files: changed,
      apply: () => {
        undoState[runId] = { workspaceId, restore: { [editPath]: before, ...(gonePath ? { [gonePath]: files[gonePath] } : {}) }, remove: [hintPath, intentPath] };
        const after = `// 任务 ${number} 改过这里（演示数据）\n${before}`;
        diffsState[`${runId}:${editPath}`] = {
          runId, stepId: step.id, filePath: editPath, kind: "modify", language: "typescript", additions: 1, deletions: 0, contentAfter: after,
          unifiedDiff: `diff --git a/${editPath} b/${editPath}\n--- a/${editPath}\n+++ b/${editPath}\n@@ -1,2 +1,3 @@\n+// 任务 ${number} 改过这里（演示数据）\n ${firstLine}`,
        };
        diffsState[`${runId}:${hintPath}`] = addedDiff(runId, step.id, hintPath, hintBody);
        files[editPath] = after;
        files[hintPath] = hintBody;
        if (gonePath) {
          const gone = files[gonePath] ?? "";
          const lines = gone.trimEnd().split("\n");
          diffsState[`${runId}:${gonePath}`] = {
            runId, stepId: step.id, filePath: gonePath, kind: "delete", language: "typescript", additions: 0, deletions: lines.length,
            unifiedDiff: `diff --git a/${gonePath} b/${gonePath}\ndeleted file mode 100644\n--- a/${gonePath}\n+++ /dev/null\n@@ -1,${lines.length} +0,0 @@\n${lines.map((l) => `-${l}`).join("\n")}`,
          };
          delete files[gonePath];
        }
      },
    };
  };
  const script = plan.map((p, i) => (p.spec ? specStep(steps[i]) : i === plan.length - 1 ? buildStep(steps[i]) : { step: steps[i], lines: [`[${steps[i].agentId}] 拆成 1 项交给实施的模型`], files: [], apply: () => {} }));
  const total = script.reduce((n, s) => n + s.files.length, 0);
  simulate(record, script, `改了 ${editPath}，新建了 TaskHint.tsx${gonePath ? "，删了没人用的 old-banner.ts" : ""}。一共 ${total} 个文件。\n验证：pnpm check 通过（演示数据，没有真的跑）。`, listener);
  return runId;
}

/** 演示里新任务的撤销：把文件放回去。 */
const undoState: Record<string, { workspaceId: string; restore: Record<string, string>; remove: string[] }> = {};

// ---------- 终端（演示） ----------
interface MockTerm { cwd: string; running: { id: string; timers: number[]; emit: (e: TermEvent) => void; started: number } | null }
const termState: Record<string, MockTerm> = {};
const CANNED: Array<{ match: RegExp; lines: string[]; code: number; ms: number }> = [
  { match: /^pnpm test/, lines: ["> vitest run", " PASS  src/lib/date.test.ts (4)", " FAIL  src/lib/export.test.ts (1)", "   expected 'tags: [x]' to be 'tags: [ x ]'", " Tests  1 failed | 4 passed"], code: 1, ms: 2400 },
  { match: /^pnpm (check|format)/, lines: ["> tsc --noEmit", "没有报错"], code: 0, ms: 1200 },
  { match: /^git status/, lines: ["On branch feat/search", "nothing to commit, working tree clean"], code: 0, ms: 150 },
  { match: /^git diff/, lines: [" src/pages/NoteList.tsx | 3 ++-", " 1 file changed, 2 insertions(+), 1 deletion(-)"], code: 0, ms: 150 },
  { match: /^pwd$/, lines: ["~/Documents/GitHub/notebook"], code: 0, ms: 60 },
];

export const mockIpc = {
  async workspaceOpen(path: string): Promise<Workspace> {
    const existing = workspacesState.find((w) => w.path === path);
    if (existing) {
      existing.lastOpenedAt = Date.now();
      return { ...existing };
    }
    const name = path.split("/").filter(Boolean).pop() || "project";
    const created: Workspace = { id: `ws-${Date.now()}`, path, name, lastOpenedAt: Date.now(), gitBranch: "main", gitignoreConfigured: true };
    workspacesState = [created, ...workspacesState];
    filesState[created.id] = { "README.md": `# ${name}\n` };
    return created;
  },

  async workspaceListRecent(): Promise<Workspace[]> {
    return [...workspacesState].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  },

  async overview(): Promise<ProjectOverview[]> {
    return (await this.workspaceListRecent()).map((workspace) => ({
      workspace,
      tasks: sessionsState
        .filter((s) => s.workspaceId === workspace.id && (runsState[s.id]?.length ?? 0) > 0)
        .sort((a, b) => b.updatedAt - a.updatedAt)
        .map((s) => {
          const runs = runsState[s.id] ?? [];
          const last = runs[runs.length - 1];
          return {
            id: s.id, workspaceId: s.workspaceId, title: s.title, createdAt: s.createdAt, updatedAt: s.updatedAt, runCount: runs.length,
            lastRun: last ? { id: last.id, number: last.number, flowId: last.flowId, flowName: last.flowName, status: last.status, startedAt: last.startedAt, finishedAt: last.finishedAt, files: [...last.files], who: last.who, undoneAt: last.undoneAt } : null,
          };
        }),
    }));
  },

  async sessionCreate(workspaceId: string, title?: string): Promise<Session> {
    const session: Session = { id: `sess-${Date.now()}`, workspaceId, title: title || "新任务", createdAt: Date.now(), updatedAt: Date.now() };
    sessionsState = [session, ...sessionsState];
    messagesState[session.id] = [];
    runsState[session.id] = [];
    return session;
  },

  async sessionList(workspaceId: string): Promise<Session[]> {
    return sessionsState.filter((s) => s.workspaceId === workspaceId).sort((a, b) => b.updatedAt - a.updatedAt);
  },

  async sessionGetMessages(sessionId: string): Promise<Message[]> {
    return [...(messagesState[sessionId] ?? [])];
  },

  async sessionGetRuns(sessionId: string): Promise<RunRecord[]> {
    return structuredClone(runsState[sessionId] ?? []);
  },

  async runDispatch(sessionId: string, prompt: string, who: string, permission: Permission, listener: (event: RunEvent) => void): Promise<string> {
    const ids = who.split(">");
    const name = (id: string) => (id === "mock" ? "假 Agent" : MOCK_AGENTS.find((a) => a.id === id)?.name ?? id);
    if (ids.length === 2) {
      return startRun(sessionId, prompt, permission, `pipe-${ids[0]}-${ids[1]}`, `${name(ids[0])} → ${name(ids[1])}`, who, [
        { agentId: ids[0], agentName: name(ids[0]), title: "写规范", spec: true },
        { agentId: ids[1], agentName: name(ids[1]), title: "实施" },
      ], listener);
    }
    return startRun(sessionId, prompt, permission, `single-${ids[0]}`, name(ids[0]), who, [{ agentId: ids[0], agentName: name(ids[0]), title: "跑完整个任务" }], listener);
  },

  /** API 模式（演示）：单模型干活一步，规划 + 实施两步。 */
  async apiRunStart(sessionId: string, prompt: string, role: ApiRole, permission: Permission, listener: (event: RunEvent) => void): Promise<string> {
    const model = (r: string) => {
      const id = apiStatusState.config.agents.find((a) => a.role === r)?.modelId;
      return apiStatusState.config.models.find((m) => m.id === id)?.name ?? "模型";
    };
    if (role === "supervisor") {
      return startRun(sessionId, prompt, permission, "api-supervisor", `规划 + 实施 · ${model("supervisor")}`, undefined, [
        { agentId: "api-supervisor", agentName: model("supervisor"), title: "规划和审查" },
        { agentId: "api-worker", agentName: model("worker"), title: "实施" },
      ], listener);
    }
    return startRun(sessionId, prompt, permission, "api-worker", `单模型干活 · ${model("worker")}`, undefined, [{ agentId: "api-worker", agentName: model("worker"), title: "单模型干活" }], listener);
  },

  async runCancel(runId: string): Promise<void> {
    clearRunTimers(runId);
    const live = activeCallbacks.get(runId);
    if (live) {
      live({ type: "RunFinished", payload: { runId, status: "cancelled", summary: "已停止：这一步没跑完。" } });
      return;
    }
    // 演示数据里本来就「在跑」的那个任务，没有计时器，直接改状态。
    const record = findRun(runId);
    if (record && record.status === "running") {
      record.status = "cancelled";
      record.finishedAt = Date.now();
      record.summary = "已停止：这一步没跑完。";
      record.steps = record.steps.map((s) => (s.status === "running" || s.status === "pending" ? { ...s, status: "cancelled" } : s));
    }
  },

  async runRetryStep(runId: string, stepId: string, onEvent: (event: RunEvent) => void): Promise<void> {
    const record = findRun(runId);
    if (!record) throw new Error("没找到这个任务");
    const step = record.steps.find((s) => s.id === stepId);
    if (!step) throw new Error("没找到这个步骤");
    record.status = "running";
    record.undoneAt = undefined;
    step.status = "running";
    const finish = (event: RunEvent) => {
      if (event.type === "RunFinished") {
        record.status = event.payload.status;
        record.summary = event.payload.summary;
        record.finishedAt = Date.now();
        record.steps = record.steps.map((s) => (s.stepIndex >= step.stepIndex ? { ...s, status: event.payload.status === "succeeded" ? "succeeded" : "cancelled", executionMode: "mock", notice: DEMO_NOTICE } : s));
        activeCallbacks.delete(runId);
      }
      onEvent(event);
    };
    activeCallbacks.set(runId, finish);
    onEvent({ type: "StepStarted", payload: { runId, stepId, stepIndex: step.stepIndex, agentId: step.agentId } });
    const retryTimer = window.setTimeout(() => {
      onEvent({ type: "StepOutput", payload: { runId, stepId, stream: "stdout", chunk: `[${step.agentId}] 这一步重跑完成（退出码 0）。` } });
      onEvent({ type: "StepFinished", payload: { runId, stepId, status: "succeeded", exitCode: 0, durationMs: 1800, changeset: { files: [], totalAdditions: 0, totalDeletions: 0 } } });
      finish({ type: "RunFinished", payload: { runId, status: "succeeded", summary: "这一步重跑完成。验证：演示数据，没有真实验证。" } });
      activeTimers.delete(runId);
    }, 1800);
    activeTimers.set(runId, [retryTimer]);
  },

  async changesGetDiff(runId: string, stepId?: string, filePath?: string): Promise<DiffResult> {
    const record = findRun(runId);
    const targetPath = filePath || record?.files[0]?.path || ".peck/tasks.md";
    const found = diffsState[`${runId}:${targetPath}`];
    if (found) return { ...found, runId, stepId };
    return {
      runId, stepId, filePath: targetPath, kind: "modify", language: "typescript", additions: 1, deletions: 0,
      unifiedDiff: `diff --git a/${targetPath} b/${targetPath}\n--- a/${targetPath}\n+++ b/${targetPath}\n@@ -1,2 +1,3 @@\n+// 任务 ${record?.number ?? runId} 改过这里（演示数据）\n export const updated = true;\n`,
    };
  },

  async runUndoPreview(runId: string): Promise<UndoPreview> {
    const record = findRun(runId);
    if (!record) throw new Error("没找到这个任务。");
    return { runId, files: record.files.map((f, i) => ({ path: f.path, kind: f.kind, changedSince: i === 0 && record.files.length > 3 && !undoState[runId] })) };
  },

  async runUndo(runId: string): Promise<RunRecord> {
    const record = findRun(runId);
    if (!record) throw new Error("没找到这个任务。");
    if (record.status === "running") throw new Error("任务还在跑，先停下来再撤销。");
    const undo = undoState[runId];
    if (undo) {
      const files = (filesState[undo.workspaceId] ??= {});
      undo.remove.forEach((p) => delete files[p]);
      Object.assign(files, undo.restore);
    }
    record.undoneAt = Date.now();
    return structuredClone(record);
  },

  async filesList(workspaceId: string): Promise<FileListing> {
    const files = filesState[workspaceId] ?? {};
    return {
      entries: Object.keys(files).sort().map((path) => ({ path, size: files[path].length || 18_400 })),
      hidden: MOCK_HIDDEN[workspaceId] ?? 0,
      truncated: false,
    };
  },

  async filesRead(workspaceId: string, path: string, runId?: string): Promise<FilePreview> {
    const files = filesState[workspaceId] ?? {};
    if (/\.(png|jpg|jpeg|gif|webp|icns)$/i.test(path)) return { path, kind: "image", content: null, size: 18_400, deleted: false };
    if (path in files) return { path, kind: "text", content: files[path], size: files[path].length, deleted: false };
    // 这个任务删掉的文件：给删之前的样子。
    const diff = runId ? diffsState[`${runId}:${path}`] : undefined;
    if (diff?.kind === "delete") {
      const before = diff.unifiedDiff.split("\n").filter((l) => l.startsWith("-") && !l.startsWith("---")).map((l) => l.slice(1)).join("\n");
      return { path, kind: "text", content: `${before}\n`, size: before.length, deleted: true };
    }
    if (diff?.contentAfter) return { path, kind: "text", content: diff.contentAfter, size: diff.contentAfter.length, deleted: false };
    return { path, kind: "missing", content: null, size: 0, deleted: false };
  },

  async settingsGet(): Promise<string | null> {
    return settingsState;
  },
  async settingsSet(value: string): Promise<void> {
    settingsState = value;
  },

  async termRun(workspaceId: string, command: string, emit: (event: TermEvent) => void): Promise<string> {
    if (workspaceIsBusy(workspaceId)) throw new Error("任务在跑，终端先锁着；做完就解开。");
    const term = (termState[workspaceId] ??= { cwd: "", running: null });
    if (term.running) throw new Error("上一条命令还在跑，先点 STOP。");
    const id = `cmd-${Date.now()}`;
    const started = Date.now();
    const timers: number[] = [];
    const finish = (code: number | null, stopped = false) => {
      term.running = null;
      emit({ type: "Exit", payload: { id, code, durationMs: Date.now() - started, cwd: term.cwd, stopped } });
    };
    term.running = { id, timers, emit, started };
    const cd = command.match(/^cd\s+(.+)$/);
    const canned = CANNED.find((c) => c.match.test(command));
    if (cd) {
      term.cwd = cd[1] === ".." || cd[1] === "~" ? "" : cd[1].replace(/\/$/, "");
      timers.push(window.setTimeout(() => finish(0), 60));
    } else if (/^(pnpm dev|sleep|tail -f|npm run dev)/.test(command)) {
      // 一直跑的命令：隔一会儿出一行，到点 STOP 为止。
      emit({ type: "Output", payload: { id, chunk: "  VITE v6  ready in 412 ms\r\n  ➜  Local:   http://localhost:5173/\r\n" } });
      const tick = () => {
        emit({ type: "Output", payload: { id, chunk: `${new Date().toLocaleTimeString("zh-CN", { hour12: false })} [vite] hmr update /src/pages/NoteList.tsx\r\n` } });
        timers.push(window.setTimeout(tick, 4000));
      };
      timers.push(window.setTimeout(tick, 4000));
    } else if (/^ls\b/.test(command)) {
      const names = new Set(Object.keys(filesState[workspaceId] ?? {}).filter((p) => p.startsWith(term.cwd ? `${term.cwd}/` : "")).map((p) => p.slice(term.cwd ? term.cwd.length + 1 : 0).split("/")[0]));
      emit({ type: "Output", payload: { id, chunk: `${[...names].sort().join("\r\n")}\r\n` } });
      timers.push(window.setTimeout(() => finish(0), 80));
    } else if (canned) {
      const gap = canned.ms / (canned.lines.length + 1);
      canned.lines.forEach((line, i) => timers.push(window.setTimeout(() => emit({ type: "Output", payload: { id, chunk: `${line}\r\n` } }), gap * (i + 1))));
      timers.push(window.setTimeout(() => finish(canned.code), canned.ms));
    } else {
      emit({ type: "Output", payload: { id, chunk: `zsh: command not found: ${command.split(/\s+/)[0]}（浏览器演示只认 ls、cd、pwd、git status、git diff、pnpm check、pnpm test、pnpm dev）\r\n` } });
      timers.push(window.setTimeout(() => finish(127), 80));
    }
    return id;
  },
  async termStop(workspaceId: string): Promise<void> {
    const running = termState[workspaceId]?.running;
    if (!running) return;
    running.timers.forEach((t) => window.clearTimeout(t));
    termState[workspaceId].running = null;
    running.emit({ type: "Output", payload: { id: running.id, chunk: "^C\r\n" } });
    running.emit({ type: "Exit", payload: { id: running.id, code: 130, durationMs: Date.now() - running.started, cwd: termState[workspaceId].cwd, stopped: true } });
  },
  async termState(workspaceId: string): Promise<TermState> {
    const term = termState[workspaceId];
    return { cwd: term?.cwd ?? "", running: term?.running?.id ?? null };
  },

  async notify(title: string, body: string): Promise<void> {
    console.info(`[通知] ${title}：${body}`);
  },

  /** API 模式任务的调用与审查历史。 */
  async apiTaskHistory(runId: string): Promise<ApiTask[]> {
    return structuredClone(apiTasksState[runId] ?? []);
  },
  async apiStatus(): Promise<ApiStatus> {
    return structuredClone(apiStatusState);
  },
  async apiSaveConfig(config: ApiConfig): Promise<void> {
    apiStatusState.config = structuredClone(config);
  },
  async apiSetKey(providerId: string, key: string | null): Promise<void> {
    apiStatusState.keys = apiStatusState.keys.map((k) => (k.providerId === providerId ? { ...k, state: key ? "saved" : "missing" } : k));
  },

  async agentsList(): Promise<AgentInfo[]> {
    return [...MOCK_AGENTS];
  },
  async agentGetConfig(agentId: string): Promise<string> {
    const agent = MOCK_AGENTS.find((a) => a.id === agentId);
    if (!agent) throw new Error("没找到这个 CLI");
    return configState[agentId] ?? `id = "${agent.id}"\nname = "${agent.name}"\ncommand = "${agent.command}"\nargs = ${JSON.stringify(agent.args)}\ncwd = "{{workspace}}"\nstdin = "none"\ntimeout = "${agent.timeout}"\npath = []\n\n[env]\n`;
  },
  async agentSaveConfig(agentId: string, content: string): Promise<void> {
    configState[agentId] = content;
  },

  // ── Model Gateway（v0.5 契约）──
  async gatewayModels(): Promise<ModelEntry[]> {
    return structuredClone(MOCK_MODEL_ENTRIES);
  },
  async gatewayConfigGet(): Promise<GatewaySettings> {
    return { config: structuredClone(gatewayConfigState), adc: false };
  },
  async gatewayConfigSet(config: GatewayConfig): Promise<GatewayConfig> {
    const projectId = config.vertex.projectId.trim();
    const location = config.vertex.location.trim().toLowerCase() || "global";
    if (projectId && !/^[a-z][a-z0-9-]{4,28}[a-z0-9]$/.test(projectId)) throw new Error("Project ID 不对：6–30 个字符，小写字母开头，只有小写字母、数字和连字符。");
    if (location !== "global" && !/^[a-z]+(-[a-z0-9]+)+\d$/.test(location)) throw new Error("Location 不对：填 global，或 us-east5 这样的区域。");
    gatewayConfigState = { vertex: { projectId, location }, cliDir: config.cliDir.trim() };
    return structuredClone(gatewayConfigState);
  },
  async gatewayEvents(_runId: string): Promise<AgentEvent[]> {
    return [];
  },
};

let gatewayConfigState: GatewayConfig = { vertex: { projectId: "", location: "global" }, cliDir: "" };
