// 左栏下面的【AGENTS】：选谁来干（站主 2026-11-13 定，原来是输入框下的三个按钮）。
//   FLOW 一排分段（API、CLI、接力、规划，开发者选项开着时多一个「假」）；
//   CLI：五家直接列出来点选，下面是这家 CLI 的模型（从 CLI 自己那里读）和思考强度；
//   API：模型用按供应商分组的下拉，下面是思考强度；接力：两对列出来点选；规划：规划的、实施的模型各一个下拉。
// 能收起，收起时头上一行写现在是谁。在跑的时候不能换。
import React, { useEffect, useRef, useState } from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { withAgentModel, type ApiConfig, type ApiStatus } from "@/lib/api";
import { ipc, type CliModel, type CliModels } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import { ago, cn, entryDot } from "@/lib/utils";
import { Dot, FoldBtn, Icon, MenuChip, ModHead } from "@/ui";
import { cliEngine, defaultEffort, firstOf, flowOf, labelOf, pickable, whoText, type FlowKind } from "@/shell/ModelPicker";

const money = (v: number) => `$${v >= 10 ? v.toFixed(0) : v >= 1 ? v.toFixed(2).replace(/\.?0+$/, "") : v.toFixed(3).replace(/0+$/, "").replace(/\.$/, "")}`;

const FLOWS: Array<{ id: FlowKind; label: string; does: string; none: string }> = [
  { id: "api", label: "API", does: "Peck 循环配一个 API 模型", none: "先在设置里配 Key" },
  { id: "cli", label: "CLI", does: "一个 CLI 整包干", none: "没检测到 CLI" },
  { id: "relay", label: "接力", does: "两个 CLI：先写规范再实施", none: "Muse、Grok 都要检测到" },
  { id: "plan", label: "规划", does: "两个 API 模型：拆、做、审", none: "用不了" },
  { id: "fake", label: "假", does: "假 Agent：照剧本演示（开发者选项）", none: "" },
];

interface Props {
  /** 模型列表（全部，含用不了的：CLI 没检测到的也列出来、变灰）。 */
  entries: ModelEntry[];
  value: ModelEntry | null;
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  apiStatus: ApiStatus | null;
  onApiConfig: (config: ApiConfig) => Promise<void>;
  /** 这个项目有任务在跑：不能换。 */
  disabled: boolean;
  onOpenSettings: () => void;
  /** PROJECTS 收起了：这一块占满。 */
  grow: boolean;
  onFold: (folded: boolean) => void;
  /** 每变一次闪一下（输入框下点了「谁」）。 */
  flash: number;
  say: (message: unknown) => void;
}

/** 强度：四档以内、字不多用分段，不然用下拉。`default` 是不指定（CLI 自己定），`off` 是不思考。 */
const EffortRow: React.FC<{ options: string[]; value: string; onChange: (v: string) => void; disabled: boolean }> = ({ options, value, onChange, disabled }) => {
  const name = (o: string) => (o === "default" ? "默认" : o === "off" ? "OFF" : o.toUpperCase());
  return (
    <div className="agents-row">
      <span className="agents-k">思考</span>
      {options.length <= 4 && options.map(name).join("").length <= 18 ? (
        <div className="pk-seg pk-seg--sm pk-seg--full" role="group" aria-label="思考强度">
          {options.map((o) => (
            <button key={o} type="button" disabled={disabled} aria-pressed={o === value} className={o === value ? "is-on" : undefined} onClick={() => onChange(o)}>{name(o)}</button>
          ))}
        </div>
      ) : (
        <MenuChip sm disabled={disabled} ariaLabel="思考强度" label={name(value)}>
          {(close) => (
            <>
              <p className="pk-menu-group">思考强度</p>
              {options.map((o) => (
                <button key={o} type="button" role="menuitem" className={o === value ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(o); close(); }}>
                  <Dot kind="" />
                  <b>{name(o)}</b>
                  <span>{o === "default" ? "CLI 自己定" : o === "off" ? "不思考，便宜、快" : ""}</span>
                </button>
              ))}
            </>
          )}
        </MenuChip>
      )}
    </div>
  );
};

