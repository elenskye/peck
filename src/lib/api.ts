import { invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import { isMockMode } from "./ipc";

export type ApiMode = "mock" | "real";
export type Thinking = "unknown" | "never" | "switch" | "always";
export interface ApiProvider { id: string; name: string; baseUrl: string; protocol: "anthropic" | "chatCompletions" | "responses" | "gemini" | "qwen" }
export interface ModelProfile {
  id: string; name: string; providerId: string; apiModel: string;
  contextWindow: number; maxOutputTokens: number; thinking: Thinking; efforts: string[]; tools: boolean;
}
export interface AgentProfile { role: "supervisor" | "worker"; modelId: string; thinking: boolean; effort: string | null; maxOutputTokens: number }
export interface ApiConfig { models: ModelProfile[]; agents: AgentProfile[]; aliyunRegion: "beijing" | "singapore"; aliyunWorkspaceId: string }
export interface ApiStatus {
  mode: ApiMode; providers: ApiProvider[]; config: ApiConfig;
  keys: { providerId: string; state: "unloaded" | "missing" | "saved" | "unavailable" }[];
}

/** 旧的规划 + 实施任务的调用和审查历史（v0.8 起只读）。 */
export interface RunLimits { maxRounds: number; maxToolCalls: number; maxContextBytes: number; maxTotalTokens: number; maxDelegations: number; maxReworks: number; workerMaxRounds: number }
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
  taskHistory: (runId: string) => isMockMode() ? mockIpc.apiTaskHistory(runId) : invoke<ApiTask[]>("api_task_history", { runId }),
  status: () => isMockMode() ? mockIpc.apiStatus() : invoke<ApiStatus>("api_status"),
  saveConfig: (config: ApiConfig) => isMockMode() ? mockIpc.apiSaveConfig(config) : invoke<void>("api_save_config", { config }),
  setMode: (mode: ApiMode) => isMockMode() ? Promise.resolve() : invoke<void>("api_set_mode", { mode }),
  loadKeys: () => isMockMode() ? Promise.resolve() : invoke<void>("api_key_load"),
  setKey: (providerId: string, key: string | null) => isMockMode() ? mockIpc.apiSetKey(providerId, key) : invoke<void>("api_key_set", { providerId, key }),
};
