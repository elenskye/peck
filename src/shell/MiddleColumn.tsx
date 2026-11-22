import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type ApiTask } from "@/lib/api";
import { canAutomate, type AgentEvent, type Control, type ModelEntry } from "@/lib/agentEvents";
import { CONTEXT_PARTS, addUsage, costCell, costRows, foldEvents, minusUsage, splitNextSteps, tokenCell, tokens, usageText, usd, type ContextView, type CostRow, type UsageLine } from "@/lib/agentView";
import { ipc, type ImageInput, type SlashList, type VerifyResult, type VerifyState } from "@/lib/ipc";
import { CLI_ONLY, PECK_COMMANDS, lastChanged, matchSlash, parseSlash, reviewPrompt, slashAt, type SlashItem } from "@/lib/slash";
import type { AppSettings } from "@/lib/settings";
import type { QuotaHint } from "@/lib/quota";
import { canDelegate } from "@/lib/presets";
import type { Permission, ProjectOverview, RunRecord, StepDescriptor, TaskBrief } from "@/lib/types";
import { PERMISSIONS, agentDot, ago, clock, cn, copyText, engineDot, entryDot, isAgentRun, isPeckRun, runState, span, stepName, timer, whoDots, whoLabel , shortenHomePath } from "@/lib/utils";
import { Blocks, Btn, Dot, DotCheck, Float, Icon, Info, MenuChip, ModHead, Seg, useNow } from "@/ui";
import { AgentView } from "@/shell/AgentView";
import { Md } from "@/shell/Markdown";
import { MarkdownEditor } from "@/shell/MarkdownEditor";
import { useScrollMemory } from "@/lib/scrollMemory";
import { RELAY_ID, cliModelOf, pickable, sendEffort, whoShort, whoText } from "@/shell/ModelPicker";

interface Props {
  project: ProjectOverview;
  task: TaskBrief | null;
  runs: RunRecord[];
  /** 这个项目有任务在跑 */
  busy: boolean;
  settings: AppSettings;
  /** 模型列表（gateway_models） */
  entries: ModelEntry[];
  /** 统一事件：任务号 → 这次的过程 */
  agentEvents: Record<string, AgentEvent[]>;
  /** 在跑的任务号（这个项目的） */
  runningRunId: string | null;
  draft: string;
  onDraft: (text: string) => void;
  onSend: (prompt: string, who: string, permission: Permission, effort?: string, model?: string, goal?: string[], worktree?: boolean, images?: ImageInput[]) => Promise<boolean>;
  /** 往某一次的事件里追加一条（批完「想记住这几条」）。 */
  onAppendEvent: (runId: string, event: AgentEvent) => void;
  onStop: () => void;
  onRetryStep: (run: RunRecord, stepId: string) => void;
  onControl: (runId: string, control: Control) => Promise<void>;
  onOpenSettings: () => void;
  /** 输入框下点了「谁」：展开、闪一下左栏的 AGENTS。 */
  onShowAgents: () => void;
  /** 项目文件夹不在了：重新找文件夹、移出列表。 */
  onRelocate: (projectId: string) => void;
  onRemoveProject: (projectId: string) => void;
  /** 归档的任务拿回左栏（拿回来才能接着发）。 */
  onRestoreTask: (taskId: string) => void;
  /** 额度提醒（v0.9 B3）：这次要用到的 CLI 快用完了，输入框上方一行。 */
  quota: QuotaBar | null;
  /** `/new`（v0.10 C2）：这个项目里新开一个任务。 */
  onNewTask: () => void;
  /** `/undo`：撤销某一次的改动。 */
  onUndo: (run: RunRecord) => Promise<void>;
  /** 项目的设置变了（NOT NOW、确认了验证命令）：重读项目列表。 */
  onProjectsChanged: () => void;
  /** 右栏现在看的是哪一次记录 */
  shownRunId: string | null;
  /** 让右栏看这一次的改动（带上文件就直接开它的 Diff） */
  onShowRun: (runId: string, path?: string) => void;
  say: (message: unknown) => void;
}

/** 额度提醒那一行（v0.9 B3）：「这次换」只换下一次发出去的；「以后都换」改 AGENTS（和套用着的预设）。 */
export interface QuotaBar {
  hint: QuotaHint;
  /** 点过「这次换」：换成了谁。 */
  swapped: ModelEntry | null;
  /** 套用着的预设（「以后都换」顺带改它）。 */
  presetName: string | null;
  onOnce: () => void;
  onAlways: () => void;
  onUndo: () => void;
  onDismiss: () => void;
}

const QuotaLine: React.FC<{ quota: QuotaBar }> = ({ quota: { hint, swapped, presetName, onOnce, onAlways, onUndo, onDismiss } }) => (
  <div className="pk-bar pk-bar--wait quota-bar" role="status">
    <Icon name={swapped ? "clock" : "bang"} size={12} />
    {swapped ? (
      <>
        <span className="pk-bar-text">{hint.text}：这一次用 {swapped.label} 代替（设置没改）</span>
        <Btn sm v="ghost" onClick={onUndo}>UNDO</Btn>
      </>
    ) : (
      <>
        <span className="pk-bar-text">{hint.text}{hint.to ? `，换成 ${hint.to.label}？` : "，别家的额度也读不到或不够"}</span>
        {hint.to && <Btn sm onClick={onOnce}>这次换</Btn>}
        {hint.to && <Btn sm v="ghost" onClick={onAlways}>{presetName ? `改进「${presetName}」` : "以后都换"}</Btn>}
        <button type="button" className="head-icon" aria-label="不提醒这一条" title="不提醒这一条" onClick={onDismiss}><Icon name="x" size={12} /></button>
      </>
    )}
  </div>
);

/** 选中的「谁来干」：记住的那个还能用就用它，不然列表里第一个。 */
export function chosenEntry(entries: ModelEntry[], settings: AppSettings): ModelEntry | null {
  const list = pickable(entries);
  return list.find((e) => e.id === settings.pick) ?? list.find((e) => e.group === "peck") ?? list[0] ?? null;
}

