// 统一事件 → 界面上的样子。 Peck Loop 和 CLI 引擎同一套：
// - 回答（Markdown，边出字边显示）；
// - 连续的工具调用和思考折叠成一组，头上一行英文（v1.0.1 学 Claude Code：「Read 4 files, edited 2 files, ran 1 command · 38s」），
//   在跑的那一步露出来；思考不算一步：在想时那一组下面一行「Thinking… 3s」，想完就不显示，也不打断上下的步骤；
// - 没批、没答的审批和 ASK 单独拿出来做卡片；任务清单取最新的一份；
// - 花费：这一次的进、出、缓存命中、美元（CLI 估的标「估」，不知道的写「未知」，不当 0）。
// - v0.6：上下文多大（输入框旁的小环）、自动压缩的每一步、后台命令、模型想记住的几条。
import type { AgentEvent, ApprovalDecision, BackgroundKind, BackgroundStatus, CompactStep, ContextPart, Cost, CostSource, CriterionCheck, FileDiff, PlanStep, ResolvedBy, ReviewVerdict, SkillReviewStatus, SkillScope, StageRole, TokenUsage, ToolKind, ToolStatus, TurnEndReason } from "@/lib/agentEvents";

export interface ToolView {
  callId: string;
  name: string;
  kind: ToolKind;
  title: string;
  input: unknown;
  status: "running" | ToolStatus;
  /** 跑的时候一段段来的输出（最多留 8KB 的尾巴）。 */
  live: string;
  output?: string | null;
  exitCode?: number | null;
  diff: FileDiff[];
  startedAt: number;
  endedAt?: number;
  durationMs?: number | null;
  approval?: { title: string; decision?: ApprovalDecision; by?: ResolvedBy; highRisk: boolean };
  subagentId?: string;
  /** 这一步交出去的子任务（委派、Claude Code 的 Task）：它的那几步挂在这一步下面（v0.9 B1）。 */
  sub?: { title: string; model?: string | null; calls: ToolView[]; status?: ToolStatus };
}

export interface ApprovalView {
  requestId: string;
  callId?: string | null;
  kind: ToolKind;
  title: string;
  detail?: string | null;
  highRisk: boolean;
  options: ApprovalDecision[];
  decision?: ApprovalDecision;
  by?: ResolvedBy;
}

export interface AskView {
  requestId: string;
  question: string;
  options: string[];
  freeText: boolean;
  answer?: string;
}

export interface ContextView {
  used: number;
  window: number | null;
  compactAt: number | null;
  parts: ContextPart[];
}

export interface BackgroundView {
  id: string;
  name: string;
  command: string;
  kind: BackgroundKind;
  status: BackgroundStatus;
  exitCode?: number | null;
  /** 任务结束了：这里只记到起的那一刻（dev server 跨任务在跑，现状看 DEV），不再转圈。 */
  past?: boolean;
}

/** 接力、审查里的一段（v0.8）：头上一行写谁做哪一段，收尾写怎么结束的、审查的结论。 */
export interface StageView {
  stage: StageRole;
  engine: string;
  title: string;
  startedAt: number;
  endedAt?: number;
  reason?: TurnEndReason;
  error?: string | null;
  verdict?: ReviewVerdict | null;
  /** 审查按验收标准逐条判的结果（v0.9）。 */
  checks?: CriterionCheck[];
  /** 写规范那一段的回答、工具调用和思考（v1.0.3）：收在这一段的头里，点开看规范全文和读过的文件，不进主线。 */
  items?: ViewItem[];
}

/** 一段思考（只用来显示「Thinking… 3s」，想完不显示）。 */
export interface ThoughtView { text: string; done: boolean; startedAt: number; endedAt?: number }

export type ViewItem =
  | { kind: "text"; id: string; text: string; done: boolean; stage?: StageRole }
  | { kind: "stage"; id: string; info: StageView }
  /** 连着的工具调用和思考（v1.0.1：思考并进来，不再单独一行）。 */
  | { kind: "tools"; id: string; calls: ToolView[]; thoughts: ThoughtView[] }
  | { kind: "asked"; id: string; ask: AskView }
  | { kind: "approved"; id: string; approval: ApprovalView }
  | { kind: "notice"; id: string; level: "info" | "warn"; text: string }
  | { kind: "error"; id: string; text: string }
  | { kind: "compacted"; id: string; step: CompactStep; text: string; before: number; after: number; summary?: string | null }
  /** 一条后台命令或留着的 dev server（最新的状态）。 */
  | { kind: "background"; id: string; bg: BackgroundView }
  /** 模型提议存一条 Skill（v0.10 C3）：不打断，卡片留在提的地方，跑着、跑完都能批。 */
  | { kind: "skill"; id: string; skill: SkillProposalView };

/** 一条 Skill 提议和批成什么样了（同一条可能先 queued 再 saved，取最后一条）。 */
export interface SkillProposalView {
  proposalId: string;
  name: string;
  description: string;
  body: string;
  scope: SkillScope;
  location: string;
  diff: string | null;
  warnings: string[];
  similar: string[];
  reviewed: { status: SkillReviewStatus; name: string; location: string; error: string | null } | null;
}

