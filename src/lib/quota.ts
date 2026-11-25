// 额度快用完时提醒换引擎（v0.9 Phase 14 B3）：发任务前看这次要用到的 CLI（实施、写规范、审查）的订阅额度
// （QUOTA 页读到的 5 小时 / 每周窗口），超过阈值就在输入框上方提示一行，能一键换成额度还够的另一家。
// 不自动换、不拦着发；额度读不到的不提示。
import type { ModelEntry } from "@/lib/agentEvents";
import type { AppSettings } from "@/lib/settings";
import type { ClientCard } from "@/lib/usage";

/** CLI 引擎 id → QUOTA 页客户端卡片的 id。 */
const CLIENT_OF: Record<string, string> = { "claude-code": "claude-code", codex: "codex", muse: "muse-code", grok: "grok" };

/** 这家 CLI 的额度卡片（QUOTA 页那一张）。 */
export function cardOf(id: string, cards: ClientCard[]): ClientCard | null {
  const engine = id.startsWith("cli:") ? id.slice(4) : null;
  return (engine && cards.find((c) => c.client === CLIENT_OF[engine])) || null;
}

/** 窗口的名字：有时长就按时长叫（5 小时、每周），不然把客户端给的长名字收短。 */
export function windowName(label: string, mins: number | null | undefined): string {
  // 已经是中文名字的（「5 小时」「每周」「每周 Sonnet」）照写。
  if (/[一-鿿]/.test(label) && !label.includes("·")) return label;
  if (mins) {
    if (mins < 1440) return `${Math.round(mins / 60)} 小时`;
    if (mins === 1440) return "每天";
    if (mins === 10080) return "每周";
    if (mins >= 40000 && mins <= 45000) return "每月";
    return `${Math.round(mins / 1440)} 天`;
  }
  const parts = label.split("·").map((x) => x.trim()).filter(Boolean);
  const name = parts.find((p) => /primary|secondary/i.test(p)) ?? parts.find((p) => /[一-鿿]/.test(p)) ?? parts[0] ?? label;
  if (/primary/i.test(name)) return "短窗口";
  if (/secondary/i.test(name)) return "长窗口";
  return name.length > 6 ? `${name.slice(0, 6)}…` : name;
}

/** 多久以后重置。 */
export function resetIn(at: number): string {
  const ms = at - Date.now();
  if (ms <= 0) return "已经";
  const h = ms / 3_600_000;
  return h < 1 ? `${Math.ceil(ms / 60_000)} 分钟后` : h < 48 ? `${Math.round(h)} 小时后` : `${Math.round(h / 24)} 天后`;
}

/** 一家的窗口读数：已用百分比读得到的那几个。快照是旧的、窗口已经过了重置时间的当清零了（v1.2.0：Codex 没开着时
 *  Peck 不去读，4 个多小时前那份「5 小时窗口 100%」一直在提醒换引擎）。 */
function windows(id: string, cards: ClientCard[], now = Date.now()): Array<{ label: string; pct: number }> {
  const engine = id.startsWith("cli:") ? id.slice(4) : null;
  const card = engine ? cards.find((c) => c.client === CLIENT_OF[engine]) : undefined;
  return (card?.windows ?? [])
    .filter((w) => w.usedPercent != null)
    .map((w) => ({ label: w.label, pct: w.resetAtMs != null && w.resetAtMs <= now ? 0 : w.usedPercent! }));
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
    const to = entries.find((e) => e.group === "cli" && e.available && e.id !== id && !involved.includes(e.id) && roomy(e.id, cards, threshold))
      ?? entries.find((e) => e.group === "cli" && e.available && e.id !== id && roomy(e.id, cards, threshold))
      ?? null;
    const window = hot.label.trim();
    return { from: id, fromLabel, text: `${fromLabel} ${window}${/[一-龥]$/.test(window) ? "" : " "}已用 ${Math.round(hot.pct)}%`, to };
  }
  return null;
}

/** 这次发任务会用到哪几家 CLI：只有单干的 CLI 有订阅额度（PLOOP 只用 API 模型）。 */
export function involvedIds(_settings: AppSettings, chosen: string | null): string[] {
  return chosen?.startsWith("cli:") ? [chosen] : [];
}

/** 「以后都换」：AGENTS 里选的 CLI 换成 `to`。 */
export function swapPatch(settings: AppSettings, from: string, to: string): Partial<AppSettings> {
  return settings.pick === from ? { pick: to, picks: { ...settings.picks, cli: to } } : {};
}
