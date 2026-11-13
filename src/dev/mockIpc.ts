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
import { MOCK_ENGINE_REPLAYS, MOCK_MODEL_ENTRIES, type ReplayEvent } from "@/dev/gatewayMock";
import { MOCK_SPILLS, PECK_CAPS, oneshotDemo, peckDemo, type DemoCheck, type Round } from "@/dev/peckMock";
import type { AgentEvent, CliAccountStatus, CliLoginState, Control, EngineAccounts, FileDiff, GatewayConfig, GatewaySettings, GatewayRunArgs, ModelEntry, TurnEndReason } from "@/lib/agentEvents";
import type { AllowRule, BgInfo, HarnessRunArgs, MemoryNote, OneshotArgs, SpendOverview, SpendPoint, VerifyConfig, VerifyState } from "@/lib/ipc";
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
  ProjectSettings,
  Workspace,
} from "@/lib/types";

const DEMO_NOTICE = "浏览器演示数据，没有调用真实 AI。";
const emptyPreview = new URLSearchParams(window.location.search).has("empty");
// 演示「文件夹不在了」：多一个找不到文件夹的项目（`?mock&empty` 时没有）。
const MISSING_DEMO: Workspace = { id: "ws-old-site", path: "~/code/old-site", name: "old-site", lastOpenedAt: Date.now() - 20 * 86_400_000, gitBranch: "main", gitignoreConfigured: true };
let workspacesState: Workspace[] = emptyPreview ? [] : [...MOCK_WORKSPACES, MISSING_DEMO];
const missingFolders = new Set<string>(emptyPreview ? [] : [MISSING_DEMO.path]);
const layoutState: { projects: string[]; tasks: Record<string, string[]> } = { projects: [], tasks: {} };
/** 拖过的按拖的排；没拖过的（新的）排在前面，照原来的先后。 */
function arrange<T>(items: T[], order: string[] | undefined, id: (t: T) => string): T[] {
  if (!order?.length) return items;
  const rank = new Map(order.map((x, i) => [x, i]));
  return [...items.filter((t) => !rank.has(id(t))), ...items.filter((t) => rank.has(id(t))).sort((a, b) => rank.get(id(a))! - rank.get(id(b))!)];
}
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
      existing.hidden = false;
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
    const visible = arrange((await this.workspaceListRecent()).filter((w) => !w.hidden), layoutState.projects, (w) => w.id);
    return visible.map((workspace) => ({
      workspace,
      missing: missingFolders.has(workspace.path),
      tasks: arrange(sessionsState
        .filter((s) => s.workspaceId === workspace.id && (runsState[s.id]?.length ?? 0) > 0)
        .sort((a, b) => b.updatedAt - a.updatedAt)
        .map((s) => {
          const runs = runsState[s.id] ?? [];
          const last = runs[runs.length - 1];
          return {
            id: s.id, workspaceId: s.workspaceId, title: s.title, createdAt: s.createdAt, updatedAt: s.updatedAt, runCount: runs.length,
            lastRun: last ? { id: last.id, number: last.number, flowId: last.flowId, flowName: last.flowName, status: last.status, startedAt: last.startedAt, finishedAt: last.finishedAt, files: [...last.files], who: last.who, undoneAt: last.undoneAt } : null,
          };
        }), layoutState.tasks[workspace.id], (t) => t.id),
    }));
  },

  async workspaceUpdate(id: string, patch: { name?: string; settings?: ProjectSettings }): Promise<Workspace> {
    const ws = workspacesState.find((w) => w.id === id);
    if (!ws) throw new Error("没找到这个项目。");
    if (patch.name != null) {
      if (!patch.name.trim()) throw new Error("名字不能空着。");
      ws.name = patch.name.trim();
    }
    if (patch.settings) ws.settings = { ...patch.settings };
    return { ...ws };
  },
  async workspaceRemove(id: string): Promise<void> {
    if (workspaceIsBusy(id)) throw new Error("这个项目有任务在跑，先停下再移出。");
    const ws = workspacesState.find((w) => w.id === id);
    if (ws) ws.hidden = true;
  },
  async workspaceRelocate(id: string, path?: string): Promise<Workspace | null> {
    const ws = workspacesState.find((w) => w.id === id);
    if (!ws) throw new Error("没找到这个项目。");
    // 浏览器里没有选文件夹的窗：演示就当选了同名的新位置。
    missingFolders.delete(ws.path);
    ws.path = path ?? ws.path.replace(/^~\/code\//, "~/Documents/GitHub/");
    filesState[ws.id] ??= { "README.md": `# ${ws.name}\n` };
    return { ...ws };
  },
  async sessionRename(id: string, title: string): Promise<void> {
    const s = sessionsState.find((x) => x.id === id);
    if (!s) throw new Error("没找到这个任务。");
    if (!title.trim()) throw new Error("名字不能空着。");
    s.title = title.trim();
  },
  async sessionDelete(id: string): Promise<void> {
    if ((runsState[id] ?? []).some((r) => r.status === "running")) throw new Error("这个任务在跑，先停下再删。");
    sessionsState = sessionsState.filter((s) => s.id !== id);
    delete runsState[id];
    delete messagesState[id];
  },
  async layoutSave(args: { projects?: string[]; workspaceId?: string; tasks?: string[] }): Promise<void> {
    if (args.projects) layoutState.projects = [...args.projects];
    if (args.workspaceId && args.tasks) layoutState.tasks[args.workspaceId] = [...args.tasks];
  },
  async spendOverview(days: number): Promise<SpendOverview> {
    return mockSpend(days);
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
    const agent = agentRuns.get(runId);
    if (agent) { agent.stopped = true; agent.waiting?.({ type: "interrupt" }); return; }
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
  async gatewayEvents(runId: string): Promise<AgentEvent[]> {
    return structuredClone(agentEventsState[runId] ?? []);
  },

  // ── CLI 登录（假的：登录几秒后自动成功；只有客户端那一个账号）──
  async cliAccountsList(checkAll: boolean): Promise<EngineAccounts[]> {
    if (checkAll) for (const g of accountState) for (const a of g.accounts) a.status = mockAccountStatus(g.engine, a.id);
    return structuredClone(accountState);
  },
  async cliAccountCheck(engine: string, id: string): Promise<CliAccountStatus> {
    const status = mockAccountStatus(engine, id);
    const a = accountGroup(engine).accounts.find((x) => x.id === id);
    if (a) a.status = status;
    return status;
  },
  async cliAccountLogin(engine: string, id: string): Promise<CliLoginState> {
    loginStarted.set(`${engine}/${id}`, Date.now());
    return this.cliAccountLoginState(engine, id);
  },
  async cliAccountLoginState(engine: string, id: string): Promise<CliLoginState> {
    const key = `${engine}/${id}`;
    const at = loginStarted.get(key);
    if (at == null) return { running: false, output: "" };
    const url = engine === "codex" ? "https://auth.openai.com/codex/device" : "https://claude.com/cai/oauth/authorize?code=true&client_id=mock";
    const output = engine === "codex" ? "在浏览器里打开链接，输入一次性代码：MOCK-1234\n" : "Opening browser to sign in…\n";
    if (Date.now() - at < 4000) return { running: true, url, output };
    loggedIn.add(key);
    return { running: false, url, output: output + "Login successful.\n", ok: true };
  },
  async cliAccountLoginInput(_engine: string, _id: string, _text: string): Promise<void> {},
  async cliAccountLoginCancel(engine: string, id: string): Promise<void> {
    loginStarted.delete(`${engine}/${id}`);
  },

  // ── 发任务（Gateway 版）：回放这个引擎录下来的过程；任务记录照旧进左栏；碰到审批、提问停下，等 gatewayControl ──
  async gatewayRunStart(args: GatewayRunArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    const engine = args.who.replace(/^cli:/, "");
    const replay = MOCK_ENGINE_REPLAYS[engine];
    if (!args.who.startsWith("cli:") || !replay) throw new Error("这个入口只跑 CLI 引擎（cli:…）。");
    if (!args.prompt.trim()) throw new Error("先写一句话。");
    const label = MOCK_MODEL_ENTRIES.find((e) => e.id === args.who)?.label ?? engine;
    const { runId, record } = createAgentRecord(args.sessionId, args.prompt, args.who, args.permission ?? "edit", `cli-${engine}`, label);
    void replayAgent(runId, [replay], [], onEvent, { costCap: null, countSpend: false }).then((end) => finishAgentRecord(record, end, label));
    return runId;
  },
  async gatewayControl(runId: string, control: Control): Promise<void> {
    const run = agentRuns.get(runId);
    if (!run) throw new Error("这个任务已经结束了。");
    if (control.type === "interrupt") run.stopped = true;
    run.waiting?.(control);
  },

  // ── Peck 循环（Phase 6）：照剧本演一遍（读 → 搜 → 改 → 跑 → 高危卡片 → 清单 → 问 → 收尾）；花费上限设得很低会停 ──
  async harnessRunStart(args: HarnessRunArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (!args.who.startsWith("api:")) throw new Error("这个入口只跑 Peck 循环（api:…）。");
    if (!args.prompt.trim()) throw new Error("先写一句话。");
    const model = args.who.slice(4);
    const entry = MOCK_MODEL_ENTRIES.find((e) => e.id === args.who);
    const label = `Peck · ${entry?.label ?? model}`;
    const { runId, record, workspaceId } = createAgentRecord(args.sessionId, args.prompt, args.who, args.permission ?? "edit", "peck", label);
    const files = (filesState[workspaceId] ??= {});
    const verify = mockVerify(workspaceId);
    const checks: DemoCheck[] = verify.config?.check.map((c) => ({ name: c.name, run: c.run })) ?? [];
    const project = (memoryState[workspaceId]?.length ?? 0) > 0 || "AGENTS.md" in files || "CLAUDE.md" in files;
    const demo = peckDemo(runId, files, args.prompt, { window: entry?.contextWindow ?? 1_000_000, checks, goal: args.goal?.[0] ?? null, project });
    const header: ReplayEvent[] = [
      { type: "session.started", engine: "peck", model, caps: PECK_CAPS, tools: ["list_directory", "read_file", "search_text", "find_files", "write_file", "write_patch", "bash_run", "update_plan", "ask_user"], mcp: [], sessionRef: `peck-${args.sessionId}`, version: null },
      { type: "turn.started", turnId: `turn-${runId}` },
    ];
    void replayAgent(runId, demo.rounds, header, onEvent, { costCap: args.costCap ?? null, countSpend: true }).then((end) => {
      if (end.diffs.some((d) => d.path === demo.edit.path)) {
        files[demo.edit.path] = demo.edit.after;
        undoState[runId] = { workspaceId, restore: { [demo.edit.path]: demo.edit.before }, remove: [] };
      }
      finishAgentRecord(record, end, label);
    });
    return runId;
  },
  async harnessOneshot(args: OneshotArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (!args.prompt.trim()) throw new Error("先写一句话。");
    const engine = args.who.startsWith("api:") ? "peck" : args.who.replace(/^cli:/, "");
    if (engine === "antigravity") throw new Error("Antigravity 只做直接调用，不进 ONE SHOT。");
    const id = `oneshot-${++oneshotCounter}`;
    const header: ReplayEvent[] = [
      { type: "session.started", engine, model: engine === "peck" ? args.who.slice(4) : null, caps: PECK_CAPS, tools: [], mcp: [], sessionRef: null, version: null },
      { type: "turn.started", turnId: `turn-${id}` },
    ];
    void replayAgent(id, oneshotDemo(args.prompt, filesState[args.workspaceId] ?? {}, engine), header, onEvent, { costCap: args.costCap ?? null, countSpend: engine === "peck" });
    return id;
  },
  async harnessOutputRead(path: string): Promise<string> {
    const text = MOCK_SPILLS[path];
    if (text == null) throw new Error("这份输出不在了。");
    return text;
  },
  async harnessAllowList(): Promise<AllowRule[]> {
    return mockAllow.slice();
  },
  async harnessAllowRemove(workspaceId: string, rule: string): Promise<AllowRule[]> {
    mockAllow = mockAllow.filter((r) => !(r.workspaceId === workspaceId && r.rule === rule));
    return mockAllow.slice();
  },
  async harnessSpend(): Promise<{ monthUsd: number; since: number }> {
    const now = new Date();
    return { monthUsd: mockMonthSpend, since: Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1) };
  },

  // ── 记忆（v0.6）──
  async memoryList(workspaceId: string): Promise<MemoryNote[]> {
    return structuredClone(memoryState[workspaceId] ?? []);
  },
  async memoryDelete(workspaceId: string, id: string): Promise<MemoryNote[]> {
    memoryState[workspaceId] = (memoryState[workspaceId] ?? []).filter((m) => m.id !== id);
    return structuredClone(memoryState[workspaceId]);
  },
  async memoryReview(runId: string, keep: string[], skip: string[]): Promise<AgentEvent> {
    const log = agentEventsState[runId];
    if (!log) throw new Error("没找到这个任务。");
    if (log.some((e) => e.type === "memory.reviewed")) throw new Error("这几条已经批过了。");
    const sessionId = Object.keys(runsState).find((sid) => runsState[sid].some((r) => r.id === runId));
    const workspaceId = sessionId ? workspaceOfSession(sessionId) : "";
    const list = (memoryState[workspaceId] ??= []);
    const saved: string[] = [];
    for (const text of keep.map((t) => t.trim()).filter(Boolean)) {
      if (list.some((m) => m.text === text)) continue;
      list.push({ id: `mem-${++memoryCounter}`, text, sourceRun: runId, createdAt: Date.now() });
      saved.push(text);
    }
    const event = { type: "memory.reviewed", saved, skipped: skip, seq: log.length, at: Date.now() } as AgentEvent;
    log.push(event);
    return structuredClone(event);
  },

  // ── 验证和后台（v0.6）──
  async verifyGet(workspaceId: string): Promise<VerifyState> {
    return structuredClone(mockVerify(workspaceId));
  },
  async verifySave(workspaceId: string, text: string): Promise<VerifyState> {
    const config = parseVerifyToml(text);
    verifyState[workspaceId] = { exists: true, text, config, error: null, guessed: null, dismissed: verifyState[workspaceId]?.dismissed ?? false, approved: true };
    filesState[workspaceId] = { ...(filesState[workspaceId] ?? {}), ".peck/verify.toml": text };
    return structuredClone(verifyState[workspaceId]);
  },
  async verifyDismiss(workspaceId: string): Promise<VerifyState> {
    const v = mockVerify(workspaceId);
    v.dismissed = true;
    return structuredClone(v);
  },
  async backgroundList(workspaceId: string): Promise<BgInfo[]> {
    return structuredClone(Object.values(serverState[workspaceId] ?? {}));
  },
  async backgroundOutput(id: string): Promise<string> {
    const s = Object.values(serverState).flatMap((m) => Object.values(m)).find((b) => b.id === id);
    if (!s) throw new Error("这个后台命令不在了。");
    return `  VITE v6.4.5  ready in 312 ms\n\n  ➜  Local:   http://localhost:5173/\n`;
  },
  async backgroundStop(id: string): Promise<void> {
    for (const m of Object.values(serverState)) for (const b of Object.values(m)) if (b.id === id && b.status === "running") { b.status = "stopped"; b.endedAt = Date.now(); }
  },
  async serverStart(workspaceId: string, name: string): Promise<BgInfo> {
    const server = mockVerify(workspaceId).config?.server.find((x) => x.name === name);
    if (!server) throw new Error(`verify.toml 里没有叫「${name}」的 server。`);
    const running = serverState[workspaceId]?.[name];
    if (running?.status === "running") return structuredClone(running);
    const info: BgInfo = { id: `srv-${name}-${++memoryCounter}`, name, command: server.run, kind: "server", status: "running", exitCode: null, ready: false, startedAt: Date.now(), endedAt: null, owner: null, log: `/mock/harness-output/servers/${name}.log` };
    (serverState[workspaceId] ??= {})[name] = info;
    window.setTimeout(() => { info.ready = true; }, 1500);
    return structuredClone(info);
  },
};

