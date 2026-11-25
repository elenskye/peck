import React, { useEffect, useState } from "react";
import { api, type ApiMode, type ApiStatus } from "@/lib/api";
import { ipc, isMockMode, type AllowRule, type McpServerConfig, type McpToolInfo, type PriceBook, type PriceRow, type PriceView } from "@/lib/ipc";
import { SUBAGENT_KINDS, type AppSettings, type SubagentKind, type SubagentSettings } from "@/lib/settings";
import type { ThemeChoice } from "@/lib/theme";
import type { CliAccount, CliAccountStatus, CliLoginState, EngineAccounts, GatewayConfig, ModelEntry } from "@/lib/agentEvents";
import type { Permission, ProjectOverview } from "@/lib/types";
import { PERMISSIONS, ago, cn, copyText, engineDot, plainLine, ruleWord, shortenHomePath, versionNumber } from "@/lib/utils";
import { Btn, ComboField, Dot, Field, Icon, MenuChip, ModHead, Row, Seg, SetHead, Switch } from "@/ui";
import { ModelChip } from "@/shell/AgentsPanel";
import { EffortPicker, defaultEffort, ploopMain, ploopable } from "@/shell/ModelPicker";
import { PluginsSettings } from "@/shell/Plugins";
import { MemorySettings } from "@/shell/Memory";

/** 设置的分组（v0.10 Refine：只写英文；DEV、NOTIFY、LIMIT 并成 GENERAL，新加 AGENTS、QUOTA；v1.1.4 加 MEMORY）。 */
export const SETTINGS_NAV = ["GENERAL", "PROVIDERS", "AGENTS", "MEMORY", "QUOTA", "ACCESS", "CLI", "LOOK", "PLUGINS", "ARCHIVE", "KEYS"] as const;
export type SettingsSection = (typeof SETTINGS_NAV)[number];

interface Props {
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  /** 模型列表（AGENTS 里编辑预设用）。 */
  entries: ModelEntry[];
  /** 打开时在哪一组（AGENTS 面板里「管理预设」直接到 AGENTS）。 */
  initial?: SettingsSection;
  /** 重新检测 CLI（重读模型列表）。 */
  onRefreshAgents: () => Promise<void>;
  apiStatus: ApiStatus | null;
  onRefreshApi: () => Promise<ApiStatus | null>;
  onClose: () => void;
  say: (message: unknown) => void;
  /** 归档里点开一个：只读地看。 */
  onOpenArchived: (group: ProjectOverview, taskId: string) => void;
  /** 放回左栏（项目移出列表了一起放回）。 */
  onRestoreArchived: (taskId: string) => Promise<void>;
  /** 连同记录删掉。 */
  onDeleteArchived: (taskId: string) => Promise<void>;
  /** 左栏的项目（PLUGINS 的小窗里按项目装、开关）。 */
  projects: ProjectOverview[];
  /** 改了某个项目的设置（自带插件的开关）：重读左栏。 */
  onProjectsChanged: () => Promise<void>;
}

const Tag: React.FC<{ v?: "ok" | "wait" | "high"; children: React.ReactNode }> = ({ v, children }) => <em className={cn("pk-tag", v && `pk-tag--${v}`)}>{children}</em>;

/** 设置：一个面板，占中栏和右栏合成的宽栏；改了马上生效（Key、Vertex、价格这类要点 SAVE）。
 *  v0.10 Refine（站主 11-17）：分组只写英文、不放 ⓘ 和说明文字。 */
export const SettingsPage: React.FC<Props> = (props) => {
  const [section, setSection] = useState<SettingsSection>(props.initial ?? "GENERAL");
  return (
    <section className="col page" aria-label="设置">
      <div className="pk-mod col-grow page-mod is-big">
        <ModHead label="SETTINGS"><Btn v="ghost" sm icon="x" onClick={props.onClose}>CLOSE</Btn></ModHead>
        <div className="set-grid">
          <nav className="set-nav" aria-label="设置分组">
            {SETTINGS_NAV.map((en) => (
              <button key={en} type="button" className={en === section ? "is-on" : undefined} aria-current={en === section} onClick={() => setSection(en)}>
                <b className="t-button">{en}</b>
              </button>
            ))}
          </nav>
          <div className="set-pane set-scope scroll" key={section}>
            {section === "GENERAL" && <General {...props} />}
            {section === "PROVIDERS" && <Providers {...props} />}
            {section === "AGENTS" && <AgentsSettings {...props} />}
            {section === "MEMORY" && <MemorySettings say={props.say} />}
            {section === "QUOTA" && <QuotaSettings say={props.say} />}
            {section === "ACCESS" && <Access {...props} />}
            {section === "CLI" && <Cli {...props} />}
            {section === "LOOK" && <Look {...props} />}
            {section === "PLUGINS" && <><PluginsSettings projects={props.projects} onProjectsChanged={props.onProjectsChanged} say={props.say} /><Mcp {...props} /></>}
            {section === "ARCHIVE" && <Archive {...props} />}
            {section === "KEYS" && <Keys />}
          </div>
        </div>
      </div>
    </section>
  );
};

// ---------- 归档 ----------

