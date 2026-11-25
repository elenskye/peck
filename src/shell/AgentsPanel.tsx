// 左栏下面的【AGENTS】：选谁来干（站主 2026-11-13 定，原来是输入框下的三个按钮；v0.8 改；v1.1.1 接力换成 PLOOP；
// v1.1.2 去掉 API 单干）。
//   FLOW 一排分段（CLI、PLOOP）；
//   CLI：五家直接列出来点选，下面是这家 CLI 的模型（从 CLI 自己那里读）和思考强度。Peck 只当外壳；
//   PLOOP：Peck Loop，主角色是一个 API 模型（按供应商分组的下拉），下面是它的思考强度；四类子代理在设置 → AGENTS。
// 能收起，收起时头上一行写现在是谁。在跑的时候不能换。
import React, { useEffect, useRef, useState } from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { ipc, type CliModel, type CliModels } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import { ago, cn, entryDot } from "@/lib/utils";
import { Dot, FoldBtn, Icon, Info, MenuChip, ModHead } from "@/ui";
import { cliEngine, defaultEffort, firstOf, flowOf, labelOf, money, pickable, ploopMain, whoShort, type FlowKind } from "@/shell/ModelPicker";

const priceOf = (e: ModelEntry) => {
  if (!e.price) return "价格未知";
  const sym = e.currency === "CNY" ? "¥" : "$";
  return `进 ${money(e.price.input, sym)} 出 ${money(e.price.output, sym)}`;
};

const FLOWS: Array<{ id: FlowKind; label: string; does: string; none: string }> = [
  { id: "cli", label: "CLI", does: "一个 CLI 整包干，Peck 只当外壳", none: "没检测到 CLI" },
  { id: "ploop", label: "PLOOP", does: "Peck Loop：一个 API 模型当主角色，能派子代理", none: "先在设置里配 Key" },
];

/** 头上的 ⓘ：子代理在哪配、强度从哪来。 */
const EFFORT_NOTE = "PLOOP 的主角色能派五类子代理：explorer 读项目、searcher 联网调研、worker 做简单的改动、member 做复杂的活、auditor 审改动。worker、member 只改划给它的文件，不重叠的同时跑、重叠的排队，进度在对话里的任务板上。每类用哪个模型、思考强度、最多几轮、同时几个在设置 → AGENTS；没单独选模型的跟主角色。";

