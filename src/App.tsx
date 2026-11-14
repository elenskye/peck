import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type ApiStatus } from "@/lib/api";
import { ipc, isMockMode } from "@/lib/ipc";
import type { AgentEvent, Control, ModelEntry } from "@/lib/agentEvents";
import { DEFAULT_SETTINGS, loadSettings, saveSettings, type AppSettings } from "@/lib/settings";
import { applyTheme } from "@/lib/theme";
import type { AgentInfo, Message, Permission, ProjectOverview, RunEvent, RunRecord, TermEvent } from "@/lib/types";
import { isAgentRun, plainLine, plainTerminal, rememberEntries, runState, shortenHomePath, whoLabel } from "@/lib/utils";
import { waitingOnYou } from "@/lib/agentView";
import { applyRunEvent } from "@/lib/runEvents";
import { TopBar } from "@/shell/TopBar";
import { LeftColumn } from "@/shell/LeftColumn";
import { chosenEntry, MiddleColumn } from "@/shell/MiddleColumn";
import { RightColumn, forgetTaskNotes, type RightTab } from "@/shell/RightColumn";
import { SettingsPage } from "@/shell/SettingsPage";
import { ProjectPage } from "@/shell/ProjectPage";
import type { InboxItem, LeftActions } from "@/shell/LeftColumn";
import { UsagePage } from "@/shell/UsagePage";
import { AgentsPanel } from "@/shell/AgentsPanel";
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
  /** 一个任务里发过好几次时，右栏和 LOG 看的是哪一次；没点过就是最新一次。 */
  const [shownRunId, setShownRunId] = useState<string | null>(null);
  const [runsByTask, setRunsByTask] = useState<Record<string, RunRecord[]>>({});
  const [messages, setMessages] = useState<Message[]>([]);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [apiStatus, setApiStatus] = useState<ApiStatus | null>(null);
  /** 模型列表（「谁来干」）。 */
  const [entries, setEntries] = useState<ModelEntry[]>([]);
  /** 统一事件：任务号 → 过程（Peck 循环、CLI 引擎的任务）。 */
  const [agentEvents, setAgentEvents] = useState<Record<string, AgentEvent[]>>({});
  /** 这次打开 App 以后发出去、还没做完的任务：任务号 → 它在哪个项目、哪个任务里。 */
  const [live, setLive] = useState<Record<string, { projectId: string; taskId: string }>>({});
  const liveRef = useRef(live);
  liveRef.current = live;
  const [page, setPage] = useState<"" | "settings" | "quota" | "project">("");
  const [tab, setTab] = useState<RightTab>("changes");
  const [wide, setWide] = useState(false);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [terms, setTerms] = useState<Record<string, TermView>>({});
  const [draft, setDraft] = useState("");
  /** 输入框下点了「谁」：AGENTS 闪一下。 */
  const [agentsFlash, setAgentsFlash] = useState(0);
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
  const refreshEntries = useCallback(async () => {
    try { const list = await ipc.gatewayModels(); rememberEntries(list); setEntries(list); } catch (e) { say(e); }
  }, [say]);

  useEffect(() => {
    let closed = false;
    let off: (() => void) | null = null;
    void (async () => {
      // 打开 App 时后台在读钥匙串（更新后会弹授权窗口）：读完重读 Key 状态和模型列表。先挂上再取，免得漏掉。
      if (!isMockMode()) {
        try {
          const f = await listen("keys-loaded", () => { void refreshApi(); void refreshEntries(); });
          if (closed) f(); else off = f;
        } catch { /* 浏览器里没有 */ }
      }
      const loaded = await loadSettings();
      setSettings(loaded);
      applyTheme(loaded.theme);
      if (loaded.appIcon !== "j2") void ipc.appIconSet(loaded.appIcon).catch(() => {});
      setReady(true);
      await Promise.all([refreshProjects(), refreshAgents(), refreshApi(), refreshEntries()]);
    })();
    return () => { closed = true; off?.(); };
  }, [refreshProjects, refreshAgents, refreshApi, refreshEntries]);

  // 回到 Peck 窗口（比如刚在访达里删了、挪了项目文件夹）就重读项目列表，文件夹不在了马上标出来。
  useEffect(() => {
    const again = () => { if (document.visibilityState === "visible") void refreshProjects(); };
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", again);
    return () => { window.removeEventListener("focus", again); document.removeEventListener("visibilitychange", again); };
  }, [refreshProjects]);

  const updateSettings = useCallback((patch: Partial<AppSettings>) => {
    const next = { ...settingsRef.current, ...patch };
    settingsRef.current = next;
    setSettings(next);
    if (patch.theme) applyTheme(patch.theme);
    if (patch.appIcon) void ipc.appIconSet(patch.appIcon).catch(say);
    void saveSettings(next).catch(say);
  }, [say]);

  const project = projects.find((p) => p.workspace.id === projectId) ?? null;
  const task = project?.tasks.find((t) => t.id === taskId) ?? null;
  const runs = useMemo(() => (taskId ? runsByTask[taskId] ?? [] : []), [runsByTask, taskId]);
  const run = runs.find((r) => r.id === shownRunId) ?? runs[runs.length - 1] ?? null;

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

  // 左栏「待你处理、在跑」（v0.7）：等你批或答的（这次打开 App 以后发的任务）、做完了还没看的、在跑的，跨项目。
  const inbox = useMemo(() => {
    const items: InboxItem[] = [];
    const waiting = new Set<string>();
    for (const [runId, l] of Object.entries(live)) {
      const ev = agentEvents[runId];
      if (ev?.length && waitingOnYou(ev)) waiting.add(l.taskId);
    }
    for (const p of projects) {
      for (const t of p.tasks) {
        const last = t.lastRun;
        if (!last) continue;
        const state = runState(last);
        const base = { projectId: p.workspace.id, taskId: t.id, title: plainLine(t.title), project: p.workspace.name, at: last.finishedAt ?? last.startedAt };
        if (waiting.has(t.id)) items.push({ ...base, kind: "waiting" });
        else if (state === "running") items.push({ ...base, kind: "running" });
        else if (state !== "undone" && (last.finishedAt ?? 0) > Math.max(settings.seen[t.id] ?? 0, settings.seenSince) && t.id !== taskId) items.push({ ...base, kind: state === "succeeded" ? "done" : "failed" });
      }
    }
    const order = { waiting: 0, failed: 1, done: 2, running: 3 };
    return items.sort((a, b) => order[a.kind] - order[b.kind] || b.at - a.at);
  }, [live, agentEvents, projects, settings.seen, settings.seenSince, taskId]);
  // 看过：打开一个任务，或者它在眼前做完了。
  const openLast = task?.lastRun;
  useEffect(() => {
    if (!taskId || !openLast || runState(openLast) === "running") return;
    const at = openLast.finishedAt ?? Date.now();
    if ((settingsRef.current.seen[taskId] ?? 0) >= at) return;
    const all = Object.entries({ ...settingsRef.current.seen, [taskId]: Date.now() }).sort((a, b) => b[1] - a[1]);
    const kept = all.slice(0, 300);
    // 挤出去的那些算「以前的都看过了」：把线抬到留下的最早那一条，免得它们又冒出来。
    const seenSince = all.length > kept.length ? Math.max(settingsRef.current.seenSince, kept[kept.length - 1][1]) : settingsRef.current.seenSince;
    updateSettings({ seen: Object.fromEntries(kept), seenSince });
  }, [taskId, openLast?.id, openLast?.status, openLast?.finishedAt]); // eslint-disable-line react-hooks/exhaustive-deps

  // 菜单栏小图标跟着变：等你 > 在跑 > 空闲；在跑两个以上图标旁边写个数（v0.7 起不同项目能同时跑）。
  const anyBusy = busyProjects.size > 0;
  const anyWaiting = inbox.some((i) => i.kind === "waiting");
  const runningCount = inbox.filter((i) => i.kind === "running" || i.kind === "waiting").length;
  useEffect(() => { void ipc.setActivity(anyWaiting ? "waiting" : anyBusy ? "running" : "idle", runningCount).catch(() => {}); }, [anyBusy, anyWaiting, runningCount]);

  // 选了任务：读它的对话和记录。
  const loadTask = useCallback(async (id: string) => {
    try {
      const [loadedRuns, loadedMessages] = await Promise.all([ipc.sessionGetRuns(id), ipc.sessionGetMessages(id)]);
      setRunsByTask((prev) => ({ ...prev, [id]: loadedRuns }));
      setMessages(loadedMessages);
      // Peck 循环、CLI 引擎的任务：过程在事件日志里（这次打开 App 以后正在收的不重读）。
      const agentRuns = loadedRuns.filter((r) => isAgentRun(r) && !liveRef.current[r.id]);
      const loaded = await Promise.all(agentRuns.map(async (r) => [r.id, await ipc.gatewayEvents(r.id).catch(() => [] as AgentEvent[])] as const));
      if (loaded.length) setAgentEvents((prev) => {
        const next = { ...prev };
        for (const [rid, evs] of loaded) {
          // 读的时候本地又追加过（比如刚批完记忆）：比读回来的新的那几条留着。
          const last = evs.length ? evs[evs.length - 1].seq : -1;
          next[rid] = [...evs, ...(prev[rid] ?? []).filter((e) => e.seq > last)];
        }
        return next;
      });
    } catch (e) { say(e); }
  }, [say]);
  useEffect(() => {
    setMessages([]);
    setFilePath(null);
    setShownRunId(null);
    if (taskId) void loadTask(taskId);
  }, [taskId, loadTask]);

  const receive = useCallback((ctx: { projectId: string; taskId: string; prompt: string; sent?: Pick<RunRecord, "who" | "permission"> }, event: RunEvent) => {
    setRunsByTask((prev) => ({ ...prev, [ctx.taskId]: applyRunEvent(prev[ctx.taskId] ?? [], ctx.taskId, ctx.prompt, event, ctx.sent) }));
    if (event.type === "RunStarted" || event.type === "StepStarted") {
      setLive((prev) => (prev[event.payload.runId] ? prev : { ...prev, [event.payload.runId]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
    }
    if (event.type === "RunFinished") {
      setLive((prev) => { const next = { ...prev }; delete next[event.payload.runId]; return next; });
      void refreshProjects();
      void ipc.sessionGetMessages(ctx.taskId).then((loaded) => setMessages((prev) => (prev.length && prev[0].sessionId !== ctx.taskId ? prev : loaded))).catch(() => {});
      if (settingsRef.current.notifyDone && !document.hasFocus()) {
        const word = event.payload.status === "succeeded" ? "做完了" : event.payload.status === "failed" ? "失败了" : "已停止";
        void ipc.notify(`Peck · ${word}`, plainLine(ctx.prompt).slice(0, 80)).catch(() => {});
      }
    }
  }, [refreshProjects]);

  /** Peck 循环、CLI 引擎的任务（统一事件）：收事件；要你批、要你答时 Peck 不在前面就发通知；做完重读任务。 */
  // 流式的事件一条一条来（每个字一条）：攒 40ms 一起放进去，不然每来一条整个任务就重算、重画一遍。
  // 用定时器不用 requestAnimationFrame：窗口藏起来时后者不走。
  const pendingEvents = useRef<Record<string, AgentEvent[]>>({});
  const flushTimer = useRef<number | null>(null);
  const queueAgentEvent = useCallback((runId: string, event: AgentEvent) => {
    (pendingEvents.current[runId] ??= []).push(event);
    if (flushTimer.current != null) return;
    flushTimer.current = window.setTimeout(() => {
      flushTimer.current = null;
      const batch = pendingEvents.current;
      pendingEvents.current = {};
      setAgentEvents((prev) => {
        const next = { ...prev };
        for (const [id, events] of Object.entries(batch)) next[id] = [...(prev[id] ?? []), ...events];
        return next;
      });
    }, 40);
  }, []);

  const onAgentEvent = useCallback((ctx: { projectId: string; taskId: string; prompt: string }, runId: string, event: AgentEvent) => {
    queueAgentEvent(runId, event);
    const s = settingsRef.current;
    if ((event.type === "approval.requested" || event.type === "user_input.requested") && s.notifyDone && !document.hasFocus()) {
      void ipc.notify(event.type === "approval.requested" ? "Peck · 等你批" : "Peck · 等你回答", event.type === "approval.requested" ? event.title : event.question).catch(() => {});
    }
    if (event.type === "turn.completed") {
      // 收尾（快照、改动、任务记录）在后台，记录写好了再重读。大仓库拍快照慢：越等越久地看，最多三分多钟。
      void (async () => {
        for (let i = 0, wait = 200; i < 120; i++, wait = Math.min(2000, Math.round(wait * 1.25))) {
          const loaded = await ipc.sessionGetRuns(ctx.taskId).catch(() => null);
          const record = loaded?.find((r) => r.id === runId);
          if (loaded && (!record || record.status !== "running")) {
            setRunsByTask((prev) => ({ ...prev, [ctx.taskId]: loaded }));
            break;
          }
          await new Promise((r) => setTimeout(r, wait));
        }
        setLive((prev) => { const next = { ...prev }; delete next[runId]; return next; });
        void refreshProjects();
        void ipc.sessionGetMessages(ctx.taskId).then((loaded) => setMessages((prev) => (prev.length && prev[0].sessionId !== ctx.taskId ? prev : loaded))).catch(() => {});
        if (s.notifyDone && !document.hasFocus()) {
          const word = event.reason === "done" ? "做完了" : event.reason === "interrupted" || event.reason === "cancelled" ? "已停止" : "停下了";
          void ipc.notify(`Peck · ${word}`, plainLine(ctx.prompt).slice(0, 80)).catch(() => {});
        }
      })();
    }
  }, [refreshProjects, queueAgentEvent]);

  /** 发任务：没选任务就新开一个（任务名是这句话），选着任务就接着在里面发。 */
  const send = useCallback(async (prompt: string, who: string, permission: Permission, effort?: string, model?: string, goal?: string[], worktree?: boolean) => {
    if (!project) return;
    if (project.missing) { say("这个项目的文件夹不在了：先重新找文件夹"); return; }
    if (busy) { say("同一个项目一次跑一个任务"); return; }
    try {
      let target = taskId;
      if (!target) {
        const created = await ipc.sessionCreate(project.workspace.id, prompt.replace(/\s+/g, " ").slice(0, 60));
        target = created.id;
      }
      const ctx = { projectId: project.workspace.id, taskId: target, prompt };
      const s = settingsRef.current;
      let runId: string;
      if (who.startsWith("api:") || who.startsWith("cli:")) {
        // 事件可能比任务号先到：先攒着，拿到任务号再一起放进去。
        let id: string | null = null;
        const early: AgentEvent[] = [];
        const onEvent = (event: AgentEvent) => { if (id) onAgentEvent(ctx, id, event); else early.push(event); };
        id = who.startsWith("api:")
          ? await ipc.harnessRunStart({ sessionId: target, prompt, who, permission, effort, costCap: s.costCap, goal, worktree }, onEvent)
          : await ipc.gatewayRunStart({ sessionId: target, prompt, who, permission, model, effort, resume: taskId ? lastSessionRef(runs, agentEvents, who) : undefined, worktree }, onEvent);
        runId = id;
        setLive((prev) => ({ ...prev, [runId]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
        setAgentEvents((prev) => ({ ...prev, [runId]: prev[runId] ?? [] }));
        for (const event of early) onAgentEvent(ctx, runId, event);
        const loaded = await ipc.sessionGetRuns(target);
        setRunsByTask((prev) => ({ ...prev, [target!]: loaded }));
      } else {
        const sent = { who: who.replace(/^flow:/, ""), permission };
        const onEvent = (event: RunEvent) => receive({ ...ctx, sent: who === "flow:plan-implement" ? { permission } : sent }, event);
        runId = who === "flow:plan-implement"
          ? await api.runStart(target, prompt, s.limits, "supervisor", "success", permission, onEvent)
          : await ipc.runDispatch(target, prompt, who.replace(/^flow:/, ""), permission, onEvent);
        setLive((prev) => ({ ...prev, [runId]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
      }
      setDraft("");
      setShownRunId(null);
      setTaskId(target);
      setTab((t) => (t === "term" || t === "oneshot" || t === "log" ? t : "changes"));
      await refreshProjects();
      setMessages(await ipc.sessionGetMessages(target));
    } catch (e) { say(e); void refreshProjects(); }
  }, [project, busy, taskId, runs, agentEvents, receive, onAgentEvent, refreshProjects, say]);

  const control = useCallback(async (runId: string, c: Control) => {
    await ipc.gatewayControl(runId, c);
  }, []);

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
        await send(`接着做上一个任务：先核对现在的文件，再做完。不要直接重放旧的工具调用，被打断的工具可能已经改过文件。\n原来要做的：${target.prompt}\n上次的结果：${root?.summary ?? target.summary ?? ""}\n最后几次工具调用：\n${evidence}`, target.flowId === "api-supervisor" ? "flow:plan-implement" : (chosenEntry(entries, settingsRef.current)?.id ?? settingsRef.current.pick), target.permission ?? settingsRef.current.permission);
      } catch (e) { say(e); }
      return;
    }
    const ctx = { projectId: project.workspace.id, taskId, prompt: target.prompt };
    try {
      setShownRunId(target.id);
      setLive((prev) => ({ ...prev, [target.id]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
      await ipc.runRetryStep(target.id, stepId, (event) => receive(ctx, event));
    } catch (e) {
      setLive((prev) => { const next = { ...prev }; delete next[target.id]; return next; });
      say(e);
    }
  }, [project, taskId, busy, entries, receive, send, say]);

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
      setPage("");
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

  /** 进一个项目：它有自己的默认谁来干、默认权限，就换成它的。 */
  const applyProjectDefaults = (id: string | null) => {
    const ws = projects.find((p) => p.workspace.id === id)?.workspace;
    const patch: Partial<AppSettings> = {};
    if (ws?.settings?.who && ws.settings.who !== settingsRef.current.pick) patch.pick = ws.settings.who;
    if (ws?.settings?.permission && ws.settings.permission !== settingsRef.current.permission) patch.permission = ws.settings.permission as Permission;
    if (Object.keys(patch).length) updateSettings(patch);
  };
  // 点项目时顺手重读一遍列表：文件夹在 App 开着的时候被删、被挪，马上就标出来。
  const selectProject = (id: string | null) => { if (id !== projectId) applyProjectDefaults(id); setProjectId(id); setTaskId(null); setPage(""); setWide(false); void refreshProjects(); };

  const act = (fn: () => Promise<unknown>) => { void fn().catch(say); };
  const relocateProject = (id: string) => act(async () => {
    const moved = await ipc.workspaceRelocate(id);
    if (!moved) return;
    await refreshProjects();
    if (projectId === id) setProjectId(moved.id);
    say(`接上了：${shortenHomePath(moved.path)}`);
  });
  const removeProject = (id: string) => act(async () => {
    await ipc.workspaceRemove(id);
    if (projectId === id) { setProjectId(null); setTaskId(null); setPage(""); }
    await refreshProjects();
    say("移出列表了；重新打开这个文件夹就回来");
  });
  const leftActions: LeftActions = {
    onRenameProject: (id, name) => act(async () => { await ipc.workspaceUpdate(id, { name }); await refreshProjects(); }),
    onProjectSettings: (id) => { setProjectId(id); setPage("project"); },
    onRevealProject: (id) => act(() => ipc.workspaceReveal(id)),
    onRelocateProject: relocateProject,
    onRemoveProject: removeProject,
    onRenameTask: (_pid, id, title) => act(async () => { await ipc.sessionRename(id, title); await refreshProjects(); }),
    onDeleteTask: (_pid, id) => act(async () => {
      await ipc.sessionDelete(id);
      forgetTaskNotes(id);
      if (taskId === id) setTaskId(null);
      setRunsByTask((prev) => { const next = { ...prev }; delete next[id]; return next; });
      await refreshProjects();
      say("任务删掉了（项目里的文件没动）");
    }),
    onReorderProjects: (ids) => {
      setProjects((prev) => ids.map((id) => prev.find((p) => p.workspace.id === id)!).filter(Boolean));
      act(() => ipc.layoutSave({ projects: ids }));
    },
    onReorderTasks: (pid, ids) => {
      setProjects((prev) => prev.map((p) => (p.workspace.id === pid ? { ...p, tasks: ids.map((id) => p.tasks.find((t) => t.id === id)!).filter(Boolean) } : p)));
      act(() => ipc.layoutSave({ workspaceId: pid, tasks: ids }));
    },
  };
  const selectTask = (pid: string, id: string | null) => { setProjectId(pid); setTaskId(id); setPage(""); };

  if (!ready) return <div className="pk pk-dots app" />;

  const native = !isMockMode();
  return (
    <div className="pk pk-dots app">
      <div className="app-drag" data-tauri-drag-region />
      <TopBar
        native={native}
        project={project?.workspace ?? null}
        page={page === "project" ? "" : page}
        onPage={(next) => setPage((p) => (p === next ? "" : next))}
      />
      <main className={page || wide ? "app-grid app-grid--wide" : "app-grid"}>
        <LeftColumn
          inbox={inbox}
          projects={projects}
          projectId={projectId}
          taskId={taskId}
          busyProjects={busyProjects}
          folded={!!settings.fold.projects}
          onFold={(projectsFolded) => updateSettings({ fold: { ...settingsRef.current.fold, projects: projectsFolded } })}
          agents={
            <AgentsPanel
              entries={entries}
              value={chosenEntry(entries, settings)}
              settings={settings}
              onSettings={updateSettings}
              apiStatus={apiStatus}
              onApiConfig={async (config) => { try { await api.saveConfig(config); await refreshApi(); } catch (e) { say(e); } }}
              disabled={busy}
              onOpenSettings={() => setPage("settings")}
              grow={!!settings.fold.projects}
              onFold={(agentsFolded) => updateSettings({ fold: { ...settingsRef.current.fold, agents: agentsFolded } })}
              flash={agentsFlash}
              say={say}
            />
          }
          onSelectProject={selectProject}
          onSelectTask={selectTask}
          onOpen={openProject}
          native={native}
          actions={leftActions}
        />
        {page === "project" && project ? (
          <ProjectPage
            key={project.workspace.id}
            project={project}
            entries={entries}
            fakeAgent={settings.fakeAgent}
            native={native}
            onChanged={refreshProjects}
            onRelocate={relocateProject}
            onReveal={(id) => act(() => ipc.workspaceReveal(id))}
            onRemove={removeProject}
            onClose={() => setPage("")}
            say={say}
          />
        ) : page === "settings" ? (
          <SettingsPage
            settings={settings}
            onSettings={updateSettings}
            agents={agents}
            onRefreshAgents={async () => { await refreshAgents(); await refreshEntries(); }}
            apiStatus={apiStatus}
            onRefreshApi={async () => { const st = await refreshApi(); void refreshEntries(); return st; }}
            onClose={() => setPage("")}
            say={say}
          />
        ) : page === "quota" ? (
          <UsagePage onClose={() => setPage("")} say={say} entries={entries} costCap={settings.costCap} />
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
                entries={entries}
                apiStatus={apiStatus}
                agentEvents={agentEvents}
                runningRunId={runningRunId}
                onControl={control}
                say={say}
                draft={draft}
                onDraft={setDraft}
                onSend={send}
                onAppendEvent={(runId, event) => setAgentEvents((prev) => ({ ...prev, [runId]: [...(prev[runId] ?? []), event] }))}
                onStop={stop}
                onRetryStep={retryStep}
                onOpen={openProject}
                onOpenSettings={() => setPage("settings")}
                onShowAgents={() => { if (settingsRef.current.fold.agents) updateSettings({ fold: { ...settingsRef.current.fold, agents: false } }); setAgentsFlash((n) => n + 1); }}
                onRelocate={relocateProject}
                onRemoveProject={removeProject}
                onProjectsChanged={() => void refreshProjects()}
                shownRunId={run?.id ?? null}
                onShowRun={(id, path) => { setShownRunId(id); setFilePath(path ?? null); setTab("changes"); }}
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
              entries={entries}
              settings={settings}
              onSettings={updateSettings}
              onToTask={(text) => setDraft((d) => (d.trim() ? `${d.trimEnd()}\n\n${text}` : text))}
              onRunUpdated={(record) => { setRunsByTask((prev) => ({ ...prev, [record.sessionId]: (prev[record.sessionId] ?? []).map((r) => (r.id === record.id ? { ...r, ...record } : r)) })); void refreshProjects(); }}
              native={native}
            />
          </>
        )}
      </main>
      {toast && (
        <div className="app-toast" role="status">
          <div className="pk-anim-reveal-s"><div className="pk-toast"><Icon name="bang" size={12} /><span>{toast}</span></div></div>
        </div>
      )}
      {run && <span className="sr-only" aria-live="polite">{`任务 ${run.number} ${whoLabel(run)}`}</span>}
    </div>
  );
};
export default App;

/** 同一个任务里接着发给同一个 CLI 引擎：带上上一次的会话号（`session.started` 的 `sessionRef`），它才记得前面说过什么。 */
function lastSessionRef(runs: RunRecord[], events: Record<string, AgentEvent[]>, who: string): string | undefined {
  const same = runs.filter((r) => r.who === who).sort((a, b) => b.number - a.number);
  for (const run of same) {
    for (const e of events[run.id] ?? []) {
      if (e.type === "session.started" && e.sessionRef) return e.sessionRef;
    }
  }
  return undefined;
}