export interface UsageLine {
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
  /** 联网搜索了几次（v1.1.1）。 */
  webSearches: number | null;
  usd: number | null;
  source: CostSource;
  /** 有过请求报不出用量。 */
  partial: boolean;
}

export interface AgentView {
  engine: string | null;
  model: string | null;
  items: ViewItem[];
  /** 还没批的审批（卡片）。 */
  approvals: ApprovalView[];
  /** 还没答的提问（ASK 卡片）。 */
  asks: AskView[];
  plan: PlanStep[] | null;
  /** 这一回合改了哪些文件（diff.updated 的最新一份；没有就从工具调用里收）。 */
  files: FileDiff[];
  usage: UsageLine | null;
  ended: { reason: string; error?: string | null } | null;
  sessionRef: string | null;
  /** 用量是 CLI 报的累计口径（续接以后接着上一次累计）：算整个任务时按会话取差。 */
  cumulativeUsage?: boolean;
  /** 最新的一份上下文大小。 */
  context: ContextView | null;
  /** 模型想记住的几条；reviewed 是站主批过的结果。 */
  memory: { notes: string[]; reviewed: { saved: string[]; skipped: string[] } | null } | null;
  /** 这个任务里的后台命令（最新的状态）。 */
  background: BackgroundView[];
  /** 接力、审查的几段（v0.8；一个引擎干的任务为空）。 */
  stages: StageView[];
  /** 插件的收尾 Hook 跑完的结果（v1.1.0，写在 recap 里）：提交了哪一笔、或者为什么没提交。 */
  hooks: Array<{ ok: boolean; text: string }>;
}

const LIVE_TAIL = 8 * 1024;

/** 同一份事件数组算过就记着（数组只换不改）：流式时一次刷新里中栏、过程、花费各要一遍，做完的那几次每次刷新也都要。
 *  结果是共用的，调用的地方只读。 */
const folded = new WeakMap<AgentEvent[], [live?: AgentView, stopped?: AgentView]>();
export function foldEvents(events: AgentEvent[], stopped = false): AgentView {
  let slot = folded.get(events);
  if (!slot) folded.set(events, (slot = []));
  return (slot[stopped ? 1 : 0] ??= foldFresh(events, stopped));
}

