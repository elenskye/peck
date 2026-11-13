// `?mock` 里 Peck 循环的剧本（主线和 Rust 的 ScriptedModel::peck_demo 同一个流程）：
// 看目录 → 读 → 搜 → 改 → 跑一条输出很长的命令（落盘）→ 删 dist（高危卡片）→ 清单 → 想记住两条 → 问一句 → 收尾。
// 一轮是一次模型请求：先报上下文多大，再出思考和字、报用量和花费，然后是这一轮要的工具。
//
// v0.6 的几个场景按这句话里的字挑（testing.md 写着）：
// - 「长任务」：读几个大文件，上下文过了压缩线，自动压缩三步（落盘、清旧输出、交接摘要）以后接着做完；
// - 「修好」：改完验证没过，交回模型修一次，再验证过了，挂 ✓；
// - 「反复」：同一个失败连续两次，停下；
// - 「并发」：后台验证跑到一半文件又改了，这次的结果过期，按新文件再跑；
// - 发的时候开了 GOAL：目标模式，模型停下就跑目标那条，没过接着修。
// 项目有验证命令（演示里 peck 有）时，主线改完文件也会在后台跑一遍、收尾时挂 ✓。
import type { AgentCaps, EventBody, ToolKind } from "@/lib/agentEvents";
import type { ReplayEvent } from "@/dev/gatewayMock";

export interface RoundState {
  /** 最近一次 ASK 的回答。 */
  answer: string;
}

export type Round = ReplayEvent[] | ((state: RoundState) => ReplayEvent[]);

/** 演示里的一条验证命令。 */
export interface DemoCheck { name: string; run: string }

export interface DemoOptions {
  /** 模型的上下文窗口。 */
  window: number;
  /** 这个项目的验证命令（.peck/verify.toml）。 */
  checks: DemoCheck[];
  /** 目标模式：要过的那条。 */
  goal?: string | null;
  /** 有没有项目说明和记忆（上下文里那一块多大）。 */
  project: boolean;
}

/** 上下文多大：Peck 的说明和工具、项目说明和记忆、对话、工具的输出。 */
function context(o: DemoOptions, conversation: number, tools: number): ReplayEvent {
  const system = 4300;
  const project = o.project ? 1100 : 0;
  return {
    type: "context.updated",
    used: system + project + conversation + tools,
    window: o.window,
    compactAt: Math.round(o.window * 0.8),
    parts: [
      { key: "system", tokens: system },
      { key: "project", tokens: project },
      { key: "conversation", tokens: conversation },
      { key: "tools", tokens: tools },
    ],
  };
}

function verifyStart(id: string, c: DemoCheck, trigger: "edit" | "end" | "goal", attempt = 0): ReplayEvent {
  return { type: "verify.started", checkId: id, name: c.name, command: c.run, trigger, attempt };
}

function verifyDone(id: string, c: DemoCheck, status: "passed" | "failed" | "stale", output: string | null, durationMs = 3200, exitCode: number | null = status === "failed" ? 2 : 0): ReplayEvent {
  return { type: "verify.completed", checkId: id, name: c.name, status, exitCode: status === "stale" ? null : exitCode, durationMs: status === "stale" ? null : durationMs, output };
}

function verifyRound(outcome: "passed" | "fixing" | "stopped", attempt: number, goal = false, reason: string | null = null): ReplayEvent {
  return { type: "verify.round", attempt, maxAttempts: goal ? 8 : 3, outcome, reason, goal };
}

const TS_ERROR = "src/App.tsx(3,7): error TS2322: Type 'number' is not assignable to type 'string'.\n\nFound 1 error in src/App.tsx:3";

export const PECK_CAPS: AgentCaps = {
  streaming: true, reasoningReplay: true, tools: "peck", approvals: "live", askUser: true, steer: true, interrupt: true,
  resume: true, mcpInject: false, usage: "per_turn", cost: "priced", quota: false, switchModel: true, needsTty: false, directOnly: false,
};

const chunks = (text: string, size = 6): string[] => {
  const chars = [...text];
  const out: string[] = [];
  for (let i = 0; i < chars.length; i += size) out.push(chars.slice(i, i + size).join(""));
  return out;
};

