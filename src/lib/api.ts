import { invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import { isMockMode } from "./ipc";

export type ApiMode = "mock" | "real";
/** 设置 → PROVIDERS 那几家（存 Key 用）。 */
export interface ApiProvider { id: string; name: string }
/** 存着的设置：只剩 Aliyun 的地域（v1.0.3 起旧 API 层的模型、角色配置不要了）。 */
export interface ApiConfig { aliyunRegion: "beijing" | "singapore" }
export interface ApiStatus {
  mode: ApiMode; providers: ApiProvider[]; config: ApiConfig;
  keys: { providerId: string; state: "unloaded" | "missing" | "saved" | "unavailable" }[];
}

export const api = {
  status: () => isMockMode() ? mockIpc.apiStatus() : invoke<ApiStatus>("api_status"),
  saveConfig: (config: ApiConfig) => isMockMode() ? mockIpc.apiSaveConfig(config) : invoke<void>("api_save_config", { config }),
  setMode: (mode: ApiMode) => isMockMode() ? Promise.resolve() : invoke<void>("api_set_mode", { mode }),
  loadKeys: () => isMockMode() ? Promise.resolve() : invoke<void>("api_key_load"),
  setKey: (providerId: string, key: string | null) => isMockMode() ? mockIpc.apiSetKey(providerId, key) : invoke<void>("api_key_set", { providerId, key }),
};
