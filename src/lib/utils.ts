import type { Permission, RunRecord, StepDescriptor } from "@/lib/types";

export function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(" ");
}

const pad = (n: number) => String(n).padStart(2, "0");

/** 14:02 */
export function clock(ts: number): string {
  const d = new Date(ts);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 刚才、3 分钟前、今天 09:12、昨天、11-10 14:02 */
export function ago(ts: number, now = Date.now()): string {
  const diff = now - ts;
  if (diff < 60_000) return "刚才";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  const d = new Date(ts);
  const today = new Date(now);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(today) - day(d)) / 86_400_000);
  if (days === 0) return `今天 ${clock(ts)}`;
  if (days === 1) return "昨天";
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${clock(ts)}`;
}

/** 48s、1:02、1:02:03 */
export function span(ms?: number | null): string {
  if (ms == null) return "";
  const total = Math.max(0, Math.round(ms / 1000));
  if (total < 60) return `${total}s`;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** 计时：02:14 */
export function timer(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(total / 60);
  return `${pad(m)}:${pad(total % 60)}`;
}

/** 站主发的话是 Markdown：当任务名、通知正文时去掉记号，并成一行。 */
export function plainLine(markdown: string): string {
  return markdown
    .replace(/```[^\n]*\n?/g, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}(?:#{1,6}\s+|>\s?|[-*+]\s+(?:\[[ xX]\]\s+)?|\d+[.)]\s+)/gm, "")
    .replace(/(\*\*|__|~~|`)(.+?)\1/g, "$2")
    .replace(/(^|[^\w*])[*_](\S(?:.*?\S)?)[*_](?=[^\w*]|$)/g, "$1$2")
    .replace(/\s+/g, " ")
    .trim();
}

export function shortenHomePath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~").replace(/^\/home\/[^/]+/, "~");
}

export function isMarkdownFile(filePath: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(filePath);
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// ---------- 谁来干 ----------

/** 身份色点：五家 CLI 各一色，API（Peck 循环、规划 + 实施）一色；空的是不上色的点。 */
export type DotKind = "muse" | "grok" | "codex" | "claude-code" | "antigravity" | "api" | "";
const ENGINE_DOTS: DotKind[] = ["muse", "grok", "codex", "claude-code", "antigravity"];
/** 引擎 id → 色点；不认识的是空的。 */
export const engineDot = (engine: string): DotKind => (ENGINE_DOTS.includes(engine as DotKind) ? (engine as DotKind) : "");
export interface WhoOption {
  id: string;
  /** 点阵字：英文大写 */
  label: string;
  /** 一步一行：谁、色点、干什么 */
  flow: Array<{ name: string; dot: DotKind; does: string }>;
}

const NAME: Record<string, string> = { muse: "Muse", grok: "Grok", mock: "假 Agent", codex: "Codex", "claude-code": "Claude Code", antigravity: "Antigravity", peck: "Peck" };
export const agentName = (id: string, fallback?: string) => NAME[id.toLowerCase()] ?? fallback ?? id;
export const agentDot = (id: string): DotKind => (id.startsWith("api") ? "api" : engineDot(id));

export const CLI_WHO: WhoOption[] = [
  { id: "muse", label: "MUSE", flow: [{ name: "Muse", dot: "muse", does: "一个人跑完整个任务：读代码、改文件、自己检查。" }] },
  { id: "grok", label: "GROK", flow: [{ name: "Grok", dot: "grok", does: "一个人跑完整个任务：读代码、改文件、自己检查。" }] },
  {
    id: "muse>grok",
    label: "MUSE→GROK",
    flow: [
      { name: "Muse", dot: "muse", does: "先写规范：要改什么，拆成一条条任务。" },
      { name: "Grok", dot: "grok", does: "照规范实施，做完一条勾一条。" },
    ],
  },
  {
    id: "grok>muse",
    label: "GROK→MUSE",
    flow: [
      { name: "Grok", dot: "grok", does: "先写规范：要改什么，拆成一条条任务。" },
      { name: "Muse", dot: "muse", does: "照规范实施，做完一条勾一条。" },
    ],
  },
];
export const FAKE_WHO: WhoOption = {
  id: "mock",
  label: "FAKE",
  flow: [{ name: "假 Agent", dot: "", does: "不推理，照剧本写几个演示文件，几秒跑完。只在开发和验收时用。" }],
};
export const API_WHO: WhoOption[] = [
  { id: "api-worker", label: "SOLO", flow: [{ name: "单模型干活", dot: "api", does: "一个模型读代码、改文件、跑检查，最后自己总结。" }] },
  {
    id: "api-supervisor",
    label: "PLAN→DO",
    flow: [
      { name: "规划的模型", dot: "api", does: "拆成几项交给实施的模型，做完逐项审查：通过、返工或接手。" },
      { name: "实施的模型", dot: "api", does: "只拿到目标、要改的文件和验收条件，做完交回。" },
    ],
  },
];

// 模型列表（gateway_models）里每一项的显示名：App 拿到列表时记一下，whoLabel 用。
const ENTRY_LABELS = new Map<string, string>();
export function rememberEntries(entries: Array<{ id: string; label: string }>): void {
  for (const e of entries) ENTRY_LABELS.set(e.id, e.label);
}

/** 模型列表里一项的身份色点：API 模型蓝，Muse 紫，Grok 橙，别的 CLI 不上色。 */
export function entryDot(id: string): DotKind {
  if (id.startsWith("api:") || id === "flow:plan-implement") return "api";
  return engineDot(id.replace(/^(cli|flow):/, "").split(">")[0]);
}

/** 新的「谁来干」（api:、cli:、flow:）的显示名。 */
export function entryLabel(id: string): string {
  const known = ENTRY_LABELS.get(id);
  if (known) return known;
  if (id.startsWith("api:")) return id.slice(4).split("/").pop() ?? id;
  if (id === "flow:plan-implement") return "规划 + 实施";
  if (id.startsWith("flow:")) return id.slice(5).split(">").map((x) => agentName(x)).join(" → ");
  if (id.startsWith("cli:")) return agentName(id.slice(4));
  return agentName(id);
}

/** 一个任务是谁干的：点阵字的短名字，旧记录也认。 */
export function whoLabel(run: Pick<RunRecord, "flowId" | "who" | "steps">): string {
  if (run.who && /^(api|cli|flow):/.test(run.who)) return entryLabel(run.who).toUpperCase();
  const id = run.who ?? (run.flowId.startsWith("single-") ? run.flowId.slice(7) : run.flowId);
  const known = [...CLI_WHO, FAKE_WHO, ...API_WHO].find((w) => w.id === id);
  if (known) return known.label;
  if (run.flowId === "idea-to-code" || run.flowId.startsWith("pipe-") || run.flowId === "muse-grok") {
    const ids = run.steps.map((s) => agentName(s.agentId, s.agentName).toUpperCase());
    if (ids.length) return ids.join("→");
  }
  return (run.steps[0]?.agentName ?? id).toUpperCase();
}

export function whoDots(run: Pick<RunRecord, "steps" | "flowId" | "who">): DotKind[] {
  if (run.who?.startsWith("flow:") && run.who !== "flow:plan-implement") return run.who.slice(5).split(">").map((x) => entryDot(`cli:${x}`)).filter(Boolean);
  if (run.who && /^(api|cli|flow):/.test(run.who)) { const d = entryDot(run.who); return d ? [d] : []; }
  if (run.flowId.startsWith("api-")) return ["api"];
  const seen: DotKind[] = [];
  for (const step of run.steps) {
    const dot = agentDot(step.agentId);
    if (dot && !seen.includes(dot)) seen.push(dot);
  }
  return seen;
}

/** 步骤的名字：「Muse 写规范」。API 模式的步骤名字本身就够。 */
export function stepName(step: StepDescriptor): string {
  const title = STEP_TITLES[step.title] ?? step.title;
  if (step.agentId.startsWith("api-")) return `${step.agentName} · ${title}`;
  const name = agentName(step.agentId, step.agentName);
  // Peck 循环、CLI 引擎的任务：步骤名就是「Peck · DeepSeek V4 Pro」「Codex」，不再重复。
  return title.startsWith(name) ? title : `${name} ${title}`;
}
// 旧记录里的步骤名换成现在的说法。
const STEP_TITLES: Record<string, string> = {
  意图与任务生成: "写规范",
  代码构建与实施: "实施",
  "Supervisor 规划、审查与接管": "规划和审查",
  "单 Worker 开发与验证": "单模型干活",
};

export const PERMISSIONS: Array<{ id: Permission; label: string; name: string; does: string }> = [
  { id: "plan", label: "PLAN", name: "只读", does: "只读：读代码、搜、写计划，不改文件；命令在只读沙箱里跑。要动手，换 EDIT 再发。" },
  { id: "edit", label: "EDIT", name: "改文件、跑命令不问", does: "改文件、跑命令不问；装依赖、起常驻进程前问一次，能选「这个项目总是允许」。命令默认联网，只能写项目里。" },
  { id: "auto", label: "AUTO", name: "除了高危都不问", does: "装依赖、起常驻进程也不问。删文件、推送和改历史、写到项目外、改系统设置、看不出会做什么的命令，不管哪档都弹卡片。" },
];

export const STATUS_WORD: Record<string, string> = {
  running: "在跑",
  succeeded: "完成",
  failed: "失败",
  cancelled: "已停止",
  undone: "已撤销",
};
/** 这个任务的过程是统一事件（Peck 循环、CLI 引擎）：界面按事件显示，不按旧的步骤日志。 */
export function isAgentRun(run: Pick<RunRecord, "who">): boolean {
  return !!run.who && (run.who.startsWith("api:") || run.who.startsWith("cli:"));
}

export function runState(run: { status: string; undoneAt?: number | null }): keyof typeof STATUS_WORD {
  return run.undoneAt ? "undone" : run.status;
}

/** 把终端输出里的控制字符收掉：颜色、光标移动不解释；回车覆盖同一行。 */
export function plainTerminal(text: string): string {
  const cleaned = text
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, "")
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b\][^\u0007\u001b]*(\u0007|\u001b\\)/g, "")
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b[@-Z\\-_]/g, "")
    .replace(/\r\n/g, "\n");
  return cleaned
    .split("\n")
    .map((line) => {
      const parts = line.split("\r");
      return parts[parts.length - 1] || parts[parts.length - 2] || "";
    })
    .join("\n")
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "");
}
