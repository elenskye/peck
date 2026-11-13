import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ModelEntry } from "@/lib/agentEvents";
import { ipc, isMockMode, type QuotaPoint, type SpendOverview, type SpendPoint } from "@/lib/ipc";
import { usageApi, type BalanceCard, type ClientCard, type UsageListResult } from "@/lib/usage";
import { agentName, cn } from "@/lib/utils";
import { Btn, Field, MenuChip, ModHead, Row, Seg } from "@/ui";

// 额度与花费（usage.rs + spend.rs 的事件日志）。原来要等站主用 Peck 另做一个（src/quota/），2026-11-13 站主改了主意：这一页就是它，本地的 Claude 接着做。

const STATUS: Record<string, string> = { ok: "读到了", not_installed: "没装", not_running: "没在运行", not_logged_in: "没登录", fields_missing: "现在读不到", read_failed: "读取失败", stale_stopped: "已停止 · 最后一次的快照", unknown: "未知", missing_key: "没配 Key", query_failed: "读取失败" };
const when = (t: number | null | undefined) => (t ? new Date(t).toLocaleString("zh-CN", { hour12: false }) : "未知");
const num = (v: number | null) => (v === null ? "未知" : v.toLocaleString());

/** 图里的系列：固定顺序（相邻两色色弱也分得开，校验过），颜色跟着引擎走，不跟名次。 */
const SERIES = ["antigravity", "api", "grok", "muse", "claude-code", "codex"] as const;
type Series = (typeof SERIES)[number];
const SERIES_NAME: Record<Series, string> = { antigravity: "Antigravity", api: "Peck 循环", grok: "Grok", muse: "Muse", "claude-code": "Claude Code", codex: "Codex" };
const seriesOf = (engine: string): Series => (engine === "peck" ? "api" : (SERIES as readonly string[]).includes(engine) ? (engine as Series) : "api");
const fill = (s: Series) => `var(--chart-${s})`;

