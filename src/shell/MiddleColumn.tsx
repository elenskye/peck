import React, { useEffect, useRef, useState } from "react";
import { api, type ApiStatus, type ApiTask } from "@/lib/api";
import type { AppSettings } from "@/lib/settings";
import type { AgentInfo, Message, Permission, ProjectOverview, RunRecord, StepDescriptor, TaskBrief } from "@/lib/types";
import { API_WHO, CLI_WHO, FAKE_WHO, PERMISSIONS, agentDot, clock, cn, runState, span, stepName, timer, whoDots, whoLabel, type WhoOption } from "@/lib/utils";
import { Blocks, Btn, Dot, DotCheck, Icon, ModHead, Seg, useNow } from "@/ui";

interface Props {
  project: ProjectOverview | null;
  task: TaskBrief | null;
  runs: RunRecord[];
  messages: Message[];
  /** 这个项目有任务在跑 */
  busy: boolean;
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  agents: AgentInfo[];
  apiStatus: ApiStatus | null;
  draft: string;
  onDraft: (text: string) => void;
  onSend: (prompt: string, who: string, permission: Permission) => void;
  onStop: () => void;
  onRetryStep: (run: RunRecord, stepId: string) => void;
  onOpen: (path?: string) => void;
  onOpenSettings: () => void;
  /** 右栏现在看的是哪一次记录 */
  shownRunId: string | null;
  /** 让右栏看这一次的改动（带上文件就直接开它的 Diff） */
  onShowRun: (runId: string, path?: string) => void;
  native: boolean;
}

/** 这个模式下能选的「谁来干」：CLI 检测到才出现；假 Agent 只在开发者选项开着时出现。 */
function whoOptions(mode: "cli" | "api", agents: AgentInfo[], fake: boolean): WhoOption[] {
  if (mode === "api") return API_WHO;
  const found = (id: string) => agents.some((a) => a.id === id && a.available && !a.isMock);
  const cli = CLI_WHO.filter((w) => w.id.split(">").every(found));
  return fake ? [...cli, FAKE_WHO] : cli;
}

