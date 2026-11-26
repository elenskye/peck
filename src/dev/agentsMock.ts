// `?mock` 里的 PLOOP 子代理（v1.1.2）：主角色一次派两个 explorer（同时跑，事件带 subagentId，挂在派的那一步下面），
// 一批做完报告一起回来；派的那一步等子代理做完才算完。还有开头那条成员名单。
// 任务板（v1.1.3）：派 worker、member 划文件，不重叠的同时跑、重叠的排队，member 再派自己的 explorer；板上进度 Peck 填。
import type { BoardItem, ToolKind } from "@/lib/agentEvents";
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

/** 成员名单：主角色和五类子代理（演示里都跟主角色）。 */
export function rosterEvent(model: string): ReplayEvent {
  const members = [
    { role: "main", model, rounds: 40 },
    { role: "explorer", model, rounds: 24, parallel: 3 },
    { role: "searcher", model, rounds: 24, parallel: 3 },
    { role: "worker", model, rounds: 24, parallel: 3 },
    { role: "member", model, rounds: 40, parallel: 2 },
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
      // 子代理自己到第几轮了（v1.1.11，写在那一步右边）。
      { type: "round.started", round: 1, max: 30, subagentId: a } as ReplayEvent,
      { type: "round.started", round: 1, max: 30, subagentId: b } as ReplayEvent,
      tool(`${a}-1`, "search_text", "search", "搜「title」", { pattern: "title" }, a),
      tool(`${b}-1`, "read_file", "read", "读 package.json", { path: "package.json" }, b),
      { type: "round.started", round: 2, max: 30, subagentId: a } as ReplayEvent,
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

/** 任务板（v1.1.3）：派 worker 1（改要改的那个文件）、worker 2（README）、member 1（src/lib，它再派一个 explorer），
 *  再派 worker 3 也改那个文件——和 worker 1 重叠，排到 worker 1 做完。板上的进度跟着事件走。 */
export function boardRound(runId: string, model: string, editPath: string): (() => ReplayEvent[])[] {
  return [() => {
    const out: ReplayEvent[] = [];
    // 回放每条隔 150ms：照这个估时刻，板上的用时看起来对。
    const t0 = Date.now();
    const at = () => t0 + out.length * 150;
    const items: BoardItem[] = [];
    const board = () => out.push({ type: "board.updated", items: items.map((i) => ({ ...i, paths: [...i.paths] })) } as ReplayEvent);
    const set = (id: string, status: BoardItem["status"]) => {
      const it = items.find((i) => i.id === id)!;
      it.status = status;
      if (status === "running") it.startedAt = at();
      if (status === "done") it.finishedAt = at();
      board();
    };
    const ids = { w1: `${runId}-w1`, w2: `${runId}-w2`, m1: `${runId}-m1`, w3: `${runId}-w3`, e1: `${runId}-m1-e1` };
    const spawn = (id: string, owner: string, number: number, title: string, task: string, paths: string[]) => {
      out.push(tool(id, "spawn_agent", "other", `派 ${owner}：${title}`, { type: owner, title, task, paths }));
      out.push({ type: "subagent.started", subagentId: id, parentCallId: id, title: `${owner} · Claude Fable 5.2`, model, kind: owner } as ReplayEvent);
      items.push({ id, title, owner, number, paths, status: "queued", model });
      board();
    };
    out.push(...say(`${runId}-bd-m`, "分成三块同时做：标题给 worker，README 给另一个 worker，src/lib 的整理比较复杂交给 member。", undefined));
    spawn(ids.w1, "worker", 1, "标题改成两行", `把 ${editPath} 的标题改成两行`, [editPath]);
    spawn(ids.w2, "worker", 2, "README 补用法", "README 里补一段用法", ["README.md"]);
    spawn(ids.m1, "member", 1, "整理 src/lib", "把 src/lib 里重复的工具函数合并，改完跑测试", ["src/lib"]);
    spawn(ids.w3, "worker", 3, "标题加副标题", `在 ${editPath} 的标题下面加一行副标题`, [editPath]);
    set(ids.w1, "running");
    set(ids.w2, "running");
    set(ids.m1, "running");
    out.push(tool(`${ids.w1}-1`, "read_file", "read", `读 ${editPath}`, { path: editPath }, ids.w1));
    out.push(tool(`${ids.w2}-1`, "read_file", "read", "读 README.md", { path: "README.md" }, ids.w2));
    out.push(tool(ids.e1, "spawn_agent", "other", "派 explorer：src/lib 里哪些函数重复了", { type: "explorer", task: "src/lib 里哪些函数重复了" }, ids.m1));
    out.push({ type: "subagent.started", subagentId: ids.e1, parentCallId: ids.e1, title: "explorer · Claude Fable 5.2", model, kind: "explorer" } as ReplayEvent);
    out.push(done(`${ids.w1}-1`, "# 标题", ids.w1));
    out.push(done(`${ids.w2}-1`, "# 项目", ids.w2));
    out.push(tool(`${ids.e1}-1`, "search_text", "search", "搜「export function」", { pattern: "export function" }, ids.e1));
    out.push(tool(`${ids.w1}-2`, "write_patch", "edit", `改 ${editPath}`, { path: editPath }, ids.w1));
    out.push(done(`${ids.e1}-1`, "src/lib/a.ts:3 …\nsrc/lib/b.ts:8 …", ids.e1));
    out.push(...say(`${ids.e1}-m`, "fmt 和 format 是同一个函数，各在 a.ts、b.ts。", ids.e1));
    out.push({ type: "subagent.completed", subagentId: ids.e1, status: "ok" } as ReplayEvent);
    out.push(done(ids.e1, "fmt 和 format 是同一个函数。", ids.m1));
    out.push(done(`${ids.w1}-2`, "改好了 1 处。", ids.w1));
    out.push(...say(`${ids.w1}-m`, "标题改成两行了。", ids.w1));
    out.push({ type: "subagent.completed", subagentId: ids.w1, status: "ok" } as ReplayEvent);
    out.push(done(ids.w1, "标题改成两行了。"));
    set(ids.w1, "done");
    set(ids.w3, "running");
    out.push(tool(`${ids.m1}-2`, "write_patch", "edit", "改 src/lib/a.ts、src/lib/b.ts", {}, ids.m1));
    out.push(tool(`${ids.w2}-2`, "write_patch", "edit", "改 README.md", { path: "README.md" }, ids.w2));
    out.push(tool(`${ids.w3}-1`, "read_file", "read", `读 ${editPath}`, { path: editPath }, ids.w3));
    out.push(done(`${ids.w2}-2`, "改好了 1 处。", ids.w2));
    out.push(...say(`${ids.w2}-m`, "README 补了一段用法。", ids.w2));
    out.push({ type: "subagent.completed", subagentId: ids.w2, status: "ok" } as ReplayEvent);
    out.push(done(ids.w2, "README 补了一段用法。"));
    set(ids.w2, "done");
    out.push(done(`${ids.w3}-1`, "# 标题\n# 第二行", ids.w3));
    out.push(done(`${ids.m1}-2`, "改好了 2 处。", ids.m1));
    out.push(tool(`${ids.m1}-3`, "bash_run", "run", "跑 pnpm test", { command: "pnpm test" }, ids.m1));
    out.push(tool(`${ids.w3}-2`, "write_patch", "edit", `改 ${editPath}`, { path: editPath }, ids.w3));
    out.push(done(`${ids.w3}-2`, "改好了 1 处。", ids.w3));
    out.push(...say(`${ids.w3}-m`, "加了副标题。", ids.w3));
    out.push({ type: "subagent.completed", subagentId: ids.w3, status: "ok" } as ReplayEvent);
    out.push(done(ids.w3, "加了副标题。"));
    set(ids.w3, "done");
    out.push(done(`${ids.m1}-3`, "12 passed", ids.m1));
    out.push(...say(`${ids.m1}-m`, "合并了 fmt 和 format，测试都过了。", ids.m1));
    out.push(usage(0.0021, ids.m1));
    out.push({ type: "subagent.completed", subagentId: ids.m1, status: "ok" } as ReplayEvent);
    out.push(done(ids.m1, "合并了 fmt 和 format，测试都过了。"));
    set(ids.m1, "done");
    return out;
  }];
}
