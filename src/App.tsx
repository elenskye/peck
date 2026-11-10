import React, { useEffect, useState } from "react";
import { ArtifactBoard } from "@/components/board/ArtifactBoard";
import { ChatPanel } from "@/components/chat/ChatPanel";
import { SplitPane } from "@/components/layout/SplitPane";
import { TitleBar } from "@/components/layout/TitleBar";
import { AgentsDialog } from "@/components/settings/AgentsDialog";
import { ApiWorkbench } from "@/components/settings/ApiWorkbench";
import { UsagePage } from "@/components/usage/UsagePage";
import { api, type ApiMode, type ApiRole, type RunLimits } from "@/lib/api";
import { useRunEvents } from "@/hooks/useRunEvents";
import { useSession } from "@/hooks/useSession";
import { useWorkspace } from "@/hooks/useWorkspace";
import { ipc, isMockMode } from "@/lib/ipc";
import type { AgentInfo, RunEvent } from "@/lib/types";

export const App: React.FC = () => {
  const { workspaces, activeWorkspace, selectWorkspace, chooseWorkspace, error: workspaceError } = useWorkspace();
  const { sessions, activeSession, messages, switchSession, createNewSession, reloadMessages, error: sessionError } = useSession(activeWorkspace?.id);
  const { runs, activeRun, activeRunId, setActiveRunId, handleRunEvent, reloadRuns, error: runsError } = useRunEvents(activeSession?.id);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agentsDialogOpen, setAgentsDialogOpen] = useState(false);
  const [apiOpen, setApiOpen] = useState(false);
  const [usageOpen, setUsageOpen] = useState(false);
  const [apiMode, setApiMode] = useState<ApiMode | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [workspacePath, setWorkspacePath] = useState("");

  const refreshAgents = async () => { setAgents(await ipc.agentsList()); };
  const perform = async (action: () => Promise<unknown>) => {
    try { setError(""); await action(); } catch (e) { setError(String(e)); }
  };
  useEffect(() => { void perform(refreshAgents); }, []);
  useEffect(() => { if (!isMockMode()) void api.status().then(s => setApiMode(s.mode)).catch(e => setError(String(e))); }, []);

  const receiveEvent = (prompt: string, event: RunEvent) => {
    handleRunEvent(prompt, event);
    if (event.type === "RunFinished") {
      setBusy(false);
      void perform(async () => { await reloadMessages(); await reloadRuns(); });
    }
  };
  const handleSendPrompt = async (prompt: string, limits?: RunLimits, scenario = "success", role: ApiRole = "worker") => {
    if (!activeSession || busy) return;
    setBusy(true);
    try {
      setError("");
      const runId = limits
        ? await api.runStart(activeSession.id, prompt, limits, role, scenario, (event) => receiveEvent(prompt, event))
        : await ipc.runDispatch(activeSession.id, prompt, (event) => receiveEvent(prompt, event));
      setActiveRunId(runId);
      await reloadMessages();
    } catch (e) { setError(String(e)); setBusy(false); }
  };
  const handleRetryStep = async (runId: string, stepId: string) => {
    if (busy) return;
    const target = runs.find((r) => r.id === runId);
    if (!target) return;
    if (target.flowId.startsWith("api-")) {
      try {
        const tasks = await api.taskHistory(runId);
        const task = tasks.find(t => !t.parentId);
        if (!task) throw new Error("任务历史不可用。");
        const evidence = tasks.flatMap(t => t.tools).slice(-6).map(t => `${t.tool.name} (${t.status}): ${t.output.slice(0, 1500)}`).join("\n");
        await handleSendPrompt(`继续此前任务，从当前工作区文件开始核对后完成。不要直接重放旧工具；中断工具可能已经修改文件。\n原目标：${target.prompt}\n上次结果：${task.summary ?? target.summary}\n最后工具证据：\n${evidence}\n子任务结果：${tasks.filter(t => t.parentId).map(t => `${t.title}: ${t.summary ?? t.status}; 审查 ${t.review?.decision ?? "未审查"}`).join("\n")}`, task.limits, "success", target.flowId === "api-supervisor" ? "supervisor" : "worker");
      } catch (e) { setError(String(e)); }
      return;
    }
    setBusy(true);
    setActiveRunId(runId);
    try {
      setError("");
      await ipc.runRetryStep(runId, stepId, (event) => receiveEvent(target.prompt, event));
    } catch (e) { setError(String(e)); setBusy(false); }
  };
  const visibleError = error || workspaceError || sessionError || runsError;

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-[#0F1115] text-[#E6EDF3]">
      <TitleBar workspaces={workspaces} activeWorkspace={activeWorkspace} agents={agents} disabled={busy}
        onSelectWorkspace={(path) => { if (!busy) void perform(() => selectWorkspace(path)); }}
        onChooseWorkspace={() => { if (!busy) void perform(chooseWorkspace); }}
        onOpenAgentsDialog={() => setAgentsDialogOpen(true)} apiMode={apiMode} onOpenApi={() => setApiOpen(true)} onOpenUsage={() => setUsageOpen(true)} />
      {visibleError && <div role="alert" className="flex shrink-0 items-center justify-between gap-4 border-b border-[#EF4444]/40 bg-[#EF4444]/10 px-4 py-2 text-xs text-[#FCA5A5]">
        <span>{visibleError}</span><button onClick={() => setError("")} className="shrink-0">关闭</button>
      </div>}
      {!activeWorkspace ? <main className="flex flex-1 flex-col items-center justify-center gap-4 p-8">
        <h1 className="text-xl font-semibold">打开一个项目，开始使用 Peck</h1>
        <p className="max-w-lg text-center text-sm leading-relaxed text-[#8B949E]">打开项目后，可选择单 Worker 开发，或由 Supervisor 委派、审查与接管。在右侧查看工具证据和真实 Diff；模型与密钥在 API 工作台配置。</p>
        <button onClick={() => setApiOpen(true)} className="rounded-lg border border-[#00E5FF]/50 px-5 py-2.5 text-sm text-[#00E5FF]">配置 API 模型与角色</button>
        {!isMockMode() && <button onClick={() => void perform(chooseWorkspace)} className="rounded-lg bg-[#00E5FF] px-5 py-2.5 text-sm font-semibold text-[#0F1115]">选择项目文件夹</button>}
        <form onSubmit={(e) => { e.preventDefault(); void perform(() => selectWorkspace(workspacePath)); }} className="flex w-full max-w-xl gap-2">
          <input aria-label="工作区路径" value={workspacePath} onChange={(e) => setWorkspacePath(e.target.value)} placeholder="或输入项目完整路径，支持 ~/" className="min-w-0 flex-1 rounded-lg border border-[#262B34] bg-[#181B21] px-3 py-2 text-sm outline-none focus:border-[#00E5FF]" />
          <button disabled={!workspacePath.trim()} className="rounded-lg border border-[#262B34] px-4 text-sm disabled:opacity-40">打开</button>
        </form>
      </main> : <SplitPane defaultLeftWidth={420} left={
        <ChatPanel sessions={sessions} activeSession={activeSession} messages={messages} runs={runs}
          activeRunId={activeRunId} agents={agents} isRunning={busy} apiMode={apiMode}
          onSelectSession={(session) => { if (!busy) void perform(() => switchSession(session)); }}
          onNewSession={() => { if (!busy) void perform(() => createNewSession()); }}
          onSelectRun={setActiveRunId} onSendPrompt={(prompt, limits, scenario, role) => { void handleSendPrompt(prompt, limits, scenario, role); }}
          onCancelRun={() => { if (activeRunId) void perform(() => ipc.runCancel(activeRunId)); }}
          onRetryStep={(runId, stepId) => { void handleRetryStep(runId, stepId); }} />
      } right={<ArtifactBoard run={activeRun} onRetryStep={(stepId) => { if (activeRun) void handleRetryStep(activeRun.id, stepId); }} />} />}
      <AgentsDialog open={agentsDialogOpen} agents={agents} onClose={() => setAgentsDialogOpen(false)} onRefresh={refreshAgents} />
      <ApiWorkbench open={apiOpen} onClose={() => setApiOpen(false)} onMode={setApiMode} />
      <UsagePage open={usageOpen} onClose={() => setUsageOpen(false)} />
    </div>
  );
};
export default App;
