import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Download, RefreshCw, Upload, Wallet, X } from "lucide-react";
import { usageApi, type BalanceCard, type ClientCard, type UsageListResult } from "@/lib/usage";
import { isMockMode } from "@/lib/ipc";

const statusLabel: Record<string, string> = { ok: "已读取", not_installed: "未安装", not_running: "未运行", not_logged_in: "未登录", fields_missing: "暂不可读取", read_failed: "读取失败", stale_stopped: "已停止 · 最后快照", unknown: "未知", missing_key: "未配置 Key" };
const fmtTime = (t: number | null) => t ? new Date(t).toLocaleString() : "未知";
const fmtTok = (v: number | null) => v === null ? "未知" : v.toLocaleString();

export function UsagePage({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [list, setList] = useState<UsageListResult | null>(null);
  const [clients, setClients] = useState<ClientCard[]>([]);
  const [balances, setBalances] = useState<BalanceCard[]>([]);
  const [mode, setMode] = useState("real");
  const [project, setProject] = useState("");
  const [model, setModel] = useState("");
  const [provider, setProvider] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [manual, setManual] = useState<Record<string, { pct: string; note: string; reset?: string }>>({});
  const fileRef = useRef<HTMLInputElement>(null);
  const filter = { mode, project: project || undefined, model: model || undefined, provider: provider || undefined };

  const reloadLedger = useCallback(async () => { setList(await usageApi.list(filter)); }, [mode, project, model, provider]); // eslint-disable-line react-hooks/exhaustive-deps
  const perform = async (fn: () => Promise<unknown>) => { setError(""); try { await fn(); } catch (e) { setError(String(e)); } };

  useEffect(() => { if (!open) return; void perform(async () => { await reloadLedger(); setClients(await usageApi.clients()); setBalances(await usageApi.balances()); }); }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (!open) return; void perform(reloadLedger); }, [mode, project, model, provider]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!open || isMockMode()) return;
    let closed = false;
    const un: (() => void)[] = [];
    void listen<ClientCard[]>("usage-updated", (e) => {
      setClients(e.payload);
      void perform(async () => { await reloadLedger(); setBalances(await usageApi.balances()); });
    }).then((f) => closed ? f() : un.push(f)).catch((e) => { if (!closed) setError(String(e)); });
    void listen<BalanceCard[]>("usage-balances-updated", (e) => setBalances(e.payload))
      .then((f) => closed ? f() : un.push(f)).catch((e) => { if (!closed) setError(String(e)); });
    return () => { closed = true; un.forEach((f) => f()); };
  }, [open, reloadLedger]);

  if (!open) return null;
  const s = list?.summary;
  return <div role="dialog" aria-modal="true" aria-label="用量监控" className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-5">
    <div className="flex h-[min(860px,94vh)] w-full max-w-6xl flex-col overflow-hidden rounded-xl border border-[#373E4B] bg-[#14171C]">
      <header className="flex shrink-0 items-center justify-between border-b border-[#262B34] px-5 py-4">
        <div><h2 className="text-sm font-semibold">用量监控</h2><p className="mt-1 text-xs text-[#F59E0B]">API 台账仅覆盖本安装的 Peck 记录；客户端卡显示可取得的订阅额度。{isMockMode() && "当前为浏览器演示态（模拟标注），不代表真实读取。"}</p></div>
        <div className="flex gap-2">
          <button className="rounded-md border border-[#373E4B] px-3 py-1.5 text-xs" onClick={() => void perform(async () => { setClients(await usageApi.refresh()); setNotice("已按运行状态刷新客户端卡（未运行的客户端不会被启动）。"); })}><RefreshCw className="mr-1 inline h-3 w-3" />刷新额度</button>
          <button aria-label="关闭用量监控" onClick={onClose}><X className="h-4 w-4" /></button>
        </div>
      </header>
      {error && <div role="alert" className="bg-[#EF4444]/10 px-5 py-2 text-xs text-[#FCA5A5]">{error}</div>}
      {notice && <p role="status" className="px-5 py-2 text-xs text-[#6EE7B7]">{notice}</p>}
      <div className="flex-1 space-y-6 overflow-y-auto p-5">
        <section className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-medium">API 台账（Peck 记录）</h3>
            <select aria-label="记录模式" value={mode} onChange={(e) => setMode(e.target.value)} className="rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1 text-xs"><option value="real">仅真实 real</option><option value="mock">仅模拟 mock</option><option value="all">全部</option></select>
            <select aria-label="项目筛选" value={project} onChange={(e) => setProject(e.target.value)} className="rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1 text-xs"><option value="">全部项目</option>{list?.projects.map((p) => <option key={p}>{p}</option>)}</select>
            <select aria-label="供应商筛选" value={provider} onChange={(e) => setProvider(e.target.value)} className="rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1 text-xs"><option value="">全部供应商</option>{list?.providers.map((p) => <option key={p} value={p}>{p}</option>)}</select>
            <select aria-label="模型筛选" value={model} onChange={(e) => setModel(e.target.value)} className="rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1 text-xs"><option value="">全部模型</option>{list?.models.map((m) => <option key={m}>{m}</option>)}</select>
            <button aria-label="重置筛选" className="rounded-md border border-[#373E4B] px-3 py-1.5 text-xs" disabled={mode === "real" && project === "" && provider === "" && model === ""} onClick={() => { setMode("real"); setProject(""); setProvider(""); setModel(""); }}>重置筛选</button>
            <button className="rounded-md border border-[#373E4B] px-3 py-1.5 text-xs" onClick={() => fileRef.current?.click()}><Upload className="mr-1 inline h-3 w-3" />导入 Peck 记录</button>
            <input ref={fileRef} type="file" accept="application/json,.json" className="hidden" onChange={(e) => { const f = e.target.files?.[0]; if (!f) return; void perform(async () => { const r = await usageApi.importJson(await f.text()); await reloadLedger(); setNotice(`导入完成：新增 ${r.imported}，重复跳过 ${r.skipped}，无效 ${r.invalid}。`); }); e.target.value = ""; }} />
            <button className="rounded-md border border-[#373E4B] px-3 py-1.5 text-xs" onClick={() => void perform(async () => { const p = await usageApi.exportCsv(filter); setNotice(p ? `CSV 已导出：${p}` : "已取消导出。"); })}><Download className="mr-1 inline h-3 w-3" />导出 CSV</button>
          </div>
          {!s ? <p className="text-xs text-[#8B949E]">正在读取台账…</p> : <div className="grid gap-3 text-xs sm:grid-cols-3">
            <div className="rounded-lg border border-[#262B34] bg-[#181B21] p-3">请求数：{s.requestCount}<br />输入 {fmtTok(s.tokens.input)} · 输出 {fmtTok(s.tokens.output)}<br />缓存读 {fmtTok(s.tokens.cacheRead)} · 缓存建 {fmtTok(s.tokens.cacheCreation)} · 推理 {fmtTok(s.tokens.reasoning)}<br />{s.tokens.unknownCount > 0 && <span className="text-[#F59E0B]">以上为已知部分，{s.tokens.unknownCount} 条存在未知字段，不填 0</span>}</div>
            {s.costs.length === 0 ? <div className="rounded-lg border border-[#262B34] bg-[#181B21] p-3">费用：未知</div> : s.costs.map((c, i) => <div key={i} className="rounded-lg border border-[#262B34] bg-[#181B21] p-3">币种：{c.currency ?? "未知币种"}（不跨币种相加）<br />实际费用：{c.actual === null ? `未知（${c.actualUnknownCount} 条缺失）` : `${c.actual}${c.actualUnknownCount ? `（已知部分，另 ${c.actualUnknownCount} 条未知）` : ""}`}<br />估算费用：{c.estimated === null ? `未知（${c.estimatedUnknownCount} 条缺失）` : `${c.estimated}${c.estimatedUnknownCount ? `（已知部分，另 ${c.estimatedUnknownCount} 条未知）` : ""}`}<br />价格版本：{c.priceVersions.join(", ") || "未知"}</div>)}
          </div>}
          <div className="max-h-56 overflow-y-auto rounded-lg border border-[#262B34]">
            {(list?.calls ?? []).map((c) => <div key={`${c.source}-${c.requestId}`} className="flex justify-between gap-3 border-b border-[#262B34] px-3 py-2 text-[11px]"><span>{c.project || "未知项目"} · {c.provider || "未知供应商"} · {c.model} · {c.mode}{c.localId ? " · 本地标识" : ""}</span><span className="text-[#8B949E]">{c.status} · {fmtTime(c.startedAt)} · tokens {c.usage ? fmtTok(c.usage.inputTokens) : "未知"}</span></div>)}
            {list && list.calls.length === 0 && <p className="p-3 text-xs text-[#8B949E]">暂无记录。空列表就是空数据，不含假费用。</p>}
          </div>
        </section>
        <section className="space-y-3">
          <h3 className="text-sm font-medium">API 账户余额（与订阅额度分开）</h3>
          <div className="grid gap-3 sm:grid-cols-2">
            {(["deepseek", "anthropic"] as const).map((p) => { const b = balances.find((x) => x.provider === p); return <div key={p} className="rounded-lg border border-[#262B34] bg-[#181B21] p-3 text-xs"><div className="flex justify-between"><strong>{p === "deepseek" ? "DeepSeek API" : "Anthropic API"}</strong><span>{b ? statusLabel[b.status] ?? b.status : "未查询"}</span></div>
              {b?.balances.map((x, i) => <p key={i} className="mt-1">{b.status !== "ok" ? "上次成功快照 · " : ""}{x.currency}：{x.totalBalance}</p>)}
              <p className="mt-1 text-[#8B949E]">{b?.message ?? (p === "deepseek" ? "后端复用已配置 Keychain 查询，Key 不回前端。" : "未知：普通推理 Key 无官方余额接口。")}</p>
              <p className="text-[#8B949E]">查询：{fmtTime(b?.checkedAt ?? null)} · 最后成功：{fmtTime(b?.lastSuccessAt ?? null)}{b?.lastFailureAt ? ` · 失败 ${fmtTime(b.lastFailureAt)}` : ""}</p>
              <button className="mt-2 rounded-md border border-[#373E4B] px-3 py-1.5" onClick={() => void perform(async () => { const card = await usageApi.balance(p); setBalances((prev) => [...prev.filter((x) => x.provider !== p), card]); })}><Wallet className="mr-1 inline h-3 w-3" />查询余额</button></div>; })}
          </div>
        </section>
        <section className="space-y-3">
          <h3 className="text-sm font-medium">客户端订阅额度</h3>
          <div className="grid gap-3 sm:grid-cols-2">
            {clients.map((c) => <div key={c.client} className={`rounded-lg border p-3 text-xs ${c.manual ? "border-[#F59E0B]/50 bg-[#F59E0B]/5" : "border-[#262B34] bg-[#181B21]"}`}>
              <div className="flex justify-between"><strong>{c.label}{c.manual ? " · 手工快照" : ""}</strong><span className={c.status === "ok" ? "text-[#10B981]" : "text-[#F59E0B]"}>{statusLabel[c.status] ?? c.status}{c.fresh ? " · 最近采集" : c.lastSuccessAt ? " · 快照" : ""}</span></div>
              <p className="mt-1 text-[#8B949E]">来源：{c.source}<br />覆盖：{c.coverage}<br />运行中：{c.running ? "是" : "否"} · 已安装：{c.installed ? "是" : "否"} · 账户：{c.accountLabel ?? c.accountId ?? "未知（官方未提供）"}{c.tier ? ` · 套餐 ${c.tier}` : ""}</p>
              {c.windows.map((w, i) => <p key={i} className="mt-1">{w.label}：{w.remainingAmount !== null ? `剩余 ${w.remainingAmount}（供应商额度单位）` : `已用 ${w.usedPercent === null ? "未知" : `${w.usedPercent}%`}`} · 重置 {fmtTime(w.resetAtMs)}</p>)}
              {c.rawLabel && <p className="mt-1 text-[#8B949E]">{c.rawLabel}</p>}
              <p className="text-[#8B949E]">最后成功：{fmtTime(c.lastSuccessAt)} · 采集：{fmtTime(c.collectedAt)}{c.lastFailureAt ? ` · 失败 ${fmtTime(c.lastFailureAt)}` : ""}</p>
              {c.failureReason && <p className="mt-1 text-[#F59E0B]">{c.failureReason}</p>}
              {c.previous && <div className="mt-2 rounded border border-[#F59E0B]/30 p-2 text-[#8B949E]">
                <p>账号未核验的历史快照 · 不代表当前账户{c.previous.manual ? " · 手工录入" : ""}</p>
                <p>{c.previous.source} · {fmtTime(c.previous.capturedAt)} · 旧账户 {c.previous.accountId ?? "未提供"}</p>
                {c.previous.windows.map((w, i) => <p key={i}>{w.label}：{w.remainingAmount !== null ? `剩余 ${w.remainingAmount}（供应商单位）` : `已用 ${w.usedPercent ?? "未知"}%`}</p>)}
              </div>}
              <div className="mt-2 flex flex-wrap gap-2">
                <input aria-label={`${c.label} 手工已用比例`} placeholder="已用 %（未知勿填0）" value={manual[c.client]?.pct ?? ""} onChange={(e) => setManual((m) => ({ ...m, [c.client]: { ...m[c.client], pct: e.target.value, note: m[c.client]?.note ?? "" } }))} className="w-28 rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1" />
                <input aria-label={`${c.label} 手工来源说明`} placeholder="来源及账号说明（必填）" value={manual[c.client]?.note ?? ""} onChange={(e) => setManual((m) => ({ ...m, [c.client]: { ...m[c.client], pct: m[c.client]?.pct ?? "", note: e.target.value } }))} className="min-w-36 flex-1 rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1" />
                <label className="text-[#8B949E]">重置（可选）<input type="datetime-local" aria-label={`${c.label} 手工重置时间`} value={manual[c.client]?.reset ?? ""} onChange={(e) => setManual((m) => ({ ...m, [c.client]: { pct: m[c.client]?.pct ?? "", note: m[c.client]?.note ?? "", reset: e.target.value } }))} className="ml-1 rounded border border-[#373E4B] bg-[#0F1115] px-2 py-1" /></label>
                <button className="rounded-md border border-[#373E4B] px-2 py-1" onClick={() => void perform(async () => { const card = await usageApi.manual({ client: c.client, usedPercent: manual[c.client]?.pct ? Number(manual[c.client].pct) : null, resetAtMs: manual[c.client]?.reset ? new Date(manual[c.client].reset!).getTime() : null, note: manual[c.client]?.note ?? "" }); setClients((prev) => prev.map((x) => x.client === card.client ? card : x)); setNotice("手工快照已保存（带来源与时间，manual 标记）。"); })}>保存手工快照</button>
              </div>
            </div>)}
          </div>
        </section>
      </div>
    </div>
  </div>;
}
