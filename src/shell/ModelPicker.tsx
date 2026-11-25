// 「谁来干」（站主 2026-11-13 定；v0.8 改；v1.1.1 接力换成 PLOOP；v1.1.2 去掉 API 单干）。
//   FLOW：怎么干——CLI（一个 CLI 整包干，Peck 只当外壳）、PLOOP（Peck Loop：一个 API 模型当主角色，能派四类子代理）。
//   这里的 WhoPicker 是小号：ONE SHOT 用 API、CLI（只读问一句），项目设置用 CLI、PLOOP。
// 支持思考强度的模型、CLI 旁边再出一个强度选择。
import React from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { entryDot, versionText } from "@/lib/utils";
import { Dot, MenuChip } from "@/ui";
import type { AppSettings } from "@/lib/settings";

/** 每百万 token 的价格：国内的模型是人民币。 */
export const money = (v: number, sym = "$") => `${sym}${v >= 10 ? v.toFixed(0) : v >= 1 ? v.toFixed(2).replace(/\.?0+$/, "") : v.toFixed(3).replace(/0+$/, "").replace(/\.$/, "")}`;

/** PLOOP（v1.1.1）：「谁来干」里的一项，主角色在设置的 `ploop.main`，子代理在 `subagents`。 */
export const PLOOP_ID = "ploop";

export const PLOOP_ENTRY: ModelEntry = {
  id: PLOOP_ID,
  group: "flow",
  label: "PLOOP",
  provider: "ploop",
  providerLabel: "Peck Loop",
  available: true,
  efforts: [],
  caps: {
    streaming: true, reasoningReplay: false, tools: "engine", approvals: "live", askUser: true, steer: false, interrupt: true,
    resume: false, mcpInject: false, usage: "per_turn", cost: "estimated", quota: false, switchModel: false, needsTty: false,
  },
};

/** 能选的：用得了的，加上 PLOOP（有一个能用的 API 模型才有）。 */
export function pickable(entries: ModelEntry[]): ModelEntry[] {
  const list = entries.filter((e) => e.available);
  return ploopable(list).length ? [...list, PLOOP_ENTRY] : list;
}

/** PLOOP 里几格能选的：只有 API 模型（CLI 引擎 Peck 只当外壳，不进工作流）。 */
export function ploopable(entries: ModelEntry[]): ModelEntry[] {
  return entries.filter((e) => e.available && e.group === "peck");
}

// ---------- FLOW ----------

export type FlowKind = "api" | "cli" | "ploop";

const FLOWS: Array<{ id: FlowKind; label: string; note: string; none: string }> = [
  { id: "api", label: "API", note: "Peck Loop · 一个 API 模型", none: "先在设置里配 Key" },
  { id: "cli", label: "CLI", note: "一个 CLI 整包干", none: "没检测到 CLI" },
  { id: "ploop", label: "PLOOP", note: "Peck Loop · 主角色 + 子代理", none: "先在设置里配 Key" },
];

/** 这一项属于哪种干法。 */
export function flowOf(e: ModelEntry): FlowKind {
  if (e.id === PLOOP_ID) return "ploop";
  if (e.group === "peck") return "api";
  return "cli";
}

/** 换了干法：上次在这种干法里选的还能用就用它，不然这种里第一个。 */
export function firstOf(entries: ModelEntry[], flow: FlowKind, remembered?: Partial<Record<FlowKind, string>>): ModelEntry | null {
  const list = entries.filter((e) => flowOf(e) === flow);
  return list.find((e) => e.id === remembered?.[flow]) ?? list[0] ?? null;
}

/** 选项片上的字：前面一个暗的按钮名（FLOW / CLI / API），后面是选中的。 */
const ChipText: React.FC<{ k: string; v: string }> = ({ k, v }) => <><i className="who-k">{k}</i>{v}</>;

function cliNote(e: ModelEntry): string {
  return [e.account, versionText(e.version) || null].filter(Boolean).join(" · ") || "订阅额度";
}

