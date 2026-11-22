import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type ApiStatus } from "@/lib/api";
import { ipc, isMockMode, type ImageInput, type Schedule, type TeamPick } from "@/lib/ipc";
import type { AgentEvent, Control, ModelEntry } from "@/lib/agentEvents";
import { DEFAULT_SETTINGS, loadSettings, migratePick, saveSettings, type AppSettings } from "@/lib/settings";
import { applyPreset, canDelegate, presetProblems } from "@/lib/presets";
import { involvedIds, quotaHint, swapPatch } from "@/lib/quota";
import { usageApi, type ClientCard } from "@/lib/usage";
import { applyTheme } from "@/lib/theme";
import type { Permission, ProjectOverview, RunRecord, TermEvent } from "@/lib/types";
import { isAgentRun, plainLine, plainTerminal, rememberEntries, runState, shortenHomePath, whoLabel } from "@/lib/utils";
import { waitingOnYou } from "@/lib/agentView";
import { TopBar } from "@/shell/TopBar";
import { LeftColumn, ProjectsPage } from "@/shell/LeftColumn";
import { chosenEntry, MiddleColumn } from "@/shell/MiddleColumn";
import { RightColumn, type RightTab } from "@/shell/RightColumn";
import { SettingsPage, type SettingsSection } from "@/shell/SettingsPage";
import { ProjectPage } from "@/shell/ProjectPage";
import type { InboxItem, LeftActions } from "@/shell/LeftColumn";
import { UsagePage } from "@/shell/UsagePage";
import { AgentsPanel } from "@/shell/AgentsPanel";
import { RELAY_ID, cliModelOf, sendEffort, teamable } from "@/shell/ModelPicker";
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

