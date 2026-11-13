import React, { useEffect, useState } from "react";
import { api, withAgentModel, type ApiConfig, type ApiMode, type ApiStatus, type ModelProfile, type RunLimits, type Thinking } from "@/lib/api";
import { ipc, isMockMode, type AllowRule } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import type { ThemeChoice } from "@/lib/theme";
import type { CliAccount, CliAccountStatus, CliLoginState, EngineAccounts, GatewayConfig, ModelEntry } from "@/lib/agentEvents";
import type { AgentInfo, Permission } from "@/lib/types";
import { PERMISSIONS, agentDot, agentName, ago, cn, engineDot } from "@/lib/utils";
import { Btn, Dot, Field, Icon, MenuChip, ModHead, Row, Seg, Switch } from "@/ui";

const NAV = [
  ["PROVIDERS", "供应商和 Key"],
  ["LIMIT", "花费上限"],
  ["ACCESS", "权限"],
  ["CLI", "CLI 检测"],
  ["NOTIFY", "通知"],
  ["LOOK", "外观"],
  ["DEV", "开发者选项"],
] as const;
type Section = (typeof NAV)[number][0];

interface Props {
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  agents: AgentInfo[];
  onRefreshAgents: () => Promise<void>;
  apiStatus: ApiStatus | null;
  onRefreshApi: () => Promise<ApiStatus | null>;
  onClose: () => void;
  say: (message: unknown) => void;
}

/** 几样里选一样：选项片 + 弹出的菜单。 */
function Pick<T extends string>({ value, options, onChange, label, disabled, dot }: {
  value: T | "";
  options: Array<{ id: T; label: string; note?: string }>;
  onChange: (id: T) => void;
  label: string;
  disabled?: boolean;
  dot?: boolean;
}) {
  const current = options.find((o) => o.id === value);
  return (
    <MenuChip ariaLabel={label} disabled={disabled} up={false} dot={dot ? "api" : undefined} label={(current?.label ?? "选一个").toUpperCase()}>
      {(close) => options.map((o) => (
        <button key={o.id} type="button" role="menuitem" className={cn("pk-menu-item", o.id === value && "is-on")} onClick={() => { onChange(o.id); close(); }}>
          {dot ? <Dot kind="api" /> : <i />}<b>{o.label.toUpperCase()}</b><span>{o.note ?? ""}</span>
        </button>
      ))}
    </MenuChip>
  );
}

const Tag: React.FC<{ v?: "ok" | "wait" | "high"; children: React.ReactNode }> = ({ v, children }) => <em className={cn("pk-tag", v && `pk-tag--${v}`)}>{children}</em>;

/** 设置：一个面板，占中栏和右栏合成的宽栏；改了马上生效（供应商和模型那一组要点 SAVE）。 */
export const SettingsPage: React.FC<Props> = (props) => {
  const [section, setSection] = useState<Section>("PROVIDERS");
  return (
    <section className="col page" aria-label="设置">
      <div className="pk-mod col-grow page-mod">
        <ModHead label="SETTINGS" note="改了马上生效"><Btn v="ghost" sm icon="x" onClick={props.onClose}>CLOSE</Btn></ModHead>
        <div className="set-grid">
          <nav className="set-nav" aria-label="设置分组">
            {NAV.map(([en, zh]) => (
              <button key={en} type="button" className={en === section ? "is-on" : undefined} aria-current={en === section} onClick={() => setSection(en)}>
                <b className="t-button">{en}</b><span>{zh}</span>
              </button>
            ))}
          </nav>
          <div className="set-pane scroll">
            {section === "PROVIDERS" && <Providers {...props} />}
            {section === "LIMIT" && <Limit {...props} />}
            {section === "ACCESS" && <Access {...props} />}
            {section === "CLI" && <Cli {...props} />}
            {section === "NOTIFY" && <Notify {...props} />}
            {section === "LOOK" && <Look {...props} />}
            {section === "DEV" && <Dev {...props} />}
          </div>
        </div>
      </div>
    </section>
  );
};

const Pane: React.FC<{ title: string; lead: string; children: React.ReactNode }> = ({ title, lead, children }) => (
  <>
    <h2 className="t-title">{title}</h2>
    <p className="lead">{lead}</p>
    {children}
  </>
);

