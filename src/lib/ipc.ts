import { Channel, invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import type { AgentEvent, GatewayConfig, GatewaySettings, ModelEntry } from "@/lib/agentEvents";
import type {
  Activity,
  AgentInfo,
  DiffResult,
  FileListing,
  FilePreview,
  Message,
  Permission,
  ProjectOverview,
  RunEvent,
  RunRecord,
  Session,
  TermEvent,
  TermState,
  UndoPreview,
  Workspace,
} from "@/lib/types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isMockMode(): boolean {
  if (typeof window === "undefined") return true;
  // A native window always uses Rust IPC, including native API mock mode.
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

  /** 发任务。`who` 是「谁来干」（muse、grok、muse>grok、grok>muse、mock）。 */
  async runDispatch(
    sessionId: string,
    prompt: string,
    who: string,
    permission: Permission,
    onEvent: (event: RunEvent) => void,
  ): Promise<string> {
    if (isMockMode()) {
      return mockIpc.runDispatch(sessionId, prompt, who, permission, onEvent);
    }
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("run_dispatch", { sessionId, prompt, who, permission, channel });
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

  async overview(): Promise<ProjectOverview[]> {
    if (isMockMode()) return mockIpc.overview();
    return invoke<ProjectOverview[]>("projects_overview");
  },
  async filesList(workspaceId: string): Promise<FileListing> {
    if (isMockMode()) return mockIpc.filesList(workspaceId);
    return invoke<FileListing>("files_list", { workspaceId });
  },
  async filesRead(workspaceId: string, path: string, runId?: string): Promise<FilePreview> {
    if (isMockMode()) return mockIpc.filesRead(workspaceId, path, runId);
    return invoke<FilePreview>("files_read", { workspaceId, path, runId });
  },
  async runUndoPreview(runId: string): Promise<UndoPreview> {
    if (isMockMode()) return mockIpc.runUndoPreview(runId);
    return invoke<UndoPreview>("run_undo_preview", { runId });
  },
  async runUndo(runId: string): Promise<RunRecord> {
    if (isMockMode()) return mockIpc.runUndo(runId);
    return invoke<RunRecord>("run_undo", { runId });
  },
  async settingsGet(): Promise<string | null> {
    if (isMockMode()) return mockIpc.settingsGet();
    return invoke<string | null>("settings_get");
  },
  async settingsSet(value: string): Promise<void> {
    if (isMockMode()) return mockIpc.settingsSet(value);
    return invoke<void>("settings_set", { value });
  },
  async termRun(workspaceId: string, command: string, onEvent: (event: TermEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.termRun(workspaceId, command, onEvent);
    const channel = new Channel<TermEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("term_run", { workspaceId, command, channel });
  },
  async termStop(workspaceId: string): Promise<void> {
    if (isMockMode()) return mockIpc.termStop(workspaceId);
    return invoke<void>("term_stop", { workspaceId });
  },
  async termState(workspaceId: string): Promise<TermState> {
    if (isMockMode()) return mockIpc.termState(workspaceId);
    return invoke<TermState>("term_state", { workspaceId });
  },
  /** 菜单栏小图标跟着变。浏览器里没有菜单栏，不做事。 */
  async setActivity(activity: Activity): Promise<void> {
    if (isMockMode()) return;
    return invoke<void>("shell_set_activity", { activity });
  },
  async notify(title: string, body: string): Promise<void> {
    if (isMockMode()) return mockIpc.notify(title, body);
    return invoke<void>("notify", { title, body });
  },

  // ── Model Gateway（v0.5 契约，src-tauri/src/gateway/）──
  /** 模型列表：API 模型（配了 Key 的能用）、CLI 引擎（检测到的能用）、接力。 */
  async gatewayModels(): Promise<ModelEntry[]> {
    if (isMockMode()) return mockIpc.gatewayModels();
    return invoke<ModelEntry[]>("gateway_models");
  },
  async gatewayConfigGet(): Promise<GatewaySettings> {
    if (isMockMode()) return mockIpc.gatewayConfigGet();
    return invoke<GatewaySettings>("gateway_config_get");
  },
  /** 存 Vertex 的 Project ID / Location、CLI 目录；格式不对会报错（中文一句）。 */
  async gatewayConfigSet(config: GatewayConfig): Promise<GatewayConfig> {
    if (isMockMode()) return mockIpc.gatewayConfigSet(config);
    return invoke<GatewayConfig>("gateway_config_set", { config });
  },
  /** 一个任务的统一事件（事件日志）。旧任务为空。 */
  async gatewayEvents(runId: string): Promise<AgentEvent[]> {
    if (isMockMode()) return mockIpc.gatewayEvents(runId);
    return invoke<AgentEvent[]>("gateway_events", { runId });
  },
};
