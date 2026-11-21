// 额度快用完时提醒换引擎（v0.9 Phase 14 B3）：发任务前看这次要用到的 CLI（实施、写规范、审查）的订阅额度
// （QUOTA 页读到的 5 小时 / 每周窗口），超过阈值就在输入框上方提示一行，能一键换成额度还够的另一家。
// 不自动换、不拦着发；额度读不到的不提示。
import type { ModelEntry } from "@/lib/agentEvents";
import type { AppSettings } from "@/lib/settings";
import type { ClientCard } from "@/lib/usage";

/** CLI 引擎 id → QUOTA 页客户端卡片的 id。 */
const CLIENT_OF: Record<string, string> = { "claude-code": "claude-code", codex: "codex", muse: "muse-code", grok: "grok", antigravity: "antigravity" };

/** 一家的窗口读数：已用百分比读得到的那几个。 */
function windows(id: string, cards: ClientCard[]): Array<{ label: string; pct: number }> {
  const engine = id.startsWith("cli:") ? id.slice(4) : null;
  const card = engine ? cards.find((c) => c.client === CLIENT_OF[engine]) : undefined;
  return (card?.windows ?? []).filter((w) => w.usedPercent != null).map((w) => ({ label: w.label, pct: w.usedPercent! }));
}

/** 这一家超过阈值的那个窗口（用得最多的）；读不到或没超过是 null。 */
export function hotWindow(id: string, cards: ClientCard[], threshold: number): { label: string; pct: number } | null {
  const hot = windows(id, cards).filter((w) => w.pct >= threshold).sort((a, b) => b.pct - a.pct);
  return hot[0] ?? null;
}

/** 这一家额度读得到、而且都没超过阈值。 */
function roomy(id: string, cards: ClientCard[], threshold: number): boolean {
  const w = windows(id, cards);
  return w.length > 0 && w.every((x) => x.pct < threshold);
}

export interface QuotaHint {
  /** 快用完的那一家（模型列表里的 id）。 */
  from: string;
  fromLabel: string;
  /** 「Claude Code 每周已用 90%」 */
  text: string;
  /** 建议换成的（额度读得到、还够的另一家 CLI）；没有就只提示。 */
  to: ModelEntry | null;
}

/** 这次要用到的 CLI 里头一个快用完的（实施的先看）。`threshold` 空着是关了提醒。 */
export function quotaHint(involved: string[], entries: ModelEntry[], cards: ClientCard[], threshold: number | null): QuotaHint | null {
  if (threshold == null) return null;
  for (const id of involved) {
    if (!id.startsWith("cli:")) continue;
    const hot = hotWindow(id, cards, threshold);
    if (!hot) continue;
    const from = entries.find((e) => e.id === id);
    const fromLabel = from?.label ?? id.slice(4);
    const to = entries.find((e) => e.group === "cli" && e.available && !e.directOnly && e.id !== id && !involved.includes(e.id) && roomy(e.id, cards, threshold))
      ?? entries.find((e) => e.group === "cli" && e.available && !e.directOnly && e.id !== id && roomy(e.id, cards, threshold))
      ?? null;
    const window = hot.label.trim();
    return { from: id, fromLabel, text: `${fromLabel} ${window}${/[一-龥]$/.test(window) ? "" : " "}已用 ${Math.round(hot.pct)}%`, to };
  }
  return null;
}

/** 这次发任务会用到哪几家：实施的、写规范的（接力）、审查的。 */
export function involvedIds(settings: AppSettings, chosen: string | null): string[] {
  if (!chosen) return [];
  const out = chosen === "relay" ? [settings.relay.build, settings.relay.spec] : [chosen];
  if (settings.review && settings.permission !== "plan") out.push(settings.review);
  return out.filter((x): x is string => !!x);
}

/** 「以后都换」：AGENTS 里用到 `from` 的那几行换成 `to`（套用着的预设也跟着改）。 */
export function swapPatch(settings: AppSettings, from: string, to: string): Partial<AppSettings> {
  const swap = (id: string | undefined) => (id === from ? to : id);
  const patch: Partial<AppSettings> = {};
  if (settings.pick === from) { patch.pick = to; patch.picks = { ...settings.picks, cli: to }; }
  if (settings.relay.spec === from || settings.relay.build === from) patch.relay = { spec: swap(settings.relay.spec), build: swap(settings.relay.build) };
  if (settings.review === from) patch.review = to;
  if (settings.preset) {
    patch.presets = settings.presets.map((p) => (p.id === settings.preset ? { ...p, spec: swap(p.spec), build: swap(p.build) ?? p.build, review: swap(p.review) } : p));
  }
  return patch;
}