function say(round: number, text: string, reasoning?: string): ReplayEvent[] {
  const out: ReplayEvent[] = [];
  if (reasoning) {
    for (const c of chunks(reasoning)) out.push({ type: "reasoning.delta", itemId: `r${round}`, text: c });
    out.push({ type: "reasoning.completed", itemId: `r${round}`, text: reasoning, opaque: null });
  }
  if (text) {
    for (const c of chunks(text)) out.push({ type: "message.delta", itemId: `m${round}`, text: c });
    out.push({ type: "message.completed", itemId: `m${round}`, text });
  }
  return out;
}

function usage(input: number, output: number, cacheRead: number, usd: number | null, source: "priced" | "estimated" = "priced"): ReplayEvent {
  return {
    type: "usage.updated",
    usage: { input, output, cacheRead, cacheWrite: 0, reasoning: Math.round(output / 3) },
    cost: { usd, source: usd == null ? "unknown" : source },
    scope: "turn",
  };
}

const num = (lines: string[], from = 1) => lines.map((l, i) => `${String(i + from).padStart(6)}\t${l}`).join("\n");

function tool(callId: string, name: string, kind: ToolKind, title: string, input: unknown): ReplayEvent {
  return { type: "tool_call.started", callId, name, kind, title, input };
}

function done(callId: string, output: string, extra: Partial<Extract<EventBody, { type: "tool_call.completed" }>> = {}): ReplayEvent {
  return { type: "tool_call.completed", callId, status: "ok", output, exitCode: null, diff: [], durationMs: 180, ...extra };
}

