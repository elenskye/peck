import React, { useEffect, useMemo, useState } from "react";
import { api, type ApiConfig, type ApiStatus, type ApiTask } from "@/lib/api";
import type { AgentEvent, Control, ModelEntry } from "@/lib/agentEvents";
import { addUsage, foldEvents, usageText, usd, type UsageLine } from "@/lib/agentView";
import type { AppSettings } from "@/lib/settings";
import type { Message, Permission, ProjectOverview, RunRecord, StepDescriptor, TaskBrief } from "@/lib/types";
import { PERMISSIONS, agentDot, clock, cn, engineDot, isAgentRun, runState, span, stepName, timer, whoDots, whoLabel , shortenHomePath } from "@/lib/utils";
import { Blocks, Btn, Dot, DotCheck, Icon, ModHead, Seg, useNow } from "@/ui";
import { AgentView } from "@/shell/AgentView";
import { Md } from "@/shell/Markdown";
import { MarkdownEditor } from "@/shell/MarkdownEditor";
import { EffortPicker, WhoPicker, defaultEffort, flowOf, labelOf, pickable } from "@/shell/ModelPicker";

interface Props {
  project: ProjectOverview | null;
  task: TaskBrief | null;
  runs: RunRecord[];
  messages: Message[];
  /** 这个项目有任务在跑 */
  busy: boolean;
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  /** 模型列表（gateway_models） */
  entries: ModelEntry[];
  apiStatus: ApiStatus | null;
  /** 统一事件：任务号 → 这次的过程 */
  agentEvents: Record<string, AgentEvent[]>;
  /** 在跑的任务号（这个项目的） */
  runningRunId: string | null;
  draft: string;
  onDraft: (text: string) => void;
  onSend: (prompt: string, who: string, permission: Permission, effort?: string) => void;
  onStop: () => void;
  onRetryStep: (run: RunRecord, stepId: string) => void;
  onControl: (runId: string, control: Control) => Promise<void>;
  onOpen: (path?: string) => void;
  onOpenSettings: () => void;
  /** 规划 + 实施在输入框下换模型：存旧 API 层的设置。 */
  onApiConfig: (config: ApiConfig) => Promise<void>;
  /** 项目文件夹不在了：重新找文件夹、移出列表。 */
  onRelocate: (projectId: string) => void;
  onRemoveProject: (projectId: string) => void;
  /** 右栏现在看的是哪一次记录 */
  shownRunId: string | null;
  /** 让右栏看这一次的改动（带上文件就直接开它的 Diff） */
  onShowRun: (runId: string, path?: string) => void;
  native: boolean;
  say: (message: unknown) => void;
}

/** 选中的「谁来干」：记住的那个还能用就用它，不然列表里第一个。 */
export function chosenEntry(entries: ModelEntry[], settings: AppSettings): ModelEntry | null {
  const list = pickable(entries, settings.fakeAgent);
  return list.find((e) => e.id === settings.pick) ?? list.find((e) => e.group === "peck") ?? list[0] ?? null;
}

/** 选了谁，NEW TASK 上写它怎么干。 */
function describe(entry: ModelEntry, apiStatus: ApiStatus | null): Array<{ name: string; dot: ReturnType<typeof agentDot>; does: string; model?: string }> {
  const modelName = (role: "worker" | "supervisor") => {
    const id = apiStatus?.config.agents.find((a) => a.role === role)?.modelId;
    return apiStatus?.config.models.find((m) => m.id === id)?.name ?? "还没选模型";
  };
  if (entry.id === "mock") return [{ name: "假 Agent", dot: "", does: "不推理，照剧本写几个演示文件，几秒跑完。只在开发和验收时用。" }];
  if (entry.group === "peck") {
    return [{ name: `Peck · ${entry.label}`, dot: "api", does: "Peck 自己的循环：读、搜、改（整批一起改）、跑命令、任务清单、问你，都用 Peck 的工具；每一步看得见，花费按价格表算，算进花费上限。" }];
  }
  if (entry.group === "cli") {
    const dot = engineDot(entry.id.slice(4));
    return [{
      name: entry.label,
      dot,
      does: entry.directOnly
        ? "直接调用：你发一句话，它自己做完；Peck 显示每一步和改动，要批的事弹卡片。不进 ONE SHOT 和任何自动化。"
        : `整包引擎：用它自己的工具和订阅额度；Peck 显示每一步、替它审批、记花费${entry.account ? `（账号：${entry.account}）` : ""}。`,
    }];
  }
  if (entry.id === "flow:plan-implement") {
    return [
      { name: "规划的模型", dot: "api", does: "拆成几项交给实施的模型，做完逐项审查：通过、返工或接手。", model: modelName("supervisor") },
      { name: "实施的模型", dot: "api", does: "只拿到目标、要改的文件和验收条件，做完交回。", model: modelName("worker") },
    ];
  }
  const [a, b] = entry.id.slice(5).split(">");
  const name = (x: string) => (x === "muse" ? "Muse" : x === "grok" ? "Grok" : x);
  return [
    { name: name(a), dot: engineDot(a), does: "先写规范：要改什么，拆成一条条任务。" },
    { name: name(b), dot: engineDot(b), does: "照规范实施，做完一条勾一条。" },
  ];
}

