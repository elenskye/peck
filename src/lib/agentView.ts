// 统一事件 → 界面上的样子。Peck 循环和 CLI 引擎同一套：
// - 回答（Markdown，边出字边显示）；思考折叠成「思考 · 12s」；
// - 连续的工具调用折叠成一行（「读了 4 个文件、改了 2 个、跑了 1 条命令 · 38s」），在跑的那一步露出来；
// - 没批、没答的审批和 ASK 单独拿出来做卡片；任务清单取最新的一份；
// - 花费：这一次的进、出、缓存命中、美元（CLI 估的标「估」，不知道的写「未知」，不当 0）。
import type { AgentEvent, ApprovalDecision, Cost, CostSource, FileDiff, PlanStep, ResolvedBy, TokenUsage, ToolKind, ToolStatus } from "@/lib/agentEvents";

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

export type ViewItem =
  | { kind: "text"; id: string; text: string; done: boolean }
  | { kind: "reasoning"; id: string; text: string; done: boolean; startedAt: number; endedAt?: number }
  | { kind: "tools"; id: string; calls: ToolView[] }
  | { kind: "asked"; id: string; ask: AskView }
  | { kind: "approved"; id: string; approval: ApprovalView }
  | { kind: "notice"; id: string; level: "info" | "warn"; text: string }
  | { kind: "error"; id: string; text: string };

export interface UsageLine {
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
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
}

const LIVE_TAIL = 8 * 1024;

