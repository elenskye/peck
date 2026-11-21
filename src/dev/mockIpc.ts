// 浏览器 `?mock` 里替代 Rust IPC：数据来自 mockData.ts，只在内存，刷新重置。
import {
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
import { MOCK_SPILLS, PECK_CAPS, cliVerifyRounds, oneshotDemo, peckDemo, type DemoCheck, type Round } from "@/dev/peckMock";
import type { AgentEvent, CliAccountStatus, CliLoginState, Control, EngineAccounts, FileDiff, GatewayConfig, GatewaySettings, GatewayRunArgs, ModelEntry, TurnEndReason } from "@/lib/agentEvents";
import type { AllowRule, BgInfo, HarnessRunArgs, McpServerConfig, McpToolInfo, MemoryNote, PriceBook, PriceRefresh, PriceRow, ProjectMcpView, SkillDraft, SkillRow, SkillVersion, SlashList, VerifyResult, OneshotArgs, Schedule, ScheduleEvent, ScheduleFired, SpendOverview, SpendPoint, TeamRunArgs, VerifyConfig, VerifyState } from "@/lib/ipc";
import pricesRaw from "../../src-tauri/defaults/prices.json?raw";
import { cliRounds, delegateRound, fixRounds, reviewRounds, specRounds, stage, type Member } from "@/dev/teamMock";
import type { ApiConfig, ApiStatus, ApiTask } from "@/lib/api";
import type {
  DiffResult,
  FileListing,
  FilePreview,
  Message,
  Permission,
  ProjectOverview,
  RunRecord,
  Session,
  StepDescriptor,
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
let sessionsState: Session[] = emptyPreview ? [] : MOCK_SESSIONS.map((s) => ({ ...s }));
/** 左栏、ARCHIVE 里一个任务的样子。 */
function taskBrief(s: Session) {
  const runs = runsState[s.id] ?? [];
  const last = runs[runs.length - 1];
  return {
    id: s.id, workspaceId: s.workspaceId, title: s.title, createdAt: s.createdAt, updatedAt: s.updatedAt, runCount: runs.length, archivedAt: s.archivedAt ?? null,
    lastRun: last ? { id: last.id, number: last.number, flowId: last.flowId, flowName: last.flowName, status: last.status, startedAt: last.startedAt, finishedAt: last.finishedAt, files: [...last.files], who: last.who, undoneAt: last.undoneAt } : null,
    skillsWaiting: skillsWaitingOf(runs),
  };
}

/** 和后台一样数没批的 Skill 提议：没有「存了」「不要」的；「排着」只在任务还在跑时算批过。 */
function skillsWaitingOf(runs: RunRecord[]): number {
  let n = 0;
  for (const r of runs) {
    const log = agentEventsState[r.id] ?? [];
    for (const e of log) {
      if (e.type !== "skill.proposed") continue;
      const done = log.some((x) => x.type === "skill.reviewed" && x.proposalId === e.proposalId && (x.status === "saved" || x.status === "skipped" || (x.status === "queued" && r.status === "running")));
      if (!done) n += 1;
    }
  }
  return n;
}
const messagesState: Record<string, Message[]> = structuredClone(MOCK_MESSAGES);
const runsState: Record<string, RunRecord[]> = structuredClone(MOCK_RUNS);
// 键是 `${runId}:${path}`；新发的任务也按这个键存。
const diffsState: Record<string, DiffResult> = structuredClone(MOCK_DIFFS);
const apiTasksState: Record<string, ApiTask[]> = structuredClone(MOCK_API_TASKS);
const filesState: Record<string, Record<string, string>> = structuredClone(MOCK_FILES);
const apiStatusState: ApiStatus = structuredClone(MOCK_API_STATUS);
let settingsState: string | null = null;
let runCounter = 13;

const allRuns = () => Object.values(runsState).flat();
const findRun = (runId: string) => allRuns().find((r) => r.id === runId);
const workspaceOfSession = (sessionId: string) => sessionsState.find((s) => s.id === sessionId)?.workspaceId ?? "";
const workspaceIsBusy = (workspaceId: string) =>
  allRuns().some((r) => r.status === "running" && workspaceOfSession(r.sessionId) === workspaceId);



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
        .filter((s) => s.workspaceId === workspace.id && !s.archivedAt && (runsState[s.id]?.length ?? 0) > 0)
        .sort((a, b) => b.updatedAt - a.updatedAt)
        .map(taskBrief), layoutState.tasks[workspace.id], (t) => t.id),
    }));
  },
  async archiveList(): Promise<ProjectOverview[]> {
    const groups: ProjectOverview[] = [];
    for (const workspace of workspacesState) {
      const tasks = sessionsState
        .filter((s) => s.workspaceId === workspace.id && s.archivedAt && (runsState[s.id]?.length ?? 0) > 0)
        .map(taskBrief)
        .sort((a, b) => (b.archivedAt ?? 0) - (a.archivedAt ?? 0));
      if (tasks.length) groups.push({ workspace: { ...workspace }, missing: missingFolders.has(workspace.path), tasks });
    }
    return groups.sort((a, b) => (b.tasks[0].archivedAt ?? 0) - (a.tasks[0].archivedAt ?? 0));
  },
  async sessionArchive(id: string): Promise<void> {
    if ((runsState[id] ?? []).some((r) => r.status === "running")) throw new Error("这个任务在跑，先停下再归档。");
    const s = sessionsState.find((x) => x.id === id);
    if (!s) throw new Error("没找到这个任务。");
    s.archivedAt = Date.now();
  },
  async sessionUnarchive(id: string): Promise<Workspace> {
    const s = sessionsState.find((x) => x.id === id);
    if (!s) throw new Error("没找到这个任务。");
    s.archivedAt = null;
    const ws = workspacesState.find((w) => w.id === s.workspaceId);
    if (!ws) throw new Error("没找到这个任务的项目。");
    ws.hidden = false;
    return { ...ws };
  },
  async workspaceArchiveTasks(id: string): Promise<number> {
    let n = 0;
    for (const s of sessionsState) {
      const runs = runsState[s.id] ?? [];
      if (s.workspaceId !== id || s.archivedAt || !runs.length || runs.some((r) => r.status === "running")) continue;
      s.archivedAt = Date.now();
      n++;
    }
    return n;
  },

  async workspaceUpdate(id: string, patch: { name?: string; settings?: ProjectSettings }): Promise<Workspace> {
    const ws = workspacesState.find((w) => w.id === id);
    if (!ws) throw new Error("没找到这个项目。");
    if (patch.name != null) {
      if (!patch.name.trim()) throw new Error("名字不能空着。");
      ws.name = patch.name.trim();
    }
    if (patch.settings) {
      const folders: { path: string; writable: boolean }[] = [];
      for (const f of patch.settings.folders ?? []) {
        const path = f.path.trim().replace(/\/+$/, "");
        if (!path) continue;
        if (!path.startsWith("/") && !path.startsWith("~/")) throw new Error(`文件夹要写完整的路径：${path}`);
        if (path === ws.path || path.startsWith(`${ws.path}/`) || ws.path.startsWith(`${path}/`)) throw new Error(`${path} 和项目文件夹是套着的，不用另加。`);
        const known = folders.find((x) => x.path === path);
        if (known) known.writable = f.writable; else folders.push({ path, writable: f.writable });
      }
      const env: { name: string; value: string }[] = [];
      for (const v of patch.settings.env ?? []) {
        const name = v.name.trim();
        if (!name) continue;
        if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) throw new Error(`变量名只能用字母、数字、下划线，不以数字开头：${name}`);
        if (["HOME", "TMPDIR", "PWD", "SHELL", "CI", "PAGER", "TERM"].includes(name) || name.startsWith("PECK_")) throw new Error(`${name} 是 Peck 自己管的，项目里不能改。`);
        const at = env.findIndex((x) => x.name === name);
        if (at >= 0) env.splice(at, 1);
        env.push({ name, value: v.value });
      }
      ws.settings = { ...patch.settings, folders, env, verifyDismissed: ws.settings?.verifyDismissed };
    }
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

  async sessionGetRuns(sessionId: string): Promise<RunRecord[]> {
    return structuredClone(runsState[sessionId] ?? []);
  },

  async runCancel(runId: string): Promise<void> {
    const agent = agentRuns.get(runId);
    if (agent) { agent.stopped = true; agent.waiting?.({ type: "interrupt" }); return; }
    // 演示数据里本来就「在跑」的那个任务，没有计时器，直接改状态。
    const record = findRun(runId);
    if (record && record.status === "running") {
      record.status = "cancelled";
      record.finishedAt = Date.now();
      record.summary = "已停止：这一步没跑完。";
      record.steps = record.steps.map((s) => (s.status === "running" || s.status === "pending" ? { ...s, status: "cancelled" } : s));
    }
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
    if (/\.(png|jpg|jpeg|gif|webp|icns)$/i.test(path)) return { path, kind: "image", content: null, size: 18_400, deleted: false, dataUrl: demoPicture(path.split("/").pop() ?? path) };
    if (/\.pdf$/i.test(path)) return { path, kind: "pdf", content: null, size: 412, deleted: false, dataUrl: `data:application/pdf;base64,${DEMO_PDF}` };
    if (/\.(docx?|xlsx?|pptx?|key|pages|numbers|rtf|od[tsp])$/i.test(path)) return { path, kind: "office", content: null, size: 24_600, deleted: false };
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

  async filesQuicklook(_workspaceId: string, path: string): Promise<string> {
    await new Promise((r) => setTimeout(r, 300));
    return demoPicture(`Quick Look · ${path.split("/").pop() ?? path}`, true);
  },
  async filesQuicklookOpen(_workspaceId: string, _path: string): Promise<void> {
    console.info("[演示] 原生 App 里这里会弹出系统的 Quick Look 窗口。");
  },
  async changesHunkRevert(runId: string, _stepId: string | undefined, filePath: string, hunk: number): Promise<void> {
    const record = findRun(runId);
    if (!record) throw new Error("没找到这个任务。");
    if (record.status === "running") throw new Error("任务还在跑，跑完再拒绝。");
    const diff = diffsState[`${runId}:${filePath}`];
    if (!diff) throw new Error("这个文件不是这个任务改的。");
    const workspaceId = workspaceOfSession(record.sessionId);
    const files = (filesState[workspaceId] ??= {});
    const blocks = diff.unifiedDiff.split(/\n(?=@@)/).filter((b) => b.startsWith("@@"));
    const block = blocks[hunk];
    if (!block) throw new Error("没有这一块。");
    const lines = block.split("\n").slice(1);
    const after = lines.filter((l) => l.startsWith(" ") || l.startsWith("+")).map((l) => l.slice(1)).join("\n");
    const before = lines.filter((l) => l.startsWith(" ") || l.startsWith("-")).map((l) => l.slice(1)).join("\n");
    const current = files[filePath];
    if (current != null && after && current.includes(after)) files[filePath] = current.replace(after, before);
    else if (current != null && after) throw new Error("这一块后来又改过了，和任务做完时对不上，没动。");
  },
  async worktreeApply(runId: string, force: boolean): Promise<{ applied: string[]; conflicts: string[]; record: RunRecord }> {
    const record = findRun(runId);
    const wt = record?.worktree;
    if (!record || !wt) throw new Error("这个任务不在独立工作区里。");
    if (wt.appliedAt || wt.discardedAt) throw new Error(wt.appliedAt ? "已经搬回去了。" : "已经丢掉了。");
    const pending = worktreeState[runId] ?? { files: {}, conflicts: [] };
    if (pending.conflicts.length && !force) return { applied: [], conflicts: pending.conflicts, record: structuredClone(record) };
    const files = (filesState[workspaceOfSession(record.sessionId)] ??= {});
    Object.assign(files, pending.files);
    record.worktree = { ...wt, appliedAt: Date.now() };
    delete worktreeState[runId];
    return { applied: Object.keys(pending.files), conflicts: [], record: structuredClone(record) };
  },
  async worktreeDiscard(runId: string): Promise<RunRecord> {
    const record = findRun(runId);
    const wt = record?.worktree;
    if (!record || !wt) throw new Error("这个任务不在独立工作区里。");
    record.worktree = { ...wt, discardedAt: Date.now() };
    delete worktreeState[runId];
    return structuredClone(record);
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

  // ── Model Gateway（v0.5 契约）──
  async gatewayModels(): Promise<ModelEntry[]> {
    return structuredClone(MOCK_MODEL_ENTRIES);
  },
  async pricesGet(): Promise<PriceBook> {
    return mockPriceBook();
  },
  async pricesSet(args: { row?: PriceRow; reset?: string; fxRate?: number }): Promise<PriceBook> {
    if (args.row) {
      const r = args.row;
      if (!r.model.trim()) throw new Error("模型名不能空着。");
      if (![r.input, r.output].every((v) => Number.isFinite(v) && v >= 0)) throw new Error("价格要是 0 到 10000 之间的数。");
      priceEdits = [...priceEdits.filter((x) => x.model !== r.model), { ...r, label: r.label.trim() || r.model, vendor: r.vendor || "other" }];
    }
    if (args.reset) priceEdits = priceEdits.filter((x) => x.model !== args.reset);
    if (args.fxRate != null) {
      if (!(args.fxRate > 0.1 && args.fxRate < 100)) throw new Error("汇率要在 0.1 到 100 之间。");
      priceFx = args.fxRate;
    }
    return mockPriceBook();
  },
  async pricesRefresh(): Promise<PriceRefresh> {
    priceRefreshedAt = Date.now();
    const rows = mockPriceBook().rows;
    return { book: mockPriceBook(), report: { changed: [], same: rows.filter((r) => r.currency === "USD").length - 2, missing: ["Orchestrator G1 Pro", "Grok 4.7 Fast"], skippedCny: rows.filter((r) => r.currency === "CNY").length } };
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
    const { runId, record, workspaceId } = createAgentRecord(args.sessionId, args.prompt, args.who, args.permission ?? "edit", `cli-${engine}`, label);
    if (args.worktree) {
      record.worktree = { path: `/mock/worktrees/${workspaceId}/${runId}`, appliedAt: null, discardedAt: null };
      worktreeState[runId] = { files: {}, conflicts: [] };
    }
    // CLI 也走后台验证（v0.10 C1）：确认过 verify.toml、不是 PLAN、项目没关、不是只做直接调用的（那种只跑一遍）。
    const entry = MOCK_MODEL_ENTRIES.find((e) => e.id === args.who);
    const verify = mockVerify(workspaceId);
    const checks: DemoCheck[] = verify.approved ? verify.config?.check.map((c) => ({ name: c.name, run: c.run })) ?? [] : [];
    const off = workspacesState.find((w) => w.id === workspaceId)?.settings?.cliVerifyOff && !args.goal?.length;
    if (args.goal?.length && entry?.directOnly) throw new Error(`${label} 只做直接调用，不开目标模式。`);
    const after = (args.permission ?? "edit") !== "plan" && !off ? cliVerifyRounds(checks, args.prompt, label, args.goal?.[0] ?? null) : [];
    // 引擎自己的收尾挪到验证之后（和真的一样：验证跑完才收尾）。
    const body = after.length ? replay.filter((e) => e.type !== "turn.completed") : replay;
    void replayAgent(runId, [body, ...(entry?.directOnly ? after.slice(0, 1) : after)], forkNotice(args.sessionId, runId, `cli:${engine}`, label), onEvent, { costCap: null, countSpend: false }).then((end) => finishAgentRecord(record, end, label));
    return runId;
  },
  // ── 合作（v0.8）：接力（写规范 → 实施）、交叉审查（要改就修一次）。一个任务、一条事件流 ──
  async teamRunStart(args: TeamRunArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (!args.prompt.trim()) throw new Error("先写一句话。");
    if (!args.planBy && !args.reviewBy) throw new Error("接力和审查至少选一样。");
    if ((args.permission ?? "edit") === "plan") throw new Error("PLAN 只读：接力和审查要动手，选 EDIT 或 AUTO。");
    const member = (who: string): Member => {
      const e = MOCK_MODEL_ENTRIES.find((x) => x.id === who);
      if (e?.directOnly) throw new Error("Antigravity 只做直接调用，不进接力和审查。");
      return { who, label: who.startsWith("api:") ? `Peck · ${e?.label ?? who.slice(4)}` : e?.label ?? who.slice(4) };
    };
    const builder = member(args.who);
    const writer = args.planBy ? member(args.planBy.who) : null;
    const reviewer = args.reviewBy ? member(args.reviewBy.who) : null;
    const title = [writer && `${writer.label} 写规范`, `${builder.label} 实施`, reviewer && `${reviewer.label} 审查`].filter(Boolean).join(" → ");
    const who = `team:${args.planBy?.who ?? ""}|${args.who}|${args.reviewBy?.who ?? ""}`;
    const { runId, record, workspaceId } = createAgentRecord(args.sessionId, args.prompt, who, args.permission ?? "edit", "team", title);
    if (args.worktree) record.worktree = { path: `/mock/worktrees/${workspaceId}/${runId}`, appliedAt: null, discardedAt: null };
    const files = (filesState[workspaceId] ??= {});
    const demo = peckDemo(runId, files, args.prompt, { window: 1_000_000, checks: [], project: false });
    const editPath = demo.edit.path;
    const sub = args.delegate ? MOCK_MODEL_ENTRIES.find((e) => e.id === args.delegate) : undefined;
    // 实施的是 Peck 循环：主线剧本（委派的话插一段子任务）；是 CLI：回放它录下来的过程。
    const buildRounds: Round[] = builder.who.startsWith("api:")
      ? [...demo.rounds.slice(0, 1), ...(sub ? [delegateRound(runId, { who: sub.id, label: sub.label }, editPath)] : []), ...demo.rounds.slice(1)]
      : cliRounds(builder);
    const pass = /通过/.test(args.prompt);
    const rounds: Round[] = [
      ...(writer ? specRounds(runId, writer, builder, editPath) : []),
      ...stage("implement", builder, buildRounds),
      ...(reviewer ? reviewRounds(runId, reviewer, editPath, pass, !!writer) : []),
      ...(reviewer && !pass ? fixRounds(runId, builder, editPath) : []),
    ];
    const header: ReplayEvent[] = [...forkNotice(args.sessionId, runId, "team", title), { type: "turn.started", turnId: `turn-${runId}` }];
    void replayAgent(runId, rounds, header, onEvent, { costCap: args.costCap ?? null, countSpend: true }).then((end) => {
      if (end.diffs.some((d) => d.path === editPath)) {
        const after = reviewer && !pass ? demo.edit.after.replace("// 待办", "// 已改") : demo.edit.after;
        if (args.worktree) worktreeState[runId] = { files: { [editPath]: after }, conflicts: [] };
        else files[editPath] = after;
        undoState[runId] = { workspaceId, restore: { [editPath]: demo.edit.before }, remove: [] };
      }
      finishAgentRecord(record, end, title);
    });
    return runId;
  },
  // ── 定时任务（v0.8）：只在内存；RUN NOW 照常发一个任务（演示里不会到点自己跑）──
  async scheduleList(workspaceId?: string): Promise<Schedule[]> {
    return structuredClone(schedulesState.filter((x) => !workspaceId || x.workspaceId === workspaceId));
  },
  async scheduleSave(schedule: Schedule): Promise<Schedule> {
    if (!schedule.prompt.trim()) throw new Error("定时任务要发的那句话还空着。");
    if (schedule.who === "cli:antigravity") throw new Error("Antigravity 只做直接调用，不进定时任务。");
    if (schedule.when.kind === "daily" && !/^([01]?\d|2[0-3]):[0-5]\d$/.test(schedule.when.time)) throw new Error(`时间写成 09:30 这样：${schedule.when.time}`);
    if (schedule.when.kind === "interval" && !(schedule.when.hours >= 1 && schedule.when.hours <= 168)) throw new Error("每隔几小时：1 到 168。");
    const next: Schedule = { ...schedule, title: schedule.title.trim() || schedule.prompt.trim().slice(0, 30), id: schedule.id || `sched-${Date.now()}`, createdAt: schedule.createdAt ?? Date.now() };
    next.nextAt = next.enabled ? mockNextAt(next) : null;
    schedulesState = schedulesState.some((x) => x.id === next.id) ? schedulesState.map((x) => (x.id === next.id ? next : x)) : [...schedulesState, next];
    return structuredClone(next);
  },
  async scheduleDelete(id: string): Promise<void> {
    schedulesState = schedulesState.filter((x) => x.id !== id);
  },
  async scheduleRunNow(id: string): Promise<ScheduleFired> {
    const s = schedulesState.find((x) => x.id === id);
    if (!s) throw new Error("没找到这个定时任务。");
    if (workspaceIsBusy(s.workspaceId)) throw new Error("这个项目有任务在跑，跑完再发。");
    const session = await this.sessionCreate(s.workspaceId, `⏰ ${s.title}`);
    let runId = "";
    const early: AgentEvent[] = [];
    const onEvent = (event: AgentEvent) => { if (runId) scheduleListeners.event?.({ runId, workspaceId: s.workspaceId, sessionId: session.id, prompt: s.prompt, event }); else early.push(event); };
    runId = s.who.startsWith("api:")
      ? await this.harnessRunStart({ sessionId: session.id, prompt: s.prompt, who: s.who, permission: s.permission, effort: s.effort ?? undefined }, onEvent)
      : await this.gatewayRunStart({ sessionId: session.id, prompt: s.prompt, who: s.who, permission: s.permission, model: s.model ?? undefined, effort: s.effort ?? undefined }, onEvent);
    for (const event of early) scheduleListeners.event?.({ runId, workspaceId: s.workspaceId, sessionId: session.id, prompt: s.prompt, event });
    const fired: ScheduleFired = { scheduleId: s.id, workspaceId: s.workspaceId, sessionId: session.id, runId, prompt: s.prompt };
    s.lastRunAt = Date.now();
    s.lastSession = session.id;
    s.lastNote = "手动发了一次";
    scheduleListeners.fired?.(fired);
    return fired;
  },
  async onSchedule(onFired: (f: ScheduleFired) => void, onEvent: (e: ScheduleEvent) => void): Promise<() => void> {
    scheduleListeners.fired = onFired;
    scheduleListeners.event = onEvent;
    // 只摘自己挂的（React 开发模式会挂两次、摘一次）。
    return () => { if (scheduleListeners.fired === onFired) { scheduleListeners.fired = null; scheduleListeners.event = null; } };
  },
  async previewStart(url: string): Promise<string> {
    if (!/^http:\/\/(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0)(:\d+)?(\/|$)/.test(url.trim())) throw new Error("预览只开本机的地址（localhost、127.0.0.1），不代理外面的网站。");
    return "about:blank#mock-preview";
  },
  async harnessImageRead(path: string): Promise<string> {
    if (!path.startsWith("/mock/")) throw new Error("只能打开 Peck 存下来的图。");
    return demoPicture(`截图 · ${path.split("/").pop() ?? path}`, true);
  },
  async openPath(_workspaceId: string, path: string, _how: "editor" | "finder", _app?: string): Promise<void> {
    if (path.includes("..")) throw new Error("只能打开项目里的文件。");
  },
  // ── 外部 MCP 服务器（v0.8）：只在内存；TEST 给一份演示的工具 ──
  async mcpServersGet(): Promise<McpServerConfig[]> {
    return structuredClone(mcpState);
  },
  async mcpServersSet(servers: McpServerConfig[]): Promise<McpServerConfig[]> {
    const seen = new Set<string>();
    const next = servers.map((x) => ({ ...x, name: x.name.trim().toLowerCase(), command: x.command.trim() }));
    for (const x of next) {
      if (!/^[a-z0-9_-]{1,24}$/.test(x.name)) throw new Error(`MCP 服务器的名字只能用小写字母、数字、- 和 _，最长 24：${x.name}`);
      if (seen.has(x.name)) throw new Error(`有两个叫 ${x.name} 的 MCP 服务器。`);
      if (!x.command) throw new Error(`${x.name} 还没写命令。`);
      seen.add(x.name);
    }
    mcpState = next;
    return structuredClone(mcpState);
  },
  async mcpServerTest(server: McpServerConfig): Promise<McpToolInfo[]> {
    await new Promise((r) => setTimeout(r, 500));
    if (/missing|nope/.test(server.command)) throw new Error(`MCP 服务器 ${server.name} 起不来：No such file or directory`);
    return [
      { server: server.name, name: "search_issues", description: "Search issues", readOnly: true, destructive: false },
      { server: server.name, name: "create_issue", description: "Create an issue", readOnly: false, destructive: false },
      { server: server.name, name: "delete_issue", description: "Delete an issue", readOnly: false, destructive: true },
    ];
  },
  // ── 斜杠命令和 Skills（v0.10 C2）：演示几个命令文件和 Skill ──
  async slashList(workspaceId: string): Promise<SlashList> {
    const off = workspacesState.find((w) => w.id === workspaceId)?.settings?.skillsOff ?? [];
    return {
      commands: [
        { name: "fix-todo", description: "把 TODO 注释都处理掉", hint: "[目录]", scope: "project", folder: null },
        { name: "changelog", description: "按最近的提交写 CHANGELOG", hint: null, scope: "user", folder: null },
      ],
      skills: [
        { name: "release", description: "How to cut a release of this project", scope: "project", off: off.includes("release") },
        { name: "draw-board", description: "Draw a board with the dw MCP tools", scope: "project-skills", off: off.includes("draw-board") },
        { name: "pdf", description: "Read and fill PDF forms", scope: "user", off: off.includes("pdf") },
      ],
    };
  },
  async slashExpand(_workspaceId: string, name: string, args: string): Promise<string> {
    if (name === "fix-todo") return `把${args ? ` ${args} 里` : "项目里"}的 TODO 注释都处理掉：能做的做了，做不了的改成一句说明为什么。`;
    if (name === "changelog") return `按最近的提交写一段 CHANGELOG，放在 CHANGELOG.md 最上面。${args ? `\n\n${args}` : ""}`;
    throw new Error(`没有 /${name} 这个命令。`);
  },
  async harnessCompact(_sessionId: string): Promise<string> {
    await new Promise((r) => setTimeout(r, 200));
    return "压好了：2 份长输出落了盘，5 份旧的工具输出换成了一句话；对话从约 38.2k 降到约 9.6k token。";
  },
  async verifyRunNow(workspaceId: string): Promise<VerifyResult[]> {
    const v = mockVerify(workspaceId);
    if (!v.approved || !v.config?.check.length) throw new Error("还没有确认过的验证命令：在项目设置里写 .peck/verify.toml、SAVE 确认。");
    await new Promise((r) => setTimeout(r, 900));
    return v.config.check.map((c, i) => ({ name: c.name, command: c.run, passed: i !== 1, exitCode: i !== 1 ? 0 : 1, timedOut: false, durationMs: 2400 + i * 900, output: i !== 1 ? "" : "FAIL src/lib/utils.test.ts > versionText\n  expected '0.167.2' to be '0.167.3'" }));
  },
  async mcpSecretSet(server: string, name: string, value: string | null): Promise<McpServerConfig[]> {
    const s = mcpState.find((x) => x.name === server);
    if (!s) throw new Error(`没有叫 ${server} 的 MCP 服务器。`);
    const names = new Set(s.secretEnv ?? []);
    if (value && value.trim()) names.add(name); else names.delete(name);
    s.secretEnv = [...names].sort();
    s.env = s.env.filter((v) => !names.has(v.name));
    return structuredClone(mcpState);
  },
  async projectMcpList(workspaceId: string): Promise<ProjectMcpView> {
    const ws = workspacesState.find((w) => w.id === workspaceId);
    const off = ws?.settings?.mcpOff ?? [];
    // 演示：edge-gateway 的项目里有一份 .mcp.json（dw）；别的项目只有设置里的。
    const project = workspaceId === workspacesState[0]?.id
      ? [{ name: "dw", command: "bench/bin/dw-mcp.sh", args: [], source: "project" as const, on: !off.includes("dw"), offInSettings: false, overridden: false, allowed: mockAllow.some((r) => r.workspaceId === workspaceId && r.rule.startsWith("mcp-project:dw:")), tools: mockHints.dw ?? [] }]
      : [];
    const rows = [
      ...project,
      ...mcpState.map((g) => ({ name: g.name, command: g.command, args: g.args, source: "settings" as const, on: g.enabled && !off.includes(g.name) && !project.some((p) => p.name === g.name), offInSettings: !g.enabled, overridden: project.some((p) => p.name === g.name), allowed: false, tools: mockHints[g.name] ?? [] })),
    ];
    return { rows, skipped: project.length ? [["figma", "Peck 循环只接 stdio 的 MCP 服务器（这个是网址那种，CLI 自己读）"]] : [], error: null };
  },
  async projectMcpTest(_workspaceId: string, name: string): Promise<McpToolInfo[]> {
    await new Promise((r) => setTimeout(r, 400));
    const tools: McpToolInfo[] = name === "dw"
      ? [
        { server: "dw", name: "list", description: "List canvases", readOnly: true, destructive: false },
        { server: "dw", name: "write_board", description: "Write a board", readOnly: false, destructive: false },
        { server: "dw", name: "delete_board", description: "Delete a board", readOnly: false, destructive: true },
      ]
      : await this.mcpServerTest(mcpState.find((x) => x.name === name) ?? { name, command: name, args: [], env: [], enabled: true });
    mockHints[name] = tools.map((t) => [t.name, t.readOnly ? "read" : t.destructive ? "destructive" : "write"]);
    return tools;
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
    if (args.worktree) record.worktree = { path: `/mock/worktrees/${workspaceId}/${runId}`, appliedAt: null, discardedAt: null };
    const files = (filesState[workspaceId] ??= {});
    const verify = mockVerify(workspaceId);
    const checks: DemoCheck[] = verify.config?.check.map((c) => ({ name: c.name, run: c.run })) ?? [];
    const project = (memoryState[workspaceId]?.length ?? 0) > 0 || "AGENTS.md" in files || "CLAUDE.md" in files;
    const demo = peckDemo(runId, files, args.prompt, { window: entry?.contextWindow ?? 1_000_000, checks, goal: args.goal?.[0] ?? null, project });
    // 委派（v0.8）：主模型先把一个查找的子任务交给便宜的模型。
    const sub = args.delegate ? MOCK_MODEL_ENTRIES.find((e) => e.id === args.delegate) : undefined;
    if (sub) demo.rounds.splice(1, 0, delegateRound(runId, { who: sub.id, label: sub.label }, demo.edit.path));
    // 「截图」两个字在这句话里：先截一下 App 的窗口（演示图）。
    if (/截图/.test(args.prompt)) demo.rounds.splice(1, 0, screenshotRound(runId));
    const header: ReplayEvent[] = [
      ...forkNotice(args.sessionId, runId, "peck", label),
      ...(args.images?.length ? [{ type: "notice", level: "info", text: `这一句带了 ${args.images.length} 张图。` } as ReplayEvent] : []),
      { type: "session.started", engine: "peck", model, caps: PECK_CAPS, tools: ["list_directory", "read_file", "search_text", "find_files", "write_file", "write_patch", "bash_run", "update_plan", "ask_user", ...(sub ? ["delegate_task"] : [])], mcp: mcpState.filter((m) => m.enabled).map((m) => ({ name: m.name, status: "connected" })), sessionRef: `peck-${args.sessionId}`, version: null },
      { type: "turn.started", turnId: `turn-${runId}` },
    ];
    void replayAgent(runId, demo.rounds, header, onEvent, { costCap: args.costCap ?? null, countSpend: true }).then((end) => {
      if (end.diffs.some((d) => d.path === demo.edit.path)) {
        if (args.worktree) {
          // 独立工作区：主文件夹不动，等 APPLY 再搬（演示：「冲突」两个字在这句话里就报一个冲突）。
          worktreeState[runId] = { files: { [demo.edit.path]: demo.edit.after }, conflicts: /冲突/.test(args.prompt) ? [demo.edit.path] : [] };
        } else {
          files[demo.edit.path] = demo.edit.after;
        }
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

  // ── 自己写 Skill（v0.10 C3）──
  async skillReview(runId: string, proposalId: string, decision: "save" | "skip", draft: SkillDraft | null): Promise<AgentEvent> {
    const log = agentEventsState[runId];
    if (!log) throw new Error("没找到这个任务。");
    const proposed = log.find((e): e is Extract<AgentEvent, { type: "skill.proposed" }> => e.type === "skill.proposed" && e.proposalId === proposalId);
    if (!proposed) throw new Error("没找到这条 Skill 提议。");
    const last = [...log].reverse().find((e) => e.type === "skill.reviewed" && e.proposalId === proposalId);
    if (last && last.type === "skill.reviewed" && last.status !== "failed") throw new Error("这条已经批过了。");
    const d: SkillDraft = draft ?? { name: proposed.name, description: proposed.description, body: proposed.body, scope: proposed.scope };
    if (decision === "save" && !/^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/.test(d.name)) throw new Error(`Skill 的名字「${d.name}」不行：只能用小写字母、数字和 -，最长 64 个，比如 release-checklist。`);
    if (decision === "save" && /sk-[A-Za-z0-9_-]{16,}/.test(d.body)) throw new Error("第 1 行像 Key、令牌或密码，这种内容存不进 Skill。去掉（写成「从钥匙串、环境变量读」）再提。");
    // 替换已有的（同名）在原处；新的照卡片上选的范围放。
    const location = d.name === proposed.name && proposed.diff ? proposed.location : d.scope === "user" ? `~/.claude/skills/${d.name}/SKILL.md` : `.claude/skills/${d.name}/SKILL.md`;
    const live = agentRuns.get(runId);
    const project = !location.startsWith("~/");
    const status = decision === "skip" ? "skipped" : live?.emit && project ? "queued" : "saved";
    const body = { type: "skill.reviewed", proposalId, status, name: d.name, location, error: null } as ReplayEvent;
    const sessionId = Object.keys(runsState).find((sid) => runsState[sid].some((r) => r.id === runId));
    const workspaceId = sessionId ? workspaceOfSession(sessionId) : "";
    const keep = () => {
      const rows = mockSkillRows(workspaceId);
      const old = rows.find((r) => r.name === d.name);
      if (old) { old.description = d.description; old.versions += 1; old.changedAt = Date.now(); }
      else rows.push({ name: d.name, description: d.description, scope: d.scope, off: false, loads: 0, lastUsed: null, changedAt: Date.now(), stale: false, versions: 0 });
    };
    if (live?.emit) {
      const event = live.emit(body);
      if (status === "queued") live.atEnd.push(() => { keep(); live.emit?.({ ...(body as object), status: "saved" } as ReplayEvent); });
      else if (status === "saved") keep();
      return structuredClone(event);
    }
    if (status === "saved") keep();
    const event = { ...(body as object), seq: log.length, at: Date.now() } as AgentEvent;
    log.push(event);
    return structuredClone(event);
  },
  async skillOverview(workspaceId: string): Promise<SkillRow[]> {
    return structuredClone(mockSkillRows(workspaceId));
  },
  async skillVersions(_workspaceId: string, name: string): Promise<SkillVersion[]> {
    const day = 86_400_000;
    if (name === "release") return [{ id: String(Date.now() - 3 * day), at: Date.now() - 3 * day, description: "How to cut a release", lines: 6 }, { id: String(Date.now() - 20 * day), at: Date.now() - 20 * day, description: "Release steps", lines: 4 }];
    const row = Object.values(skillRowsState).flat().find((r) => r.name === name);
    return row && row.versions > 0 ? [{ id: String(Date.now() - 60_000), at: Date.now() - 60_000, description: row.description, lines: 5 }] : [];
  },
  async skillRestore(workspaceId: string, name: string, _id: string): Promise<SkillRow[]> {
    const rows = mockSkillRows(workspaceId);
    const row = rows.find((r) => r.name === name);
    if (!row) throw new Error(`没有叫 ${name} 的 Skill 了。`);
    row.versions += 1;
    row.changedAt = Date.now();
    return structuredClone(rows);
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
  async backgroundStop(id: string): Promise<void> {
    for (const m of Object.values(serverState)) for (const b of Object.values(m)) if (b.id === id && b.status === "running") { b.status = "stopped"; b.endedAt = Date.now(); }
  },
  async serverStart(workspaceId: string, name: string): Promise<BgInfo> {
    const server = mockVerify(workspaceId).config?.server.find((x) => x.name === name);
    if (!server) throw new Error(`verify.toml 里没有叫「${name}」的 server。`);
    const running = serverState[workspaceId]?.[name];
    if (running?.status === "running") return structuredClone(running);
    const info: BgInfo = { id: `srv-${name}-${++memoryCounter}`, name, command: server.run, kind: "server", status: "running", exitCode: null, ready: false, startedAt: Date.now(), endedAt: null, owner: null, log: `/mock/harness-output/servers/${name}.log`, url: "http://localhost:5173/" };
    (serverState[workspaceId] ??= {})[name] = info;
    window.setTimeout(() => { info.ready = true; }, 1500);
    return structuredClone(info);
  },
};

// ── 预览、独立工作区（v0.7，演示）──
const DEMO_PDF = "JVBERi0xLjQKMSAwIG9iajw8L1R5cGUvQ2F0YWxvZy9QYWdlcyAyIDAgUj4+ZW5kb2JqCjIgMCBvYmo8PC9UeXBlL1BhZ2VzL0tpZHNbMyAwIFJdL0NvdW50IDE+PmVuZG9iagozIDAgb2JqPDwvVHlwZS9QYWdlL1BhcmVudCAyIDAgUi9NZWRpYUJveFswIDAgMzAwIDE2MF0vQ29udGVudHMgNCAwIFIvUmVzb3VyY2VzPDwvRm9udDw8L0YxIDUgMCBSPj4+Pj4+ZW5kb2JqCjQgMCBvYmo8PC9MZW5ndGggNTI+PnN0cmVhbQpCVCAvRjEgMTggVGYgMzAgOTAgVGQgKFBlY2sgZGVtbyBQREYpIFRqIEVUCmVuZHN0cmVhbSBlbmRvYmoKNSAwIG9iajw8L1R5cGUvRm9udC9TdWJ0eXBlL1R5cGUxL0Jhc2VGb250L0hlbHZldGljYT4+ZW5kb2JqCnRyYWlsZXI8PC9Sb290IDEgMCBSPj4KJSVFT0Y=";
/** 独立工作区里做完、还没搬回的改动（任务号 → 文件）。 */
const worktreeState: Record<string, { files: Record<string, string>; conflicts: string[] }> = {};

/** 演示用的图（SVG 的 data URL）：深海底、青色的像素鸟格子。 */
function demoPicture(label: string, page = false): string {
  const w = page ? 600 : 480;
  const h = page ? 780 : 300;
  const dots = Array.from({ length: 48 }, (_, i) => `<rect x="${40 + (i % 12) * 14}" y="${60 + Math.floor(i / 12) * 14}" width="10" height="10" fill="${(i * 7) % 5 ? "#0B3B4F" : "#00E5FF"}"/>`).join("");
  const lines = page ? Array.from({ length: 18 }, (_, i) => `<rect x="40" y="${150 + i * 30}" width="${520 - (i % 4) * 60}" height="10" fill="#C9D4E5"/>`).join("") : "";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="${page ? "#FFFFFF" : "#06121F"}"/>${page ? "" : dots}<text x="40" y="${page ? 100 : 250}" font-family="sans-serif" font-size="20" fill="${page ? "#06121F" : "#9FB3C8"}">${label.replace(/[<&]/g, "")}</text>${lines}</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

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
/** 项目设置 SKILLS 的演示数据（v0.10 C3）：slashList 那几个加上批进来的。 */
const skillRowsState: Record<string, SkillRow[]> = {};
function mockSkillRows(workspaceId: string): SkillRow[] {
  const off = workspacesState.find((w) => w.id === workspaceId)?.settings?.skillsOff ?? [];
  const day = 86_400_000;
  const rows = (skillRowsState[workspaceId] ??= [
    { name: "release", description: "How to cut a release of this project", scope: "project", off: false, loads: 7, lastUsed: Date.now() - 2 * day, changedAt: Date.now() - 3 * day, stale: false, versions: 2 },
    { name: "draw-board", description: "Draw a board with the dw MCP tools", scope: "project-skills", off: false, loads: 0, lastUsed: null, changedAt: Date.now() - 45 * day, stale: true, versions: 0 },
    { name: "pdf", description: "Read and fill PDF forms", scope: "user", off: false, loads: 3, lastUsed: Date.now() - 40 * day, changedAt: Date.now() - 90 * day, stale: true, versions: 0 },
  ]);
  for (const r of rows) r.off = off.includes(r.name);
  return rows;
}

const agentRuns = new Map<string, { waiting: null | ((c: Control) => void); stopped: boolean; emit?: (body: ReplayEvent) => AgentEvent; atEnd: Array<() => void> }>();
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

let schedulesState: Schedule[] = [
  { id: "sched-demo", workspaceId: "ws-peck", title: "每天看一眼 TODO", prompt: "列出项目里还没做完的 TODO 和 FIXME，按文件分组，别改文件", who: "api:deepseek/deepseek-flash", permission: "plan", when: { kind: "daily", time: "09:30", weekdays: true }, offsetMin: 480, enabled: true, nextAt: Date.now() + 5 * 3_600_000, lastRunAt: Date.now() - 19 * 3_600_000, lastSession: null, lastNote: "发出去了", createdAt: Date.now() - 7 * 86_400_000 },
];
const scheduleListeners: { fired: ((f: ScheduleFired) => void) | null; event: ((e: ScheduleEvent) => void) | null } = { fired: null, event: null };

/** 演示里的下一次：和 Rust 的 next_after 一样的算法。 */
function mockNextAt(s: Schedule): number {
  const now = Date.now();
  if (s.when.kind === "interval") return now + s.when.hours * 3_600_000;
  const [h, m] = s.when.time.split(":").map(Number);
  const offset = s.offsetMin * 60_000;
  const day = 86_400_000;
  let at = Math.floor((now + offset) / day) * day + (h * 60 + m) * 60_000;
  if (at <= now + offset) at += day;
  if (s.when.weekdays) while (((Math.floor(at / day) + 3) % 7 + 7) % 7 >= 5) at += day;
  return at - offset;
}

/** 演示：记下的 MCP 工具标注（项目设置里 TEST 过的）。 */
const mockHints: Record<string, [string, "read" | "write" | "destructive"][]> = {};

let mcpState: McpServerConfig[] = [
  { name: "github", command: "npx", args: ["-y", "@modelcontextprotocol/server-github"], env: [], enabled: true },
];

/** 截 App 窗口的一步（演示）：输出里写截图存在哪，界面按路径显示。 */
function screenshotRound(runId: string): ReplayEvent[] {
  const id = `${runId}-shot`;
  const path = `/mock/harness-output/${runId}/1-shot.png`;
  return [
    { type: "tool_call.started", callId: id, name: "screenshot_app", kind: "read", title: "截 Peck Dev 的窗口", input: { app: "Peck Dev" } },
    { type: "tool_call.completed", callId: id, status: "ok", output: `截了「Peck Dev · Peck」的窗口（182 KB），图在下面。截图存在 ${path}。`, exitCode: null, diff: [], durationMs: 900 },
    { type: "message.completed", itemId: `${id}-m`, text: "截图里标题还是英文，左上角那一行要改。" },
  ];
}

/** 和 Rust 的 history::family_of 一样：Peck 循环（不管哪个模型）、某个 CLI、合作。 */
const familyOf = (who?: string | null) => (!who ? "old" : who.startsWith("api:") ? "peck" : who.startsWith("team:") ? "team" : who.startsWith("cli:") ? who : `old:${who}`);

/** 换了引擎接着发（分叉）：这一家上一次以后别家做过几次，写一行。 */
function forkNotice(sessionId: string, runId: string, family: string, to: string): ReplayEvent[] {
  const runs = (runsState[sessionId] ?? []).filter((r) => r.id !== runId);
  let last = -1;
  runs.forEach((r, i) => { if (familyOf(r.who) === family) last = i; });
  const since = runs.slice(last + 1);
  if (since.length === 0) return [];
  const from = [...new Set(since.map((r) => r.flowName))];
  return [{ type: "notice", level: "info", text: `换成 ${to} 接着做：前面 ${since.length} 次（${from.join("、")}）的经过带过去了；原来的会话留着，换回去照样接得上。` }];
}

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
  const run: { waiting: null | ((c: Control) => void); stopped: boolean; emit?: (body: ReplayEvent) => AgentEvent; atEnd: Array<() => void> } = { waiting: null, stopped: false, atEnd: [] };
  agentRuns.set(runId, run);
  const log: AgentEvent[] = [];
  agentEventsState[runId] = log;
  let seq = 0;
  let ended = false;
  const send = (body: ReplayEvent): AgentEvent => {
    const event = { ...(body as object), seq: seq++, at: Date.now() } as AgentEvent;
    if (event.type === "turn.completed") ended = true;
    log.push(event);
    onEvent(event);
    return event;
  };
  // 跑着的时候批 Skill（v0.10 C3）：事件从这里发，序号接着。
  run.emit = send;
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
  // 收尾：跑着时批的 Skill 这时写进去（真的 Peck 是拍完收尾快照以后）。
  for (const f of run.atEnd) f();
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
// 价格表：内置的就是 Rust 那份 defaults/prices.json；改的只在内存。
const PRICES_BUILTIN = JSON.parse(pricesRaw) as { date: string; vendors: PriceBook["vendors"]; rows: PriceRow[] };
let priceEdits: PriceRow[] = [];
let priceFx = 6.7;
let priceRefreshedAt: number | null = null;
function mockPriceBook(): PriceBook {
  const rows: PriceBook["rows"] = PRICES_BUILTIN.rows.map((r) => ({ ...r, origin: "builtin" as const }));
  for (const e of priceEdits) {
    const i = rows.findIndex((r) => r.model === e.model);
    if (i >= 0) rows[i] = { ...e, origin: "edited" };
    else rows.push({ ...e, origin: "custom" });
  }
  return { fxRate: priceFx, defaultFx: 6.7, vendors: PRICES_BUILTIN.vendors, rows, builtinDate: PRICES_BUILTIN.date, refreshedAt: priceRefreshedAt };
}

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
    ["muse", "muse", 0.1, true],
    ["antigravity", "antigravity", 0.2, true],
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