/** 项目文件夹不在了：任务记录和快照都还在，换个文件夹接上，或者移出列表。 */
const MissingFolder: React.FC<{ project: ProjectOverview; onRelocate: (id: string) => void; onRemove: (id: string) => void }> = ({ project, onRelocate, onRemove }) => {
  const [armed, setArmed] = useState(false);
  const { workspace, tasks } = project;
  return (
    <section className="col" aria-label="对话">
      <div className="pk-mod pk-mod--wait col-grow home">
        <ModHead label="MISSING" note={workspace.name} />
        <h1 className="t-hero">文件夹不在了</h1>
        <p className="lead">找不到 <span className="t-code">{shortenHomePath(workspace.path)}</span>：可能被删了、挪了地方，或者外接盘没插。{tasks.length ? `这个项目的 ${tasks.length} 个任务和快照都还在，` : ""}换成它现在的位置就能接着用。</p>
        <div className="set-inline">
          <Btn v="primary" icon="folder" onClick={() => onRelocate(workspace.id)}>FIND FOLDER</Btn>
          <Btn v={armed ? "danger" : "ghost"} icon="trash" onClick={() => { if (!armed) { setArmed(true); return; } onRemove(workspace.id); }}>{armed ? "CONFIRM" : "REMOVE"}</Btn>
          <span className="hint">{armed ? "再点一次：移出列表（记录不删，重新打开这个文件夹就回来）" : "外接盘没插的话，插上就好，不用动。"}</span>
        </div>
      </div>
    </section>
  );
};

