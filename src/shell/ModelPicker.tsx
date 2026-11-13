// 「谁来干」：输入框下三个按钮（站主 2026-11-13 定）。
//   FLOW：怎么干——API（Peck 循环配一个 API 模型）、CLI（一个 CLI 整包干）、接力（两个 CLI，先写规范再实施）、
//         规划 + 实施（两个 API 模型），开发者选项开着时还有假 Agent。
//   CLI、API：选谁；FLOW 决定显示哪个、怎么选（CLI 单选一家；接力选一对；API 按供应商单选一个模型；
//         规划 + 实施分别选规划的、实施的模型）。
// 支持思考强度的模型、CLI 旁边再出一个强度选择。
import React from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { withAgentModel, type ApiConfig, type ApiStatus } from "@/lib/api";
import { entryDot } from "@/lib/utils";
import { Dot, Icon, MenuChip } from "@/ui";

const money = (v: number) => `$${v >= 10 ? v.toFixed(0) : v >= 1 ? v.toFixed(2).replace(/\.?0+$/, "") : v.toFixed(3).replace(/0+$/, "").replace(/\.$/, "")}`;

/** 开发者选项开着时多出来的假 Agent。 */
export const FAKE_ENTRY_ID = "mock";

export const FAKE_ENTRY: ModelEntry = {
  id: FAKE_ENTRY_ID,
  group: "flow",
  label: "假 Agent",
  provider: "mock",
  providerLabel: "开发者选项",
  available: true,
  directOnly: false,
  efforts: [],
  caps: {
    streaming: false, reasoningReplay: false, tools: "engine", approvals: "none", askUser: false, steer: false, interrupt: false,
    resume: false, mcpInject: false, usage: "none", cost: "unknown", quota: false, switchModel: false, needsTty: false, directOnly: false,
  },
};

export function pickable(entries: ModelEntry[], fake: boolean): ModelEntry[] {
  const list = entries.filter((e) => e.available);
  return fake ? [...list, FAKE_ENTRY] : list;
}

// ---------- FLOW ----------

export type FlowKind = "api" | "cli" | "relay" | "plan" | "fake";

const FLOWS: Array<{ id: FlowKind; label: string; note: string; none: string }> = [
  { id: "api", label: "API", note: "Peck 循环 · 一个 API 模型", none: "先在设置里配 Key" },
  { id: "cli", label: "CLI", note: "一个 CLI 整包干", none: "没检测到 CLI" },
  { id: "relay", label: "接力", note: "两个 CLI · 先写规范再实施", none: "Muse、Grok 都要检测到" },
  { id: "plan", label: "规划 + 实施", note: "两个 API 模型 · 拆、做、审", none: "用不了" },
  { id: "fake", label: "假 Agent", note: "开发者选项 · 照剧本演示", none: "" },
];

/** 这一项属于哪种干法。 */
export function flowOf(e: ModelEntry): FlowKind {
  if (e.id === FAKE_ENTRY_ID) return "fake";
  if (e.group === "peck") return "api";
  if (e.group === "cli") return "cli";
  return e.id === "flow:plan-implement" ? "plan" : "relay";
}

/** 换了干法：上次在这种干法里选的还能用就用它，不然这种里第一个。 */
export function firstOf(entries: ModelEntry[], flow: FlowKind, remembered?: Partial<Record<FlowKind, string>>): ModelEntry | null {
  const list = entries.filter((e) => flowOf(e) === flow);
  return list.find((e) => e.id === remembered?.[flow]) ?? list[0] ?? null;
}

/** 选项片上的字：前面一个暗的按钮名（FLOW / CLI / API），后面是选中的。 */
const ChipText: React.FC<{ k: string; v: string }> = ({ k, v }) => <><i className="who-k">{k}</i>{v}</>;

function cliNote(e: ModelEntry): string {
  if (e.directOnly) return "直接调用";
  return [e.account, e.version ? `v${e.version.replace(/^v/, "")}` : null].filter(Boolean).join(" · ") || "订阅额度";
}

function apiNote(e: ModelEntry): string {
  const p = e.price;
  return p ? `每百万 进 ${money(p.input)} 出 ${money(p.output)}` : "价格未知";
}

export function labelOf(e: ModelEntry): string {
  const name = e.label.toUpperCase();
  return e.account ? `${name} · ${e.account}` : name;
}

interface WhoProps {
  /** 能选的（已经滤掉用不了的；假 Agent 要的话已经加上）。 */
  entries: ModelEntry[];
  value: ModelEntry | null;
  onChange: (id: string) => void;
  /** 每种干法上次选的哪个。 */
  remembered?: Partial<Record<FlowKind, string>>;
  disabled?: boolean;
  up?: boolean;
  onOpenSettings?: () => void;
  /** 规划 + 实施要：旧 API 层的两个角色和它们的模型。没给就只显示、不能在这里换。 */
  apiStatus?: ApiStatus | null;
  onApiConfig?: (config: ApiConfig) => Promise<void>;
  /** 只给这几种干法（ONE SHOT 只要 API、CLI）。 */
  only?: FlowKind[];
  /** 跟在 CLI / API 后面的东西（思考强度）。 */
  children?: React.ReactNode;
}

