import React, { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { isMockMode } from "@/lib/ipc";
import { usageApi, type BalanceCard, type ClientCard, type UsageListResult } from "@/lib/usage";
import { cn } from "@/lib/utils";
import { Btn, Field, MenuChip, ModHead, Row } from "@/ui";

// 现在的用量页（usage.rs）照旧可用，换了新外壳的样子和用词；v0.5 起站主用 Peck 做的「额度与花费」接上以后，这一页退役。

const STATUS: Record<string, string> = { ok: "读到了", not_installed: "没装", not_running: "没在运行", not_logged_in: "没登录", fields_missing: "现在读不到", read_failed: "读取失败", stale_stopped: "已停止 · 最后一次的快照", unknown: "未知", missing_key: "没配 Key" };
const when = (t: number | null | undefined) => (t ? new Date(t).toLocaleString("zh-CN", { hour12: false }) : "未知");
const num = (v: number | null) => (v === null ? "未知" : v.toLocaleString());

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

export const UsagePage: React.FC<{ onClose: () => void; say: (message: unknown) => void }> = ({ onClose, say }) => {
  const [list, setList] = useState<UsageListResult | null>(null);
  const [clients, setClients] = useState<ClientCard[]>([]);
  const [balances, setBalances] = useState<BalanceCard[]>([]);
  const [mode, setMode] = useState("real");
  const [project, setProject] = useState("");
  const [model, setModel] = useState("");
  const [provider, setProvider] = useState("");
  const [manual, setManual] = useState<Record<string, { pct: string; note: string; reset?: string }>>({});
  const fileRef = useRef<HTMLInputElement>(null);
  const filter = { mode, project: project || undefined, model: model || undefined, provider: provider || undefined };

  const reload = useCallback(async () => { setList(await usageApi.list(filter)); }, [mode, project, model, provider]); // eslint-disable-line react-hooks/exhaustive-deps
  const act = async (fn: () => Promise<unknown>) => { try { await fn(); } catch (e) { say(e); } };

  useEffect(() => { void act(async () => { setClients(await usageApi.clients()); setBalances(await usageApi.balances()); }); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { void act(reload); }, [reload]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (isMockMode()) return;
    let closed = false;
    const off: Array<() => void> = [];
    void listen<ClientCard[]>("usage-updated", (e) => { setClients(e.payload); void act(async () => { await reload(); setBalances(await usageApi.balances()); }); })
      .then((f) => (closed ? f() : off.push(f))).catch(say);
    void listen<BalanceCard[]>("usage-balances-updated", (e) => setBalances(e.payload)).then((f) => (closed ? f() : off.push(f))).catch(say);
    return () => { closed = true; off.forEach((f) => f()); };
  }, [reload]); // eslint-disable-line react-hooks/exhaustive-deps

  const s = list?.summary;
  const setM = (client: string, patch: Partial<{ pct: string; note: string; reset: string }>) => setManual((m) => ({ ...m, [client]: { pct: m[client]?.pct ?? "", note: m[client]?.note ?? "", reset: m[client]?.reset, ...patch } }));
  return (
    <section className="col page" aria-label="额度与花费">
      <div className="pk-mod col-grow page-mod">
        <ModHead label="QUOTA" note={isMockMode() ? "额度与花费 · 浏览器演示，没有读真实的客户端" : "额度与花费"}>
          <Btn v="ghost" sm onClick={() => void act(async () => { setClients(await usageApi.refresh()); say("刷新过了；没在运行的客户端不会被启动"); })}>REFRESH</Btn>
          <Btn v="ghost" sm icon="x" onClick={onClose}>CLOSE</Btn>
        </ModHead>
        <div className="set-pane scroll quota">
          <h2 className="t-title">API 花了多少</h2>
          <p className="lead">只算这台 Mac 上 Peck 自己发的请求。未知的不填 0，不同币种不加在一起。</p>
          <div className="set-inline">
            <Filter label="哪种请求" value={mode} onChange={setMode} options={[["real", "真实"], ["mock", "模拟"], ["all", "全部"]]} />
            <Filter label="项目" value={project} onChange={setProject} options={[["", "全部项目"], ...(list?.projects ?? []).map((p): [string, string] => [p, p])]} />
            <Filter label="供应商" value={provider} onChange={setProvider} options={[["", "全部供应商"], ...(list?.providers ?? []).map((p): [string, string] => [p, p])]} />
            <Filter label="模型" value={model} onChange={setModel} options={[["", "全部模型"], ...(list?.models ?? []).map((p): [string, string] => [p, p])]} />
            <Btn v="ghost" sm disabled={mode === "real" && !project && !provider && !model} onClick={() => { setMode("real"); setProject(""); setProvider(""); setModel(""); }}>RESET</Btn>
            <span className="grow" />
            <Btn sm onClick={() => fileRef.current?.click()}>IMPORT</Btn>
            <input ref={fileRef} type="file" accept="application/json,.json" hidden onChange={(e) => { const f = e.target.files?.[0]; if (!f) return; void act(async () => { const r = await usageApi.importJson(await f.text()); await reload(); say(`导入好了：新增 ${r.imported}，重复跳过 ${r.skipped}，无效 ${r.invalid}`); }); e.target.value = ""; }} />
            <Btn sm onClick={() => void act(async () => { const p = await usageApi.exportCsv(filter); say(p ? "CSV 导出好了" : "没有导出"); })}>EXPORT CSV</Btn>
          </div>
          {!s ? <p className="empty-line">在读记录…</p> : (
            <div className="quota-cards">
              <div className="pk-mod pk-mod--sunk">
                <p className="lbl">REQUESTS</p>
                <p><b className="pk-readout pk-readout--num">{s.requestCount}</b></p>
                <p className="hint">输入 {num(s.tokens.input)} · 输出 {num(s.tokens.output)} · 缓存读 {num(s.tokens.cacheRead)} · 缓存建 {num(s.tokens.cacheCreation)} · 思考 {num(s.tokens.reasoning)} Token</p>
                {s.tokens.unknownCount > 0 && <p className="hint is-warn">这是已知的部分，另有 {s.tokens.unknownCount} 条有未知的字段</p>}
              </div>
              {s.costs.length === 0 ? <div className="pk-mod pk-mod--sunk"><p className="lbl">COST</p><p className="hint">费用：未知</p></div> : s.costs.map((c, i) => (
                <div key={i} className="pk-mod pk-mod--sunk">
                  <p className="lbl">COST · {c.currency ?? "未知币种"}</p>
                  <p className="hint">实际：{c.actual === null ? `未知（${c.actualUnknownCount} 条没有）` : `${c.actual}${c.actualUnknownCount ? `（另有 ${c.actualUnknownCount} 条未知）` : ""}`}</p>
                  <p className="hint">估算：{c.estimated === null ? `未知（${c.estimatedUnknownCount} 条没有）` : `${c.estimated}${c.estimatedUnknownCount ? `（另有 ${c.estimatedUnknownCount} 条未知）` : ""}`}</p>
                  <p className="hint">价格表：{c.priceVersions.join("、") || "未知"}</p>
                </div>
              ))}
            </div>
          )}
          <div className="quota-calls scroll">
            {(list?.calls ?? []).map((c) => (
              <p key={`${c.source}-${c.requestId}`}><span>{c.project || "未知项目"} · {c.provider || "未知供应商"} · {c.model}{c.mode === "mock" ? " · 模拟" : ""}</span><span className="hint">{c.status} · {when(c.startedAt)} · {c.usage ? num(c.usage.inputTokens) : "未知"} Token</span></p>
            ))}
            {list && list.calls.length === 0 && <p className="empty-line">还没有记录。</p>}
          </div>

          <h2 className="t-title">API 账户余额</h2>
          <p className="lead">和订阅额度是两回事。</p>
          <div className="set-rows">
            {(["deepseek", "anthropic"] as const).map((p) => {
              const b = balances.find((x) => x.provider === p);
              return (
                <Row key={p} name={p === "deepseek" ? "DeepSeek API" : "Anthropic API"} desc={<>
                  {b?.balances.map((x, i) => <span key={i}>{b.status !== "ok" ? "上次成功时 · " : ""}{x.currency}：{x.totalBalance}；</span>)}
                  {b?.message ?? (p === "deepseek" ? "用已经配好的 Key 查，Key 不出后端。" : "未知：普通的 Key 没有官方的余额接口。")} · 查于 {when(b?.checkedAt)}
                </>}>
                  <em className="pk-tag">{b ? STATUS[b.status] ?? b.status : "没查过"}</em>
                  <Btn sm onClick={() => void act(async () => { const card = await usageApi.balance(p); setBalances((prev) => [...prev.filter((x) => x.provider !== p), card]); })}>CHECK</Btn>
                </Row>
              );
            })}
          </div>

          <h2 className="t-title">客户端的订阅额度</h2>
          <p className="lead">只在对应的客户端运行时读；退出就停，留最后一次的快照。读不到的不推算。</p>
          <div className="set-rows">
            {clients.map((c) => (
              <div key={c.client} className="set-block">
                <Row name={<>{c.label}{c.manual ? " · 手工记的" : ""}</>} off={c.status !== "ok"} desc={<>
                  {c.windows.map((w, i) => <span key={i}>{w.label}：{w.remainingAmount !== null ? `还剩 ${w.remainingAmount}` : `用了 ${w.usedPercent === null ? "未知" : `${w.usedPercent}%`}`} · {when(w.resetAtMs)} 重置；</span>)}
                  来源：{c.source} · {c.running ? "在运行" : "没在运行"} · 账户 {c.accountLabel ?? c.accountId ?? "未知"}{c.tier ? ` · ${c.tier}` : ""} · 最后成功 {when(c.lastSuccessAt)}
                  {c.failureReason ? ` · ${c.failureReason}` : ""}
                  {c.previous ? ` · 另有一份没核对过账号的旧快照（${when(c.previous.capturedAt)}），不代表现在的账户` : ""}
                </>}>
                  <em className={cn("pk-tag", c.status === "ok" && "pk-tag--ok")}>{STATUS[c.status] ?? c.status}</em>
                </Row>
                <div className="set-inline">
                  <Field width={130} aria-label={`${c.label} 手工记：用了百分之几`} placeholder="用了 %" value={manual[c.client]?.pct ?? ""} onChange={(e) => setM(c.client, { pct: e.target.value })} />
                  <Field aria-label={`${c.label} 手工记：从哪看来的`} placeholder="从哪看来的、哪个账号（要填）" value={manual[c.client]?.note ?? ""} onChange={(e) => setM(c.client, { note: e.target.value })} />
                  <Field width={230} type="datetime-local" aria-label={`${c.label} 手工记：什么时候重置`} value={manual[c.client]?.reset ?? ""} onChange={(e) => setM(c.client, { reset: e.target.value })} />
                  <Btn sm onClick={() => void act(async () => {
                    const m = manual[c.client];
                    const card = await usageApi.manual({ client: c.client, usedPercent: m?.pct ? Number(m.pct) : null, resetAtMs: m?.reset ? new Date(m.reset).getTime() : null, note: m?.note ?? "" });
                    setClients((prev) => prev.map((x) => (x.client === card.client ? card : x)));
                    say("手工记的快照存好了");
                  })}>SAVE</Btn>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
};