export const MiddleColumn: React.FC<Props> = (props) => {
  const { project, task, runs, settings, entries, apiStatus, busy } = props;
  const [permission, setPermission] = useState<Permission>(settings.permission);
  useEffect(() => setPermission(settings.permission), [settings.permission]);

  if (!project) {
    return (
      <section className="col" aria-label="对话">
        <div className="pk-mod col-grow home">
          <ModHead label="PECK" note="只在你这台 Mac 上的 Harness" />
          <h1 className="t-hero">先选一个项目</h1>
          <p className="lead">在左边点一个项目，或者打开一个项目文件夹。给它一句话，选谁来干：Peck 自己的循环配一个 API 模型，或者一个 CLI 引擎。每一步看得见，要批的事弹卡片；整个任务能一键撤销。</p>
          {props.native && <div><Btn v="primary" icon="folder" onClick={() => props.onOpen()}>OPEN FOLDER</Btn></div>}
        </div>
      </section>
    );
  }

  if (project.missing) return <MissingFolder project={project} onRelocate={props.onRelocate} onRemove={props.onRemoveProject} />;

  const who = chosenEntry(entries, settings);
  const access = PERMISSIONS.find((p) => p.id === permission)!;
  const composer = (
    <Composer
      {...props}
      who={who}
      permission={permission}
      onPermission={setPermission}
      placeholder={task ? (busy ? "下一句话…（这个任务跑完再发）" : "下一句话…") : "一句话说要做什么…（↵ 发送，⇧↵ 换行）"}
    />
  );

  if (!task) {
    const flow = who ? describe(who, apiStatus) : [];
    return (
      <section className="col" aria-label="对话">
        <div className="pk-mod col-grow home scroll">
          <ModHead label="NEW TASK" note={project.workspace.name} />
          <h1 className="t-hero">{project.workspace.name} 要做什么？</h1>
          <p className="lead">一句话说清楚，再在下面选谁来干。开始前先存一份快照，做完默认留着；不满意，一键撤销整个任务。</p>
          {who ? (
            <div>
              <p className="lbl">WHO · {labelOf(who)}</p>
              <ol className={cn("flow", flow.length === 1 && "flow--one")}>
                {flow.map((f, i) => (
                  <li key={i}>
                    <p><span className="flow-k">{i + 1}</span><Dot kind={f.dot} /><b className="t-name">{f.name}</b>{f.model && <em>{f.model}</em>}</p>
                    <span>{f.does}</span>
                  </li>
                ))}
              </ol>
            </div>
          ) : (
            <div className="empty">
              <p>还没有能用的：在设置里配一家 API 的 Key，或者看一下 CLI 检测。</p>
              <div><Btn icon="gear" onClick={props.onOpenSettings}>SETTINGS</Btn></div>
            </div>
          )}
          <div>
            <p className="lbl">ACCESS · {access.label}</p>
            <p className="note">{access.does}</p>
          </div>
        </div>
        {composer}
      </section>
    );
  }

  return (
    <section className="col" aria-label="对话">
      <div className="col-grow scroll task-stack">
        {runs.length === 0 && <div className="pk-mod"><p className="empty-line">在读这个任务…</p></div>}
        {runs.map((run, i) => (
          <React.Fragment key={run.id}>
            <div className="you sel">
              <span className="t-label">YOU</span>
              <Md text={run.prompt} className="you-text" />
              <span className="you-meta">{clock(run.startedAt)}</span>
            </div>
            <TaskModule
              run={run}
              latest={i === runs.length - 1}
              shown={run.id === props.shownRunId}
              onShow={runs.length > 1 ? () => props.onShowRun(run.id) : undefined}
              onRetryStep={props.onRetryStep}
              onShowFile={(path) => props.onShowRun(run.id, path)}
              busy={busy}
              events={props.agentEvents[run.id]}
              live={props.runningRunId === run.id}
              onControl={(c) => props.onControl(run.id, c)}
              say={props.say}
            />
          </React.Fragment>
        ))}
      </div>
      {composer}
    </section>
  );
};

/** 这个任务一共花了多少（几次加起来）。 */
function taskUsage(runs: RunRecord[], events: Record<string, AgentEvent[]>): UsageLine | null {
  let total: UsageLine | null = null;
  for (const r of runs) {
    const ev = events[r.id];
    if (!ev) continue;
    const u = foldEvents(ev).usage;
    if (u) total = total ? addUsage(total, u) : u;
  }
  return total;
}

// ---------- 输入位 ----------

const Composer: React.FC<Props & {
  who: ModelEntry | null;
  permission: Permission;
  onPermission: (p: Permission) => void;
  placeholder: string;
}> = ({ draft, onDraft, onSend, onStop, busy, who, permission, onPermission, placeholder, settings, onSettings, entries, onOpenSettings, onApiConfig, apiStatus, runs, agentEvents, task }) => {
  const options = pickable(entries, settings.fakeAgent);
  const ready = !!draft.trim() && !!who && !busy;
  const effort = who ? settings.efforts[who.id] ?? defaultEffort(who) : "off";
  const submit = () => {
    if (!ready || !who) return;
    const chosen = who.efforts.includes(effort) ? effort : undefined;
    onSend(draft.trim(), who.id, permission, chosen);
  };
  const spent = useMemo(() => (task ? taskUsage(runs, agentEvents) : null), [task, runs, agentEvents]);
  const money = spent ? usd(spent.usd) : null;
  return (
    <div className="pk-mod composer">
      <div className={cn("composer-in", busy && "is-off")}>
        <span className="pk-prompt">›</span>
        <MarkdownEditor value={draft} onChange={onDraft} onSubmit={submit} placeholder={placeholder} disabled={busy} ariaLabel="一句话说要做什么" />
      </div>
      <div className="composer-row">
        <WhoPicker entries={options} value={who} disabled={busy} remembered={settings.picks} onOpenSettings={onOpenSettings} apiStatus={apiStatus} onApiConfig={onApiConfig}
          onChange={(pick) => { const e = options.find((x) => x.id === pick); onSettings({ pick, picks: e ? { ...settings.picks, [flowOf(e)]: pick } : settings.picks }); }}>
          <EffortPicker entry={who} value={effort} disabled={busy} onChange={(e) => who && onSettings({ efforts: { ...settings.efforts, [who.id]: e } })} />
        </WhoPicker>
        <Seg label="权限" sm disabled={busy} items={PERMISSIONS.map((p) => ({ id: p.id, label: p.label }))} value={permission} onChange={onPermission} />
        <span className="grow" />
        {spent && <span className="composer-cost t-code" title={`这个任务一共：${usageText(spent)}`}>{money == null ? "花费未知" : `${spent.source === "estimated" ? "估 " : ""}${money}`}</span>}
        {busy ? (
          <Btn v="danger" icon="stop" onClick={onStop}>STOP</Btn>
        ) : (
          <>
            <span className="kbd" title="⇧↵ 或 ⌘↵ 换行">↵</span>
            <Btn v="primary" iconEnd="send" disabled={!ready} onClick={submit}>SEND</Btn>
          </>
        )}
      </div>
    </div>
  );
};

