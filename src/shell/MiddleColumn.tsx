import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, type ApiStatus, type ApiTask } from "@/lib/api";
import type { AgentEvent, Control, ModelEntry } from "@/lib/agentEvents";
import { CONTEXT_PARTS, addUsage, foldEvents, splitNextSteps, tokens, usageText, usd, type ContextView, type UsageLine } from "@/lib/agentView";
import { ipc, type BgInfo, type VerifyState } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import type { Message, Permission, ProjectOverview, RunRecord, StepDescriptor, TaskBrief } from "@/lib/types";
import { PERMISSIONS, agentDot, clock, cn, engineDot, entryDot, isAgentRun, isPeckRun, runState, span, stepName, timer, whoDots, whoLabel , shortenHomePath } from "@/lib/utils";
import { Blocks, Btn, Dot, DotCheck, Icon, Info, MenuChip, ModHead, Seg, useNow } from "@/ui";
import { AgentView } from "@/shell/AgentView";
import { Md } from "@/shell/Markdown";
import { MarkdownEditor } from "@/shell/MarkdownEditor";
import { cliModelOf, labelOf, pickable, sendEffort, whoText } from "@/shell/ModelPicker";

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
  onSend: (prompt: string, who: string, permission: Permission, effort?: string, model?: string, goal?: string[], worktree?: boolean) => void;
  /** 往某一次的事件里追加一条（批完「想记住这几条」）。 */
  onAppendEvent: (runId: string, event: AgentEvent) => void;
  onStop: () => void;
  onRetryStep: (run: RunRecord, stepId: string) => void;
  onControl: (runId: string, control: Control) => Promise<void>;
  onOpen: (path?: string) => void;
  onOpenSettings: () => void;
  /** 输入框下点了「谁」：展开、闪一下左栏的 AGENTS。 */
  onShowAgents: () => void;
  /** 项目文件夹不在了：重新找文件夹、移出列表。 */
  onRelocate: (projectId: string) => void;
  onRemoveProject: (projectId: string) => void;
  /** 项目的设置变了（NOT NOW、确认了验证命令）：重读项目列表。 */
  onProjectsChanged: () => void;
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