/** Peck 循环的演示剧本。`files` 是这个项目的假文件（路径 → 内容）；`prompt` 里的字挑场景（见文件开头）。 */
export function peckDemo(runId: string, files: Record<string, string>, prompt = "", options: DemoOptions = { window: 1_000_000, checks: [], project: false }): { rounds: Round[]; edit: { path: string; before: string; after: string } } {
  const o = options;
  const check = o.checks[0] ?? { name: "check", run: "pnpm check" };
  const scenario = o.goal ? "goal" : /长任务/.test(prompt) ? "long" : /反复/.test(prompt) ? "repeat" : /并发/.test(prompt) ? "stale" : /修好/.test(prompt) ? "fix" : "main";
  const paths = Object.keys(files).sort();
  const editPath = paths.find((p) => /\.(tsx?|rs|go)$/.test(p) && !p.includes("legacy")) ?? paths[0] ?? "README.md";
  const before = files[editPath] ?? "";
  const lines = before.split("\n");
  const first = lines[0] ?? "";
  const after = `// 待办：标题改成中文（Peck 循环演示）\n${before}`;
  const word = first.replace(/[^\w<>]+/g, " ").trim().split(" ").find((w) => w.length > 2) ?? "export";
  const patch = `--- a/${editPath}\n+++ b/${editPath}\n@@ -1,3 +1,4 @@\n+// 待办：标题改成中文（Peck 循环演示）\n ${lines.slice(0, 3).join("\n ")}\n`;
  const top = [...new Set(paths.map((p) => (p.includes("/") ? `${p.split("/")[0]}/` : p)))];
  const testLog = Array.from({ length: 1400 }, (_, i) => (i % 70 === 0 ? `\n RUN  v3.2.0 /project · 第 ${i / 70 + 1} 组` : `  ✓ src/case-${String(i).padStart(4, "0")}.test.ts (3 tests) ${2 + (i % 9)}ms`)).join("\n");
  const tail = "\n Test Files  1400 passed (1400)\n      Tests  4200 passed (4200)\n   Duration  6.12s";
  const spillAt = `/mock/harness-output/${runId}/1-bash.log`;
  const shown = `退出码 0（6.4s）\n[输出共 ${Math.ceil((testLog + tail).length / 1024)} KB，太长：完整的存在 ${spillAt}（用 read_file 分段读）。下面是开头和结尾]\n${testLog.slice(0, 2048)}\n…（中间省略）…\n${(testLog + tail).slice(-6144)}`;
  MOCK_SPILLS[spillAt] = testLog + tail;
  const editRound = (id: string, extra: ReplayEvent[] = []): ReplayEvent[] => [
    tool(id, "write_patch", "edit", `改 ${editPath}`, { edits: [{ path: editPath, old_text: first, new_text: `// 待办：标题改成中文（Peck 循环演示）\n${first}` }] }),
    done(id, `改好了 1 处：\n\n${editPath} 第 1 行：\n${num(after.split("\n").slice(0, 4))}`, { diff: [{ path: editPath, change: "modified", additions: 1, deletions: 0, patch }] }),
    { type: "diff.updated", files: [{ path: editPath, change: "modified", additions: 1, deletions: 0, patch }] },
    ...extra,
  ];
  const read = (id: string): ReplayEvent[] => [tool(id, "read_file", "read", `读 ${editPath}`, { path: editPath }), done(id, num(lines.slice(0, 40)))];
  const edited = { path: editPath, before, after };
  if (scenario !== "main") return { rounds: scenarioRounds(scenario, o, check, read, editRound, editPath, runId), edit: edited };
  const verifying = o.checks.length > 0;
  const rounds: Round[] = [
    [
      context(o, 600, 0),
      ...say(1, "我先看一下项目结构。", "先看项目里有什么，再找要改的地方。"),
      usage(4200, 120, 0, 0.00061),
      tool("c1", "list_directory", "read", "看 项目根目录", { path: ".", depth: 2 }),
      done("c1", `./\n${top.join("\n")}`),
    ],
    [
      context(o, 900, 1800),
      ...say(2, ""),
      usage(900, 60, 4200, 0.00021),
      tool("c2", "read_file", "read", `读 ${editPath}`, { path: editPath }),
      done("c2", num(lines.slice(0, 40))),
    ],
    [
      context(o, 1300, 3200),
      ...say(3, "", `要改的在 ${editPath} 开头。搜一下别处有没有一样的写法。`),
      usage(1400, 80, 5100, 0.00028),
      tool("c3", "search_text", "search", `搜「${word}」`, { pattern: word, mode: "content" }),
      done("c3", `${editPath}:1: ${first}\n（1 个文件，1 处）`),
    ],
    [
      context(o, 1700, 3900),
      ...say(4, "找到了，在开头加一行说明。"),
      usage(600, 140, 6500, 0.00024),
      tool("c4", "write_patch", "edit", `改 ${editPath}`, { edits: [{ path: editPath, old_text: first, new_text: `// 待办：标题改成中文（Peck 循环演示）\n${first}` }] }),
      done("c4", `改好了 1 处：\n\n${editPath} 第 1 行：\n${num(after.split("\n").slice(0, 4))}`, { diff: [{ path: editPath, change: "modified", additions: 1, deletions: 0, patch }] }),
      { type: "diff.updated", files: [{ path: editPath, change: "modified", additions: 1, deletions: 0, patch }] },
      ...(verifying ? [verifyStart("v1", check, "edit")] : []),
    ],
    [
      context(o, 2100, 4300),
      ...say(5, "跑一下测试。"),
      usage(300, 40, 7100, 0.00013),
      tool("c5", "bash_run", "run", "跑 pnpm test", { command: "pnpm test", timeout_sec: 120 }),
      ...chunks(testLog.slice(0, 1800), 300).map((text): ReplayEvent => ({ type: "tool_call.output_delta", callId: "c5", text })),
      done("c5", shown, { exitCode: 0, durationMs: 6400 }),
      ...(verifying ? [verifyDone("v1", check, "passed", "退出码 0（3.2s）")] : []),
    ],
    [
      context(o, 2400, 8200),
      ...say(6, "测试过了。顺手把旧的构建产物 dist/ 删掉。"),
      usage(500, 60, 7300, 0.00017),
      tool("c6", "bash_run", "run", "跑 rm -rf dist", { command: "rm -rf dist" }),
      { type: "approval.requested", requestId: "approve-1", callId: "c6", kind: "run", title: "高危：删文件", detail: "rm -rf dist", highRisk: true, options: ["allow_once", "deny_once"] },
      done("c6", "退出码 0（0.1s）", { exitCode: 0, durationMs: 120 }),
    ],
    [
      context(o, 2700, 8400),
      ...say(7, ""),
      usage(900, 90, 7400, 0.00026),
      tool("c7", "update_plan", "other", "更新任务清单", { steps: [] }),
      { type: "plan.updated", steps: [
        { text: "找到要改的地方", status: "completed" },
        { text: "改成中文", status: "completed" },
        { text: "跑测试", status: "completed" },
        { text: "确认要不要也改副标题", status: "in_progress" },
      ], explanation: null },
      done("c7", "任务清单更新了（4 步）。"),
      tool("c7m", "memory", "other", "想记住几条", { notes: ["测试用 pnpm test，4200 个测试 6 秒左右跑完", "dist/ 是构建产物，可以删"] }),
      { type: "memory.proposed", notes: ["测试用 pnpm test，4200 个测试 6 秒左右跑完", "dist/ 是构建产物，可以删"] },
      done("c7m", "记下了 2 条，任务结束时站主决定要不要记住。"),
    ],
    [
      context(o, 3000, 8600),
      ...say(8, ""),
      usage(200, 50, 8300, 0.00019),
      tool("c8", "ask_user", "other", "问你", { question: "副标题也要改成中文吗？", options: ["一起改", "先不改"] }),
      { type: "user_input.requested", requestId: "ask-1", question: "副标题也要改成中文吗？", options: ["一起改", "先不改"], freeText: true },
      done("c8", "站主回答：（见上）"),
    ],
    (state) => [
      ...say(9, `改好了：\n\n- \`${editPath}\` 开头加了一行说明\n- \`pnpm test\` 通过（4200 个测试；完整日志在命令那一步点开）\n- 副标题：你说「${state.answer}」，${state.answer.includes("不") ? "这次先不动" : "下一句我接着改"}\n\n| 文件 | 改动 |\n| --- | --- |\n| \`${editPath}\` | +1 |`, "站主回答了，收尾。"),
      usage(150, 160, 8500, 0.00031),
      ...(verifying ? [verifyRound("passed", 0)] : []),
      context(o, 3400, 8600),
    ],
  ];
  return { rounds, edit: { path: editPath, before, after } };
}