/** 一项怎么干（NEW TASK 上的一行）。 */
function describeOne(entry: ModelEntry): { name: string; dot: ReturnType<typeof agentDot>; does: string } {
  if (entry.group === "peck") {
    return { name: `Peck · ${entry.label}`, dot: "api", does: "Peck 自己的循环：读、搜、改（整批一起改）、跑命令、任务清单、问你，都用 Peck 的工具；每一步看得见，花费按价格表算，算进花费上限。" };
  }
  return {
    name: entry.label,
    dot: engineDot(entry.id.slice(4)),
    does: entry.directOnly
      ? "直接调用：你发一句话，它自己做完；Peck 显示每一步和改动，要批的事弹卡片。不进 ONE SHOT 和任何自动化。"
      : `整包引擎：用它自己的工具和订阅额度；Peck 显示每一步、替它审批、记花费${entry.account ? `（账号：${entry.account}）` : ""}。`,
  };
}

/** 选了谁，NEW TASK 上写它怎么干：接力写两家，开了审查、委派各多一行。 */
function describe(entry: ModelEntry, entries: ModelEntry[], settings: AppSettings): Array<{ name: string; dot: ReturnType<typeof agentDot>; does: string; model?: string }> {
  const find = (id?: string) => (id ? entries.find((e) => e.id === id) : undefined);
  const out: Array<{ name: string; dot: ReturnType<typeof agentDot>; does: string; model?: string }> = [];
  let builder: ModelEntry | undefined = entry;
  if (entry.id === RELAY_ID) {
    const spec = find(settings.relay.spec);
    builder = find(settings.relay.build);
    out.push(spec ? { ...describeOne(spec), does: "先只读地看项目，写一份实施规范交给下一个。" } : { name: "写规范的", dot: "", does: "在 AGENTS 里选一个。" });
    out.push(builder ? { ...describeOne(builder), does: `照规范动手改。${describeOne(builder).does}` } : { name: "实施的", dot: "", does: "在 AGENTS 里选一个。" });
  } else {
    out.push(describeOne(entry));
  }
  const reviewer = find(settings.review);
  if (reviewer) out.push({ ...describeOne(reviewer), does: "做完以后只读地看这次的 Diff；说要改就交回实施的那一个修一次（PLAN 不审查）。" });
  const sub = find(settings.delegate);
  if (sub && canDelegate(builder)) out.push({ name: `委派 · ${sub.label}`, dot: "api", does: "写得清楚的子任务交给它（同样的工具和权限），一个任务最多 6 次。" });
  return out;
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
        <p className="lead">找不到 <span className="t-code">{shortenHomePath(workspace.path)}</span><Info text={`可能被删了、挪了地方，或者外接盘没插（插上就好，不用动）。${tasks.length ? `这个项目的 ${tasks.length} 个任务和快照都还在，` : ""}换成它现在的位置就能接着用；移出列表不删记录，重新打开这个文件夹就回来。`} /></p>
        <div className="set-inline">
          <Btn v="primary" icon="folder" onClick={() => onRelocate(workspace.id)}>FIND FOLDER</Btn>
          <Btn v={armed ? "danger" : "ghost"} icon="trash" onClick={() => { if (!armed) { setArmed(true); return; } onRemove(workspace.id); }}>{armed ? "CONFIRM" : "REMOVE"}</Btn>
          {armed && <span className="hint">再点一次：移出列表</span>}
        </div>
      </div>
    </section>
  );
};

/** 这个项目的 .peck/verify.toml（切项目、任务跑完时重读）。按项目记：切过去以后、读回来之前不拿上一个项目的。 */
function useVerify(projectId: string | null, busy: boolean, say: (m: unknown) => void): [VerifyState | null, (id: string, v: VerifyState) => void] {
  const [held, setHeld] = useState<{ id: string; state: VerifyState } | null>(null);
  const sayRef = useRef(say);
  sayRef.current = say;
  useEffect(() => {
    if (!projectId) { setHeld(null); return; }
    let stop = false;
    ipc.verifyGet(projectId).then((v) => { if (!stop) setHeld({ id: projectId, state: v }); }).catch((e) => { if (!stop) { setHeld(null); sayRef.current(e); } });
    return () => { stop = true; };
  }, [projectId, busy]);
  const set = useCallback((id: string, v: VerifyState) => setHeld({ id, state: v }), []);
  return [held && held.id === projectId ? held.state : null, set];
}

/** 第一次打开项目：Peck 猜的验证命令，站主确认（能改）才写进 .peck/verify.toml。 */
const VerifyBanner: React.FC<{ projectId: string; state: VerifyState; onState: (id: string, v: VerifyState) => void; onChanged: () => void; say: (m: unknown) => void }> = ({ projectId, state, onState, onChanged, say }) => {
  // 没有文件：Peck 猜的；有文件但没确认过（项目自带的、模型或别人改过的）：现在的原文。
  const [text, setText] = useState(state.guessed ?? state.text ?? "");
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const config = useMemo(() => parseGuess(text), [text]);
  const act = async (f: () => Promise<VerifyState>) => {
    setBusy(true);
    try { onState(projectId, await f()); onChanged(); } catch (e) { say(e); } finally { setBusy(false); }
  };
  return (
    <div className="pk-mod pk-mod--wait verify-guess">
      <ModHead label="VERIFY" note={<>{state.exists ? "还没确认的验证命令" : "Peck 猜的验证命令"}<Info text={state.exists
        ? "项目里的 .peck/verify.toml 是自带的或者改过了：确认了 Peck 才在后台替模型跑这些、才起 dev server。"
        : "改了文件，Peck 在后台替模型跑这些；模型停下时没过，就把失败交回去修（最多 3 次）。确认了才写进 .peck/verify.toml，以后在项目设置里改。"} /></>} />
      {editing ? (
        <textarea className="verify-text" value={text} spellCheck={false} onChange={(e) => setText(e.target.value)} rows={Math.min(16, text.split("\n").length + 1)} aria-label="verify.toml" />
      ) : (
        <ul className="verify-list">
          {config.checks.map((c) => <li key={c.name}><b className="t-name">{c.name}</b><span className="t-code">{c.run}</span>{c.when && <em>{c.when}</em>}</li>)}
          {config.servers.map((c) => <li key={c.name}><b className="t-name">{c.name}</b><span className="t-code">{c.run}</span><em>dev server</em></li>)}
        </ul>
      )}
      <div className="set-inline">
        <Btn v="primary" icon="check" disabled={busy || !text.trim()} onClick={() => void act(() => ipc.verifySave(projectId, text))}>CONFIRM</Btn>
        <Btn v="ghost" icon="pen" disabled={busy} onClick={() => setEditing(!editing)}>{editing ? "DONE" : "EDIT"}</Btn>
        <span className="grow" />
        <Btn v="ghost" disabled={busy} onClick={() => void act(() => ipc.verifyDismiss(projectId))}>NOT NOW</Btn>
      </div>
    </div>
  );
};