export function foldEvents(events: AgentEvent[], stopped = false): AgentView {
  const view: AgentView = { engine: null, model: null, items: [], approvals: [], asks: [], plan: null, files: [], usage: null, ended: null, sessionRef: null };
  const tools = new Map<string, ToolView>();
  const texts = new Map<string, Extract<ViewItem, { kind: "text" }>>();
  const thoughts = new Map<string, Extract<ViewItem, { kind: "reasoning" }>>();
  const approvals = new Map<string, ApprovalView>();
  const asks = new Map<string, AskView>();
  const subStreamed = new Set<string>();
  let diffFiles: FileDiff[] | null = null;
  // 用量：一回合一次（turn）的加起来；累计（cumulative）的取最后一条。
  let turnUsage: UsageLine | null = null;
  let cumulative: UsageLine | null = null;
  const lastGroup = (): Extract<ViewItem, { kind: "tools" }> | null => {
    const last = view.items[view.items.length - 1];
    return last && last.kind === "tools" ? last : null;
  };
  for (const e of events) {
    switch (e.type) {
      case "session.started":
        view.engine = e.engine;
        view.model = e.model ?? null;
        view.sessionRef = e.sessionRef ?? view.sessionRef;
        break;
      case "message.delta":
      case "message.completed": {
        // 子任务（Claude Code 的 Task）说的话不是主回答：放进那个子任务那一步的输出里。
        if (e.subagentId) {
          const parent = tools.get(e.subagentId);
          if (parent) {
            if (e.type === "message.delta") { parent.live += e.text; subStreamed.add(e.itemId); }
            else if (!subStreamed.has(e.itemId)) parent.live += `${e.text}\n`;
            else parent.live += "\n";
          }
          break;
        }
        let item = texts.get(e.itemId);
        if (!item) {
          item = { kind: "text", id: `t-${e.itemId}-${e.seq}`, text: "", done: false };
          texts.set(e.itemId, item);
          view.items.push(item);
        }
        if (e.type === "message.delta") item.text += e.text;
        else { item.text = e.text; item.done = true; }
        break;
      }
      case "reasoning.delta":
      case "reasoning.completed": {
        if (e.subagentId) break;
        let item = thoughts.get(e.itemId);
        if (!item) {
          if (e.type === "reasoning.completed" && !e.text.trim()) break;
          item = { kind: "reasoning", id: `r-${e.itemId}-${e.seq}`, text: "", done: false, startedAt: e.at };
          thoughts.set(e.itemId, item);
          view.items.push(item);
        }
        if (e.type === "reasoning.delta") item.text += e.text;
        else { item.text = e.text || item.text; item.done = true; item.endedAt = e.at; }
        break;
      }
      case "tool_call.started": {
        const call: ToolView = { callId: e.callId, name: e.name, kind: e.kind, title: e.title, input: e.input, status: "running", live: "", diff: [], startedAt: e.at, subagentId: e.subagentId };
        tools.set(e.callId, call);
        const group = lastGroup();
        if (group) group.calls.push(call);
        else view.items.push({ kind: "tools", id: `g-${e.callId}-${e.seq}`, calls: [call] });
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
      case "error":
        // 不致命的（Codex 自己会重试的那种）只是一行提醒，不画成红条。
        if (e.fatal === false) view.items.push({ kind: "notice", id: `e-${e.seq}`, level: "warn", text: e.message });
        else view.items.push({ kind: "error", id: `e-${e.seq}`, text: e.message });
        break;
      case "turn.completed":
        view.ended = { reason: e.reason, error: e.error };
        // 回合结束了还在「在跑」的工具，按结束原因收掉。
        for (const call of tools.values()) {
          if (call.status === "running") { call.status = e.reason === "done" ? "ok" : "cancelled"; call.endedAt = e.at; }
        }
        for (const t of texts.values()) t.done = true;
        for (const t of thoughts.values()) { if (!t.done) t.endedAt = e.at; t.done = true; }
        break;
      default:
        break;
    }
  }
  if (stopped && !view.ended && events.length > 0) {
    // 记录上已经结束、事件里却没有收尾：按「停下了」收掉，时间记到最后一条事件。
    const at = events[events.length - 1].at;
    view.ended = { reason: "cancelled", error: null };
    for (const call of tools.values()) if (call.status === "running") { call.status = "cancelled"; call.endedAt = at; }
    for (const t of texts.values()) t.done = true;
    for (const t of thoughts.values()) { if (!t.done) t.endedAt = at; t.done = true; }
  }
  view.approvals = view.ended ? [] : [...approvals.values()].filter((a) => !a.decision);
  view.asks = view.ended ? [] : [...asks.values()].filter((a) => a.answer == null);
  view.usage = cumulative ?? turnUsage;
  view.files = diffFiles ?? collectDiffs([...tools.values()]);
  return view;
}

function usageLine(u: TokenUsage, c: Cost): UsageLine {
  const known = u.input != null || u.output != null;
  return {
    input: u.input ?? null,
    output: u.output ?? null,
    cacheRead: u.cacheRead ?? null,
    cacheWrite: u.cacheWrite ?? null,
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

/** 一组工具调用折叠成一句：「读了 4 个文件、改了 2 个、跑了 1 条命令 · 38s」。 */
export function groupSummary(calls: ToolView[]): string {
  const count = (k: ToolKind) => calls.filter((c) => c.kind === k).length;
  const edited = new Set(calls.filter((c) => c.kind === "edit").flatMap((c) => (c.diff.length ? c.diff.map((d) => d.path) : [c.callId])));
  const parts: string[] = [];
  if (count("read")) parts.push(`读了 ${count("read")} 个文件`);
  if (count("search")) parts.push(`搜了 ${count("search")} 次`);
  if (edited.size) parts.push(`改了 ${edited.size} 个`);
  if (count("run")) parts.push(`跑了 ${count("run")} 条命令`);
  if (count("fetch")) parts.push(`取了 ${count("fetch")} 个网页`);
  const other = count("mcp") + count("other");
  if (other) parts.push(`其他 ${other} 步`);
  return parts.join("、") || `${calls.length} 步`;
}

export function groupSpan(calls: ToolView[], now: number): number {
  const start = Math.min(...calls.map((c) => c.startedAt));
  const end = calls.some((c) => c.status === "running") ? now : Math.max(...calls.map((c) => c.endedAt ?? c.startedAt));
  return Math.max(0, end - start);
}

/** 落盘的完整输出路径（bash_run 的结果里写着「完整的存在 …」）。 */
export function spillPath(output?: string | null): string | null {
  const m = output?.match(/完整的存在 (\S+?\.log)/);
  return m ? m[1] : null;
}

const fmt = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(n >= 100_000 ? 0 : 1)}k` : n.toLocaleString());

/** 「$0.0123」「< $0.0001」；不知道是 null。 */
export function usd(v: number | null | undefined): string | null {
  if (v == null) return null;
  if (v === 0) return "$0";
  if (v < 0.0001) return "< $0.0001";
  if (v < 0.01) return `$${v.toFixed(4)}`;
  if (v < 1) return `$${v.toFixed(3)}`;
  return `$${v.toFixed(2)}`;
}

/** 一行花费：「进 12.3k · 出 640 · 缓存命中 8.1k · $0.0042」；CLI 估的「估 $0.03」。 */
export function usageText(u: UsageLine): string {
  const bits: string[] = [];
  if (u.input != null) bits.push(`进 ${fmt(u.input + (u.cacheRead ?? 0) + (u.cacheWrite ?? 0))}`);
  if (u.output != null) bits.push(`出 ${fmt(u.output)}`);
  if (u.cacheRead) bits.push(`缓存命中 ${fmt(u.cacheRead)}`);
  const money = usd(u.usd);
  if (money == null) bits.push("花费未知");
  else bits.push(`${u.source === "estimated" ? "估 " : ""}${money}${u.partial ? "（有一部分算不出）" : ""}`);
  return bits.join(" · ");
}
