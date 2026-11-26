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
  { name: "review", description: "用现在这个审查上一次的改动，只读", kind: "peck", instant: true },
  { name: "compact", description: "压缩这个任务的对话：落盘、清旧输出、写交接摘要（叫一次模型）；后面可以写要重点留住什么。CLI 的转给它自己压", hint: "[重点]", kind: "peck" },
  { name: "undo", description: "撤销上一次的改动：交给模型用任务快照放回去，动之前等你批", kind: "peck", instant: true },
  { name: "cost", description: "这个任务、这个月花了多少", kind: "peck", instant: true },
];

/** CLI 自己的交互式命令：Peck 不转发。 */
export const CLI_ONLY = new Set([
  "model", "login", "logout", "config", "permissions", "mcp", "hooks", "init", "resume", "clear", "exit", "quit", "help",
  "status", "doctor", "agents", "memory", "theme", "vim", "terminal-setup", "bug", "release-notes", "ide", "upgrade",
  "usage", "add-dir", "export", "rewind", "context", "statusline", "settings", "approvals", "diff", "fork", "skills",
]);

/** 光标前的 `/查询词`（v1.2.0 接上提示词）：`/` 在整句开头或空格、换行后面，光标停在这个词末尾（后面是空白或没字了）才弹。
 *  `head`：整句只有它，命令在前、提示词在后；不然只列提示词。`/` 前面不是空白的（网址、路径、`a/b`）不弹。
 *  `start` 是 `/` 的位置，选了提示词就把 `start` 到光标换掉。 */
export function slashAt(text: string, cursor = text.length): { start: number; query: string; head: boolean } | null {
  if (cursor < 0 || cursor > text.length) return null;
  const after = text.slice(cursor);
  if (after && !/^\s/.test(after)) return null;
  const m = /(?:^|\s)\/([^\s/]{0,130})$/.exec(text.slice(0, cursor));
  if (!m) return null;
  const start = cursor - m[1].length - 1;
  return { start, query: m[1], head: start === 0 && !after.trim() };
}

/** 这句话是不是一个斜杠命令：`/名字` 后面空格接参数（可以换行）。 */
export function parseSlash(text: string): { name: string; args: string } | null {
  const m = text.trim().match(/^\/([A-Za-z0-9_.:-]{1,130})(?:\s+([\s\S]*))?$/);
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