/** 这个项目的 dev server（DEV 那一片用）：项目有 server 才每 2.5 秒看一次，窗口藏着时不看。 */
function useBackground(projectId: string | null, watch: boolean): [BgInfo[], () => void] {
  const [list, setList] = useState<BgInfo[]>([]);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    setList([]);
    if (!projectId) return;
    let stop = false;
    let timer = 0;
    const read = async () => {
      if (document.visibilityState === "visible" || !watch) {
        try { const next = await ipc.backgroundList(projectId); if (!stop) setList(next); } catch { /* 读不到就不显示 */ }
      }
      if (!stop && watch) timer = window.setTimeout(() => void read(), 2500);
    };
    void read();
    return () => { stop = true; window.clearTimeout(timer); };
  }, [projectId, watch, tick]);
  return [list, () => setTick((t) => t + 1)];
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
  const { project, task, runs, settings, entries, apiStatus, busy } = props;
  const [permission, setPermission] = useState<Permission>(settings.permission);
  useEffect(() => setPermission(settings.permission), [settings.permission]);
  const projectId = project && !project.missing ? project.workspace.id : null;
  const [verify, setVerify] = useVerify(projectId, busy, props.say);
  const hasServers = !!verify?.approved && !!verify.config?.server.length;
  const [background, refreshBackground] = useBackground(projectId, hasServers);

  if (!project) {
    return (
      <section className="col" aria-label="对话">
        <div className="pk-mod col-grow home">
          <ModHead label="PECK" note="只在你这台 Mac 上的 Harness" />
          <h1 className="t-hero">先选一个项目<Info text="在左边点一个项目，或者打开一个项目文件夹。给它一句话，选谁来干：Peck 自己的循环配一个 API 模型，或者一个 CLI 引擎。每一步看得见，要批的事弹卡片；整个任务能一键撤销。" /></h1>
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
      verify={verify}
      background={background}
      onBackground={refreshBackground}
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
          <h1 className="t-hero">{project.workspace.name} 要做什么？<Info text="一句话说清楚，谁来干在左栏 AGENTS 里选。开始前先存一份快照，做完默认留着；不满意，一键撤销整个任务。WORKTREE 是在项目的一个 git worktree 里做，做完再搬回。" /></h1>
          {who ? (
            <div>
              <p className="lbl">WHO · {labelOf(who)}</p>
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
          <div>
            <p className="lbl">ACCESS · {access.label}<Info text={access.does} /></p>
          </div>
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
              onAppend={(e) => props.onAppendEvent(run.id, e)}
              onNext={(text) => props.onDraft(props.draft.trim() ? `${props.draft.trimEnd()}\n${text}` : text)}
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
  const pop = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<React.CSSProperties>({});
  const ratio = context.window ? Math.min(1, context.used / context.window) : 0;
  const line = context.window && context.compactAt ? context.compactAt / context.window : 0.8;
  const warn = ratio >= line;
  const lit = Math.max(context.used > 0 ? 1 : 0, Math.round(ratio * 12));
  useEffect(() => {
    if (!open) return;
    const r = ref.current?.getBoundingClientRect();
    if (r) setPlace({ left: Math.max(8, Math.min(r.right - 300, window.innerWidth - 308)), bottom: window.innerHeight - r.top + 6 });
    const away = (e: MouseEvent) => { const t = e.target as Node; if (!ref.current?.contains(t) && !pop.current?.contains(t)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("keydown", esc); };
  }, [open]);
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
      {open && createPortal(
        <div ref={pop} className="pk-anim-reveal-s menu-pop menu-pop--fixed ctx-pop" style={place} role="dialog" aria-label="上下文">
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
        </div>,
        document.querySelector(".app") ?? document.body,
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

// ---------- 输入位 ----------

const Composer: React.FC<Props & {
  who: ModelEntry | null;
  verify: VerifyState | null;
  background: BgInfo[];
  onBackground: () => void;
  permission: Permission;
  onPermission: (p: Permission) => void;
  placeholder: string;
}> = ({ draft, onDraft, onSend, onStop, busy, who, permission, onPermission, placeholder, settings, onShowAgents, apiStatus, runs, agentEvents, task, project, verify, background, onBackground, say }) => {
  const [goal, setGoal] = useState<string | null>(null);
  // 独立工作区（v0.7）：git 项目、Peck 循环或 CLI 引擎、不是 PLAN；发出去以后关掉（一次一选）。
  const [worktree, setWorktree] = useState(false);
  const worktreeable = !!project?.workspace.gitBranch && !!who && (who.group === "peck" || who.group === "cli") && permission !== "plan";
  useEffect(() => { if (!worktreeable) setWorktree(false); }, [worktreeable]);
  // 换了项目：上一个项目选的目标不带过来。
  const projectKey = project?.workspace.id;
  useEffect(() => setGoal(null), [projectKey]);
  // 只认站主确认过的那一份（项目自带的、改过的，确认前不跑、不起）。
  const checks = (verify?.approved && verify.config?.check) || [];
  const servers = (verify?.approved && verify.config?.server) || [];
  // 目标模式只给 Peck 循环（Antigravity 这类直接调用的不进）；PLAN 只读，不开。
  const goalable = !!who && who.group === "peck" && checks.length > 0 && permission !== "plan";
  useEffect(() => { if (!goalable) setGoal(null); }, [goalable]);
  const ready = !!draft.trim() && !!who && !busy;
  // Esc 关掉的那一稿不再弹；接着打字又弹。
  const [dismissed, setDismissed] = useState<string | null>(null);
  const mention = dismissed === draft ? null : mentionAt(draft);
  const others = useMemo(() => {
    if (!mention || !project) return [];
    const q = mention.query.toLowerCase();
    return project.tasks.filter((t) => t.id !== task?.id && t.runCount > 0 && (!q || t.title.toLowerCase().includes(q))).slice(0, 6);
  }, [mention?.query, project, task?.id]);
  const [active, setActive] = useState(0);
  useEffect(() => setActive(0), [mention?.query]);
  const popping = !!mention && others.length > 0;
  const pick = (t: { id: string; title: string }) => {
    if (!mention) return;
    const title = t.title.replace(/[[\]()\\`]/g, " ").trim() || "旧任务";
    onDraft(`${draft.slice(0, mention.start)}[@${title}](peck:task/${t.id}) `);
  };
  const send = () => {
    if (!ready || !who) return;
    onSend(draft.trim(), who.id, permission, sendEffort(who, settings), cliModelOf(who, settings), goal ? [goal] : undefined, worktree || undefined);
    setWorktree(false);
  };
  // ↵：弹层开着就插选中的那个；SEND 按钮照原样发。
  const submit = () => {
    if (popping) { pick(others[Math.min(active, others.length - 1)]); return; }
    send();
  };
  const onKey = (key: "up" | "down" | "escape") => {
    if (!popping) return false;
    if (key === "escape") setDismissed(draft);
    else setActive((i) => (i + (key === "down" ? 1 : others.length - 1)) % others.length);
    return true;
  };
  const spent = useMemo(() => (task ? taskUsage(runs, agentEvents) : null), [task, runs, agentEvents]);
  const context = useMemo(() => (task ? taskContext(runs, agentEvents) : null), [task, runs, agentEvents]);
  const money = spent ? usd(spent.usd) : null;
  const serverOf = (name: string) => background.find((b) => b.kind === "server" && b.name === name && b.status === "running");
  const anyServer = servers.some((s) => serverOf(s.name));
  const toggleServer = async (name: string) => {
    if (!project) return;
    try {
      const running = serverOf(name);
      if (running) await ipc.backgroundStop(running.id);
      else await ipc.serverStart(project.workspace.id, name);
    } catch (e) { say(e); }
    window.setTimeout(onBackground, 300);
  };
  return (
    <div className="pk-mod composer">
      {popping && (
        <div className="pk-menu mention-pop" role="listbox" aria-label="引用一个旧任务">
          <p className="mention-head t-label">@ 旧任务</p>
          {others.map((t, i) => (
            <button key={t.id} type="button" role="option" aria-selected={i === active} className={cn("pk-menu-item", i === active && "is-on")} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(t)}>
              <Dot kind="" />
              <b className="mention-title">{t.title}</b>
              <span>{t.runCount} 次</span>
            </button>
          ))}
        </div>
      )}
      <div className={cn("composer-in", busy && "is-off")}>
        <span className="pk-prompt">›</span>
        <MarkdownEditor value={draft} onChange={onDraft} onSubmit={submit} onKey={onKey} placeholder={placeholder} disabled={busy} ariaLabel="一句话说要做什么" />
      </div>
      {/* 谁来干在左栏 AGENTS 里选；这里一行写现在是谁，点它跳过去。 */}
      <button type="button" className="composer-who" title="在左栏 AGENTS 里换" onClick={onShowAgents}>
        <Dot kind={who ? entryDot(who.id) : ""} />
        <span>{whoText(who, settings, apiStatus)}</span>
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
                <p className="pk-menu-group menu-note">目标模式：模型每次停下，Peck 都跑这条；没过就交回去接着修，最多 8 次。</p>
                {checks.map((c) => (
                  <button key={c.name} type="button" role="menuitemradio" aria-checked={goal === c.name} className={cn("pk-menu-item", goal === c.name && "is-on")} onClick={() => { setGoal(c.name); close(); }}>
                    <Dot kind="" /><b>{c.name.toUpperCase()}</b><span className="t-code">{c.run}</span>
                  </button>
                ))}
                {checks.length > 1 && <button type="button" role="menuitemradio" aria-checked={goal === "*"} className={cn("pk-menu-item", goal === "*" && "is-on")} onClick={() => { setGoal("*"); close(); }}><Dot kind="" /><b>ALL</b><span>全部</span></button>}
                <button type="button" role="menuitemradio" aria-checked={!goal} className={cn("pk-menu-item", !goal && "is-on")} onClick={() => { setGoal(null); close(); }}><Dot kind="" /><b>OFF</b><span>不开目标模式</span></button>
              </>
            )}
          </MenuChip>
        )}
        <span className="grow" />
        {servers.length > 0 && (
          <MenuChip sm label={anyServer ? "DEV ON" : "DEV"} ariaLabel="常驻的 dev server" title="常驻的 dev server（.peck/verify.toml）：跨任务一直在，退出 Peck 才停">
            {() => (
              <>
                <p className="pk-menu-group menu-note">常驻的 dev server：跨任务一直在，模型能读它的输出，退出 Peck 才停。</p>
                {servers.map((sv) => {
                  const running = serverOf(sv.name);
                  return (
                    <button key={sv.name} type="button" className={cn("pk-menu-item", running && "is-on")} onClick={() => void toggleServer(sv.name)}>
                      <i className={cn("pk-dot", running && "dot-live")} />
                      <b>{sv.name.toUpperCase()}</b>
                      <span>{running ? (running.ready ? "在跑 · 点了停" : "在起 · 点了停") : "没在跑 · 点了起"}</span>
                    </button>
                  );
                })}
              </>
            )}
          </MenuChip>
        )}
        {context && <ContextRing context={context} />}
        {spent && <span className="composer-cost t-code" title={`这个任务一共：${usageText(spent)}`}>{money == null ? "花费未知" : `${spent.source === "estimated" ? "估 " : ""}${money}`}</span>}
        {busy ? (
          <Btn v="danger" icon="stop" onClick={onStop}>STOP</Btn>
        ) : (
          <>
            <span className="kbd" title="⇧↵ 或 ⌘↵ 换行">↵</span>
            <Btn v="primary" iconEnd="send" disabled={!ready} onClick={send}>SEND</Btn>
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

/** 每轮的 recap（v0.7）：做完 / 停下、改了几个文件、验证、花费；最新的一轮带下一步按钮（模型建议的，点了放进输入框）。 */
const Recap: React.FC<{
  run: RunRecord;
  view: ReturnType<typeof foldEvents> | null;
  elapsed: number;
  adds: number;
  dels: number;
  latest: boolean;
  busy: boolean;
  plan: { status: string }[] | null;
  donePlan: number;
  onNext: (text: string) => void;
  onShow?: () => void;
}> = ({ run, view, elapsed, adds, dels, latest, busy, plan, donePlan, onNext, onShow }) => {
  const state = runState(run);
  const done = state === "succeeded";
  const last = view ? [...view.items].reverse().find((i) => i.kind === "text") : undefined;
  // 只有 Peck 循环被要求写「→ 下一步」；CLI 引擎回答末尾的箭头列表照原样留在正文里。
  const steps = last && last.kind === "text" && isPeckRun(run) ? splitNextSteps(last.text).steps : [];
  const money = view?.usage ? usd(view.usage.usd) : null;
  const wt = run.worktree;
  const facts: string[] = [];
  facts.push(run.files.length ? `改了 ${run.files.length} 个文件` : "没改文件");
  if (view?.verified) facts.push(view.verified.outcome === "passed" ? (view.verified.goal ? "目标达到了" : "验证都过了") : view.verified.outcome === "stopped" ? "验证没过" : "验证在修");
  if (plan && plan.length) facts.push(`清单 ${donePlan}/${plan.length}`);
  if (wt && !wt.appliedAt && !wt.discardedAt) facts.push("在独立工作区，还没搬回");
  if (money) facts.push(money);
  const nexts = steps.length ? steps : done ? [] : ["继续"];
  return (
    <div className={cn("summary recap sel", !done && "recap--stopped")}>
      <div className="summary-top">
        {done ? <DotCheck /> : <Icon name={state === "cancelled" ? "stop" : "bang"} />}
        <div><b className="t-name">{done ? "做完了" : state === "cancelled" ? "停了" : "停下了"}</b><span>{facts.join(" · ")} · 用了 {span(elapsed)}</span></div>
        {run.files.length > 0 && <p className="summary-n"><span className="pk-add">+{adds}</span><span className="pk-del">−{dels}</span></p>}
      </div>
      {/* 事件日志没读到（或者没记下）时，回答只在任务记录里：照写出来。 */}
      {!last && run.summary && <p className="summary-text">{run.summary}</p>}
      {latest && (nexts.length > 0 || onShow || run.files.length > 0) && (
        <div className="recap-next">
          {nexts.map((n, i) => <Btn key={i} sm disabled={busy} onClick={() => onNext(n)}>{n}</Btn>)}
          {run.files.length > 0 && onShow && <Btn v="ghost" sm onClick={onShow}>CHANGES</Btn>}
        </div>
      )}
    </div>
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
}> = ({ run, latest, shown, onShow, busy, onRetryStep, onShowFile, events, live, onControl, onAppend, onNext, say }) => {
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
        <AgentView events={events ?? []} running={running && (live || !!events?.length)} who={whoLabel(run)} onControl={onControl} onShowFile={onShowFile} say={say} runId={run.id} onAppend={onAppend} nextSteps={latest && isPeckRun(run)} />
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
        <Recap run={run} view={view} elapsed={elapsed} adds={adds} dels={dels} latest={latest} busy={busy}
          plan={plan} donePlan={donePlan} onNext={onNext} onShow={run.files.length ? () => onShowFile(run.files[0].path) : undefined} />
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