interface Props {
  /** 模型列表（全部，含用不了的：CLI 没检测到的也列出来、变灰）。 */
  entries: ModelEntry[];
  value: ModelEntry | null;
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  /** 这个项目有任务在跑：不能换。 */
  disabled: boolean;
  /** 打开设置（`AGENTS`：PLOOP 的子代理）。 */
  onOpenSettings: (section?: "AGENTS") => void;
  /** PROJECTS 收起了：这一块占满。 */
  grow: boolean;
  /** 全部项目页：左栏只有这一块，一直展开、不给收起。 */
  solo?: boolean;
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

export const AgentsPanel: React.FC<Props> = ({ entries, value, settings, onSettings, disabled, onOpenSettings, grow, solo, onFold: fold, flash, say }) => {
  const folded = !solo && !!settings.fold.agents;
  const options = pickable(entries);
  const main = ploopMain(entries, settings);
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
  // PLOOP 主角色没选过（或选的那个已经不在模型目录里）时显示的是第一项：把它真的存下来。选过的暂时用不了不动它（v1.1.12）。
  useEffect(() => {
    if (flow !== "ploop" || !main || main.id === settings.ploop.main) return;
    if (settings.ploop.main && entries.some((e) => e.id === settings.ploop.main)) return;
    onSettings({ ploop: { ...settings.ploop, main: main.id } });
  }, [flow, main, settings.ploop, onSettings, entries]);
  // 头上一行放不下就截断，收起 / 展开的按钮不会被挤出去（站主 11-17 的 Bug：接力的长名字把按钮挤没了，重启也打不开）。
  const note = folded ? whoShort(value, settings, entries, false) : disabled ? "在跑，跑完再换" : undefined;

  return (
    <div ref={ref} className={cn("pk-mod left-agents", folded && "is-folded", grow && !folded && "is-grow", lit && "is-flash")} aria-label="谁来干">
      <ModHead label="AGENTS" after={!folded && <Info text={EFFORT_NOTE} />} note={note && <span className="mod-note-text" title={note}>{note}</span>}>
        {!solo && <FoldBtn name="AGENTS" folded={folded} onFold={fold} />}
      </ModHead>
      {!folded && (
        <div className="agents-body scroll">
          <div className="pk-seg pk-seg--sm pk-seg--full agents-flow" role="group" aria-label="FLOW：怎么干">
            {FLOWS.map((f) => {
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
          {flow === "cli" && value && <CliBody entries={entries} value={value} settings={settings} onSettings={onSettings} choose={choose} disabled={disabled} say={say} />}
          {/* PLOOP：主角色（只列 API 模型）和它的思考强度；子代理在设置 → AGENTS（站主 11-17：进阶的放设置）。 */}
          {flow === "ploop" && main && (
            <ApiBody entries={options} value={main} settings={settings} onSettings={onSettings} choose={(id) => onSettings({ ploop: { ...settings.ploop, main: id } })} disabled={disabled} label="主角色" />
          )}
          {!value && (
            <button type="button" className="pk-menu-action agents-more" onClick={() => onOpenSettings()}>
              <Icon name="plus" size={12} />配 Key、看 CLI 检测<em>设置</em>
            </button>
          )}
        </div>
      )}
    </div>
  );
};

/** 模型名收短：去掉供应商前缀（目录里没有的 id 也能显示）。 */
const bareName = (id: string) => id.replace(/^(api|cli):/, "").replace(/^[a-z-]+\//, "");

/** 选一个模型的下拉（设置里的子代理用）。`off` 给了就能不选（写成 `off` 的字）。
 *  CLI 不带版本号（站主 11-17：版本在设置 → CLI 里看）；API 模型带价格（每百万 token）。 */
export const ModelChip: React.FC<{ label: string; entries: ModelEntry[]; value: string; off?: string; disabled?: boolean; onChange: (id: string) => void }> = ({ label: aria, entries, value, off, disabled, onChange }) => {
  const chosen = entries.find((e) => e.id === value) ?? null;
  const groups: Array<{ key: string; title: string; items: ModelEntry[] }> = [];
  for (const e of entries) {
    const key = e.group === "cli" ? "cli" : e.provider;
    let g = groups.find((x) => x.key === key);
    if (!g) groups.push((g = { key, title: e.group === "cli" ? "CLI" : e.providerLabel.toUpperCase(), items: [] }));
    g.items.push(e);
  }
  const label = chosen ? labelOf(chosen) : value ? bareName(value).toUpperCase() : off ?? "选一个";
  return (
    <MenuChip sm disabled={disabled} ariaLabel={aria} title={label} dot={chosen ? (chosen.group === "cli" ? entryDot(chosen.id) : "api") : undefined} label={label}>
      {(close) => (
        <>
          {off && (
            <button type="button" role="menuitem" className={!value ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(""); close(); }}>
              <Dot kind="" />
              <b>{off}</b>
              <span />
            </button>
          )}
          {groups.map((g, gi) => (
            <React.Fragment key={g.key}>
              {(gi > 0 || off) && <div className="pk-menu-sep" />}
              <p className="pk-menu-group">{g.title}{g.key !== "cli" && groups.findIndex((x) => x.key !== "cli") === gi && <em className="menu-group-note">每百万 token</em>}</p>
              {g.items.map((e) => (
                <button key={e.id} type="button" role="menuitem" className={value === e.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { onChange(e.id); close(); }}>
                  <Dot kind={e.group === "cli" ? entryDot(e.id) : "api"} />
                  <b>{labelOf(e)}</b>
                  <span>{e.group === "cli" ? "" : priceOf(e)}</span>
                </button>
              ))}
            </React.Fragment>
          ))}
        </>
      )}
    </MenuChip>
  );
};

/** 一列能点选的（CLI 五家）：用不了的变灰、写为什么。 */
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

const ApiBody: React.FC<{ entries: ModelEntry[]; value: ModelEntry; settings: AppSettings; onSettings: Props["onSettings"]; choose: (id: string) => void; disabled: boolean; label?: string }> = ({ entries, value, settings, onSettings, choose, disabled, label = "模型" }) => {
  const groups: Array<{ key: string; title: string; items: ModelEntry[] }> = [];
  for (const e of entries.filter((x) => flowOf(x) === "api")) {
    let g = groups.find((x) => x.key === e.provider);
    if (!g) groups.push((g = { key: e.provider, title: e.providerLabel.toUpperCase(), items: [] }));
    g.items.push(e);
  }
  const thinking = value.efforts.length > 0 && value.thinking !== "never";
  const effortOptions = value.thinking === "always" ? value.efforts : ["off", ...value.efforts];
  const effort = settings.efforts[value.id] ?? defaultEffort(value);
  return (
    <>
      <div className="agents-row">
        <span className="agents-k">{label}</span>
        <MenuChip sm disabled={disabled} ariaLabel={label} title={labelOf(value)} dot="api" label={labelOf(value)}>
          {(close) => groups.map((g, gi) => (
            <React.Fragment key={g.key}>
              {gi > 0 && <div className="pk-menu-sep" />}
              <p className="pk-menu-group">{g.title}{gi === 0 && <em className="menu-group-note">每百万 token</em>}</p>
              {g.items.map((e) => (
                <button key={e.id} type="button" role="menuitem" className={value.id === e.id ? "pk-menu-item is-on" : "pk-menu-item"} onClick={() => { choose(e.id); close(); }}>
                  <Dot kind="api" />
                  <b>{labelOf(e)}</b>
                  <span>{priceOf(e)}</span>
                </button>
              ))}
            </React.Fragment>
          ))}
        </MenuChip>
      </div>
      {thinking && <EffortRow options={effortOptions} value={effortOptions.includes(effort) ? effort : defaultEffort(value)} disabled={disabled} onChange={(v) => onSettings({ efforts: { ...settings.efforts, [value.id]: v } })} />}
    </>
  );
};

const STALE_LIST_MS = 3 * 86_400_000;

/** CLI：四家点选，下面是这家的模型和强度。模型列表从 CLI 读（存着，REFRESH 重读；存了 3 天以上自己重读）。 */
const CliBody: React.FC<{ entries: ModelEntry[]; value: ModelEntry; settings: AppSettings; onSettings: Props["onSettings"]; choose: (id: string) => void; disabled: boolean; say: Props["say"] }> = ({ entries, value, settings, onSettings, choose, disabled, say }) => {
  const engine = cliEngine(value)!;
  const [list, setList] = useState<CliModels | null>(null);
  const [reading, setReading] = useState(false);
  // 存着的清单超过 3 天就在后台重读一次（v1.2.0：Codex 的清单停在 11-13，新出的 gpt-6.2-sol 选不到）；悄悄的，读不到不报。
  // 回来时已经换成别的引擎了就丢掉（后台重读要几秒，不然会把新引擎的清单冲掉）。
  const current = useRef(engine);
  current.current = engine;
  const read = async (refresh: boolean) => {
    const asked = engine;
    setReading(true);
    try {
      const r = await ipc.cliModels(asked, refresh);
      if (current.current !== asked) return;
      setList(r);
      if (refresh && r.error) say(r.error);
      if (!refresh && r.source === "cli" && r.readAt && Date.now() - r.readAt > STALE_LIST_MS) {
        const fresh = await ipc.cliModels(asked, true).catch(() => null);
        if (fresh && !fresh.error && current.current === asked) setList(fresh);
      }
    } catch (e) {
      if (current.current === asked) say(e);
    } finally {
      if (current.current === asked) setReading(false);
    }
  };
  useEffect(() => { setList(null); void read(false); }, [engine]); // eslint-disable-line react-hooks/exhaustive-deps
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
  // 没选就是 CLI 自己的默认：写它现在默认的那个（站主 11-17：不要一长串）。
  const label = chosenId ? (chosen?.label ?? chosenId).toUpperCase() : fallback ? fallback.label.toUpperCase() : "默认";
  const pickModel = (id: string) => {
    const next = models.find((m) => m.id === id) ?? fallback;
    const keep = effort === "default" || (next?.efforts ?? []).includes(effort);
    onSettings({
      cliModels: { ...settings.cliModels, [engine]: id },
      cliModelNames: { ...settings.cliModelNames, [engine]: id ? models.find((m) => m.id === id)?.label ?? id : "" },
      efforts: keep ? settings.efforts : { ...settings.efforts, [value.id]: "default" },
    });
  };
  // 版本号不放这里（站主 11-17：设置 → CLI 里看得到就行）。
  const cliNote = (e: ModelEntry) => (!e.available ? "没检测到" : "");
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
                <span>CLI 自己定</span>
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
                  <button type="button" className="pk-menu-action" disabled={reading} onClick={() => { void read(true); close(); }}>
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