// ---------- 供应商和 Key ----------

const KEY_STATE = { unloaded: "在读钥匙串：Peck 更新以后系统会弹窗问，点「始终允许」", missing: "没配 Key，「谁来干」里不出现", saved: "Key 已保存 · 认证要发一次请求才知道", unavailable: "钥匙串没授权：点下面 READ KEYS 再问一次" } as const;
const THINKING: Record<Thinking, string> = { unknown: "能力还没填", never: "不能思考", switch: "思考能开关", always: "一直思考" };

/** 旧 API 层的设置（百炼的地域、规划 + 实施的模型）：改了要点 SAVE；存的时候整份一起存。 */
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
  if (!apiStatus || !config) return <Pane title="供应商和 Key" lead="在读设置…">{null}</Pane>;
  return (
    <Pane title="供应商和 Key" lead="Key 存在钥匙串里，不进配置、历史和模型上下文。只有配了 Key 的供应商，才会出现在「谁来干」里；每家有哪些模型、什么价格，在模型列表里看。">
      <div className="set-rows">
        {apiStatus.providers.map((p) => {
          const state = apiStatus.keys.find((k) => k.providerId === p.id)?.state ?? "missing";
          return (
            <div key={p.id} className="set-block">
              <Row name={p.name} off={state !== "saved"} desc={KEY_STATE[state]}>
                {state === "saved" ? <Tag v="ok">SAVED</Tag> : <Tag>{state === "unloaded" || state === "unavailable" ? "NOT READ" : "NO KEY"}</Tag>}
              </Row>
              <div className="set-inline">
                <Field type="password" aria-label={`${p.name} 的 Key`} placeholder={`粘贴 ${p.name} 的 Key`} value={keys[p.id] ?? ""} off={busy} onChange={(e) => setKeys((k) => ({ ...k, [p.id]: e.target.value }))} />
                <Btn sm disabled={busy || !keys[p.id]?.trim()} onClick={() => void act(async () => { await api.setKey(p.id, keys[p.id]); setKeys((k) => ({ ...k, [p.id]: "" })); await onRefreshApi(); say(apiStatus.mode === "mock" ? "假 Key 只存在内存里，没进钥匙串" : "Key 已保存到钥匙串；认证通过要发一次请求才知道"); })}>SAVE</Btn>
                <Btn sm v="danger" disabled={busy || state !== "saved"} onClick={() => void act(async () => { await api.setKey(p.id, null); await onRefreshApi(); })}>REMOVE</Btn>
              </div>
              {p.id === "aliyun" && (
                <div className="set-inline">
                  <Seg label="阿里云服务地域" sm items={[{ id: "beijing", label: "BEIJING" }, { id: "singapore", label: "SINGAPORE" }]} value={config.aliyunRegion} onChange={(aliyunRegion) => change({ ...config, aliyunRegion })} />
                  <Field aria-label="阿里云业务空间 ID" placeholder="业务空间 ID（可以不填）" value={config.aliyunWorkspaceId} onChange={(e) => change({ ...config, aliyunWorkspaceId: e.target.value.trim() })} />
                  <Btn sm disabled={busy || !dirty} onClick={() => void save("百炼的地域存好了")}>SAVE</Btn>
                  <span className="hint">地域要和 Key 一致</span>
                </div>
              )}
            </div>
          );
        })}
      </div>
      <div className="set-inline"><Btn sm disabled={busy} onClick={() => void act(async () => { await api.loadKeys(); await onRefreshApi(); })}>READ KEYS</Btn><span className="hint">打开 App 时自动读。AI 的 Key 一包、其他 Key 一包，Peck 每次更新最多问两次授权；拒了就点这里再问。</span></div>
      <Vertex say={say} />
    </Pane>
  );
};