const Archive: React.FC<Props> = ({ onOpenArchived, onRestoreArchived, onDeleteArchived, say }) => {
  const [groups, setGroups] = useState<ProjectOverview[] | null>(null);
  const [find, setFind] = useState("");
  const [armed, setArmed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => ipc.archiveList().then(setGroups);
  useEffect(() => { load().catch(say); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const act = async (fn: () => Promise<unknown>, message: string) => {
    setBusy(true);
    try { await fn(); await load(); say(message); } catch (e) { say(e); } finally { setBusy(false); setArmed(null); }
  };
  const total = groups?.reduce((n, g) => n + g.tasks.length, 0) ?? 0;
  const q = find.trim().toLowerCase();
  const shown = (groups ?? [])
    .map((g) => ({ ...g, tasks: q ? g.tasks.filter((t) => `${t.title} ${g.workspace.name}`.toLowerCase().includes(q)) : g.tasks }))
    .filter((g) => g.tasks.length);
  return (
    <>
      {total > 6 && <Field className="arch-find" aria-label="在归档里找" placeholder="任务名、项目名" value={find} onChange={(e) => setFind(e.target.value)} />}
      {groups === null ? <p className="hint">在读…</p> : total === 0 ? <p className="hint">还没有。</p> : shown.length === 0 ? <p className="hint">没找到。</p> : shown.map((g) => (
        <div key={g.workspace.id} className="arch-group">
          <p className="arch-head">
            <b className="t-name">{g.workspace.name}</b>
            {!g.missing && <span>{g.workspace.hidden ? "移出列表了" : shortenHomePath(g.workspace.path)}</span>}
          </p>
          <div className="set-rows set-rows--acts">
            {g.tasks.map((t) => (
              <Row key={t.id} name={plainLine(t.title)} desc={`${t.lastRun ? `任务 ${t.lastRun.number}` : ""}${t.runCount > 1 ? ` 等 ${t.runCount} 次` : ""} · ${ago(t.archivedAt ?? t.updatedAt)} 归档`}>
                <Btn sm disabled={busy} aria-label={`只读打开「${plainLine(t.title)}」`} onClick={() => onOpenArchived(g, t.id)}>OPEN</Btn>
                <Btn sm disabled={busy} aria-label={`把「${plainLine(t.title)}」放回左栏`} onClick={() => void act(() => onRestoreArchived(t.id), "放回左栏了")}>RESTORE</Btn>
                <Btn sm v="danger" disabled={busy} aria-label={armed === t.id ? `再点一次删掉「${plainLine(t.title)}」` : `删掉「${plainLine(t.title)}」`} onClick={() => { if (armed !== t.id) { setArmed(t.id); return; } void act(() => onDeleteArchived(t.id), "删掉了（项目里的文件没动）"); }}>{armed === t.id ? "CONFIRM" : "DELETE"}</Btn>
              </Row>
            ))}
          </div>
        </div>
      ))}
    </>
  );
};

// ---------- 供应商和 Key（v1.1.6：只分 API KEYS、TOOL KEYS 两组，Vertex AI 并进 API KEYS） ----------

const KEY_STATE: Record<string, string | undefined> = { unloaded: "在读钥匙串", unavailable: "钥匙串没授权" };

/** 不是 AI 供应商的 Key（和后端 `OTHER_KEYS` 对应）：Tavily 是 Peck Loop 联网搜索的备用；Mathpix 是 `ocr_pdf`，两样都填了才给模型这个工具。 */
const TOOL_KEYS = [
  { id: "tavily", name: "Tavily" },
  { id: "mathpix_id", name: "Mathpix App ID", hint: "粘贴 Mathpix 的 App ID" },
  { id: "mathpix_key", name: "Mathpix App Key", hint: "粘贴 Mathpix 的 App Key" },
];

/** 一块：头一行名字、（Aliyun 的地域、Vertex 的 Location）、状态；下面一行框、SAVE、REMOVE。 */
const KeyBlock: React.FC<{ name: string; off: boolean; desc?: string; tag: React.ReactNode; option?: React.ReactNode; field: React.ReactNode; save: React.ReactNode; remove: React.ReactNode }> = ({ name, off, desc, tag, option, field, save, remove }) => (
  <div className={cn("set-block", !!option && "set-block--key-option")}>
    <Row name={name} off={off} desc={desc}>{option}<span className="set-tag">{tag}</span></Row>
    <div className="set-line set-line--key">{field}{save}{remove}</div>
  </div>
);

const Providers: React.FC<Props> = ({ apiStatus, onRefreshApi, say }) => {
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  if (!apiStatus) return <p className="hint">在读设置…</p>;
  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try { await fn(); } catch (e) { say(e); } finally { setBusy(false); }
  };
  const keyBlock = (p: { id: string; name: string; hint?: string }) => {
    const state = apiStatus.keys.find((k) => k.providerId === p.id)?.state ?? "missing";
    // Aliyun 的地域：点了就存。
    const region = p.id === "aliyun" && (
      <Seg label="Aliyun 的地域" sm disabled={busy} items={[{ id: "beijing", label: "BEIJING" }, { id: "singapore", label: "SINGAPORE" }]} value={apiStatus.config.aliyunRegion}
        onChange={(aliyunRegion) => void act(async () => { await api.saveConfig({ ...apiStatus.config, aliyunRegion }); await onRefreshApi(); say("Aliyun 的地域存好了"); })} />
    );
    return (
      <KeyBlock key={p.id} name={p.name} off={state !== "saved"} desc={KEY_STATE[state]} option={region}
        tag={state === "saved" ? <Tag v="ok">SAVED</Tag> : <Tag>{state === "unloaded" || state === "unavailable" ? "NOT READ" : "NO KEY"}</Tag>}
        field={<Field type="password" aria-label={`${p.name} 的 Key`} placeholder={p.hint ?? `粘贴 ${p.name} 的 Key`} value={keys[p.id] ?? ""} off={busy} onChange={(e) => setKeys((k) => ({ ...k, [p.id]: e.target.value }))} />}
        save={<Btn sm disabled={busy || !keys[p.id]?.trim()} onClick={() => void act(async () => { await api.setKey(p.id, keys[p.id]); setKeys((k) => ({ ...k, [p.id]: "" })); await onRefreshApi(); say(apiStatus.mode === "mock" ? "假 Key 只存在内存里，没进钥匙串" : "Key 已保存到钥匙串；认证通过要发一次请求才知道"); })}>SAVE</Btn>}
        remove={<Btn sm v="danger" disabled={busy || state !== "saved"} onClick={() => void act(async () => { await api.setKey(p.id, null); await onRefreshApi(); })}>REMOVE</Btn>} />
    );
  };
  return (
    <>
      <SetHead label="API KEYS"><Btn sm disabled={busy} onClick={() => void act(async () => { await api.loadKeys(); await onRefreshApi(); })}>READ KEYS</Btn></SetHead>
      <div className="set-rows">
        {apiStatus.providers.map(keyBlock)}
        <Vertex say={say} />
      </div>
      <SetHead label="TOOL KEYS" />
      <div className="set-rows">{TOOL_KEYS.map(keyBlock)}</div>
    </>
  );
};

// ---------- GENERAL：开发者选项 → 通知 → 花费上限（站主 11-17 并成一组） ----------

/** 这个月已经花了多少（Peck Loop）：「花了 / 上限」写在名字下面，右边一格和输入框一样大的 20 格进度（到 80% 变琥珀、到上限变红）。 */
const SpendRow: React.FC<{ cap: number | null }> = ({ cap }) => {
  const [spent, setSpent] = useState<number | null>(null);
  useEffect(() => { void ipc.harnessSpend().then((s) => setSpent(s.monthUsd)).catch(() => setSpent(null)); }, []);
  const used = spent == null ? "—" : `$${spent.toFixed(spent < 1 ? 4 : 2)}`;
  const part = cap && spent != null ? spent / cap : 0;
  const lit = Math.min(20, Math.round(part * 20));
  return (
    <Row name="这个月花了" desc={`${used} / ${cap ? `$${cap}` : "∞"}`}>
      <div className="set-meter">
        <div className={cn("q-meter", part >= 1 ? "is-bad" : part >= 0.8 && "is-warn")} role="meter" aria-valuenow={Math.round(part * 100)} aria-valuemin={0} aria-valuemax={100} aria-label={cap ? `这个月用了上限的 ${Math.round(part * 100)}%` : "没设上限"}>
          {Array.from({ length: 20 }, (_, i) => <i key={i} className={cn(i < lit && "is-on")} />)}
        </div>
      </div>
    </Row>
  );
};

/** 有上下限的数字框：打字时照原样留着，离开或回车才夹到范围里存（以前每敲一个字就夹，打不出 120；v0.10 收尾审查）。
 *  `decimals` 给了就能写小数（汇率）；`unit` 写在框里右边。宽度跟着设置页的统一宽度。 */
const BoundedNumber: React.FC<{ value: number; min: number; max: number; label: string; unit?: string; decimals?: number; off?: boolean; onCommit: (v: number) => void }> = ({ value, min, max, label, unit, decimals, off, onCommit }) => {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft == null) return;
    const v = Number(draft);
    const round = (x: number) => (decimals ? Math.round(x * 10 ** decimals) / 10 ** decimals : Math.round(x));
    if (draft.trim() !== "" && Number.isFinite(v)) onCommit(Math.min(max, Math.max(min, round(v))));
    setDraft(null);
  };
  return (
    <Field off={off} unit={unit} type="number" aria-label={label} min={min} max={max} step={decimals ? 1 / 10 ** decimals : 1} value={draft ?? String(value)}
      onChange={(e) => setDraft(e.target.value)} onBlur={commit} onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />
  );
};

