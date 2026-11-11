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

export type DotKind = "muse" | "grok" | "api" | "";
export interface WhoOption {
  id: string;
  /** 点阵字：英文大写 */
  label: string;
  /** 一步一行：谁、色点、干什么 */
  flow: Array<{ name: string; dot: DotKind; does: string }>;
}

const NAME: Record<string, string> = { muse: "Muse", grok: "Grok", mock: "假 Agent" };
export const agentName = (id: string, fallback?: string) => NAME[id.toLowerCase()] ?? fallback ?? id;
export const agentDot = (id: string): DotKind =>
  id === "muse" || id === "grok" ? id : id.startsWith("api") ? "api" : "";

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

/** 一个任务是谁干的：点阵字的短名字，旧记录也认。 */
export function whoLabel(run: Pick<RunRecord, "flowId" | "who" | "steps">): string {
  const id = run.who ?? (run.flowId.startsWith("single-") ? run.flowId.slice(7) : run.flowId);
  const known = [...CLI_WHO, FAKE_WHO, ...API_WHO].find((w) => w.id === id);
  if (known) return known.label;
  if (run.flowId === "idea-to-code" || run.flowId.startsWith("pipe-") || run.flowId === "muse-grok") {
    const ids = run.steps.map((s) => agentName(s.agentId, s.agentName).toUpperCase());
    if (ids.length) return ids.join("→");
  }
  return (run.steps[0]?.agentName ?? id).toUpperCase();
}

export function whoDots(run: Pick<RunRecord, "steps" | "flowId">): DotKind[] {
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
  return `${agentName(step.agentId, step.agentName)} ${title}`;
}
// 旧记录里的步骤名换成现在的说法。
const STEP_TITLES: Record<string, string> = {
  意图与任务生成: "写规范",
  代码构建与实施: "实施",
  "Supervisor 规划、审查与接管": "规划和审查",
  "单 Worker 开发与验证": "单模型干活",
};

export const PERMISSIONS: Array<{ id: Permission; label: string; name: string; does: string }> = [
  { id: "plan", label: "PLAN", name: "只读", does: "只读：读代码、写计划，不改文件、不跑命令。要动手，换 EDIT 再发。" },
  { id: "edit", label: "EDIT", name: "改文件、跑命令不问", does: "改文件、跑命令都不问；删文件、联网装依赖、改项目外的文件，开始前一张卡一次批。" },
  { id: "auto", label: "AUTO", name: "全部放行", does: "装依赖、联网也不问；删文件、改项目外的文件（推送也算）照样一张卡批。" },
];

export const STATUS_WORD: Record<string, string> = {
  running: "在跑",
  succeeded: "完成",
  failed: "失败",
  cancelled: "已停止",
  undone: "已撤销",
};
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
