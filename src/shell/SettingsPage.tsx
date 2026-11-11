import React, { useEffect, useState } from "react";
import { api, type ApiCall, type ApiConfig, type ApiEvent, type ApiMode, type ApiStatus, type ModelProfile, type RunLimits, type Thinking } from "@/lib/api";
import { ipc, isMockMode } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import type { ThemeChoice } from "@/lib/theme";
import type { AgentInfo, Permission } from "@/lib/types";
import { PERMISSIONS, agentDot, agentName, clock, cn } from "@/lib/utils";
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

// ---------- 供应商和 Key、模型 ----------

const KEY_STATE = { unloaded: "还没读钥匙串", missing: "没配 Key，「谁来干」里不出现", saved: "Key 已保存 · 认证要发一次请求才知道", unavailable: "钥匙串现在读不了" } as const;
const THINKING: Record<Thinking, string> = { unknown: "能力还没填", never: "不能思考", switch: "思考能开关", always: "一直思考" };

const Providers: React.FC<Props> = ({ apiStatus, onRefreshApi, say }) => {
  const [config, setConfig] = useState<ApiConfig | null>(apiStatus?.config ?? null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [editor, setEditor] = useState<ModelProfile | null>(null);
  useEffect(() => { if (!dirty) setConfig(apiStatus?.config ?? null); }, [apiStatus, dirty]);
  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try { await fn(); } catch (e) { say(e); } finally { setBusy(false); }
  };
  if (!apiStatus || !config) return <Pane title="供应商和 Key" lead="在读设置…">{null}</Pane>;
  const change = (next: ApiConfig) => { setConfig(next); setDirty(true); };
  return (
    <Pane title="供应商和 Key" lead="Key 存在钥匙串里，不进配置、历史和模型上下文。「已保存」和「认证通过」分开写。只有配了 Key 的供应商，才会出现在「谁来干」里。">
      <div className="set-rows">
        {apiStatus.providers.map((p) => {
          const state = apiStatus.keys.find((k) => k.providerId === p.id)?.state ?? "missing";
          const lastOk = apiStatus.calls.find((c) => c.mode === "real" && c.model.providerId === p.id && c.status === "completed");
          return (
            <div key={p.id} className="set-block">
              <Row name={p.name} off={state !== "saved"} desc={<>{KEY_STATE[state]}{lastOk ? ` · 上次真实请求成功 ${clock(lastOk.startedAt)}` : ""}</>}>
                {state === "saved" ? <Tag v="ok">SAVED</Tag> : <Tag>{state === "unloaded" ? "NOT READ" : "NO KEY"}</Tag>}
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
                  <span className="hint">地域要和 Key 一致</span>
                </div>
              )}
            </div>
          );
        })}
      </div>
      <div className="set-inline"><Btn sm disabled={busy} onClick={() => void act(async () => { await api.loadKeys(); await onRefreshApi(); })}>READ KEYS</Btn><span className="hint">看状态不读 Key 的内容；点这里才去读钥匙串。</span></div>

      <p className="lbl">MODELS</p>
      <div className="set-rows">
        {config.agents.map((a, i) => {
          const model = config.models.find((m) => m.id === a.modelId);
          const update = (patch: Partial<typeof a>) => change({ ...config, agents: config.agents.map((v, j) => (j === i ? { ...v, ...patch } : v)) });
          return (
            <div key={a.role} className="set-block">
              <Row name={a.role === "supervisor" ? "规划的模型" : "实施的模型"} desc={a.role === "supervisor" ? "规划 + 实施里负责拆任务和审查" : "单模型干活也用它"}>
                <Pick dot label={`${a.role === "supervisor" ? "规划" : "实施"}的模型`} disabled={busy} value={a.modelId} options={config.models.map((m) => ({ id: m.id, label: m.name, note: THINKING[m.thinking] }))}
                  onChange={(id) => { const next = config.models.find((m) => m.id === id)!; update({ modelId: next.id, thinking: next.thinking !== "never" && next.thinking !== "unknown", effort: next.efforts.includes("medium") ? "medium" : next.efforts[0] ?? null, maxOutputTokens: Math.min(a.maxOutputTokens, next.maxOutputTokens) }); }} />
              </Row>
              {model && (
                <div className="set-inline">
                  <span className="hint">思考</span>
                  <Switch label="思考" on={a.thinking && model.thinking !== "unknown"} disabled={busy || model.thinking !== "switch"} onChange={(on) => update({ thinking: on, effort: on ? model.efforts[0] ?? null : null })} />
                  {a.thinking && model.efforts.length > 0 && <Pick label="思考强度" disabled={busy} value={a.effort ?? ""} options={model.efforts.map((e) => ({ id: e, label: e }))} onChange={(effort) => update({ effort })} />}
                  <span className="hint">一次最多输出</span>
                  <Field width={130} type="number" aria-label="一次最多输出多少 Token" min={64} max={model.maxOutputTokens} value={a.maxOutputTokens} onChange={(e) => update({ maxOutputTokens: Number(e.target.value) })} />
                  <span className="hint">Token（思考也算在里面）</span>
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
        <Row name="加一个模型" desc="思考能力、档位、工具支持要填清楚，不按名字猜。">
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
        <Btn v="primary" disabled={busy || !dirty || !!editor} onClick={() => void act(async () => { await api.saveConfig(config); setDirty(false); await onRefreshApi(); say("模型设置存好了，下一个任务生效"); })}>SAVE</Btn>
        <span className="hint">{dirty ? "模型这一组有没存的修改" : "模型这一组要点 SAVE 才生效；Key 是存一个生效一个。"}</span>
      </div>
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

const LIMITS: Array<[keyof RunLimits, string, string, number, number]> = [
  ["maxRounds", "模型轮数", "一个任务里所有模型加起来最多跑几轮", 1, 40],
  ["maxToolCalls", "工具次数", "读文件、改文件、跑命令都算", 1, 200],
  ["maxContextBytes", "上下文大小", "字节", 8000, 800000],
  ["maxTotalTokens", "Token 总数", "输入加输出", 1024, 2000000],
  ["maxDelegations", "最多交给实施的模型几项", "规划 + 实施", 0, 8],
  ["maxReworks", "每项最多返工几次", "规划 + 实施", 0, 2],
  ["workerMaxRounds", "实施的模型每项最多几轮", "规划 + 实施", 1, 20],
];

const Limit: React.FC<Props> = ({ settings, onSettings }) => {
  const [more, setMore] = useState(false);
  return (
    <Pane title="花费上限" lead="一个月里所有 API 任务加起来的钱，按供应商的价格表估算，算不出来的写「未知」。CLI 用的是订阅额度，不算在这里。">
      <div className="set-rows">
        <Row name="每月上限" desc="现在先存着这个数；按价格表估算、到了就停，从 v0.6 起生效。">
          <span className="hint">$</span>
          <Field width={130} type="number" aria-label="每月花费上限（美元）" min={0} value={settings.costCap ?? ""} onChange={(e) => onSettings({ costCap: e.target.value === "" ? null : Math.max(0, Number(e.target.value)) })} />
        </Row>
        <Row name="其他几个上限" desc="API 模式的任务到了哪一个都会停下，现在就生效。平时不用动。">
          <Btn sm onClick={() => setMore(!more)}>{more ? "HIDE" : "SHOW"}</Btn>
        </Row>
        {more && LIMITS.map(([key, name, desc, min, max]) => (
          <Row key={key} name={name} desc={desc}>
            <Field width={130} type="number" aria-label={name} min={min} max={max} value={settings.limits[key]} onChange={(e) => onSettings({ limits: { ...settings.limits, [key]: Math.min(max, Math.max(min, Number(e.target.value) || min)) } })} />
          </Row>
        ))}
      </div>
    </Pane>
  );
};

// ---------- 权限 ----------

const Access: React.FC<Props> = ({ settings, onSettings }) => (
  <Pane title="权限" lead="发任务时默认用哪一档；每个任务发出前还能在输入框旁边换。">
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
      {["删文件", "推送", "改项目外的文件", "改系统设置"].map((t) => <span key={t} className="chip-quiet"><Icon name="lock" size={12} />{t}</span>)}
    </div>
    <p className="lead">这几样算高危，开始前一张卡一次批，项目外的默认不勾。</p>
    <p className="note note--quiet">这一版权限档只记在任务上：CLI 照它自己的权限设置跑，Peck 不加 --yolo。按档位管住 CLI、出高危卡片从 v0.5 起；API 模式按档位给工具从 v0.6 起。</p>
  </Pane>
);

// ---------- CLI 检测 ----------

const AgentRow: React.FC<{ agent: AgentInfo; onRefresh: () => Promise<void>; say: Props["say"] }> = ({ agent, onRefresh, say }) => {
  const [toml, setToml] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const open = async () => { setBusy(true); try { setToml(await ipc.agentGetConfig(agent.id)); } catch (e) { say(e); } finally { setBusy(false); } };
  const save = async () => {
    if (toml == null) return;
    setBusy(true);
    try { await ipc.agentSaveConfig(agent.id, toml); await onRefresh(); setToml(null); say("清单存好了，下一个任务生效"); } catch (e) { say(e); } finally { setBusy(false); }
  };
  return (
    <div className="set-block">
      <Row
        name={<><Dot kind={agentDot(agent.id)} />{agentName(agent.id, agent.name)}<span className="t-code set-code">{agent.command} {agent.args.join(" ")}</span></>}
        off={!agent.available}
        desc={agent.isMock ? "内置，不推理，照剧本写演示文件" : agent.available ? `检测到${agent.version ? ` · ${agent.version}` : ""} · 登录和额度要跑任务时才知道${agent.diagnostic ? ` · ${agent.diagnostic}` : ""}` : `没检测到，「谁来干」里不出现${agent.diagnostic ? ` · ${agent.diagnostic}` : ""}`}
      >
        {agent.available ? <Tag v="ok">FOUND</Tag> : <Tag>NOT FOUND</Tag>}
        <Btn sm disabled={busy} onClick={() => (toml == null ? void open() : setToml(null))}>{toml == null ? "EDIT" : "CLOSE"}</Btn>
      </Row>
      {toml != null && (
        <div className="set-toml">
          <textarea className="t-code sel" aria-label={`${agent.name} 的清单（TOML）`} rows={11} spellCheck={false} value={toml} onChange={(e) => setToml(e.target.value)} />
          <div className="set-inline"><Btn sm v="primary" disabled={busy} onClick={() => void save()}>SAVE</Btn><span className="hint">存了以后下一个任务生效。登录交给 CLI 自己，Key 不要写进清单。</span></div>
        </div>
      )}
    </div>
  );
};

const Cli: React.FC<Props> = ({ agents, onRefreshAgents, say }) => {
  const [busy, setBusy] = useState(false);
  return (
    <Pane title="CLI 检测" lead="只跑 version 看能不能启动。检测到不等于登录了、有额度；认证、额度、超时出错会让任务停下，不当成成功。">
      <div className="set-rows">
        {agents.filter((a) => !a.isMock).map((a) => <AgentRow key={a.id} agent={a} onRefresh={onRefreshAgents} say={say} />)}
        <Row name="第三家 CLI" off desc="v1.0 以后接；加一家只加一份清单"><Tag>LATER</Tag></Row>
      </div>
      <div className="set-inline"><Btn icon="undo" disabled={busy} onClick={() => { setBusy(true); void onRefreshAgents().then(() => say("重新检测过了")).finally(() => setBusy(false)); }}>RECHECK</Btn></div>
    </Pane>
  );
};

// ---------- 通知、外观 ----------

const Notify: React.FC<Props> = ({ settings, onSettings }) => (
  <Pane title="通知" lead="菜单栏小图标和 macOS 通知。">
    <div className="set-rows">
      <Row name="做完发通知" desc="任务做完、失败或停下时发一条 macOS 通知；Peck 就在最前面时不发。"><Switch label="做完发通知" on={settings.notifyDone} onChange={(notifyDone) => onSettings({ notifyDone })} /></Row>
      <Row name="菜单栏小图标" desc="有任务在跑时变样（三颗点轮流亮）；点它回到 Peck。"><Tag v="ok">ON</Tag></Row>
      <Row name="要你回答时发通知" off desc="停下来问你、要你批高危的事：v0.5 有了这两种卡片以后接上。"><Tag>LATER</Tag></Row>
    </div>
  </Pane>
);

const Look: React.FC<Props> = ({ settings, onSettings }) => (
  <Pane title="外观" lead="默认跟随系统的明暗。">
    <div className="set-rows">
      <Row name="明暗" desc="跟随系统，或者固定一种">
        <Seg<ThemeChoice> label="明暗" sm items={[{ id: "system", label: "SYSTEM" }, { id: "dark", label: "DARK" }, { id: "light", label: "LIGHT" }]} value={settings.theme} onChange={(theme) => onSettings({ theme })} />
      </Row>
      <Row name="App 图标" off desc="换成像素鸟 J2、能换回现在的图标：v0.8 做。"><Tag>LATER</Tag></Row>
    </div>
  </Pane>
);

// ---------- 开发者选项 ----------

const SCENARIOS = [["success", "正常回答"], ["authentication", "认证失败"], ["rateLimit", "额度用完"], ["disconnect", "网络断开"], ["slow", "回得很慢（能停）"]] as const;
const CALL_WORD: Record<string, string> = { running: "在跑", completed: "完成", failed: "失败", cancelled: "已停止", interrupted: "被打断" };

const Dev: React.FC<Props> = ({ settings, onSettings, agents, onRefreshAgents, apiStatus, onRefreshApi, say }) => {
  const native = !isMockMode();
  const fake = agents.find((a) => a.isMock);
  const [role, setRole] = useState<"worker" | "supervisor">("worker");
  const [prompt, setPrompt] = useState("");
  const [scenario, setScenario] = useState<string>("success");
  const [running, setRunning] = useState<string | null>(null);
  const [shown, setShown] = useState<ApiCall | null>(null);
  const [text, setText] = useState("");
  const [reasoning, setReasoning] = useState("");
  const agent = apiStatus?.config.agents.find((a) => a.role === role);
  const model = apiStatus?.config.models.find((m) => m.id === agent?.modelId);
  const allowance = apiStatus?.allowances.find((a) => a.apiModel === model?.apiModel && a.providerId === model?.providerId);
  const receive = (event: ApiEvent) => {
    if (event.type === "Text") setText((t) => t + event.text);
    else if (event.type === "Reasoning") setReasoning((t) => t + event.text);
    else if (event.type === "PersistenceError") say(event.message);
    else if (event.type === "Finished") {
      setRunning(null); setShown(event.call); setText(event.call.text); setReasoning(event.call.reasoning);
      if (event.call.error) say(event.call.error);
      void onRefreshApi();
    }
  };
  const send = async () => {
    if (!prompt.trim() || running) return;
    const id = crypto.randomUUID();
    setRunning(id); setShown(null); setText(""); setReasoning("");
    try { await api.start(id, role, prompt.trim(), apiStatus?.mode === "mock" ? scenario : "success", receive); } catch (e) { say(e); setRunning(null); }
  };
  return (
    <Pane title="开发者选项" lead="给测试和验收用，平时不用动。">
      <div className="set-rows">
        <Row name="假 Agent" desc="开了以后「谁来干」里多一个 FAKE：不推理，照剧本写几个演示文件，几秒跑完。拿临时文件夹当项目来试。">
          <Switch label="假 Agent" on={settings.fakeAgent} onChange={(fakeAgent) => onSettings({ fakeAgent })} />
        </Row>
        {fake && <AgentRow agent={fake} onRefresh={onRefreshAgents} say={say} />}
        <Row name="API 请求发到哪" off={!native} desc={!native ? "浏览器演示里不发请求。" : apiStatus?.mode === "real" ? "真实：每发一次都占授权次数、花账户里的钱，失败和重试也算。" : "模拟：不联网，用假模型，Key 只在内存，不占真实的授权次数。"}>
          <Seg<ApiMode> label="API 请求发到哪" sm disabled={!native || !!running} items={[{ id: "mock", label: "MOCK" }, { id: "real", label: "REAL" }]} value={apiStatus?.mode ?? "mock"} onChange={(mode) => void api.setMode(mode).then(onRefreshApi).catch(say)} />
        </Row>
        <Row name="导出调用记录" off={!native} desc="脱敏的 JSON：不含 Key、路径、提示词和回复，未知费用写空">
          <Btn sm disabled={!native} onClick={() => void api.exportCalls().then((path) => say(path ? "调用记录导出好了" : "没有导出")).catch(say)}>EXPORT</Btn>
        </Row>
      </div>

      <p className="lbl">ONE REQUEST</p>
      <p className="lead">问一次、答一次，不读项目、不改文件。v0.6 起搬进右栏的 ONE SHOT，这里先留着。</p>
      <div className="set-inline">
        <Pick label="用哪个模型" value={role} disabled={!!running} options={[{ id: "worker", label: "实施的模型" }, { id: "supervisor", label: "规划的模型" }]} onChange={setRole} />
        <span className="hint">{model?.name ?? "还没选模型"} · {agent?.thinking ? `思考 ${agent.effort ?? "默认"}` : "不思考"} · {allowance ? `已用 ${allowance.used}/${allowance.limit} 次` : "这个模型没有授权次数"}</span>
      </div>
      <div className="set-toml">
        <textarea className="sel" aria-label="单次请求的内容" rows={3} placeholder="问一句…" value={prompt} disabled={!!running} onChange={(e) => setPrompt(e.target.value)} />
        <div className="set-inline">
          {apiStatus?.mode === "mock" && <Pick label="模拟哪种情况" value={scenario} disabled={!!running} options={SCENARIOS.map(([id, label]) => ({ id, label }))} onChange={setScenario} />}
          <span className="grow" />
          {running ? <Btn v="danger" icon="stop" onClick={() => void api.cancel(running).catch(say)}>STOP</Btn> : <Btn iconEnd="send" disabled={!native || !prompt.trim() || !allowance || allowance.used >= allowance.limit || model?.thinking === "unknown"} onClick={() => void send()}>SEND</Btn>}
        </div>
      </div>
      {(text || reasoning || running || shown) && (
        <div className="pk-mod pk-mod--sunk set-answer sel">
          {shown && <p className="hint">{shown.model.name} · {CALL_WORD[shown.status] ?? shown.status} · 输入 {shown.response?.usage.inputTokens ?? "未知"} / 输出 {shown.response?.usage.outputTokens ?? "未知"} Token · {shown.budgetCounted ? "占了一次" : "发出前就停了，没占次数"}</p>}
          {reasoning && <details><summary className="hint">思考的内容或摘要</summary><p className="note">{reasoning}</p></details>}
          <p className="t-body answer-text">{text || (running ? "在等模型回答…" : "")}</p>
          {shown?.error && <p className="is-err">{shown.error}</p>}
        </div>
      )}
      {apiStatus && apiStatus.calls.length > 0 && (
        <>
          <p className="lbl">LAST CALLS</p>
          <div className="pk-tree calls">
            {apiStatus.calls.slice(0, 50).map((c) => (
              <button key={c.id} type="button" className={cn("call-row", shown?.id === c.id && "is-on")} disabled={!!running} onClick={() => { setShown(c); setText(c.text); setReasoning(c.reasoning); }}>
                <span className="t-label">{clock(c.startedAt)}</span><span className="call-p">{c.prompt}</span><span className="hint">{c.model.name} · {CALL_WORD[c.status] ?? c.status}</span>
              </button>
            ))}
          </div>
        </>
      )}
    </Pane>
  );
};
