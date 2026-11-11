import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type ApiStatus } from "@/lib/api";
import { ipc, isMockMode } from "@/lib/ipc";
import { DEFAULT_SETTINGS, loadSettings, saveSettings, type AppSettings } from "@/lib/settings";
import { applyTheme } from "@/lib/theme";
import type { AgentInfo, Message, Permission, ProjectOverview, RunEvent, RunRecord, TermEvent } from "@/lib/types";
import { plainTerminal, whoLabel } from "@/lib/utils";
import { applyRunEvent } from "@/lib/runEvents";
import { TopBar } from "@/shell/TopBar";
import { LeftColumn } from "@/shell/LeftColumn";
import { MiddleColumn } from "@/shell/MiddleColumn";
import { RightColumn, type RightTab } from "@/shell/RightColumn";
import { SettingsPage } from "@/shell/SettingsPage";
import { UsagePage } from "@/shell/UsagePage";
import { Icon } from "@/ui";

export interface TermEntry {
  id: string;
  command: string;
  cwd: string;
  output: string;
  /** 跑完才有 */
  code?: number | null;
  durationMs?: number;
  stopped?: boolean;
}
export interface TermView {
  cwd: string;
  entries: TermEntry[];
  runningId: string | null;
  /** 切走以后跑完的那条：页签上一颗绿点或红点 */
  unread: "" | "ok" | "bad";
}
const EMPTY_TERM: TermView = { cwd: "", entries: [], runningId: null, unread: "" };
const MAX_TERM_OUTPUT = 60_000;