const App: React.FC = () => {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [ready, setReady] = useState(false);
  const [projects, setProjects] = useState<ProjectOverview[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  /** 从设置 → ARCHIVE 打开的归档任务（只读）：它那一组，项目可能已经移出列表。 */
  const [peek, setPeek] = useState<ProjectOverview | null>(null);
  /** 一个任务里发过好几次时，右栏和 LOG 看的是哪一次；没点过就是最新一次。 */
  const [shownRunId, setShownRunId] = useState<string | null>(null);
  const [runsByTask, setRunsByTask] = useState<Record<string, RunRecord[]>>({});
  const [apiStatus, setApiStatus] = useState<ApiStatus | null>(null);
  /** 模型列表（「谁来干」）。 */
  const [entries, setEntries] = useState<ModelEntry[]>([]);
  /** 统一事件：任务号 → 过程（Peck 循环、CLI 引擎的任务）。 */
  const [agentEvents, setAgentEvents] = useState<Record<string, AgentEvent[]>>({});
  /** 这次打开 App 以后发出去、还没做完的任务：任务号 → 它在哪个项目、哪个任务里。 */
  const [live, setLive] = useState<Record<string, { projectId: string; taskId: string }>>({});
  const liveRef = useRef(live);
  liveRef.current = live;
  const projectsRef = useRef(projects);
  projectsRef.current = projects;
  const [page, setPage] = useState<"" | "settings" | "quota" | "project">("");
  /** 设置打开时先到哪一组（AGENTS 面板的「管理预设」到 AGENTS）。 */
  const [settingsAt, setSettingsAt] = useState<SettingsSection | undefined>(undefined);
  const [tab, setTab] = useState<RightTab>("files");
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
      await Promise.all([refreshProjects(), refreshApi(), refreshEntries()]);
    })();
    return () => { closed = true; off?.(); };
  }, [refreshProjects, refreshApi, refreshEntries]);

  // 额度提醒（v0.9 B3）：QUOTA 页那几张客户端卡片（每家的 5 小时 / 每周窗口）；用量监控更新时推过来。
  const [clients, setClients] = useState<ClientCard[]>([]);
  useEffect(() => {
    let closed = false;
    let off: (() => void) | null = null;
    void usageApi.clients().then((c) => { if (!closed) setClients(c); }).catch(() => {});
    if (!isMockMode()) {
      void listen<ClientCard[]>("usage-updated", (e) => setClients(e.payload)).then((f) => { if (closed) f(); else off = f; }).catch(() => {});
    }
    return () => { closed = true; off?.(); };
  }, []);
  /** 「这次换」：只换下一次发出去的那一个（from → to），发完就清掉；设置不动。 */
  const [swapOnce, setSwapOnce] = useState<{ from: string; to: string } | null>(null);
  const swapRef = useRef(swapOnce);
  swapRef.current = swapOnce;
  const [quotaDismissed, setQuotaDismissed] = useState<string | null>(null);
  const chosenId = useMemo(() => (settings.pick === RELAY_ID ? RELAY_ID : chosenEntry(entries, settings)?.id ?? null), [entries, settings]);
  const hint = useMemo(() => {
    const h = quotaHint(involvedIds(settings, chosenId), entries, clients, settings.quotaWarn);
    return h && `${h.from}|${h.text}` !== quotaDismissed ? h : null;
  }, [settings, chosenId, entries, clients, quotaDismissed]);
  // 换的那一家这次已经用不到了（AGENTS 改了）：「这次换」作废。
  useEffect(() => {
    if (swapOnce && !involvedIds(settings, chosenId).includes(swapOnce.from)) setSwapOnce(null);
  }, [swapOnce, settings, chosenId]);

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

  const listed = projects.find((p) => p.workspace.id === projectId) ?? null;
  const peekTask = peek && peek.workspace.id === projectId ? peek.tasks.find((t) => t.id === taskId) ?? null : null;
  const project = peekTask ? listed ?? peek : listed;
  const task = peekTask ?? listed?.tasks.find((t) => t.id === taskId) ?? null;
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
        // 没批的 Skill 提议（v0.10 C3，后台数的）：跑完了也算等你。
        if (waiting.has(t.id) || (t.skillsWaiting ?? 0) > 0) items.push({ ...base, kind: "waiting" });
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
      const loadedRuns = await ipc.sessionGetRuns(id);
      setRunsByTask((prev) => ({ ...prev, [id]: loadedRuns }));
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
    setFilePath(null);
    setShownRunId(null);
    if (taskId) void loadTask(taskId);
  }, [taskId, loadTask]);
  const taskIdRef = useRef(taskId);
  taskIdRef.current = taskId;
  /** 别处（定时任务）往一个对话里发了一次：正看着它就重读。 */
  const loadTaskRef = useRef((_id: string) => Promise.resolve());
  loadTaskRef.current = async (id: string) => { if (taskIdRef.current === id) await loadTask(id); };

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
        for (const [id, events] of Object.entries(batch)) {
          // 本地先追加过的（跑着时批的 Skill：命令的返回值和事件流各来一份）不重复放。
          const have = new Set((prev[id] ?? []).slice(-40).map((e) => e.seq));
          next[id] = [...(prev[id] ?? []), ...events.filter((e) => !have.has(e.seq))];
        }
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
        if (s.notifyDone && !document.hasFocus()) {
          const word = event.reason === "done" ? "做完了" : event.reason === "interrupted" || event.reason === "cancelled" ? "已停止" : "停下了";
          void ipc.notify(`Peck · ${word}`, plainLine(ctx.prompt).slice(0, 80)).catch(() => {});
        }
      })();
    }
  }, [refreshProjects, queueAgentEvent]);

  // 定时任务：设、改、取消都在对话里（模型的工具），这里只读列表。发出去的任务和自己发的一样进「在跑」、收事件；
  // 做完进左栏 NOW。`null` 是还没读回来。
  const [schedules, setSchedules] = useState<Schedule[] | null>(null);
  const refreshSchedules = useCallback(() => { void ipc.scheduleList().then(setSchedules).catch(() => setSchedules((prev) => prev ?? [])); }, []);
  const onAgentEventRef = useRef(onAgentEvent);
  onAgentEventRef.current = onAgentEvent;
  useEffect(() => {
    let off: (() => void) | null = null;
    let closed = false;
    refreshSchedules();
    void ipc.onSchedule(
      (f) => {
        setLive((prev) => ({ ...prev, [f.runId]: { projectId: f.workspaceId, taskId: f.sessionId } }));
        setAgentEvents((prev) => ({ ...prev, [f.runId]: prev[f.runId] ?? [] }));
        void refreshProjects();
        // 正看着的那个对话里接着跑的：把这一次读进来。
        void loadTaskRef.current(f.sessionId);
      },
      (e) => onAgentEventRef.current({ projectId: e.workspaceId, taskId: e.sessionId, prompt: e.prompt }, e.runId, e.event),
      refreshSchedules,
    ).then((f) => { if (closed) f(); else off = f; }).catch(() => {});
    return () => { closed = true; off?.(); };
  }, [refreshProjects, refreshSchedules]);
  /** 去这个定时任务跑的地方：它记着的那个对话，或这个项目的新任务页（错过的卡片在那里）。 */
  const goSchedule = useCallback((s: Schedule) => {
    const p = projectsRef.current.find((x) => x.workspace.id === s.workspaceId);
    if (!p) { say("这个定时任务的项目不在列表里了"); return; }
    setPeek(null);
    setProjectId(p.workspace.id);
    setTaskId(s.session && p.tasks.some((t) => t.id === s.session) ? s.session : null);
    setPage("");
    setWide(false);
  }, [say]);
  // 打开 App 时有错过的（App 关着时到点的）：跳到第一个的项目里，卡片就在眼前。只在刚打开时跳一次。
  const popped = useRef(false);
  useEffect(() => {
    if (popped.current || !ready || schedules === null || (schedules.length > 0 && projects.length === 0)) return;
    popped.current = true;
    const missed = schedules.filter((s) => s.missedAt && projects.some((p) => p.workspace.id === s.workspaceId));
    if (!missed.length) return;
    goSchedule(missed[0]);
    say(missed.length > 1 ? `有 ${missed.length} 个定时任务错过了：点 PROJECTS 头上的钟挨个看` : `定时任务「${missed[0].title}」错过了：RUN NOW 补跑，或 SKIP`);
  }, [ready, schedules, projects, goSchedule, say]);
  const onMissed = useCallback(async (s: Schedule, run: boolean) => {
    try {
      const fired = await ipc.scheduleMissed(s.id, run);
      refreshSchedules();
      if (fired) { setProjectId(fired.workspaceId); setTaskId(fired.sessionId); setPage(""); }
      else say("跳过了这一次");
    } catch (e) { say(e); }
  }, [refreshSchedules, say]);

  /** 发任务：没选任务就新开一个（任务名是这句话），选着任务就接着在里面发。发出去了回 true（输入框这才清掉附图）。 */
  const send = useCallback(async (prompt: string, who: string, permission: Permission, effort?: string, model?: string, worktree?: boolean, images?: ImageInput[]): Promise<boolean> => {
    if (!project) return false;
    if (project.missing) { say("这个项目的文件夹不在了：先重新找文件夹"); return false; }
    if (busy) { say("同一个项目一次跑一个任务"); return false; }
    const s = settingsRef.current;
    // 额度提醒里点了「这次换」（v0.9 B3）：这一次把 from 换成 to（模型、强度照 to 自己的），设置不动。
    const swap = swapRef.current;
    const swapped = (id?: string) => (swap && id === swap.from ? swap.to : id);
    if (swap && who === swap.from) {
      const e = entries.find((x) => x.id === swap.to) ?? null;
      who = swap.to;
      model = cliModelOf(e, s);
      effort = sendEffort(e, s);
    }
    // 接力（v0.8）：实施的是 relay.build、写规范的是 relay.spec；开了审查（不是 PLAN）也走合作。
    // 存着的可能已经用不了（CLI 这次没检测到、Key 删了）：只认 AGENTS 里现在能选的，和界面上显示的一样。
    const team = teamable(entries);
    const usable = (id?: string) => (id && team.some((e) => e.id === id) ? id : undefined);
    const pickOf = (id: string): TeamPick => {
      const e = entries.find((x) => x.id === id) ?? null;
      return { who: id, model: cliModelOf(e, s), effort: sendEffort(e, s) };
    };
    const relay = who === RELAY_ID;
    const spec = relay ? usable(swapped(s.relay.spec)) : undefined;
    const builder = relay ? usable(swapped(s.relay.build)) : who;
    if (!builder) { say("接力还没选实施的那一个（或选的那个现在用不了）"); return false; }
    if (relay && !spec) { say("接力还没选写规范的那一个（或选的那个现在用不了）"); return false; }
    if (relay && permission === "plan") { say("PLAN 只读：接力要动手，选 EDIT 或 AUTO"); return false; }
    // Antigravity 这类只做直接调用的不进审查、委派。
    const automatable = relay || team.some((e) => e.id === builder);
    const reviewId = automatable && permission !== "plan" ? usable(swapped(s.review)) : undefined;
    // 委派（v0.9 B1）：实施的是 API 模型或能经本机 MCP 委派的 CLI（Claude Code、Codex、Grok）。
    const delegate = canDelegate(entries.find((e) => e.id === builder)) ? usable(s.delegate) : undefined;
    try {
      let target = taskId;
      if (!target) {
        const created = await ipc.sessionCreate(project.workspace.id, prompt.replace(/\s+/g, " ").slice(0, 60));
        target = created.id;
      }
      const ctx = { projectId: project.workspace.id, taskId: target, prompt };
      let runId: string;
      const builderPick = relay ? pickOf(builder) : { who, model, effort };
      const reviewBy = reviewId ? pickOf(reviewId) : undefined;
      // 事件可能比任务号先到：先攒着，拿到任务号再一起放进去。
      let id: string | null = null;
      const early: AgentEvent[] = [];
      const onEvent = (event: AgentEvent) => { if (id) onAgentEvent(ctx, id, event); else early.push(event); };
      if (relay || reviewBy) {
        id = await ipc.teamRunStart({ sessionId: target, prompt, who: builderPick.who, model: builderPick.model, effort: builderPick.effort, permission, costCap: s.costCap, worktree, delegate, planBy: spec ? pickOf(spec) : undefined, reviewBy, images }, onEvent);
      } else if (who.startsWith("api:")) {
        id = await ipc.harnessRunStart({ sessionId: target, prompt, who, permission, effort, costCap: s.costCap, worktree, delegate, images }, onEvent);
      } else {
        id = await ipc.gatewayRunStart({ sessionId: target, prompt, who, permission, model, effort, resume: taskId ? lastSessionRef(runs, agentEvents, who) : undefined, worktree, images, delegate, costCap: s.costCap }, onEvent);
      }
      runId = id;
      if (swap) setSwapOnce(null);
      setLive((prev) => ({ ...prev, [runId]: { projectId: ctx.projectId, taskId: ctx.taskId } }));
      setAgentEvents((prev) => ({ ...prev, [runId]: prev[runId] ?? [] }));
      for (const event of early) onAgentEvent(ctx, runId, event);
      const loaded = await ipc.sessionGetRuns(target);
      setRunsByTask((prev) => ({ ...prev, [target!]: loaded }));
      setDraft("");
      setShownRunId(null);
      setTaskId(target);
      await refreshProjects();
      return true;
    } catch (e) { say(e); void refreshProjects(); return false; }
  }, [project, busy, taskId, runs, agentEvents, entries, onAgentEvent, refreshProjects, say]);

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

  /** 旧记录（v0.8 以前的接力、规划 + 实施、假 Agent）没做完的：不重放旧的那一步，用现在选的谁从现在的文件接着做。 */
  const retryStep = useCallback(async (target: RunRecord, _stepId: string) => {
    // 归档的任务（只在设置 → ARCHIVE 里打开看）不重跑：拿回左栏才能接着发。
    if (!project || !taskId || peekTask) return;
    if (busy) { say("同一个项目一次跑一个任务"); return; }
    try {
      let evidence = "";
      if (target.flowId.startsWith("api-")) {
        const history = await api.taskHistory(target.id).catch(() => []);
        evidence = history.flatMap((t) => t.tools ?? []).slice(-6).map((t) => `${t.tool.name} (${t.status}): ${t.output.slice(0, 1500)}`).join("\n");
      }
      const who = chosenEntry(entries, settingsRef.current)?.id ?? settingsRef.current.pick;
      await send(`接着做上一个任务：先核对现在的文件，再做完。不要直接重放旧的步骤，被打断的那一步可能已经改过文件。\n原来要做的：${target.prompt}\n上次的结果：${target.summary ?? ""}${evidence ? `\n最后几次工具调用：\n${evidence}` : ""}`, who, (target.permission as Permission | undefined) ?? settingsRef.current.permission);
    } catch (e) { say(e); }
  }, [project, taskId, peekTask, busy, entries, send, say]);

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
    // v0.8 以前项目里存的旧接力、规划 + 实施、假 Agent 照全局的办法换掉（换不了的不动现在选的）。
    // 项目指定了合作预设（v0.9 B2）：套用它（里面有用不了的就不换，提示一句），不看 who。
    const preset = ws?.settings?.preset ? settingsRef.current.presets.find((p) => p.id === ws.settings!.preset) : undefined;
    const problems = preset ? presetProblems(preset, entries) : [];
    if (preset && !problems.length) Object.assign(patch, applyPreset(preset, settingsRef.current));
    else if (preset) say(`这个项目的预设「${preset.name}」现在用不了：${problems.join("；")}`);
    const moved = !preset && ws?.settings?.who ? migratePick(ws.settings.who, settingsRef.current.relay) : null;
    if (moved?.pick && moved.pick !== settingsRef.current.pick) { patch.pick = moved.pick; if (moved.relay !== settingsRef.current.relay) patch.relay = moved.relay; }
    if (ws?.settings?.permission && ws.settings.permission !== settingsRef.current.permission) patch.permission = ws.settings.permission as Permission;
    if (Object.keys(patch).length) updateSettings(patch);
  };
  // 点项目时顺手重读一遍列表：文件夹在 App 开着的时候被删、被挪，马上就标出来。
  const selectProject = (id: string | null) => { if (id !== projectId) applyProjectDefaults(id); setPeek(null); setProjectId(id); setTaskId(null); setPage(""); setWide(false); void refreshProjects(); };

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
  const archiveProjectTasks = (id: string) => act(async () => {
    const n = await ipc.workspaceArchiveTasks(id);
    if (projectId === id) setTaskId(null);
    await refreshProjects();
    say(n ? `归档了 ${n} 个任务：设置 → ARCHIVE 里看、拿回来` : "没有能归档的任务（在跑的不归档）");
  });
  /** 设置 → ARCHIVE 里点开一个：只读地看，左栏不动。 */
  const openArchived = (group: ProjectOverview, id: string) => { setPeek(group); setProjectId(group.workspace.id); setTaskId(id); setPage(""); setWide(false); };
  /** 拿回左栏（项目移出列表了一起放回）。`open` 时跳过去接着看。 */
  const restoreArchived = async (id: string, open: boolean) => {
    const ws = await ipc.sessionUnarchive(id);
    await refreshProjects();
    if (open) { setPeek(null); setProjectId(ws.id); setTaskId(id); setPage(""); }
    else setPeek((p) => (p?.tasks.some((t) => t.id === id) ? null : p));
  };
  const deleteArchived = async (id: string) => {
    await ipc.sessionDelete(id);
    if (taskId === id) setTaskId(null);
    setPeek((p) => (p?.tasks.some((t) => t.id === id) ? null : p));
    setRunsByTask((prev) => { const next = { ...prev }; delete next[id]; return next; });
  };
  const leftActions: LeftActions = {
    onRenameProject: (id, name) => act(async () => { await ipc.workspaceUpdate(id, { name }); await refreshProjects(); }),
    onProjectSettings: (id) => { setProjectId(id); setPage("project"); },
    onRevealProject: (id) => act(() => ipc.workspaceReveal(id)),
    onRelocateProject: relocateProject,
    onRemoveProject: removeProject,
    onRenameTask: (_pid, id, title) => act(async () => { await ipc.sessionRename(id, title); await refreshProjects(); }),
    onArchiveTask: (_pid, id) => act(async () => {
      await ipc.sessionArchive(id);
      if (taskId === id) setTaskId(null);
      await refreshProjects();
      say("归档了：记录都在，设置 → ARCHIVE 里看、拿回来");
    }),
    onArchiveProjectTasks: archiveProjectTasks,
    onDeleteTask: (_pid, id) => act(async () => {
      await ipc.sessionDelete(id);
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
  const selectTask = (pid: string, id: string | null) => { setPeek(null); setProjectId(pid); setTaskId(id); setPage(""); };

  // 键盘（v0.8）：⌘N 新任务、⌘. 停、⌘, 设置、⌘1–5 右栏页签、⌘L 到输入框、⌘[ ⌘] 上一个 / 下一个任务、Esc 收回宽栏和设置页。
  // ⌘N、⌘[ ⌘] 只认左栏里的项目（看归档的任务时不在归档里翻）。
  const keysRef = useRef({ project: listed, taskId, page, wide, stop, selectTask, selectTab });
  keysRef.current = { project: listed, taskId, page, wide, stop, selectTask, selectTab };
  useEffect(() => {
    const TABS: RightTab[] = ["files", "log", "oneshot", "preview", "term"];
    const onKey = (e: KeyboardEvent) => {
      const k = keysRef.current;
      if (e.defaultPrevented || e.isComposing || e.keyCode === 229) return;
      if (e.key === "Escape") {
        // 菜单、弹层、确认框自己处理 Esc。
        if (document.querySelector(".pk-menu, .info-pop, .ctx-menu, [role=dialog]")) return;
        if (k.page) { setPage(""); e.preventDefault(); }
        else if (k.wide) { setWide(false); e.preventDefault(); }
        return;
      }
      if (!e.metaKey || e.ctrlKey || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "n" && !e.shiftKey && k.project) { e.preventDefault(); k.selectTask(k.project.workspace.id, null); }
      else if (key === "." && !e.shiftKey) { e.preventDefault(); void k.stop(); }
      else if (key === "," && !e.shiftKey) { e.preventDefault(); setSettingsAt(undefined); setPage((p) => (p === "settings" ? "" : "settings")); }
      else if (key === "l" && !e.shiftKey) { e.preventDefault(); setPage(""); window.setTimeout(() => document.querySelector<HTMLElement>(".md-editor .cm-content")?.focus(), 0); }
      else if (/^[1-5]$/.test(key) && !e.shiftKey) { e.preventDefault(); setPage(""); k.selectTab(TABS[Number(key) - 1]); }
      else if ((key === "[" || key === "]") && k.project) {
        const list = k.project.tasks;
        if (!list.length) return;
        e.preventDefault();
        const at = list.findIndex((t) => t.id === k.taskId);
        const next = key === "]" ? (at < 0 ? 0 : Math.min(list.length - 1, at + 1)) : (at < 0 ? 0 : Math.max(0, at - 1));
        k.selectTask(k.project.workspace.id, list[next].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!ready) return <div className="pk pk-dots app" />;

  const native = !isMockMode();
  const schedule = { list: schedules ?? [], entries, onGo: goSchedule, onRemove: (id: string) => act(async () => { await ipc.scheduleDelete(id); refreshSchedules(); say("定时任务取消了"); }) };
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
          all={!project}
          folded={!!settings.fold.projects}
          onFold={(projectsFolded) => updateSettings({ fold: { ...settingsRef.current.fold, projects: projectsFolded } })}
          agents={
            <AgentsPanel
              entries={entries}
              value={chosenEntry(entries, settings)}
              settings={settings}
              onSettings={updateSettings}
              disabled={busy}
              onOpenSettings={(section) => { setSettingsAt(section); setPage("settings"); }}
              grow={!project || !!settings.fold.projects}
              solo={!project}
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
          schedules={schedule}
        />
        {page === "project" && project ? (
          <ProjectPage
            key={project.workspace.id}
            project={project}
            entries={entries}
            native={native}
            onChanged={refreshProjects}
            onRelocate={relocateProject}
            onReveal={(id) => act(() => ipc.workspaceReveal(id))}
            onRemove={removeProject}
            onArchiveAll={archiveProjectTasks}
            presets={settings.presets}
            onClose={() => setPage("")}
            say={say}
          />
        ) : page === "settings" ? (
          <SettingsPage
            key={settingsAt ?? "-"}
            initial={settingsAt}
            entries={entries}
            onOpenArchived={openArchived}
            onRestoreArchived={(id) => restoreArchived(id, false)}
            onDeleteArchived={deleteArchived}
            settings={settings}
            onSettings={updateSettings}
            onRefreshAgents={refreshEntries}
            apiStatus={apiStatus}
            onRefreshApi={async () => { const st = await refreshApi(); void refreshEntries(); return st; }}
            onClose={() => { setPage(""); setSettingsAt(undefined); }}
            say={say}
          />
        ) : page === "quota" ? (
          <UsagePage onClose={() => setPage("")} say={say} entries={entries} costCap={settings.costCap} />
        ) : (
          <>
            {!wide && !project && (
              <ProjectsPage inbox={inbox} projects={projects} taskId={taskId} busyProjects={busyProjects} onSelectProject={selectProject} onSelectTask={selectTask} onOpen={openProject} native={native} actions={leftActions} schedules={schedule} />
            )}
            {!wide && project && (
              <MiddleColumn
                project={project}
                task={task}
                runs={runs}
                busy={busy}
                settings={settings}
                entries={entries}
                agentEvents={agentEvents}
                runningRunId={runningRunId}
                onControl={control}
                say={say}
                draft={draft}
                onDraft={setDraft}
                onSend={send}
                onAppendEvent={(runId, event) => {
                  setAgentEvents((prev) => {
                    const list = prev[runId] ?? [];
                    // 事件流里已经来过（跑着时批的 Skill）就不再放；序号比最后一条小的按序号插回去。
                    if (list.some((e) => e.seq === event.seq)) return prev;
                    return { ...prev, [runId]: [...list, event].sort((a, b) => a.seq - b.seq) };
                  });
                  // 批了 Skill：左栏 NOW 的「等你」跟着变。
                  if (event.type === "skill.reviewed") void refreshProjects();
                }}
                onStop={stop}
                onRetryStep={retryStep}
                onOpenSettings={() => { setSettingsAt(undefined); setPage("settings"); }}
                onShowAgents={() => { if (settingsRef.current.fold.agents) updateSettings({ fold: { ...settingsRef.current.fold, agents: false } }); setAgentsFlash((n) => n + 1); }}
                onRelocate={relocateProject}
                onRemoveProject={removeProject}
                onRestoreTask={(id) => act(async () => { await restoreArchived(id, true); say("拿回左栏了，可以接着发"); })}
                quota={hint ? {
                  hint,
                  swapped: swapOnce && swapOnce.from === hint.from ? entries.find((e) => e.id === swapOnce.to) ?? null : null,
                  presetName: settings.presets.find((p) => p.id === settings.preset)?.name ?? null,
                  onOnce: () => { if (hint.to) setSwapOnce({ from: hint.from, to: hint.to.id }); },
                  onAlways: () => { if (hint.to) { updateSettings(swapPatch(settingsRef.current, hint.from, hint.to.id)); setSwapOnce(null); say(`以后用 ${hint.to.label} 代替 ${hint.fromLabel}`); } },
                  onUndo: () => setSwapOnce(null),
                  onDismiss: () => { setQuotaDismissed(`${hint.from}|${hint.text}`); setSwapOnce(null); },
                } : null}
                onNewTask={() => { if (project) selectTask(project.workspace.id, null); }}
                shownRunId={run?.id ?? null}
                onShowRun={(id, path) => { setShownRunId(id); setFilePath(path ?? null); setTab(path ? "files" : "log"); }}
                onRunUpdated={(record) => { setRunsByTask((prev) => ({ ...prev, [record.sessionId]: (prev[record.sessionId] ?? []).map((r) => (r.id === record.id ? { ...r, ...record } : r)) })); void refreshProjects(); }}
                missed={(schedules ?? []).filter((x) => x.workspaceId === project.workspace.id && !!x.missedAt)}
                onMissed={onMissed}
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
              term={term}
              onTermRun={termRun}
              onTermStop={termStop}
              onTermClear={termClear}
              say={say}
              entries={entries}
              settings={settings}
              onSettings={updateSettings}
              onToTask={(text) => setDraft((d) => (d.trim() ? `${d.trimEnd()}\n\n${text}` : text))}
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