// ── 记忆、验证命令、dev server（v0.6，演示）──
let memoryCounter = 0;
const memoryState: Record<string, MemoryNote[]> = {
  "ws-peck": [
    { id: "mem-a", text: "提交说明照站主的写法：v<版本>: <一句小写英文>", sourceRun: "run-14", createdAt: Date.now() - 5 * 86_400_000 },
    { id: "mem-b", text: "界面文案用中文，专有名词保留英文（Muse、Grok、API、Key）", sourceRun: null, createdAt: Date.now() - 2 * 86_400_000 },
  ],
};
const PECK_VERIFY = `# Peck 的验证命令：模型改了文件，Peck 在后台替它跑；模型停下时没过就把失败交回去修（最多 3 次）。

[[check]]
name = "check"
run = "pnpm check"
when = ["**/*.{ts,tsx}"]

[[check]]
name = "cargo-test"
run = "cargo test --manifest-path src-tauri/Cargo.toml"
when = ["**/*.rs", "**/Cargo.toml"]
timeout = 600

[[server]]
name = "dev"
run = "pnpm dev"
ready = "Local:"
autostart = false
`;
const NOTEBOOK_GUESS = `# Peck 的验证命令：模型改了文件，Peck 在后台替它跑；模型停下时没过就把失败交回去修（最多 3 次）。

[[check]]
name = "check"
run = "pnpm check"
when = ["**/*.{ts,tsx,js,jsx,mjs,cjs,vue,svelte,css}"]

[[check]]
name = "test"
run = "pnpm test"
when = ["**/*.{ts,tsx,js,jsx,mjs,cjs,vue,svelte,css}"]

[[server]]
name = "dev"
run = "pnpm dev"
autostart = false
`;
const verifyState: Record<string, VerifyState> = {
  "ws-peck": { exists: true, text: PECK_VERIFY, config: parseVerifyToml(PECK_VERIFY), error: null, guessed: null, dismissed: false, approved: true },
  "ws-notebook": { exists: false, text: null, config: null, error: null, guessed: NOTEBOOK_GUESS, dismissed: false, approved: false },
};
const serverState: Record<string, Record<string, BgInfo>> = {};