const ModelEditor: React.FC<{ model: ModelProfile; status: ApiStatus; onSave: (m: ModelProfile) => void; onCancel: () => void }> = ({ model: initial, status, onSave, onCancel }) => {
  const [model, setModel] = useState(initial);
  const [efforts, setEfforts] = useState(initial.efforts.join(", "));
  const known = (model.providerId === "anthropic" && model.apiModel === "claude-opus-5-5") || (model.providerId === "deepseek" && model.apiModel === "deepseek-flash");
  const identify = (apiModel: string, providerId = model.providerId) => {
    const opus = providerId === "anthropic" && apiModel === "claude-opus-5-5";
    const flash = providerId === "deepseek" && apiModel === "deepseek-flash";
    const next = { ...model, apiModel, providerId, thinking: (opus ? "always" : flash ? "switch" : "unknown") as Thinking, efforts: opus ? ["low", "medium", "high", "xhigh", "max"] : flash ? ["low", "high", "max"] : [], tools: opus || flash };
    setModel(next);
    setEfforts(next.efforts.join(", "));
  };
  return (
    <div className="pk-mod pk-mod--sunk set-editor">
      <p className="lbl">EDIT MODEL</p>
      <Row name="名字"><Field width={260} aria-label="模型的名字" value={model.name} onChange={(e) => setModel({ ...model, name: e.target.value })} /></Row>
      <Row name="供应商"><Pick label="模型的供应商" value={model.providerId} options={status.providers.map((p) => ({ id: p.id, label: p.name }))} onChange={(id) => identify(model.apiModel, id)} /></Row>
      <Row name="API 里的模型 ID"><Field width={260} aria-label="API 里的模型 ID" value={model.apiModel} onChange={(e) => identify(e.target.value)} /></Row>
      <Row name="上下文上限 / 最多输出" desc="单位是 Token">
        <Field width={130} type="number" aria-label="上下文上限" value={model.contextWindow} onChange={(e) => setModel({ ...model, contextWindow: Number(e.target.value) })} />
        <Field width={130} type="number" aria-label="最多输出" value={model.maxOutputTokens} onChange={(e) => setModel({ ...model, maxOutputTokens: Number(e.target.value) })} />
      </Row>
      <Row name="思考能力" desc={known ? "内置模型，能力是核对过的。" : "自己填的能力没在真实服务上验过。"}>
        <Pick label="思考能力" disabled={known} value={model.thinking} options={(Object.keys(THINKING) as Thinking[]).map((id) => ({ id, label: THINKING[id] }))} onChange={(thinking) => setModel({ ...model, thinking, efforts: thinking === "never" ? [] : model.efforts })} />
      </Row>
      <Row name="思考档位" desc="照这个模型的官方说法填，用逗号隔开，比如 low, high">
        <Field width={260} aria-label="思考档位" off={known || model.thinking === "never" || model.providerId === "aliyun"} value={efforts} onChange={(e) => setEfforts(e.target.value)} />
      </Row>
      <Row name="支持工具调用" desc="不支持的模型不能干活"><Switch label="支持工具调用" on={model.tools} disabled={known} onChange={(tools) => setModel({ ...model, tools })} /></Row>
      <div className="set-inline">
        <Btn disabled={!model.name.trim() || !model.apiModel.trim()} onClick={() => onSave({ ...model, efforts: known ? model.efforts : model.thinking === "never" ? [] : efforts.split(",").map((s) => s.trim()).filter(Boolean) })}>APPLY</Btn>
        <Btn v="ghost" onClick={onCancel}>CANCEL</Btn>
      </div>
    </div>
  );
};

// ---------- 花费上限 ----------

/** 这个月已经花了多少（Peck 循环）。 */
const SpendNote: React.FC<{ cap: number | null }> = ({ cap }) => {
  const [spent, setSpent] = useState<number | null>(null);
  useEffect(() => { void ipc.harnessSpend().then((s) => setSpent(s.monthUsd)).catch(() => setSpent(null)); }, []);
  const used = spent == null ? "读不到" : `$${spent.toFixed(spent < 1 ? 4 : 2)}`;
  return <>到了就在下一次请求前停下。这个月已经花了 {used}{cap ? `，上限 $${cap}` : "，没设上限"}。</>;
};