function foldFresh(events: AgentEvent[], stopped: boolean): AgentView {
  const view: AgentView = { engine: null, model: null, items: [], approvals: [], asks: [], plan: null, files: [], usage: null, ended: null, sessionRef: null, context: null, memory: null, background: [], stages: [], hooks: [] };
  let stageNow: StageView | null = null;
  const bgs = new Map<string, BackgroundView>();
  const tools = new Map<string, ToolView>();
  const texts = new Map<string, Extract<ViewItem, { kind: "text" }>>();
  const thoughts = new Map<string, ThoughtView>();
  // 开始动手（调工具）或开口说话了，前面没收尾的思考就算想完了（有的引擎不发 reasoning.completed）。
  const thoughtsDone = (at: number) => { for (const t of thoughts.values()) if (!t.done) { t.done = true; t.endedAt = at; } };
  const approvals = new Map<string, ApprovalView>();
  const asks = new Map<string, AskView>();
  const skills = new Map<string, SkillProposalView>();
  const subStreamed = new Set<string>();
  // CLI 经本机 MCP 委派的子任务（v0.9 B1）：Peck 不知道 CLI 那一步的编号，挂到最近一个在跑的 delegate_task 那一步。
  const alias = new Map<string, string>();
  const parentOf = (sid: string) => tools.get(alias.get(sid) ?? sid);
  let diffFiles: FileDiff[] | null = null;
  // 用量：一回合一次（turn）的加起来；累计（cumulative）的取最后一条。
  let turnUsage: UsageLine | null = null;
  let cumulative: UsageLine | null = null;
  // 写规范那一段的话、工具调用、思考收进那一段（对话里只露实施那一个的输出）；卡片、提醒、出错照旧在主线。
  const box = (): ViewItem[] => (stageNow?.items ?? view.items);
  const lastGroup = (): Extract<ViewItem, { kind: "tools" }> | null => {
    const items = box();
    const last = items[items.length - 1];
    return last && last.kind === "tools" ? last : null;
  };
  for (const e of events) {
    switch (e.type) {
      case "session.started":
        if (e.subagentId) break;
        view.engine = e.engine;
        view.model = e.model ?? null;
        view.sessionRef = e.sessionRef ?? view.sessionRef;
        break;
      case "message.delta":
      case "message.completed": {
        // 子任务（Claude Code 的 Task）说的话不是主回答：放进那个子任务那一步的输出里。
        if (e.subagentId) {
          const parent = parentOf(e.subagentId);
          if (parent) {
            if (e.type === "message.delta") { parent.live += e.text; subStreamed.add(e.itemId); }
            else if (!subStreamed.has(e.itemId)) parent.live += `${e.text}\n`;
            else parent.live += "\n";
          }
          break;
        }
        thoughtsDone(e.at);
        // 每一段的引擎各自从 m1 编号：同一个号在不同段里是两段话。
        const key = stageNow ? `${view.stages.length}:${e.itemId}` : e.itemId;
        let item = texts.get(key);
        if (!item) {
          item = { kind: "text", id: `t-${key}-${e.seq}`, text: "", done: false, stage: stageNow?.stage };
          texts.set(key, item);
          box().push(item);
        }
        if (e.type === "message.delta") item.text += e.text;
        else { item.text = e.text; item.done = true; }
        break;
      }
      case "stage.started": {
        // 上一段的话收尾（它的引擎不会再说了）。
        for (const t of texts.values()) t.done = true;
        stageNow = { stage: e.stage, engine: e.engine, title: e.title, startedAt: e.at, items: e.stage === "spec" ? [] : undefined };
        view.stages.push(stageNow);
        view.items.push({ kind: "stage", id: `s-${e.seq}`, info: stageNow });
        break;
      }
      case "stage.completed": {
        const st = [...view.stages].reverse().find((x) => x.stage === e.stage && x.endedAt == null);
        if (st) { st.endedAt = e.at; st.reason = e.reason; st.error = e.error; st.verdict = e.verdict; st.checks = e.checks ?? []; }
        for (const call of tools.values()) {
          if (call.status === "running") { call.status = e.reason === "done" ? "ok" : "cancelled"; call.endedAt = e.at; }
        }
        for (const t of texts.values()) t.done = true;
        stageNow = null;
        break;
      }
      case "reasoning.delta":
      case "reasoning.completed": {
        if (e.subagentId) break;
        const rkey = stageNow ? `${view.stages.length}:${e.itemId}` : e.itemId;
        let item = thoughts.get(rkey);
        if (!item) {
          if (e.type === "reasoning.completed" && !e.text.trim()) break;
          item = { text: "", done: false, startedAt: e.at };
          thoughts.set(rkey, item);
          // 并进前面那一组工具调用；前面不是就新开一组（后面的工具调用接着进这一组）。
          const group = lastGroup();
          if (group) group.thoughts.push(item);
          else box().push({ kind: "tools", id: `g-r-${rkey}-${e.seq}`, calls: [], thoughts: [item] });
        }
        if (e.type === "reasoning.delta") item.text += e.text;
        else { item.text = e.text || item.text; item.done = true; item.endedAt = e.at; }
        break;
      }
      case "tool_call.started": {
        const call: ToolView = { callId: e.callId, name: e.name, kind: e.kind, title: e.title, input: e.input, status: "running", live: "", diff: [], startedAt: e.at, subagentId: e.subagentId };
        tools.set(e.callId, call);
        // 子任务的步骤挂在交出去的那一步下面（找不到那一步就照旧排在组里）。
        const parent = e.subagentId ? parentOf(e.subagentId) : undefined;
        if (parent && parent !== call) (parent.sub ??= { title: "Subagent", calls: [] }).calls.push(call);
        else {
          thoughtsDone(e.at);
          const group = lastGroup();
          if (group) group.calls.push(call);
          else box().push({ kind: "tools", id: `g-${e.callId}-${e.seq}`, calls: [call], thoughts: [] });
        }
        const pending = [...approvals.values()].find((a) => a.callId === e.callId);
        if (pending) call.approval = { title: pending.title, decision: pending.decision, by: pending.by, highRisk: pending.highRisk };
        break;
      }
      case "tool_call.output_delta": {
        const call = tools.get(e.callId);
        if (call) call.live = (call.live + e.text).slice(-LIVE_TAIL);
        break;
      }
      case "tool_call.completed": {
        const call = tools.get(e.callId);
        if (!call) break;
        call.status = e.status;
        call.output = e.output;
        call.exitCode = e.exitCode;
        call.diff = e.diff ?? [];
        call.endedAt = e.at;
        call.durationMs = e.durationMs ?? e.at - call.startedAt;
        break;
      }
      case "diff.updated":
        if (e.subagentId) break;
        diffFiles = e.files;
        break;
      case "plan.updated":
        view.plan = e.steps;
        break;
      case "approval.requested": {
        const a: ApprovalView = { requestId: e.requestId, callId: e.callId, kind: e.kind, title: e.title, detail: e.detail, highRisk: e.highRisk, options: e.options };
        approvals.set(e.requestId, a);
        const call = e.callId ? tools.get(e.callId) : undefined;
        if (call) call.approval = { title: e.title, highRisk: e.highRisk };
        break;
      }
      case "approval.resolved": {
        const a = approvals.get(e.requestId);
        if (!a) break;
        a.decision = e.decision;
        a.by = e.by;
        const call = a.callId ? tools.get(a.callId) : undefined;
        if (call) call.approval = { title: a.title, decision: e.decision, by: e.by, highRisk: a.highRisk };
        else view.items.push({ kind: "approved", id: `a-${e.requestId}`, approval: a });
        break;
      }
      case "user_input.requested":
        asks.set(e.requestId, { requestId: e.requestId, question: e.question, options: e.options ?? [], freeText: e.freeText ?? true });
        break;
      case "user_input.resolved": {
        const a = asks.get(e.requestId);
        if (!a) break;
        a.answer = e.answer;
        view.items.push({ kind: "asked", id: `q-${e.requestId}`, ask: a });
        break;
      }
      case "usage.updated": {
        const line = usageLine(e.usage, e.cost);
        if (e.scope === "cumulative") cumulative = line;
        else turnUsage = turnUsage ? addUsage(turnUsage, line) : line;
        break;
      }
      case "notice":
        view.items.push({ kind: "notice", id: `n-${e.seq}`, level: e.level, text: e.text });
        break;
      case "context.updated":
        if (e.subagentId) break;
        view.context = { used: e.used, window: e.window ?? null, compactAt: e.compactAt ?? null, parts: e.parts ?? [] };
        break;
      case "context.compacted":
        box().push({ kind: "compacted", id: `c-${e.seq}`, step: e.step, text: e.text, before: e.before, after: e.after, summary: e.summary });
        break;
      case "memory.proposed": {
        const m = (view.memory ??= { notes: [], reviewed: null });
        for (const n of e.notes) if (!m.notes.includes(n)) m.notes.push(n);
        break;
      }
      case "memory.reviewed":
        view.memory = { notes: view.memory?.notes ?? [], reviewed: { saved: e.saved, skipped: e.skipped ?? [] } };
        break;
      case "skill.proposed": {
        if (skills.has(e.proposalId)) break;
        const skill: SkillProposalView = { proposalId: e.proposalId, name: e.name, description: e.description, body: e.body, scope: e.scope, location: e.location, diff: e.diff ?? null, warnings: e.warnings ?? [], similar: e.similar ?? [], reviewed: null };
        skills.set(e.proposalId, skill);
        view.items.push({ kind: "skill", id: `s-${e.proposalId}`, skill });
        break;
      }
      case "hook.ran":
        view.hooks.push({ ok: e.ok, text: e.text });
        break;
      case "skill.reviewed": {
        const skill = skills.get(e.proposalId);
        if (skill) skill.reviewed = { status: e.status, name: e.name, location: e.location, error: e.error ?? null };
        break;
      }
      case "background.updated": {
        const known = bgs.get(e.id);
        const bg: BackgroundView = { id: e.id, name: e.name, command: e.command, kind: e.kind, status: e.status, exitCode: e.exitCode };
        bgs.set(e.id, bg);
        if (known) {
          const item = view.items.find((i) => i.kind === "background" && i.bg.id === e.id);
          if (item && item.kind === "background") item.bg = bg;
        } else view.items.push({ kind: "background", id: `b-${e.id}`, bg });
        break;
      }
      case "error":
        // 不致命的（Codex 自己会重试的那种）只是一行提醒，不画成红条。
        if (e.fatal === false) view.items.push({ kind: "notice", id: `e-${e.seq}`, level: "warn", text: e.message });
        else view.items.push({ kind: "error", id: `e-${e.seq}`, text: e.message });
        break;
      case "subagent.started": {
        // 交出去的那一步：Peck Loop 的委派、Claude Code 的 Task 编号和那一步一样；CLI 经本机 MCP 委派的对不上，
        // 挂到最近一个还在跑、名字里有 delegate_task 的那一步。
        let parent = tools.get(e.subagentId);
        if (!parent) {
          parent = [...tools.values()].reverse().find((c) => c.status === "running" && /delegate_task/.test(c.name) && !c.sub?.calls.length && ![...alias.values()].includes(c.callId));
          if (parent) alias.set(e.subagentId, parent.callId);
        }
        if (parent) parent.sub = { title: e.title, model: e.model ?? null, calls: parent.sub?.calls ?? [] };
        break;
      }
      case "subagent.completed": {
        const parent = parentOf(e.subagentId);
        if (parent?.sub) parent.sub.status = e.status;
        break;
      }
      case "turn.started":
        // 又开了一轮（Claude Code 的后台子代理做完、把主线叫醒，v0.9）：上一轮的「结束了」不算数。
        if (!e.subagentId) view.ended = null;
        break;
      case "turn.completed":
        if (e.subagentId) break;
        view.ended = { reason: e.reason, error: e.error };
        // 回合结束了还在「在跑」的工具，按结束原因收掉。
        for (const call of tools.values()) {
          if (call.status === "running") { call.status = e.reason === "done" ? "ok" : "cancelled"; call.endedAt = e.at; }
        }
        for (const t of texts.values()) t.done = true;
        for (const t of thoughts.values()) { if (!t.done) t.endedAt = e.at; t.done = true; }
        // 回合结束时还没批、没答的作废：引擎已经不等了。不然 Claude Code 的后台子代理把主线叫醒（又一个 turn.started）时，
        // 这些旧卡片会冒出来、点了也送不到（v0.10 收尾审查）。
        for (const [id, a] of approvals) if (!a.decision) approvals.delete(id);
        for (const [id, q] of asks) if (q.answer == null) asks.delete(id);
        break;
      default:
        break;
    }
  }
  if (stopped && !view.ended && events.length > 0) {
    // 记录上已经结束、事件里却没有收尾：按「停下了」收掉，时间记到最后一条事件。
    const at = events[events.length - 1].at;
    view.ended = { reason: "interrupted", error: null };
    for (const call of tools.values()) if (call.status === "running") { call.status = "cancelled"; call.endedAt = at; }
    for (const t of texts.values()) t.done = true;
    for (const t of thoughts.values()) { if (!t.done) t.endedAt = at; t.done = true; }
  }
  view.approvals = view.ended ? [] : [...approvals.values()].filter((a) => !a.decision);
  view.asks = view.ended ? [] : [...asks.values()].filter((a) => a.answer == null);
  view.usage = cumulative ?? turnUsage;
  view.cumulativeUsage = cumulative != null;
  view.files = diffFiles ?? collectDiffs([...tools.values()]);
  view.background = [...bgs.values()];
  // 审查最后那一行结论（VERDICT: …）不显示，结论写在这一段的头上。
  // 逐条判的 `CHECK n: …` 也一样：清单画在头上（v0.9）。
  const checked = view.stages.some((s) => s.stage === "review" && (s.checks?.length ?? 0) > 0);
  for (const item of view.items) {
    if (item.kind !== "text" || item.stage !== "review") continue;
    item.text = item.text.replace(/^\s*[`*]*VERDICT:\s*\w+[`*]*\s*$/gim, "");
    if (checked) item.text = item.text.replace(/^\s*(?:[-*•]\s+)?[`*]*CHECK\s*#?\d+\s*[:：.].*$/gim, "").replace(/\n{3,}/g, "\n\n");
    item.text = item.text.trim();
  }
  if (view.ended) {
    // 模型起的后台命令跟着任务停了；留着的 dev server 以后的变化不进任务记录，只记到起过。
    for (const item of view.items) {
      if (item.kind !== "background" || item.bg.status !== "running") continue;
      item.bg = item.bg.kind === "server" ? { ...item.bg, past: true } : { ...item.bg, status: "stopped" };
    }
  }
  return view;
}

function usageLine(u: TokenUsage, c: Cost): UsageLine {
  // Peck 的 web_search 那一条只有搜索次数和花费（没有 token），也算报了。
  const known = u.input != null || u.output != null || u.webSearches != null;
  return {
    input: u.input ?? null,
    output: u.output ?? null,
    cacheRead: u.cacheRead ?? null,
    cacheWrite: u.cacheWrite ?? null,
    webSearches: u.webSearches ?? null,
    usd: c.usd ?? null,
    source: c.source,
    partial: !known,
  };
}

const plus = (a: number | null, b: number | null) => (a == null && b == null ? null : (a ?? 0) + (b ?? 0));

export function addUsage(a: UsageLine, b: UsageLine): UsageLine {
  return {
    input: plus(a.input, b.input),
    output: plus(a.output, b.output),
    cacheRead: plus(a.cacheRead, b.cacheRead),
    cacheWrite: plus(a.cacheWrite, b.cacheWrite),
    webSearches: plus(a.webSearches, b.webSearches),
    // 有一次算不出花费，总数就不准：标出来，但已知的照加。
    usd: plus(a.usd, b.usd),
    source: a.source === b.source ? a.source : a.source === "unknown" || b.source === "unknown" ? "unknown" : a.source === "estimated" || b.source === "estimated" ? "estimated" : a.source,
    partial: a.partial || b.partial || (a.usd == null) !== (b.usd == null),
  };
}

function collectDiffs(calls: ToolView[]): FileDiff[] {
  const byPath = new Map<string, FileDiff>();
  for (const c of calls) for (const d of c.diff) byPath.set(d.path, d);
  return [...byPath.values()];
}

// ---------- 一组的英文摘要、每一步的英文标题（v1.0.1，学 Claude Code） ----------

/** 这一步算哪一类（摘要里按类计数）。名字比 kind 细：读 Skill、截图、查旧任务、搜网页各算一类。 */
function activity(c: ToolView): string {
  const n = c.name.toLowerCase();
  if (n === "search_tasks") return "memory";
  if (/web_?search/.test(n) || c.title.startsWith("搜网页")) return "web";
  if (n === "load_skill") return "skill";
  if (n === "screenshot_app") return "shot";
  if (n === "list_directory") return "list";
  if (c.sub || /delegate_task$/.test(n) || n === "task" || n === "agent") return "agent";
  if (n === "update_plan" || /todo/.test(n)) return "plan";
  if (/ask_user$/.test(n)) return "ask";
  if (n === "memory") return "note";
  if (n === "create_skill") return "newskill";
  if (c.kind === "mcp" || c.kind === "other") return "tool";
  return c.kind;
}

const count = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const times = (base: string, n: number) => (n === 1 ? base : `${base} ${n} times`);

const PHRASE: Record<string, (n: number) => string> = {
  read: (n) => `read ${count(n, "file")}`,
  list: (n) => `listed ${count(n, "folder")}`,
  search: (n) => `searched for ${count(n, "pattern")}`,
  edit: (n) => `edited ${count(n, "file")}`,
  run: (n) => `ran ${count(n, "command")}`,
  fetch: (n) => `fetched ${count(n, "page")}`,
  web: (n) => times("searched the web", n),
  memory: (n) => times("recalled memory", n),
  skill: (n) => `used ${count(n, "skill")}`,
  shot: (n) => `took ${count(n, "screenshot")}`,
  agent: (n) => `ran ${count(n, "subagent")}`,
  plan: () => "updated the plan",
  ask: () => "asked you",
  note: () => "proposed memory",
  newskill: (n) => `proposed ${count(n, "skill")}`,
  tool: (n) => `used ${count(n, "tool")}`,
};

/** 一组工具调用折叠成一句：「Read 4 files, edited 2 files, ran 1 command」（按第一次出现的先后）。 */
export function groupSummary(calls: ToolView[]): string {
  const order: string[] = [];
  const counts = new Map<string, number>();
  const edited = new Set<string>();
  for (const c of calls) {
    const k = activity(c);
    if (!counts.has(k)) { order.push(k); counts.set(k, 0); }
    if (k === "edit") for (const p of c.diff.length ? c.diff.map((d) => d.path) : [c.callId]) edited.add(p);
    else counts.set(k, counts.get(k)! + 1);
  }
  if (edited.size) counts.set("edit", edited.size);
  const text = order.map((k) => (PHRASE[k] ?? PHRASE.tool)(counts.get(k)!)).join(", ");
  return text ? text[0].toUpperCase() + text.slice(1) : count(calls.length, "step");
}

/** 一组从第一步（或第一段思考）开始到最后一步结束用了多久；有在跑的算到现在。 */
export function groupSpan(calls: ToolView[], now: number, thoughts: ThoughtView[] = []): number {
  const marks = [...calls.map((c) => c.startedAt), ...thoughts.map((t) => t.startedAt)];
  if (!marks.length) return 0;
  const start = Math.min(...marks);
  const live = calls.some((c) => c.status === "running") || thoughts.some((t) => !t.done);
  const end = live ? now : Math.max(...calls.map((c) => c.endedAt ?? c.startedAt), ...thoughts.map((t) => t.endedAt ?? t.startedAt));
  return Math.max(0, end - start);
}

// 各引擎给的步骤标题是中文、一个字的动词打头（Peck Loop 在 harness/tools、CLI 在 gateway/convert）。
// 在这里翻成英文，旧记录也一样；认不出的照原样。
const TITLE_RULES: Array<[RegExp, string | ((...m: string[]) => string)]> = [
  [/^后台跑 (.*)$/s, "Run in background $1"],
  [/^停后台命令 ?(.*)$/s, "Stop background command $1"],
  [/^看后台命令 ?(.*)$/s, "Check background command $1"],
  [/^看任务快照$/, "List task snapshots"],
  [/^回到任务 (\d+) 开始前$/, "Restore to before task $1"],
  [/^回到任务 (\d+) 结束后$/, "Restore to after task $1"],
  [/^设定时任务[：:]\s*(.*)$/s, "Schedule: $1"],
  [/^看定时任务$/, "List schedules"],
  [/^取消定时任务 ?(.*)$/s, "Cancel schedule $1"],
  [/^看任务 (\d+)$/, "Recall task $1"],
  [/^搜旧任务「(.*)」$/s, "Recall past tasks “$1”"],
  [/^看最近的任务$/, "Recall recent tasks"],
  [/^看 项目根目录$/, "List project root"],
  [/^看 (.*)$/s, "List $1"],
  [/^读 Skill (\S+) 的 (.*)$/s, "Read skill $1 · $2"],
  [/^读 Skill (.*)$/s, "Read skill $1"],
  [/^读 (.*)$/s, "Read $1"],
  [/^搜网页 (.*)$/s, "Search the web: $1"],
  [/^搜网页$/, "Search the web"],
  [/^供应商那边跑 (.*)$/s, "Run $1 on the provider"],
  [/^取网页 (.*)$/s, "Fetch $1"],
  [/^取 (.*)$/s, "Fetch $1"],
  [/^搜「(.*)」$/s, "Search “$1”"],
  [/^搜 (.*)$/s, "Search $1"],
  [/^找文件 (.*)$/s, "Find files $1"],
  [/^找 (.*)$/s, "Find $1"],
  [/^写 (.*)$/s, "Write $1"],
  [/^改 (.*) 等 (\d+) 个文件$/s, (_m, p, n) => `Edit ${p} and ${Number(n) - 1} more`],
  [/^改 (.*)（(\d+) 处）$/s, "Edit $1 ($2 edits)"],
  [/^改文件$/, "Edit files"],
  [/^改 (.*)$/s, "Edit $1"],
  [/^打补丁$/, "Apply patch"],
  [/^跑 (.*)$/s, "Run $1"],
  [/^截 (.*) 的窗口$/s, "Screenshot $1"],
  [/^更新任务清单$/, "Update plan"],
  [/^交给子任务[：:]\s*(.*)$/s, "Delegate: $1"],
  [/^交给子任务$/, "Delegate"],
  [/^子任务 (.*)$/s, "Subagent: $1"],
  [/^子任务$/, "Subagent"],
  [/^(问你|问站主)$/, "Ask you"],
  [/^想记住几条$/, "Propose memory"],
  [/^想存成 Skill (.*)$/s, "Propose skill $1"],
  [/^提交：(.*)$/s, "Commit $1"],
  [/^用 MCP (.*)$/s, "MCP $1"],
  [/^列出文件$/, "List files"],
  [/^想一想$/, "Think"],
  [/^用 (.*)$/s, "Use $1"],
];

/** 步骤标题翻成英文（「跑 pnpm test」→「Run pnpm test」）。 */
export function englishTitle(title: string): string {
  for (const [re, to] of TITLE_RULES) {
    const m = title.match(re);
    if (!m) continue;
    const out = typeof to === "string" ? title.replace(re, to) : to(...m);
    return out.replace(/、/g, ", ").replace(/（/g, " (").replace(/）/g, ")");
  }
  return title;
}

/** 一段里读过的文件（按先后、去重）：读文件那几步的标题是「读 a.ts」，Codex 一步读几个用「、」连着；读 Skill 不算。 */
export function readFiles(items: ViewItem[]): string[] {
  const out: string[] = [];
  for (const item of items) {
    if (item.kind !== "tools") continue;
    for (const c of item.calls) {
      const m = c.kind === "read" ? c.title.match(/^读 (?!Skill )(.+)$/s) : null;
      for (const p of m ? m[1].split("、") : []) {
        const path = p.trim();
        if (path && !out.includes(path)) out.push(path);
      }
    }
  }
  return out;
}

/** 落盘的完整输出路径（bash_run 的结果里写着「完整的存在 …」）。 */
export function spillPath(output?: string | null): string | null {
  const m = output?.match(/完整的存在 (\S+?\.log)/);
  return m ? m[1] : null;
}

const fmt = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(n >= 100_000 ? 0 : 1)}k` : n.toLocaleString());

/** 回答末尾模型建议的下一步（每行 `→ ` 打头，最多 3 条）：拆成正文和下一步。多出来的留在正文里。 */
export function splitNextSteps(text: string): { body: string; steps: string[] } {
  const lines = text.replace(/\s+$/, "").split("\n");
  const steps: string[] = [];
  while (lines.length && steps.length < 3 && /^\s*(→|->)\s+\S/.test(lines[lines.length - 1])) {
    steps.unshift(lines.pop()!.replace(/^\s*(→|->)\s+/, "").trim());
  }
  return { body: lines.join("\n").replace(/\s+$/, ""), steps };
}

/** 还有没有等站主批、等站主回答的（只看这几种事件，不整个折一遍；左栏 NOW 每批事件都要算）。
 *  没批的 Skill 提议（v0.10 C3）也算，任务跑完了也还等着。 */
export function waitingOnYou(events: AgentEvent[]): boolean {
  const open = new Set<string>();
  const skills = new Set<string>();
  for (const e of events) {
    if (e.type === "approval.requested" || e.type === "user_input.requested") open.add(e.requestId);
    else if (e.type === "approval.resolved" || e.type === "user_input.resolved") open.delete(e.requestId);
    else if (e.type === "turn.completed") open.clear();
    else if (e.type === "skill.proposed") skills.add(e.proposalId);
    else if (e.type === "skill.reviewed" && e.status !== "failed") skills.delete(e.proposalId);
  }
  return open.size > 0 || skills.size > 0;
}

/** Token 数：「12.3k」「640」「1M」。 */
export const tokens = (n: number): string => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(2)}M` : fmt(n));

/** 上下文的几块叫什么。 */
export const CONTEXT_PARTS: Record<string, string> = {
  system: "Peck 的说明和工具",
  project: "项目说明和记忆",
  conversation: "对话和思考",
  tools: "工具的输出",
};

/** 自动压缩三步叫什么。 */
export const COMPACT_STEP: Record<string, string> = { spill: "落盘", clear: "清旧输出", summary: "交接摘要" };

/** 「$0.0123」「< $0.0001」；不知道是 null。 */
export function usd(v: number | null | undefined): string | null {
  if (v == null) return null;
  if (v === 0) return "$0";
  if (v < 0.0001) return "< $0.0001";
  if (v < 0.01) return `$${v.toFixed(4)}`;
  if (v < 1) return `$${v.toFixed(3)}`;
  return `$${v.toFixed(2)}`;
}

/** 一行花费：「进 12.3k · 出 640 · 缓存命中 8.1k · 搜 2 次 · $0.0042」；CLI 估的「估 $0.03」。 */
export function usageText(u: UsageLine): string {
  const bits: string[] = [];
  if (u.input != null) bits.push(`进 ${fmt(u.input + (u.cacheRead ?? 0) + (u.cacheWrite ?? 0))}`);
  if (u.output != null) bits.push(`出 ${fmt(u.output)}`);
  if (u.cacheRead) bits.push(`缓存命中 ${fmt(u.cacheRead)}`);
  if (u.webSearches) bits.push(`搜 ${u.webSearches} 次`);
  const money = usd(u.usd);
  if (money == null) bits.push("花费未知");
  else bits.push(`${u.source === "estimated" ? "估 " : ""}${money}${u.partial ? "（有一部分算不出）" : ""}`);
  return bits.join(" · ");
}

// ---------- 按段列花费（v0.9 Phase 14 B4） ----------

/** 一段（或委派出去的一个子任务）花的。 */
export interface CostRow {
  id: string;
  /** 合作里的哪一段；`main` 是没分段的任务的主线，`sub` 是委派的子任务。 */
  role: StageRole | "main" | "sub";
  title: string;
  /** 引擎 id：`peck` 是 Peck Loop（API 按量）；别的是 CLI（订阅）。 */
  engine: string;
  model: string | null;
  usage: UsageLine | null;
  startedAt: number;
  endedAt: number | null;
}

/** 合作的几段、委派的子任务各花了多少。只有一段（没合作、没委派）返回空。
 *  累计口径（Claude Code 续接）按段取差：按审查改的那一段续接了实施的会话，减掉实施那段报到的。 */
export function costRows(events: AgentEvent[], mainTitle: string): CostRow[] {
  const rows: CostRow[] = [];
  const subs = new Map<string, CostRow>();
  const turn = new Map<CostRow, UsageLine>();
  const cum = new Map<CostRow, { first: UsageLine; last: UsageLine; base: UsageLine | null }>();
  const lastCum = new Map<string, UsageLine>();
  let cur: CostRow | null = null;
  // 放在对象里：在闭包里赋值，TS 才不会把它收窄成永远是 null。
  const top: { main: CostRow | null } = { main: null };
  const mainRow = (at: number): CostRow => {
    if (!top.main) { top.main = { id: "main", role: "main", title: mainTitle, engine: "peck", model: null, usage: null, startedAt: at, endedAt: null }; rows.unshift(top.main); }
    return top.main;
  };
  for (const e of events) {
    switch (e.type) {
      case "session.started":
        if (!e.subagentId && !cur) { const m = mainRow(e.at); m.engine = e.engine; m.model = e.model ?? m.model; }
        break;
      case "stage.started":
        cur = { id: `s-${e.seq}`, role: e.stage, title: e.title, engine: e.engine, model: e.model ?? null, usage: null, startedAt: e.at, endedAt: null };
        rows.push(cur);
        break;
      case "stage.completed":
        if (cur) cur.endedAt = e.at;
        cur = null;
        break;
      case "subagent.started": {
        const row: CostRow = { id: `sub-${e.subagentId}`, role: "sub", title: e.title, engine: "peck", model: null, usage: null, startedAt: e.at, endedAt: null };
        subs.set(e.subagentId, row);
        rows.push(row);
        break;
      }
      case "subagent.completed": {
        const row = subs.get(e.subagentId);
        if (row) row.endedAt = e.at;
        break;
      }
      case "turn.completed":
        if (top.main && top.main.endedAt == null) top.main.endedAt = e.at;
        break;
      case "usage.updated": {
        const row = (e.subagentId ? subs.get(e.subagentId) : null) ?? cur ?? mainRow(e.at);
        if (e.model && !row.model) row.model = e.model;
        const line = usageLine(e.usage, e.cost);
        if (e.scope === "cumulative") {
          const seen = cum.get(row);
          const prior = lastCum.get(row.engine) ?? null;
          // 续接的会话接着累计：这一段第一次报的数不比上一段报的少，就减掉上一段的。
          const base = seen ? seen.base : row.role === "fix" && prior && (line.input ?? 0) >= (prior.input ?? 0) ? prior : null;
          cum.set(row, { first: seen?.first ?? line, last: line, base });
          lastCum.set(row.engine, line);
        } else {
          const had = turn.get(row);
          turn.set(row, had ? addUsage(had, line) : line);
        }
        break;
      }
    }
  }
  for (const row of rows) {
    const t = turn.get(row) ?? null;
    const c = cum.get(row);
    const fromCum = c ? (c.base ? minusUsage(c.last, c.base) : c.last) : null;
    row.usage = t && fromCum ? addUsage(t, fromCum) : t ?? fromCum;
  }
  // 主线没花钱、也没别的段（比如合作的总头）：不列。
  const shown = rows.filter((r) => r !== top.main || r.usage || rows.length === 1);
  return shown.length >= 2 ? shown : [];
}

export function minusUsage(a: UsageLine, b: UsageLine): UsageLine {
  const minus = (x: number | null, y: number | null) => (x == null ? null : Math.max(0, x - (y ?? 0)));
  return { ...a, input: minus(a.input, b.input), output: minus(a.output, b.output), cacheRead: minus(a.cacheRead, b.cacheRead), cacheWrite: minus(a.cacheWrite, b.cacheWrite), usd: a.usd == null ? null : Math.max(0, a.usd - (b.usd ?? 0)) };
}

/** 一行花费表里的「花了多少」：API 写美元，CLI 写「订阅」（自己估了就带上估的）。 */
export function costCell(row: Pick<CostRow, "engine" | "usage">): string {
  const money = usd(row.usage?.usd ?? null);
  if (row.engine !== "peck") return money ? `订阅 · 估 ${money}` : "订阅";
  if (!row.usage) return "—";
  return money == null ? "算不出" : `${money}${row.usage.partial ? "+" : ""}`;
}

/** 一段用了多少 tokens：「进 12.3k / 出 640」（进包括缓存）。 */
export function tokenCell(u: UsageLine | null): string {
  if (!u || (u.input == null && u.output == null)) return "—";
  return `${tokens((u.input ?? 0) + (u.cacheRead ?? 0) + (u.cacheWrite ?? 0))} / ${tokens(u.output ?? 0)}`;
}
