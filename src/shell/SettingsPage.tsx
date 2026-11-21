import React, { useEffect, useState } from "react";
import { api, type ApiConfig, type ApiMode, type ApiStatus } from "@/lib/api";
import { ipc, isMockMode, type AllowRule, type McpServerConfig, type McpToolInfo, type PriceBook, type PriceRow, type PriceView } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import type { ThemeChoice } from "@/lib/theme";
import type { CliAccount, CliAccountStatus, CliLoginState, EngineAccounts, GatewayConfig, ModelEntry } from "@/lib/agentEvents";
import type { Permission, ProjectOverview } from "@/lib/types";
import { BUILTIN_PRESETS, RELAY, applyPreset, canDelegate, currentSetup, presetProblems, presetSummary, type Preset } from "@/lib/presets";
import { PERMISSIONS, ago, cn, copyText, engineDot, plainLine, ruleWord, shortenHomePath, versionNumber } from "@/lib/utils";
import { Btn, ComboField, Dot, Field, Icon, MenuChip, ModHead, Row, Seg, Switch } from "@/ui";
import { ModelChip } from "@/shell/AgentsPanel";
import { pickable, teamable } from "@/shell/ModelPicker";
import { chosenEntry } from "@/shell/MiddleColumn";

/** 设置的分组（v0.10 Refine：只写英文；DEV、NOTIFY、LIMIT 并成 GENERAL，新加 AGENTS、QUOTA）。 */
export const SETTINGS_NAV = ["GENERAL", "PROVIDERS", "AGENTS", "QUOTA", "ACCESS", "CLI", "LOOK", "MCP", "ARCHIVE", "KEYS"] as const;
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
}

const Tag: React.FC<{ v?: "ok" | "wait" | "high"; children: React.ReactNode }> = ({ v, children }) => <em className={cn("pk-tag", v && `pk-tag--${v}`)}>{children}</em>;

/** 设置：一个面板，占中栏和右栏合成的宽栏；改了马上生效（Key、Vertex、价格这类要点 SAVE）。
 *  v0.10 Refine（站主 11-17）：分组只写英文、不放 ⓘ 和说明文字。 */
export const SettingsPage: React.FC<Props> = (props) => {
  const [section, setSection] = useState<SettingsSection>(props.initial ?? "GENERAL");
  return (
    <section className="col page" aria-label="设置">
      <div className="pk-mod col-grow page-mod">
        <ModHead label="SETTINGS"><Btn v="ghost" sm icon="x" onClick={props.onClose}>CLOSE</Btn></ModHead>
        <div className="set-grid">
          <nav className="set-nav" aria-label="设置分组">
            {SETTINGS_NAV.map((en) => (
              <button key={en} type="button" className={en === section ? "is-on" : undefined} aria-current={en === section} onClick={() => setSection(en)}>
                <b className="t-button">{en}</b>
              </button>
            ))}
          </nav>
          <div className="set-pane scroll" key={section}>
            {section === "GENERAL" && <General {...props} />}
            {section === "PROVIDERS" && <Providers {...props} />}
            {section === "AGENTS" && <AgentsSettings {...props} />}
            {section === "QUOTA" && <QuotaSettings say={props.say} />}
            {section === "ACCESS" && <Access {...props} />}
            {section === "CLI" && <Cli {...props} />}
            {section === "LOOK" && <Look {...props} />}
            {section === "MCP" && <Mcp {...props} />}
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
          <div className="set-rows">
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

// ---------- 供应商和 Key ----------

const KEY_STATE: Record<string, string | undefined> = { unloaded: "在读钥匙串", unavailable: "钥匙串没授权" };

/** 旧 API 层留下的设置（现在只用 Aliyun 的地域和业务空间）：改了要点 SAVE；存的时候整份一起存。 */
function useApiConfig(apiStatus: ApiStatus | null, onRefreshApi: Props["onRefreshApi"], say: Props["say"]) {
  const [config, setConfig] = useState<ApiConfig | null>(apiStatus?.config ?? null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (!dirty) setConfig(apiStatus?.config ?? null); }, [apiStatus, dirty]);
  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try { await fn(); } catch (e) { say(e); } finally { setBusy(false); }
  };
  const change = (next: ApiConfig) => { setConfig(next); setDirty(true); };
  const save = (message: string) => act(async () => { if (!config) return; await api.saveConfig(config); setDirty(false); await onRefreshApi(); say(message); });
  return { config, change, dirty, busy, act, save };
}