const Limit: React.FC<Props> = ({ settings, onSettings }) => (
  <Pane title="花费上限" lead="一个月里 Peck 循环和 ONE SHOT（API 模型）加起来的钱，按价格表算。CLI 用的是订阅额度，不算在这里。">
    <div className="set-rows">
      <Row name="每月上限" desc={<SpendNote cap={settings.costCap} />}>
        <span className="hint">$</span>
        <Field width={130} type="number" aria-label="每月花费上限（美元）" min={0} value={settings.costCap ?? ""} onChange={(e) => onSettings({ costCap: e.target.value === "" ? null : Math.max(0, Number(e.target.value)) })} />
      </Row>
    </div>
    <p className="hint">设了上限时，价格表里没有价格的模型发不出去（花了多少算不出来）。Peck 循环另有轮数（40）和上下文（85%）的上限；旧的规划 + 实施的上限在开发者选项里。</p>
  </Pane>
);

// ---------- 权限 ----------

const HIGH_RISK = ["删文件", "推送和改 Git 历史", "写到项目外", "改系统设置", "看不出会做什么的命令"];
const RULE_WORD: Record<string, string> = { "install-deps": "装依赖", "long-running": "起常驻进程" };

const Access: React.FC<Props> = ({ settings, onSettings, say }) => {
  const [rules, setRules] = useState<AllowRule[] | null>(null);
  useEffect(() => {
    let live = true;
    ipc.harnessAllowList().then((r) => { if (live) setRules(r); }).catch(say);
    return () => { live = false; };
  }, []);
  return (
    <Pane title="权限" lead="发任务时默认用哪一档；每次发之前还能在输入框旁边换。Peck 循环按档位给工具；CLI 引擎由 Peck 换成它们自己的参数，弹的卡片是同一套。">
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
      <p className="hint">这几样算高危：不管哪一档都弹卡片，只有 ALLOW 和 DENY，不能「总是允许」。</p>
      <p className="lbl">ALWAYS ALLOWED</p>
      {rules && rules.length > 0 ? (
        <div className="set-rows">
          {rules.map((r) => (
            <Row key={`${r.workspaceId}:${r.rule}`} name={RULE_WORD[r.rule] ?? r.rule} desc={<><span className="t-code">{r.workspace ?? "项目已经不在列表里"}</span> · {ago(r.createdAt)}</>}>
              <Btn sm v="danger" onClick={() => void ipc.harnessAllowRemove(r.workspaceId, r.rule).then((next) => { setRules(next); say("撤掉了，下次又会问"); }).catch(say)}>REMOVE</Btn>
            </Row>
          ))}
        </div>
      ) : (
        <p className="hint">{rules ? "还没有。EDIT 档装依赖、起常驻进程时会弹卡，点 ALWAYS 就记在这里；撤掉以后下次又会问。" : "在读…"}</p>
      )}
    </Pane>
  );
};

// ---------- CLI ----------

const AgentRow: React.FC<{ agent: AgentInfo; onRefresh: () => Promise<void>; say: Props["say"] }> = ({ agent, onRefresh, say }) => {
  const [toml, setToml] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // 一行只放版本号和命令：完整的参数在 EDIT 里看，鼠标停在命令上也能看全。
  const version = agent.version?.match(/\d+\.\d+(\.\d+)?/)?.[0] ?? "";
  const command = [agent.command.split("/").pop() ?? agent.command, ...agent.args].join(" ");
  const open = async () => { setBusy(true); try { setToml(await ipc.agentGetConfig(agent.id)); } catch (e) { say(e); } finally { setBusy(false); } };
  const save = async () => {
    if (toml == null) return;
    setBusy(true);
    try { await ipc.agentSaveConfig(agent.id, toml); await onRefresh(); setToml(null); say("清单存好了，下一个任务生效"); } catch (e) { say(e); } finally { setBusy(false); }
  };
  return (
    <div className="set-block">
      <Row
        name={<><Dot kind={agentDot(agent.id)} />{agentName(agent.id, agent.name)}{version && <span className="t-code set-ver">{version}</span>}</>}
        off={!agent.available}
        desc={<>
          <span className="t-code set-cmd" title={command}>{command}</span>
          {!agent.available && <span>没检测到，「谁来干」里不出现{agent.diagnostic ? ` · ${agent.diagnostic}` : ""}</span>}
        </>}
      >
        {agent.available ? <Tag v="ok">FOUND</Tag> : <Tag>NOT FOUND</Tag>}
        <Btn sm disabled={busy} onClick={() => (toml == null ? void open() : setToml(null))}>{toml == null ? "EDIT" : "CLOSE"}</Btn>
      </Row>
      {toml != null && (
        <div className="set-toml">
          <textarea className="t-code sel" aria-label={`${agent.name} 的清单（TOML）`} rows={11} spellCheck={false} autoCorrect="off" autoCapitalize="off" value={toml} onChange={(e) => setToml(e.target.value)} />
          <div className="set-inline"><Btn sm v="primary" disabled={busy} onClick={() => void save()}>SAVE</Btn><span className="hint">存了以后下一个任务生效。登录交给 CLI 自己，Key 不要写进清单。</span></div>
        </div>
      )}
    </div>
  );
};

