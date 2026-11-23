// 输入框的斜杠命令（v0.10 C2）：Peck 自己的几个（不带前缀），加上插件带的（v1.1.0，一律 `/<插件名>:<命令>`）。
// 插件的命令选了以后把正文展开进这句话（后端 slash_expand），哪家引擎都能用；CLI 自己的交互式命令（/model、/login）不转发。

import type { RunRecord } from "@/lib/types";

export interface SlashItem {
  name: string;
  description: string;
  hint?: string | null;
  /** peck：Peck 自己的；file：插件带的。 */
  kind: "peck" | "file";
  /** 插件带的：哪个插件（界面上的名字）。 */
  plugin?: string | null;
  /** 选了马上做（不用再写参数）。 */
  instant?: boolean;
}

export const PECK_COMMANDS: SlashItem[] = [
  { name: "plan", description: "切到 PLAN（只读）；后面写了话就用 PLAN 发出去", hint: "[这句话]", kind: "peck" },
  { name: "edit", description: "切到 EDIT；后面写了话就用 EDIT 发出去", hint: "[这句话]", kind: "peck" },
  { name: "auto", description: "切到 AUTO；后面写了话就用 AUTO 发出去", hint: "[这句话]", kind: "peck" },
  { name: "new", description: "这个项目里新开一个任务", kind: "peck", instant: true },
  { name: "review", description: "审查上一次的改动（AGENTS 里选的审查；没选就用现在这个），只读", kind: "peck", instant: true },
  { name: "compact", description: "压一压这个任务的对话（Peck Loop；不叫模型、不花钱）", kind: "peck", instant: true },
  { name: "undo", description: "撤销上一次的改动：交给模型用任务快照放回去，动之前等你批", kind: "peck", instant: true },
  { name: "cost", description: "这个任务、这个月花了多少", kind: "peck", instant: true },
];

/** CLI 自己的交互式命令：Peck 不转发。 */
export const CLI_ONLY = new Set([
  "model", "login", "logout", "config", "permissions", "mcp", "hooks", "init", "resume", "clear", "exit", "quit", "help",
  "status", "doctor", "agents", "memory", "theme", "vim", "terminal-setup", "bug", "release-notes", "ide", "upgrade",
  "usage", "add-dir", "export", "rewind", "context", "statusline", "settings", "approvals", "diff", "fork", "skills",
]);

/** 整句只有 `/` 加几个字（还没写参数）：弹列表。 */
export function slashAt(draft: string): { query: string } | null {
  const m = draft.match(/^\/([A-Za-z0-9_.:-]{0,64})$/);
  return m ? { query: m[1] } : null;
}

/** 这句话是不是一个斜杠命令：`/名字` 后面空格接参数（可以换行）。 */
export function parseSlash(text: string): { name: string; args: string } | null {
  const m = text.trim().match(/^\/([A-Za-z0-9_.:-]{1,64})(?:\s+([\s\S]*))?$/);
  return m ? { name: m[1], args: (m[2] ?? "").trim() } : null;
}

/** 按打的字筛：名字开头的在前，再是名字、描述里有的。 */
export function matchSlash(items: SlashItem[], query: string): SlashItem[] {
  const q = query.toLowerCase();
  if (!q) return items;
  const starts = items.filter((i) => i.name.toLowerCase().startsWith(q));
  const rest = items.filter((i) => !starts.includes(i) && (i.name.toLowerCase().includes(q) || i.description.toLowerCase().includes(q)));
  return [...starts, ...rest];
}

/** 这个任务里最近一次改过文件、还没撤销的那一次。 */
export function lastChanged(runs: RunRecord[]): { run: RunRecord; index: number } | null {
  for (let i = runs.length - 1; i >= 0; i--) {
    const r = runs[i];
    if (r.status !== "running" && !r.undoneAt && (r.files?.length ?? 0) > 0) return { run: r, index: i };
  }
  return null;
}

/** `/undo` 发出去的那句话（v1.0.2：撤销交给模型，它用 `restore_snapshot`，动之前弹卡片等站主批）。 */
export function undoPrompt(number: number | undefined, args: string): string {
  if (args) return `撤销改动：${args}`;
  return `撤销任务 ${number} 的改动：把它改过的文件放回它开始前的样子。`;
}

/** `/review` 发出去的那句话：只读地审上一次的改动，最后一行写结论。 */
export function reviewPrompt(run: RunRecord, index: number): string {
  const files = (run.files ?? []).map((f) => `- ${f.path}（${f.kind === "create" ? "新建" : f.kind === "delete" ? "删了" : "改了"}）`).join("\n");
  const asked = run.prompt.replace(/\s+/g, " ").slice(0, 120);
  return [
    `审查这个任务第 ${index + 1} 次的改动（当时说的：「${asked}」）。只读，不改文件。`,
    "",
    "改了的文件：",
    files,
    "",
    "看这些文件现在的样子（git 项目可以用 git diff 对照），逐条说有没有问题：做对了没有、漏了什么、会不会弄坏别的地方、有没有更简单的写法。",
    "最后一行只写 `VERDICT: PASS` 或 `VERDICT: CHANGES`。",
  ].join("\n");
}