// ---------- 一个任务：状态、进度、步骤、总结 ----------

const STATE: Record<string, { word: string; cls: string }> = {
  running: { word: "RUN", cls: "" },
  succeeded: { word: "DONE", cls: "pk-state--done" },
  failed: { word: "FAIL", cls: "pk-state--err" },
  cancelled: { word: "STOP", cls: "pk-state--off" },
  undone: { word: "UNDONE", cls: "pk-state--off" },
};

const TaskModule: React.FC<{
  run: RunRecord;
  latest: boolean;
  /** 右栏正看着这一次 */
  shown: boolean;
  /** 这个任务里发过好几次才有：让右栏看这一次的改动 */
  onShow?: () => void;
  busy: boolean;
  onRetryStep: (run: RunRecord, stepId: string) => void;
  onShowFile: (path: string) => void;
  /** 统一事件（Peck 循环、CLI 引擎的任务才有） */
  events?: AgentEvent[];
  /** 这次正在跑（这次打开 App 以后发的） */
  live: boolean;
  onControl: (control: Control) => Promise<void>;
  say: (message: unknown) => void;
}> = ({ run, latest, shown, onShow, busy, onRetryStep, onShowFile, events, live, onControl, say }) => {
  const state = runState(run);
  const running = state === "running";
  const now = useNow(running);
  const agent = isAgentRun(run);
  const view = useMemo(() => (agent && events ? foldEvents(events) : null), [agent, events]);
  const doneSteps = run.steps.filter((s) => s.status === "succeeded").length;
  const doneTasks = run.tasks.filter((t) => t.completed).length;
  const plan = view?.plan ?? null;
  const donePlan = plan ? plan.filter((p) => p.status === "completed").length : 0;
  // 进度：有任务清单就按清单，没有按步骤；在跑的那一步算半步。
  const ratio = plan && plan.length > 0 ? donePlan / plan.length : run.tasks.length > 0 ? doneTasks / run.tasks.length : run.steps.length > 0 ? doneSteps / run.steps.length : 0;
  const lit = state === "succeeded" ? 28 : Math.min(27, Math.round(ratio * 28));
  const adds = run.files.reduce((n, f) => n + f.additions, 0);
  const dels = run.files.reduce((n, f) => n + f.deletions, 0);
  const elapsed = (run.finishedAt ?? now) - run.startedAt;
  const retryable = (state === "failed" || state === "cancelled") && !busy;
  const dots = whoDots(run);
  const [open, setOpen] = useState<string | null>(null);

  return (
    <section className={cn("pk-mod task", running && "pk-mod--run", state === "failed" && "pk-mod--err", latest && "task--latest")} aria-label={`任务 ${run.number}`}>
      {running && <span className="pk-scan pk-scan--loop" />}
      <ModHead
        label="TASK"
        note={<>{dots.map((d) => <Dot key={d} kind={d} />)} {whoLabel(run)}{run.permission ? ` · ${run.permission.toUpperCase()}` : ""}</>}
        count={agent ? (plan && plan.length ? `${donePlan}/${plan.length}` : undefined) : `${doneSteps}/${run.steps.length}`}
      >
        {onShow && (
          <Btn v="ghost" sm className={cn(shown && "is-here")} aria-pressed={shown} aria-label={`在右栏看任务 ${run.number} 的改动`} onClick={onShow}>CHANGES</Btn>
        )}
      </ModHead>
      <div className="task-st">
        <span className={cn("pk-state", STATE[state].cls)}>{STATE[state].word}</span>
        <span className="task-info">
          任务 {run.number} · {clock(run.startedAt)} 开始{run.files.length > 0 ? ` · 改了 ${run.files.length} 个文件` : ""}
        </span>
        <b className={cn("pk-readout task-clock", state === "succeeded" && "pk-readout--done", !running && state !== "succeeded" && "task-clock--off")}>{timer(elapsed)}</b>
      </div>
      <Blocks lit={lit} still={!running} />
      {agent ? (
        <AgentView events={events ?? []} running={running && (live || !!events?.length)} who={whoLabel(run)} onControl={onControl} onShowFile={onShowFile} say={say} />
      ) : (
        <>
          {run.tasks.length > 0 && <Checklist run={run} />}
          <ol className="steps">
            {run.steps.map((step) => (
              <Step key={step.id} run={run} step={step} now={now} open={open === step.id} onToggle={() => setOpen(open === step.id ? null : step.id)} onRetry={retryable && (step.status === "failed" || step.status === "cancelled") ? () => onRetryStep(run, step.id) : undefined} />
            ))}
          </ol>
        </>
      )}
      {run.flowId.startsWith("api-") && <ApiHistory run={run} onShowFile={onShowFile} />}
      {state === "succeeded" && (
        <div className="summary sel">
          <div className="summary-top">
            <DotCheck />
            <div><b className="t-name">做完了</b><span>{run.files.length} 个文件 · 用了 {span(elapsed)}</span></div>
            <p className="summary-n"><span className="pk-add">+{adds}</span><span className="pk-del">−{dels}</span></p>
          </div>
          {run.summary && !agent && <p className="summary-text">{run.summary}</p>}
          {agent ? (
            <div className="verify">
              {plan && plan.length > 0 && <p className={donePlan === plan.length ? "is-ok" : "is-warn"}><Icon name={donePlan === plan.length ? "check" : "bang"} size={12} />任务清单 {donePlan}/{plan.length} 条勾了</p>}
              {!events?.length && run.summary && <Md text={run.summary} className="summary-text" />}
            </div>
          ) : (
            <div className="verify">
              {run.tasks.length > 0 && <p className={doneTasks === run.tasks.length ? "is-ok" : "is-warn"}><Icon name={doneTasks === run.tasks.length ? "check" : "bang"} size={12} />任务清单 {doneTasks}/{run.tasks.length} 条勾了</p>}
              {run.steps.map((s) => <p key={s.id} className="is-ok"><Icon name="check" size={12} />{stepName(s)}{s.exitCode != null ? ` · 退出码 ${s.exitCode}` : ""}</p>)}
            </div>
          )}
          <p className="keep">默认留着。不满意，在右边撤销整个任务。</p>
        </div>
      )}
      {(state === "failed" || state === "cancelled") && run.summary && (
        <div className={cn("pk-bar", state === "failed" && "pk-bar--bad", "task-bar sel")}>
          <Icon name={state === "failed" ? "x" : "stop"} />
          <span className="pk-bar-text">{run.summary}</span>
        </div>
      )}
      {agent && (state === "failed" || state === "cancelled") && latest && <p className="keep">文件和过程都留着；接着说一句就能继续（不重放跑过的工具）。</p>}
      {state === "undone" && run.undoneAt && (
        <p className="back"><Icon name="undo" size={12} /><span>已回到 {clock(run.startedAt)} 开始前的样子</span><span className="back-n">你的 Git 没动</span></p>
      )}
    </section>
  );
};

