import { invoke } from "@tauri-apps/api/core";
import { isMockMode } from "./ipc";

export interface QuotaWindow { label: string; usedPercent: number | null; remainingAmount: number | null; resetAtMs: number | null; windowDurationMins: number | null }
export interface QuotaSnapshot { accountId: string | null; windows: QuotaWindow[]; source: string; capturedAt: number | null; manual: boolean }
export interface ClientCard { client: string; label: string; status: string; source: string; coverage: string; accountId: string | null; accountLabel?: string | null; tier: string | null; windows: QuotaWindow[]; rawLabel: string; lastSuccessAt: number | null; lastFailureAt: number | null; failureReason: string | null; collectedAt: number; fresh: boolean; running: boolean; installed: boolean; manual: boolean; previous: QuotaSnapshot | null }
export interface BalanceCard { provider: string; label: string; status: string; balances: { currency: string; totalBalance: string; grantedBalance: string | null; toppedUpBalance: string | null }[]; checkedAt: number | null; message: string; lastSuccessAt: number | null; lastFailureAt: number | null }

const now = Date.now();
export const mockClients: ClientCard[] = ["codex", "claude-code", "antigravity", "muse-code", "grok"].map((client) => ({ client, label: { codex: "Codex", "claude-code": "Claude Code", antigravity: "Antigravity", "muse-code": "Muse", grok: "Grok Build" }[client]!, status: "fields_missing", source: "浏览器演示，没有读真实的客户端", coverage: "演示数据", accountId: null, tier: null, windows: [], rawLabel: "", lastSuccessAt: null, lastFailureAt: now, failureReason: "浏览器演示里不读客户端，到 Peck.app 里看。", collectedAt: now, fresh: false, running: false, installed: false, manual: false, previous: null }));

// 演示额度提醒（v0.9 B3）：Claude Code 每周用到 90%，Codex 还多（验收点：某家调到 90%，发任务前提醒换另一家）。
const demoWindows: Record<string, QuotaWindow[]> = {
  "claude-code": [
    { label: "5 小时", usedPercent: 26, remainingAmount: null, resetAtMs: now + 3 * 3_600_000, windowDurationMins: 300 },
    { label: "每周", usedPercent: 90, remainingAmount: null, resetAtMs: now + 2 * 86_400_000, windowDurationMins: 10_080 },
  ],
  codex: [
    { label: "5 小时", usedPercent: 12, remainingAmount: null, resetAtMs: now + 4 * 3_600_000, windowDurationMins: 300 },
    { label: "每周", usedPercent: 31, remainingAmount: null, resetAtMs: now + 5 * 86_400_000, windowDurationMins: 10_080 },
  ],
};
for (const c of mockClients) {
  const w = demoWindows[c.client];
  if (!w) continue;
  Object.assign(c, { windows: w, status: "ok", failureReason: null, lastFailureAt: null, lastSuccessAt: now, fresh: true, installed: true, source: "浏览器演示数据" });
}

export const usageApi = {
  clients: (): Promise<ClientCard[]> => isMockMode() ? Promise.resolve(mockClients) : invoke("usage_clients"),
  refresh: (): Promise<ClientCard[]> => isMockMode() ? Promise.resolve(mockClients) : invoke("usage_refresh"),
  manual: (input: { client: string; usedPercent: number | null; resetAtMs: number | null; note: string }) => isMockMode() ? Promise.reject(new Error("浏览器演示里存不了手工记的快照")) : invoke<ClientCard>("usage_manual_snapshot", { input }),
  balance: (provider: string): Promise<BalanceCard> => isMockMode() ? Promise.resolve({ provider, label: provider, status: "unknown", balances: [], checkedAt: now, lastSuccessAt: null, lastFailureAt: null, message: "浏览器演示里不查余额。" }) : invoke("usage_balance", { provider }),
  balances: (): Promise<BalanceCard[]> => isMockMode() ? Promise.resolve([]) : invoke("usage_balances"),
};
