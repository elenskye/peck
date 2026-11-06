import React, { useEffect, useState } from "react";
import { ArtifactBoard } from "@/components/board/ArtifactBoard";
import { ChatPanel } from "@/components/chat/ChatPanel";
import { SplitPane } from "@/components/layout/SplitPane";
import { TitleBar } from "@/components/layout/TitleBar";
import { AgentsDialog } from "@/components/settings/AgentsDialog";
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
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [workspacePath, setWorkspacePath] = useState("");

  const refreshAgents = async () => { setAgents(await ipc.agentsList()); };
  const perform = async (action: () => Promise<unknown>) => {
    try { setError(""); await action(); } catch (e) { setError(String(e)); }
  };
  useEffect(() => { void perform(refreshAgents); }, []);

  const receiveEvent = (prompt: string, event: RunEvent) => {
    handleRunEvent(prompt, event);
    if (event.type === "RunFinished") {
      setBusy(false);
      void perform(async () => { await reloadMessages(); await reloadRuns(); });
    }
  };
  const handleSendPrompt = async (prompt: string) => {
    if (!activeSession || busy) return;
    setBusy(true);
    try {
      setError("");
      const runId = await ipc.runDispatch(activeSession.id, prompt, (event) => receiveEvent(prompt, event));
      setActiveRunId(runId);
      await reloadMessages();
    } catch (e) { setError(String(e)); setBusy(false); }
  };
  const handleRetryStep = async (runId: string, stepId: string) => {
    if (busy) return;
    const target = runs.find((r) => r.id === runId);
    if (!target) return;
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
        onOpenAgentsDialog={() => setAgentsDialogOpen(true)} />
      {visibleError && <div role="alert" className="flex shrink-0 items-center justify-between gap-4 border-b border-[#EF4444]/40 bg-[#EF4444]/10 px-4 py-2 text-xs text-[#FCA5A5]">
        <span>{visibleError}</span><button onClick={() => setError("")} className="shrink-0">关闭</button>
      </div>}
      {!activeWorkspace ? <main className="flex flex-1 flex-col items-center justify-center gap-4 p-8">
        <h1 className="text-xl font-semibold">打开一个项目，开始使用 Peck</h1>
        <p className="max-w-lg text-center text-sm leading-relaxed text-[#8B949E]">无需同时订阅 Muse 和 Grok。可以先用内置模拟 Agent 体验流程，或只使用一个已配置的 CLI。</p>
        {!isMockMode() && <button onClick={() => void perform(chooseWorkspace)} className="rounded-lg bg-[#00E5FF] px-5 py-2.5 text-sm font-semibold text-[#0F1115]">选择项目文件夹</button>}
        <form onSubmit={(e) => { e.preventDefault(); void perform(() => selectWorkspace(workspacePath)); }} className="flex w-full max-w-xl gap-2">
          <input aria-label="工作区路径" value={workspacePath} onChange={(e) => setWorkspacePath(e.target.value)} placeholder="或输入项目完整路径，支持 ~/" className="min-w-0 flex-1 rounded-lg border border-[#262B34] bg-[#181B21] px-3 py-2 text-sm outline-none focus:border-[#00E5FF]" />
          <button disabled={!workspacePath.trim()} className="rounded-lg border border-[#262B34] px-4 text-sm disabled:opacity-40">打开</button>
        </form>
      </main> : <SplitPane defaultLeftWidth={420} left={
        <ChatPanel sessions={sessions} activeSession={activeSession} messages={messages} runs={runs}
          activeRunId={activeRunId} agents={agents} isRunning={busy}
          onSelectSession={(session) => { if (!busy) void perform(() => switchSession(session)); }}
          onNewSession={() => { if (!busy) void perform(() => createNewSession()); }}
          onSelectRun={setActiveRunId} onSendPrompt={(prompt) => { void handleSendPrompt(prompt); }}
          onCancelRun={() => { if (activeRunId) void perform(() => ipc.runCancel(activeRunId)); }}
          onRetryStep={(runId, stepId) => { void handleRetryStep(runId, stepId); }} />
      } right={<ArtifactBoard run={activeRun} onRetryStep={(stepId) => { if (activeRun) void handleRetryStep(activeRun.id, stepId); }} />} />}
      <AgentsDialog open={agentsDialogOpen} agents={agents} onClose={() => setAgentsDialogOpen(false)} onRefresh={refreshAgents} />
    </div>
  );
};
export default App;