export const AgentsPanel: React.FC<Props> = ({ entries, value, settings, onSettings, apiStatus, onApiConfig, disabled, onOpenSettings, grow, onFold: fold, flash, say }) => {
  const folded = !!settings.fold.agents;
  const options = pickable(entries, settings.fakeAgent);
  const flow = value ? flowOf(value) : null;
  const ref = useRef<HTMLDivElement>(null);
  const [lit, setLit] = useState(false);
  useEffect(() => {
    if (!flash) return;
    ref.current?.scrollIntoView({ block: "nearest" });
    setLit(true);
    const t = window.setTimeout(() => setLit(false), 700);
    return () => window.clearTimeout(t);
  }, [flash]);

  const choose = (id: string) => {
    const e = options.find((x) => x.id === id);
    if (!e) return;
    onSettings({ pick: id, picks: { ...settings.picks, [flowOf(e)]: id } });
  };
  const flows = FLOWS.filter((f) => f.id !== "fake" || settings.fakeAgent).filter((f) => f.id !== "plan" || options.some((e) => flowOf(e) === "plan"));

  return (
    <div ref={ref} className={cn("pk-mod left-agents", folded && "is-folded", grow && !folded && "is-grow", lit && "is-flash")} aria-label="谁来干">
      <ModHead label="AGENTS" note={folded ? whoText(value, settings, apiStatus) : disabled ? "在跑，跑完再换" : undefined}>
        <FoldBtn name="AGENTS" folded={folded} onFold={fold} />
      </ModHead>
      {!folded && (
        <div className="agents-body scroll">
          <div className="pk-seg pk-seg--sm pk-seg--full agents-flow" role="group" aria-label="FLOW：怎么干">
            {flows.map((f) => {
              const first = firstOf(options, f.id, settings.picks);
              return (
                <button key={f.id} type="button" disabled={disabled} aria-pressed={flow === f.id} aria-disabled={!first} title={first ? f.does : f.none}
                  className={cn(flow === f.id && "is-on", !first && "is-off")}
                  onClick={() => { if (!first) { say(f.none); return; } if (flow !== f.id) choose(first.id); }}>
                  {f.label}
                </button>
              );
            })}
          </div>
          {flow === "api" && value && <ApiBody entries={options} value={value} settings={settings} onSettings={onSettings} choose={choose} disabled={disabled} />}
          {flow === "cli" && value && <CliBody entries={entries} value={value} settings={settings} onSettings={onSettings} choose={choose} disabled={disabled} say={say} />}
          {flow === "relay" && value && <PickList items={entries.filter((e) => e.group === "flow" && flowOf(e) === "relay")} value={value} choose={choose} disabled={disabled} note={relayNote} />}
          {flow === "plan" && <PlanBody apiStatus={apiStatus} onApiConfig={onApiConfig} disabled={disabled} />}
          {flow === "fake" && <p className="hint agents-hint">不推理，照剧本写几个演示文件，几秒跑完。只在开发和验收时用。</p>}
          {!value && <p className="hint agents-hint">没有能用的：先配 Key 或装 CLI。</p>}
          <button type="button" className="pk-menu-action agents-more" onClick={onOpenSettings}>
            <Icon name="plus" size={12} />配 Key、看 CLI 检测<em>设置</em>
          </button>
        </div>
      )}
    </div>
  );
};

function relayNote(e: ModelEntry): string {
  if (!e.available) return e.unavailableReason ?? "用不了";
  const [a, b] = e.label.split("→").map((x) => x.trim());
  return a && b ? `${a} 写规范，${b} 实施` : "先写规范再实施";
}

/** 一列能点选的（CLI 五家、接力两对）：用不了的变灰、写为什么。 */
const PickList: React.FC<{ items: ModelEntry[]; value: ModelEntry; choose: (id: string) => void; disabled: boolean; note: (e: ModelEntry) => string }> = ({ items, value, choose, disabled, note }) => (
  <ul className="agents-list" role="listbox" aria-label="选一个">
    {items.map((e) => (
      <li key={e.id}>
        <button type="button" role="option" aria-selected={value.id === e.id} aria-disabled={!e.available} disabled={disabled}
          className={cn("pk-menu-item", value.id === e.id && "is-on", !e.available && "is-off")}
          onClick={() => e.available && choose(e.id)}>
          <Dot kind={entryDot(e.id)} />
          <b>{labelOf(e)}</b>
          <span>{note(e)}</span>
        </button>
      </li>
    ))}
  </ul>
);

