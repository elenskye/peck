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

/** CLI 版本号：从 `--version` 的原样输出（「codex-cli 0.167.2」「2.1.322 (Claude Code)」）里取出数字那一段。 */
export const versionNumber = (raw?: string | null): string => raw?.match(/\d+\.\d+(\.\d+)?/)?.[0] ?? "";

/** 显示用：`v0.167.2`，取不到就空。 */
export const versionText = (raw?: string | null): string => { const v = versionNumber(raw); return v ? `v${v}` : ""; };

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// ---------- 谁来干 ----------

/** 身份色点：五家 CLI 各一色，API（Peck Loop）一色；空的是不上色的点。 */
export type DotKind = "muse" | "grok" | "codex" | "claude-code" | "antigravity" | "api" | "";
const ENGINE_DOTS: DotKind[] = ["muse", "grok", "codex", "claude-code", "antigravity"];
/** 引擎 id → 色点；不认识的是空的。 */
export const engineDot = (engine: string): DotKind => (ENGINE_DOTS.includes(engine as DotKind) ? (engine as DotKind) : "");
const NAME: Record<string, string> = { muse: "Muse", grok: "Grok", codex: "Codex", "claude-code": "Claude Code", antigravity: "Antigravity", peck: "Peck" };
export const agentName = (id: string, fallback?: string) => NAME[id.toLowerCase()] ?? fallback ?? id;

// 模型列表（gateway_models）里每一项的显示名：App 拿到列表时记一下，whoLabel 用。
const ENTRY_LABELS = new Map<string, string>();
export function rememberEntries(entries: Array<{ id: string; label: string }>): void {
  for (const e of entries) ENTRY_LABELS.set(e.id, e.label);
}

/** 模型列表里一项的身份色点：API 模型蓝，CLI 各一色。 */
export function entryDot(id: string): DotKind {
  if (id.startsWith("api:")) return "api";
  return engineDot(id.replace(/^cli:/, ""));
}

/** 「谁来干」（api:、cli:）的显示名。 */
export function entryLabel(id: string): string {
  const known = ENTRY_LABELS.get(id);
  if (known) return known;
  if (id.startsWith("api:")) return id.slice(4).split("/").pop() ?? id;
  if (id.startsWith("cli:")) return agentName(id.slice(4));
  return agentName(id);
}

/** 合作的任务（v0.8）记的是 `team:<写规范>|<实施>|<审查>`。 */
export function teamParts(who: string): { spec: string; build: string; review: string } | null {
  if (!who.startsWith("team:")) return null;
  const [spec = "", build = "", review = ""] = who.slice(5).split("|");
  return { spec, build, review };
}

/** 一个任务是谁干的：点阵字的短名字。v0.8 以前的旧记录没有「谁」，写「旧记录」。 */
export function whoLabel(run: Pick<RunRecord, "who">): string {
  const team = run.who ? teamParts(run.who) : null;
  if (team) {
    const name = (id: string) => entryLabel(id).toUpperCase();
    return [team.spec ? `${name(team.spec)} → ${name(team.build)}` : name(team.build), team.review && `审 ${name(team.review)}`].filter(Boolean).join(" · ");
  }
  return run.who ? entryLabel(run.who).toUpperCase() : "旧记录";
}

export function whoDots(run: Pick<RunRecord, "who">): DotKind[] {
  const team = run.who ? teamParts(run.who) : null;
  if (team) return [...new Set([team.spec, team.build, team.review].filter(Boolean).map((id) => entryDot(id)).filter(Boolean))];
  const d = run.who ? entryDot(run.who) : "";
  return d ? [d] : [];
}

/** 步骤的名字：Peck Loop、CLI 引擎的任务步骤名就是「Peck · DeepSeek V4 Pro」「Codex」，不再重复。 */
export function stepName(step: StepDescriptor): string {
  const name = agentName(step.agentId, step.agentName);
  return step.title.startsWith(name) ? step.title : `${name} ${step.title}`;
}

export const PERMISSIONS: Array<{ id: Permission; label: string; name: string; does: string }> = [
  { id: "plan", label: "PLAN", name: "只读", does: "只读：读代码、搜、写计划，不改文件；命令在只读沙箱里跑。要动手，换 EDIT 再发。" },
  { id: "edit", label: "EDIT", name: "改文件、跑命令不问", does: "改文件、跑命令不问；装依赖、起常驻进程前问一次，能选「这个项目总是允许」。命令默认联网，只能写项目里。" },
  { id: "auto", label: "AUTO", name: "除了高危都不问", does: "装依赖、起常驻进程也不问；高危的（删文件、推送、写到项目外这些）不管哪档都弹卡片。" },
];

export const STATUS_WORD: Record<string, string> = {
  running: "在跑",
  succeeded: "完成",
  failed: "失败",
  cancelled: "已停止",
  undone: "已撤销",
};
/** 复制到剪贴板（v0.8）：先用 navigator.clipboard，不行退回 execCommand。 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch { /* 下面再试 */ }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  const ok = document.execCommand("copy");
  area.remove();
  if (!ok) throw new Error("复制不了：剪贴板没开放");
}

/** Peck Loop（`api:<供应商>/<模型>`）跑的。 */
export function isPeckRun(run: Pick<RunRecord, "who">): boolean {
  return !!run.who?.startsWith("api:");
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

/** 「总是允许」那一条怎么写给站主看（项目设置、设置 → 权限）。 */
export function ruleWord(rule: string): string {
  if (rule === "install-deps") return "装依赖";
  if (rule === "long-running") return "起常驻进程";
  const project = /^mcp-project:([^:]+):/.exec(rule);
  if (project) return `起项目里的 MCP ${project[1]}`;
  if (rule.startsWith("mcp:")) return `MCP ${rule.slice(4)} 会改东西的工具`;
  return rule;
}
