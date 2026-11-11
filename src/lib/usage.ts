import { invoke } from "@tauri-apps/api/core";
import { isMockMode } from "./ipc";

export interface UsageTokens { inputTokens: number | null; outputTokens: number | null; cacheReadTokens: number | null; cacheCreationTokens: number | null; reasoningTokens: number | null }
export interface UsageCall { source: string; requestId: string; project: string; runId: string | null; taskId: string | null; round: number | null; mode: string; provider: string; model: string; role: string; startedAt: number; finishedAt: number | null; status: string; budgetCounted: boolean; usage: UsageTokens | null; currency: string | null; actualCost: number | null; estimatedCost: number | null; priceVersion: string | null; localId: boolean }
export interface UsageFilter { mode?: string; project?: string; model?: string; provider?: string }
export interface CostSummary { currency: string | null; actual: number | null; estimated: number | null; actualUnknownCount: number; estimatedUnknownCount: number; priceVersions: string[] }
export interface UsageSummary { requestCount: number; tokens: { input: number | null; output: number | null; cacheRead: number | null; cacheCreation: number | null; reasoning: number | null; unknownCount: number }; costs: CostSummary[] }
export interface UsageListResult { calls: UsageCall[]; summary: UsageSummary; projects: string[]; models: string[]; providers: string[] }
export interface QuotaWindow { label: string; usedPercent: number | null; remainingAmount: number | null; resetAtMs: number | null; windowDurationMins: number | null }
export interface QuotaSnapshot { accountId: string | null; windows: QuotaWindow[]; source: string; capturedAt: number | null; manual: boolean }
export interface ClientCard { client: string; label: string; status: string; source: string; coverage: string; accountId: string | null; accountLabel?: string | null; tier: string | null; windows: QuotaWindow[]; rawLabel: string; lastSuccessAt: number | null; lastFailureAt: number | null; failureReason: string | null; collectedAt: number; fresh: boolean; running: boolean; installed: boolean; manual: boolean; previous: QuotaSnapshot | null }
export interface BalanceCard { provider: string; label: string; status: string; balances: { currency: string; totalBalance: string; grantedBalance: string | null; toppedUpBalance: string | null }[]; checkedAt: number | null; message: string; lastSuccessAt: number | null; lastFailureAt: number | null }

const now = Date.now();
export const mockUsageList: UsageListResult = { calls: [], summary: { requestCount: 0, tokens: { input: null, output: null, cacheRead: null, cacheCreation: null, reasoning: null, unknownCount: 0 }, costs: [] }, projects: [], models: [], providers: [] };
export const mockClients: ClientCard[] = ["codex", "claude-code", "antigravity", "muse-code", "grok"].map((client) => ({ client, label: { codex: "Codex", "claude-code": "Claude Code", antigravity: "Antigravity", "muse-code": "Muse", grok: "Grok Build" }[client]!, status: "fields_missing", source: "浏览器演示，没有读真实的客户端", coverage: "演示数据", accountId: null, tier: null, windows: [], rawLabel: "", lastSuccessAt: null, lastFailureAt: now, failureReason: "浏览器演示里不读客户端，到 Peck.app 里看。", collectedAt: now, fresh: false, running: false, installed: false, manual: false, previous: null }));

export const usageApi = {
  list: (filter: UsageFilter): Promise<UsageListResult> => isMockMode() ? Promise.resolve(mockUsageList) : invoke("usage_list", { filter }),
  importJson: (json: string) => isMockMode() ? Promise.resolve({ imported: 0, skipped: 0, invalid: 0 }) : invoke<{ imported: number; skipped: number; invalid: number }>("usage_import", { json }),
  exportCsv: (filter: UsageFilter) => isMockMode() ? Promise.resolve(null) : invoke<string | null>("usage_export_csv", { filter }),
  clients: (): Promise<ClientCard[]> => isMockMode() ? Promise.resolve(mockClients) : invoke("usage_clients"),
  refresh: (): Promise<ClientCard[]> => isMockMode() ? Promise.resolve(mockClients) : invoke("usage_refresh"),
  manual: (input: { client: string; usedPercent: number | null; resetAtMs: number | null; note: string }) => isMockMode() ? Promise.reject(new Error("浏览器演示里存不了手工记的快照")) : invoke<ClientCard>("usage_manual_snapshot", { input }),
  balance: (provider: string): Promise<BalanceCard> => isMockMode() ? Promise.resolve({ provider, label: provider, status: "unknown", balances: [], checkedAt: now, lastSuccessAt: null, lastFailureAt: null, message: "浏览器演示里不查余额。" }) : invoke("usage_balance", { provider }),
  balances: (): Promise<BalanceCard[]> => isMockMode() ? Promise.resolve([]) : invoke("usage_balances"),
};