const Step: React.FC<{ run: RunRecord; step: StepDescriptor; now: number; open: boolean; onToggle: () => void; onRetry?: () => void }> = ({ run, step, now, open, onToggle, onRetry }) => {
  const kind = step.status === "succeeded" ? "ok" : step.status === "running" ? "run" : step.status === "failed" ? "fail" : step.status === "cancelled" ? "fail" : "todo";
  const logs = run.logs.filter((l) => l.stepId === step.id);
  const time = step.status === "running" && step.startedAt ? timer(now - step.startedAt) : step.durationMs != null ? span(step.durationMs) : "";
  const files = run.stepFiles?.[step.id] ?? run.files.filter((f) => f.stepId === step.id);
  const tail = logs.slice(-3);
  const showTail = step.status === "running" && !open && tail.length > 0;
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard.writeText(logs.map((l) => l.chunk).join("\n")).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1400); });
  };
  const desc = step.status === "pending" ? "" : step.status === "running" ? (files.length ? `在改，已经动了 ${files.length} 个文件` : "在跑") : [files.length ? `改了 ${files.length} 个文件` : "", step.status === "cancelled" ? "已停止" : "", step.exitCode != null && step.exitCode !== 0 ? `退出码 ${step.exitCode}` : ""].filter(Boolean).join(" · ");
  return (
    <li className={cn("pk-step", `pk-step--${kind}`)}>
      <span className="pk-step-node">
        {kind === "run" ? <span className="pk-run" /> : kind === "ok" ? <Icon name="check" size={12} /> : kind === "fail" ? <Icon name={step.status === "cancelled" ? "stop" : "x"} size={12} /> : null}
      </span>
      <div className="step-body">
        <button type="button" className="step-head" aria-expanded={open} onClick={onToggle} disabled={step.status === "pending"}>
          <b className="pk-step-name">{stepName(step)}</b>
          {step.status !== "pending" && <Icon name={open ? "triD" : "triR"} size={12} className="step-tri" />}
          <span className="pk-step-time">{time}</span>
        </button>
        {desc && <p className="pk-step-desc">{desc}</p>}
        {(step.status === "failed" || open) && step.notice && <p className="pk-step-desc step-notice">{step.notice}</p>}
        {showTail && (
          <div className="pk-mod pk-mod--sunk out sel">
            {tail.map((l) => <p key={l.id} className={l.stream === "stderr" ? "is-err" : undefined}>{l.chunk}</p>)}
          </div>
        )}
        {open && (
          <div className="pk-mod pk-mod--sunk out out--full sel">
            <div className="out-lines scroll">
              {logs.length === 0 ? <p className="is-dim">这一步还没有输出</p> : logs.map((l) => <p key={l.id} className={l.stream === "stderr" ? (step.status === "failed" ? "is-err" : "is-dim") : undefined}>{l.chunk}</p>)}
            </div>
            <div className="out-acts">
              {step.expects.length > 0 && <span className="hint">该写出：{step.expects.join("、")}</span>}
              <span className="grow" />
              {logs.length > 0 && <Btn v="ghost" sm onClick={copy}>{copied ? "COPIED" : "COPY"}</Btn>}
            </div>
          </div>
        )}
        {onRetry && <div className="step-retry"><Btn v="danger" sm icon="undo" onClick={onRetry}>{run.flowId.startsWith("api-") ? "CONTINUE" : "RETRY"}</Btn><span className="hint">{run.flowId.startsWith("api-") ? "从现在的文件接着发一个任务，不重放跑过的工具" : "修好以后，从这一步重跑"}</span></div>}
      </div>
    </li>
  );
};

