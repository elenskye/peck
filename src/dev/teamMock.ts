// `?mock` 里的多引擎合作（v0.8 Phase 11）：接力（写规范 → 实施）、交叉审查（要改就交回修一次）、委派（子任务）。
// 每一段前面一条 stage.started、后面一条 stage.completed（这一段引擎自己的 turn.completed 换掉），
// 整个任务最后一条 turn.completed 由 replayAgent 补。审查默认说要改；这句话里带「通过」就直接过。
import type { AgentCaps, CriterionCheck, StageRole, ToolKind } from "@/lib/agentEvents";
import type { ReplayEvent } from "@/dev/gatewayMock";
import { MOCK_ENGINE_REPLAYS } from "@/dev/gatewayMock";
import type { Round } from "@/dev/peckMock";

export interface Member {
  /** api:… / cli:… */
  who: string;
  label: string;
}

const STAGE_CAPS: AgentCaps = {
  streaming: true, reasoningReplay: true, tools: "peck", approvals: "live", askUser: true, steer: true, interrupt: true,
  resume: true, mcpInject: false, usage: "per_turn", cost: "estimated", quota: false, switchModel: true, needsTty: false, directOnly: false,
};

const engineOf = (m: Member) => (m.who.startsWith("api:") ? "peck" : m.who.slice(4));
const modelOf = (m: Member) => (m.who.startsWith("api:") ? m.who.slice(4) : null);

function tool(callId: string, name: string, kind: ToolKind, title: string, input: unknown, subagentId?: string): ReplayEvent {
  return { type: "tool_call.started", callId, name, kind, title, input, ...(subagentId ? { subagentId } : {}) } as ReplayEvent;
}

function done(callId: string, output: string, extra: Partial<Extract<ReplayEvent, { type: "tool_call.completed" }>> = {}, subagentId?: string): ReplayEvent {
  return { type: "tool_call.completed", callId, status: "ok", output, exitCode: null, diff: [], durationMs: 400, ...extra, ...(subagentId ? { subagentId } : {}) } as ReplayEvent;
}

function say(id: string, text: string, subagentId?: string): ReplayEvent[] {
  const parts = text.match(/[\s\S]{1,18}/g) ?? [text];
  const sub = subagentId ? { subagentId } : {};
  return [
    ...parts.map((p) => ({ type: "message.delta", itemId: id, text: p, ...sub }) as ReplayEvent),
    { type: "message.completed", itemId: id, text, ...sub } as ReplayEvent,
  ];
}

function usage(usd: number, subagentId?: string): ReplayEvent {
  return { type: "usage.updated", usage: { input: 3200, output: 420, cacheRead: 1800, cacheWrite: null, reasoning: null }, cost: { usd, source: "estimated" }, scope: "turn", ...(subagentId ? { subagentId } : {}) } as ReplayEvent;
}

/** 一段：头一条 stage.started；里面引擎自己的 turn.completed 换成 stage.completed；没有就补一条。 */
export function stage(role: StageRole, m: Member, rounds: Round[], verdict?: "pass" | "changes", checks?: CriterionCheck[]): Round[] {
  const word = { spec: "写规范", implement: "实施", review: "审查", fix: "按审查改" }[role];
  let closed = false;
  const swap = (events: ReplayEvent[]): ReplayEvent[] =>
    events.map((e) => {
      if (e.type !== "turn.completed") return e;
      closed = true;
      return { type: "stage.completed", stage: role, reason: e.reason, error: e.error ?? null, verdict: verdict ?? null, ...(checks ? { checks } : {}) } as ReplayEvent;
    });
  const wrapped: Round[] = rounds.map((r) => (typeof r === "function" ? (s) => swap(r(s)) : swap(r)));
  return [
    [
      { type: "stage.started", stage: role, engine: engineOf(m), model: modelOf(m), title: `${m.label} ${word}` } as ReplayEvent,
      { type: "session.started", engine: engineOf(m), model: modelOf(m), caps: STAGE_CAPS, tools: [], mcp: [], sessionRef: null, version: null } as ReplayEvent,
    ],
    ...wrapped,
    () => (closed ? [] : [{ type: "stage.completed", stage: role, reason: "done", error: null, verdict: verdict ?? null, ...(checks ? { checks } : {}) } as ReplayEvent]),
  ];
}

/** 演示规范里的验收标准（v0.9：规范按固定格式写，审查逐条判）。 */
export const MOCK_CRITERIA = (editPath: string) => [`\`${editPath}\` 开头的说明写的是做完以后的样子`, "只改这一个文件", "`pnpm check` 过"];