/** 猜的 TOML 粗读一遍给站主看（名字、命令、when）；写错了照样能 EDIT，CONFIRM 时由 Peck 校验。 */
function parseGuess(text: string): { checks: { name: string; run: string; when?: string }[]; servers: { name: string; run: string }[] } {
  const checks: { name: string; run: string; when?: string }[] = [];
  const servers: { name: string; run: string }[] = [];
  let cur: { name: string; run: string; when?: string } | null = null;
  for (const line of text.split("\n")) {
    const t = line.trim();
    if (t === "[[check]]") { cur = { name: "", run: "" }; checks.push(cur); continue; }
    if (t === "[[server]]") { cur = { name: "", run: "" }; servers.push(cur); continue; }
    const m = t.match(/^(name|run|when)\s*=\s*(.+)$/);
    if (!cur || !m) continue;
    const v = m[2].replace(/^"|"$/g, "").replace(/\\"/g, '"');
    if (m[1] === "name") cur.name = v;
    else if (m[1] === "run") cur.run = v;
    else cur.when = m[2].trim().replace(/^\[|\]$/g, "").split(/"\s*,\s*"/).map((x) => x.replace(/"/g, "").trim()).join("  ");
  }
  return { checks, servers };
}

export const MiddleColumn: React.FC<Props> = (props) => {
  const { project, task, runs, settings, entries, busy } = props;
  const [permission, setPermission] = useState<Permission>(settings.permission);
  useEffect(() => setPermission(settings.permission), [settings.permission]);
  const projectId = project.missing ? null : project.workspace.id;
  const [verify, setVerify] = useVerify(projectId, busy, props.say);
  // 对话的位置记忆（v1.0.1）：进来到最新一轮、回来回到上次的位置。
  const stackRef = useRef<HTMLDivElement>(null);
  useScrollMemory(stackRef, task?.id ?? null, runs.length);

  // 归档的任务只读：文件夹不在了也看得了记录。
  const archivedAt = task?.archivedAt ?? null;
  if (project.missing && !archivedAt) return <MissingFolder project={project} onRelocate={props.onRelocate} onRemove={props.onRemoveProject} />;

  const who = chosenEntry(entries, settings);
  const composer = (
    <Composer
      {...props}
      who={who}
      verify={verify}
      permission={permission}
      onPermission={setPermission}
      placeholder={task ? (busy ? "下一句话…（这个任务跑完再发）" : "下一句话…") : "一句话说要做什么…"}
    />
  );

  if (!task) {
    const flow = who ? describe(who, entries, settings) : [];
    return (
      <section className="col" aria-label="对话">
        <div className="pk-mod col-grow home scroll">
          <ModHead label="NEW TASK" />
          <h1 className="t-hero">{project.workspace.name} 要做什么？<Info text="一句话说清楚，谁来干在左栏 AGENTS 里选。开始前先存一份快照，做完默认留着；不满意，一键撤销整个任务。WORKTREE 是在项目的一个 git worktree 里做，做完再搬回。" /></h1>
          {who ? (
            <div>
              <p className="lbl">WHO</p>
              <ol className={cn("flow", flow.length === 1 && "flow--one")}>
                {flow.map((f, i) => (
                  <li key={i}>
                    <p><span className="flow-k">{i + 1}</span><Dot kind={f.dot} /><b className="t-name">{f.name}</b>{f.model && <em>{f.model}</em>}<Info text={f.does} /></p>
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
          {verify && !verify.dismissed && !verify.error && ((!verify.exists && verify.guessed) || (verify.exists && !verify.approved)) && projectId && (
            <VerifyBanner key={`${projectId}:${verify.guessed ?? verify.text}`} projectId={projectId} state={verify} onState={setVerify} onChanged={props.onProjectsChanged} say={props.say} />
          )}
          {verify?.error && <div className="pk-bar pk-bar--bad task-bar sel"><Icon name="bang" /><span className="pk-bar-text">{verify.error} 在项目设置里改好之前，Peck 不替模型跑验证命令。</span></div>}
        </div>
        {composer}
      </section>
    );
  }

  return (
    <section className="col" aria-label="对话">
      <div className="col-grow scroll task-stack" ref={stackRef}>
        {runs.length === 0 && <div className="pk-mod"><p className="empty-line">在读这个任务…</p></div>}
        {runs.map((run, i) => (
          <React.Fragment key={run.id}>
            <div className="you sel">
              <span className="t-label">YOU</span>
              <Md text={run.prompt} className="you-text" />
            </div>
            <TaskModule
              run={run}
              // 归档的只能看：不给「继续」、下一步、RETRY，卡片也不能批（拿回左栏才能接着发；v0.10 收尾审查）。
              latest={i === runs.length - 1 && !archivedAt}
              readOnly={!!archivedAt}
              shown={run.id === props.shownRunId}
              onShow={runs.length > 1 ? () => props.onShowRun(run.id) : undefined}
              onRetryStep={props.onRetryStep}
              onShowFile={(path) => props.onShowRun(run.id, path)}
              busy={busy}
              events={props.agentEvents[run.id]}
              live={props.runningRunId === run.id}
              onControl={(c) => props.onControl(run.id, c)}
              onAppend={(e) => props.onAppendEvent(run.id, e)}
              onNext={(text) => props.onDraft(props.draft.trim() ? `${props.draft.trimEnd()}\n${text}` : text)}
              say={props.say}
            />
          </React.Fragment>
        ))}
      </div>
      {archivedAt && task ? (
        <div className="pk-bar archived-bar" role="status">
          <Icon name="archive" />
          <span className="pk-bar-text">{ago(archivedAt)} 归档的，只能看<Info text="归档的任务不在左栏列，记录、快照都留着，模型查旧任务照样查得到。拿回左栏才能接着发。" /></span>
          <Btn sm v="primary" icon="undo" onClick={() => props.onRestoreTask(task.id)}>RESTORE</Btn>
        </div>
      ) : composer}
    </section>
  );
};

/** 这个任务一共花了多少（几次加起来）。 */
function taskUsage(runs: RunRecord[], events: Record<string, AgentEvent[]>): UsageLine | null {
  let total: UsageLine | null = null;
  // CLI 续接同一个会话时报的是累计（v0.10 查花费时补的）：同一家同一个会话，这一次不比上一次少就减掉上一次的。
  const lastBySession = new Map<string, UsageLine>();
  for (const r of runs) {
    const ev = events[r.id];
    if (!ev) continue;
    const view = foldEvents(ev);
    let u = view.usage;
    if (!u) continue;
    if (view.cumulativeUsage && view.sessionRef) {
      const key = `${view.engine}:${view.sessionRef}`;
      const prev = lastBySession.get(key);
      lastBySession.set(key, u);
      if (prev && (u.input ?? 0) >= (prev.input ?? 0)) u = minusUsage(u, prev);
    }
    total = total ? addUsage(total, u) : u;
  }
  return total;
}

/** 这个任务最新的一份上下文大小（最后一次报的）。 */
function taskContext(runs: RunRecord[], events: Record<string, AgentEvent[]>): ContextView | null {
  for (let i = runs.length - 1; i >= 0; i--) {
    const ev = events[runs[i].id];
    if (!ev) continue;
    for (let j = ev.length - 1; j >= 0; j--) {
      const e = ev[j];
      if (e.type === "context.updated") return { used: e.used, window: e.window ?? null, compactAt: e.compactAt ?? null, parts: e.parts ?? [] };
    }
  }
  return null;
}

/** 输入框旁的用量小环：12 个点，亮几个是用了窗口的几成；到了压缩线变琥珀。点开看分成哪几块、压缩线在哪。 */
const ContextRing: React.FC<{ context: ContextView }> = ({ context }) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const ratio = context.window ? Math.min(1, context.used / context.window) : 0;
  const line = context.window && context.compactAt ? context.compactAt / context.window : 0.8;
  const warn = ratio >= line;
  const lit = Math.max(context.used > 0 ? 1 : 0, Math.round(ratio * 12));
  const pct = (n: number) => `${Math.round(n * 100)}%`;
  const total = context.parts.reduce((n, p) => n + p.tokens, 0) || 1;
  // 12 个点围成一圈（像素格，16×16）
  const dots = Array.from({ length: 12 }, (_, i) => {
    const a = (i / 12) * Math.PI * 2 - Math.PI / 2;
    return { x: Math.round(7 + Math.cos(a) * 6), y: Math.round(7 + Math.sin(a) * 6), on: i < lit };
  });
  return (
    <>
      <button ref={ref} type="button" className={cn("ctx-ring", warn && "is-warn")} aria-label={`上下文用了 ${pct(ratio)}`} title={`上下文 ${tokens(context.used)}${context.window ? ` / ${tokens(context.window)}` : ""}`} aria-expanded={open} onClick={() => setOpen(!open)}>
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          {dots.map((d, i) => <rect key={i} x={d.x} y={d.y} width="2" height="2" className={d.on ? "is-on" : undefined} />)}
        </svg>
      </button>
      {open && (
        <Float anchor={ref} side="top" align="end" onClose={() => setOpen(false)} className="pk-anim-reveal-s menu-pop menu-pop--fixed ctx-pop" role="dialog" aria-label="上下文">
          <div className="pk-mod">
            <ModHead label="CONTEXT" note={context.window ? `${tokens(context.used)} / ${tokens(context.window)} · ${pct(ratio)}` : tokens(context.used)} />
            {context.window != null && (
              <div className="ctx-bar" aria-hidden="true">
                {Array.from({ length: 20 }, (_, i) => <i key={i} className={cn(i < Math.round(ratio * 20) && (warn ? "is-warn" : "is-on"), i === Math.round(line * 20) - 1 && "is-line")} />)}
              </div>
            )}
            <ul className="ctx-parts">
              {context.parts.map((p) => (
                <li key={p.key}><span>{CONTEXT_PARTS[p.key] ?? p.key}</span><b className="t-code">{tokens(p.tokens)}</b><em>{pct(p.tokens / total)}</em></li>
              ))}
            </ul>
            <p className="note">压缩线 {pct(line)}{context.compactAt ? `（${tokens(context.compactAt)}）` : ""}：到了先把长输出落盘、再清掉旧的工具输出，还不够就让模型写一份交接摘要接着做。</p>
          </div>
        </Float>
      )}
    </>
  );
};

/** 输入框里的 `@`：在末尾打 @ 加几个字，列出这个项目的旧任务，点一个插进去（发出去时 Peck 带上那个任务的经过）。 */
function mentionAt(draft: string): { start: number; query: string } | null {
  const m = draft.match(/(^|\s)@([^\s@[\]()]{0,24})$/);
  if (!m) return null;
  return { start: draft.length - m[2].length - 1, query: m[2] };
}

/** 输入框上方弹出的列表（@ 旧任务、/ 命令）：和输入框一样宽（左右各缩 16），交给 Float 摆（上面放不下就压矮）。 */
const AbovePop: React.FC<{ anchor: React.RefObject<HTMLDivElement | null>; label: string; children: React.ReactNode }> = ({ anchor, label, children }) => (
  <Float anchor={anchor} side="top" inset={16}>
    <div className="pk-menu mention-pop" role="listbox" aria-label={label}>{children}</div>
  </Float>
);

// ---------- 斜杠命令的结果（v0.10 C2） ----------

interface SlashNote {
  title: string;
  text: string;
  results?: VerifyResult[];
  confirm?: { label: string; run: () => Promise<void> };
}

/** 输入框上方一块：/verify、/cost、/compact 的结果，/undo 的确认。 */
const SlashNoteBar: React.FC<{ note: SlashNote; onClose: () => void; say: (m: unknown) => void }> = ({ note, onClose, say }) => {
  const [busy, setBusy] = useState(false);
  const bad = note.results?.some((r) => !r.passed);
  return (
    <div className={cn("pk-bar slash-note", bad ? "pk-bar--bad" : note.results ? "pk-bar--ok" : note.confirm ? "" : "slash-note--quiet")} role="status">
      <b className="t-label">{note.title}</b>
      <div className="slash-note-body">
        <p>{note.text}</p>
        {note.results && (
          <ul className="slash-results">
            {note.results.map((r) => (
              <li key={r.name}>
                <Icon name={r.passed ? "check" : "x"} size={12} />
                <b>{r.name}</b>
                <span className="t-code">{r.command}</span>
                <em>{r.passed ? "过了" : r.timedOut ? "超时" : `退出码 ${r.exitCode ?? "?"}`} · {(r.durationMs / 1000).toFixed(1)}s</em>
                {!r.passed && r.output.trim() && <details><summary>输出</summary><pre className="t-code">{r.output}</pre></details>}
              </li>
            ))}
          </ul>
        )}
      </div>
      {note.confirm && <Btn sm v="danger" disabled={busy} onClick={() => { setBusy(true); void note.confirm!.run().catch(say).finally(() => setBusy(false)); }}>{note.confirm.label}</Btn>}
      <button type="button" className="head-icon" aria-label="关上" title="关上" onClick={onClose}><Icon name="x" size={12} /></button>
    </div>
  );
};

// ---------- 输入位 ----------

const Composer: React.FC<Props & {
  who: ModelEntry | null;
  verify: VerifyState | null;
  permission: Permission;
  onPermission: (p: Permission) => void;
  placeholder: string;
}> = ({ draft, onDraft, onSend, onStop, busy, who, permission, onPermission, placeholder, settings, onShowAgents, entries, runs, agentEvents, task, project, verify, say, quota, onNewTask, onUndo }) => {
  const [goal, setGoal] = useState<string | null>(null);
  // 独立工作区（v0.7）：git 项目、Peck 循环或 CLI 引擎、不是 PLAN；发出去以后关掉（一次一选）。
  const [worktree, setWorktree] = useState(false);
  const worktreeable = !!project.workspace.gitBranch && !!who && (who.group === "peck" || who.group === "cli") && permission !== "plan";
  useEffect(() => { if (!worktreeable) setWorktree(false); }, [worktreeable]);
  // 换了项目：上一个项目选的目标不带过来。
  const projectKey = project.workspace.id;
  useEffect(() => setGoal(null), [projectKey]);
  // 只认站主确认过的那一份（项目自带的、改过的，确认前不跑、不起）。
  const checks = (verify?.approved && verify.config?.check) || [];
  // 目标模式：Peck 循环和 CLI（v0.10 C1；Antigravity 这类直接调用的不进）；PLAN 只读，不开。
  const goalable = !!who && (who.group === "peck" || (who.group === "cli" && !who.directOnly)) && checks.length > 0 && permission !== "plan";
  useEffect(() => { if (!goalable) setGoal(null); }, [goalable]);
  const ready = !!draft.trim() && !!who && !busy;
  // Esc 关掉的那一稿不再弹；接着打字又弹。
  const [dismissed, setDismissed] = useState<string | null>(null);
  const mention = dismissed === draft ? null : mentionAt(draft);
  const others = useMemo(() => {
    if (!mention) return [];
    const q = mention.query.toLowerCase();
    return project.tasks.filter((t) => t.id !== task?.id && t.runCount > 0 && (!q || t.title.toLowerCase().includes(q))).slice(0, 6);
  }, [mention?.query, project, task?.id]);
  // 斜杠命令（v0.10 C2）：整句只有「/几个字」时弹，Peck 自己的在前，再是项目和你的命令文件。
  const slashQ = dismissed === draft ? null : slashAt(draft);
  const slashOpen = !!slashQ;
  const [slashData, setSlashData] = useState<SlashList | null>(null);
  useEffect(() => {
    if (!slashOpen) return;
    let live = true;
    ipc.slashList(projectKey).then((l) => { if (live) setSlashData(l); }).catch(() => { if (live) setSlashData(null); });
    return () => { live = false; };
  }, [slashOpen, projectKey]);
  const slashItems = useMemo(() => {
    if (!slashQ) return [];
    const files: SlashItem[] = (slashData?.commands ?? [])
      .filter((c) => !PECK_COMMANDS.some((p) => p.name === c.name))
      .map((c) => ({ name: c.name, description: c.description, hint: c.hint, kind: "file", scope: c.scope, folder: c.folder }));
    return matchSlash([...PECK_COMMANDS, ...files], slashQ.query).slice(0, 12);
  }, [slashQ?.query, slashData]); // eslint-disable-line react-hooks/exhaustive-deps
  const [note, setNote] = useState<SlashNote | null>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  useEffect(() => setNote(null), [projectKey, task?.id]);
  const [active, setActive] = useState(0);
  useEffect(() => setActive(0), [mention?.query, slashQ?.query]);
  const popKind: "mention" | "slash" | null = mention && others.length > 0 ? "mention" : slashQ && slashItems.length > 0 ? "slash" : null;
  const popping = popKind !== null;
  const popCount = popKind === "mention" ? others.length : slashItems.length;
  const pick = (t: { id: string; title: string }) => {
    if (!mention) return;
    const title = t.title.replace(/[[\]()\\`]/g, " ").trim() || "旧任务";
    onDraft(`${draft.slice(0, mention.start)}[@${title}](peck:task/${t.id}) `);
  };
  const sendText = (text: string, level: Permission) => {
    if (!who) return Promise.resolve(false);
    return onSend(text, who.id, level, sendEffort(who, settings), cliModelOf(who, settings), goal ? [goal] : undefined, worktree || undefined, shots.length ? shots.map((s) => ({ mediaType: s.mediaType, data: s.data })) : undefined)
      .then((ok) => { if (ok) { setWorktree(false); setShots([]); } return ok; });
  };
  const send = () => {
    if (!ready || !who) return;
    const slash = parseSlash(draft);
    if (slash) { void runSlash(slash.name, slash.args).catch(say); return; }
    void sendText(draft.trim(), permission);
  };
  /** 跑一个斜杠命令：Peck 自己的就地做；命令文件展开成一句话照常发。 */
  const runSlash = async (name: string, args: string) => {
    const pid = projectKey;
    switch (name) {
      case "plan": case "edit": case "auto": {
        const level = name as Permission;
        onPermission(level);
        if (args) await sendText(args, level);
        else { onDraft(""); say(`切到 ${level.toUpperCase()} 了`); }
        return;
      }
      case "new": onDraft(""); onNewTask(); return;
      case "review": {
        const last = lastChanged(runs);
        if (!last) { say("这个任务还没有改过文件（或者都撤销了），没什么可审的"); return; }
        // 没选审查：用现在这个；现在是接力的话用接力里实施的那一个（接力不能 PLAN 发，以前会报错；v0.10 收尾审查）。
        const current = who?.id === RELAY_ID ? entries.find((e) => e.id === settings.relay.build && e.available !== false) ?? null : who;
        const pick = entries.find((e) => e.id === settings.review && canAutomate(e) && e.available !== false) ?? current;
        if (!pick) return;
        const ok = await onSend(reviewPrompt(last.run, last.index), pick.id, "plan", sendEffort(pick, settings), cliModelOf(pick, settings));
        if (ok) say(`${pick.label} 在审（PLAN，只读）`);
        return;
      }
      case "compact": {
        if (!task) { say("先选一个任务"); return; }
        onDraft("");
        setNote({ title: "COMPACT", text: await ipc.harnessCompact(task.id) });
        return;
      }
      case "undo": {
        const last = lastChanged(runs);
        if (!last) { say("这个任务没有能撤销的改动"); return; }
        onDraft("");
        setNote({
          title: "UNDO",
          text: `撤销第 ${last.index + 1} 次的改动？${last.run.files.length} 个文件回到那一次开始前的样子。`,
          confirm: { label: "UNDO", run: async () => { await onUndo(last.run); setNote({ title: "UNDO", text: `第 ${last.index + 1} 次的改动撤销了。` }); } },
        });
        return;
      }
      case "verify": {
        onDraft("");
        setNote({ title: "VERIFY", text: "在跑…" });
        try {
          const results = await ipc.verifyRunNow(pid);
          const failed = results.filter((r) => !r.passed).length;
          setNote({ title: "VERIFY", text: failed ? `${failed} 条没过` : "都过了", results });
        } catch (e) { setNote(null); say(e); }
        return;
      }
      case "cost": {
        onDraft("");
        const month = await ipc.harnessSpend().catch(() => null);
        setNote({ title: "COST", text: [spent ? `这个任务：${usageText(spent)}` : "这个任务还没有花费记录", month ? `这个月 Peck 循环：${usd(month.monthUsd) ?? "$0"}（CLI 用订阅，额度在 QUOTA 里看）` : null].filter(Boolean).join("\n") });
        return;
      }
    }
    const known = slashData?.commands.some((c) => c.name === name) || (await ipc.slashList(pid).catch(() => null))?.commands.some((c) => c.name === name);
    if (!known) {
      say(CLI_ONLY.has(name) ? `/${name} 是 CLI 自己的交互式命令，Peck 不转发${name === "model" ? "：模型在左栏 AGENTS 里换" : ""}` : `没有 /${name} 这个命令（Peck 的、项目和你的 .claude/commands 里都没有）`);
      return;
    }
    await sendText(await ipc.slashExpand(pid, name, args), permission);
  };
  const pickSlash = (item: SlashItem) => {
    if (item.instant) { onDraft(""); void runSlash(item.name, "").catch(say); return; }
    onDraft(`/${item.name} `);
  };
  // 贴图、拖图进输入框（v0.8）：最多 6 张，每张 6MB 以内；发出去以后清掉。
  const [shots, setShots] = useState<Array<ImageInput & { url: string; name: string }>>([]);
  const addFiles = (files: File[]) => {
    const images = files.filter((f) => /^image\/(png|jpeg|gif|webp)$/.test(f.type));
    if (!images.length) return false;
    for (const f of images) {
      if (f.size > 6 * 1024 * 1024) { say(`${f.name || "这张图"}太大了（超过 6MB）`); continue; }
      const reader = new FileReader();
      reader.onload = () => {
        const url = String(reader.result ?? "");
        const data = url.slice(url.indexOf(",") + 1);
        setShots((prev) => (prev.length >= 6 ? (say("一句话最多带 6 张图"), prev) : [...prev, { mediaType: f.type, data, url, name: f.name || "贴进来的图" }]));
      };
      reader.readAsDataURL(f);
    }
    return true;
  };
  // ↵：弹层开着就插选中的那个；不然发出去。
  const submit = () => {
    if (popKind === "mention") { pick(others[Math.min(active, others.length - 1)]); return; }
    if (popKind === "slash") { pickSlash(slashItems[Math.min(active, slashItems.length - 1)]); return; }
    send();
  };
  const onKey = (key: "up" | "down" | "escape") => {
    if (!popping) return false;
    if (key === "escape") setDismissed(draft);
    else setActive((i) => (i + (key === "down" ? 1 : popCount - 1)) % popCount);
    return true;
  };
  const spent = useMemo(() => (task ? taskUsage(runs, agentEvents) : null), [task, runs, agentEvents]);
  const context = useMemo(() => (task ? taskContext(runs, agentEvents) : null), [task, runs, agentEvents]);
  const money = spent ? usd(spent.usd) : null;
  return (
    <div className="pk-mod composer" ref={boxRef}>
      {quota && !busy && <QuotaLine quota={quota} />}
      {note && <SlashNoteBar note={note} onClose={() => setNote(null)} say={say} />}
      {popKind === "slash" && (
        <AbovePop anchor={boxRef} label="斜杠命令">
          <div className="pk-menu-scroll">
            <p className="mention-head t-label">/ 命令</p>
            {slashItems.map((c, i) => (
              <button key={`${c.kind}-${c.name}`} type="button" role="option" aria-selected={i === active} className={cn("pk-menu-item", i === active && "is-on")} onMouseDown={(e) => e.preventDefault()} onClick={() => pickSlash(c)}>
                <Dot kind="" />
                <b className="slash-name t-code">/{c.name}{c.hint ? <em> {c.hint}</em> : null}</b>
                <span className="slash-desc">{c.description}{c.kind === "file" ? ` · ${c.scope === "user" ? "你的" : "项目的"}${c.folder ? ` ${c.folder}` : ""}` : ""}</span>
              </button>
            ))}
          </div>
        </AbovePop>
      )}
      {popKind === "mention" && (
        <AbovePop anchor={boxRef} label="引用一个旧任务">
          <div className="pk-menu-scroll">
            <p className="mention-head t-label">@ 旧任务</p>
            {others.map((t, i) => (
              <button key={t.id} type="button" role="option" aria-selected={i === active} className={cn("pk-menu-item", i === active && "is-on")} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(t)}>
                <Dot kind="" />
                <b className="mention-title">{t.title}</b>
                <span>{t.runCount} 次</span>
              </button>
            ))}
          </div>
        </AbovePop>
      )}
      {shots.length > 0 && (
        <div className="composer-shots" aria-label="附的图">
          {shots.map((s, i) => (
            <span key={i} className="composer-shot" title={s.name}>
              <img src={s.url} alt={s.name} />
              <button type="button" aria-label={`不要这张图：${s.name}`} onClick={() => setShots((prev) => prev.filter((_, j) => j !== i))}><Icon name="x" size={12} /></button>
            </span>
          ))}
        </div>
      )}
      <div className={cn("composer-in", busy && "is-off")}
        onPasteCapture={(e) => { if (!e.clipboardData.getData("text/plain") && addFiles([...e.clipboardData.files])) e.preventDefault(); }}
        onDragOver={(e) => { if ([...e.dataTransfer.items].some((i) => i.type.startsWith("image/"))) e.preventDefault(); }}
        onDrop={(e) => { if (addFiles([...e.dataTransfer.files])) e.preventDefault(); }}>
        <span className="pk-prompt">›</span>
        <MarkdownEditor value={draft} onChange={onDraft} onSubmit={submit} onKey={onKey} placeholder={placeholder} disabled={busy} ariaLabel="一句话说要做什么" />
      </div>
      {/* 谁来干在左栏 AGENTS 里选；这里一行短的写现在是谁（用着预设就写预设名），悬停看全的，点它跳过去。 */}
      <button type="button" className="composer-who" title={`${whoText(who, settings, entries)}\n在左栏 AGENTS 里换`} onClick={onShowAgents}>
        <Dot kind={who ? entryDot(who.id) : ""} />
        <span>{whoShort(who, settings, entries)}</span>
      </button>
      <div className="composer-row">
        <Seg label="权限" sm disabled={busy} items={PERMISSIONS.map((p) => ({ id: p.id, label: p.label }))} value={permission} onChange={onPermission} />
        {worktreeable && (
          <button type="button" className={cn("pk-chip pk-chip--sm", worktree && "is-on")} aria-pressed={worktree} disabled={busy}
            title="独立工作区：在项目的一个 git worktree 里做，项目文件夹不动；做完在 CHANGES 里 APPLY 搬回或 DISCARD 丢掉"
            onClick={() => setWorktree(!worktree)}>
            <span className="pk-chip-label">WORKTREE</span>
          </button>
        )}
        {goalable && (
          <MenuChip sm label={goal ? `GOAL · ${goal === "*" ? "ALL" : goal.toUpperCase()}` : "GOAL"} ariaLabel="目标模式" title="目标模式：一直做到这条验证命令过了（最多修 8 次）" disabled={busy}>
            {(close) => (
              <>
                <p className="pk-menu-group menu-note">做到这条过了为止，最多修 8 次</p>
                {checks.map((c) => (
                  <button key={c.name} type="button" role="menuitemradio" aria-checked={goal === c.name} className={cn("pk-menu-item", goal === c.name && "is-on")} onClick={() => { setGoal(c.name); close(); }}>
                    <Dot kind="" /><b>{c.name.toUpperCase()}</b><span className="t-code">{c.run}</span>
                  </button>
                ))}
                {checks.length > 1 && <button type="button" role="menuitemradio" aria-checked={goal === "*"} className={cn("pk-menu-item", goal === "*" && "is-on")} onClick={() => { setGoal("*"); close(); }}><Dot kind="" /><b>ALL</b><span /></button>}
                <button type="button" role="menuitemradio" aria-checked={!goal} className={cn("pk-menu-item", !goal && "is-on")} onClick={() => { setGoal(null); close(); }}><Dot kind="" /><b>OFF</b><span /></button>
              </>
            )}
          </MenuChip>
        )}
        <span className="grow" />
        {context && <ContextRing context={context} />}
        {spent && <span className="composer-cost t-code" title={`这个任务一共：${usageText(spent)}`}>{money == null ? "花费未知" : `${spent.source === "estimated" ? "估 " : ""}${money}`}</span>}
        {busy ? (
          <Btn v="danger" icon="stop" onClick={onStop}>STOP</Btn>
        ) : (
          <button type="button" className="kbd kbd-send" aria-label="发出去" title="↵ 发出去；⇧↵ 或 ⌘↵ 换行" disabled={!ready} onClick={send}>↵</button>
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

/** 每轮的 recap（v0.7）：做完 / 停下、改了几个文件、验证（花费在过程最后一行，清单进度在头上）；最新的一轮带下一步按钮（模型建议的，点了放进输入框）。 */
const Recap: React.FC<{
  run: RunRecord;
  view: ReturnType<typeof foldEvents> | null;
  /** 这一次的事件（按段列花费用）。 */
  events?: AgentEvent[];
  adds: number;
  dels: number;
  latest: boolean;
  busy: boolean;
  onNext: (text: string) => void;
}> = ({ run, view, events, adds, dels, latest, busy, onNext }) => {
  const state = runState(run);
  const rows = useMemo(() => (events ? costRows(events, whoLabel(run)) : []), [events, run]);
  const done = state === "succeeded";
  const last = view ? [...view.items].reverse().find((i) => i.kind === "text") : undefined;
  // 只有 Peck 循环被要求写「→ 下一步」；CLI 引擎回答末尾的箭头列表照原样留在正文里。
  const steps = last && last.kind === "text" && isPeckRun(run) ? splitNextSteps(last.text).steps : [];
  const wt = run.worktree;
  const facts: string[] = [];
  facts.push(run.files.length ? `改了 ${run.files.length} 个文件` : "没改文件");
  if (view?.verified) facts.push(view.verified.outcome === "passed" ? (view.verified.goal ? "目标达到了" : "验证都过了") : view.verified.outcome === "stopped" ? "验证没过" : "验证在修");
  if (wt && !wt.appliedAt && !wt.discardedAt) facts.push("在独立工作区，还没搬回");
  const nexts = steps.length ? steps : done ? [] : ["继续"];
  return (
    <div className={cn("summary recap sel", !done && "recap--stopped")}>
      <div className="summary-top">
        {done ? <DotCheck /> : <Icon name={state === "cancelled" ? "stop" : "bang"} />}
        <div><b className="t-name">{done ? "做完了" : state === "cancelled" ? "停了" : "停下了"}</b><span>{facts.join(" · ")}</span></div>
        {run.files.length > 0 && <p className="summary-n"><span className="pk-add">+{adds}</span><span className="pk-del">−{dels}</span></p>}
      </div>
      {/* 事件日志没读到（或者没记下）时，回答只在任务记录里：照写出来。停下、失败的原因上面红条已经写了，不再重复。 */}
      {!last && run.summary && done && <p className="summary-text">{run.summary}</p>}
      {rows.length > 0 && <CostTable rows={rows} ms={(run.finishedAt ?? run.startedAt) - run.startedAt} />}
      {latest && nexts.length > 0 && (
        <div className="recap-next">
          {nexts.map((n, i) => <Btn key={i} sm disabled={busy} onClick={() => onNext(n)}>{n}</Btn>)}
        </div>
      )}
    </div>
  );
};

const ROLE_WORD: Record<CostRow["role"], string> = { spec: "SPEC", implement: "BUILD", review: "REVIEW", fix: "FIX", main: "MAIN", sub: "SUB" };

/** 合作、委派的任务做完：每段谁做的、用了多少 tokens、花了多少、用了多久，最后一行合计（v0.9 B4）。 */
const CostTable: React.FC<{ rows: CostRow[]; ms: number }> = ({ rows, ms }) => {
  const api = rows.filter((r) => r.engine === "peck");
  const cli = rows.filter((r) => r.engine !== "peck");
  const apiUsd = api.reduce((n, r) => n + (r.usage?.usd ?? 0), 0);
  const apiUnknown = api.some((r) => r.usage && r.usage.usd == null);
  const cliUsd = cli.reduce((n, r) => n + (r.usage?.usd ?? 0), 0);
  const total = rows.reduce<UsageLine | null>((t, r) => (r.usage ? (t ? addUsage(t, r.usage) : r.usage) : t), null);
  const money = [api.length ? `${usd(apiUsd) ?? "$0"}${apiUnknown ? "+" : ""}` : null, cli.length ? (cliUsd > 0 ? `订阅 · 估 ${usd(cliUsd)}` : "订阅") : null].filter(Boolean).join(" + ");
  return (
    <table className="cost-table" aria-label="每段的花费">
      <thead><tr><th>段</th><th>谁</th><th>进 / 出</th><th>花费</th><th>用时</th></tr></thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.id} className={r.role === "sub" ? "is-sub" : undefined}>
            <td className="t-label">{ROLE_WORD[r.role]}</td>
            <td className="cost-who" title={r.model ?? undefined}><Dot kind={r.engine === "peck" ? "api" : engineDot(r.engine)} />{r.title}</td>
            <td className="t-code">{tokenCell(r.usage)}</td>
            <td className="t-code">{costCell(r)}</td>
            <td className="t-code">{r.endedAt != null ? span(r.endedAt - r.startedAt) : "—"}</td>
          </tr>
        ))}
      </tbody>
      <tfoot><tr><td className="t-label">合计</td><td /><td className="t-code">{tokenCell(total)}</td><td className="t-code">{money}</td><td className="t-code">{span(ms)}</td></tr></tfoot>
    </table>
  );
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
  onAppend: (event: AgentEvent) => void;
  /** recap 的下一步按钮：放进输入框。 */
  onNext: (text: string) => void;
  say: (message: unknown) => void;
  /** 归档的任务只读：不重跑、卡片不能批。 */
  readOnly?: boolean;
}> = ({ run, latest, shown, onShow, busy, onRetryStep, onShowFile, events, live, onControl, onAppend, onNext, say, readOnly }) => {
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
  const retryable = (state === "failed" || state === "cancelled") && !busy && !readOnly;
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
          任务 {run.number} · {clock(run.startedAt)} 开始{running && run.files.length > 0 ? ` · 改了 ${run.files.length} 个文件` : ""}
        </span>
        <b className={cn("pk-readout task-clock", state === "succeeded" && "pk-readout--done", !running && state !== "succeeded" && "task-clock--off")}>{timer(elapsed)}</b>
      </div>
      <Blocks lit={lit} still={!running} />
      {agent ? (
        <AgentView events={events ?? []} running={running && (live || !!events?.length)} who={whoLabel(run)} onControl={onControl} onShowFile={onShowFile} say={say} runId={readOnly ? undefined : run.id} onAppend={readOnly ? undefined : onAppend} nextSteps={latest && (isPeckRun(run) || !!run.who?.startsWith("team:"))} />
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
      {state === "succeeded" && !agent && (
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
        </div>
      )}
      {(state === "failed" || state === "cancelled") && run.summary && (
        <div className={cn("pk-bar", state === "failed" && "pk-bar--bad", "task-bar sel")}>
          <Icon name={state === "failed" ? "x" : "stop"} />
          <span className="pk-bar-text">{run.summary}</span>
        </div>
      )}
      {agent && !running && state !== "undone" && (
        <Recap run={run} view={view} events={events} adds={adds} dels={dels} latest={latest} busy={busy} onNext={onNext} />
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
    void copyText(logs.map((l) => l.chunk).join("\n")).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1400); });
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
        {onRetry && <div className="step-retry"><Btn v="danger" sm icon="undo" onClick={onRetry}>{run.flowId.startsWith("api-") ? "CONTINUE" : "RETRY"}</Btn><Info text={run.flowId.startsWith("api-") ? "从现在的文件接着发一个任务，不重放跑过的工具" : "修好以后，从这一步重跑"} /></div>}
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