type Scenario = "long" | "fix" | "repeat" | "stale" | "goal";

/** v0.6 的几个场景（长任务压缩、修好、反复失败、并发过期、目标模式）。 */
function scenarioRounds(
  scenario: Scenario, o: DemoOptions, check: DemoCheck,
  read: (id: string) => ReplayEvent[], editRound: (id: string, extra?: ReplayEvent[]) => ReplayEvent[], editPath: string, runId: string,
): Round[] {
  const W = o.window;
  const k = (f: number) => Math.round(W * f);
  if (scenario === "long") {
    const big = (id: string, path: string, kb: number): ReplayEvent[] => [
      tool(id, "read_file", "read", `读 ${path}`, { path }),
      done(id, `${num(Array.from({ length: 40 }, (_, i) => `// ${path} 第 ${i + 1} 行（演示：这个文件很长）`))}\n[读到第 2000 行，共 ${kb * 20} 行；从 2001 行接着读]`),
    ];
    const spilled = `/mock/harness-output/${runId}/ctx-1.log`;
    MOCK_SPILLS[spilled] = "（演示：压缩时落盘的完整工具输出）\n".repeat(400);
    return [
      [context(o, 600, 0), ...say(1, "这个任务要读的东西多，我先看结构。"), usage(4800, 90, 0, 0.0007), tool("l1", "list_directory", "read", "看 项目根目录", { path: ".", depth: 2 }), done("l1", "./\nsrc/\ndocs/\nfixtures/")],
      [context(o, 900, 1800), ...say(2, "", "先把两个大的夹具文件读一遍。"), usage(6000, 70, 4800, 0.0009), ...big("l2", "fixtures/events-2026-10.jsonl", 900)],
      [context(o, 1300, k(0.36)), ...say(3, ""), usage(k(0.36), 80, 6000, 0.05), ...big("l3", "fixtures/events-2026-11.jsonl", 1200)],
      [context(o, 1700, k(0.66)), ...say(4, "再跑一遍测试看看现在的状态。"), usage(k(0.3), 90, k(0.36), 0.04),
        tool("l4", "bash_run", "run", "跑 pnpm test", { command: "pnpm test" }), done("l4", "退出码 0（41.2s）\n[输出共 2.1 MB，太长：完整的存在 /mock/harness-output/l4.log]", { exitCode: 0, durationMs: 41200 })],
      [
        context(o, 2100, k(0.81)),
        { type: "context.compacted", step: "spill", before: k(0.83), after: k(0.71), text: "3 份长的工具输出落了盘，上下文里只留开头、结尾和路径", summary: null },
        { type: "context.compacted", step: "clear", before: k(0.71), after: k(0.64), text: "清掉了 2 份旧的工具输出（最近两轮的留着）", summary: null },
        { type: "context.compacted", step: "summary", before: k(0.64), after: 9800, text: "前面的对话换成了一份交接摘要（加上你最近说的几句）",
          summary: "## 目标\n把两个月的事件夹具按日期合并，测试照旧通过。\n\n## 做了什么\n- 读了 `fixtures/events-2026-10.jsonl`、`fixtures/events-2026-11.jsonl`（各 2000 行以内的部分）\n- `pnpm test` 通过（41s）\n\n## 现在\n两个文件的格式一样，按 `at` 排序就能合并。\n\n## 下一步\n改 `" + editPath + "`，加一行说明，再收尾。" },
        context(o, 2400, 7400),
        ...say(5, "压缩以后接着做：照交接摘要，改要改的那一处。"), usage(7400, 120, 0, 0.0011),
        ...editRound("l5"),
      ],
      [context(o, 2800, 8200), ...say(6, `接着做完了：\n\n- 上下文快满时自动压缩了一次（落盘 → 清旧输出 → 交接摘要），之后照摘要接着做\n- \`${editPath}\` 加了一行说明\n- 完整的工具输出都落了盘，点开那几步能看`), usage(900, 160, 7400, 0.0004), context(o, 3100, 8200)],
    ];
  }
  if (scenario === "fix") {
    return [
      [context(o, 600, 0), ...say(1, "", "先读要改的文件。"), usage(4300, 80, 0, 0.0006), ...read("f1")],
      [context(o, 1000, 1900), ...say(2, "改标题。"), usage(900, 120, 4300, 0.0003), ...editRound("f2", [verifyStart("v1", check, "edit")])],
      [context(o, 1400, 2300), ...say(3, "改好了。"), usage(500, 40, 5200, 0.0002),
        verifyDone("v1", check, "failed", `退出码 2（3.4s）\n${TS_ERROR}`, 3400), verifyRound("fixing", 1)],
      [context(o, 1900, 2900), ...say(4, "", "验证没过：第 3 行把数字赋给了 string。改回字符串。"), usage(800, 110, 5700, 0.0003),
        tool("f4", "write_patch", "edit", `改 ${editPath}`, { edits: [{ path: editPath, old_text: "count: 0", new_text: "count: \"0\"" }] }),
        done("f4", "改好了 1 处", { diff: [{ path: editPath, change: "modified", additions: 2, deletions: 1 }] }), verifyStart("v2", check, "edit", 1)],
      [context(o, 2300, 3100), ...say(5, `修好了：\n\n- \`${editPath}\` 开头加了一行说明\n- 第一次验证没过（第 3 行类型不对），改回字符串以后 \`${check.run}\` 过了`), usage(400, 130, 6500, 0.0003),
        verifyDone("v2", check, "passed", "退出码 0（3.1s）", 3100), verifyRound("passed", 1)],
    ];
  }
  if (scenario === "repeat") {
    const failOut = `退出码 2（3.3s）\n${TS_ERROR}`;
    return [
      [context(o, 600, 0), ...say(1, "改一下。"), usage(4300, 90, 0, 0.0006), ...editRound("r1", [verifyStart("v1", check, "edit")])],
      [context(o, 1000, 1200), ...say(2, "改好了。"), usage(600, 40, 4300, 0.0002), verifyDone("v1", check, "failed", failOut, 3300), verifyRound("fixing", 1)],
      [context(o, 1500, 2100), ...say(3, "", "可能是别处的问题，换个文件改。"), usage(900, 100, 4900, 0.0003),
        tool("r3", "write_patch", "edit", "改 src/types.ts", { edits: [{ path: "src/types.ts", old_text: "x", new_text: "y" }] }),
        done("r3", "改好了 1 处", { diff: [{ path: "src/types.ts", change: "modified", additions: 1, deletions: 1 }] }), verifyStart("v2", check, "edit", 1)],
      [context(o, 1900, 2600), ...say(4, "这次应该好了。"), usage(400, 40, 5800, 0.0002), verifyDone("v2", check, "failed", failOut, 3300),
        verifyRound("stopped", 1, false, `验证没过（${check.name}）：同一个失败连续两次，先停下。`),
        { type: "turn.completed", turnId: `turn-${runId}`, reason: "limit", error: `验证没过（${check.name}）：同一个失败连续两次，先停下。失败的输出在验证卡片里；接着说一句就能继续。` }],
    ];
  }
  if (scenario === "stale") {
    return [
      [context(o, 600, 0), ...say(1, "两处都要改，先改第一处。"), usage(4300, 90, 0, 0.0006), ...editRound("s1", [verifyStart("v1", check, "edit")])],
      [context(o, 1000, 1200), ...say(2, "", "第二处在同一个文件里，接着改。"), usage(700, 80, 4300, 0.0002),
        tool("s2", "write_patch", "edit", `改 ${editPath}`, { edits: [{ path: editPath, old_text: "a", new_text: "b" }] }),
        done("s2", "改好了 1 处", { diff: [{ path: editPath, change: "modified", additions: 2, deletions: 1 }] }),
        verifyDone("v1", check, "stale", "跑的时候文件又改了，这次的结果不算，按新的文件再跑。"),
        verifyStart("v2", check, "edit")],
      [context(o, 1400, 1700), ...say(3, "两处都改好了。"), usage(500, 60, 5000, 0.0002), verifyDone("v2", check, "passed", "退出码 0（3.0s）", 3000), verifyRound("passed", 0)],
    ];
  }
  // 目标模式
  return [
    [context(o, 600, 0), ...say(1, "", "目标是让检查过，先看现在的状态。"), usage(4300, 80, 0, 0.0006), ...read("g1")],
    [context(o, 1000, 1900), ...say(2, "现在的代码看起来没问题。"), usage(800, 60, 4300, 0.0003),
      verifyStart("v1", check, "goal"), verifyDone("v1", check, "failed", `退出码 2（3.4s）\n${TS_ERROR}`, 3400), verifyRound("fixing", 1, true)],
    [context(o, 1500, 2600), ...say(3, "", "目标没达到：第 3 行类型不对。"), usage(900, 120, 5100, 0.0003), ...editRound("g3", [verifyStart("v2", check, "edit", 1)])],
    [context(o, 1900, 3000), ...say(4, `目标达到了：\`${check.run}\` 过了（自动修了 1 次）。`), usage(400, 90, 6000, 0.0002), verifyDone("v2", check, "passed", "退出码 0（3.1s）", 3100), verifyRound("passed", 1, true)],
  ];
}

/** ONE SHOT 的剧本：读一个文件，答一段。CLI 的花费标「估」。 */
export function oneshotDemo(question: string, files: Record<string, string>, engine: string): Round[] {
  const path = Object.keys(files).find((p) => /readme/i.test(p)) ?? Object.keys(files)[0] ?? "README.md";
  const body = (files[path] ?? "").split("\n").slice(0, 20);
  const cli = engine !== "peck";
  return [
    [
      ...say(1, "", "先读一下项目说明。"),
      usage(2100, 60, 0, cli ? 0.004 : 0.00034, cli ? "estimated" : "priced"),
      tool("o1", "read_file", "read", `读 ${path}`, { path }),
      done("o1", num(body)),
    ],
    [
      ...say(2, `（演示回答）你问的是：「${question.slice(0, 40)}」。\n\n读了 \`${path}\`：这个项目的说明在开头几行；要改什么回到中栏发任务，ONE SHOT 只读、不改文件。`),
      usage(600, 140, 2100, cli ? 0.003 : 0.00022, cli ? "estimated" : "priced"),
    ],
  ];
}

/** 落盘的完整输出（`harnessOutputRead` 读这里）。 */
export const MOCK_SPILLS: Record<string, string> = {};