/** FLOW、CLI、API 三个按钮；FLOW 决定后两个出不出。 */
export const WhoPicker: React.FC<WhoProps> = ({ entries, value, onChange, remembered, disabled, up = true, onOpenSettings, apiStatus, onApiConfig, only, children }) => {
  const flow = value ? flowOf(value) : null;
  // API、CLI、接力总列出来（用不了的变灰、写为什么）；规划 + 实施、假 Agent 有才列。
  const flows = FLOWS.filter((f) => !only || only.includes(f.id))
    .filter((f) => f.id === "api" || f.id === "cli" || f.id === "relay" || entries.some((e) => flowOf(e) === f.id));
  const current = FLOWS.find((f) => f.id === flow);
  const settingsAction = (close: () => void) => onOpenSettings && (
    <>
      <div className="pk-menu-sep" />
      <button type="button" className="pk-menu-action" onClick={() => { close(); onOpenSettings(); }}>
        <Icon name="plus" size={12} />配 Key、看 CLI 检测<em>设置</em>
      </button>
    </>
  );
  return (
    <>
      <MenuChip sm up={up} disabled={disabled} ariaLabel="FLOW：怎么干" title={current ? `FLOW：${current.label}` : "FLOW"} label={<ChipText k="FLOW" v={current ? current.label.toUpperCase() : "选一种"} />}>
        {(close) => (
          <>
            <p className="pk-menu-group">FLOW</p>
            {flows.map((f) => {
              const first = firstOf(entries, f.id, remembered);
              return (
                <button key={f.id} type="button" role="menuitem" aria-disabled={!first} className={flow === f.id ? "pk-menu-item is-on" : "pk-menu-item"}
                  onClick={() => { if (!first) return; if (flow !== f.id) onChange(first.id); close(); }}>
                  <Dot kind={f.id === "api" || f.id === "plan" ? "api" : f.id === "fake" ? "" : first ? entryDot(first.id) : ""} />
                  <b>{f.label.toUpperCase()}</b>
                  <span>{first ? f.note : f.none}</span>
                </button>
              );
            })}
            {settingsAction(close)}
          </>
        )}
      </MenuChip>
      {(flow === "cli" || flow === "relay") && value && <CliChip entries={entries} value={value} flow={flow} onChange={onChange} disabled={disabled} up={up} settingsAction={settingsAction} />}
      {flow === "api" && value && <ApiChip entries={entries} value={value} onChange={onChange} disabled={disabled} up={up} settingsAction={settingsAction} />}
      {flow === "plan" && <PlanChip apiStatus={apiStatus ?? null} onApiConfig={onApiConfig} disabled={disabled} up={up} />}
      {(flow === "api" || flow === "cli") && children}
    </>
  );
};

const CliChip: React.FC<{
  entries: ModelEntry[]; value: ModelEntry; flow: "cli" | "relay"; onChange: (id: string) => void; disabled?: boolean; up: boolean;
  settingsAction: (close: () => void) => React.ReactNode;
}> = ({ entries, value, flow, onChange, disabled, up, settingsAction }) => {
  const list = entries.filter((e) => flowOf(e) === flow);
  return (
    <MenuChip sm up={up} disabled={disabled} ariaLabel={flow === "cli" ? "CLI：哪一家" : "CLI：哪两家接力"} title={labelOf(value)} dot={entryDot(value.id)} label={<ChipText k="CLI" v={labelOf(value)} />}>
      {(close) => (
        <>
          <p className="pk-menu-group">{flow === "cli" ? "CLI" : "接力 · 写规范 → 实施"}</p>
          {list.map((e) => (
            <button key={e.id} type="button" role="menuitem" className={value.id === e.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(e.id); close(); }}>
              <Dot kind={entryDot(e.id)} />
              <b>{e.label.toUpperCase()}</b>
              <span>{flow === "cli" ? cliNote(e) : relayNote(e)}</span>
            </button>
          ))}
          {settingsAction(close)}
        </>
      )}
    </MenuChip>
  );
};

function relayNote(e: ModelEntry): string {
  const [a, b] = e.label.split("→").map((x) => x.trim());
  return a && b ? `${a} 写规范，${b} 实施` : "先写规范再实施";
}