function mockVerify(workspaceId: string): VerifyState {
  return (verifyState[workspaceId] ??= { exists: false, text: null, config: null, error: null, guessed: null, dismissed: false, approved: false });
}

/** 演示用的极简 TOML：[[check]]、[[server]]，值是字符串、字符串数组、数字、true / false。 */
function parseVerifyToml(text: string): VerifyConfig {
  const config: VerifyConfig = { check: [], server: [] };
  let cur: Record<string, unknown> | null = null;
  text.split("\n").forEach((raw, i) => {
    const line = raw.replace(/#.*$/, "").trim();
    if (!line) return;
    if (line === "[[check]]") { cur = { name: "", run: "" }; config.check.push(cur as never); return; }
    if (line === "[[server]]") { cur = { name: "", run: "", autostart: false }; config.server.push(cur as never); return; }
    const m = line.match(/^(\w+)\s*=\s*(.+)$/);
    if (!m || !cur) throw new Error(`verify.toml 写得不对：第 ${i + 1} 行看不懂。`);
    try { cur[m[1]] = JSON.parse(m[2]); } catch { throw new Error(`verify.toml 写得不对：第 ${i + 1} 行的值看不懂。`); }
  });
  for (const c of [...config.check, ...config.server]) {
    if (!c.name?.trim()) throw new Error("每一条都要有名字。");
    if (!c.run?.trim()) throw new Error(`「${c.name}」没写 run（要跑的命令）。`);
  }
  return config;
}

// ── 统一事件的任务（CLI 引擎、Peck 循环、ONE SHOT）：回放 ──
const agentRuns = new Map<string, { waiting: null | ((c: Control) => void); stopped: boolean }>();
const agentEventsState: Record<string, AgentEvent[]> = {};
/** 这个月 Peck 循环（API）已经花了多少（演示）。 */
let mockMonthSpend = 0.42;
// 设置 → 权限里的「总是允许」：演示两条。
let mockAllow: AllowRule[] = [
  { workspaceId: "ws-peck", workspace: "~/Documents/GitHub/personal-cli-studio/peck", rule: "install-deps", createdAt: Date.now() - 3_600_000 },
  { workspaceId: "ws-notebook", workspace: "~/code/notebook", rule: "long-running", createdAt: Date.now() - 86_400_000 },
];
let oneshotCounter = 0;

interface AgentEnd { reason: TurnEndReason; error: string | null; text: string; diffs: FileDiff[] }

function createAgentRecord(sessionId: string, prompt: string, who: string, permission: Permission, flowId: string, label: string) {
  const workspaceId = workspaceOfSession(sessionId);
  if (workspaceIsBusy(workspaceId)) throw new Error("同一个项目一次跑一个任务。");
  const number = runCounter++;
  const runId = `run-${number}`;
  const engine = who.replace(/^(api|cli):/, "").split("/")[0];
  const step: StepDescriptor = { id: `${runId}-step-1`, stepIndex: 0, agentId: who.startsWith("api:") ? "peck" : engine, agentName: label, title: label, status: "running", expects: [], startedAt: Date.now() };
  const record: RunRecord = { id: runId, number, sessionId, flowId, flowName: label, prompt, status: "running", startedAt: Date.now(), steps: [step], tasks: [], files: [], logs: [], who, permission };
  runsState[sessionId] = [...(runsState[sessionId] ?? []), record];
  messagesState[sessionId] = [...(messagesState[sessionId] ?? []), { id: `msg-u-${number}`, sessionId, role: "user", content: prompt, createdAt: Date.now() }];
  const session = sessionsState.find((s) => s.id === sessionId);
  if (session) session.updatedAt = Date.now();
  return { runId, record, workspaceId };
}

function finishAgentRecord(record: RunRecord, end: AgentEnd, label: string) {
  record.status = end.reason === "done" ? "succeeded" : end.reason === "interrupted" || end.reason === "cancelled" ? "cancelled" : "failed";
  record.finishedAt = Date.now();
  record.summary = end.reason === "done" ? end.text || `${label} 做完了。` : record.status === "cancelled" ? "已停止。" : end.error ?? "没做完。";
  const stepId = record.steps[0]?.id ?? "";
  record.steps = record.steps.map((s) => ({ ...s, status: record.status === "succeeded" ? "succeeded" : record.status === "cancelled" ? "cancelled" : "failed", finishedAt: Date.now(), durationMs: Date.now() - (s.startedAt ?? Date.now()), executionMode: "mock", notice: DEMO_NOTICE }));
  record.files = end.diffs.map((d) => ({ path: d.path, kind: d.change === "added" ? "create" : d.change === "deleted" ? "delete" : "modify", additions: d.additions ?? 0, deletions: d.deletions ?? 0, stepId }));
  for (const d of end.diffs) {
    diffsState[`${record.id}:${d.path}`] = {
      runId: record.id, stepId, filePath: d.path, kind: d.change === "added" ? "create" : d.change === "deleted" ? "delete" : "modify",
      language: /\.tsx?$/.test(d.path) ? "typescript" : "text", additions: d.additions ?? 0, deletions: d.deletions ?? 0,
      unifiedDiff: d.patch ?? "", contentAfter: undefined,
    } as DiffResult;
  }
  messagesState[record.sessionId] = [...(messagesState[record.sessionId] ?? []), { id: `msg-a-${record.number}`, sessionId: record.sessionId, role: "assistant", content: record.summary, runId: record.id, createdAt: Date.now() }];
}

/** 一轮一轮演；每轮开头看花费上限。碰到审批、提问停下等回答；被拒的命令换成「没批」。 */
async function replayAgent(
  runId: string, rounds: Round[], header: ReplayEvent[], onEvent: (event: AgentEvent) => void,
  opts: { costCap: number | null; countSpend: boolean },
): Promise<AgentEnd> {
  const run = { waiting: null as null | ((c: Control) => void), stopped: false };
  agentRuns.set(runId, run);
  const log: AgentEvent[] = [];
  agentEventsState[runId] = log;
  let seq = 0;
  let ended = false;
  const send = (body: ReplayEvent) => {
    const event = { ...(body as object), seq: seq++, at: Date.now() } as AgentEvent;
    if (event.type === "turn.completed") ended = true;
    log.push(event);
    onEvent(event);
  };
  const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const state = { answer: "" };
  const denied = new Set<string>();
  const diffs = new Map<string, FileDiff>();
  let spent = 0;
  let text = "";
  let limit: string | null = null;
  // 剧本自己写了收尾（验证反复失败停下这类）：照它的原因结束。
  let scripted: { reason: TurnEndReason; error: string | null } | null = null;
  for (const e of header) send(e);
  for (let i = 0; i < rounds.length && !run.stopped && !scripted; i++) {
    const cap = opts.costCap;
    if (opts.countSpend && cap != null && cap > 0 && mockMonthSpend + spent >= cap) {
      limit = `这个月的 API 花费到了上限（$${cap.toFixed(2)}，已用 $${(mockMonthSpend + spent).toFixed(2)}），没发下一次请求。在设置里调高上限再接着做。`;
      break;
    }
    const round = rounds[i];
    for (const ev of typeof round === "function" ? round(state) : round) {
      if (run.stopped) break;
      await pause(ev.type.endsWith(".delta") ? 35 : 150);
      if (ev.type === "tool_call.output_delta" && denied.has(ev.callId)) continue;
      if (ev.type === "tool_call.completed" && denied.has(ev.callId)) {
        send({ ...ev, status: "denied", output: "站主没批这条命令。换个办法，或者说明为什么非这样不可。", exitCode: null });
        continue;
      }
      send(ev);
      if (ev.type === "turn.completed") { scripted = { reason: ev.reason, error: ev.error ?? null }; break; }
      if (ev.type === "usage.updated" && ev.cost.usd != null) spent += ev.cost.usd;
      if (ev.type === "message.completed") text = ev.text;
      if (ev.type === "tool_call.completed") for (const d of ev.diff ?? []) diffs.set(d.path, d);
      if (ev.type === "approval.requested" || ev.type === "user_input.requested") {
        let control: Control;
        do {
          control = await new Promise<Control>((resolve) => { run.waiting = resolve; });
          run.waiting = null;
        } while (control.type === "append");
        if (control.type === "interrupt") { run.stopped = true; break; }
        if (control.type === "approve") {
          send({ type: "approval.resolved", requestId: control.requestId, decision: control.decision, by: "user" });
          if (control.decision.startsWith("deny") && ev.type === "approval.requested" && ev.callId) denied.add(ev.callId);
        }
        if (control.type === "answer") {
          send({ type: "user_input.resolved", requestId: control.requestId, answer: control.answer });
          state.answer = control.answer;
        }
      }
    }
  }
  const reason: TurnEndReason = run.stopped ? "interrupted" : scripted ? scripted.reason : limit ? "limit" : "done";
  if (scripted && !limit) limit = scripted.error;
  if (!ended || run.stopped) send({ type: "turn.completed", turnId: `turn-${runId}`, reason, error: limit });
  if (opts.countSpend) mockMonthSpend += spent;
  agentRuns.delete(runId);
  return { reason, error: limit, text, diffs: [...diffs.values()] };
}

const loginStarted = new Map<string, number>();
const loggedIn = new Set<string>(["claude-code/default", "codex/default"]);
let accountState: EngineAccounts[] = [
  { engine: "claude-code", label: "Claude Code", defaultNote: "用 ~/.claude，和 Claude 客户端是同一个账号", accounts: [{ id: "default", label: "客户端的账号", isDefault: true, current: true, status: null }] },
  { engine: "codex", label: "Codex", defaultNote: "用 ~/.codex，和 ChatGPT 客户端是同一个账号", accounts: [{ id: "default", label: "客户端的账号", isDefault: true, current: true, status: null }] },
];
function accountGroup(engine: string): EngineAccounts {
  const g = accountState.find((x) => x.engine === engine);
  if (!g) throw new Error(`${engine} 不在 Peck 里登录。`);
  return g;
}
function mockAccountStatus(engine: string, id: string): CliAccountStatus {
  if (!loggedIn.has(`${engine}/${id}`)) return { loggedIn: false, detail: "没登录" };
  return engine === "codex"
    ? { loggedIn: true, plan: "ChatGPT", detail: "已登录（ChatGPT）" }
    : { loggedIn: true, plan: "max", email: "you@example.com", detail: "已登录（Claude 订阅）" };
}

let gatewayConfigState: GatewayConfig = { vertex: { projectId: "", location: "global" }, cliDir: "" };

/** QUOTA 页的演示数据：近 30 天，按天随机但固定（同一个种子），几家引擎都有。 */
function mockSpend(days: number): SpendOverview {
  const now = Date.now();
  const since = now - days * 86_400_000;
  let seed = 7;
  const rand = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  const points: SpendPoint[] = [];
  const keys: Array<[string, string, number, boolean]> = [
    ["peck", "deepseek/deepseek-flash", 0.06, false],
    ["peck", "anthropic/claude-haiku-5-5", 0.12, false],
    ["peck", "oneshot", 0.01, false],
    ["claude-code", "claude-code", 0.3, true],
    ["codex", "codex", 0.18, true],
    ["grok", "grok", 0.08, true],
  ];
  for (let d = days - 1; d >= 0; d--) {
    const day = now - d * 86_400_000;
    for (const [engine, key, scale, estimated] of keys) {
      if (rand() < 0.45) continue;
      const n = 1 + Math.floor(rand() * 4);
      for (let i = 0; i < n; i++) {
        const input = Math.round(2000 + rand() * 40000);
        points.push({ at: day - Math.floor(rand() * 8) * 3_600_000, engine, key, usd: +(scale * (0.2 + rand())).toFixed(4), estimated, input, output: Math.round(input / 12), cacheRead: Math.round(input * 0.6) });
      }
    }
  }
  const quota = ["claude-code", "codex", "muse"].flatMap((engine, k) => [0, 1, 2, 3, 4, 5, 6].flatMap((d) => [
    { at: now - (6 - d) * 86_400_000, engine, label: "5 小时", usedPercent: Math.round(10 + rand() * 70), resetsAt: now + (2 + k) * 3_600_000 },
    { at: now - (6 - d) * 86_400_000, engine, label: "每周", usedPercent: Math.min(98, 20 + d * (8 + k * 3)), resetsAt: now + (3 + k) * 86_400_000 },
  ]));
  return { since, points: points.filter((p) => p.at >= since), quota };
}
