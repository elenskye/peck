// 界面设置：一整段 JSON 存在数据库里（`app_settings` 的 `ui`），浏览器 `?mock` 里只在内存。
import { ipc } from "@/lib/ipc";
import type { ThemeChoice } from "@/lib/theme";
import type { Permission } from "@/lib/types";

export type SubagentKind = "explorer" | "searcher" | "worker" | "member" | "auditor";

/** 五类子代理（v1.1.2 起，member 是 v1.1.3）：设置 → AGENTS 一类一块，顺序照这里。默认的轮数、同时几个和后端一样。 */
export const SUBAGENT_KINDS: Array<{ id: SubagentKind; does: string; parallel: number | null; rounds: number }> = [
  { id: "explorer", does: "读、搜项目（只读）", parallel: 3, rounds: 24 },
  { id: "searcher", does: "联网检索、深度调研", parallel: 3, rounds: 24 },
  { id: "worker", does: "简单的改动，只改划给它的文件", parallel: 3, rounds: 24 },
  { id: "member", does: "复杂的活，权限和主角色一样，能再派 explorer、searcher", parallel: 2, rounds: 40 },
  { id: "auditor", does: "审这个任务的改动，一次一个", parallel: null, rounds: 24 },
];

/** 一类子代理（v1.1.2）：关没关、用哪个模型（空着跟主角色）、最多几轮、同时几个（auditor 没有）。 */
export interface SubagentSettings {
  off?: boolean;
  model?: string;
  rounds?: number;
  parallel?: number;
}

export interface AppSettings {
  theme: ThemeChoice;
  /** 「谁来干」：`ploop`（PLOOP，主角色见 `ploop.main`）或者一个 CLI（cli:…）。v1.1.2 起没有 API 单干。 */
  pick: string;
  /** 每种干法（FLOW：cli、ploop）上次选的哪个；换 FLOW 时回到它。 */
  picks: Partial<Record<string, string>>;
  /** PLOOP 的主角色（v1.1.2）：一个 API 模型（api:…）。 */
  ploop: { main?: string };
  /** PLOOP 的四类子代理（v1.1.2，设置 → AGENTS）：模型空着跟主角色；后端发任务时从这里读。 */
  subagents: Partial<Record<SubagentKind, SubagentSettings>>;
  /** 额度提醒（v0.9 B3）：要用到的 CLI 订阅窗口用到这么多（百分比）就提醒换一家；null 是关了。 */
  quotaWarn: number | null;
  /** 每家 CLI 用哪个模型（引擎 id → CLI 里的模型名；空着用 CLI 自己的默认）。 */
  cliModels: Record<string, string>;
  /** 选的那个模型叫什么（显示用，比如「Opus 5.5」）。 */
  cliModelNames: Record<string, string>;
  /** 左栏收起的模块（PROJECTS、AGENTS）；`left`、`right` 是顶栏上收起的整栏（v1.1.12）。 */
  fold: { projects?: boolean; agents?: boolean; left?: boolean; right?: boolean };
  /** 每个模型上次选的思考强度（`off` 是不思考；CLI 的 `default` 是不指定、用 CLI 自己的）。 */
  efforts: Record<string, string>;
  /** ONE SHOT 上次选的谁来答。 */
  oneshotPick: string;
  /** 发任务时默认的权限档。 */
  permission: Permission;
  /** 任务做完、失败、要你批或答时发 macOS 通知（Peck 不在最前面时）。 */
  notifyDone: boolean;
  /** 每月花费上限（美元）：Peck Loop（API）这个月花到这么多，下一次请求前停下。空着不管。 */
  costCap: number | null;
  /** Peck Loop 一个任务最多几轮（v0.10，5–400，默认 40；后台发任务时读这里存的）。 */
  maxRounds: number;
  /** ONE SHOT 最多几轮（v1.0.3，5–400，默认 12）。 */
  oneshotRounds: number;
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
  ploop: {},
  subagents: {},
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
  oneshotRounds: 12,
  appIcon: "j2",
  editorApp: "",
  seen: {},
  seenSince: 0,
};

export async function loadSettings(): Promise<AppSettings> {
  try {
    const raw = await ipc.settingsGet();
    if (!raw) {
      const fresh = { ...DEFAULT_SETTINGS, seenSince: Date.now() };
      void saveSettings(fresh).catch(() => {});
      return fresh;
    }
    // 旧版本存的 `mode`、`who`（v0.8 以前的「谁来干」）、`presets`、`preset`（v1.1.1 去掉的预设）、`subRounds` 不要了：读的时候丢掉。
    // 接力（`relay`）、API 单干（v1.1.2 去掉）换成 PLOOP：现在选着单干的就是那个模型当主角色，不然是接力 / 旧 PLOOP 实施的那一个；
    // 委派、审查的模型挪给 worker、auditor。
    type Old = { mode?: unknown; who?: unknown; presets?: unknown; preset?: unknown; subRounds?: unknown; relay?: { build?: string }; review?: string; delegate?: string; ploop?: { main?: string; build?: string } };
    const { mode: _mode, who: _who, presets: _presets, preset: _preset, subRounds: _sub, relay, review, delegate, ...saved } = JSON.parse(raw) as Partial<AppSettings> & Old;
    const api = (id?: string) => (id?.startsWith("api:") ? id : undefined);
    const oldPloop = (saved.ploop ?? {}) as { main?: string; build?: string };
    const single = api(saved.pick);
    const main = single ?? api(oldPloop.main) ?? api(oldPloop.build) ?? api(relay?.build) ?? api(saved.picks?.api);
    const subagents: AppSettings["subagents"] = { ...saved.subagents };
    if (!saved.subagents && api(delegate)) subagents.worker = { model: delegate };
    if (!saved.subagents && api(review)) subagents.auditor = { model: review };
    const settings: AppSettings = {
      ...DEFAULT_SETTINGS,
      ...saved,
      pick: saved.pick === "relay" || single ? "ploop" : saved.pick ?? "",
      ploop: { main },
      subagents,
      quotaWarn: saved.quotaWarn === null ? null : typeof saved.quotaWarn === "number" ? Math.min(100, Math.max(50, saved.quotaWarn)) : 85,
      picks: Object.fromEntries(Object.entries({ ...saved.picks }).map(([k, v]) => (k === "relay" ? ["ploop", "ploop"] : [k, v])).filter(([k]) => k === "cli" || k === "ploop")),
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
    // 换过旧设置的也马上存回去：后端发任务时读存着的 `subagents`（委派、审查挪过去的模型），不存它看不到。
    const migrated = [relay, review, delegate, _sub, _presets, _preset, oldPloop.build].some((v) => v !== undefined) || !!single || "relay" in (saved.picks ?? {});
    if (saved.seenSince === undefined || migrated) void saveSettings(settings).catch(() => {});
    return settings;
  } catch {
    return { ...DEFAULT_SETTINGS, seenSince: Date.now() };
  }
}

export function saveSettings(settings: AppSettings): Promise<void> {
  return ipc.settingsSet(JSON.stringify(settings));
}
