import { Channel, invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import { isMockMode } from "./ipc";
import type { Permission, RunEvent } from "./types";

export type ApiRole = "worker" | "supervisor";
export type ApiMode = "mock" | "real";
export type Thinking = "unknown" | "never" | "switch" | "always";
export interface ApiProvider { id: string; name: string; baseUrl: string; protocol: "anthropic" | "chatCompletions" | "responses" | "gemini" | "qwen" }
export interface ModelProfile {
  id: string; name: string; providerId: string; apiModel: string;
  contextWindow: number; maxOutputTokens: number; thinking: Thinking; efforts: string[]; tools: boolean;
}
export interface AgentProfile { role: "supervisor" | "worker"; modelId: string; thinking: boolean; effort: string | null; maxOutputTokens: number }
export interface ApiConfig { models: ModelProfile[]; agents: AgentProfile[]; aliyunRegion: "beijing" | "singapore"; aliyunWorkspaceId: string }
export interface Allowance { providerId: string; apiModel: string; limit: number; used: number }
export interface Usage { inputTokens: number | null; outputTokens: number | null; cacheReadTokens: number | null; cacheCreationTokens: number | null; reasoningTokens: number | null }
export interface ApiCall {
  id: string; mode: ApiMode; role: string; model: ModelProfile; settings: AgentProfile; prompt: string;
  taskId: string | null; runId: string | null; round: number | null;
  startedAt: number; finishedAt: number | null; status: string; budgetCounted: boolean;
  text: string; reasoning: string; error: string | null;
  response: { text: string; reasoning: string; message: unknown; usage: Usage; stopReason: string | null; responseModel: string | null } | null;
}
export interface ApiStatus {
  mode: ApiMode; providers: ApiProvider[]; config: ApiConfig;
  keys: { providerId: string; state: "unloaded" | "missing" | "saved" | "unavailable" }[];
  allowances: Allowance[]; calls: ApiCall[];
}
export type ApiEvent = { type: "Text" | "Reasoning"; id: string; text: string } | { type: "Finished"; call: ApiCall } | { type: "PersistenceError"; message: string };

export interface RunLimits { maxRounds: number; maxToolCalls: number; maxContextBytes: number; maxTotalTokens: number; maxDelegations: number; maxReworks: number; workerMaxRounds: number }
export const defaultRunLimits: RunLimits = { maxRounds: 12, maxToolCalls: 80, maxContextBytes: 180000, maxTotalTokens: 500000, maxDelegations: 4, maxReworks: 1, workerMaxRounds: 6 };
export interface ApiTask {
  id: string; runId: string; parentId: string | null; previousTaskId: string | null; stepId: string; title: string; paths: string[]; acceptance: string; attempt: number;
  review: { decision: "accept" | "rework" | "takeover"; feedback: string; reviewedAt: number } | null;
  changeset: { files: import("./types").FileChangeItem[] };
  mode: ApiMode; model: ModelProfile; settings: AgentProfile;
  status: string; round: number; limits: RunLimits; callIds: string[]; summary: string | null;
  inputTokens: number | null; outputTokens: number | null;
  tools: { callId: string; tool: { id: string; name: string; arguments: unknown }; status: string; output: string; startedAt: number; finishedAt: number | null }[];
}

export const api = {
  runStart: (sessionId: string, prompt: string, limits: RunLimits, role: ApiRole, scenario: string, permission: Permission, onEvent: (event: RunEvent) => void) => {
    if (isMockMode()) return mockIpc.apiRunStart(sessionId, prompt, role, permission, onEvent);
    const channel = new Channel<RunEvent>(); channel.onmessage = onEvent;
    return invoke<string>("api_run_start", { sessionId, prompt, limits, role, scenario, permission, channel });
  },
  taskHistory: (runId: string) => isMockMode() ? mockIpc.apiTaskHistory(runId) : invoke<ApiTask[]>("api_task_history", { runId }),
  exportCalls: () => isMockMode() ? Promise.resolve(null) : invoke<string | null>("api_export_calls"),
  status: () => isMockMode() ? mockIpc.apiStatus() : invoke<ApiStatus>("api_status"),
  saveConfig: (config: ApiConfig) => isMockMode() ? mockIpc.apiSaveConfig(config) : invoke<void>("api_save_config", { config }),
  setMode: (mode: ApiMode) => isMockMode() ? Promise.resolve() : invoke<void>("api_set_mode", { mode }),
  loadKeys: () => isMockMode() ? Promise.resolve() : invoke<void>("api_key_load"),
  setKey: (providerId: string, key: string | null) => isMockMode() ? mockIpc.apiSetKey(providerId, key) : invoke<void>("api_key_set", { providerId, key }),
  start: (id: string, role: string, prompt: string, scenario: string, onEvent: (event: ApiEvent) => void) => {
    if (isMockMode()) return Promise.reject(new Error("浏览器演示里不发单次请求，到 Peck.app 里试。"));
    const channel = new Channel<ApiEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("api_request_start", { id, role, prompt, scenario, channel });
  },
  cancel: (id: string) => isMockMode() ? Promise.resolve() : invoke<void>("api_request_cancel", { id }),
};
