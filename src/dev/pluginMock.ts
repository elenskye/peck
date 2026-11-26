// `?mock` 的插件演示数据（v1.1.0）：三个自带插件、库里一个完整插件（Git Plan）加一个单独的 Skill；
// 三个项目各一种状态——notebook 装着旧版（出 UPDATE）、peck 装着最新的、edge-gateway 没装。只在内存，刷新重置。

import type { CommandFile, PluginInfo, PluginList, ProjectPlugin, ProjectPlugins, SkillRow } from "@/lib/ipc";

const builtin = (name: string, title: string, description: string, skill: string, tools: Array<[string, string]>): PluginInfo => ({
  name, title, version: "1.0.0", description, kind: "plugin", source: "builtin", everyProject: false,
  skills: [{ name, description: skill }],
  commands: [],
  tools: tools.map(([n, d]) => ({ name: n, description: d, known: true })),
  hooks: [],
  mcp: [],
});

const MOCK_BUILTIN: PluginInfo[] = [
  builtin("peck-snapshots", "Snapshots", "撤销、回退某次任务的改动。Peck 在每个任务开始前、结束后各存一份快照；跟模型说一声「撤掉刚才的改动」，它列出快照、跟你确认，再放回去。",
    "站主要撤销、反悔、回退某次任务的改动时用：列出任务快照，确认后按任务号放回去",
    [["restore_snapshot", "列出这个项目最近的任务快照；按任务号回到它开始前（撤销）或结束后（重做）。动之前弹高危卡片，批了才动。"]]),
  builtin("peck-schedules", "Schedules", "定时任务。跟模型说「每周一早上九点检查依赖更新」，到点 Peck 替你在这个项目里发这句话；看、改、取消也在对话里说。",
    "站主要定时做一件事，或者要看、改、取消定时任务时用",
    [["schedule_task", "设或改一个定时任务：什么时候、到点发哪句话、谁来干。"], ["list_schedules", "列出这个项目的定时任务和下一次什么时候跑。"], ["cancel_schedule", "取消一个定时任务。"]]),
  builtin("peck-skills", "Skill Creator", "让模型把干活时摸出来的、以后用得上的做法提成一条 Skill。只是提议：你点 SAVE 才写进这个项目的 Skill 文件夹，只管这个项目。",
    "站主要你「存成 Skill」，或者摸出了一套以后还用得上的做法时用：怎么判断值不值得存、怎么写，再用 create_skill 提议",
    [["create_skill", "提议存一条 Skill（名字、一句描述、正文）。弹一张卡片，SAVE 才写进项目；同名的是替换，旧版留着能退回。"]]),
];

const gitPlan = (version: string): PluginInfo => ({
  name: "git-plan", title: "Git Plan", version, kind: "plugin", source: "library", everyProject: true,
  description: "版本号怎么升、什么时候分 Phase、提交说明怎么写；一轮做完给改到的每个 git 仓库各提交一笔，不 push。",
  skills: [{ name: "git-plan", description: "开工前定这次是哪种程度的开发、版本号怎么升、提交说明怎么写时用" }],
  commands: [
    { name: "git-plan:plan", description: "给一个新版本写 Phase 计划", hint: "<版本> <要做的>" },
    { name: "git-plan:release", description: "把项目里几处版本号一起改掉", hint: "<x.y.z>" },
    ...(version === "1.0.0" ? [] : [{ name: "git-plan:commits", description: "汇报还没推的提交：短哈希、说明、一句话做了什么", hint: null }]),
  ],
  tools: [{ name: "git_commit", description: "提交这一轮改过的文件。只会 commit，不 push、不改历史、不署名；说明不合格式的拒绝。", known: true }],
  hooks: [
    ...(version === "1.0.0" ? [] : [{ event: "Stop", kind: "command", ask: null, run: 'sh "${CLAUDE_PLUGIN_ROOT}/hooks/handoff.sh"' }]),
    { event: "Stop", kind: "peck", ask: "用一行概括这轮改了什么，按 Git Plan 的格式", run: "git_commit" },
  ],
  mcp: [],
});