function apiNote(e: ModelEntry): string {
  const p = e.price;
  // 国内的模型按人民币（价格表，v0.10.1）。
  const sym = e.currency === "CNY" ? "¥" : "$";
  return p ? `每百万 进 ${money(p.input, sym)} 出 ${money(p.output, sym)}` : "价格未知";
}

export function labelOf(e: ModelEntry): string {
  // Vertex 上的和供应商自己的同名（v1.1.12）：不标出来分不清，选错了也看不出来。
  const name = e.provider.startsWith("vertex") ? `${e.label.toUpperCase()} · VERTEX` : e.label.toUpperCase();
  return e.account ? `${name} · ${e.account}` : name;
}

interface WhoProps {
  /** 能选的（已经滤掉用不了的）。 */
  entries: ModelEntry[];
  value: ModelEntry | null;
  onChange: (id: string) => void;
  /** 每种干法上次选的哪个。 */
  remembered?: Partial<Record<FlowKind, string>>;
  disabled?: boolean;
  up?: boolean;
  /** 跟在 CLI / API 后面的东西（思考强度）。 */
  children?: React.ReactNode;
  /** 能选哪几种干法（默认 API、CLI：ONE SHOT 用；项目设置是 CLI、PLOOP）。 */
  flows?: FlowKind[];
}

/** FLOW、CLI、API 三个按钮；FLOW 决定后两个出不出。 */
export const WhoPicker: React.FC<WhoProps> = ({ entries, value, onChange, remembered, disabled, up = true, children, flows = ["api", "cli"] }) => {
  const flow = value ? flowOf(value) : null;
  const shown = FLOWS.filter((f) => flows.includes(f.id));
  const current = shown.find((f) => f.id === flow);
  return (
    <>
      <MenuChip sm up={up} disabled={disabled} ariaLabel="FLOW：怎么干" title={current ? `FLOW：${current.label}` : "FLOW"} label={<ChipText k="FLOW" v={current ? current.label.toUpperCase() : "选一种"} />}>
        {(close) => (
          <>
            <p className="pk-menu-group">FLOW</p>
            {shown.map((f) => {
              const first = firstOf(entries, f.id, remembered);
              return (
                <button key={f.id} type="button" role="menuitem" aria-disabled={!first} className={flow === f.id ? "pk-menu-item is-on" : "pk-menu-item"}
                  onClick={() => { if (!first) return; if (flow !== f.id) onChange(first.id); close(); }}>
                  <Dot kind={f.id === "api" || f.id === "ploop" ? "api" : first ? entryDot(first.id) : ""} />
                  <b>{f.label.toUpperCase()}</b>
                  <span>{first ? f.note : f.none}</span>
                </button>
              );
            })}
          </>
        )}
      </MenuChip>
      {flow === "cli" && value && <CliChip entries={entries} value={value} onChange={onChange} disabled={disabled} up={up} />}
      {flow === "api" && value && <ApiChip entries={entries} value={value} onChange={onChange} disabled={disabled} up={up} />}
      {(flow === "api" || flow === "cli") && children}
    </>
  );
};

const CliChip: React.FC<{
  entries: ModelEntry[]; value: ModelEntry; onChange: (id: string) => void; disabled?: boolean; up: boolean;
}> = ({ entries, value, onChange, disabled, up }) => {
  const list = entries.filter((e) => e.group === "cli");
  return (
    <MenuChip sm up={up} disabled={disabled} ariaLabel="CLI：哪一家" title={labelOf(value)} dot={entryDot(value.id)} label={<ChipText k="CLI" v={labelOf(value)} />}>
      {(close) => (
        <>
          <p className="pk-menu-group">CLI</p>
          {list.map((e) => (
            <button key={e.id} type="button" role="menuitem" className={value.id === e.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(e.id); close(); }}>
              <Dot kind={entryDot(e.id)} />
              <b>{e.label.toUpperCase()}</b>
              <span>{cliNote(e)}</span>
            </button>
          ))}
        </>
      )}
    </MenuChip>
  );
};