export const App: React.FC = () => {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [ready, setReady] = useState(false);
  const [projects, setProjects] = useState<ProjectOverview[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [runsByTask, setRunsByTask] = useState<Record<string, RunRecord[]>>({});
  const [messages, setMessages] = useState<Message[]>([]);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [apiStatus, setApiStatus] = useState<ApiStatus | null>(null);
  /** 这次打开 App 以后发出去、还没做完的任务：任务号 → 它在哪个项目、哪个任务里。 */
  const [live, setLive] = useState<Record<string, { projectId: string; taskId: string }>>({});
  const [page, setPage] = useState<"" | "settings" | "quota">("");
  const [tab, setTab] = useState<RightTab>("changes");
  const [wide, setWide] = useState(false);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [terms, setTerms] = useState<Record<string, TermView>>({});
  const [draft, setDraft] = useState("");
  const [toast, setToast] = useState("");
  const toastTimer = useRef<number>(0);
  const tabRef = useRef(tab);
  tabRef.current = tab;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  /** 底部提示条：一时不能做、或做了会怎样的事，说一句（2.8 秒）。 */
  const say = useCallback((message: unknown) => {
    const text = String(message instanceof Error ? message.message : message).replace(/^Error:\s*/, "");
    setToast(text);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(""), text.length > 40 ? 5600 : 2800);
  }, []);

  const refreshProjects = useCallback(async () => {
    try { setProjects(await ipc.overview()); } catch (e) { say(e); }
  }, [say]);
  const refreshAgents = useCallback(async () => {
    try { setAgents(await ipc.agentsList()); } catch (e) { say(e); }
  }, [say]);
  const refreshApi = useCallback(async () => {
    try { const status = await api.status(); setApiStatus(status); return status; } catch (e) { say(e); return null; }
  }, [say]);

  useEffect(() => {
    void (async () => {
      const loaded = await loadSettings();
      setSettings(loaded);
      applyTheme(loaded.theme);
      setReady(true);
      await Promise.all([refreshProjects(), refreshAgents(), refreshApi()]);
    })();
  }, [refreshProjects, refreshAgents, refreshApi]);

  const updateSettings = useCallback((patch: Partial<AppSettings>) => {
    const next = { ...settingsRef.current, ...patch };
    settingsRef.current = next;
    setSettings(next);
    if (patch.theme) applyTheme(patch.theme);
    void saveSettings(next).catch(say);
  }, [say]);

  const project = projects.find((p) => p.workspace.id === projectId) ?? null;
  const task = project?.tasks.find((t) => t.id === taskId) ?? null;
  const runs = useMemo(() => (taskId ? runsByTask[taskId] ?? [] : []), [runsByTask, taskId]);
  const run = runs[runs.length - 1] ?? null;

  // 一个项目有没有任务在跑：这次发出去的，或者列表里还标着在跑的。
  const busyProjects = useMemo(() => {
    const set = new Set(Object.values(live).map((l) => l.projectId));
    for (const p of projects) if (p.tasks.some((t) => t.lastRun?.status === "running")) set.add(p.workspace.id);
    return set;
  }, [live, projects]);
  const busy = projectId ? busyProjects.has(projectId) : false;
  const runningRunId = useMemo(() => {
    if (!projectId) return null;
    const fromLive = Object.entries(live).find(([, l]) => l.projectId === projectId)?.[0];
    return fromLive ?? project?.tasks.find((t) => t.lastRun?.status === "running")?.lastRun?.id ?? null;
  }, [live, project, projectId]);

  // 菜单栏小图标跟着变。
  const anyBusy = busyProjects.size > 0;
  useEffect(() => { void ipc.setActivity(anyBusy ? "running" : "idle").catch(() => {}); }, [anyBusy]);

  // 选了任务：读它的对话和记录。
  const loadTask = useCallback(async (id: string) => {
    try {
      const [loadedRuns, loadedMessages] = await Promise.all([ipc.sessionGetRuns(id), ipc.sessionGetMessages(id)]);
      setRunsByTask((prev) => ({ ...prev, [id]: loadedRuns }));
      setMessages(loadedMessages);
    } catch (e) { say(e); }
  }, [say]);
  useEffect(() => {
    setMessages([]);
    setFilePath(null);
    if (taskId) void loadTask(taskId);
  }, [taskId, loadTask]);

  const receive = useCallback((ctx: { projectId: string; taskId: string; prompt: string }, event: RunEvent) => {
    setRunsByTask((prev) => ({ ...prev, [ctx.taskId]: applyRunEvent(prev[ctx.taskId] ?? [], ctx.taskId, ctx.prompt, event) }));
    if (event.type === "RunStarted" || event.type === "StepStarted") {
      setLive((prev) => (prev[event.payload.runId] ? prev : { ...prev, [event.payload.runId]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
    }
    if (event.type === "RunFinished") {
      setLive((prev) => { const next = { ...prev }; delete next[event.payload.runId]; return next; });
      void refreshProjects();
      void ipc.sessionGetMessages(ctx.taskId).then((loaded) => setMessages((prev) => (prev.length && prev[0].sessionId !== ctx.taskId ? prev : loaded))).catch(() => {});
      if (settingsRef.current.notifyDone && !document.hasFocus()) {
        const word = event.payload.status === "succeeded" ? "做完了" : event.payload.status === "failed" ? "失败了" : "已停止";
        void ipc.notify(`Peck · ${word}`, ctx.prompt.slice(0, 80)).catch(() => {});
      }
    }
  }, [refreshProjects]);

  /** 发任务：没选任务就新开一个（任务名是这句话），选着任务就接着在里面发。 */
  const send = useCallback(async (prompt: string, who: string, permission: Permission) => {
    if (!project) return;
    if (busy) { say("同一个项目一次跑一个任务"); return; }
    try {
      let target = taskId;
      if (!target) {
        const created = await ipc.sessionCreate(project.workspace.id, prompt.replace(/\s+/g, " ").slice(0, 60));
        target = created.id;
      }
      const ctx = { projectId: project.workspace.id, taskId: target, prompt };
      const onEvent = (event: RunEvent) => receive(ctx, event);
      const s = settingsRef.current;
      const runId = who.startsWith("api-")
        ? await api.runStart(target, prompt, s.limits, who === "api-supervisor" ? "supervisor" : "worker", "success", permission, onEvent)
        : await ipc.runDispatch(target, prompt, who, permission, onEvent);
      setLive((prev) => ({ ...prev, [runId]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
      setDraft("");
      setTaskId(target);
      setTab((t) => (t === "term" || t === "oneshot" ? t : "changes"));
      await refreshProjects();
      setMessages(await ipc.sessionGetMessages(target));
    } catch (e) { say(e); void refreshProjects(); }
  }, [project, busy, taskId, receive, refreshProjects, say]);

  const stop = useCallback(async () => {
    if (!runningRunId) return;
    try {
      await ipc.runCancel(runningRunId);
      // 列表里标着在跑、但不是这次发出去的（比如演示数据里那个）：停了以后重读一遍。
      if (!live[runningRunId]) { await refreshProjects(); if (taskId) await loadTask(taskId); }
    } catch (e) { say(e); }
  }, [runningRunId, live, refreshProjects, taskId, loadTask, say]);

  const retryStep = useCallback(async (target: RunRecord, stepId: string) => {
    if (!project || !taskId) return;
    if (busy) { say("同一个项目一次跑一个任务"); return; }
    if (target.flowId.startsWith("api-")) {
      // API 模式不重放跑过的工具：把上次的结果带上，从现在的文件接着发一个任务。
      try {
        const history = await api.taskHistory(target.id);
        const root = history.find((t) => !t.parentId);
        const evidence = history.flatMap((t) => t.tools).slice(-6).map((t) => `${t.tool.name} (${t.status}): ${t.output.slice(0, 1500)}`).join("\n");
        await send(`接着做上一个任务：先核对现在的文件，再做完。不要直接重放旧的工具调用，被打断的工具可能已经改过文件。\n原来要做的：${target.prompt}\n上次的结果：${root?.summary ?? target.summary ?? ""}\n最后几次工具调用：\n${evidence}`, target.flowId, target.permission ?? settingsRef.current.permission);
      } catch (e) { say(e); }
      return;
    }
    const ctx = { projectId: project.workspace.id, taskId, prompt: target.prompt };
    try {
      setLive((prev) => ({ ...prev, [target.id]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
      await ipc.runRetryStep(target.id, stepId, (event) => receive(ctx, event));
    } catch (e) {
      setLive((prev) => { const next = { ...prev }; delete next[target.id]; return next; });
      say(e);
    }
  }, [project, taskId, busy, receive, send, say]);

  const undo = useCallback(async (target: RunRecord) => {
    try {
      const updated = await ipc.runUndo(target.id);
      setRunsByTask((prev) => ({ ...prev, [updated.sessionId]: (prev[updated.sessionId] ?? []).map((r) => (r.id === updated.id ? { ...r, undoneAt: updated.undoneAt } : r)) }));
      await refreshProjects();
    } catch (e) { say(e); throw e; }
  }, [refreshProjects, say]);

  const openProject = useCallback(async (path?: string) => {
    try {
      const opened = path ? await ipc.workspaceOpen(path) : await ipc.workspaceChoose();
      if (!opened) {
        if (isMockMode()) say("浏览器演示里选不了文件夹，输入一个路径试试");
        return;
      }
      await refreshProjects();
      setProjectId(opened.id);
      setTaskId(null);
    } catch (e) { say(e); }
  }, [refreshProjects, say]);

  // ---------- 终端 ----------
  const term = (projectId && terms[projectId]) || EMPTY_TERM;
  useEffect(() => {
    if (!projectId) return;
    void ipc.termState(projectId).then((state) => setTerms((prev) => ({ ...prev, [projectId]: { ...(prev[projectId] ?? EMPTY_TERM), cwd: state.cwd, runningId: state.running ?? prev[projectId]?.runningId ?? null } }))).catch(() => {});
  }, [projectId]);
  const patchTerm = useCallback((id: string, fn: (view: TermView) => TermView) => {
    setTerms((prev) => ({ ...prev, [id]: fn(prev[id] ?? EMPTY_TERM) }));
  }, []);
  const termRun = useCallback(async (command: string) => {
    if (!projectId) return;
    const target = projectId;
    // 输出可能比「命令起来了」的回执先到：还没拿到命令号时，先记在刚加的那一条上。
    const pendingId = `pending-${Date.now()}`;
    const onEvent = (event: TermEvent) => {
      patchTerm(target, (view) => {
        const entries = view.entries.map((entry) => {
          if (entry.id !== event.payload.id && entry.id !== pendingId) return entry;
          if (event.type === "Output") return { ...entry, id: event.payload.id, output: (entry.output + plainTerminal(event.payload.chunk)).slice(-MAX_TERM_OUTPUT) };
          return { ...entry, id: event.payload.id, code: event.payload.code, durationMs: event.payload.durationMs, stopped: event.payload.stopped };
        });
        if (event.type === "Output") return { ...view, entries, runningId: view.runningId === pendingId ? event.payload.id : view.runningId };
        return { ...view, entries, cwd: event.payload.cwd, runningId: null, unread: tabRef.current === "term" ? "" : event.payload.code === 0 ? "ok" : "bad" };
      });
    };
    try {
      patchTerm(target, (view) => ({ ...view, runningId: pendingId, unread: "", entries: [...view.entries, { id: pendingId, command, cwd: view.cwd, output: "" }].slice(-40) }));
      const id = await ipc.termRun(target, command, onEvent);
      patchTerm(target, (view) => ({ ...view, runningId: view.runningId === pendingId ? id : view.runningId, entries: view.entries.map((e) => (e.id === pendingId ? { ...e, id } : e)) }));
    } catch (e) {
      patchTerm(target, (view) => ({ ...view, runningId: view.runningId === pendingId ? null : view.runningId, entries: view.entries.filter((entry) => entry.id !== pendingId) }));
      say(e);
    }
  }, [projectId, patchTerm, say]);
  const termStop = useCallback(() => { if (projectId) void ipc.termStop(projectId).catch(say); }, [projectId, say]);
  const termClear = useCallback(() => {
    if (projectId) patchTerm(projectId, (view) => ({ ...view, unread: "", entries: view.entries.filter((e) => e.id === view.runningId) }));
  }, [projectId, patchTerm]);
  const selectTab = useCallback((next: RightTab) => {
    setTab(next);
    if (next === "term" && projectId) patchTerm(projectId, (view) => ({ ...view, unread: "" }));
  }, [projectId, patchTerm]);

  const selectProject = (id: string | null) => { setProjectId(id); setTaskId(null); setPage(""); setWide(false); };
  const selectTask = (pid: string, id: string | null) => { setProjectId(pid); setTaskId(id); setPage(""); };

  if (!ready) return <div className="pk pk-dots app" />;

  const native = !isMockMode();
  return (
    <div className="pk pk-dots app">
      <div className="app-drag" data-tauri-drag-region />
      <TopBar
        native={native}
        project={project?.workspace ?? null}
        mode={settings.mode}
        onMode={(mode) => updateSettings({ mode })}
        page={page}
        onPage={(next) => setPage((p) => (p === next ? "" : next))}
      />
      <main className={page || wide ? "app-grid app-grid--wide" : "app-grid"}>
        <LeftColumn
          projects={projects}
          projectId={projectId}
          taskId={taskId}
          busyProjects={busyProjects}
          run={run}
          onSelectProject={selectProject}
          onSelectTask={selectTask}
          onOpen={openProject}
          native={native}
        />
        {page === "settings" ? (
          <SettingsPage
            settings={settings}
            onSettings={updateSettings}
            agents={agents}
            onRefreshAgents={refreshAgents}
            apiStatus={apiStatus}
            onRefreshApi={refreshApi}
            onClose={() => setPage("")}
            say={say}
          />
        ) : page === "quota" ? (
          <UsagePage onClose={() => setPage("")} say={say} />
        ) : (
          <>
            {!wide && (
              <MiddleColumn
                project={project}
                task={task}
                runs={runs}
                messages={messages}
                busy={busy}
                settings={settings}
                onSettings={updateSettings}
                agents={agents}
                apiStatus={apiStatus}
                draft={draft}
                onDraft={setDraft}
                onSend={send}
                onStop={stop}
                onRetryStep={retryStep}
                onOpen={openProject}
                onOpenSettings={() => setPage("settings")}
                onShowFile={(path) => { setFilePath(path); setTab("changes"); }}
                native={native}
              />
            )}
            <RightColumn
              project={project}
              run={run}
              tab={tab}
              onTab={selectTab}
              wide={wide}
              onWide={setWide}
              busy={busy}
              filePath={filePath}
              onFilePath={setFilePath}
              onUndo={undo}
              term={term}
              onTermRun={termRun}
              onTermStop={termStop}
              onTermClear={termClear}
              say={say}
            />
          </>
        )}
      </main>
      {toast && (
        <div className="app-toast" role="status">
          <div className="pk-toast pk-anim-reveal-s"><Icon name="bang" size={12} /><span>{toast}</span></div>
        </div>
      )}
      {run && <span className="sr-only" aria-live="polite">{`任务 ${run.number} ${whoLabel(run)}`}</span>}
    </div>
  );
};
export default App;
