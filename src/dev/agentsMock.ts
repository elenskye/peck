// `?mock` 里的 PLOOP 子代理（v1.1.2）：主角色一次派两个 explorer（同时跑，事件带 subagentId，挂在派的那一步下面），
// 一批做完报告一起回来；派的那一步等子代理做完才算完。还有开头那条成员名单（LOG 里看）。
import type { ToolKind } from "@/lib/agentEvents";
import type { ReplayEvent } from "@/dev/gatewayMock";

function tool(callId: string, name: string, kind: ToolKind, title: string, input: unknown, subagentId?: string): ReplayEvent {
  return { type: "tool_call.started", callId, name, kind, title, input, ...(subagentId ? { subagentId } : {}) } as ReplayEvent;
}

function done(callId: string, output: string, subagentId?: string): ReplayEvent {
  return { type: "tool_call.completed", callId, status: "ok", output, exitCode: null, diff: [], durationMs: 1200, ...(subagentId ? { subagentId } : {}) } as ReplayEvent;
}

function say(id: string, text: string, subagentId?: string): ReplayEvent[] {
  const sub = subagentId ? { subagentId } : {};
  return [{ type: "message.delta", itemId: id, text, ...sub } as ReplayEvent, { type: "message.completed", itemId: id, text, ...sub } as ReplayEvent];
}

function usage(usd: number, subagentId?: string): ReplayEvent {
  return { type: "usage.updated", usage: { input: 2400, output: 180, cacheRead: 1200, cacheWrite: null, reasoning: null }, cost: { usd, source: "priced" }, scope: "turn", ...(subagentId ? { subagentId } : {}) } as ReplayEvent;
}

/** 成员名单：主角色和四类子代理（演示里都跟主角色）。 */
export function rosterEvent(model: string): ReplayEvent {
  const members = [
    { role: "main", model, rounds: 40 },
    { role: "explorer", model, rounds: 24, parallel: 3 },
    { role: "searcher", model, rounds: 24, parallel: 3 },
    { role: "worker", model, rounds: 24 },
    { role: "auditor", model, rounds: 24 },
  ];
  return { type: "agents.roster", members } as ReplayEvent;
}

/** 派两个 explorer：一个找标题用在哪，一个看测试怎么跑。一批做完才回来。 */
export function spawnRound(runId: string, model: string, editPath: string): ReplayEvent[][] {
  const a = `${runId}-sp1`;
  const b = `${runId}-sp2`;
  const taskA = `找出 ${editPath} 里标题用在哪些地方，列出行号`;
  const taskB = "看看这个项目的测试怎么跑、要跑多久";
  return [
    [
      ...say(`${runId}-sp-m`, "", undefined),
      tool(a, "spawn_agent", "other", `派 explorer：${taskA}`, { type: "explorer", task: taskA }),
      { type: "subagent.started", subagentId: a, parentCallId: a, title: "explorer · Claude Fable 5.2", model, kind: "explorer" } as ReplayEvent,
      tool(b, "spawn_agent", "other", `派 explorer：${taskB}`, { type: "explorer", task: taskB }),
      { type: "subagent.started", subagentId: b, parentCallId: b, title: "explorer · Claude Fable 5.2", model, kind: "explorer" } as ReplayEvent,
      tool(`${a}-1`, "search_text", "search", "搜「title」", { pattern: "title" }, a),
      tool(`${b}-1`, "read_file", "read", "读 package.json", { path: "package.json" }, b),
      done(`${a}-1`, `${editPath}:1: …\n${editPath}:12: …`, a),
      done(`${b}-1`, "{ \"scripts\": { \"test\": \"vitest\" } }", b),
      ...say(`${a}-m`, `标题用在 ${editPath} 第 1 行和第 12 行，都只是显示。`, a),
      usage(0.0004, a),
      { type: "subagent.completed", subagentId: a, status: "ok" } as ReplayEvent,
      done(a, `标题用在 ${editPath} 第 1 行和第 12 行，都只是显示。`),
      ...say(`${b}-m`, "测试是 `pnpm test`（vitest），全跑大概 20 秒。", b),
      usage(0.0003, b),
      { type: "subagent.completed", subagentId: b, status: "ok" } as ReplayEvent,
      done(b, "测试是 `pnpm test`（vitest），全跑大概 20 秒。"),
    ],
  ];
}