const zhCopy: PluginInfo = {
  name: "zh-ui-copy", title: "zh-ui-copy", version: "1.0.0", kind: "skill", source: "library", everyProject: false,
  description: "把界面文案改成中文：先查用词对照，改完跑测试",
  skills: [{ name: "zh-ui-copy", description: "把界面文案改成中文：先查用词对照，改完跑测试" }],
  commands: [], tools: [], hooks: [], mcp: [],
};

const LATEST = "1.2.0";
const everyProject: Record<string, boolean> = { "git-plan": true, "zh-ui-copy": false };
/** 每个项目装着的：名字 → 版本。 */
const installed: Record<string, Record<string, string>> = {
  "ws-notebook": { "git-plan": "1.0.0" },
  "ws-peck": { "git-plan": LATEST },
};
const libraryNow = (): PluginInfo[] => [gitPlan(LATEST), zhCopy].map((p) => ({ ...p, everyProject: everyProject[p.name] ?? p.everyProject }));

export function mockPluginList(): PluginList {
  return { builtin: MOCK_BUILTIN, library: libraryNow() };
}

export function mockEveryProject(name: string, on: boolean): PluginList {
  everyProject[name] = on;
  return mockPluginList();
}

export function mockProjectPlugins(workspaceId: string, pluginsOff: string[]): ProjectPlugins {
  const mine = installed[workspaceId] ?? {};
  const library: ProjectPlugin[] = libraryNow().map((p) => {
    const have = mine[p.name];
    const shown = have && p.name === "git-plan" ? { ...gitPlan(have), source: "project" as const, everyProject: p.everyProject } : p;
    return { ...shown, off: false, installed: have ?? null, latest: p.version, update: !!have && have !== p.version };
  });
  return {
    builtin: MOCK_BUILTIN.map((p) => ({ ...p, off: pluginsOff.includes(p.name), installed: null, latest: null, update: false })),
    library,
    skillsDir: ".claude/skills",
  };
}

export function mockInstall(workspaceId: string, name: string): void {
  const item = libraryNow().find((p) => p.name === name);
  if (!item) throw new Error(`插件库里没有 ${name}。`);
  (installed[workspaceId] ??= {})[name] = item.version;
}

export function mockRemove(workspaceId: string, name: string): void {
  if (!installed[workspaceId]?.[name]) throw new Error(`这个项目里没装 ${name}。`);
  delete installed[workspaceId][name];
}

/** 这个项目里装着的插件带的命令（`/` 列出来的）。 */
export function mockPluginCommands(workspaceId: string): CommandFile[] {
  const have = installed[workspaceId]?.["git-plan"];
  return have ? gitPlan(have).commands.map((c) => ({ ...c, plugin: "Git Plan" })) : [];
}

export function mockExpand(workspaceId: string, name: string, args: string): string {
  if (!mockPluginCommands(workspaceId).some((c) => c.name === name)) throw new Error(`没有 /${name} 这个命令。`);
  if (name === "git-plan:plan") return `按 Git Plan 给这个版本写 Phase 计划，写进项目的路线文档，先不动代码。\n\n${args}`.trim();
  if (name === "git-plan:release") return `把项目里写版本号的几处一起改成 ${args || "下一个版本"}，改完列出改了哪几处。`;
  return "列出本地还没推的提交：短哈希、说明、一句话做了什么。不要 push。";
}

/** 装着的插件、库里的 Skill 带进项目的 Skill（项目设置 SKILLS 里和项目自己的列在一起）。 */
export function mockPluginSkillRows(workspaceId: string): SkillRow[] {
  const mine = installed[workspaceId] ?? {};
  const day = 86_400_000;
  const rows: SkillRow[] = [];
  if (mine["git-plan"]) rows.push({ name: "git-plan", description: gitPlan(LATEST).skills[0].description, scope: "project", plugin: "git-plan", off: false, loads: 4, lastUsed: Date.now() - day, changedAt: Date.now() - 2 * day, stale: false, versions: 0 });
  if (mine["zh-ui-copy"]) rows.push({ name: "zh-ui-copy", description: zhCopy.description, scope: "project", plugin: null, off: false, loads: 0, lastUsed: null, changedAt: Date.now(), stale: false, versions: 0 });
  return rows;
}
