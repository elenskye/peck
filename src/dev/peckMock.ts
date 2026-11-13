// `?mock` 里 Peck 循环的剧本（和 Rust 的 ScriptedModel::peck_demo 同一个流程）：
// 看目录 → 读 → 搜 → 改 → 跑一条输出很长的命令（落盘）→ 删 dist（高危卡片）→ 清单 → 问一句 → 收尾。
// 一轮是一次模型请求：先出思考和字，再报用量和花费，然后是这一轮要的工具。
import type { AgentCaps, EventBody, ToolKind } from "@/lib/agentEvents";
import type { ReplayEvent } from "@/dev/gatewayMock";

export interface RoundState {
  /** 最近一次 ASK 的回答。 */
  answer: string;
}

export type Round = ReplayEvent[] | ((state: RoundState) => ReplayEvent[]);

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

/** Peck 循环的演示剧本。`files` 是这个项目的假文件（路径 → 内容）。 */
export function peckDemo(runId: string, files: Record<string, string>): { rounds: Round[]; edit: { path: string; before: string; after: string } } {
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
  const rounds: Round[] = [
    [
      ...say(1, "我先看一下项目结构。", "先看项目里有什么，再找要改的地方。"),
      usage(4200, 120, 0, 0.00061),
      tool("c1", "list_directory", "read", "看 项目根目录", { path: ".", depth: 2 }),
      done("c1", `./\n${top.join("\n")}`),
    ],
    [
      ...say(2, ""),
      usage(900, 60, 4200, 0.00021),
      tool("c2", "read_file", "read", `读 ${editPath}`, { path: editPath }),
      done("c2", num(lines.slice(0, 40))),
    ],
    [
      ...say(3, "", `要改的在 ${editPath} 开头。搜一下别处有没有一样的写法。`),
      usage(1400, 80, 5100, 0.00028),
      tool("c3", "search_text", "search", `搜「${word}」`, { pattern: word, mode: "content" }),
      done("c3", `${editPath}:1: ${first}\n（1 个文件，1 处）`),
    ],
    [
      ...say(4, "找到了，在开头加一行说明。"),
      usage(600, 140, 6500, 0.00024),
      tool("c4", "write_patch", "edit", `改 ${editPath}`, { edits: [{ path: editPath, old_text: first, new_text: `// 待办：标题改成中文（Peck 循环演示）\n${first}` }] }),
      done("c4", `改好了 1 处：\n\n${editPath} 第 1 行：\n${num(after.split("\n").slice(0, 4))}`, { diff: [{ path: editPath, change: "modified", additions: 1, deletions: 0, patch }] }),
      { type: "diff.updated", files: [{ path: editPath, change: "modified", additions: 1, deletions: 0, patch }] },
    ],
    [
      ...say(5, "跑一下测试。"),
      usage(300, 40, 7100, 0.00013),
      tool("c5", "bash_run", "run", "跑 pnpm test", { command: "pnpm test", timeout_sec: 120 }),
      ...chunks(testLog.slice(0, 1800), 300).map((text): ReplayEvent => ({ type: "tool_call.output_delta", callId: "c5", text })),
      done("c5", shown, { exitCode: 0, durationMs: 6400 }),
    ],
    [
      ...say(6, "测试过了。顺手把旧的构建产物 dist/ 删掉。"),
      usage(500, 60, 7300, 0.00017),
      tool("c6", "bash_run", "run", "跑 rm -rf dist", { command: "rm -rf dist" }),
      { type: "approval.requested", requestId: "approve-1", callId: "c6", kind: "run", title: "高危：删文件", detail: "rm -rf dist", highRisk: true, options: ["allow_once", "deny_once"] },
      done("c6", "退出码 0（0.1s）", { exitCode: 0, durationMs: 120 }),
    ],
    [
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
    ],
    [
      ...say(8, ""),
      usage(200, 50, 8300, 0.00019),
      tool("c8", "ask_user", "other", "问你", { question: "副标题也要改成中文吗？", options: ["一起改", "先不改"] }),
      { type: "user_input.requested", requestId: "ask-1", question: "副标题也要改成中文吗？", options: ["一起改", "先不改"], freeText: true },
      done("c8", "站主回答：（见上）"),
    ],
    (state) => [
      ...say(9, `改好了：\n\n- \`${editPath}\` 开头加了一行说明\n- \`pnpm test\` 通过（4200 个测试；完整日志在命令那一步点开）\n- 副标题：你说「${state.answer}」，${state.answer.includes("不") ? "这次先不动" : "下一句我接着改"}\n\n| 文件 | 改动 |\n| --- | --- |\n| \`${editPath}\` | +1 |`, "站主回答了，收尾。"),
      usage(150, 160, 8500, 0.00031),
    ],
  ];
  return { rounds, edit: { path: editPath, before, after } };
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