const Providers: React.FC<Props> = ({ apiStatus, onRefreshApi, say }) => {
  const { config, change, dirty, busy, act, save } = useApiConfig(apiStatus, onRefreshApi, say);
  const [keys, setKeys] = useState<Record<string, string>>({});
  if (!apiStatus || !config) return <p className="hint">在读设置…</p>;
  return (
    <>
      <div className="set-rows">
        {apiStatus.providers.map((p) => {
          const state = apiStatus.keys.find((k) => k.providerId === p.id)?.state ?? "missing";
          return (
            <div key={p.id} className="set-block">
              <Row name={p.name} off={state !== "saved"} desc={KEY_STATE[state]}>
                {state === "saved" ? <Tag v="ok">SAVED</Tag> : <Tag>{state === "unloaded" || state === "unavailable" ? "NOT READ" : "NO KEY"}</Tag>}
              </Row>
              {/* 网格：Key 框和下面业务空间 ID 框的右边对齐（站主 11-17）。 */}
              <div className="set-keys">
                <Field className="set-keys-wide" type="password" aria-label={`${p.name} 的 Key`} placeholder={`粘贴 ${p.name} 的 Key`} value={keys[p.id] ?? ""} off={busy} onChange={(e) => setKeys((k) => ({ ...k, [p.id]: e.target.value }))} />
                <Btn sm disabled={busy || !keys[p.id]?.trim()} onClick={() => void act(async () => { await api.setKey(p.id, keys[p.id]); setKeys((k) => ({ ...k, [p.id]: "" })); await onRefreshApi(); say(apiStatus.mode === "mock" ? "假 Key 只存在内存里，没进钥匙串" : "Key 已保存到钥匙串；认证通过要发一次请求才知道"); })}>SAVE</Btn>
                <Btn sm v="danger" disabled={busy || state !== "saved"} onClick={() => void act(async () => { await api.setKey(p.id, null); await onRefreshApi(); })}>REMOVE</Btn>
                {p.id === "aliyun" && (
                  <>
                    <Seg label="Aliyun 的地域" sm items={[{ id: "beijing", label: "BEIJING" }, { id: "singapore", label: "SINGAPORE" }]} value={config.aliyunRegion} onChange={(aliyunRegion) => change({ ...config, aliyunRegion })} />
                    <Field aria-label="Aliyun 的业务空间 ID" placeholder="业务空间 ID（可以不填）" value={config.aliyunWorkspaceId} onChange={(e) => change({ ...config, aliyunWorkspaceId: e.target.value.trim() })} />
                    <Btn sm disabled={busy || !dirty} onClick={() => void save("Aliyun 的地域存好了")}>SAVE</Btn>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>
      <div className="set-inline"><Btn sm disabled={busy} onClick={() => void act(async () => { await api.loadKeys(); await onRefreshApi(); })}>READ KEYS</Btn></div>
      <Vertex say={say} />
    </>
  );
};

// ---------- GENERAL：开发者选项 → 通知 → 花费上限（站主 11-17 并成一组） ----------

/** 这个月已经花了多少（Peck 循环）：一条 20 格的进度（到 80% 变琥珀、到上限变红），右边写「花了 / 上限」。 */
const SpendMeter: React.FC<{ cap: number | null }> = ({ cap }) => {
  const [spent, setSpent] = useState<number | null>(null);
  useEffect(() => { void ipc.harnessSpend().then((s) => setSpent(s.monthUsd)).catch(() => setSpent(null)); }, []);
  const used = spent == null ? "—" : `$${spent.toFixed(spent < 1 ? 4 : 2)}`;
  const part = cap && spent != null ? spent / cap : 0;
  const lit = Math.min(20, Math.round(part * 20));
  return (
    <>
      <div className={cn("q-meter set-meter", part >= 1 ? "is-bad" : part >= 0.8 && "is-warn")} role="meter" aria-valuenow={Math.round(part * 100)} aria-valuemin={0} aria-valuemax={100} aria-label={cap ? `这个月用了上限的 ${Math.round(part * 100)}%` : "没设上限"}>
        {Array.from({ length: 20 }, (_, i) => <i key={i} className={cn(i < lit && "is-on")} />)}
      </div>
      <span className="set-meter-text t-code">{used} / {cap ? `$${cap}` : "∞"}</span>
    </>
  );
};

/** 数字那几行的控件：前面的单位、数字框、后面的单位各占固定的宽，几行的框就对齐了。 */
const Num: React.FC<{ pre?: string; post?: string; children: React.ReactNode }> = ({ pre, post, children }) => (
  <span className="set-num"><span className="set-unit">{pre}</span>{children}<span className="set-unit">{post}</span></span>
);
/** GENERAL 里三个数字框一样宽。 */
const NUM_WIDTH = 120;

/** 有上下限的数字框：打字时照原样留着，离开或回车才夹到范围里存（以前每敲一个字就夹，打不出 120；v0.10 收尾审查）。
 *  `decimals` 给了就能写小数（汇率）。 */
const BoundedNumber: React.FC<{ value: number; min: number; max: number; width: number; label: string; decimals?: number; off?: boolean; onCommit: (v: number) => void }> = ({ value, min, max, width, label, decimals, off, onCommit }) => {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft == null) return;
    const v = Number(draft);
    const round = (x: number) => (decimals ? Math.round(x * 10 ** decimals) / 10 ** decimals : Math.round(x));
    if (draft.trim() !== "" && Number.isFinite(v)) onCommit(Math.min(max, Math.max(min, round(v))));
    setDraft(null);
  };
  return (
    <Field width={width} off={off} type="number" aria-label={label} min={min} max={max} step={decimals ? 1 / 10 ** decimals : 1} value={draft ?? String(value)}
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
        <Num pre="$"><Field width={NUM_WIDTH} type="number" aria-label="每月花费上限（美元）" min={0} value={settings.costCap ?? ""} onChange={(e) => onSettings({ costCap: e.target.value === "" ? null : Math.max(0, Number(e.target.value)) })} /></Num>
      </Row>
      <Row name="这个月花了"><SpendMeter cap={settings.costCap} /></Row>
      <Row name="每个任务最多几轮">
        <Num post="轮"><BoundedNumber width={NUM_WIDTH} label="Peck 循环每个任务最多几轮" min={5} max={400} value={settings.maxRounds ?? 40} onCommit={(v) => onSettings({ maxRounds: v })} /></Num>
      </Row>
      <Row name="额度提醒" off={settings.quotaWarn == null}>
        <Switch label="额度提醒" on={settings.quotaWarn != null} onChange={(on) => onSettings({ quotaWarn: on ? 85 : null })} />
        <Num post="%"><BoundedNumber width={NUM_WIDTH} label="额度提醒的百分比" min={50} max={100} off={settings.quotaWarn == null} value={settings.quotaWarn ?? 85} onCommit={(v) => onSettings({ quotaWarn: v })} /></Num>
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
      <div className="set-rows">
        <CliDir say={say} onSaved={() => recheck("CLI 目录存好了，重新检测过了")} />
        {engines?.map((e) => <EngineRow key={e.id} entry={e} />)}
      </div>
      <div className="set-inline"><Btn icon="undo" disabled={busy} onClick={() => recheck()}>RECHECK</Btn></div>
      <CliAccounts say={say} />
    </>
  );
};

/** 一家 CLI：色点、名字、版本；在哪找到的。版本和录夹具时不一样不再标 CHANGED（站主 11-17：没必要一直显示）。 */
const EngineRow: React.FC<{ entry: ModelEntry }> = ({ entry: e }) => {
  const found = versionNumber(e.version);
  const desc = !e.available ? undefined : [e.location ?? null, e.directOnly ? "只做直接调用" : null].filter(Boolean).join(" · ") || undefined;
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
      <p className="lbl">LOGIN</p>
      <div className="set-rows">
        {groups.flatMap((g) => g.accounts.map((a) => <AccountRow key={g.engine} engine={g.engine} engineLabel={g.label} note={g.defaultNote} account={a} say={say} />))}
      </div>
      <div className="set-inline"><Btn icon="undo" disabled={checking} onClick={() => void recheck().then(() => say("登录状态都看过了"))}>{checking ? "CHECKING" : "CHECK ALL"}</Btn></div>
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
  return (
    <div className="set-block">
      <Row name={engineLabel} desc={[who, note].filter(Boolean).join(" · ") || undefined}>
        {tag}
        <Btn sm disabled={busy || !!login?.running} onClick={() => void startLogin()}>{status?.loggedIn ? "RELOGIN" : "LOGIN"}</Btn>
        <Btn sm disabled={busy} onClick={() => void check()}>CHECK</Btn>
      </Row>
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
  const save = async (message: string) => {
    if (!draft) return;
    setBusy(true);
    try {
      const next = await ipc.gatewayConfigSet(draft);
      setSaved(next); setDraft(next); say(message);
    } catch (e) { say(e); } finally { setBusy(false); }
  };
  return { saved, draft, setDraft, adc, busy, dirty, save };
}

const Vertex: React.FC<{ say: (m: unknown) => void }> = ({ say }) => {
  const { saved, draft, setDraft, adc, busy, dirty, save } = useGatewayConfig(say);
  if (!draft) return null;
  const project = draft.vertex.projectId;
  // 存过 Project ID 才算（站主 11-17：GCLOUD OK 右边一个方框，NEED PROJECT ID / PROJECT ID SAVED）。
  const savedProject = !!saved?.vertex.projectId;
  const set = (vertex: Partial<GatewayConfig["vertex"]>) => setDraft({ ...draft, vertex: { ...draft.vertex, ...vertex } });
  return (
    <>
      <p className="lbl">VERTEX AI</p>
      <div className="set-rows">
        <div className="set-block">
          <Row name="Claude 和 Gemini" off={!adc || !savedProject}>
            {adc ? <Tag v="ok">GCLOUD OK</Tag> : <Tag>NO GCLOUD</Tag>}
            {savedProject ? <Tag v="ok">PROJECT ID SAVED</Tag> : <Tag>NEED PROJECT ID</Tag>}
          </Row>
          <div className="set-inline">
            <Field aria-label="Vertex AI 的 Project ID" placeholder="Project ID" value={project} off={busy} onChange={(e) => set({ projectId: e.target.value })} />
            <Field width={150} aria-label="Vertex AI 的 Location" placeholder="global" value={draft.vertex.location} off={busy} onChange={(e) => set({ location: e.target.value })} />
            <Btn sm disabled={busy || !dirty} onClick={() => void save("Vertex AI 的设置存好了")}>SAVE</Btn>
          </div>
        </div>
      </div>
    </>
  );
};

const CliDir: React.FC<{ say: (m: unknown) => void; onSaved: () => void }> = ({ say, onSaved }) => {
  const { draft, setDraft, busy, dirty, save } = useGatewayConfig(say);
  if (!draft) return null;
  return (
    <div className="set-block">
      <Row name="CLI 目录">{draft.cliDir ? <Tag v="ok">SET</Tag> : <Tag>PATH</Tag>}</Row>
      <div className="set-inline">
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
      <ComboField width={240} label="编辑器" placeholder="系统默认" emptyLabel="系统默认" options={EDITORS} value={settings.editorApp} onChange={(editorApp) => onSettings({ editorApp })} />
    </Row>
    <Row name="App 图标">
      <Seg<"j2" | "a3"> label="App 图标" sm items={[{ id: "j2", label: "J2" }, { id: "a3", label: "A3" }]} value={settings.appIcon} onChange={(appIcon) => onSettings({ appIcon })} />
    </Row>
  </div>
);

// ---------- MCP 服务器（v0.8） ----------

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
      <div className="set-rows">
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
          <Row name="名字"><Field width={200} aria-label="MCP 服务器的名字" placeholder="小写字母、数字、- 和 _" value={draft.server.name} onChange={(e) => setDraft({ ...draft, server: { ...draft.server, name: e.target.value } })} /></Row>
          <Row name="命令"><Field width={320} aria-label="命令" placeholder="npx" value={draft.server.command} onChange={(e) => setDraft({ ...draft, server: { ...draft.server, command: e.target.value } })} /></Row>
          <Row name="参数"><Field width={320} aria-label="参数" placeholder="-y @modelcontextprotocol/server-filesystem ." value={draft.args} onChange={(e) => setDraft({ ...draft, args: e.target.value })} /></Row>
          <Row name="环境变量">
            <textarea className="t-code sel set-env" aria-label="环境变量" placeholder="NAME=value" rows={3} spellCheck={false} value={draft.env} onChange={(e) => setDraft({ ...draft, env: e.target.value })} />
          </Row>
          {draft.index >= 0 && <McpSecrets server={list?.[draft.index] ?? draft.server} onSaved={(next) => { setList(next); const s = next[draft.index]; if (s) setDraft({ ...draft, server: { ...draft.server, secretEnv: s.secretEnv } }); }} say={say} />}
          <div className="set-inline">
            <Btn v="primary" disabled={busy || !draft.server.name.trim() || !draft.server.command.trim()} onClick={apply}>SAVE</Btn>
            <Btn v="ghost" onClick={() => setDraft(null)}>CANCEL</Btn>
          </div>
        </div>
      ) : (
        <div className="set-inline"><Btn sm icon="plus" disabled={busy || !list} onClick={() => setDraft({ server: { ...EMPTY_SERVER }, args: "", env: "", index: -1 })}>ADD SERVER</Btn></div>
      )}
    </>
  );
};

/** 一台 MCP 服务器放在钥匙串里的环境变量（v0.10）：只列名字；改值就再存一次，REMOVE 从钥匙串删掉。 */
const McpSecrets: React.FC<{ server: McpServerConfig; onSaved: (next: McpServerConfig[]) => void; say: (m: unknown) => void }> = ({ server, onSaved, say }) => {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const set = async (n: string, v: string | null, message: string) => {
    try { onSaved(await ipc.mcpSecretSet(server.name, n, v)); setName(""); setValue(""); say(message); } catch (e) { say(e); }
  };
  return (
    <Row name="钥匙串">
      <div className="set-stack">
        {(server.secretEnv ?? []).map((n) => (
          <div key={n} className="set-inline">
            <span className="t-code">{n}=••••</span>
            <Btn sm v="danger" onClick={() => void set(n, null, `${n} 从钥匙串删了`)}>REMOVE</Btn>
          </div>
        ))}
        <div className="set-inline">
          <Field width={150} aria-label="钥匙串里的变量名" placeholder="GITHUB_TOKEN" value={name} onChange={(e) => setName(e.target.value)} />
          <Field width={180} type="password" aria-label="变量的值" placeholder="值" value={value} onChange={(e) => setValue(e.target.value)} />
          <Btn sm disabled={!name.trim() || !value.trim()} onClick={() => void set(name.trim(), value, `${name.trim()} 存进钥匙串了`)}>SAVE</Btn>
        </div>
      </div>
    </Row>
  );
};

// ---------- 键盘（v0.8） ----------

const KEYS: Array<[string, string]> = [
  ["⌘N", "这个项目里新开一个任务"],
  ["⌘L", "到输入框"],
  ["↵ / ⇧↵", "发出去 / 换行"],
  ["⌘.", "停下在跑的任务"],
  ["⌘[ / ⌘]", "上一个 / 下一个任务"],
  ["⌘1 – ⌘6", "右栏：CHANGES、LOG、ONE SHOT、FILES、PREVIEW、TERM"],
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

// ---------- AGENTS：委派、预设（站主 11-17：从左栏挪过来，预设在这里能改） ----------

type PresetDraft = { id: string | null; name: string; spec: string; build: string; review: string; delegate: string };

const roles = (p: Pick<Preset, "spec" | "build" | "review" | "delegate" | "name">) => JSON.stringify([p.name, p.spec ?? "", p.build, p.review ?? "", p.delegate ?? ""]);

const AgentsSettings: React.FC<Props> = ({ settings, onSettings, entries, say }) => {
  const team = teamable(entries);
  const apis = team.filter((e) => e.group === "peck");
  const builders = pickable(entries).filter((e) => e.id !== RELAY);
  const [draft, setDraft] = useState<PresetDraft | null>(null);
  // 「CONFIRM」记着是哪一套，4 秒没点第二下就作废。
  const [armed, setArmed] = useState<string | null>(null);
  useEffect(() => {
    if (!armed) return;
    const t = window.setTimeout(() => setArmed(null), 4000);
    return () => window.clearTimeout(t);
  }, [armed]);
  const chosen = settings.pick === RELAY ? RELAY : chosenEntry(entries, settings)?.id ?? null;
  const now = currentSetup(settings, chosen);
  const builderNow = settings.pick === RELAY ? settings.relay.build : chosen;
  const use = (p: Preset) => {
    const problems = presetProblems(p, entries);
    if (problems.length) { say(`「${p.name}」现在用不了：${problems.join("；")}`); return; }
    onSettings(applyPreset(p, settings));
    say(`用上了「${p.name}」`);
  };
  const save = () => {
    if (!draft) return;
    const name = draft.name.trim().slice(0, 24);
    if (!name || !draft.build) return;
    const body = { name, spec: draft.spec || undefined, build: draft.build, review: draft.review || undefined, delegate: draft.delegate || undefined };
    if (draft.id) {
      const presets = settings.presets.map((p) => (p.id === draft.id ? { ...p, ...body } : p));
      const patch: Partial<AppSettings> = { presets };
      // 改的是正在用的那套：左栏 AGENTS 跟着变（用不了就只存）。
      const edited = presets.find((p) => p.id === draft.id);
      if (edited && settings.preset === edited.id && !presetProblems(edited, entries).length) Object.assign(patch, applyPreset(edited, settings), { presets });
      onSettings(patch);
      say(`「${name}」存好了`);
    } else {
      onSettings({ presets: [...settings.presets, { id: `preset:${Date.now().toString(36)}`, ...body }] });
      say(`加了预设「${name}」`);
    }
    setDraft(null);
  };
  const editor = draft && (
    <div className="pk-mod pk-mod--sunk set-editor">
      <p className="lbl">{draft.id ? "EDIT PRESET" : "ADD PRESET"}</p>
      <Row name="名字"><Field width={240} aria-label="预设的名字" value={draft.name} autoFocus onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></Row>
      <Row name="写规范"><ModelChip label="写规范" entries={team} value={draft.spec} off="OFF" onChange={(spec) => setDraft({ ...draft, spec })} /></Row>
      <Row name="实施"><ModelChip label="实施" entries={draft.spec ? team : builders} value={draft.build} onChange={(build) => setDraft({ ...draft, build })} /></Row>
      <Row name="审查"><ModelChip label="审查" entries={team} value={draft.review} off="OFF" onChange={(review) => setDraft({ ...draft, review })} /></Row>
      <Row name="委派"><ModelChip label="委派" entries={apis} value={draft.delegate} off="OFF" onChange={(delegate) => setDraft({ ...draft, delegate })} /></Row>
      <div className="set-inline">
        <Btn v="primary" disabled={!draft.name.trim() || !draft.build} onClick={save}>SAVE</Btn>
        <Btn v="ghost" onClick={() => setDraft(null)}>CANCEL</Btn>
      </div>
    </div>
  );
  return (
    <>
      <div className="set-rows">
        <Row name="委派" off={!settings.delegate} desc={settings.delegate && !canDelegate(entries.find((e) => e.id === builderNow)) ? "现在实施的这一个不委派" : undefined}>
          <ModelChip label="委派给谁" entries={apis} value={settings.delegate} off="OFF" onChange={(delegate) => onSettings({ delegate })} />
        </Row>
      </div>
      <p className="lbl">PRESETS</p>
      <div className="set-rows">
        {settings.presets.map((p) => {
          if (draft?.id === p.id) return <React.Fragment key={p.id}>{editor}</React.Fragment>;
          const problems = presetProblems(p, entries);
          const builtin = BUILTIN_PRESETS.find((b) => b.id === p.id);
          return (
            <Row key={p.id} name={p.name} off={problems.length > 0} desc={problems[0] ?? presetSummary(p, entries)}>
              {p.id === settings.preset ? <Tag v="ok">IN USE</Tag> : <Btn sm disabled={problems.length > 0} onClick={() => use(p)}>USE</Btn>}
              <Btn sm onClick={() => setDraft({ id: p.id, name: p.name, spec: p.spec ?? "", build: p.build, review: p.review ?? "", delegate: p.delegate ?? "" })}>EDIT</Btn>
              {builtin ? (
                roles(builtin) !== roles(p) && <Btn sm onClick={() => { onSettings({ presets: settings.presets.map((x) => (x.id === p.id ? { ...builtin } : x)) }); say(`「${builtin.name}」回到自带的样子`); }}>RESET</Btn>
              ) : (
                <Btn sm v="danger" onClick={() => {
                  if (armed !== p.id) { setArmed(p.id); return; }
                  setArmed(null);
                  onSettings({ presets: settings.presets.filter((x) => x.id !== p.id), ...(settings.preset === p.id ? { preset: "" } : {}) });
                  say(`删掉了预设「${p.name}」`);
                }}>{armed === p.id ? "CONFIRM" : "DELETE"}</Btn>
              )}
            </Row>
          );
        })}
      </div>
      {draft && !draft.id ? editor : !draft && (
        <div className="set-inline">
          <Btn sm icon="plus" onClick={() => setDraft({ id: null, name: "", spec: now?.spec ?? "", build: now?.build ?? builders[0]?.id ?? "", review: now?.review ?? "", delegate: now?.delegate ?? "" })}>ADD PRESET</Btn>
        </div>
      )}
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
          <Row name="模型名"><Field width={260} aria-label="模型名" placeholder="gpt-6-luna" value={draft.model} onChange={(e) => set({ model: e.target.value })} /></Row>
          <Row name="显示名"><Field width={260} aria-label="显示名" placeholder="GPT-6 Luna" value={draft.label} onChange={(e) => set({ label: e.target.value })} /></Row>
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
          <span className="hint">1 USD =</span>
          <BoundedNumber width={110} decimals={3} min={0.2} max={99} label="汇率：1 美元换多少人民币" value={book.fxRate} onCommit={(fxRate) => void run(() => ipc.pricesSet({ fxRate }), "汇率存好了")} />
          <span className="hint">CNY</span>
        </Row>
        <Row name="价格表" desc={`内置 ${book.builtinDate}${book.refreshedAt ? ` · ${ago(book.refreshedAt)}刷新过` : ""}`}>
          <Btn sm icon="undo" disabled={busy} onClick={() => void refresh()}>{busy ? "REFRESHING" : "REFRESH"}</Btn>
        </Row>
      </div>
      {groups.map(({ v, rows }) => (
        <React.Fragment key={v.id}>
          <p className="lbl">{v.label.toUpperCase()}<em className="menu-group-note">每百万 token</em></p>
          <div className="set-rows">
            {rows.map((r) => (draft && !draft.isNew && draft.model === r.model ? <React.Fragment key={r.model}>{editor}</React.Fragment> : (
              <Row key={r.model} name={r.label} desc={<span className="t-code">{priceText(r)}</span>}>
                {r.origin === "edited" && <Tag v="wait">EDITED</Tag>}
                {r.origin === "online" && <Tag v="ok">UPDATED</Tag>}
                {r.origin === "custom" && <Tag>ADDED</Tag>}
                <Btn sm disabled={busy} onClick={() => edit(r)}>EDIT</Btn>
                {r.origin === "edited" && <Btn sm disabled={busy} onClick={() => void run(() => ipc.pricesSet({ reset: r.model }), `${r.label} 回到内置的价格`)}>RESET</Btn>}
                {r.origin === "custom" && <Btn sm v="danger" disabled={busy} onClick={() => void run(() => ipc.pricesSet({ reset: r.model }), `删掉了 ${r.label}`)}>DELETE</Btn>}
              </Row>
            )))}
          </div>
        </React.Fragment>
      ))}
      {draft?.isNew ? editor : !draft && (
        <div className="set-inline">
          <Btn sm icon="plus" onClick={() => setDraft({ isNew: true, model: "", label: "", vendor: "other", currency: "USD", input: "", output: "", cacheRead: "", cacheWrite: "", peak: "", tiers: [] })}>ADD MODEL</Btn>
        </div>
      )}
    </>
  );
};