const General: React.FC<Props> = ({ settings, onSettings, apiStatus, onRefreshApi, say }) => {
  const native = !isMockMode();
  return (
    <div className="set-rows">
      <Row name="API 请求发到哪" off={!native} desc={!native ? "浏览器演示里不发请求" : undefined}>
        <Seg<ApiMode> label="API 请求发到哪" sm disabled={!native} items={[{ id: "mock", label: "MOCK" }, { id: "real", label: "REAL" }]} value={apiStatus?.mode ?? "mock"} onChange={(mode) => void api.setMode(mode).then(onRefreshApi).catch(say)} />
      </Row>
      <Row name="发通知"><Switch label="发通知" on={settings.notifyDone} onChange={(notifyDone) => onSettings({ notifyDone })} /></Row>
      <Row name="每月上限">
        <Field type="number" unit="USD" aria-label="每月花费上限（美元）" placeholder="不设" min={0} value={settings.costCap ?? ""} onChange={(e) => onSettings({ costCap: e.target.value === "" ? null : Math.max(0, Number(e.target.value)) })} />
      </Row>
      <SpendRow cap={settings.costCap} />
      <Row name="每个任务最多几轮" off={settings.roundsUnlimited}>
        <Switch label="限制每个任务的轮数" on={!settings.roundsUnlimited} onChange={(on) => onSettings({ roundsUnlimited: !on })} />
        <BoundedNumber unit="轮" label="Peck Loop 每个任务最多几轮" min={5} max={1000} off={settings.roundsUnlimited} value={settings.maxRounds ?? 40} onCommit={(v) => onSettings({ maxRounds: v })} />
      </Row>
      <Row name="ONE SHOT 最多几轮">
        <BoundedNumber unit="轮" label="ONE SHOT 最多几轮" min={5} max={1000} value={settings.oneshotRounds ?? 12} onCommit={(v) => onSettings({ oneshotRounds: v })} />
      </Row>
      <Row name="额度提醒" off={settings.quotaWarn == null}>
        <Switch label="额度提醒" on={settings.quotaWarn != null} onChange={(on) => onSettings({ quotaWarn: on ? 85 : null })} />
        <BoundedNumber unit="%" label="额度提醒的百分比" min={50} max={100} off={settings.quotaWarn == null} value={settings.quotaWarn ?? 85} onCommit={(v) => onSettings({ quotaWarn: v })} />
      </Row>
    </div>
  );
};

// ---------- 权限 ----------

const HIGH_RISK = ["删文件", "推送和改 Git 历史", "写到项目外", "改系统设置", "看不出会做什么的命令"];

const Access: React.FC<Props> = ({ settings, onSettings, say }) => {
  const [rules, setRules] = useState<AllowRule[] | null>(null);
  useEffect(() => {
    let live = true;
    ipc.harnessAllowList().then((r) => { if (live) setRules(r); }).catch(say);
    return () => { live = false; };
  }, []);
  return (
    <>
      <div className="pk-ask-opts set-opts" role="radiogroup" aria-label="默认的权限档">
        {PERMISSIONS.map((p, i) => (
          <button key={p.id} type="button" className="pk-opt" role="radio" aria-checked={settings.permission === p.id} onClick={() => onSettings({ permission: p.id as Permission })}>
            <span className="pk-radio" />
            <span>
              <b className="pk-opt-name"><span className="t-label">{p.label}</span>{p.name}{p.id === "edit" && <em className="pk-tag pk-tag--rec">DEFAULT</em>}</b>
              <span className="pk-opt-desc">{p.does}</span>
            </span>
            <span className="pk-opt-key">{i + 1}</span>
          </button>
        ))}
      </div>
      <p className="lbl">ALWAYS ASK</p>
      <div className="chips">
        {HIGH_RISK.map((t) => <span key={t} className="chip-quiet"><Icon name="lock" size={12} />{t}</span>)}
      </div>
      <p className="lbl">ALWAYS ALLOWED</p>
      {rules && rules.length > 0 ? (
        <div className="set-rows">
          {rules.map((r) => (
            <Row key={`${r.workspaceId}:${r.rule}`} name={ruleWord(r.rule)} desc={<><span className="t-code">{r.workspace ?? "项目已经不在列表里"}</span> · {ago(r.createdAt)}</>}>
              <Btn sm v="danger" onClick={() => void ipc.harnessAllowRemove(r.workspaceId, r.rule).then((next) => { setRules(next); say("撤掉了，下次又会问"); }).catch(say)}>REMOVE</Btn>
            </Row>
          ))}
        </div>
      ) : (
        <p className="hint">{rules ? "还没有。" : "在读…"}</p>
      )}
    </>
  );
};

