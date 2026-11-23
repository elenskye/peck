// 合作预设（v0.9 Phase 14 B2）：AGENTS 里存几套组合（写规范 / 实施 / 审查 / 委派各选谁），一键切换。
// 预设只是「把 AGENTS 那几行设成这样」：套用以后改的还是 settings 里的 pick / relay / review / delegate，
// 发任务的逻辑不变。自带两套（能改、不能删）；预设里的谁现在用不了（没 Key、CLI 没检测到）标灰并写原因。
import type { ModelEntry } from "@/lib/agentEvents";
import type { AppSettings } from "@/lib/settings";

export interface Preset {
  id: string;
  name: string;
  /** 写规范的（有就是接力）。 */
  spec?: string;
  /** 实施的。 */
  build: string;
  /** 审查的（空着不审查）。 */
  review?: string;
  /** 委派给谁（空着不委派；实施的是 Antigravity 时不委派）。 */
  delegate?: string;
  /** 自带的：能改、不能删。 */
  builtin?: boolean;
}

/** 站主 2026-11-15 定的两套（roadmap B2）。 */
export const BUILTIN_PRESETS: Preset[] = [
  { id: "builtin:cheap", name: "省钱", build: "api:deepseek/deepseek-flash", review: "cli:claude-code", delegate: "api:deepseek/deepseek-flash", builtin: true },
  { id: "builtin:steady", name: "稳妥", spec: "cli:claude-code", build: "cli:codex", review: "api:anthropic/claude-sonnet-5-5", builtin: true },
];

export const RELAY = "relay";

/** 实施的这一个能不能委派（v0.9 B1）：API 模型（Peck Loop）或经本机 MCP 的 CLI——Claude Code、Codex、Grok、Muse
 *  （Muse 经会话 MCP，v0.10 接上；文件夹要在 Muse 里信任过）。Antigravity 只做直接调用。 */
export function canDelegate(entry: Pick<ModelEntry, "id" | "directOnly"> | null | undefined): boolean {
  if (!entry || entry.directOnly) return false;
  return entry.id.startsWith("api:") || ["cli:claude-code", "cli:codex", "cli:grok", "cli:muse"].includes(entry.id);
}

/** 存着的预设：自带的缺了补上（顺序：自带的在前）。 */
export function normalizePresets(saved: Preset[] | undefined): Preset[] {
  const list = (saved ?? []).filter((p) => p && typeof p.id === "string" && typeof p.build === "string" && p.build);
  for (const b of BUILTIN_PRESETS) if (!list.some((p) => p.id === b.id)) list.push(b);
  return [...list.filter((p) => p.builtin), ...list.filter((p) => !p.builtin)];
}

/** AGENTS 现在是什么组合（写规范、实施、审查、委派）。`chosen` 是界面上选中的那一个（`relay` 或模型列表里的 id）。 */
export function currentSetup(settings: AppSettings, chosen: string | null): Omit<Preset, "id" | "name"> | null {
  if (!chosen) return null;
  if (chosen === RELAY) {
    if (!settings.relay.build) return null;
    return { spec: settings.relay.spec || undefined, build: settings.relay.build, review: settings.review || undefined, delegate: settings.delegate || undefined };
  }
  return { build: chosen, review: settings.review || undefined, delegate: settings.delegate || undefined };
}

/** 现在的组合和这个预设一样吗（委派只在实施的能委派时才算）。 */
export function matches(p: Preset, now: Omit<Preset, "id" | "name"> | null, canDelegate: (build: string) => boolean): boolean {
  if (!now) return false;
  const d = (x: Omit<Preset, "id" | "name">) => (canDelegate(x.build) ? x.delegate ?? "" : "");
  return (p.spec ?? "") === (now.spec ?? "") && p.build === now.build && (p.review ?? "") === (now.review ?? "") && d(p) === d(now);
}

/** 套用：把 AGENTS 的几行设成预设的样子。 */
export function applyPreset(p: Preset, settings: AppSettings): Partial<AppSettings> {
  const patch: Partial<AppSettings> = { review: p.review ?? "", delegate: p.delegate ?? "", preset: p.id };
  if (p.spec) {
    patch.pick = RELAY;
    patch.relay = { spec: p.spec, build: p.build };
    patch.picks = { ...settings.picks, relay: RELAY };
  } else {
    patch.pick = p.build;
    patch.picks = { ...settings.picks, [p.build.startsWith("cli:") ? "cli" : "api"]: p.build };
  }
  return patch;
}

/** 预设里谁现在用不了、为什么（空着就是都能用）。 */
export function presetProblems(p: Preset, entries: ModelEntry[]): string[] {
  const out: string[] = [];
  const check = (id: string | undefined, role: string, automated: boolean) => {
    if (!id) return;
    const e = entries.find((x) => x.id === id);
    const name = e?.label ?? id.replace(/^(api|cli):/, "");
    if (!e) out.push(id.startsWith("api:") ? `${role}的 ${name} 没配 Key` : `${role}的 ${name} 不在列表里`);
    else if (!e.available) out.push(e.group === "cli" ? `${role}的 ${name} 没检测到` : `${role}的 ${name} 现在用不了`);
    else if (automated && e.directOnly) out.push(`${name} 只做直接调用，不能${role}`);
  };
  check(p.spec, "写规范", true);
  check(p.build, "实施", !!(p.spec || p.review || p.delegate));
  check(p.review, "审查", true);
  if (p.delegate && !p.delegate.startsWith("api:")) out.push("委派只能交给 API 模型");
  else check(p.delegate, "委派", true);
  if (p.delegate && !canDelegate(entries.find((e) => e.id === p.build))) out.push("实施的这一个不能委派");
  return out;
}

/** 一行说预设里都有谁：「Claude Code 写规范 → Codex 实施 · Sonnet 5.5 审查」。 */
export function presetSummary(p: Preset, entries: ModelEntry[]): string {
  const name = (id: string) => entries.find((e) => e.id === id)?.label ?? id.replace(/^(api|cli):/, "").replace(/^[a-z-]+\//, "");
  const head = p.spec ? `${name(p.spec)} 写规范 → ${name(p.build)} 实施` : `${name(p.build)} 实施`;
  const tail = [p.review && `${name(p.review)} 审查`, p.delegate && `${name(p.delegate)} 委派`].filter(Boolean);
  return [head, ...tail].join(" · ");
}