const ApiBody: React.FC<{ entries: ModelEntry[]; value: ModelEntry; settings: AppSettings; onSettings: Props["onSettings"]; choose: (id: string) => void; disabled: boolean }> = ({ entries, value, settings, onSettings, choose, disabled }) => {
  const groups: Array<{ key: string; title: string; items: ModelEntry[] }> = [];
  for (const e of entries.filter((x) => flowOf(x) === "api")) {
    let g = groups.find((x) => x.key === e.provider);
    if (!g) groups.push((g = { key: e.provider, title: e.providerLabel.toUpperCase(), items: [] }));
    g.items.push(e);
  }
  const price = (e: ModelEntry) => (e.price ? `每百万 进 ${money(e.price.input)} 出 ${money(e.price.output)}` : "价格未知");
  const thinking = value.efforts.length > 0 && value.thinking !== "never";
  const effortOptions = value.thinking === "always" ? value.efforts : ["off", ...value.efforts];
  const effort = settings.efforts[value.id] ?? defaultEffort(value);
  return (
    <>
      <div className="agents-row">
        <span className="agents-k">模型</span>
        <MenuChip sm disabled={disabled} ariaLabel="API 模型" title={labelOf(value)} dot="api" label={labelOf(value)}>
          {(close) => groups.map((g, gi) => (
            <React.Fragment key={g.key}>
              {gi > 0 && <div className="pk-menu-sep" />}
              <p className="pk-menu-group">{g.title}</p>
              {g.items.map((e) => (
                <button key={e.id} type="button" role="menuitem" className={value.id === e.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { choose(e.id); close(); }}>
                  <Dot kind="api" />
                  <b>{labelOf(e)}</b>
                  <span>{price(e)}</span>
                </button>
              ))}
            </React.Fragment>
          ))}
        </MenuChip>
      </div>
      <p className="hint agents-hint">{value.providerLabel} · {price(value)}</p>
      {thinking && <EffortRow options={effortOptions} value={effortOptions.includes(effort) ? effort : defaultEffort(value)} disabled={disabled} onChange={(v) => onSettings({ efforts: { ...settings.efforts, [value.id]: v } })} />}
    </>
  );
};