// ---------- CLI ----------

const Cli: React.FC<Props> = ({ onRefreshAgents, say }) => {
  const [engines, setEngines] = useState<ModelEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => ipc.gatewayModels().then((m) => setEngines(m.filter((e) => e.group === "cli")));
  useEffect(() => { load().catch(say); }, []);
  const recheck = (message = "重新检测过了") => {
    setBusy(true);
    void onRefreshAgents().then(load).then(() => say(message)).catch(say).finally(() => setBusy(false));
  };
  return (
    <>
      <SetHead label="ENGINES"><Btn sm icon="undo" disabled={busy} onClick={() => recheck()}>RECHECK</Btn></SetHead>
      <div className="set-rows">
        <CliDir say={say} onSaved={() => recheck("CLI 目录存好了，重新检测过了")} />
        {engines?.map((e) => <EngineRow key={e.id} entry={e} />)}
      </div>
      <CliAccounts say={say} />
    </>
  );
};

/** 一家 CLI：色点、名字、版本；在哪找到的。版本和录夹具时不一样不再标 CHANGED（站主 11-17：没必要一直显示）。 */
const EngineRow: React.FC<{ entry: ModelEntry }> = ({ entry: e }) => {
  const found = versionNumber(e.version);
  const desc = !e.available ? undefined : e.location ?? undefined;
  return (
    <Row key={e.id} name={<><Dot kind={engineDot(e.provider)} />{e.label}{found && <span className="t-code set-ver">{found}</span>}</>} off={!e.available} desc={desc}>
      {e.available ? <Tag v="ok">FOUND</Tag> : <Tag>NOT FOUND</Tag>}
    </Row>
  );
};

// ---------- 登录：Claude Code、Codex 只用和客户端同一个账号（站主 2026-11-13 定） ----------

const CliAccounts: React.FC<{ say: (m: unknown) => void }> = ({ say }) => {
  const [groups, setGroups] = useState<EngineAccounts[] | null>(null);
  const [checking, setChecking] = useState(false);
  const recheck = async () => {
    setChecking(true);
    try { setGroups(await ipc.cliAccountsList(true)); } catch (e) { say(e); } finally { setChecking(false); }
  };
  useEffect(() => {
    let live = true;
    ipc.cliAccountsList(false).then((g) => { if (live) setGroups(g); }).catch(say);
    // 只在打开时读一次（同 useGatewayConfig）。
    return () => { live = false; };
  }, []);
  if (!groups) return null;
  return (
    <>
      <SetHead label="LOGIN"><Btn sm icon="undo" disabled={checking} onClick={() => void recheck().then(() => say("登录状态都看过了"))}>{checking ? "CHECKING" : "CHECK ALL"}</Btn></SetHead>
      <div className="set-rows">
        {groups.flatMap((g) => g.accounts.map((a) => <AccountRow key={g.engine} engine={g.engine} engineLabel={g.label} note={g.defaultNote} account={a} say={say} />))}
      </div>
    </>
  );
};