export const MiddleColumn: React.FC<Props> = (props) => {
  const { project, task, runs, settings, onSettings, agents, apiStatus, busy } = props;
  const [permission, setPermission] = useState<Permission>(settings.permission);
  useEffect(() => setPermission(settings.permission), [settings.permission]);

  if (!project) {
    return (
      <section className="col" aria-label="对话">
        <div className="pk-mod col-grow home">
          <ModHead label="PECK" note="本机的 Coding Agent" />
          <h1 className="t-hero">先选一个项目</h1>
          <p className="lead">在左边点一个项目，或者打开一个项目文件夹。给它一句话，Peck 调本机的 Muse、Grok 或 API 模型去改代码；改动看得见，整个任务能一键撤销。</p>
          {props.native && <div><Btn v="primary" icon="folder" onClick={() => props.onOpen()}>OPEN FOLDER</Btn></div>}
        </div>
      </section>
    );
  }

  const options = whoOptions(settings.mode, agents, settings.fakeAgent);
  const remembered = settings.who[settings.mode];
  const who = options.find((o) => o.id === remembered) ?? options[0] ?? null;
  const pickWho = (id: string) => onSettings({ who: { ...settings.who, [settings.mode]: id } });
  const access = PERMISSIONS.find((p) => p.id === permission)!;
  const modelName = (role: "worker" | "supervisor") => {
    const id = apiStatus?.config.agents.find((a) => a.role === role)?.modelId;
    return apiStatus?.config.models.find((m) => m.id === id)?.name ?? "还没选模型";
  };

  const composer = (
    <Composer
      {...props}
      options={options}
      who={who}
      onWho={pickWho}
      permission={permission}
      onPermission={setPermission}
      placeholder={task ? (busy ? "下一句话…（这个任务跑完再发）" : "下一句话…") : "一句话说要做什么…"}
    />
  );

  if (!task) {
    return (
      <section className="col" aria-label="对话">
        <div className="pk-mod col-grow home scroll">
          <ModHead label="NEW TASK" note={settings.mode === "api" ? `${project.workspace.name} · API 模式` : project.workspace.name} />
          <h1 className="t-hero">{project.workspace.name} 要做什么？</h1>
          <p className="lead">
            {settings.mode === "api"
              ? "API 模式：Peck 自己调模型干活，读写只在项目里。开始前先存一份快照，做完默认留着；不满意，一键撤销整个任务。"
              : "一句话说清楚，再选谁来干。开始前先存一份快照，做完默认留着；不满意，一键撤销整个任务。"}
          </p>
          {who ? (
            <div>
              <p className="lbl">WHO · {who.label}</p>
              <ol className={cn("flow", who.flow.length === 1 && "flow--one")}>
                {who.flow.map((f, i) => (
                  <li key={i}>
                    <p><span className="flow-k">{i + 1}</span><Dot kind={f.dot} /><b className="t-name">{f.name}</b>{settings.mode === "api" && <em>{modelName(who.id === "api-supervisor" && i === 0 ? "supervisor" : "worker")}</em>}</p>
                    <span>{f.does}</span>
                  </li>
                ))}
              </ol>
            </div>
          ) : (
            <div className="empty">
              <p>没检测到能用的 CLI。到设置里看一下 CLI 检测，或者把顶栏的模式换成 API。</p>
              <div><Btn icon="gear" onClick={props.onOpenSettings}>SETTINGS</Btn></div>
            </div>
          )}
          <div>
            <p className="lbl">ACCESS · {access.label}</p>
            <p className="note">{access.does}</p>
            <p className="note note--quiet">这一版权限档只记在任务上；管住 CLI 和工具从 v0.5 起。</p>
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
            <p className="you sel">
              <span className="t-label">YOU</span>
              <span className="t-name you-text">{run.prompt}</span>
              <span className="you-meta">{clock(run.startedAt)}</span>
            </p>
            <TaskModule
              run={run}
              latest={i === runs.length - 1}
              shown={run.id === props.shownRunId}
              onShow={runs.length > 1 ? () => props.onShowRun(run.id) : undefined}
              onRetryStep={props.onRetryStep}
              onShowFile={(path) => props.onShowRun(run.id, path)}
              busy={busy}
            />
          </React.Fragment>
        ))}
      </div>
      {composer}
    </section>
  );
};

// ---------- 输入位 ----------

