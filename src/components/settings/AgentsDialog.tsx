import React, { useState } from "react";
import { Cpu, RefreshCw, X } from "lucide-react";
import { ipc, isMockMode } from "@/lib/ipc";
import type { AgentInfo } from "@/lib/types";

interface AgentsDialogProps {
  open: boolean;
  agents: AgentInfo[];
  onClose: () => void;
  onRefresh: () => Promise<void>;
}

export const AgentsDialog: React.FC<AgentsDialogProps> = ({ open, agents, onClose, onRefresh }) => {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  if (!open) return null;
  const editConfig = async (id: string) => {
    setError(""); setBusy(true);
    try { setContent(await ipc.agentGetConfig(id)); setEditingId(id); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const saveConfig = async () => {
    if (!editingId) return;
    setError(""); setBusy(true);
    try { await ipc.agentSaveConfig(editingId, content); await onRefresh(); setEditingId(null); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  return <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 p-6 backdrop-blur-xs" role="dialog" aria-modal="true" aria-label="Agent 设置">
    <div className="flex max-h-full w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-[#262B34] bg-[#181B21] shadow-2xl">
      <div className="flex items-center justify-between border-b border-[#262B34] px-5 py-4">
        <h3 className="flex items-center gap-2 text-sm font-semibold"><Cpu className="h-4 w-4 text-[#00E5FF]" />Agent 配置与检测</h3>
        <button aria-label="关闭设置" onClick={onClose}><X className="h-4 w-4 text-[#8B949E]" /></button>
      </div>
      <div className="space-y-3 overflow-y-auto p-5 text-xs">
        <p className="leading-relaxed text-[#8B949E]">无需同时订阅两家。使用 @muse 或 @grok 可单独完成任务；@mock 不调用 AI。检测到 CLI 仅表示本地程序存在，认证、额度与权限需要在对应 CLI 中配置。</p>
        {isMockMode() && <p className="rounded border border-[#F59E0B]/30 bg-[#F59E0B]/10 p-2 text-[#F59E0B]">浏览器演示数据；配置编辑不写入本机文件。</p>}
        {error && <p role="alert" className="text-[#FCA5A5]">{error}</p>}
        {agents.map((agent) => <div key={agent.id} className="rounded-lg border border-[#262B34] bg-[#14171C] p-3.5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div><strong className="text-[#00E5FF]">@{agent.id}</strong><span className="ml-2">{agent.name}</span>{agent.version && <span className="ml-2 text-[#8B949E]">{agent.version}</span>}</div>
            <span className={`rounded-full px-2.5 py-1 ${agent.isMock ? "bg-[#3B82F6]/15 text-[#60A5FA]" : agent.available ? "bg-[#F59E0B]/15 text-[#F59E0B]" : "bg-[#EF4444]/15 text-[#FCA5A5]"}`}>
              {agent.isMock ? "内置模拟可用" : agent.available ? "已检测到 CLI · 认证未验证" : "CLI 未安装"}
            </span>
          </div>
          <p className="mt-2 text-[#8B949E]">{agent.diagnostic || (agent.available ? "演示数据不代表本机认证状态。" : "请安装 CLI 或修改路径；也可继续使用模拟 Agent。")}</p>
          <code className="mt-2 block break-all text-[11px] text-[#C9D1D9]">{agent.command} {agent.args.join(" ")}</code>
          {agent.resolvedPath && <p className="mt-1 break-all font-mono text-[11px] text-[#8B949E]">{agent.resolvedPath}</p>}
          {agent.configPath && <p className="mt-1 break-all text-[11px] text-[#6E7681]">配置：{agent.configPath}</p>}
          <button disabled={busy} onClick={() => void editConfig(agent.id)} className="mt-2 text-[#00E5FF] disabled:opacity-40">编辑配置</button>
        </div>)}
        {editingId && <div className="space-y-2 rounded-lg border border-[#00E5FF]/40 p-3">
          <h4 className="font-semibold">编辑 @{editingId}</h4>
          <p className="text-[#8B949E]">保存后下次运行生效。密钥建议交由对应 CLI 登录管理，避免写入项目文件。不会自动放开 CLI 的权限策略。</p>
          <textarea aria-label="Agent TOML 配置" value={content} onChange={(e) => setContent(e.target.value)} rows={12} spellCheck={false} className="w-full rounded bg-[#0F1115] p-3 font-mono text-xs text-[#E6EDF3] outline-none" />
          <div className="flex gap-3"><button disabled={busy} onClick={() => void saveConfig()} className="rounded bg-[#00E5FF] px-3 py-1.5 font-semibold text-[#0F1115]">保存并重新检测</button><button disabled={busy} onClick={() => setEditingId(null)}>取消</button></div>
        </div>}
      </div>
      <div className="flex items-center justify-between border-t border-[#262B34] px-5 py-3 text-xs">
        <button disabled={busy} onClick={() => { setBusy(true); setError(""); void onRefresh().catch((e) => setError(String(e))).finally(() => setBusy(false)); }} className="flex items-center gap-1.5 text-[#00E5FF]"><RefreshCw className="h-3 w-3" />重新检测</button>
        <button onClick={onClose} className="rounded bg-[#00E5FF] px-4 py-1.5 font-semibold text-[#0F1115]">完成</button>
      </div>
    </div>
  </div>;
};
