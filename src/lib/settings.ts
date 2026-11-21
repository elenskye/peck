// 界面设置：一整段 JSON 存在数据库里（`app_settings` 的 `ui`），浏览器 `?mock` 里只在内存。
import { ipc } from "@/lib/ipc";
import type { ThemeChoice } from "@/lib/theme";
import type { Permission } from "@/lib/types";
import { normalizePresets, type Preset } from "@/lib/presets";

export interface AppSettings {
  theme: ThemeChoice;
  /** 「谁来干」：模型列表里的 id（api:…、cli:…），或者 `relay`（接力，两家见 `relay`）。空着用列表里第一个能用的。 */
  pick: string;
  /** 每种干法（FLOW：api、cli、relay）上次选的哪个；换 FLOW 时回到它。 */
  picks: Partial<Record<string, string>>;
  /** 接力（v0.8）：谁写规范、谁实施（模型列表里的 id，API 模型或 CLI，Antigravity 除外）。 */
  relay: { spec?: string; build?: string };
  /** 交叉审查（v0.8）：做完以后谁来看这次的改动（空着不审查）。 */
  review: string;
  /** 委派（v0.8）：子任务交给哪个 API 模型（空着不委派）。v0.9 起实施的是 CLI（Antigravity 除外）也能委派。 */
  delegate: string;
  /** 合作预设（v0.9 B2）：存着的几套组合（自带两套在前）。 */
  presets: Preset[];
  /** 上次套用的预设（空着是没用预设）；AGENTS 改了以后它还在，界面标「改过」。 */
  preset: string;
  /** 额度提醒（v0.9 B3）：要用到的 CLI 订阅窗口用到这么多（百分比）就提醒换一家；null 是关了。 */
  quotaWarn: number | null;
  /** 每家 CLI 用哪个模型（引擎 id → CLI 里的模型名；空着用 CLI 自己的默认）。 */
  cliModels: Record<string, string>;
  /** 选的那个模型叫什么（显示用，比如「Opus 5.5」）。 */
  cliModelNames: Record<string, string>;
  /** 左栏收起的模块（PROJECTS、AGENTS）。 */
  fold: { projects?: boolean; agents?: boolean };
  /** 每个模型上次选的思考强度（`off` 是不思考；CLI 的 `default` 是不指定、用 CLI 自己的）。 */
  efforts: Record<string, string>;
  /** ONE SHOT 上次选的谁来答。 */
  oneshotPick: string;
  /** 发任务时默认的权限档。 */
  permission: Permission;
  /** 任务做完、失败、要你批或答时发 macOS 通知（Peck 不在最前面时）。 */
  notifyDone: boolean;
  /** 每月花费上限（美元）：Peck 循环（API）这个月花到这么多，下一次请求前停下。空着不管。 */
  costCap: number | null;
  /** Peck 循环一个任务最多几轮（v0.10，5–400，默认 40；后台发任务时读这里存的）。 */
  maxRounds: number;
  /** Dock 里的 App 图标。 */
  appIcon: "j2" | "a3";
  /** 用哪个编辑器打开文件（v0.8，App 的名字，比如 Visual Studio Code；空着用系统默认的 App）。 */
  editorApp: string;
  /** 左栏「待你处理」（v0.7）：每个任务上次看过的时间；没看过的、比 `seenSince` 晚做完的算没看。 */
  seen: Record<string, number>;
  seenSince: number;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "system",
  pick: "",
  picks: {},
  relay: {},
  review: "",
  delegate: "",
  presets: normalizePresets(undefined),
  preset: "",
  quotaWarn: 85,
  cliModels: {},
  cliModelNames: {},
  fold: {},
  efforts: {},
  oneshotPick: "",
  permission: "edit",
  notifyDone: true,
  costCap: 20,
  maxRounds: 40,
  appIcon: "j2",
  editorApp: "",
  seen: {},
  seenSince: 0,
};

/** v0.4 存的「模式 + 谁来干」换成模型列表里的 id。 */
function legacyPick(saved: { mode?: string; who?: { cli?: string; api?: string } }): string {
  if (saved.mode === "api") return "";
  const cli = saved.who?.cli;
  if (cli === "muse>grok" || cli === "grok>muse") return `flow:${cli}`;
  if (cli === "muse" || cli === "grok") return `cli:${cli}`;
  return "";
}

/** v0.8 以前存的「谁来干」：旧的接力换成新的接力（同样两家），规划 + 实施、假 Agent 没了，回到默认。 */
export function migratePick(pick: string, relay: AppSettings["relay"]): { pick: string; relay: AppSettings["relay"] } {
  const old = pick.match(/^flow:(muse|grok)>(muse|grok)$/);
  if (old) return { pick: "relay", relay: { spec: `cli:${old[1]}`, build: `cli:${old[2]}` } };
  if (pick.startsWith("flow:") || pick === "mock") return { pick: "", relay };
  return { pick, relay };
}

export async function loadSettings(): Promise<AppSettings> {
  try {
    const raw = await ipc.settingsGet();
    if (!raw) {
      const fresh = { ...DEFAULT_SETTINGS, seenSince: Date.now() };
      void saveSettings(fresh).catch(() => {});
      return fresh;
    }
    const saved = JSON.parse(raw) as Partial<AppSettings> & { mode?: string; who?: { cli?: string; api?: string } };
    const moved = migratePick(saved.pick ?? legacyPick(saved), { ...saved.relay });
    const settings: AppSettings = {
      ...DEFAULT_SETTINGS,
      ...saved,
      pick: moved.pick,
      relay: moved.relay,
      review: saved.review ?? "",
      delegate: saved.delegate ?? "",
      presets: normalizePresets(saved.presets),
      preset: saved.preset ?? "",
      quotaWarn: saved.quotaWarn === null ? null : typeof saved.quotaWarn === "number" ? Math.min(100, Math.max(50, saved.quotaWarn)) : 85,
      picks: Object.fromEntries(Object.entries({ ...saved.picks }).filter(([k]) => k === "api" || k === "cli" || k === "relay")),
      cliModels: { ...saved.cliModels },
      cliModelNames: { ...saved.cliModelNames },
      fold: { ...saved.fold },
      efforts: { ...saved.efforts },
      appIcon: saved.appIcon === "a3" ? "a3" : "j2",
      seen: { ...saved.seen },
      // 头一回用这一版：以前做完的都当看过了。
      seenSince: saved.seenSince ?? Date.now(),
    };
    // 这条线马上记下来，不然每次打开都往后挪，两次打开之间做完的就算看过了。
    if (saved.seenSince === undefined) void saveSettings(settings).catch(() => {});
    return settings;
  } catch {
    return { ...DEFAULT_SETTINGS, seenSince: Date.now() };
  }
}

export function saveSettings(settings: AppSettings): Promise<void> {
  return ipc.settingsSet(JSON.stringify(settings));
}