/** `.peck/tasks.md` 的任务清单，默认收着。 */
const Checklist: React.FC<{ run: RunRecord }> = ({ run }) => {
  const [open, setOpen] = useState(false);
  const done = run.tasks.filter((t) => t.completed).length;
  return (
    <div className="fold">
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>任务清单</span>
        <em className="t-label">{done}/{run.tasks.length}</em>
      </button>
      {open && (
        <ul className="checklist sel">
          {run.tasks.map((t) => (
            <li key={t.id} className={t.completed ? "is-done" : undefined}>
              <span className={cn("pk-check", t.completed && "is-on")}><Icon name="check" size={12} /></span>
              <span>{t.text}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};

const REVIEW = { accept: "通过", rework: "返工", takeover: "接手" } as const;
const CALL_STATE: Record<string, string> = { running: "在跑", completed: "完成", failed: "失败", paused: "到上限停下", cancelled: "已停止", interrupted: "被打断" };

/** API 模式：每个模型的调用和工具记录、交给实施的模型的每一项和审查结果。默认收着。 */
const ApiHistory: React.FC<{ run: RunRecord; onShowFile: (path: string) => void }> = ({ run, onShowFile }) => {
  const [open, setOpen] = useState(false);
  const [tasks, setTasks] = useState<ApiTask[] | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!open) return;
    let stopped = false;
    let timerId = 0;
    const refresh = async () => {
      try { const next = await api.taskHistory(run.id); if (!stopped) { setTasks(next); setError(""); } }
      catch (e) { if (!stopped) setError(String(e)); }
      if (!stopped && run.status === "running") timerId = window.setTimeout(() => void refresh(), 1500);
    };
    void refresh();
    return () => { stopped = true; window.clearTimeout(timerId); };
  }, [open, run.id, run.status]);
  const root = tasks?.find((t) => !t.parentId);
  const sum = (pick: (t: ApiTask) => number | null) => (tasks && tasks.every((t) => pick(t) != null) ? tasks.reduce((n, t) => n + (pick(t) ?? 0), 0).toLocaleString() : "未知");
  return (
    <div className="fold">
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>调用和审查记录</span>
      </button>
      {open && (
        <div className="api-history sel">
          {error && <p className="is-err">读不出记录：{error}</p>}
          {!tasks && !error && <p className="hint">在读…</p>}
          {tasks && !root && <p className="hint">这个任务没有调用记录。</p>}
          {tasks && root && (
            <>
              <p className="hint">
                一共 {tasks.reduce((n, t) => n + t.round, 0)}/{root.limits.maxRounds} 轮 · {tasks.reduce((n, t) => n + t.tools.length, 0)}/{root.limits.maxToolCalls} 次工具 · 输入 {sum((t) => t.inputTokens)} / 输出 {sum((t) => t.outputTokens)} Token
              </p>
              {tasks.map((t) => (
                <details key={t.id} className={cn("api-task", t.parentId && "api-task--child")}>
                  <summary>
                    <Dot kind={agentDot("api")} />
                    <b className="t-name">{t.parentId ? `交给实施的模型：${t.title}` : t.settings.role === "supervisor" ? "规划的模型" : "单模型干活"}</b>
                    <span>{t.model.name} · {CALL_STATE[t.status] ?? t.status}{t.parentId ? ` · 第 ${t.attempt} 次` : ""}{t.review ? ` · ${REVIEW[t.review.decision]}` : ""}</span>
                  </summary>
                  <p className="hint">{t.round} 轮 · 输入 {t.inputTokens ?? "未知"} / 输出 {t.outputTokens ?? "未知"} Token · 思考 {t.settings.thinking ? t.settings.effort ?? "默认" : "关"}</p>
                  {t.paths?.length > 0 && <p className="hint">能改：<span className="t-code">{t.paths.join("、")}</span> · 验收：{t.acceptance}</p>}
                  {t.review && <p className="note">{REVIEW[t.review.decision]}：{t.review.feedback}</p>}
                  {t.summary && <p className="note">{t.summary}</p>}
                  {!!t.changeset?.files.length && <Btn v="ghost" sm onClick={() => onShowFile(t.changeset.files[0].path)}>DIFF · {t.changeset.files.length}</Btn>}
                  {t.tools.map((tool, i) => (
                    <details key={`${tool.callId}-${i}`} className="api-tool">
                      <summary className={tool.status === "failed" || tool.status === "interrupted" ? "is-err" : undefined}>
                        <span className="t-code">{i + 1}. {tool.tool.name}</span><span>{CALL_STATE[tool.status] ?? tool.status} · {clock(tool.startedAt)}</span>
                      </summary>
                      <pre className="t-code">{JSON.stringify(tool.tool.arguments, null, 2)}</pre>
                      <pre className="t-code">{tool.output || "还没有结果"}</pre>
                    </details>
                  ))}
                </details>
              ))}
              {root.status === "interrupted" && <p className="note">被打断的操作可能已经改过文件。接着做会新发一个任务、先核对文件，不重放旧的工具。</p>}
            </>
          )}
        </div>
      )}
    </div>
  );
};