const AccountRow: React.FC<{ engine: string; engineLabel: string; note: string; account: CliAccount; say: (m: unknown) => void }> = ({ engine, engineLabel, note, account, say }) => {
  const [status, setStatus] = useState<CliAccountStatus | null>(account.status ?? null);
  const [login, setLogin] = useState<CliLoginState | null>(null);
  const [busy, setBusy] = useState(false);
  const [code, setCode] = useState("");
  useEffect(() => { if (account.status) setStatus(account.status); }, [account.status]);
  // 登录中每秒看一次；结束了自动验一次权。
  useEffect(() => {
    if (!login?.running) return;
    const timer = window.setInterval(() => {
      ipc.cliAccountLoginState(engine, account.id).then(async (s) => {
        setLogin(s);
        if (!s.running) {
          const st = await ipc.cliAccountCheck(engine, account.id);
          setStatus(st);
          say(st.loggedIn ? `${engineLabel} 登录好了：记得让客户端也是这个账号` : `${engineLabel} 还没登录上，看看下面的输出`);
        }
      }).catch(say);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [login?.running]);
  const run = async (f: () => Promise<void>) => { setBusy(true); try { await f(); } catch (e) { say(e); } finally { setBusy(false); } };
  const check = () => run(async () => { setStatus(await ipc.cliAccountCheck(engine, account.id)); });
  const startLogin = () => run(async () => { setLogin(await ipc.cliAccountLogin(engine, account.id)); });
  const cancel = () => run(async () => { await ipc.cliAccountLoginCancel(engine, account.id); setLogin(null); });
  const copy = (url: string) => { void copyText(url).then(() => say("登录链接复制好了：到你要用的浏览器（或隐身窗口）里打开")).catch(say); };
  const tag = status == null ? <Tag>NOT CHECKED</Tag> : status.loggedIn ? <Tag v="ok">SIGNED IN</Tag> : <Tag v="high">SIGNED OUT</Tag>;
  const who = status?.loggedIn ? [status.email, status.plan].filter(Boolean).join(" · ") : status?.detail;
  const row = (
    <Row name={engineLabel} desc={[who, note].filter(Boolean).join(" · ") || undefined}>
      {tag}
      <Btn sm disabled={busy || !!login?.running} onClick={() => void startLogin()}>{status?.loggedIn ? "RELOGIN" : "LOGIN"}</Btn>
      <Btn sm disabled={busy} onClick={() => void check()}>CHECK</Btn>
    </Row>
  );
  if (!login) return row;
  return (
    <div className="set-block">
      {row}
      {login && (
        <div className="set-login">
          {login.url ? (
            <div className="set-inline">
              <span className="t-code set-cmd" title={login.url}>{login.url}</span>
              <Btn sm v="primary" onClick={() => copy(login.url!)}>COPY</Btn>
              {login.running && <Btn sm onClick={() => void cancel()}>CANCEL</Btn>}
            </div>
          ) : login.running ? <p className="hint">在等登录链接……</p> : null}
          {login.running && <p className="hint">把链接复制到你要用的浏览器（或另一个浏览器用户、隐身窗口）里打开、登录，登好以后这里自动验一次。{engine === "codex" ? "Codex 要输下面输出里的一次性代码。" : ""}</p>}
          {login.output.trim() && <pre className="t-code set-out">{login.output.trim().split("\n").slice(-8).join("\n")}</pre>}
          {login.running && engine === "claude-code" && (
            <div className="set-inline">
              <Field aria-label="粘回登录页给的 code" placeholder="登录页要你粘 code 时贴在这里（多半用不上）" value={code} onChange={(e) => setCode(e.target.value)} />
              <Btn sm disabled={!code.trim()} onClick={() => void run(async () => { await ipc.cliAccountLoginInput(engine, account.id, code); setCode(""); })}>SEND</Btn>
            </div>
          )}
          {!login.running && login.ok != null && <p className="hint">{login.ok ? "登录命令结束了。" : "登录没成功：看看上面的输出，再点一次 LOGIN。"}</p>}
        </div>
      )}
    </div>
  );
};

// ---------- Model Gateway 的设置：Vertex AI、CLI 目录 ----------

/** 读、改、存 Gateway 的设置。两处用：Vertex（供应商和 Key）、CLI 目录（CLI 检测）。 */
function useGatewayConfig(say: (m: unknown) => void) {
  const [saved, setSaved] = useState<GatewayConfig | null>(null);
  const [draft, setDraft] = useState<GatewayConfig | null>(null);
  const [adc, setAdc] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    ipc.gatewayConfigGet().then((s) => { if (live) { setSaved(s.config); setDraft(s.config); setAdc(s.adc); } }).catch(say);
    return () => { live = false; };
    // 只在打开时读一次：say 每次渲染可能是新的，跟着它重读会冲掉正在填的内容。
  }, []);
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(saved);
  const save = async (message: string, override?: GatewayConfig) => {
    const config = override ?? draft;
    if (!config) return;
    setBusy(true);
    try {
      const next = await ipc.gatewayConfigSet(config);
      setSaved(next); setDraft(next); say(message);
    } catch (e) { say(e); } finally { setBusy(false); }
  };
  return { saved, draft, setDraft, adc, busy, dirty, save };
}

/** Vertex AI（v1.1.6 并进 API KEYS，和各家 Key 一样的一块）：头一行 Location 和状态（先说缺的：没有 gcloud、没存 Project ID），
 *  下面一行 Project ID、SAVE（Project ID 和 Location 一起存）、REMOVE（清掉 Project ID）。 */
const Vertex: React.FC<{ say: (m: unknown) => void }> = ({ say }) => {
  const { saved, draft, setDraft, adc, busy, dirty, save } = useGatewayConfig(say);
  if (!draft) return null;
  const savedProject = !!saved?.vertex.projectId;
  const set = (vertex: Partial<GatewayConfig["vertex"]>) => setDraft({ ...draft, vertex: { ...draft.vertex, ...vertex } });
  return (
    <KeyBlock name="Vertex AI" off={!adc || !savedProject}
      option={<Field unit="LOCATION" aria-label="Vertex AI 的 Location" placeholder="global" value={draft.vertex.location} off={busy} onChange={(e) => set({ location: e.target.value })} />}
      tag={!adc ? <Tag>NO GCLOUD</Tag> : savedProject ? <Tag v="ok">SAVED</Tag> : <Tag>NO PROJECT ID</Tag>}
      field={<Field aria-label="Vertex AI 的 Project ID" placeholder="Project ID" value={draft.vertex.projectId} off={busy} onChange={(e) => set({ projectId: e.target.value })} />}
      save={<Btn sm disabled={busy || !dirty} onClick={() => void save("Vertex AI 的设置存好了")}>SAVE</Btn>}
      remove={<Btn sm v="danger" disabled={busy || !savedProject} onClick={() => saved && void save("Vertex AI 的 Project ID 清掉了", { ...saved, vertex: { ...saved.vertex, projectId: "" } })}>REMOVE</Btn>} />
  );
};

const CliDir: React.FC<{ say: (m: unknown) => void; onSaved: () => void }> = ({ say, onSaved }) => {
  const { draft, setDraft, busy, dirty, save } = useGatewayConfig(say);
  if (!draft) return null;
  return (
    <div className="set-block">
      <Row name="CLI 目录">{draft.cliDir ? <Tag v="ok">SET</Tag> : <Tag>PATH</Tag>}</Row>
      <div className="set-line set-line--save">
        <Field aria-label="CLI 目录" placeholder="~/…/cli/bin" value={draft.cliDir} off={busy} onChange={(e) => setDraft({ ...draft, cliDir: e.target.value })} />
        <Btn sm disabled={busy || !dirty} onClick={() => void save("CLI 目录存好了").then(onSaved)}>SAVE</Btn>
      </div>
    </div>
  );
};

// ---------- 外观 ----------

const EDITORS = ["Visual Studio Code", "Cursor", "Zed", "Xcode", "Sublime Text", "Nova", "TextEdit"];

const Look: React.FC<Props> = ({ settings, onSettings }) => (
  <div className="set-rows">
    <Row name="明暗">
      <Seg<ThemeChoice> label="明暗" sm items={[{ id: "system", label: "SYSTEM" }, { id: "dark", label: "DARK" }, { id: "light", label: "LIGHT" }]} value={settings.theme} onChange={(theme) => onSettings({ theme })} />
    </Row>
    <Row name="编辑器">
      <ComboField label="编辑器" placeholder="系统默认" emptyLabel="系统默认" options={EDITORS} value={settings.editorApp} onChange={(editorApp) => onSettings({ editorApp })} />
    </Row>
    <Row name="App 图标">
      <Seg<"j2" | "a3"> label="App 图标" sm items={[{ id: "j2", label: "J2" }, { id: "a3", label: "A3" }]} value={settings.appIcon} onChange={(appIcon) => onSettings({ appIcon })} />
    </Row>
  </div>
);

// ---------- MCP 服务器（v0.8；v1.1.0 起在 PLUGINS 页的 MCP SERVERS 一块） ----------

/** 参数：空格分开，带空格的用引号包起来。 */
function splitArgs(text: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}
const joinArgs = (args: string[]) => args.map((a) => (/\s/.test(a) ? `"${a}"` : a)).join(" ");

const EMPTY_SERVER: McpServerConfig = { name: "", command: "", args: [], env: [], enabled: true };

const Mcp: React.FC<Props> = ({ say }) => {
  const [list, setList] = useState<McpServerConfig[] | null>(null);
  const [draft, setDraft] = useState<{ server: McpServerConfig; args: string; env: string; index: number } | null>(null);
  const [tools, setTools] = useState<Record<string, McpToolInfo[] | string>>({});
  const [busy, setBusy] = useState(false);
  useEffect(() => { ipc.mcpServersGet().then(setList).catch(say); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const save = async (next: McpServerConfig[], message: string) => {
    setBusy(true);
    try { setList(await ipc.mcpServersSet(next)); setDraft(null); say(message); } catch (e) { say(e); } finally { setBusy(false); }
  };
  const test = async (server: McpServerConfig) => {
    setTools((t) => ({ ...t, [server.name]: "在起…" }));
    try { const found = await ipc.mcpServerTest(server); setTools((t) => ({ ...t, [server.name]: found })); }
    catch (e) { setTools((t) => ({ ...t, [server.name]: String(e) })); }
  };
  const edit = (server: McpServerConfig, index: number) => setDraft({ server: { ...server }, args: joinArgs(server.args), env: server.env.map((v) => `${v.name}=${v.value}`).join("\n"), index });
  const apply = () => {
    if (!draft || !list) return;
    const env = draft.env.split("\n").map((l) => l.trim()).filter(Boolean).map((l) => { const i = l.indexOf("="); return { name: (i < 0 ? l : l.slice(0, i)).trim(), value: i < 0 ? "" : l.slice(i + 1) }; });
    const server = { ...draft.server, args: splitArgs(draft.args), env };
    const next = draft.index < 0 ? [...list, server] : list.map((s, i) => (i === draft.index ? server : s));
    void save(next, draft.index < 0 ? `加上了 ${server.name}，下一个任务起` : `${server.name} 存好了，下一个任务生效`);
  };
  return (
    <>
      <SetHead label="MCP SERVERS">
        {!draft && <Btn sm icon="plus" disabled={busy || !list} onClick={() => setDraft({ server: { ...EMPTY_SERVER }, args: "", env: "", index: -1 })}>ADD SERVER</Btn>}
      </SetHead>
      <div className="set-rows set-rows--acts">
        {list?.map((server, i) => {
          const found = tools[server.name];
          return (
            <div key={server.name} className="set-block">
              <Row name={<span className="t-code">{server.name}</span>} off={!server.enabled}
                desc={<span className="t-code set-cmd" title={[server.command, ...server.args].join(" ")}>{[server.command.split("/").pop(), ...server.args].join(" ")}</span>}>
                <Switch label={`${server.name} 开着`} on={server.enabled} disabled={busy} onChange={(enabled) => void save(list.map((x, j) => (j === i ? { ...x, enabled } : x)), enabled ? `${server.name} 开了` : `${server.name} 关了`)} />
                <Btn sm disabled={busy} onClick={() => void test(server)}>TEST</Btn>
                <Btn sm disabled={busy} onClick={() => edit(server, i)}>EDIT</Btn>
                <Btn sm v="danger" disabled={busy} onClick={() => void save(list.filter((_, j) => j !== i), `拿掉了 ${server.name}`)}>REMOVE</Btn>
              </Row>
              {found && (
                typeof found === "string" ? <p className="hint">{found}</p> : (
                  <div className="chips">
                    {found.length === 0 && <span className="hint">没有工具。</span>}
                    {found.map((t) => <span key={t.name} className="chip-quiet" title={t.description}>{t.readOnly ? <Icon name="lock" size={12} /> : null}{t.name}{t.destructive ? " · 会删改" : ""}</span>)}
                  </div>
                )
              )}
            </div>
          );
        })}
        {list && list.length === 0 && !draft && <p className="hint">还没有。</p>}
        {!list && <p className="hint">在读…</p>}
      </div>
      {draft ? (
        <div className="pk-mod pk-mod--sunk set-editor">
          <p className="lbl">{draft.index < 0 ? "ADD SERVER" : "EDIT SERVER"}</p>
          <Row name="名字"><Field aria-label="MCP 服务器的名字" placeholder="小写字母、数字、- 和 _" value={draft.server.name} onChange={(e) => setDraft({ ...draft, server: { ...draft.server, name: e.target.value } })} /></Row>
          <Row name="命令"><Field aria-label="命令" placeholder="npx" value={draft.server.command} onChange={(e) => setDraft({ ...draft, server: { ...draft.server, command: e.target.value } })} /></Row>
          <Row name="参数"><Field aria-label="参数" placeholder="-y @modelcontextprotocol/server-filesystem ." value={draft.args} onChange={(e) => setDraft({ ...draft, args: e.target.value })} /></Row>
          <Row name="环境变量">
            <textarea className="t-code sel set-env" aria-label="环境变量" placeholder="NAME=value" rows={3} spellCheck={false} value={draft.env} onChange={(e) => setDraft({ ...draft, env: e.target.value })} />
          </Row>
          {draft.index >= 0 && <McpSecrets server={list?.[draft.index] ?? draft.server} onSaved={(next) => { setList(next); const s = next[draft.index]; if (s) setDraft({ ...draft, server: { ...draft.server, secretEnv: s.secretEnv } }); }} say={say} />}
          <div className="set-inline">
            <Btn v="primary" disabled={busy || !draft.server.name.trim() || !draft.server.command.trim()} onClick={apply}>SAVE</Btn>
            <Btn v="ghost" onClick={() => setDraft(null)}>CANCEL</Btn>
          </div>
        </div>
      ) : null}
    </>
  );
};

/** 一台 MCP 服务器放在钥匙串里的环境变量（v0.10）：只列名字；改值就再存一次，REMOVE 从钥匙串删掉。
 *  v1.1.6：一块两行，存着的一行一个（名字、REMOVE），最下面一行变量名、值、SAVE。 */
const McpSecrets: React.FC<{ server: McpServerConfig; onSaved: (next: McpServerConfig[]) => void; say: (m: unknown) => void }> = ({ server, onSaved, say }) => {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const set = async (n: string, v: string | null, message: string) => {
    try { onSaved(await ipc.mcpSecretSet(server.name, n, v)); setName(""); setValue(""); say(message); } catch (e) { say(e); }
  };
  return (
    <div className="set-block">
      <Row name="钥匙串" />
      {(server.secretEnv ?? []).map((n) => (
        <div key={n} className="set-line set-line--pair">
          <span className="set-code">{n}=••••</span>
          <Btn sm v="danger" onClick={() => void set(n, null, `${n} 从钥匙串删了`)}>REMOVE</Btn>
        </div>
      ))}
      <div className="set-line set-line--pair">
        <Field aria-label="钥匙串里的变量名" placeholder="GITHUB_TOKEN" value={name} onChange={(e) => setName(e.target.value)} />
        <Field type="password" aria-label="变量的值" placeholder="值" value={value} onChange={(e) => setValue(e.target.value)} />
        <Btn sm disabled={!name.trim() || !value.trim()} onClick={() => void set(name.trim(), value, `${name.trim()} 存进钥匙串了`)}>SAVE</Btn>
      </div>
    </div>
  );
};

// ---------- 键盘（v0.8） ----------

const KEYS: Array<[string, string]> = [
  ["⌘N", "这个项目里新开一个任务"],
  ["⌘L", "到输入框"],
  ["↵ / ⇧↵", "发出去 / 换行"],
  ["⌘.", "停下在跑的任务"],
  ["⌘[ / ⌘]", "上一个 / 下一个任务"],
  ["⌘1 – ⌘4", "右栏：FILES、ONE SHOT、PROMPT、TERM"],
  ["⌘,", "设置（再按一次关上）"],
  ["Esc", "关菜单和弹层；收回宽栏；关设置、项目设置"],
  ["↑ ↓ ↵", "@ 旧任务的弹层里选"],
];

const Keys: React.FC = () => (
  <div className="set-rows">
    {KEYS.map(([k, what]) => (
      <Row key={k} name={<span className="t-code set-key">{k}</span>} desc={what} />
    ))}
  </div>
);

// ---------- AGENTS：PLOOP 的主角色和四类子代理（v1.1.2；每个模型的思考强度也在这里调） ----------

const AgentsSettings: React.FC<Props> = ({ settings, onSettings, entries }) => {
  const apis = ploopable(entries);
  const main = ploopMain(entries, settings);
  const effort = (e: ModelEntry | null) => (
    <EffortPicker entry={e} value={e ? settings.efforts[e.id] ?? defaultEffort(e) : ""} up={false} onChange={(v) => e && onSettings({ efforts: { ...settings.efforts, [e.id]: v } })} />
  );
  const patch = (kind: SubagentKind, next: Partial<SubagentSettings>) => onSettings({ subagents: { ...settings.subagents, [kind]: { ...settings.subagents[kind], ...next } } });
  // 一块两行（v1.1.6）：头一行名字、做什么、开关；下面一行四格——模型、思考强度、最多几轮、同时几个，几块的格子对齐（没有的那格空着）。
  return (
    <>
      <SetHead label="MAIN" />
      <div className="set-rows">
        <div className="set-block">
          <Row name="主角色" />
          <div className="set-line set-line--agent">
            <ModelChip label="PLOOP 的主角色" entries={apis} value={main?.id ?? ""} onChange={(id) => onSettings({ ploop: { ...settings.ploop, main: id } })} />
            <span>{effort(main)}</span>
          </div>
        </div>
      </div>
      <SetHead label="SUBAGENTS" />
      <div className="set-rows">
        {SUBAGENT_KINDS.map((k) => {
          const s = settings.subagents[k.id] ?? {};
          const chosen = apis.find((e) => e.id === s.model) ?? null;
          return (
            <div key={k.id} className="set-block">
              <Row name={k.id} off={!!s.off} desc={k.does}>
                <Switch label={`${k.id} 开着`} on={!s.off} onChange={(on) => patch(k.id, { off: !on })} />
              </Row>
              <div className="set-line set-line--agent">
                <ModelChip label={`${k.id} 用哪个模型`} entries={apis} value={chosen?.id ?? ""} off="跟主角色" disabled={!!s.off} onChange={(id) => patch(k.id, { model: id })} />
                <span>{effort(chosen ?? main)}</span>
                <BoundedNumber unit="轮" label={`${k.id} 最多几轮`} min={5} max={1000} off={!!s.off} value={s.rounds ?? k.rounds} onCommit={(v) => patch(k.id, { rounds: v })} />
                <span>{k.parallel !== null && <BoundedNumber unit="个同时" label={`${k.id} 同时最多几个`} min={1} max={8} off={!!s.off} value={s.parallel ?? k.parallel} onCommit={(v) => patch(k.id, { parallel: v })} />}</span>
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
};

// ---------- QUOTA：API 价格表（站主 11-17）：改价、刷新、汇率 ----------

type PriceDraft = { isNew: boolean; model: string; label: string; vendor: string; currency: PriceRow["currency"]; input: string; output: string; cacheRead: string; cacheWrite: string; peak: string; tiers: PriceRow["tiers"] };

const num = (v: number | null | undefined) => (v == null ? "" : String(+v.toFixed(6)));
const kTokens = (n: number) => (n >= 1e6 ? `${+(n / 1e6).toFixed(2)}M` : `${Math.round(n / 1000)}K`);

/** 一行价格：¥ 进 1 · 出 4 · 缓存命中 0.02 · 高峰 ×2。 */
function priceText(r: PriceRow): string {
  const sym = r.currency === "CNY" ? "¥" : "$";
  const parts = [`进 ${num(r.input)}`, `出 ${num(r.output)}`];
  if (r.cacheRead != null) parts.push(`缓存命中 ${num(r.cacheRead)}`);
  if (r.cacheWrite != null) parts.push(`缓存写入 ${num(r.cacheWrite)}`);
  for (const t of r.tiers ?? []) parts.push(`超 ${kTokens(t.aboveTokens)} 进 ${num(t.input)} 出 ${num(t.output)}`);
  if (r.peak) parts.push(`高峰 ×${num(r.peak)}`);
  return `${sym} ${parts.join(" · ")}`;
}

const QuotaSettings: React.FC<{ say: Props["say"] }> = ({ say }) => {
  const [book, setBook] = useState<PriceBook | null>(null);
  const [draft, setDraft] = useState<PriceDraft | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { ipc.pricesGet().then(setBook).catch(say); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const run = async (fn: () => Promise<PriceBook>, message: string) => {
    setBusy(true);
    try { setBook(await fn()); say(message); return true; } catch (e) { say(e); return false; } finally { setBusy(false); }
  };
  const refresh = async () => {
    setBusy(true);
    try {
      const r = await ipc.pricesRefresh();
      setBook(r.book);
      const { changed } = r.report;
      say(changed.length ? `刷新好了：${changed.length} 个变了（${changed.slice(0, 3).join("、")}${changed.length > 3 ? " 等" : ""}）` : "刷新好了：美元价都没变");
    } catch (e) { say(e); } finally { setBusy(false); }
  };
  const edit = (r: PriceView) => setDraft({ isNew: false, model: r.model, label: r.label, vendor: r.vendor, currency: r.currency, input: num(r.input), output: num(r.output), cacheRead: num(r.cacheRead), cacheWrite: num(r.cacheWrite), peak: num(r.peak), tiers: r.tiers });
  const saveDraft = async () => {
    if (!draft) return;
    const n = (s: string) => (s.trim() === "" ? null : Number(s));
    const [input, output] = [n(draft.input), n(draft.output)];
    if (input == null || output == null || [input, output, n(draft.cacheRead), n(draft.cacheWrite), n(draft.peak)].some((v) => v != null && !Number.isFinite(v))) { say("进、出要填数字"); return; }
    const row: PriceRow = { model: draft.model.trim(), label: draft.label.trim(), vendor: draft.vendor, currency: draft.currency, input, output, cacheRead: n(draft.cacheRead), cacheWrite: n(draft.cacheWrite), tiers: draft.tiers ?? [], peak: n(draft.peak) };
    if (await run(() => ipc.pricesSet({ row }), `${row.label || row.model} 的价格存好了，下一次请求起按它算`)) setDraft(null);
  };
  if (!book) return <p className="hint">在读…</p>;
  const vendorIds = new Set(book.vendors.map((v) => v.id));
  const groups = [...book.vendors, { id: "other", label: "Other" }]
    .map((v) => ({ v, rows: book.rows.filter((r) => (v.id === "other" ? !vendorIds.has(r.vendor) : r.vendor === v.id)) }))
    .filter((g) => g.rows.length);
  const set = (patch: Partial<PriceDraft>) => draft && setDraft({ ...draft, ...patch });
  const field = (k: "input" | "output" | "cacheRead" | "cacheWrite" | "peak", name: string) => draft && (
    <div className="set-price-f">
      <span className="hint">{name}</span>
      <Field type="number" min={0} step="any" aria-label={name} value={draft[k]} onChange={(e) => set({ [k]: e.target.value })} />
    </div>
  );
  const editor = draft && (
    <div className="pk-mod pk-mod--sunk set-editor">
      <p className="lbl">{draft.isNew ? "ADD MODEL" : draft.label}</p>
      {draft.isNew && (
        <>
          <Row name="模型名"><Field aria-label="模型名" placeholder="gpt-6-luna" value={draft.model} onChange={(e) => set({ model: e.target.value })} /></Row>
          <Row name="显示名"><Field aria-label="显示名" placeholder="GPT-6 Luna" value={draft.label} onChange={(e) => set({ label: e.target.value })} /></Row>
          <Row name="哪家">
            <MenuChip sm up={false} ariaLabel="哪家" label={(book.vendors.find((v) => v.id === draft.vendor)?.label ?? "Other").toUpperCase()}>
              {(close) => [...book.vendors, { id: "other", label: "Other" }].map((v) => (
                <button key={v.id} type="button" role="menuitem" className={cn("pk-menu-item", v.id === draft.vendor && "is-on")} onClick={() => { set({ vendor: v.id, currency: v.id === "deepseek" || v.id === "aliyun" ? "CNY" : draft.currency }); close(); }}>
                  <Dot kind="" /><b>{v.label.toUpperCase()}</b><span />
                </button>
              ))}
            </MenuChip>
          </Row>
          <Row name="币种"><Seg<PriceRow["currency"]> label="币种" sm items={[{ id: "USD", label: "USD" }, { id: "CNY", label: "CNY" }]} value={draft.currency} onChange={(currency) => set({ currency })} /></Row>
        </>
      )}
      <div className="set-price-grid">
        {field("input", "进")}
        {field("output", "出")}
        {field("cacheRead", "缓存命中")}
        {field("cacheWrite", "缓存写入")}
        {(draft.currency === "CNY" || draft.peak !== "") && field("peak", "高峰倍数")}
      </div>
      <div className="set-inline">
        <Btn v="primary" disabled={busy || !draft.model.trim() || draft.input === "" || draft.output === ""} onClick={() => void saveDraft()}>SAVE</Btn>
        <Btn v="ghost" onClick={() => setDraft(null)}>CANCEL</Btn>
      </div>
    </div>
  );
  return (
    <>
      <div className="set-rows">
        <Row name="汇率" desc={book.fxRate !== book.defaultFx ? `默认 ${book.defaultFx}` : undefined}>
          <BoundedNumber unit="CNY / USD" decimals={3} min={0.2} max={99} label="汇率：1 美元换多少人民币" value={book.fxRate} onCommit={(fxRate) => void run(() => ipc.pricesSet({ fxRate }), "汇率存好了")} />
        </Row>
        <Row name="价格表" desc={`内置 ${book.builtinDate}${book.refreshedAt ? ` · ${ago(book.refreshedAt)}刷新过` : ""}`}>
          <Btn sm icon="plus" disabled={busy || !!draft} onClick={() => setDraft({ isNew: true, model: "", label: "", vendor: "other", currency: "USD", input: "", output: "", cacheRead: "", cacheWrite: "", peak: "", tiers: [] })}>ADD MODEL</Btn>
          <Btn sm icon="undo" disabled={busy} onClick={() => void refresh()}>{busy ? "REFRESHING" : "REFRESH"}</Btn>
        </Row>
      </div>
      {draft?.isNew && editor}
      {groups.map(({ v, rows }) => (
        <React.Fragment key={v.id}>
          <p className="lbl">{v.label.toUpperCase()}<em className="menu-group-note">每百万 token</em></p>
          <div className="set-rows set-rows--acts">
            {rows.map((r) => (draft && !draft.isNew && draft.model === r.model ? <React.Fragment key={r.model}>{editor}</React.Fragment> : (
              <Row key={r.model} name={r.label} desc={<span className="t-code" title={priceText(r)}>{priceText(r)}</span>}>
                {r.origin === "edited" && <Tag v="wait">EDITED</Tag>}
                {r.origin === "online" && <Tag v="ok">UPDATED</Tag>}
                {r.origin === "custom" && <Tag>ADDED</Tag>}
                {r.origin === "edited" && <Btn sm disabled={busy} onClick={() => void run(() => ipc.pricesSet({ reset: r.model }), `${r.label} 回到内置的价格`)}>RESET</Btn>}
                {r.origin === "custom" && <Btn sm v="danger" disabled={busy} onClick={() => void run(() => ipc.pricesSet({ reset: r.model }), `删掉了 ${r.label}`)}>DELETE</Btn>}
                <Btn sm disabled={busy} onClick={() => edit(r)}>EDIT</Btn>
              </Row>
            )))}
          </div>
        </React.Fragment>
      ))}
    </>
  );
};