const ApiChip: React.FC<{
  entries: ModelEntry[]; value: ModelEntry; onChange: (id: string) => void; disabled?: boolean; up: boolean;
}> = ({ entries, value, onChange, disabled, up }) => {
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

// ---------- 发出去时带什么（AGENTS 里选的） ----------

/** CLI 引擎的 id（`cli:codex` → `codex`）；不是 CLI 是 null。 */
export function cliEngine(e: ModelEntry | null): string | null {
  return e && e.group === "cli" ? e.id.slice(4) : null;
}

/** CLI 用哪个模型：AGENTS 里选的；没选是 undefined（CLI 自己的默认）。 */
export function cliModelOf(e: ModelEntry | null, settings: { cliModels: Record<string, string> }): string | undefined {
  const engine = cliEngine(e);
  return engine ? settings.cliModels[engine] || undefined : undefined;
}

/** 发出去带的思考强度：API 照模型的档（OFF 不带）；CLI 是 AGENTS 里选的（`default` 不带，CLI 自己定）。 */
export function sendEffort(e: ModelEntry | null, settings: { efforts: Record<string, string> }): string | undefined {
  if (!e) return undefined;
  if (e.group === "cli") {
    const v = settings.efforts[e.id];
    return v && v !== "default" ? v : undefined;
  }
  return effortFor(e, settings.efforts);
}

/** PLOOP 的主角色：选过的就是它，哪怕现在用不了（钥匙串还没点、Key 还在读）；没选过、或者选的那个已经不在模型目录里，
 *  才用列表里第一个能用的 API 模型（v1.1.12：以前选的暂时用不了就换成第一个，换的还存进设置，悄悄换了一家）。 */
export function ploopMain(entries: ModelEntry[], settings: { ploop?: { main?: string } }): ModelEntry | null {
  const chosen = entries.find((e) => e.group === "peck" && e.id === settings.ploop?.main);
  return chosen ?? ploopable(entries)[0] ?? null;
}

/** 短的一行（输入框下、AGENTS 收起时）：PLOOP 写「PLOOP · 主角色」，CLI 写它的名字。 */
export function whoShort(e: ModelEntry | null, settings: AppSettings, entries: ModelEntry[], _flags = true): string {
  if (!e) return "还没选谁来干";
  if (flowOf(e) !== "ploop") return labelOf(e);
  const main = ploopMain(entries, settings);
  return main ? `PLOOP · ${labelOf(main)}` : "PLOOP";
}

/** 全的一行（悬停看）：PLOOP 写主角色和它的思考强度；CLI 写模型和强度。 */
export function whoText(
  e: ModelEntry | null,
  settings: { cliModels: Record<string, string>; cliModelNames?: Record<string, string>; efforts: Record<string, string>; ploop?: { main?: string } },
  entries: ModelEntry[] = [],
): string {
  if (!e) return "还没选谁来干";
  const flow = flowOf(e);
  if (flow === "ploop") {
    const main = ploopMain(entries, settings);
    if (!main) return "PLOOP · 还没有能用的 API 模型";
    const v = main.efforts.length > 0 && main.thinking !== "never" ? settings.efforts[main.id] ?? defaultEffort(main) : null;
    return ["PLOOP", `主角色 ${labelOf(main)}`, v ? (v === "off" ? "不思考" : `思考 ${v.toUpperCase()}`) : null, "子代理在设置 → AGENTS"].filter(Boolean).join(" · ");
  }
  const parts = [labelOf(e)];
  if (flow === "cli") {
    const engine = cliEngine(e)!;
    const id = cliModelOf(e, settings);
    parts.push((id ? settings.cliModelNames?.[engine] || id : "默认模型").toUpperCase());
  }
  if (flow === "api" && e.efforts.length > 0 && e.thinking !== "never") {
    const v = settings.efforts[e.id] ?? defaultEffort(e);
    parts.push(v === "off" ? "不思考" : `思考 ${v.toUpperCase()}`);
  } else {
    const v = sendEffort(e, settings);
    if (v) parts.push(`思考 ${v.toUpperCase()}`);
  }
  return parts.join(" · ");
}