/** 写规范（只读）：看两眼，按固定格式写一份规范，带编号的验收标准。 */
export function specRounds(runId: string, writer: Member, builder: Member, editPath: string): Round[] {
  const criteria = MOCK_CRITERIA(editPath).map((c, i) => `${i + 1}. ${c}`).join("\n");
  const thought = "先看要改的文件和周围的写法，再按五个标题写。";
  return stage("spec", writer, [
    [...(thought.match(/.{1,6}/g) ?? []).map((t) => ({ type: "reasoning.delta", itemId: `${runId}-st`, text: t }) as ReplayEvent), { type: "reasoning.completed", itemId: `${runId}-st`, text: thought, opaque: null } as ReplayEvent],
    [tool(`${runId}-s1`, "read_file", "read", `读 ${editPath}`, { path: editPath })],
    [done(`${runId}-s1`, "     1\t// …（演示）"), tool(`${runId}-s2`, "read_file", "read", "读 README.md", { path: "README.md" })],
    [done(`${runId}-s2`, "# peck（演示）")],
    [
      ...say(`${runId}-spec`, `## 目标\n照这句话改好，别的不动（给 ${builder.label}）。\n\n## 改哪些文件\n\`${editPath}\`：开头加一行说明。\n\n## 步骤\n1. 先读文件\n2. 改一处\n3. 跑一遍检查\n\n## 验收标准\n${criteria}\n\n## 不做什么\n不动别的文件和样式。`),
      usage(0.004),
    ],
  ]);
}

/** CLI 实施：回放这家录下来的过程（turn.completed 换成这一段的收尾）。 */
export function cliRounds(m: Member): Round[] {
  const replay = MOCK_ENGINE_REPLAYS[m.who.slice(4)] ?? [];
  return [replay.filter((e) => e.type !== "session.started")];
}

/** 审查（只读）：看一遍 Diff，挑一处要改的（或者直接过）。接力带了验收标准就逐条判（v0.9）。 */
export function reviewRounds(runId: string, reviewer: Member, editPath: string, pass: boolean, withCriteria = false): Round[] {
  const notes = [
    pass ? "第 1 行写的是「已改」" : `第 1 行写成了「待办」，其实已经做完了，改成「已改」`,
    `只动了 \`${editPath}\``,
    "演示里没跑，按 Diff 看类型没问题",
  ];
  const checks: CriterionCheck[] | undefined = withCriteria
    ? MOCK_CRITERIA(editPath).map((criterion, i) => ({ n: i + 1, criterion, pass: i === 0 ? pass : true, note: notes[i] }))
    : undefined;
  const text = checks
    ? `${checks.map((c) => `CHECK ${c.n}: ${c.pass ? "PASS" : "FAIL"} — ${c.note}`).join("\n")}\n\n别的没看到问题。\n\nVERDICT: ${pass ? "PASS" : "CHANGES"}`
    : pass
    ? `看了这次的改动：\`${editPath}\` 只加了一行说明，没碰别的，能留着。\n\nVERDICT: PASS`
    : `看了这次的改动，有一处要改：\n\n1. \`${editPath}\` 第 1 行：说明写成了「待办」，其实已经做完了，改成「已改」。\n\n别的没问题。\n\nVERDICT: CHANGES`;
  return stage(
    "review",
    reviewer,
    [
      [tool(`${runId}-r1`, "read_file", "read", `读 ${editPath}`, { path: editPath }), done(`${runId}-r1`, "     1\t// 待办：标题改成中文（Peck Loop 演示）")],
      [...say(`${runId}-review`, text), usage(0.006)],
    ],
    pass ? "pass" : "changes",
    checks,
  );
}

/** 按审查改一次。 */
export function fixRounds(runId: string, builder: Member, editPath: string): Round[] {
  const patch = `--- a/${editPath}\n+++ b/${editPath}\n@@ -1,1 +1,1 @@\n-// 待办：标题改成中文（Peck Loop 演示）\n+// 已改：标题改成中文（Peck Loop 演示）\n`;
  return stage("fix", builder, [
    [
      tool(`${runId}-f1`, "write_patch", "edit", `改 ${editPath}`, { edits: [{ path: editPath, old_text: "// 待办", new_text: "// 已改" }] }),
      done(`${runId}-f1`, `改好了 1 处：${editPath} 第 1 行`, { diff: [{ path: editPath, change: "modified", additions: 1, deletions: 1, patch }] }),
    ],
    [...say(`${runId}-fix`, `按审查改了：\`${editPath}\` 的说明换成「已改」。审查的另一处没有意见。\n\n→ 在 FILES 里看一眼改过的文件\n→ 跑一遍 pnpm check`), usage(0.003)],
  ]);
}

/** 委派的一次：主模型交给子任务，子任务读一个文件、写一句报告（事件带 subagentId）。 */
export function delegateRound(runId: string, sub: Member, editPath: string): ReplayEvent[] {
  const id = `${runId}-del`;
  const task = `查一下 ${editPath} 里还有哪些地方用到标题，列出来（不用改）`;
  return [
    tool(id, "delegate_task", "other", `交给子任务：${task}`, { task }),
    { type: "subagent.started", subagentId: id, parentCallId: id, title: `${sub.label}：${task.slice(0, 30)}` } as ReplayEvent,
    tool(`${id}-1`, "search_text", "search", "搜「title」", { pattern: "title" }, id),
    done(`${id}-1`, `${editPath}:1: …\n${editPath}:12: …`, {}, id),
    ...say(`${id}-m`, `用到标题的有两处：${editPath} 第 1 行和第 12 行，都是显示用的，没有别的依赖。`, id),
    usage(0.0004, id),
    { type: "subagent.completed", subagentId: id, status: "ok" } as ReplayEvent,
    done(id, `子任务（${sub.label}）做完了；没改新的文件。\n它的报告：\n用到标题的有两处：${editPath} 第 1 行和第 12 行。`),
  ];
}