const Composer: React.FC<Props & {
  options: WhoOption[];
  who: WhoOption | null;
  onWho: (id: string) => void;
  permission: Permission;
  onPermission: (p: Permission) => void;
  placeholder: string;
}> = ({ draft, onDraft, onSend, onStop, busy, options, who, onWho, permission, onPermission, placeholder, settings, apiStatus, onOpenSettings }) => {
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 132)}px`;
  }, [draft]);
  const ready = !!draft.trim() && !!who && !busy;
  const submit = () => { if (ready && who) onSend(draft.trim(), who.id, permission); };
  const apiModel = (role: "worker" | "supervisor") => {
    const id = apiStatus?.config.agents.find((a) => a.role === role)?.modelId;
    return (apiStatus?.config.models.find((m) => m.id === id)?.name ?? "还没选模型").toUpperCase();
  };
  return (
    <div className="pk-mod composer" onKeyDown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === "Enter") { e.preventDefault(); submit(); } }}>
      <label className={cn("composer-in", busy && "is-off")}>
        <span className="pk-prompt">›</span>
        <textarea
          ref={ref}
          rows={1}
          aria-label="一句话说要做什么"
          value={draft}
          disabled={busy}
          placeholder={placeholder}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
          onChange={(e) => onDraft(e.target.value)}
        />
      </label>
      <div className="composer-row" role="group" aria-label="谁来干">
        {options.map((o) => (
          <button key={o.id} type="button" disabled={busy} className={cn("pk-chip pk-chip--sm", who?.id === o.id && "is-on")} aria-pressed={who?.id === o.id} onClick={() => onWho(o.id)}>
            {o.flow.map((f, i) => <React.Fragment key={i}>{i > 0 && "→"}<Dot kind={f.dot} />{o.flow.length > 1 ? f.name.toUpperCase() : null}</React.Fragment>)}
            {(o.flow.length === 1 || settings.mode === "api") && o.label}
          </button>
        ))}
        {options.length === 0 && <span className="hint">没检测到能用的 CLI</span>}
        {settings.mode === "api" && who && (
          <button type="button" className="pk-chip pk-chip--sm" onClick={onOpenSettings} aria-label="换模型（在设置里）">
            <Dot kind="api" />{who.id === "api-supervisor" ? `${apiModel("supervisor")} → ${apiModel("worker")}` : apiModel("worker")}
          </button>
        )}
      </div>
      <div className="composer-row">
        <Seg label="权限" sm disabled={busy} items={PERMISSIONS.map((p) => ({ id: p.id, label: p.label }))} value={permission} onChange={onPermission} />
        <span className="grow" />
        {busy ? (
          <Btn v="danger" icon="stop" onClick={onStop}>STOP</Btn>
        ) : (
          <>
            <span className="kbd">⌘↵</span>
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
}> = ({ run, latest, shown, onShow, busy, onRetryStep, onShowFile }) => {
  const state = runState(run);
  const running = state === "running";
  const now = useNow(running);
  const doneSteps = run.steps.filter((s) => s.status === "succeeded").length;
  const doneTasks = run.tasks.filter((t) => t.completed).length;
  // 进度：有任务清单就按清单，没有按步骤；在跑的那一步算半步。
  const ratio = run.tasks.length > 0 ? doneTasks / run.tasks.length : run.steps.length > 0 ? doneSteps / run.steps.length : 0;
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
        count={`${doneSteps}/${run.steps.length}`}
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
      {run.tasks.length > 0 && <Checklist run={run} />}
      <ol className="steps">
        {run.steps.map((step) => (
          <Step key={step.id} run={run} step={step} now={now} open={open === step.id} onToggle={() => setOpen(open === step.id ? null : step.id)} onRetry={retryable && (step.status === "failed" || step.status === "cancelled") ? () => onRetryStep(run, step.id) : undefined} />
        ))}
      </ol>
      {run.flowId.startsWith("api-") && <ApiHistory run={run} onShowFile={onShowFile} />}
      {state === "succeeded" && (
        <div className="summary sel">
          <div className="summary-top">
            <DotCheck />
            <div><b className="t-name">做完了</b><span>{run.files.length} 个文件 · 用了 {span(elapsed)}</span></div>
            <p className="summary-n"><span className="pk-add">+{adds}</span><span className="pk-del">−{dels}</span></p>
          </div>
          {run.summary && <p className="summary-text">{run.summary}</p>}
          <div className="verify">
            {run.tasks.length > 0 && <p className={doneTasks === run.tasks.length ? "is-ok" : "is-warn"}><Icon name={doneTasks === run.tasks.length ? "check" : "bang"} size={12} />任务清单 {doneTasks}/{run.tasks.length} 条勾了</p>}
            {run.steps.map((s) => <p key={s.id} className="is-ok"><Icon name="check" size={12} />{stepName(s)}{s.exitCode != null ? ` · 退出码 ${s.exitCode}` : ""}</p>)}
          </div>
          <p className="keep">默认留着。不满意，在右边撤销整个任务。</p>
        </div>
      )}
      {(state === "failed" || state === "cancelled") && run.summary && (
        <div className={cn("pk-bar", state === "failed" && "pk-bar--bad", "task-bar sel")}>
          <Icon name={state === "failed" ? "x" : "stop"} />
          <span className="pk-bar-text">{run.summary}</span>
        </div>
      )}
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
