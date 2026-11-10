import { useEffect, useState } from "react";
import { Download, KeyRound, Plus, RefreshCw, Send, Square, X } from "lucide-react";
import { api, type ApiCall, type ApiConfig, type ApiEvent, type ApiMode, type ApiProvider, type ApiStatus, type ModelProfile, type Thinking } from "@/lib/api";
import { isMockMode } from "@/lib/ipc";

const inputClass = "w-full rounded-md border border-[#373E4B] bg-[#0F1115] px-3 py-2 text-xs text-[#E6EDF3] outline-none focus:border-[#00E5FF] disabled:opacity-50";
const buttonClass = "shrink-0 whitespace-nowrap rounded-md border border-[#373E4B] px-3 py-2 text-xs hover:border-[#00E5FF] disabled:opacity-40";
const statusLabel: Record<string, string> = { running: "请求中", completed: "已完成", failed: "失败", cancelled: "已停止", interrupted: "已中断" };
const thinkingLabel: Record<Thinking, string> = { unknown: "能力待确认", never: "不能思考", switch: "可开关思考", always: "始终思考" };

export function ApiWorkbench({ open, onClose, onMode }: { open: boolean; onClose: () => void; onMode: (mode: ApiMode) => void }) {
  const [status, setStatus] = useState<ApiStatus | null>(null);
  const [config, setConfig] = useState<ApiConfig | null>(null);
  const [tab, setTab] = useState<"settings" | "request">("settings");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({});
  const [editor, setEditor] = useState<ModelProfile | null>(null);
  const [role, setRole] = useState("worker");
  const [prompt, setPrompt] = useState("");
  const [scenario, setScenario] = useState("success");
  const [running, setRunning] = useState<string | null>(null);
  const [selected, setSelected] = useState<ApiCall | null>(null);
  const [text, setText] = useState("");
  const [reasoning, setReasoning] = useState("");
  const native = !isMockMode();

  const refresh = async (reset = true) => {
    const next = await api.status();
    setStatus(next); onMode(next.mode);
    if (reset) { setConfig(next.config); setDirty(false); }
    return next;
  };
  const perform = async (action: () => Promise<unknown>) => {
    setSaving(true); setError(""); setNotice("");
    try { await action(); } catch (e) { setError(String(e)); try { await refresh(false); } catch { /* Keep the original actionable error. */ } }
    finally { setSaving(false); }
  };
  useEffect(() => {
    if (open && native) void perform(() => refresh());
    if (!open) setKeyInputs({});
    // Refresh only when opening; request deltas must not reload editable settings.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  if (!open) return null;
  const busy = saving || !!running;
  const agent = config?.agents.find(a => a.role === role);
  const model = config?.models.find(m => m.id === agent?.modelId);
  const allowance = status?.allowances.find(a => a.apiModel === model?.apiModel && a.providerId === model?.providerId);
  const updateConfig = (next: ApiConfig) => { setConfig(next); setDirty(true); setNotice(""); };
  const receive = (event: ApiEvent) => {
    if (event.type === "Text") setText(t => t + event.text);
    else if (event.type === "Reasoning") setReasoning(t => t + event.text);
    else if (event.type === "PersistenceError") setError(event.message);
    else if (event.type === "Finished") {
      setRunning(null); setSelected(event.call); setText(event.call.text); setReasoning(event.call.reasoning);
      if (event.call.error) setError(event.call.error);
      void refresh(false).catch(e => setError(String(e)));
    }
  };
  const send = async () => {
    if (!prompt.trim() || running || saving || dirty) return;
    const id = crypto.randomUUID();
    setRunning(id); setSelected(null); setText(""); setReasoning(""); setError(""); setNotice("");
    try { await api.start(id, role, prompt.trim(), status?.mode === "mock" ? scenario : "success", receive); }
    catch (e) { setError(String(e)); setRunning(null); }
  };

  return <div role="dialog" aria-modal="true" aria-label="API 工作台" className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-5 backdrop-blur-xs">
    <div className="flex h-[min(820px,94vh)] w-full max-w-5xl flex-col overflow-hidden rounded-xl border border-[#373E4B] bg-[#14171C] shadow-2xl">
      <header className="flex shrink-0 items-center justify-between gap-4 border-b border-[#262B34] px-5 py-4">
        <div><h2 className="text-sm font-semibold">API 工作台</h2><p className="mt-1 text-xs text-[#8B949E]">配置 Supervisor 与 Worker，查看模型响应及调用记录。</p></div>
        <div className="flex items-center gap-3">
          {native && status && <select aria-label="API 运行模式" disabled={busy || dirty} value={status.mode} onChange={e => void perform(async () => { await api.setMode(e.target.value as ApiMode); setKeyInputs({}); setSelected(null); setText(""); setReasoning(""); await refresh(); })} className={inputClass}>
            <option value="mock">桌面 · API 模拟</option><option value="real">桌面 · 真实 API</option>
          </select>}
          <button aria-label="关闭 API 工作台" disabled={busy} onClick={() => { setKeyInputs({}); onClose(); }} className="disabled:opacity-30"><X className="h-4 w-4" /></button>
        </div>
      </header>
      {!native ? <p className="p-6 text-sm text-[#F59E0B]">API 工作台使用桌面端密钥、数据库和请求服务。请打开 Peck.app；浏览器演示不代替原生 API 测试。</p> : <>
        <div className={`shrink-0 border-b border-[#262B34] px-5 py-2 text-xs ${status?.mode === "real" ? "bg-[#F59E0B]/10 text-[#F59E0B]" : "bg-[#3B82F6]/10 text-[#93C5FD]"}`}>
          {status?.mode === "real" ? "真实 API：发送请求会消耗授权次数与账户费用；失败和重试也计数。" : "API 模拟：使用真实桌面数据库和模拟模型，密钥只存本次进程内存；不联网、不消耗真实授权次数。"}
        </div>
        <nav className="flex shrink-0 gap-5 border-b border-[#262B34] px-5 text-xs">
          {([['settings', '模型与密钥'], ['request', '运行与历史']] as const).map(([id, label]) => <button key={id} onClick={() => setTab(id)} className={`border-b-2 py-3 ${tab === id ? "border-[#00E5FF] text-[#00E5FF]" : "border-transparent text-[#8B949E]"}`}>{label}</button>)}
        </nav>
        {error && <div role="alert" className="shrink-0 bg-[#EF4444]/10 px-5 py-2 text-xs text-[#FCA5A5]">{error}</div>}
        {notice && <p role="status" className="shrink-0 px-5 py-2 text-xs text-[#6EE7B7]">{notice}</p>}
        {!status || !config ? <p className="p-5 text-xs text-[#8B949E]">正在读取桌面设置…</p> : tab === "settings" ? <div className="flex-1 space-y-5 overflow-y-auto p-5">
          <section className="space-y-3">
            <div className="flex items-center justify-between"><h3 className="text-sm font-medium">供应商密钥</h3><button disabled={busy} onClick={() => void perform(async () => { await api.loadKeys(); await refresh(false); })} className={buttonClass}><KeyRound className="mr-1 inline h-3 w-3" />读取密钥状态</button></div>
            {status.providers.map(p => {
              const key = status.keys.find(k => k.providerId === p.id);
              const lastSuccess = status.calls.find(c => c.mode === "real" && c.model.providerId === p.id && c.status === "completed");
              return <div key={p.id} className="rounded-lg border border-[#262B34] bg-[#181B21] p-3">
                <div className="flex justify-between gap-3 text-xs"><strong>{p.name}</strong><span className="text-[#8B949E]">{{ unloaded: "尚未读取钥匙串", missing: "未保存", saved: "Key 已保存 · 当前有效性待验证", unavailable: "钥匙串暂不可读取" }[key?.state ?? "missing"]}</span></div>
                <p className="mt-1 break-all text-[11px] text-[#6E7681]">{p.baseUrl}</p>
                {p.id === "aliyun" && <div className="mt-2 grid gap-2 text-xs sm:grid-cols-2">
                  <label>服务地域<select aria-label="阿里云服务地域" disabled={busy} value={config.aliyunRegion} onChange={e => updateConfig({ ...config, aliyunRegion: e.target.value as ApiConfig["aliyunRegion"] })} className={inputClass}><option value="beijing">中国内地 · 北京</option><option value="singapore">国际 · 新加坡</option></select></label>
                  <label>业务空间 ID（可选）<input aria-label="阿里云业务空间 ID" disabled={busy} value={config.aliyunWorkspaceId} onChange={e => updateConfig({ ...config, aliyunWorkspaceId: e.target.value.trim() })} placeholder="留空使用地域通用入口" className={inputClass} /></label>
                  <p className="text-[#8B949E] sm:col-span-2">地域须与 Key 一致；填写业务空间 ID 可使用官方空间域名。保存设置后生效。</p>
                </div>}
                {lastSuccess && <p className="mt-1 text-[11px] text-[#8B949E]">上次真实调用成功：{lastSuccess.model.apiModel} · {new Date(lastSuccess.startedAt).toLocaleString()}</p>}
                <div className="mt-3 flex gap-2"><input type="password" aria-label={`${p.name} API Key`} autoComplete="off" spellCheck={false} disabled={busy} value={keyInputs[p.id] ?? ""} placeholder={status.mode === "mock" ? "可填假 Key，模拟请求无需真实密钥" : "输入或替换 API Key"} onChange={e => setKeyInputs(k => ({ ...k, [p.id]: e.target.value }))} className={inputClass} />
                  <button disabled={busy || !keyInputs[p.id]?.trim()} onClick={() => void perform(async () => { await api.setKey(p.id, keyInputs[p.id]); setKeyInputs(k => ({ ...k, [p.id]: "" })); await refresh(false); setNotice(status.mode === "mock" ? "模拟 Key 已保存在内存。" : "Key 已保存到钥匙串；尚未发起模型调用。"); })} className={buttonClass}>保存</button>
                  <button disabled={busy || key?.state !== "saved"} onClick={() => void perform(async () => { await api.setKey(p.id, null); await refresh(false); })} className={buttonClass}>移除</button>
                </div>
              </div>;
            })}
          </section>
          <section className="space-y-3"><h3 className="text-sm font-medium">角色与默认请求设置</h3>
            <div className="grid gap-3 sm:grid-cols-2">{config.agents.map((a, i) => {
              const m = config.models.find(m => m.id === a.modelId)!;
              const update = (patch: Partial<typeof a>) => updateConfig({ ...config, agents: config.agents.map((v, j) => j === i ? { ...v, ...patch } : v) });
              return <div key={a.role} className="space-y-3 rounded-lg border border-[#262B34] bg-[#181B21] p-3 text-xs">
                <strong>{a.role === "supervisor" ? "Supervisor · 规划与审查" : "Worker · 执行任务"}</strong>
                <label className="block space-y-1"><span className="text-[#8B949E]">模型</span><select aria-label={`${a.role} 模型`} disabled={busy} value={a.modelId} onChange={e => { const next = config.models.find(m => m.id === e.target.value)!; update({ modelId: next.id, thinking: next.thinking !== "never", effort: next.efforts.includes("medium") ? "medium" : next.efforts[0] ?? null, maxOutputTokens: Math.min(a.maxOutputTokens, next.maxOutputTokens) }); }} className={inputClass}>{config.models.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}</select></label>
                <div className="flex items-center justify-between"><span>{thinkingLabel[m.thinking]}</span><label className="flex gap-2"><input aria-label={`${a.role} 思考开关`} type="checkbox" disabled={busy || m.thinking !== "switch"} checked={a.thinking && m.thinking !== "unknown"} onChange={e => update({ thinking: e.target.checked, effort: e.target.checked ? m.efforts[0] ?? null : null })} />思考</label></div>
                {a.thinking && m.thinking !== "unknown" && m.efforts.length > 0 && <label className="block space-y-1"><span className="text-[#8B949E]">思考强度</span><select aria-label={`${a.role} 思考强度`} disabled={busy} value={a.effort ?? ""} onChange={e => update({ effort: e.target.value || null })} className={inputClass}><option value="">供应商默认</option>{m.efforts.map(e => <option key={e}>{e}</option>)}</select></label>}
                <label className="block space-y-1"><span className="text-[#8B949E]">单次输出上限（含思考 tokens）</span><input aria-label={`${a.role} 输出上限`} type="number" min={64} max={m.maxOutputTokens} disabled={busy} value={a.maxOutputTokens} onChange={e => update({ maxOutputTokens: Number(e.target.value) })} className={inputClass} /></label>
                {a.thinking && <p className="text-[11px] leading-relaxed text-[#8B949E]">思考也占输出预算。高强度可能需要更大上限，实际消耗以供应商返回值为准。</p>}
              </div>;
            })}</div>
          </section>
          <section className="space-y-3"><div className="flex justify-between"><h3 className="text-sm font-medium">模型配置</h3><button disabled={busy} onClick={() => setEditor({ id: crypto.randomUUID(), name: "自定义模型", providerId: "deepseek", apiModel: "", contextWindow: 131072, maxOutputTokens: 4096, thinking: "unknown", efforts: [], tools: false })} className="text-xs text-[#00E5FF]"><Plus className="mr-1 inline h-3 w-3" />添加模型</button></div>
            {config.models.map(m => <div key={m.id} className="flex items-center justify-between rounded border border-[#262B34] px-3 py-2 text-xs"><div><span>{m.name}</span><code className="ml-3 text-[#8B949E]">{m.apiModel}</code><span className="ml-3 text-[#8B949E]">{thinkingLabel[m.thinking]}</span></div><button disabled={busy} onClick={() => setEditor({ ...m })} className="text-[#00E5FF]">编辑</button></div>)}
            <p className="text-[11px] leading-relaxed text-[#8B949E]">模型能力来自已核实预设或手动配置。当前真实测试授权仅覆盖 Opus 5.5 与 DeepSeek Flash；其他模型可保存配置，发送前仍需相应预算。</p>
            {editor && <ModelEditor key={editor.id} model={editor} providers={status.providers} disabled={busy} onCancel={() => setEditor(null)} onChange={setEditor} onSave={(edited) => {
              updateConfig({ ...config, models: config.models.some(m => m.id === edited.id) ? config.models.map(m => m.id === edited.id ? edited : m) : [...config.models, edited], agents: config.agents.map(a => {
                if (a.modelId !== edited.id) return a;
                const thinking = edited.thinking === "always" || (edited.thinking !== "never" && a.thinking);
                return { ...a, thinking, effort: thinking && a.effort && edited.efforts.includes(a.effort) ? a.effort : null, maxOutputTokens: Math.min(a.maxOutputTokens, edited.maxOutputTokens) };
              }) }); setEditor(null);
            }} />}
          </section>
          <button disabled={busy || !dirty || !!editor} onClick={() => void perform(async () => { await api.saveConfig(config); await refresh(); setNotice("模型与角色设置已保存，下次请求生效。"); })} className="rounded-md bg-[#00E5FF] px-4 py-2 text-xs font-semibold text-[#0F1115] disabled:opacity-40">保存模型与角色设置{dirty ? " · 有未保存修改" : ""}</button>
        </div> : <div className="grid min-h-0 flex-1 grid-cols-[240px_minmax(0,1fr)]">
          <aside className="overflow-y-auto border-r border-[#262B34] p-3"><h3 className="mb-3 text-xs text-[#8B949E]">最近 50 次请求 · 保留真实 / 模拟标记</h3>{status.calls.length === 0 && <p className="text-xs text-[#6E7681]">暂无请求记录</p>}{status.calls.map(c => <button key={c.id} disabled={!!running} onClick={() => { setSelected(c); setText(c.text); setReasoning(c.reasoning); }} className={`mb-2 w-full rounded border p-2.5 text-left text-xs ${selected?.id === c.id ? "border-[#00E5FF]/50 bg-[#00E5FF]/5" : "border-[#262B34]"}`}><div className="flex justify-between"><span className={c.mode === "real" ? "text-[#F59E0B]" : "text-[#93C5FD]"}>{c.mode === "real" ? "真实 API" : "模拟"}</span><span>{statusLabel[c.status]}</span></div><p className="mt-1 truncate">{c.prompt}</p><p className="mt-1 truncate text-[10px] text-[#8B949E]">{c.model.name} · {new Date(c.startedAt).toLocaleTimeString()}</p></button>)}</aside>
          <main className="flex min-h-0 flex-col p-4">
            <div className="mb-3 flex shrink-0 items-center justify-between gap-3 text-xs text-[#8B949E]"><p>此处为单次模型请求。项目自动开发请回到主界面选择 API Worker。</p><button disabled={busy} className={buttonClass} onClick={() => void perform(async () => { const path = await api.exportCalls(); setNotice(path ? `脱敏调用记录已保存：${path}` : "已取消导出。"); })}><Download className="mr-1 inline h-3 w-3" />导出脱敏调用记录</button></div>
            <div className="mb-3 flex shrink-0 items-center gap-3 text-xs"><label>本次角色 <select aria-label="API 请求角色" disabled={busy} value={role} onChange={e => setRole(e.target.value)} className="ml-2 rounded border border-[#373E4B] bg-[#181B21] p-1.5"><option value="worker">Worker</option><option value="supervisor">Supervisor</option></select></label><span className="text-[#8B949E]">{model?.name} · {agent?.thinking ? `思考 ${agent.effort ?? "默认"}` : "不思考"}</span></div>
            <div className="mb-3 shrink-0 rounded-md border border-[#262B34] p-2 text-[11px] text-[#8B949E]">{status.mode === "mock" ? "模拟账本" : "跨项目累计授权"}：{allowance ? `${allowance.used} / ${allowance.limit} 次已占用，剩余 ${Math.max(0, allowance.limit - allowance.used)} 次` : "该模型尚无调用预算"}。失败/中断也保留占用，切换角色不会重置。</div>
            <div aria-live="polite" className="selectable-text min-h-0 flex-1 space-y-3 overflow-y-auto rounded-md border border-[#262B34] bg-[#0F1115] p-3 text-xs leading-relaxed">
              {selected && <div className="space-y-1 border-b border-[#262B34] pb-2 text-[11px] text-[#8B949E]"><p>{selected.mode === "real" ? "真实 API" : "模拟"} · {statusLabel[selected.status]} · {selected.role} · {selected.model.apiModel} · {selected.settings.thinking ? `思考 ${selected.settings.effort ?? "默认"}` : "不思考"}</p>{selected.runId && <p>{selected.runId} · 第 {selected.round} 轮 · {selected.id}</p>}<p>输入 {selected.response?.usage.inputTokens ?? "未知"} / 输出 {selected.response?.usage.outputTokens ?? "未知"} tokens{selected.response?.stopReason ? ` · 结束原因 ${selected.response.stopReason}` : ""}</p><p>缓存读取 {selected.response?.usage.cacheReadTokens ?? "未知"} / 缓存创建 {selected.response?.usage.cacheCreationTokens ?? "未知"} tokens · {selected.budgetCounted ? "已占用一次预算" : "发送前停止，未占用预算"}</p></div>}
              {reasoning && <details><summary className="cursor-pointer text-[#8B949E]">供应商返回的思考内容或摘要</summary><pre className="mt-2 whitespace-pre-wrap font-sans text-[#8B949E]">{reasoning}</pre></details>}
              <pre className="whitespace-pre-wrap break-words font-sans">{text || (running ? "正在等待模型响应…" : "在下方描述需要模型分析的任务。")}</pre>
              {selected?.error && <p className="text-[#FCA5A5]">{selected.error}</p>}
            </div>
            <div className="mt-3 shrink-0 space-y-2"><textarea aria-label="API 任务内容" value={prompt} onChange={e => setPrompt(e.target.value)} disabled={busy} rows={3} placeholder="描述实际任务，例如：为 API 台账设计第一版的数据字段和验收条件。" className={inputClass} />
              <div className="flex items-center justify-between gap-2">{status.mode === "mock" ? <select aria-label="API 模拟场景" value={scenario} disabled={busy} onChange={e => setScenario(e.target.value)} className="rounded border border-[#373E4B] bg-[#181B21] p-1.5 text-xs"><option value="success">模拟正常响应</option><option value="authentication">模拟认证失败</option><option value="rateLimit">模拟额度限制</option><option value="disconnect">模拟网络中断</option><option value="slow">模拟慢响应（可停止）</option></select> : <span className="text-[11px] text-[#8B949E]">每次发送计入真实授权预算</span>}
                {running ? <button onClick={() => void api.cancel(running).catch(e => setError(String(e)))} className={`${buttonClass} text-[#FCA5A5]`}><Square className="mr-1 inline h-3 w-3" />停止请求</button> : <button disabled={saving || dirty || !prompt.trim() || !allowance || allowance.used >= allowance.limit || model?.thinking === "unknown"} onClick={() => void send()} className="rounded-md bg-[#00E5FF] px-3 py-2 text-xs font-semibold text-[#0F1115] disabled:opacity-40"><Send className="mr-1 inline h-3 w-3" />{status.mode === "mock" ? "发送模拟请求" : "发送真实请求"}</button>}
              </div>{dirty && <p className="text-[11px] text-[#F59E0B]">请先在“模型与密钥”保存设置，再发送请求。</p>}
            </div>
          </main>
        </div>}
        <footer className="flex shrink-0 items-center justify-between border-t border-[#262B34] px-5 py-3 text-[11px] text-[#8B949E]"><span>密钥不写入普通设置或请求历史 · 模拟次数与真实授权分开</span><button disabled={busy || dirty} onClick={() => void perform(() => refresh())} className="flex items-center gap-1 disabled:opacity-30"><RefreshCw className="h-3 w-3" />刷新状态</button></footer>
      </>}
    </div>
  </div>;
}

function ModelEditor({ model, providers, disabled, onChange, onSave, onCancel }: { model: ModelProfile; providers: ApiProvider[]; disabled: boolean; onChange: (model: ModelProfile) => void; onSave: (model: ModelProfile) => void; onCancel: () => void }) {
  const [effortDraft, setEffortDraft] = useState(model.efforts.join(", "));
  useEffect(() => { setEffortDraft(model.efforts.join(", ")); }, [model.apiModel, model.providerId]);
  const known = (model.providerId === "anthropic" && model.apiModel === "claude-opus-5-5") || (model.providerId === "deepseek" && model.apiModel === "deepseek-flash");
  const changeId = (apiModel: string, providerId = model.providerId) => {
    const opus = providerId === "anthropic" && apiModel === "claude-opus-5-5";
    const flash = providerId === "deepseek" && apiModel === "deepseek-flash";
    onChange({ ...model, apiModel, providerId, thinking: opus ? "always" : flash ? "switch" : "unknown", efforts: opus ? ["low", "medium", "high", "xhigh", "max"] : flash ? ["low", "high", "max"] : [], tools: opus || flash });
  };
  return <div className="space-y-3 rounded-lg border border-[#00E5FF]/30 p-3 text-xs"><h4>编辑模型</h4><div className="grid grid-cols-2 gap-3">
    <label>显示名称<input aria-label="模型显示名称" disabled={disabled} value={model.name} onChange={e => onChange({ ...model, name: e.target.value })} className={inputClass} /></label>
    <label>供应商<select aria-label="模型供应商" disabled={disabled} value={model.providerId} onChange={e => changeId(model.apiModel, e.target.value)} className={inputClass}>{providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
    <label className="col-span-2">API 模型 ID<input aria-label="API 模型 ID" disabled={disabled} value={model.apiModel} onChange={e => changeId(e.target.value)} className={inputClass} /></label>
    <label>上下文上限<input aria-label="模型上下文上限" type="number" disabled={disabled} value={model.contextWindow} onChange={e => onChange({ ...model, contextWindow: Number(e.target.value) })} className={inputClass} /></label>
    <label>最大输出<input aria-label="模型最大输出" type="number" disabled={disabled} value={model.maxOutputTokens} onChange={e => onChange({ ...model, maxOutputTokens: Number(e.target.value) })} className={inputClass} /></label>
    <label>思考能力<select aria-label="模型思考能力" disabled={disabled || known} value={model.thinking} onChange={e => onChange({ ...model, thinking: e.target.value as Thinking, efforts: e.target.value === "never" ? [] : model.efforts })} className={inputClass}>{Object.entries(thinkingLabel).map(([v, name]) => <option key={v} value={v}>{name}</option>)}</select></label>
    <label>原生强度档位<input aria-label="模型思考档位" disabled={disabled || known || model.thinking === "never" || model.providerId === "aliyun"} value={known ? model.efforts.join(", ") : effortDraft} placeholder={model.providerId === "aliyun" ? "使用思考开关，留空" : "按该模型官方能力填写，例如 low, high"} onChange={e => setEffortDraft(e.target.value)} className={inputClass} /></label>
    <label className="flex items-center gap-2"><input aria-label="模型支持工具调用" type="checkbox" disabled={disabled || known} checked={model.tools} onChange={e => onChange({ ...model, tools: e.target.checked })} />支持函数工具调用</label>
  </div><p className="text-[#8B949E]">{known ? "能力采用已核实预设。" : "手动能力尚未经过真实服务验证；模型目录不等同于账户权限。"}</p><div className="flex gap-2"><button disabled={disabled || !model.name.trim() || !model.apiModel.trim()} onClick={() => onSave({ ...model, efforts: known ? model.efforts : model.thinking === "never" ? [] : effortDraft.split(",").map(s => s.trim()).filter(Boolean) })} className={buttonClass}>应用到设置</button><button disabled={disabled} onClick={onCancel} className={buttonClass}>取消</button></div></div>;
}