const ApiChip: React.FC<{
  entries: ModelEntry[]; value: ModelEntry; onChange: (id: string) => void; disabled?: boolean; up: boolean;
  settingsAction: (close: () => void) => React.ReactNode;
}> = ({ entries, value, onChange, disabled, up, settingsAction }) => {
  // 按供应商分组。
  const groups: Array<{ key: string; title: string; items: ModelEntry[] }> = [];
  for (const e of entries.filter((x) => flowOf(x) === "api")) {
    let g = groups.find((x) => x.key === e.provider);
    if (!g) groups.push((g = { key: e.provider, title: e.providerLabel.toUpperCase(), items: [] }));
    g.items.push(e);
  }
  return (
    <MenuChip sm up={up} disabled={disabled} ariaLabel="API：哪个模型" title={labelOf(value)} dot="api" label={<ChipText k="API" v={labelOf(value)} />}>
      {(close) => (
        <>
          {groups.map((g, gi) => (
            <React.Fragment key={g.key}>
              {gi > 0 && <div className="pk-menu-sep" />}
              <p className="pk-menu-group">{g.title}</p>
              {g.items.map((e) => (
                <button key={e.id} type="button" role="menuitem" className={value.id === e.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(e.id); close(); }}>
                  <Dot kind="api" />
                  <b>{e.label.toUpperCase()}</b>
                  <span>{apiNote(e)}</span>
                </button>
              ))}
            </React.Fragment>
          ))}
          {settingsAction(close)}
        </>
      )}
    </MenuChip>
  );
};

/** 规划 + 实施：规划的、实施的模型各选一个（旧 API 层的模型档案，配了 Key 的才能选）。 */
const PlanChip: React.FC<{ apiStatus: ApiStatus | null; onApiConfig?: (config: ApiConfig) => Promise<void>; disabled?: boolean; up: boolean }> = ({ apiStatus, onApiConfig, disabled, up }) => {
  const config = apiStatus?.config;
  if (!config) return null;
  const name = (role: "supervisor" | "worker") => {
    const id = config.agents.find((a) => a.role === role)?.modelId;
    return config.models.find((m) => m.id === id)?.name ?? "没选";
  };
  const keyed = (providerId: string) => apiStatus.keys.find((k) => k.providerId === providerId)?.state === "saved";
  const text = `${name("supervisor").toUpperCase()} → ${name("worker").toUpperCase()}`;
  return (
    <MenuChip sm up={up} disabled={disabled || !onApiConfig} ariaLabel="API：规划和实施的模型" title={`规划 ${name("supervisor")} · 实施 ${name("worker")}`} dot="api" label={<ChipText k="API" v={text} />}>
      {(close) => (
        <>
          {(["supervisor", "worker"] as const).map((role, ri) => {
            const chosen = config.agents.find((a) => a.role === role)?.modelId;
            return (
              <React.Fragment key={role}>
                {ri > 0 && <div className="pk-menu-sep" />}
                <p className="pk-menu-group">{role === "supervisor" ? "规划的模型" : "实施的模型"}</p>
                {config.models.map((m) => {
                  const ok = keyed(m.providerId);
                  return (
                    <button key={m.id} type="button" role="menuitem" aria-disabled={!ok} className={chosen === m.id ? "pk-menu-item is-on" : "pk-menu-item"}
                      onClick={() => { if (!ok || !onApiConfig) return; void onApiConfig(withAgentModel(config, role, m.id)); close(); }}>
                      <Dot kind="api" />
                      <b>{m.name.toUpperCase()}</b>
                      <span>{ok ? (role === "supervisor" ? "拆任务、审查" : "做交过来的子任务") : "没配 Key"}</span>
                    </button>
                  );
                })}
              </React.Fragment>
            );
          })}
        </>
      )}
    </MenuChip>
  );
};

// ---------- 思考强度 ----------

/** 思考强度：能开能关的多一个 OFF；不支持的不出。 */
export const EffortPicker: React.FC<{
  entry: ModelEntry | null;
  value: string;
  onChange: (effort: string) => void;
  disabled?: boolean;
  up?: boolean;
}> = ({ entry, value, onChange, disabled, up = true }) => {
  if (!entry || entry.efforts.length === 0 || entry.thinking === "never") return null;
  const options = entry.thinking === "always" || entry.group !== "peck" ? entry.efforts : ["off", ...entry.efforts];
  const current = options.includes(value) ? value : defaultEffort(entry);
  return (
    <MenuChip sm up={up} disabled={disabled} ariaLabel="思考强度" label={`思考 ${current === "off" ? "OFF" : current.toUpperCase()}`}>
      {(close) => (
        <>
          <p className="pk-menu-group">思考强度</p>
          {options.map((o) => (
            <button key={o} type="button" role="menuitem" className={o === current ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(o); close(); }}>
              <Dot kind="" />
              <b>{o === "off" ? "OFF" : o.toUpperCase()}</b>
              <span>{o === "off" ? "不思考，便宜、快" : ""}</span>
            </button>
          ))}
        </>
      )}
    </MenuChip>
  );
}

/** 没选过时的强度：中间那一档。 */
export function defaultEffort(entry: ModelEntry): string {
  if (entry.efforts.length === 0) return "off";
  return entry.efforts.includes("medium") ? "medium" : entry.efforts[Math.floor((entry.efforts.length - 1) / 2)];
}

/** 发任务时带的强度（OFF 和不支持的不带）。 */
export function effortFor(entry: ModelEntry | null, saved: Record<string, string>): string | undefined {
  if (!entry || entry.efforts.length === 0 || entry.thinking === "never") return undefined;
  const v = saved[entry.id] ?? defaultEffort(entry);
  return v === "off" || !entry.efforts.includes(v) ? undefined : v;
}