/** CLI：五家点选，下面是这家的模型和强度。模型列表从 CLI 读（存着，REFRESH 重读）。 */
const CliBody: React.FC<{ entries: ModelEntry[]; value: ModelEntry; settings: AppSettings; onSettings: Props["onSettings"]; choose: (id: string) => void; disabled: boolean; say: Props["say"] }> = ({ entries, value, settings, onSettings, choose, disabled, say }) => {
  const engine = cliEngine(value)!;
  const [list, setList] = useState<CliModels | null>(null);
  const [reading, setReading] = useState(false);
  const read = (refresh: boolean) => {
    setReading(true);
    ipc.cliModels(engine, refresh)
      .then((r) => { setList(r); if (refresh && r.error) say(r.error); })
      .catch(say)
      .finally(() => setReading(false));
  };
  useEffect(() => { setList(null); read(false); }, [engine]); // eslint-disable-line react-hooks/exhaustive-deps
  const models = list?.engine === engine ? list.models : [];
  const chosenId = settings.cliModels[engine] ?? "";
  const chosen = models.find((m) => m.id === chosenId) ?? null;
  const fallback = models.find((m) => m.isDefault) ?? null;
  const shown: CliModel | null = chosen ?? (chosenId ? null : fallback);
  // 没有「CLI 默认」标记的（Claude Code 的别名）：几个模型的强度都一样，就照它们的给。
  const same = models.length > 0 && models.every((m) => m.efforts.join() === models[0].efforts.join());
  const efforts = shown?.efforts ?? (same ? models[0].efforts : []);
  const effortOptions = efforts.length ? ["default", ...efforts] : [];
  const effort = settings.efforts[value.id] ?? "default";
  const label = chosenId ? (chosen?.label ?? chosenId).toUpperCase() : `默认${fallback ? ` · ${fallback.label.toUpperCase()}` : ""}`;
  const pickModel = (id: string) => {
    const next = models.find((m) => m.id === id) ?? fallback;
    const keep = effort === "default" || (next?.efforts ?? []).includes(effort);
    onSettings({ cliModels: { ...settings.cliModels, [engine]: id }, efforts: keep ? settings.efforts : { ...settings.efforts, [value.id]: "default" } });
  };
  const cliNote = (e: ModelEntry) => (!e.available ? "没检测到" : e.directOnly ? "直接调用" : e.version ? `v${e.version.replace(/^v/, "")}` : "");
  return (
    <>
      <PickList items={entries.filter((e) => e.group === "cli")} value={value} choose={choose} disabled={disabled} note={cliNote} />
      <div className="agents-row">
        <span className="agents-k">模型</span>
        <MenuChip sm disabled={disabled} ariaLabel={`${value.label} 的模型`} title={label} dot={entryDot(value.id)} label={reading && !models.length ? "在读…" : label}>
          {(close) => (
            <>
              <p className="pk-menu-group">{value.label.toUpperCase()} 的模型</p>
              <button type="button" role="menuitem" className={!chosenId ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { pickModel(""); close(); }}>
                <Dot kind="" />
                <b>默认</b>
                <span>{fallback ? `CLI 自己定 · 现在是 ${fallback.label}` : "CLI 自己定"}</span>
              </button>
              {models.map((m) => (
                <button key={m.id} type="button" role="menuitem" className={chosenId === m.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { pickModel(m.id); close(); }}>
                  <Dot kind={entryDot(value.id)} />
                  <b>{m.label.toUpperCase()}</b>
                  <span>{m.isDefault ? "CLI 默认" : m.label !== m.id ? m.id : ""}</span>
                </button>
              ))}
              {chosenId && !chosen && models.length > 0 && <p className="pk-menu-group">选过的 {chosenId} 这次没列出来</p>}
              {list?.source !== "builtin" && (
                <>
                  <div className="pk-menu-sep" />
                  <button type="button" className="pk-menu-action" disabled={reading} onClick={() => { read(true); close(); }}>
                    <Icon name="clock" size={12} />{reading ? "在读…" : "重新读一遍"}<em>{list?.readAt ? `上次 ${ago(list.readAt, Date.now())}` : "还没读过"}</em>
                  </button>
                </>
              )}
            </>
          )}
        </MenuChip>
      </div>
      {list?.error && <p className="hint agents-hint is-warn">{list.error}{models.length ? "（下拉里是上次读到的）" : ""}</p>}
      {effortOptions.length > 0 && <EffortRow options={effortOptions} value={effortOptions.includes(effort) ? effort : "default"} disabled={disabled} onChange={(v) => onSettings({ efforts: { ...settings.efforts, [value.id]: v } })} />}
    </>
  );
};

/** 规划 + 实施：规划的、实施的模型各一个下拉（旧 API 层的模型档案，配了 Key 的才能选）。 */
const PlanBody: React.FC<{ apiStatus: ApiStatus | null; onApiConfig: (config: ApiConfig) => Promise<void>; disabled: boolean }> = ({ apiStatus, onApiConfig, disabled }) => {
  const config = apiStatus?.config;
  if (!config) return <p className="hint agents-hint">在读设置…</p>;
  const keyed = (providerId: string) => apiStatus.keys.find((k) => k.providerId === providerId)?.state === "saved";
  return (
    <>
      {(["supervisor", "worker"] as const).map((role) => {
        const chosen = config.agents.find((a) => a.role === role)?.modelId;
        const name = config.models.find((m) => m.id === chosen)?.name ?? "没选";
        return (
          <div key={role} className="agents-row">
            <span className="agents-k">{role === "supervisor" ? "规划" : "实施"}</span>
            <MenuChip sm disabled={disabled} ariaLabel={role === "supervisor" ? "规划的模型" : "实施的模型"} title={name} dot="api" label={name.toUpperCase()}>
              {(close) => (
                <>
                  <p className="pk-menu-group">{role === "supervisor" ? "规划的模型 · 拆任务、审查" : "实施的模型 · 做交过来的子任务"}</p>
                  {config.models.map((m) => {
                    const ok = keyed(m.providerId);
                    return (
                      <button key={m.id} type="button" role="menuitem" aria-disabled={!ok} className={chosen === m.id ? "pk-menu-item is-on" : "pk-menu-item"}
                        onClick={() => { if (!ok) return; void onApiConfig(withAgentModel(config, role, m.id)); close(); }}>
                        <Dot kind="api" />
                        <b>{m.name.toUpperCase()}</b>
                        <span>{ok ? "" : "没配 Key"}</span>
                      </button>
                    );
                  })}
                </>
              )}
            </MenuChip>
          </div>
        );
      })}
    </>
  );
};
