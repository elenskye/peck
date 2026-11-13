// 界面设置：一整段 JSON 存在数据库里（`app_settings` 的 `ui`），浏览器 `?mock` 里只在内存。
import { defaultRunLimits, type RunLimits } from "@/lib/api";
import { ipc } from "@/lib/ipc";
import type { ThemeChoice } from "@/lib/theme";
import type { Permission } from "@/lib/types";

export interface AppSettings {
  theme: ThemeChoice;
  /** 「谁来干」：模型列表里的 id（api:…、cli:…、flow:…；开发者选项开着时还有 mock）。空着用列表里第一个能用的。 */
  pick: string;
  /** 每种干法（FLOW：api、cli、relay、plan、fake）上次选的哪个；换 FLOW 时回到它。 */
  picks: Partial<Record<string, string>>;
  /** 每个模型上次选的思考强度（`off` 是不思考）。 */
  efforts: Record<string, string>;
  /** ONE SHOT 上次选的谁来答。 */
  oneshotPick: string;
  /** 发任务时默认的权限档。 */
  permission: Permission;
  /** 任务做完、失败、要你批或答时发 macOS 通知（Peck 不在最前面时）。 */
  notifyDone: boolean;
  /** 开发者选项：「谁来干」里多一个假 Agent。 */
  fakeAgent: boolean;
  /** 每月花费上限（美元）：Peck 循环（API）这个月花到这么多，下一次请求前停下。空着不管。 */
  costCap: number | null;
  /** 规划 + 实施（旧的 API 模式）几个上限，照存。 */
  limits: RunLimits;
  /** Dock 里的 App 图标。 */
  appIcon: "j2" | "a3";
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "system",
  pick: "",
  picks: {},
  efforts: {},
  oneshotPick: "",
  permission: "edit",
  notifyDone: true,
  fakeAgent: false,
  costCap: 20,
  limits: defaultRunLimits,
  appIcon: "j2",
};

/** v0.4 存的「模式 + 谁来干」换成模型列表里的 id。 */
function legacyPick(saved: { mode?: string; who?: { cli?: string; api?: string } }): string {
  if (saved.mode === "api") return saved.who?.api === "api-supervisor" ? "flow:plan-implement" : "";
  const cli = saved.who?.cli;
  if (cli === "muse>grok" || cli === "grok>muse") return `flow:${cli}`;
  if (cli === "muse" || cli === "grok") return `cli:${cli}`;
  if (cli === "mock") return "mock";
  return "";
}

export async function loadSettings(): Promise<AppSettings> {
  try {
    const raw = await ipc.settingsGet();
    if (!raw) return DEFAULT_SETTINGS;
    const saved = JSON.parse(raw) as Partial<AppSettings> & { mode?: string; who?: { cli?: string; api?: string } };
    return {
      ...DEFAULT_SETTINGS,
      ...saved,
      pick: saved.pick ?? legacyPick(saved),
      picks: { ...saved.picks },
      efforts: { ...saved.efforts },
      limits: { ...DEFAULT_SETTINGS.limits, ...saved.limits },
      appIcon: saved.appIcon === "a3" ? "a3" : "j2",
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(settings: AppSettings): Promise<void> {
  return ipc.settingsSet(JSON.stringify(settings));
}
