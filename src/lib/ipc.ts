import { Channel, invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import type {
  AgentInfo,
  DiffResult,
  Message,
  RunEvent,
  RunRecord,
  Session,
  Workspace,
} from "@/lib/types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isMockMode(): boolean {
  if (typeof window === "undefined") return true;
  const params = new URLSearchParams(window.location.search);
  if (params.has("mock")) return true;
  // Automatically fall back to mock mode when opened in a standalone browser outside Tauri
  return typeof window.__TAURI_INTERNALS__ === "undefined";
}

export const ipc = {
  async workspaceChoose(): Promise<Workspace | null> {
    if (isMockMode()) return null;
    return invoke<Workspace | null>("workspace_choose");
  },
  async workspaceOpen(path: string): Promise<Workspace> {
    if (isMockMode()) {
      return mockIpc.workspaceOpen(path);
    }
    return invoke<Workspace>("workspace_open", { path });
  },

  async workspaceListRecent(): Promise<Workspace[]> {
    if (isMockMode()) {
      return mockIpc.workspaceListRecent();
    }
    return invoke<Workspace[]>("workspace_list_recent");
  },

  async sessionCreate(workspaceId: string, title?: string): Promise<Session> {
    if (isMockMode()) {
      return mockIpc.sessionCreate(workspaceId, title);
    }
    return invoke<Session>("session_create", { workspaceId, title });
  },

  async sessionList(workspaceId: string): Promise<Session[]> {
    if (isMockMode()) {
      return mockIpc.sessionList(workspaceId);
    }
    return invoke<Session[]>("session_list", { workspaceId });
  },

  async sessionGetMessages(sessionId: string): Promise<Message[]> {
    if (isMockMode()) {
      return mockIpc.sessionGetMessages(sessionId);
    }
    return invoke<Message[]>("session_get_messages", { sessionId });
  },

  async sessionGetRuns(sessionId: string): Promise<RunRecord[]> {
    if (isMockMode()) {
      return mockIpc.sessionGetRuns(sessionId);
    }
    return invoke<RunRecord[]>("session_get_runs", { sessionId });
  },

  async runDispatch(
    sessionId: string,
    prompt: string,
    onEvent: (event: RunEvent) => void,
  ): Promise<string> {
    if (isMockMode()) {
      return mockIpc.runDispatch(sessionId, prompt, onEvent);
    }
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("run_dispatch", {
      sessionId,
      prompt,
      channel,
    });
  },

  async runCancel(runId: string): Promise<void> {
    if (isMockMode()) {
      return mockIpc.runCancel(runId);
    }
    return invoke<void>("run_cancel", { runId });
  },

  async runRetryStep(
    runId: string,
    stepId: string,
    onEvent: (event: RunEvent) => void,
  ): Promise<void> {
    if (isMockMode()) {
      return mockIpc.runRetryStep(runId, stepId, onEvent);
    }
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("run_retry_step", {
      runId,
      stepId,
      channel,
    });
  },

  async changesGetDiff(
    runId: string,
    stepId?: string,
    filePath?: string,
  ): Promise<DiffResult> {
    if (isMockMode()) {
      return mockIpc.changesGetDiff(runId, stepId, filePath);
    }
    return invoke<DiffResult>("changes_get_diff", {
      runId,
      stepId,
      filePath,
    });
  },

  async agentsList(): Promise<AgentInfo[]> {
    if (isMockMode()) {
      return mockIpc.agentsList();
    }
    return invoke<AgentInfo[]>("agents_list");
  },
  async agentGetConfig(agentId: string): Promise<string> {
    if (isMockMode()) return mockIpc.agentGetConfig(agentId);
    return invoke<string>("agent_get_config", { agentId });
  },
  async agentSaveConfig(agentId: string, content: string): Promise<void> {
    if (isMockMode()) return mockIpc.agentSaveConfig(agentId, content);
    return invoke<void>("agent_save_config", { agentId, content });
  },
};