const versionNumber = (raw?: string | null) => raw?.match(/\d+\.\d+(\.\d+)?/)?.[0] ?? "";

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
    <Pane title="CLI 检测" lead="Peck 在「CLI 目录」、你终端里的 PATH 和常见的安装位置找这五家，只跑 --version。检测到不等于登录了、有额度；登录看下面的「登录」。">
      <div className="set-rows">
        <CliDir say={say} onSaved={() => recheck("CLI 目录存好了，重新检测过了")} />
        {engines?.map((e) => <EngineRow key={e.id} entry={e} />)}
      </div>
      <div className="set-inline"><Btn icon="undo" disabled={busy} onClick={() => recheck()}>RECHECK</Btn></div>
      <CliAccounts say={say} />
    </Pane>
  );
};

/** 一家 CLI：色点、名字、版本；在哪找到的、和录夹具时的版本对不对、账号。 */
const EngineRow: React.FC<{ entry: ModelEntry }> = ({ entry: e }) => {
  const found = versionNumber(e.version);
  const changed = !!found && !!e.testedVersion && found !== e.testedVersion;
  const desc = !e.available
    ? "没检测到：把它所在的目录填进上面的 CLI 目录，或者加进终端的 PATH，再点 RECHECK"
    : [
        changed ? `Peck 按 ${e.testedVersion} 测过，这台是 ${found}：格式可能变了，过程显示不对就告诉本地重录` : null,
        e.location ?? null,
        e.directOnly ? "只做直接调用" : null,
        e.account ? `账号：${e.account}` : null,
      ].filter(Boolean).join(" · ") || undefined;
  return (
    <Row key={e.id} name={<><Dot kind={engineDot(e.provider)} />{e.label}{found && <span className="t-code set-ver">{found}</span>}</>} off={!e.available} desc={desc}>
      {!e.available ? <Tag>NOT FOUND</Tag> : changed ? <Tag v="wait">CHANGED</Tag> : <Tag v="ok">FOUND</Tag>}
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
      <p className="lbl">登录</p>
      <p className="hint">Claude Code、Codex 用的就是 Claude 客户端、ChatGPT 客户端那一个账号（CLI 自己的默认位置）。要换账号点 RELOGIN；换了以后让 CLI 和客户端是同一个账号。登录时 Peck 不自动开浏览器，给你链接，自己挑浏览器去登。</p>
      <div className="set-rows">
        {groups.flatMap((g) => g.accounts.map((a) => <AccountRow key={g.engine} engine={g.engine} engineLabel={g.label} note={g.defaultNote} account={a} say={say} />))}
      </div>
      <div className="set-inline"><Btn icon="undo" disabled={checking} onClick={() => void recheck().then(() => say("登录状态都看过了"))}>{checking ? "CHECKING" : "CHECK ALL"}</Btn><span className="hint">只跑各家的登录状态命令，不发模型请求、不花额度。</span></div>
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
  const copy = (url: string) => { void navigator.clipboard?.writeText(url).then(() => say("登录链接复制好了：到你要用的浏览器（或隐身窗口）里打开")).catch(say); };
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
  return { draft, setDraft, adc, busy, dirty, save };
}

const Vertex: React.FC<{ say: (m: unknown) => void }> = ({ say }) => {
  const { draft, setDraft, adc, busy, dirty, save } = useGatewayConfig(say);
  if (!draft) return null;
  const project = draft.vertex.projectId;
  const desc = !adc
    ? "先在终端跑一次 gcloud auth application-default login，Peck 用它的登录，不要 Key"
    : project
      ? "用 gcloud 的登录；项目要开计费、启用 Vertex AI，Claude 还要在 Model Garden 里启用"
      : "填了 Project ID，Vertex 上的 Claude 和 Gemini 才出现在模型列表里";
  const set = (vertex: Partial<GatewayConfig["vertex"]>) => setDraft({ ...draft, vertex: { ...draft.vertex, ...vertex } });
  return (
    <>
      <p className="lbl">VERTEX AI</p>
      <div className="set-rows">
        <div className="set-block">
          <Row name="Vertex AI · Claude 和 Gemini" off={!adc || !project} desc={desc}>
            {adc ? <Tag v="ok">GCLOUD OK</Tag> : <Tag>NO GCLOUD</Tag>}
          </Row>
          <div className="set-inline">
            <Field aria-label="Vertex AI 的 Project ID" placeholder="Project ID" value={project} off={busy} onChange={(e) => set({ projectId: e.target.value })} />
            <Field width={150} aria-label="Vertex AI 的 Location" placeholder="global" value={draft.vertex.location} off={busy} onChange={(e) => set({ location: e.target.value })} />
            <Btn sm disabled={busy || !dirty} onClick={() => void save("Vertex AI 的设置存好了")}>SAVE</Btn>
          </div>
          <span className="hint">Location 不填就是 global；指定区域写 us-east5 这样的。</span>
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
      <Row name="CLI 目录" desc="先在这里找。打开 App 时空着，Peck 会自己找一个（旧清单旁边的 bin、你终端的 PATH）存上">{draft.cliDir ? <Tag v="ok">SET</Tag> : <Tag>PATH</Tag>}</Row>
      <div className="set-inline">
        <Field aria-label="CLI 目录" placeholder="~/…/cli/bin" value={draft.cliDir} off={busy} onChange={(e) => setDraft({ ...draft, cliDir: e.target.value })} />
        <Btn sm disabled={busy || !dirty} onClick={() => void save("CLI 目录存好了").then(onSaved)}>SAVE</Btn>
      </div>
    </div>
  );
};

// ---------- 通知、外观 ----------

const Notify: React.FC<Props> = ({ settings, onSettings }) => (
  <Pane title="通知" lead="菜单栏小图标和 macOS 通知。">
    <div className="set-rows">
      <Row name="做完发通知" desc="任务做完、失败或停下时发一条 macOS 通知；Peck 就在最前面时不发。"><Switch label="做完发通知" on={settings.notifyDone} onChange={(notifyDone) => onSettings({ notifyDone })} /></Row>
      <Row name="菜单栏小图标" desc="有任务在跑时变样（三颗点轮流亮）；点它回到 Peck。"><Tag v="ok">ON</Tag></Row>
      <Row name="要你批、要你答时发通知" desc="弹审批卡片、ASK 卡片时发一条通知；和「做完发通知」一起开关。"><Tag v={settings.notifyDone ? "ok" : undefined}>{settings.notifyDone ? "ON" : "OFF"}</Tag></Row>
    </div>
  </Pane>
);

const Look: React.FC<Props> = ({ settings, onSettings }) => (
  <Pane title="外观" lead="默认跟随系统的明暗。">
    <div className="set-rows">
      <Row name="明暗" desc="跟随系统，或者固定一种">
        <Seg<ThemeChoice> label="明暗" sm items={[{ id: "system", label: "SYSTEM" }, { id: "dark", label: "DARK" }, { id: "light", label: "LIGHT" }]} value={settings.theme} onChange={(theme) => onSettings({ theme })} />
      </Row>
      <Row name="App 图标" desc="Dock 里的图标，切了马上换；访达里的还是 J2。">
        <Seg<"j2" | "a3"> label="App 图标" sm items={[{ id: "j2", label: "J2" }, { id: "a3", label: "A3" }]} value={settings.appIcon} onChange={(appIcon) => onSettings({ appIcon })} />
      </Row>
    </div>
  </Pane>
);

// ---------- 开发者选项 ----------

const Dev: React.FC<Props> = ({ settings, onSettings, agents, onRefreshAgents, apiStatus, onRefreshApi, say }) => {
  const native = !isMockMode();
  const legacy = agents.filter((a) => a.isMock || a.id === "muse" || a.id === "grok");
  return (
    <Pane title="开发者选项" lead="给测试和验收用，平时不用动。">
      <div className="set-rows">
        <Row name="假 Agent" desc="开了以后「谁来干」里多一个 FAKE：不推理，照剧本写几个演示文件，几秒跑完。拿临时文件夹当项目来试。">
          <Switch label="假 Agent" on={settings.fakeAgent} onChange={(fakeAgent) => onSettings({ fakeAgent })} />
        </Row>
        <Row name="API 请求发到哪" off={!native} desc={!native ? "浏览器演示里不发请求。" : apiStatus?.mode === "real" ? "真实：Peck 循环、ONE SHOT 发真请求、花真钱（受花费上限管）；旧的规划 + 实施另外还占授权次数。" : "模拟：不联网，Peck 循环和 ONE SHOT 用假模型演示，Key 只在内存。"}>
          <Seg<ApiMode> label="API 请求发到哪" sm disabled={!native} items={[{ id: "mock", label: "MOCK" }, { id: "real", label: "REAL" }]} value={apiStatus?.mode ?? "mock"} onChange={(mode) => void api.setMode(mode).then(onRefreshApi).catch(say)} />
        </Row>
        <Row name="导出调用记录" off={!native} desc="旧的规划 + 实施的调用记录，脱敏的 JSON：不含 Key、路径、提示词和回复，未知费用写空">
          <Btn sm disabled={!native} onClick={() => void api.exportCalls().then((path) => say(path ? "调用记录导出好了" : "没有导出")).catch(say)}>EXPORT</Btn>
        </Row>
      </div>

      <p className="lbl">LEGACY</p>
      <p className="lead">旧的接力（Muse → Grok、Grok → Muse）、规划 + 实施和假 Agent 还跑在旧代码上，v0.8 并进 Peck 循环和多引擎合作；它们的设置先放这里。</p>
      <div className="set-rows">
        {legacy.map((a) => <AgentRow key={a.id} agent={a} onRefresh={onRefreshAgents} say={say} />)}
      </div>
      <PlanModels apiStatus={apiStatus} onRefreshApi={onRefreshApi} say={say} />
      <PlanLimits settings={settings} onSettings={onSettings} />
    </Pane>
  );
};

/** 规划 + 实施用的两个模型（旧 API 层的模型档案）。 */
const PlanModels: React.FC<Pick<Props, "apiStatus" | "onRefreshApi" | "say">> = ({ apiStatus, onRefreshApi, say }) => {
  const { config, change, dirty, busy, save } = useApiConfig(apiStatus, onRefreshApi, say);
  const [editor, setEditor] = useState<ModelProfile | null>(null);
  if (!apiStatus || !config) return null;
  return (
    <>
      <p className="lbl">规划 + 实施的模型</p>
      <div className="set-rows">
        {config.agents.map((a, i) => {
          const model = config.models.find((m) => m.id === a.modelId);
          const update = (patch: Partial<typeof a>) => change({ ...config, agents: config.agents.map((v, j) => (j === i ? { ...v, ...patch } : v)) });
          return (
            <div key={a.role} className="set-block">
              <Row name={a.role === "supervisor" ? "规划的模型" : "实施的模型"} desc={a.role === "supervisor" ? "拆任务和审查" : "做交过来的子任务"}>
                <Pick dot label={`${a.role === "supervisor" ? "规划" : "实施"}的模型`} disabled={busy} value={a.modelId} options={config.models.map((m) => ({ id: m.id, label: m.name, note: THINKING[m.thinking] }))}
                  onChange={(id) => change(withAgentModel(config, a.role, id))} />
              </Row>
              {model && (
                <div className="set-inline">
                  <span className="hint">思考</span>
                  <Switch label="思考" on={a.thinking && model.thinking !== "unknown"} disabled={busy || model.thinking !== "switch"} onChange={(on) => update({ thinking: on, effort: on ? model.efforts[0] ?? null : null })} />
                  {a.thinking && model.efforts.length > 0 && <Pick label="思考强度" disabled={busy} value={a.effort ?? ""} options={model.efforts.map((e) => ({ id: e, label: e }))} onChange={(effort) => update({ effort })} />}
                  <span className="hint">一次最多输出</span>
                  <Field width={130} type="number" aria-label="一次最多输出多少 Token" min={64} max={model.maxOutputTokens} value={a.maxOutputTokens} onChange={(e) => update({ maxOutputTokens: Number(e.target.value) })} />
                  <span className="hint">Token</span>
                </div>
              )}
            </div>
          );
        })}
        {config.models.map((m) => (
          <Row key={m.id} name={m.name} desc={<><span className="t-code">{m.apiModel}</span> · {THINKING[m.thinking]}{m.tools ? "" : " · 不支持工具"}</>}>
            <Btn sm disabled={busy} onClick={() => setEditor({ ...m })}>EDIT</Btn>
          </Row>
        ))}
        <Row name="加一个模型" desc="只给规划 + 实施用。思考能力、档位、工具支持要填清楚，不按名字猜。">
          <Btn sm icon="plus" disabled={busy} onClick={() => setEditor({ id: crypto.randomUUID(), name: "新模型", providerId: "deepseek", apiModel: "", contextWindow: 131072, maxOutputTokens: 4096, thinking: "unknown", efforts: [], tools: false })}>ADD MODEL</Btn>
        </Row>
      </div>
      {editor && (
        <ModelEditor key={editor.id} model={editor} status={apiStatus} onCancel={() => setEditor(null)} onSave={(edited) => {
          change({
            ...config,
            models: config.models.some((m) => m.id === edited.id) ? config.models.map((m) => (m.id === edited.id ? edited : m)) : [...config.models, edited],
            agents: config.agents.map((a) => {
              if (a.modelId !== edited.id) return a;
              const thinking = edited.thinking === "always" || (edited.thinking !== "never" && a.thinking);
              return { ...a, thinking, effort: thinking && a.effort && edited.efforts.includes(a.effort) ? a.effort : null, maxOutputTokens: Math.min(a.maxOutputTokens, edited.maxOutputTokens) };
            }),
          });
          setEditor(null);
        }} />
      )}
      <div className="set-inline set-save">
        <Btn v="primary" disabled={busy || !dirty || !!editor} onClick={() => void save("规划 + 实施的模型存好了，下一个任务生效")}>SAVE</Btn>
        <span className="hint">{dirty ? "有没存的修改" : "改了要点 SAVE 才生效。"}</span>
      </div>
    </>
  );
};

const LIMITS: Array<[keyof RunLimits, string, string, number, number]> = [
  ["maxRounds", "模型轮数", "一个任务里所有模型加起来最多跑几轮", 1, 40],
  ["maxToolCalls", "工具次数", "读文件、改文件、跑命令都算", 1, 200],
  ["maxContextBytes", "上下文大小", "字节", 8000, 800000],
  ["maxTotalTokens", "Token 总数", "输入加输出", 1024, 2000000],
  ["maxDelegations", "最多交给实施的模型几项", "规划 + 实施", 0, 8],
  ["maxReworks", "每项最多返工几次", "规划 + 实施", 0, 2],
  ["workerMaxRounds", "实施的模型每项最多几轮", "规划 + 实施", 1, 20],
];

/** 规划 + 实施的几个上限（到了哪一个都会停下）。 */
const PlanLimits: React.FC<Pick<Props, "settings" | "onSettings">> = ({ settings, onSettings }) => {
  const [more, setMore] = useState(false);
  return (
    <div className="set-rows">
      <Row name="规划 + 实施的上限" desc="轮数、工具次数、上下文、Token、委派和返工；到了哪一个都会停下。平时不用动。">
        <Btn sm onClick={() => setMore(!more)}>{more ? "HIDE" : "SHOW"}</Btn>
      </Row>
      {more && LIMITS.map(([key, name, desc, min, max]) => (
        <Row key={key} name={name} desc={desc}>
          <Field width={130} type="number" aria-label={name} min={min} max={max} value={settings.limits[key]} onChange={(e) => onSettings({ limits: { ...settings.limits, [key]: Math.min(max, Math.max(min, Number(e.target.value) || min)) } })} />
        </Row>
      ))}
    </div>
  );
};
