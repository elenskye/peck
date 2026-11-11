// 界面设置：一整段 JSON 存在数据库里（`app_settings` 的 `ui`），浏览器 `?mock` 里只在内存。
import { defaultRunLimits, type RunLimits } from "@/lib/api";
import { ipc } from "@/lib/ipc";
import type { ThemeChoice } from "@/lib/theme";
import type { Permission } from "@/lib/types";

export interface AppSettings {
  theme: ThemeChoice;
  /** 顶栏的模式：决定「谁来干」列哪一组。 */
  mode: "cli" | "api";
  /** 每种模式上次选的「谁来干」。 */
  who: { cli: string; api: string };
  /** 发任务时默认的权限档。 */
  permission: Permission;
  /** 任务做完、失败时发 macOS 通知。 */
  notifyDone: boolean;
  /** 开发者选项：「谁来干」里多一个假 Agent。 */
  fakeAgent: boolean;
  /** 每月花费上限（美元）；v0.6 起按价格表估算拦截，现在先存着。 */
  costCap: number | null;
  /** API 模式其他几个上限，照存。 */
  limits: RunLimits;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "system",
  mode: "cli",
  who: { cli: "muse>grok", api: "api-worker" },
  permission: "edit",
  notifyDone: true,
  fakeAgent: false,
  costCap: 20,
  limits: defaultRunLimits,
};

export async function loadSettings(): Promise<AppSettings> {
  try {
    const raw = await ipc.settingsGet();
    if (!raw) return DEFAULT_SETTINGS;
    const saved = JSON.parse(raw) as Partial<AppSettings>;
    return {
      ...DEFAULT_SETTINGS,
      ...saved,
      who: { ...DEFAULT_SETTINGS.who, ...saved.who },
      limits: { ...DEFAULT_SETTINGS.limits, ...saved.limits },
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(settings: AppSettings): Promise<void> {
  return ipc.settingsSet(JSON.stringify(settings));
}
