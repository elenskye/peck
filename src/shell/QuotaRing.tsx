// 输入框右下角的额度环（v1.1.9，站主定）：和上下文小环并排，样子不一样——上下文是 12 个点，这个是一段实心的弧。
// - CLI：这家订阅的几个窗口（5 小时、每周……），环显示用得最多的那个；点开每个窗口一行。读不到的画虚线，
//   点开写原因和 Peck 自己记的近 5 小时、近 7 天用量（按价格表估的）。
// - PLOOP：这个月 API 花了多少 / 每月花费上限。
import React, { useEffect, useRef, useState } from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { tokens, usd } from "@/lib/agentView";
import { ipc, type SpendPoint } from "@/lib/ipc";
import { cardOf, resetIn, windowName } from "@/lib/quota";
import type { AppSettings } from "@/lib/settings";
import type { ClientCard } from "@/lib/usage";
import { cn } from "@/lib/utils";
import { Float, ModHead } from "@/ui";

interface Row { key: string; name: string; pct: number | null; note: string; stale?: boolean }

const R = 5.5;
const LEN = 2 * Math.PI * R;

/** 20 格的刻度条：亮几格是用了几成。 */
const Meter: React.FC<{ pct: number | null; level: string }> = ({ pct, level }) => (
  <span className={cn("q-meter q-meter--sm", level)} aria-hidden="true">
    {Array.from({ length: 20 }, (_, i) => <i key={i} className={pct != null && i < Math.round(Math.min(100, pct) / 5) ? "is-on" : undefined} />)}
  </span>
);

export const QuotaRing: React.FC<{
  who: ModelEntry;
  clients: ClientCard[];
  settings: AppSettings;
  /** 变了就重读这个月的花费（任务跑完）。 */
  stamp: string;
}> = ({ who, clients, settings, stamp }) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const cli = who.group === "cli";
  const engine = cli ? who.id.slice(4) : "peck";
  const card = cli ? cardOf(who.id, clients) : null;
  const warnAt = settings.quotaWarn ?? 85;
  const level = (pct: number | null) => (pct == null ? "" : pct >= 95 ? "is-bad" : pct >= warnAt ? "is-warn" : "");

  // PLOOP：这个月花了多少。
  const [month, setMonth] = useState<number | null>(null);
  useEffect(() => {
    if (cli) return;
    let live = true;
    ipc.harnessSpend().then((s) => { if (live) setMonth(s.monthUsd); }).catch(() => {});
    return () => { live = false; };
  }, [cli, stamp]);
  // 读不到额度的 CLI（没读数，或者读数都过了重置时间）：点开时算 Peck 自己记的近 5 小时、近 7 天。
  const [points, setPoints] = useState<SpendPoint[] | null>(null);
  const now = Date.now();
  const stale = (w: { resetAtMs?: number | null }) => w.resetAtMs != null && w.resetAtMs <= now;
  const unreadable = cli && !(card?.windows ?? []).some((w) => w.usedPercent != null && !stale(w));
  useEffect(() => {
    if (!open || !unreadable) return;
    let live = true;
    ipc.spendOverview(7).then((o) => { if (live) setPoints(o.points.filter((p) => p.engine === engine)); }).catch(() => {});
    return () => { live = false; };
  }, [open, unreadable, engine, stamp]);

  const rows: Row[] = cli
    ? (card?.windows ?? []).filter((w) => w.usedPercent != null).map((w) => {
        // 过了重置时间的读数作废：不画旧的百分比。
        const old = stale(w);
        return { key: w.label, name: windowName(w.label, w.windowDurationMins), pct: old ? null : w.usedPercent, note: old ? "已重置" : w.resetAtMs ? resetIn(w.resetAtMs) : "", stale: old };
      })
    : [{ key: "month", name: "本月", pct: settings.costCap && month != null ? (month / settings.costCap) * 100 : null, note: `${usd(month ?? 0) ?? "$0"}${settings.costCap ? ` / $${settings.costCap}` : ""}` }];
  // 环：没过重置时间的窗口里用得最多的那个。
  const live = rows.filter((r) => r.pct != null && !r.stale);
  const top = live.length ? Math.max(...live.map((r) => r.pct!)) : null;
  const tally = (ms: number) => {
    const list = (points ?? []).filter((p) => p.at >= now - ms);
    const sum = (f: (p: SpendPoint) => number | null | undefined) => list.reduce((n, p) => n + (f(p) ?? 0), 0);
    return { tokens: sum((p) => (p.input ?? 0) + (p.output ?? 0) + (p.cacheRead ?? 0) + (p.cacheWrite ?? 0)), usd: sum((p) => p.usd) };
  };
  const label = cli ? `${who.label} 的额度${top != null ? `用了 ${Math.round(top)}%` : "读不到"}` : `这个月 API 花了 ${usd(month ?? 0) ?? "$0"}`;
  return (
    <>
      <button ref={ref} type="button" className={cn("quota-ring", level(top), top == null && "is-unknown")} aria-label={label} title={label} aria-expanded={open} onClick={() => setOpen(!open)}>
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          <circle className="quota-ring-track" cx="8" cy="8" r={R} />
          {top != null && <circle className="quota-ring-arc" cx="8" cy="8" r={R} strokeDasharray={`${(Math.min(100, Math.max(top, 4)) / 100) * LEN} ${LEN}`} transform="rotate(-90 8 8)" />}
        </svg>
      </button>
      {open && (
        <Float anchor={ref} side="top" align="end" onClose={() => setOpen(false)} className="pk-anim-reveal-s menu-pop menu-pop--fixed ctx-pop" role="dialog" aria-label="额度">
          <div className="pk-mod">
            <ModHead label="QUOTA" note={cli ? who.label : "PLOOP"} />
            {rows.length > 0 && (
              <ul className="quota-rows">
                {rows.map((r) => (
                  <li key={r.key} className={r.stale ? "is-stale" : undefined}>
                    <span>{r.name}</span>
                    <Meter pct={r.pct} level={r.stale ? "" : level(r.pct)} />
                    <b className="t-code">{r.pct != null ? `${Math.round(r.pct)}%` : ""}</b>
                    <em>{r.note}</em>
                  </li>
                ))}
              </ul>
            )}
            {unreadable && (
              <>
                {(card?.failureReason || rows.length === 0) && <p className="note">{card?.failureReason ?? "读不到"}</p>}
                {points && points.length > 0 && (
                  <ul className="ctx-parts">
                    {([["近 5 小时", 5 * 3_600_000], ["近 7 天", 7 * 86_400_000]] as const).map(([name, ms]) => {
                      const t = tally(ms);
                      return <li key={name}><span>{name}</span><b className="t-code">{tokens(t.tokens)}</b><em>{t.usd ? `估 ${usd(t.usd)}` : ""}</em></li>;
                    })}
                  </ul>
                )}
              </>
            )}
          </div>
        </Float>
      )}
    </>
  );
};