const money = (v: number) => (v >= 100 ? `$${v.toFixed(0)}` : v >= 1 ? `$${v.toFixed(2)}` : v > 0 ? `$${v.toFixed(v < 0.01 ? 4 : 3)}` : "$0");
const tokens = (v: number) => (v >= 1e6 ? `${(v / 1e6).toFixed(1)}M` : v >= 1e3 ? `${(v / 1e3).toFixed(1)}k` : `${v}`);
const dayKey = (t: number) => { const d = new Date(t); return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`; };
const dayLabel = (t: number) => { const d = new Date(t); return `${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };

function Filter({ label, value, options, onChange }: { label: string; value: string; options: Array<[string, string]>; onChange: (v: string) => void }) {
  return (
    <MenuChip sm up={false} ariaLabel={label} label={(options.find(([id]) => id === value)?.[1] ?? label).toUpperCase()}>
      {(close) => options.map(([id, name]) => (
        <button key={id} type="button" role="menuitem" className={cn("pk-menu-item", id === value && "is-on")} onClick={() => { onChange(id); close(); }}>
          <i /><b>{name.toUpperCase()}</b><span />
        </button>
      ))}
    </MenuChip>
  );
}

/** 宽度跟着容器变（图按真实像素画，字不会被缩放）。 */
function useWidth<T extends HTMLElement>(): [React.RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(640);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setWidth(Math.max(320, Math.floor(entry.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, width];
}

/** 0 到 max 之间取几条好读的刻度。 */
function ticks(max: number): number[] {
  if (max <= 0) return [0];
  const raw = max / 4;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = 0; v <= max + step * 0.001; v += step) out.push(+v.toFixed(6));
  if (out[out.length - 1] < max) out.push(+(out[out.length - 1] + step).toFixed(6));
  return out;
}

export const UsagePage: React.FC<{ onClose: () => void; say: (message: unknown) => void; entries: ModelEntry[]; costCap: number | null }> = ({ onClose, say, entries, costCap }) => {
  const [days, setDays] = useState<"7" | "30" | "90">("30");
  const [spend, setSpend] = useState<SpendOverview | null>(null);
  const [monthUsd, setMonthUsd] = useState<number | null>(null);
  const [clients, setClients] = useState<ClientCard[]>([]);
  const [balances, setBalances] = useState<BalanceCard[]>([]);
  const act = async (fn: () => Promise<unknown>) => { try { await fn(); } catch (e) { say(e); } };

  const reloadSpend = useCallback(async () => {
    const [overview, month] = await Promise.all([ipc.spendOverview(Number(days)), ipc.harnessSpend()]);
    setSpend(overview);
    setMonthUsd(month.monthUsd);
  }, [days]);
  useEffect(() => { void act(reloadSpend); }, [reloadSpend]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { void act(async () => { setClients(await usageApi.clients()); setBalances(await usageApi.balances()); }); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (isMockMode()) return;
    let closed = false;
    const off: Array<() => void> = [];
    void listen<ClientCard[]>("usage-updated", (e) => { setClients(e.payload); void act(reloadSpend); }).then((f) => (closed ? f() : off.push(f))).catch(say);
    void listen<BalanceCard[]>("usage-balances-updated", (e) => setBalances(e.payload)).then((f) => (closed ? f() : off.push(f))).catch(say);
    return () => { closed = true; off.forEach((f) => f()); };
  }, [reloadSpend]); // eslint-disable-line react-hooks/exhaustive-deps

  const points = spend?.points ?? [];
  const cliUsd = points.filter((p) => p.engine !== "peck").reduce((n, p) => n + (p.usd ?? 0), 0);
  const deepseek = balances.find((b) => b.provider === "deepseek");
  const label = (key: string, engine: string) => key === "oneshot" ? "ONE SHOT" : engine === "peck" ? entries.find((e) => e.id === `api:${key}`)?.label ?? key : agentName(engine);

  return (
    <section className="col page" aria-label="额度与花费">
      <div className="pk-mod col-grow page-mod">
        <ModHead label="QUOTA" note={isMockMode() ? "额度与花费 · 浏览器演示数据" : "额度与花费"}>
          <Seg<"7" | "30" | "90"> label="看多少天" sm items={[{ id: "7", label: "7D" }, { id: "30", label: "30D" }, { id: "90", label: "90D" }]} value={days} onChange={setDays} />
          <Btn v="ghost" sm onClick={() => void act(async () => { setClients(await usageApi.refresh()); await reloadSpend(); say("刷新过了；没在运行的客户端不会被启动"); })}>REFRESH</Btn>
          <Btn v="ghost" sm icon="x" onClick={onClose}>CLOSE</Btn>
        </ModHead>
        <div className="set-pane scroll quota">
          <div className="q-tiles">
            <Tile label="THIS MONTH · API" value={monthUsd == null ? "—" : money(monthUsd)} note={costCap ? `上限 ${money(costCap)} · Peck 循环和 ONE SHOT` : "没设上限 · Peck 循环和 ONE SHOT"} meter={costCap && monthUsd != null ? monthUsd / costCap : null} />
            <Tile label={`${days} DAYS · CLI`} value={money(cliUsd)} note="CLI 自己估的，用的是订阅，不是真扣钱" />
            <Tile label="DEEPSEEK BALANCE" value={deepseek?.balances[0] ? balanceText(deepseek.balances[0].totalBalance, deepseek.balances[0].currency) : "—"}
              note={deepseek ? `${STATUS[deepseek.status] ?? deepseek.status} · ${when(deepseek.checkedAt)}` : "没查过"}
              action={<Btn sm v="ghost" onClick={() => void act(async () => { const card = await usageApi.balance("deepseek"); setBalances((prev) => [...prev.filter((x) => x.provider !== "deepseek"), card]); })}>CHECK</Btn>} />
          </div>

          <h2 className="t-title">每天花了多少</h2>
          <p className="lead">近 {days} 天，按谁花的叠起来。Peck 循环和 ONE SHOT 按价格表算；CLI 的是它们自己估的。悬停看每天的明细。</p>
          <DailyChart points={points} days={Number(days)} />

          <h2 className="t-title">花在哪</h2>
          <p className="lead">近 {days} 天，按模型和引擎。</p>
          <Breakdown points={points} label={label} />

          <h2 className="t-title">订阅额度</h2>
          <p className="lead">刻度和数字都是还剩多少。客户端的读数只在它运行时读，退出就停，留最后一次的快照；CLI 在 Peck 的任务里报的也记着，折线是近 7 天还剩的。读不到的不推算。</p>
          <div className="q-clients">
            {clients.map((c) => <ClientQuota key={c.client} card={c} history={(spend?.quota ?? []).filter((q) => q.engine === (c.client === "muse-code" ? "muse" : c.client))} say={say} onCard={(card) => setClients((prev) => prev.map((x) => (x.client === card.client ? card : x)))} />)}
          </div>

          <details className="q-legacy">
            <summary className="lbl">旧的规划 + 实施的调用记录</summary>
            <LegacyCalls say={say} />
          </details>
        </div>
      </div>
    </section>
  );
};

const Tile: React.FC<{ label: string; value: string; note: string; meter?: number | null; action?: React.ReactNode }> = ({ label, value, note, meter, action }) => {
  const lit = meter == null ? 0 : Math.min(20, Math.round(meter * 20));
  return (
    <div className="pk-mod pk-mod--sunk q-tile">
      <p className="lbl">{label}</p>
      <p className="q-tile-v"><b className="pk-readout pk-readout--num">{value}</b>{action}</p>
      {meter != null && (
        <div className={cn("q-meter", meter >= 1 ? "is-bad" : meter >= 0.8 && "is-warn")} role="meter" aria-valuenow={Math.round(meter * 100)} aria-valuemin={0} aria-valuemax={100} aria-label={`用了上限的 ${Math.round(meter * 100)}%`}>
          {Array.from({ length: 20 }, (_, i) => <i key={i} className={cn(i < lit && "is-on")} />)}
        </div>
      )}
      <p className="hint">{note}</p>
    </div>
  );
};

/** 每天一根柱子，按引擎叠起来；悬停一整列看明细。 */
const DailyChart: React.FC<{ points: SpendPoint[]; days: number }> = ({ points, days }) => {
  const [boxRef, width] = useWidth<HTMLDivElement>();
  const [hover, setHover] = useState<number | null>(null);
  const [table, setTable] = useState(false);
  const data = useMemo(() => {
    const now = new Date();
    const cols = Array.from({ length: days }, (_, i) => {
      const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (days - 1 - i));
      return { key: dayKey(d.getTime()), at: d.getTime(), by: {} as Partial<Record<Series, number>>, total: 0, unknown: 0 };
    });
    const index = new Map(cols.map((c, i) => [c.key, i]));
    for (const p of points) {
      const i = index.get(dayKey(p.at));
      if (i == null) continue;
      if (p.usd == null) { cols[i].unknown += 1; continue; }
      const s = seriesOf(p.engine);
      cols[i].by[s] = (cols[i].by[s] ?? 0) + p.usd;
      cols[i].total += p.usd;
    }
    return cols;
  }, [points, days]);
  const present = SERIES.filter((s) => data.some((c) => (c.by[s] ?? 0) > 0));
  const max = Math.max(...data.map((c) => c.total), 0);
  const scale = ticks(max);
  const top = scale[scale.length - 1] || 1;
  // 左边留多宽按最长的刻度字算（一个等宽字约 7 像素），小数多的时候不被切掉。
  const tickText = (v: number) => "$" + String(+v.toFixed(4));
  const H = 180, padL = Math.max(40, Math.max(...scale.map((v) => tickText(v).length)) * 7 + 14), padB = 22, padT = 8;
  const plotW = width - padL - 8;
  const step = plotW / days;
  const barW = Math.max(2, Math.min(28, step - (days > 60 ? 1 : 3)));
  const y = (v: number) => padT + (H - padT - padB) * (1 - v / top);
  const labelEvery = days <= 7 ? 1 : days <= 30 ? 5 : 15;
  const total = data.reduce((n, c) => n + c.total, 0);
  const col = hover != null ? data[hover] : null;
  return (
    <div className="q-chart" ref={boxRef}>
      <div className="q-chart-head">
        {present.length > 1 && (
          <ul className="q-legend" aria-label="图例">
            {present.map((s) => <li key={s}><i style={{ background: fill(s) }} />{SERIES_NAME[s]}</li>)}
          </ul>
        )}
        <span className="grow" />
        <span className="hint">合计 {money(total)}</span>
        <Seg<"chart" | "table"> label="图或表" sm items={[{ id: "chart", label: "CHART" }, { id: "table", label: "TABLE" }]} value={table ? "table" : "chart"} onChange={(v) => setTable(v === "table")} />
      </div>
      {max === 0 ? <p className="empty-line">这段时间没有记下花费。</p> : table ? (
        <table className="q-table">
          <thead><tr><th>日期</th>{present.map((s) => <th key={s}>{SERIES_NAME[s]}</th>)}<th>合计</th></tr></thead>
          <tbody>
            {data.filter((c) => c.total > 0).reverse().map((c) => (
              <tr key={c.key}><td>{dayLabel(c.at)}</td>{present.map((s) => <td key={s}>{c.by[s] ? money(c.by[s]!) : "—"}</td>)}<td><b>{money(c.total)}</b></td></tr>
            ))}
          </tbody>
        </table>
      ) : (
        <div className="q-plot">
          <svg width={width} height={H} role="img" aria-label={`近 ${days} 天每天的花费，合计 ${money(total)}，最多的一天 ${money(max)}`} onMouseLeave={() => setHover(null)}>
            {scale.map((v) => (
              <g key={v}>
                <line x1={padL} x2={width - 8} y1={y(v)} y2={y(v)} className={v === 0 ? "q-axis" : "q-grid"} />
                <text x={padL - 8} y={y(v) + 4} textAnchor="end" className="q-tick">{tickText(v)}</text>
              </g>
            ))}
            {data.map((c, i) => {
              const x = padL + step * i + (step - barW) / 2;
              let acc = 0;
              return (
                <g key={c.key}>
                  {SERIES.map((s) => {
                    const v = c.by[s];
                    if (!v) return null;
                    const y1 = y(acc + v), y0 = y(acc);
                    acc += v;
                    const h = y0 - y1;
                    // 叠在一起的段之间留 2 像素的底色缝；太矮的段不留缝。
                    const gap = acc - v > 0 && h > 4 ? 2 : 0;
                    return <rect key={s} x={x} y={y1} width={barW} height={Math.max(1, h - gap)} style={{ fill: fill(s) }} opacity={hover == null || hover === i ? 1 : 0.45} />;
                  })}
                  {i % labelEvery === (days - 1) % labelEvery && <text x={x + barW / 2} y={H - 6} textAnchor="middle" className="q-tick">{dayLabel(c.at)}</text>}
                  <rect x={padL + step * i} y={padT} width={step} height={H - padT - padB} fill="transparent" onMouseEnter={() => setHover(i)} />
                </g>
              );
            })}
          </svg>
          {col && (
            <div className="q-tip pk-mod" style={{ left: Math.min(width - 200, Math.max(0, padL + step * hover! + step / 2 - 90)) }}>
              <p className="lbl">{dayLabel(col.at)} · {money(col.total)}</p>
              {SERIES.filter((s) => col.by[s]).reverse().map((s) => <p key={s} className="q-tip-row"><i style={{ background: fill(s) }} /><span>{SERIES_NAME[s]}</span><b>{money(col.by[s]!)}</b></p>)}
              {col.total === 0 && <p className="hint">这天没有花费</p>}
              {col.unknown > 0 && <p className="hint">另有 {col.unknown} 次算不出花费</p>}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

/** 按模型和引擎的横条：花了多少、几次、多少 Token。 */
const Breakdown: React.FC<{ points: SpendPoint[]; label: (key: string, engine: string) => string }> = ({ points, label }) => {
  const rows = useMemo(() => {
    const map = new Map<string, { key: string; engine: string; usd: number; n: number; input: number; output: number; estimated: boolean }>();
    for (const p of points) {
      const id = `${p.engine}:${p.key}`;
      const r = map.get(id) ?? { key: p.key, engine: p.engine, usd: 0, n: 0, input: 0, output: 0, estimated: p.estimated };
      r.usd += p.usd ?? 0; r.n += 1; r.input += (p.input ?? 0) + (p.cacheRead ?? 0); r.output += p.output ?? 0;
      map.set(id, r);
    }
    return [...map.values()].sort((a, b) => b.usd - a.usd);
  }, [points]);
  if (rows.length === 0) return <p className="empty-line">这段时间没有记下花费。</p>;
  const max = Math.max(...rows.map((r) => r.usd), 1e-9);
  return (
    <ul className="q-bars">
      {rows.map((r) => (
        <li key={`${r.engine}:${r.key}`}>
          <span className="q-bars-name"><i style={{ background: fill(seriesOf(r.engine)) }} />{label(r.key, r.engine)}</span>
          <span className="q-bars-track"><span style={{ width: `${Math.max(1, (r.usd / max) * 100)}%`, background: fill(seriesOf(r.engine)) }} /></span>
          <b className="q-bars-v">{money(r.usd)}{r.estimated ? " 估" : ""}</b>
          <span className="q-bars-n">{r.n} 次 · 进 {tokens(r.input)} · 出 {tokens(r.output)}</span>
        </li>
      ))}
    </ul>
  );
};

/** 一个客户端的订阅额度：每个窗口一条刻度条，旁边近 7 天的折线。 */
const ClientQuota: React.FC<{ card: ClientCard; history: QuotaPoint[]; say: (m: unknown) => void; onCard: (c: ClientCard) => void }> = ({ card: c, history, say, onCard }) => {
  const [manualOpen, setManualOpen] = useState(false);
  const [m, setM] = useState({ pct: "", note: "", reset: "" });
  const engine = c.client === "muse-code" ? "muse" : c.client;
  const s = seriesOf(engine);
  // 窗口：客户端读到的优先；没读到就用任务里最近一次报的。
  const latest = new Map<string, QuotaPoint>();
  for (const q of history) latest.set(q.label, q);
  const fromClient = c.windows.length > 0;
  const windows = fromClient
    ? c.windows.map((w) => ({ key: w.label, name: windowName(w.label, w.windowDurationMins), used: w.usedPercent, remaining: w.remainingAmount, reset: w.resetAtMs }))
    : [...latest.values()].map((q) => ({ key: q.label, name: windowName(q.label, null), used: q.usedPercent as number | null, remaining: null as number | null, reset: q.resetsAt }));
  const weekAgo = Date.now() - 7 * 86_400_000;
  const tag = fromClient ? STATUS[c.status] ?? c.status : windows.length ? "任务里报的" : STATUS[c.status] ?? c.status;
  return (
    <div className="pk-mod pk-mod--sunk q-client">
      <div className="q-client-head">
        <i className="q-dot" style={{ background: fill(s) }} />
        <b className="t-name">{c.label}</b>
        <span className="grow" />
        <em className={cn("pk-tag", (c.status === "ok" || (!fromClient && windows.length > 0)) && "pk-tag--ok")}>{tag}</em>
      </div>
      {windows.length === 0 && <p className="hint">{c.failureReason ?? "还没读到额度。"}</p>}
      {windows.length > 0 && fromClient && c.status !== "ok" && c.failureReason && <p className="hint q-client-warn">这次没读到：{c.failureReason} 下面是 {when(c.lastSuccessAt)} 的读数。</p>}
      {windows.map((w) => {
        // 刻度和数字都是「还剩多少」（站主 2026-11-13 定）。过了重置时间的读数是旧的：刻度变暗，不当成现在还剩多少。
        const stale = w.reset != null && w.reset <= Date.now();
        const left = w.used == null ? null : 100 - Math.max(0, Math.min(100, w.used));
        const lit = left == null ? 0 : Math.round(left / 5);
        const line = history.filter((q) => q.label === w.key && q.at >= weekAgo);
        return (
          <div key={w.key} className={cn("q-window", stale && "is-stale")}>
            <span className="q-window-l" title={w.key}>{w.name}</span>
            <span className={cn("q-meter q-meter--sm", !stale && left != null && left <= 10 ? "is-bad" : !stale && left != null && left <= 25 && "is-warn")} style={{ ["--q-on" as string]: fill(s) }} role="meter" aria-valuenow={left ?? undefined} aria-valuemin={0} aria-valuemax={100} aria-label={`${c.label} ${w.name} ${stale ? "旧读数，" : ""}还剩 ${left == null ? "未知" : `${Math.round(left)}%`}`}>
              {Array.from({ length: 20 }, (_, i) => <i key={i} className={cn(i < lit && "is-on")} />)}
            </span>
            <b className="q-window-v">{w.remaining != null ? `剩 ${w.remaining}` : left == null ? "未知" : `剩 ${Math.round(left)}%`}</b>
            <Spark points={line} color={fill(s)} />
            <span className="hint q-window-r" title={w.reset ? `重置时间 ${when(w.reset)}` : undefined}>{stale ? "旧读数" : w.reset ? `${resetIn(w.reset)}重置` : ""}</span>
          </div>
        );
      })}
      <p className="hint">{c.source} · {c.running ? "在运行" : "没在运行"}{c.accountLabel || c.accountId ? ` · ${c.accountLabel ?? c.accountId}` : ""}{c.tier ? ` · ${c.tier}` : ""} · 最后读到 {when(c.lastSuccessAt)}{c.previous ? " · 另有一份没核对过账号的旧快照" : ""}</p>
      {manualOpen ? (
        <div className="set-inline">
          <Field width={110} aria-label={`${c.label} 手工记：还剩百分之几`} placeholder="还剩 %" value={m.pct} onChange={(e) => setM({ ...m, pct: e.target.value })} />
          <Field aria-label={`${c.label} 手工记：从哪看来的`} placeholder="从哪看来的、哪个账号（要填）" value={m.note} onChange={(e) => setM({ ...m, note: e.target.value })} />
          <Field width={210} type="datetime-local" aria-label={`${c.label} 手工记：什么时候重置`} value={m.reset} onChange={(e) => setM({ ...m, reset: e.target.value })} />
          <Btn sm onClick={() => void usageApi.manual({ client: c.client, usedPercent: m.pct ? Math.max(0, Math.min(100, 100 - Number(m.pct))) : null, resetAtMs: m.reset ? new Date(m.reset).getTime() : null, note: m.note }).then((card) => { onCard(card); setManualOpen(false); say("手工记的快照存好了"); }).catch(say)}>SAVE</Btn>
        </div>
      ) : <button type="button" className="link" onClick={() => setManualOpen(true)}>手工记一笔</button>}
    </div>
  );
};

/** 余额带上币种：人民币写 ¥，美元写 $，别的写在后面。 */
function balanceText(amount: string, currency: string): string {
  const c = (currency || "").toUpperCase();
  if (c === "CNY" || c === "RMB") return "¥" + amount;
  if (c === "USD") return "$" + amount;
  return c ? `${amount} ${c}` : amount;
}

/** 窗口的名字：有时长就按时长叫（5 小时、每周），不然把客户端给的长名字收短。 */
function windowName(label: string, mins: number | null | undefined): string {
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

function resetIn(at: number): string {
  const ms = at - Date.now();
  if (ms <= 0) return "已经";
  const h = ms / 3_600_000;
  return h < 1 ? `${Math.ceil(ms / 60_000)} 分钟后` : h < 48 ? `${Math.round(h)} 小时后` : `${Math.round(h / 24)} 天后`;
}

/** 近 7 天还剩百分之几的折线（2 像素的线，0–100 固定刻度）。 */
const Spark: React.FC<{ points: QuotaPoint[]; color: string }> = ({ points, color }) => {
  const W = 72, H = 22;
  if (points.length < 2) return <span className="q-spark q-spark--none" title="近 7 天的读数不够画线" />;
  const t0 = points[0].at, t1 = points[points.length - 1].at || t0 + 1;
  const left = (p: QuotaPoint) => 100 - Math.max(0, Math.min(100, p.usedPercent));
  const xy = points.map((p) => [((p.at - t0) / Math.max(1, t1 - t0)) * (W - 4) + 2, H - 2 - (left(p) / 100) * (H - 4)]);
  return (
    <svg className="q-spark" width={W} height={H} role="img" aria-label={`近 7 天还剩的从 ${Math.round(left(points[0]))}% 到 ${Math.round(left(points[points.length - 1]))}%`}>
      <polyline points={xy.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ")} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" />
      <circle cx={xy[xy.length - 1][0]} cy={xy[xy.length - 1][1]} r={2.5} fill={color} />
    </svg>
  );
};

/** 旧的 API 层（规划 + 实施）的调用记录：筛选、导入导出照旧，收起来放在最后。 */
const LegacyCalls: React.FC<{ say: (m: unknown) => void }> = ({ say }) => {
  const [list, setList] = useState<UsageListResult | null>(null);
  const [mode, setMode] = useState("real");
  const [project, setProject] = useState("");
  const [model, setModel] = useState("");
  const [provider, setProvider] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);
  const filter = { mode, project: project || undefined, model: model || undefined, provider: provider || undefined };
  const reload = useCallback(async () => { setList(await usageApi.list(filter)); }, [mode, project, model, provider]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { void reload().catch(say); }, [reload]); // eslint-disable-line react-hooks/exhaustive-deps
  const s = list?.summary;
  return (
    <>
      <p className="hint">规划 + 实施还在旧的 API 层上（v0.8 并进新循环），它的请求记在这里；Peck 循环和 ONE SHOT 的在上面的图里。</p>
      <div className="set-inline">
        <Filter label="哪种请求" value={mode} onChange={setMode} options={[["real", "真实"], ["mock", "模拟"], ["all", "全部"]]} />
        <Filter label="项目" value={project} onChange={setProject} options={[["", "全部项目"], ...(list?.projects ?? []).map((p): [string, string] => [p, p])]} />
        <Filter label="供应商" value={provider} onChange={setProvider} options={[["", "全部供应商"], ...(list?.providers ?? []).map((p): [string, string] => [p, p])]} />
        <Filter label="模型" value={model} onChange={setModel} options={[["", "全部模型"], ...(list?.models ?? []).map((p): [string, string] => [p, p])]} />
        <span className="grow" />
        <Btn sm onClick={() => fileRef.current?.click()}>IMPORT</Btn>
        <input ref={fileRef} type="file" accept="application/json,.json" hidden onChange={(e) => { const f = e.target.files?.[0]; if (!f) return; void (async () => { const r = await usageApi.importJson(await f.text()); await reload(); say(`导入好了：新增 ${r.imported}，重复跳过 ${r.skipped}，无效 ${r.invalid}`); })().catch(say); e.target.value = ""; }} />
        <Btn sm onClick={() => void usageApi.exportCsv(filter).then((p) => say(p ? "CSV 导出好了" : "没有导出")).catch(say)}>EXPORT CSV</Btn>
      </div>
      {s && <p className="hint">{s.requestCount} 次请求 · 输入 {num(s.tokens.input)} · 输出 {num(s.tokens.output)} Token{s.costs.map((c, i) => <span key={i}> · {c.currency ?? "未知币种"} 实际 {c.actual ?? "未知"}，估算 {c.estimated ?? "未知"}</span>)}</p>}
      <div className="quota-calls scroll">
        {(list?.calls ?? []).map((c) => (
          <p key={`${c.source}-${c.requestId}`}><span>{c.project || "未知项目"} · {c.provider || "未知供应商"} · {c.model}{c.mode === "mock" ? " · 模拟" : ""}</span><span className="hint">{c.status} · {when(c.startedAt)} · {c.usage ? num(c.usage.inputTokens) : "未知"} Token</span></p>
        ))}
        {list && list.calls.length === 0 && <p className="empty-line">还没有记录。</p>}
      </div>
      <Row name="Anthropic API 余额" desc="未知：普通的 Key 没有官方的余额接口。" />
    </>
  );
};
