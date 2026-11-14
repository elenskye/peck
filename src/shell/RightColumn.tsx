import React, { useEffect, useMemo, useRef, useState } from "react";
import type { TermView } from "@/App";
import { canAutomate, type AgentEvent, type ModelEntry } from "@/lib/agentEvents";
import { foldEvents } from "@/lib/agentView";
import { ipc } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import type { DiffResult, FileChangeItem, FileListing, FilePreview, ProjectOverview, RunRecord, UndoPreview } from "@/lib/types";
import { clock, cn, fileSize, isMarkdownFile, runState, shortenHomePath, span, stepName } from "@/lib/utils";
import { Btn, Icon, Info, ModHead, Twinkle, type IconName } from "@/ui";
import { AgentView } from "@/shell/AgentView";
import { Md } from "@/shell/Markdown";
import { highlight, type Token } from "@/lib/highlight";
import { MarkdownEditor } from "@/shell/MarkdownEditor";
import { EffortPicker, WhoPicker, cliModelOf, defaultEffort, sendEffort } from "@/shell/ModelPicker";

export type RightTab = "changes" | "log" | "oneshot" | "files" | "term";

interface Props {
  project: ProjectOverview | null;
  run: RunRecord | null;
  tab: RightTab;
  onTab: (tab: RightTab) => void;
  wide: boolean;
  onWide: (wide: boolean) => void;
  /** 这个项目有任务在跑：终端锁着 */
  busy: boolean;
  filePath: string | null;
  onFilePath: (path: string | null) => void;
  onUndo: (run: RunRecord) => Promise<void>;
  term: TermView;
  onTermRun: (command: string) => void;
  onTermStop: () => void;
  onTermClear: () => void;
  say: (message: unknown) => void;
  /** 模型列表（ONE SHOT 选谁来答） */
  entries: ModelEntry[];
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  /** 把 ONE SHOT 的回答、CHANGES 里攒的批注放进中栏输入框 */
  onToTask: (text: string) => void;
  /** 任务记录变了（独立工作区搬回、丢掉） */
  onRunUpdated: (record: RunRecord) => void;
  /** 原生窗口（Quick Look 只在原生里有） */
  native: boolean;
}

const Expand: React.FC<{ wide: boolean; onWide: (wide: boolean) => void }> = ({ wide, onWide }) => (
  <Btn v="ghost" sm square aria-label={wide ? "收回右栏" : "展开到宽栏"} onClick={() => onWide(!wide)}>
    <Icon name={wide ? "collapse" : "expand"} size={12} />
  </Btn>
);

/** 右栏：顶上一条页签，一次开一个；从不自己切走正在看的页签。 */
export const RightColumn: React.FC<Props> = (props) => {
  const { run, tab, onTab, wide, onWide, busy, term } = props;
  const state = run ? runState(run) : null;
  // 页签只有图标，选中的那个带名字（站主 2026-11-13 定）；悬停看名字。
  const tabs: Array<{ id: RightTab; label: string; icon: IconName; extra?: React.ReactNode }> = [
    { id: "changes", label: "CHANGES", icon: "changes", extra: run && run.files.length > 0 ? <em>{run.files.length}</em> : null },
    { id: "log", label: "LOG", icon: "log" },
    { id: "oneshot", label: "ONE SHOT", icon: "oneshot" },
    { id: "files", label: "FILES", icon: "files" },
    {
      id: "term",
      label: "TERM",
      icon: "term",
      extra: (
        <>
          {busy && <Icon name="lock" size={12} />}
          {term.runningId && tab !== "term" && <Twinkle />}
          {term.unread && tab !== "term" && <i className={cn("pk-badge", `pk-badge--${term.unread}`)} />}
        </>
      ),
    },
  ];
  return (
    <section className={cn("col right", wide && "right--wide")} aria-label="右栏">
      {wide && run && state !== "running" && run.finishedAt && Date.now() - run.finishedAt < 60_000 && (
        <div className={cn("pk-bar", state === "succeeded" ? "pk-bar--ok" : "pk-bar--bad")}>
          <Icon name={state === "succeeded" ? "check" : "x"} />
          <span className="pk-bar-text">任务 {run.number} {state === "succeeded" ? "做完了" : "停下了"}，回去看总结</span>
          <Btn sm onClick={() => onWide(false)}>BACK</Btn>
        </div>
      )}
      <div className="pk-seg pk-seg--full pk-tabs pk-tabs--icons" role="tablist">
        {tabs.map((t) => (
          <button key={t.id} type="button" role="tab" aria-selected={t.id === tab} aria-label={t.label} title={t.id === tab ? undefined : t.label} className={t.id === tab ? "is-on" : undefined} onClick={() => onTab(t.id)}>
            <Icon name={t.icon} size={12} />
            {t.id === tab && <span className="tab-name">{t.label}</span>}
            {t.extra}
          </button>
        ))}
      </div>
      {tab === "changes" && <Changes {...props} />}
      {tab === "log" && <TaskLog run={props.run} />}
      {/* ONE SHOT 一直挂着，只是藏起来：切去别的页签再回来，问到一半的、答完的都还在，STOP 也还能点。 */}
      <OneShot key={props.project?.workspace.id ?? "none"} {...props} shown={tab === "oneshot"} />
      {tab === "files" && <Files {...props} />}
      {tab === "term" && <Term {...props} />}
    </section>
  );
};

// ---------- LOG ----------

/** 这个任务的记录：存快照、每一步、做完、撤销（原来在左栏，站主 2026-11-13 挪到右栏）。 */
const TaskLog: React.FC<{ run: RunRecord | null }> = ({ run }) => {
  const items: Array<{ t: number; title: string; note: string }> = [];
  if (run) {
    items.push({ t: run.startedAt, title: "存了任务前快照", note: "撤销就回到这一刻" });
    for (const step of run.steps) {
      if (step.status === "succeeded" && step.finishedAt) items.push({ t: step.finishedAt, title: `${stepName(step)} 做完`, note: `用了 ${span(step.durationMs)}` });
      if (step.status === "failed" && step.finishedAt) items.push({ t: step.finishedAt, title: `${stepName(step)} 失败`, note: step.exitCode != null ? `退出码 ${step.exitCode}` : "没跑完" });
    }
    if (run.finishedAt && run.status === "succeeded") items.push({ t: run.finishedAt, title: "做完了", note: `${run.files.length} 个文件 · 默认留着` });
    if (run.finishedAt && run.status === "cancelled") items.push({ t: run.finishedAt, title: "已停止", note: "改过的文件留着，能撤销" });
    if (run.undoneAt) items.push({ t: run.undoneAt, title: "撤销了", note: `回到 ${clock(run.startedAt)} 开始前的样子` });
    items.sort((a, b) => a.t - b.t);
  }
  return (
    <div className="pk-mod col-grow">
      <ModHead label="LOG" note={run ? `任务 ${run.number}` : "这个任务"} />
      {items.length === 0 ? (
        <p className="empty-line">发任务以后，存快照、每一步、做完、撤销都记在这里。</p>
      ) : (
        <ul className="scroll log col-grow">
          {items.map((item, i) => (
            <li key={i}>
              <span className="log-t">{clock(item.t)}</span>
              <span>
                <b className="t-name">{item.title}</b>
                <span className="log-n">{item.note}</span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};

// ---------- ONE SHOT ----------

/** 边干活边问：选一个引擎（API 模型和 CLI，Antigravity 除外），按 PLAN 档只读回答，不锁终端，答完一行花费。 */
const OneShot: React.FC<Props & { shown: boolean }> = ({ project, entries, settings, onSettings, onToTask, wide, onWide, say, shown }) => {
  const hide = shown ? undefined : { display: "none" };
  const options = entries.filter((e) => e.available && e.group !== "flow" && canAutomate(e));
  const pick = options.find((e) => e.id === settings.oneshotPick) ?? options.find((e) => e.id === settings.pick) ?? options.find((e) => e.group === "peck") ?? options[0] ?? null;
  const [question, setQuestion] = useState("");
  const [asked, setAsked] = useState("");
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [runId, setRunId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const view = useMemo(() => foldEvents(events), [events]);
  const answer = [...view.items].reverse().find((i) => i.kind === "text");
  const effort = pick ? settings.efforts[pick.id] ?? defaultEffort(pick) : "off";
  const ask = async () => {
    const prompt = question.trim();
    if (!project || !pick || !prompt || running) return;
    setAsked(prompt);
    setEvents([]);
    setRunning(true);
    try {
      const id = await ipc.harnessOneshot(
        // CLI 用 AGENTS 里给这家选的模型和强度。
        { workspaceId: project.workspace.id, prompt, who: pick.id, model: cliModelOf(pick, settings), effort: sendEffort(pick, settings), costCap: settings.costCap },
        (event) => {
          setEvents((prev) => [...prev, event]);
          if (event.type === "turn.completed") setRunning(false);
        },
      );
      setRunId(id);
      setQuestion("");
    } catch (e) { setRunning(false); say(e); }
  };
  const stop = () => { if (runId) void ipc.gatewayControl(runId, { type: "interrupt" }).catch(() => setRunning(false)); };
  if (!project) {
    return (
      <div className="pk-mod col-grow" style={hide}>
        <ModHead label="ONE SHOT" note="只读"><Expand wide={wide} onWide={onWide} /></ModHead>
        <p className="empty-line">先选一个项目。</p>
      </div>
    );
  }
  return (
    <div className="pk-mod col-grow oneshot" style={hide}>
      <ModHead label="ONE SHOT" note={<>只读 · 不打断任务<Info text="边干活边问一句。它能读整个项目，但不改文件；任务进度和终端输出它看不到，要带上就贴进问题里。API 模型按次算钱，算进花费上限；CLI 用它们的订阅额度。Antigravity 只做直接调用，不在这里。" /></>}><Expand wide={wide} onWide={onWide} /></ModHead>
      <div className="col-grow scroll oneshot-body">
        {!asked ? (
          <div className="empty">
            <p>边干活边问一句，只读。</p>
          </div>
        ) : (
          <>
            <div className="you sel"><span className="t-label">ASK</span><span className="you-text">{asked}</span></div>
            <AgentView events={events} running={running} who={pick?.label.toUpperCase() ?? ""} onControl={(c) => (runId ? ipc.gatewayControl(runId, c) : Promise.resolve())} onShowFile={() => {}} say={say} />
            {!running && answer && answer.kind === "text" && (
              <div className="oneshot-acts">
                <Btn v="ghost" sm onClick={() => void navigator.clipboard.writeText(answer.text)}>COPY</Btn>
                <Btn v="ghost" sm onClick={() => onToTask(answer.text)}>TO TASK</Btn>
              </div>
            )}
          </>
        )}
      </div>
      <div className="oneshot-in">
        <div className={cn("composer-in", running && "is-off")}>
          <span className="pk-prompt">›</span>
          <MarkdownEditor value={question} onChange={setQuestion} onSubmit={() => void ask()} placeholder="问一句…（↵ 发送，⇧↵ 换行）" disabled={running} ariaLabel="ONE SHOT 问一句" />
        </div>
        <div className="composer-row">
          <WhoPicker entries={options} value={pick} disabled={running} only={["api", "cli"]} remembered={settings.picks}
            onChange={(oneshotPick) => onSettings({ oneshotPick })}>
            <EffortPicker entry={pick} value={effort} disabled={running} onChange={(e) => pick && onSettings({ efforts: { ...settings.efforts, [pick.id]: e } })} />
          </WhoPicker>
          <span className="grow" />
          {running ? <Btn v="danger" sm icon="stop" onClick={stop}>STOP</Btn> : <Btn v="primary" sm disabled={!question.trim() || !pick} onClick={() => void ask()}>ASK</Btn>}
        </div>
      </div>
    </div>
  );
};

// ---------- Diff ----------

interface DiffLine { type: "head" | "hunk" | "add" | "del" | "ctx"; text: string; oldNo?: number; newNo?: number }

function parseDiff(raw: string): DiffLine[] {
  const out: DiffLine[] = [];
  let oldNo = 1;
  let newNo = 1;
  for (const line of raw.replace(/\n$/, "").split("\n")) {
    if (/^(diff --git|index |--- |\+\+\+ |new file mode|deleted file mode|old mode|new mode|similarity|rename )/.test(line)) continue;
    if (line.startsWith("\\ No newline")) continue;
    if (line.startsWith("@@")) {
      const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (m) { oldNo = Number(m[1]); newNo = Number(m[2]); }
      out.push({ type: "hunk", text: line.replace(/^@@[^@]*@@ ?/, "") });
    } else if (line.startsWith("+")) out.push({ type: "add", text: line.slice(1), newNo: newNo++ });
    else if (line.startsWith("-")) out.push({ type: "del", text: line.slice(1), oldNo: oldNo++ });
    else out.push({ type: "ctx", text: line.slice(1), oldNo: oldNo++, newNo: newNo++ });
  }
  return out;
}

/** 这个文件里任务加的行号（FILES 预览左边那条绿）。 */
function addedLines(raw: string): Set<number> {
  return new Set(parseDiff(raw).filter((l) => l.type === "add").map((l) => l.newNo!));
}

/** 一条批注：哪个文件、哪一行（任务后的行号；删掉的行用任务前的）、那一行写的什么、批了什么。 */
export interface DiffComment { id: string; runId: string; path: string; line: number; side: "new" | "old"; quote: string; text: string }
export type HunkState = "accepted" | "rejected";

/** 浏览器里存一份（只是这台机器上的方便：刷新、重开还在；读不到就从空的开始）。换了 key 当场读新的，不先露一下旧的。 */
function useStored<T>(key: string, init: T): [T, (next: T) => void] {
  const read = (): T => { try { const raw = localStorage.getItem(key); return raw ? (JSON.parse(raw) as T) : init; } catch { return init; } };
  const [held, setHeld] = useState<{ key: string; value: T }>(() => ({ key, value: read() }));
  const value = held.key === key ? held.value : read();
  useEffect(() => { if (held.key !== key) setHeld({ key, value: read() }); }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (next: T) => { setHeld({ key, value: next }); try { localStorage.setItem(key, JSON.stringify(next)); } catch { /* 存不了就只在这次 */ } };
  return [value, set];
}

/** 按块接受 / 拒绝的状态只留最近这么多条（键是「任务:文件:第几块」）。 */
const HUNK_KEEP = 2000;

/** 删任务时把它在这台机器上存的批注一起清掉。 */
export function forgetTaskNotes(taskId: string) {
  try { localStorage.removeItem(`peck.comments.${taskId}`); } catch { /* 读不到就算了 */ }
}

const DiffView: React.FC<{
  diff: DiffResult;
  split: boolean;
  /** 能按块接受 / 拒绝时才有。 */
  hunks?: { state: (i: number) => HunkState | undefined; act: (i: number, action: HunkState) => void; busy: number | null };
  /** 能批注时才有。 */
  notes?: { list: DiffComment[]; add: (line: number, side: "new" | "old", quote: string, text: string) => void; remove: (id: string) => void };
}> = ({ diff, split, hunks, notes }) => {
  const lines = useMemo(() => parseDiff(diff.unifiedDiff), [diff.unifiedDiff]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  if (lines.length === 0) return <p className="empty-line">这个文件没有文字上的改动（可能是图片、二进制，或者只是权限变了）。</p>;
  const row = (l: DiffLine, i: number, side?: "old" | "new") => (
    <div key={i} className={cn("dl", `dl--${l.type}`)}>
      <span className="dl-n">{l.type === "hunk" ? "" : side === "old" ? l.oldNo ?? "" : side === "new" ? l.newNo ?? "" : l.newNo ?? l.oldNo ?? ""}</span>
      <span className="dl-m">{l.type === "add" ? "+" : l.type === "del" ? "−" : ""}</span>
      <span className="dl-t">{l.type === "hunk" ? `⋯ ${l.text}` : l.text || " "}</span>
    </div>
  );
  if (split) {
    return (
      <div className="diff diff--split sel scroll">
        <div>{lines.filter((l) => l.type !== "add").map((l, i) => row(l, i, "old"))}</div>
        <div>{lines.filter((l) => l.type !== "del").map((l, i) => row(l, i, "new"))}</div>
      </div>
    );
  }
  let hunk = -1;
  return (
    <div className="diff sel scroll">
      {lines.map((l, i) => {
        if (l.type === "hunk") {
          hunk += 1;
          const h = hunk;
          const state = hunks?.state(h);
          return (
            <div key={i} className={cn("dl dl--hunk", state && `is-${state}`)}>
              <span className="dl-n" />
              <span className="dl-m" />
              <span className="dl-t">⋯ {l.text}</span>
              {hunks && (
                <span className="dl-acts">
                  {state ? <em className={cn("t-label", state === "rejected" ? "is-bad" : "is-ok")}>{state === "rejected" ? "REJECTED" : "ACCEPTED"}</em> : (
                    <>
                      <button type="button" className="dl-act" disabled={hunks.busy != null} onClick={() => hunks.act(h, "rejected")}>REJECT</button>
                      <button type="button" className="dl-act" disabled={hunks.busy != null} onClick={() => hunks.act(h, "accepted")}>ACCEPT</button>
                    </>
                  )}
                </span>
              )}
            </div>
          );
        }
        const side: "new" | "old" = l.type === "del" ? "old" : "new";
        const no = side === "new" ? l.newNo! : l.oldNo!;
        const key = `${side}:${no}`;
        const mine = notes?.list.filter((c) => c.side === side && c.line === no) ?? [];
        return (
          <React.Fragment key={i}>
            <div className={cn("dl", `dl--${l.type}`, mine.length > 0 && "has-note")}>
              {notes ? (
                <button type="button" className="dl-n dl-n--btn" title="批注这一行" onClick={() => { setEditing(editing === key ? null : key); setDraft(""); }}>{l.newNo ?? l.oldNo ?? ""}</button>
              ) : <span className="dl-n">{l.newNo ?? l.oldNo ?? ""}</span>}
              <span className="dl-m">{l.type === "add" ? "+" : l.type === "del" ? "−" : ""}</span>
              <span className="dl-t">{l.text || " "}</span>
            </div>
            {mine.map((c) => (
              <div key={c.id} className="dl-note">
                <Icon name="chat" size={12} />
                <span className="dl-note-t">{c.text}</span>
                <button type="button" className="dl-act" aria-label="删掉这条批注" onClick={() => notes!.remove(c.id)}><Icon name="x" size={12} /></button>
              </div>
            ))}
            {notes && editing === key && (
              <div className="dl-edit">
                <textarea autoFocus rows={2} value={draft} aria-label="批注" spellCheck={false}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setEditing(null);
                    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229) {
                      e.preventDefault();
                      if (draft.trim()) { notes.add(no, side, l.text, draft.trim()); setEditing(null); }
                    }
                  }} />
                <div className="dl-edit-acts">
                  <Btn v="ghost" sm onClick={() => setEditing(null)}>CANCEL</Btn>
                  <Btn sm disabled={!draft.trim()} onClick={() => { notes.add(no, side, l.text, draft.trim()); setEditing(null); }}>ADD</Btn>
                </div>
              </div>
            )}
          </React.Fragment>
        );
      })}
    </div>
  );
};

/** 攒着的批注 → 一段话（放进中栏输入框，接着这个任务发）。 */
export function commentsToPrompt(list: DiffComment[]): string {
  const byFile = new Map<string, DiffComment[]>();
  for (const c of list) byFile.set(c.path, [...(byFile.get(c.path) ?? []), c]);
  const parts = ["针对改动的批注，照这些改："];
  for (const [path, items] of byFile) {
    for (const c of items.sort((a, b) => a.line - b.line)) {
      const quote = c.quote.trim() ? `（\`${c.quote.trim().slice(0, 80)}\`）` : "";
      parts.push(`- \`${path}\` ${c.side === "old" ? "原来的" : ""}第 ${c.line} 行${quote}：${c.text}`);
    }
  }
  return parts.join("\n");
}

/** Markdown 只做够看规范和清单的那几样：标题、列表、勾选、引用、代码。 */
const Markdown: React.FC<{ text: string }> = ({ text }) => {
  const inline = (s: string) => s.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).map((part, i) =>
    part.startsWith("`") && part.endsWith("`") ? <code key={i} className="t-code">{part.slice(1, -1)}</code>
      : part.startsWith("**") && part.endsWith("**") ? <b key={i}>{part.slice(2, -2)}</b> : part);
  const out: React.ReactNode[] = [];
  let fence: string[] | null = null;
  text.split("\n").forEach((line, i) => {
    if (line.trim().startsWith("```")) {
      if (fence) { out.push(<pre key={i} className="t-code md-code">{fence.join("\n")}</pre>); fence = null; } else fence = [];
      return;
    }
    if (fence) { fence.push(line); return; }
    const check = line.match(/^\s*[-*]\s*\[([ xX])\]\s+(.*)$/);
    const bullet = line.match(/^\s*(?:[-*]|\d+\.)\s+(.*)$/);
    if (/^#{1,6}\s/.test(line)) out.push(<p key={i} className={line.startsWith("# ") ? "t-title md-h" : "t-name md-h"}>{inline(line.replace(/^#+\s/, ""))}</p>);
    else if (line.startsWith("> ")) out.push(<p key={i} className="md-quote">{inline(line.slice(2))}</p>);
    else if (check) out.push(<p key={i} className={cn("md-li", check[1] !== " " && "is-done")}><span className={cn("pk-check", check[1] !== " " && "is-on")}><Icon name="check" size={12} /></span><span>{inline(check[2])}</span></p>);
    else if (bullet) out.push(<p key={i} className="md-li"><i className="md-dot" /><span>{inline(bullet[1])}</span></p>);
    else if (line.trim()) out.push(<p key={i}>{inline(line)}</p>);
  });
  return <div className="md sel scroll">{out}</div>;
};

// ---------- CHANGES ----------

/** 文件行：改动列表和文件树共用（标法一样：NEW、DEL、+ −）。 */
const FileRow: React.FC<{ name: string; title?: string; mark?: FileChangeItem; on?: boolean; depth?: number; dir?: "open" | "closed"; gone?: boolean; onClick: () => void }> = ({ name, title, mark, on, depth = 0, dir, gone, onClick }) => (
  <button type="button" title={title} className={cn("pk-row", dir && "pk-row--dir", (mark?.kind === "delete" || gone) && "pk-row--del", on && "is-on", gone && "pk-anim-dissolve")} style={depth ? { paddingLeft: 6 + depth * 14 } : undefined} onClick={onClick}>
    <Icon name={dir === "open" ? "triD" : dir === "closed" ? "triR" : "file"} size={12} />
    <span className="pk-row-name">{name}</span>
    {mark?.kind === "create" ? <em className="pk-tag pk-tag--new">NEW</em> : mark?.kind === "delete" ? <em className="pk-tag pk-tag--del">DEL</em> : <span />}
    {mark && mark.kind !== "delete" && mark.additions > 0 ? <span className="pk-add">+{mark.additions}</span> : <span />}
    {mark && mark.deletions > 0 ? <span className="pk-del">−{mark.deletions}</span> : <span />}
  </button>
);

const Changes: React.FC<Props> = ({ project, run, wide, onWide, filePath, onFilePath, onUndo, busy, onToTask, onRunUpdated, say }) => {
  const [stepId, setStepId] = useState<string | null>(null);
  const [hunkStates, storeHunkStates] = useStored<Record<string, HunkState>>("peck.hunks", {});
  // 新的放最后，太多了丢最早的。
  const setHunkStates = (next: Record<string, HunkState>) => {
    const keys = Object.keys(next);
    storeHunkStates(keys.length > HUNK_KEEP ? Object.fromEntries(keys.slice(-HUNK_KEEP).map((k) => [k, next[k]])) : next);
  };
  const [hunkBusy, setHunkBusy] = useState<number | null>(null);
  const [comments, setComments] = useStored<DiffComment[]>(`peck.comments.${run?.sessionId ?? "none"}`, []);
  const [conflicts, setConflicts] = useState<string[] | null>(null);
  const [moving, setMoving] = useState(false);
  useEffect(() => { setConflicts(null); }, [run?.id]);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [diffError, setDiffError] = useState("");
  const [view, setView] = useState<"diff" | "split" | "view">("diff");
  const [confirm, setConfirm] = useState<UndoPreview | null>(null);
  const [undoing, setUndoing] = useState(false);
  const [copied, setCopied] = useState(false);
  // 撤销的动效：out 是一行行溶掉、扫一道光，back 是以已撤销的样子露出来。
  const [fx, setFx] = useState<"out" | "back" | null>(null);
  const fxTimers = useRef<number[]>([]);
  const clearFx = () => { fxTimers.current.forEach((t) => window.clearTimeout(t)); fxTimers.current = []; setFx(null); };
  useEffect(() => { setStepId(null); setConfirm(null); clearFx(); }, [run?.id]);
  useEffect(() => () => fxTimers.current.forEach((t) => window.clearTimeout(t)), []);

  const files = !run ? [] : stepId ? run.stepFiles?.[stepId] ?? run.files.filter((f) => f.stepId === stepId) : run.files;
  const selected = files.find((f) => f.path === filePath) ?? null;
  const state = run ? runState(run) : null;
  const adds = files.reduce((n, f) => n + f.additions, 0);
  const dels = files.reduce((n, f) => n + f.deletions, 0);

  useEffect(() => {
    let cancelled = false;
    setDiff(null);
    setDiffError("");
    if (!run || !selected) return;
    void ipc.changesGetDiff(run.id, stepId ?? undefined, selected.path).then((d) => { if (!cancelled) setDiff(d); }).catch((e) => {
      if (cancelled) return;
      setDiff(null);
      setDiffError(String(e instanceof Error ? e.message : e).replace(/^Error:\s*/, "") || "没有原因");
    });
    return () => { cancelled = true; };
    // 文件的行数变了（这一步收尾了）就重读一遍
  }, [run?.id, run?.status, stepId, selected?.path, selected?.additions, selected?.deletions]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (!wide && view === "split") setView("diff"); }, [wide, view]);

  const askUndo = async () => {
    if (!run) return;
    try { setConfirm(await ipc.runUndoPreview(run.id)); } catch { setConfirm({ runId: run.id, files: run.files.map((f) => ({ path: f.path, kind: f.kind, changedSince: false })) }); }
  };
  const doUndo = async () => {
    if (!run) return;
    setUndoing(true);
    try {
      await onUndo(run);
      setConfirm(null);
      clearFx();
      setFx("out");
      fxTimers.current = [window.setTimeout(() => setFx("back"), 700), window.setTimeout(() => setFx(null), 950)];
    } catch { /* 提示条里说了为什么 */ } finally { setUndoing(false); }
  };
  const copy = () => {
    if (!diff) return;
    void navigator.clipboard.writeText(view === "view" && diff.contentAfter ? diff.contentAfter : diff.unifiedDiff).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1400); });
  };

  const canView = !!diff && isMarkdownFile(diff.filePath) && !!diff.contentAfter;
  const wt = run?.worktree ?? null;
  const wtPending = !!wt && !wt.appliedAt && !wt.discardedAt;
  // 按块接受 / 拒绝：做完了、没撤销、独立工作区没丢掉、这个项目没在跑，看的是整个任务（不是某一步）时才能点。
  const hunkKey = (i: number) => `${run?.id}:${selected?.path}:${i}`;
  const canAct = !!run && !!selected && state !== "running" && state !== "undone" && !wt?.discardedAt && !busy && !stepId;
  const actHunk = async (i: number, action: HunkState) => {
    if (!run || !selected) return;
    if (action === "accepted") { setHunkStates({ ...hunkStates, [hunkKey(i)]: action }); return; }
    setHunkBusy(i);
    try {
      await ipc.changesHunkRevert(run.id, undefined, selected.path, i);
      setHunkStates({ ...hunkStates, [hunkKey(i)]: action });
    } catch (e) { say(e); } finally { setHunkBusy(null); }
  };
  const fileNotes = comments.filter((c) => c.runId === run?.id && c.path === selected?.path);
  const addNote = (line: number, side: "new" | "old", quote: string, text: string) => {
    if (!run || !selected) return;
    setComments([...comments, { id: `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`, runId: run.id, path: selected.path, line, side, quote, text }]);
  };
  const sendNotes = () => {
    if (comments.length === 0) return;
    onToTask(commentsToPrompt(comments));
    setComments([]);
  };
  const apply = async (force: boolean) => {
    if (!run) return;
    setMoving(true);
    try {
      const r = await ipc.worktreeApply(run.id, force);
      if (r.conflicts.length && !force) setConflicts(r.conflicts);
      else { setConflicts(null); onRunUpdated(r.record); say(`搬回了 ${r.applied.length} 个文件`); }
    } catch (e) { say(e); } finally { setMoving(false); }
  };
  const discard = async () => {
    if (!run) return;
    setMoving(true);
    try { onRunUpdated(await ipc.worktreeDiscard(run.id)); setConflicts(null); } catch (e) { say(e); } finally { setMoving(false); }
  };
  const list = (
    <div className="changes-list">
      <p className="snap"><Icon name="clock" size={12} /><span>{run ? `任务 ${run.number} · 任务前快照 · ${clock(run.startedAt)}` : "发任务时先存一份快照"}</span></p>
      <div className="nums">
        <b className={cn("pk-readout pk-readout--num", state === "undone" && "nums-off")}>{files.length}</b><span>个文件</span>
        <b className="pk-readout pk-readout--num pk-readout--done">+{adds}</b>
        <b className="pk-readout pk-readout--num nums-del">−{dels}</b>
      </div>
      {run && run.steps.length > 1 && (
        <div className="step-filter" role="group" aria-label="看哪一步的改动">
          <button type="button" className={cn("pk-chip pk-chip--sm", !stepId && "is-on")} onClick={() => setStepId(null)}>ALL</button>
          {run.steps.map((s) => (
            <button key={s.id} type="button" title={stepName(s)} className={cn("pk-chip pk-chip--sm", stepId === s.id && "is-on")} onClick={() => setStepId(s.id)}>STEP {s.stepIndex + 1}</button>
          ))}
        </div>
      )}
      {files.length === 0 ? (
        <p className="empty-line">{!run ? "发一个任务，它改了哪些文件会列在这里。" : state === "running" ? "还没有改动。它一动文件，这里就会列出来。" : "这个任务没有改文件。"}</p>
      ) : (
        <div className={cn("pk-tree scroll", state === "undone" && "is-undone", fx && `fx-${fx}`)}>
          {files.map((f) => <FileRow key={f.path} name={wide ? f.path : f.path.split("/").pop()!} title={f.path} mark={f} on={f.path === filePath} onClick={() => onFilePath(f.path === filePath ? null : f.path)} />)}
        </div>
      )}
    </div>
  );
  const preview = selected && (
    <div className="pk-mod pk-mod--sunk preview">
      <div className="preview-head">
        <span className="t-code preview-path" title={selected.path}>{selected.path}</span>
        <span className="grow" />
        <div className="pk-seg pk-seg--sm" role="group" aria-label="怎么看">
          <button type="button" className={view === "diff" ? "is-on" : undefined} onClick={() => setView("diff")}>DIFF</button>
          {wide && <button type="button" className={view === "split" ? "is-on" : undefined} onClick={() => setView("split")}>SPLIT</button>}
          {canView && <button type="button" className={view === "view" ? "is-on" : undefined} onClick={() => setView("view")}>VIEW</button>}
        </div>
        <Btn v="ghost" sm onClick={copy} disabled={!diff}>{copied ? "COPIED" : "COPY"}</Btn>
      </div>
      {diffError ? <p className="empty-line is-err sel">读不出这个文件的改动：{diffError}</p> : !diff ? <p className="empty-line">在读这个文件的改动…</p> : view === "view" && canView ? <Markdown text={diff.contentAfter!} /> : (
        <DiffView diff={diff} split={view === "split"}
          hunks={canAct ? { state: (i) => hunkStates[hunkKey(i)], act: (i, a) => void actHunk(i, a), busy: hunkBusy } : undefined}
          notes={run && state !== "undone" ? { list: fileNotes, add: addNote, remove: (id) => setComments(comments.filter((c) => c.id !== id)) } : undefined} />
      )}
    </div>
  );
  const changed = confirm?.files.filter((f) => f.changedSince).length ?? 0;
  const restored = confirm?.files.filter((f) => f.kind === "delete").length ?? 0;
  const notesBar = comments.length > 0 && (
    <div className="pk-bar notes-bar">
      <Icon name="chat" />
      <span className="pk-bar-text">{comments.length} 条批注</span>
      <Btn v="ghost" sm onClick={() => setComments([])}>CLEAR</Btn>
      <Btn v="primary" sm onClick={sendNotes}>TO TASK</Btn>
    </div>
  );
  const worktreeBar = run && wt && (
    <div className={cn("pk-bar wt-bar", wtPending && "pk-bar--wait")}>
      <Icon name={wtPending ? "folder" : wt.appliedAt ? "check" : "trash"} />
      <span className="pk-bar-text">{wtPending ? (state === "running" ? "在独立工作区里做，项目文件夹不动" : "在独立工作区里做完了，项目文件夹还没动") : wt.appliedAt ? `搬回了项目 · ${clock(wt.appliedAt)}` : `丢掉了独立工作区 · ${clock(wt.discardedAt!)}`}</span>
      {wtPending && state !== "running" && !conflicts && (
        <>
          <Btn v="ghost" sm disabled={moving || busy} onClick={() => void discard()}>DISCARD</Btn>
          <Btn v="primary" sm disabled={moving || busy} onClick={() => void apply(false)}>APPLY</Btn>
        </>
      )}
    </div>
  );
  const conflictBox = conflicts && (
    <div className="confirm">
      <p className="confirm-t"><Icon name="bang" /><b className="t-name">{conflicts.length} 个文件在项目里也改过了</b></p>
      <ul className="wt-conflicts t-code">{conflicts.map((c) => <li key={c}>{c}</li>)}</ul>
      <div className="confirm-row">
        <Btn v="fill" disabled={moving} onClick={() => void apply(true)}>APPLY ANYWAY</Btn>
        <Btn disabled={moving} onClick={() => setConflicts(null)}>CANCEL</Btn>
      </div>
    </div>
  );
  const foot = run && (
    <div className="changes-foot">
      {conflictBox}
      {wtPending || wt?.discardedAt ? null : state === "undone" ? (
        fx === "out" ? null : <p className={cn("back", fx === "back" && "fx-back")}><Icon name="check" size={12} /><span>已撤销。要再做一次，点左边 NEW TASK。</span></p>
      ) : confirm ? (
        <div className="confirm">
          <p className="confirm-t"><Icon name="undo" /><b className="t-name">撤销这个任务？</b></p>
          <p className="note">
            {confirm.files.length} 个文件回到 {clock(run.startedAt)} 开始前的样子{restored > 0 ? `，删掉的 ${restored} 个放回来` : ""}。
            {changed > 0 ? `其中 ${changed} 个在任务做完以后又改过，也会回到任务前。` : ""}
            你后来在别的文件里改的不动；你的 Git 不动。
          </p>
          <div className="confirm-row">
            <Btn v="fill" disabled={undoing || confirm.files.length === 0} onClick={() => void doUndo()}>{undoing ? "UNDOING" : `UNDO ${confirm.files.length} FILES`}</Btn>
            <Btn disabled={undoing} onClick={() => setConfirm(null)}>CANCEL</Btn>
          </div>
        </div>
      ) : state !== "running" && run.files.length > 0 ? (
        <>
          <p className="set-inline undo-row">
            <Btn v="danger" icon="undo" disabled={busy} onClick={() => void askUndo()}>UNDO TASK</Btn>
            {busy ? <span className="hint">这个项目有任务在跑</span> : <Info text={`回到 ${clock(run.startedAt)} 开始前的样子；只动这个任务改过的文件，你的 Git 不动。`} />}
          </p>
        </>
      ) : null}
    </div>
  );
  return (
    <div className="pk-mod col-grow changes">
      <ModHead label="CHANGES" note={project ? (wtPending ? "独立工作区" : "直接写进项目") : undefined}><Expand wide={wide} onWide={onWide} /></ModHead>
      {worktreeBar}
      {notesBar}
      <div className={cn("changes-body", wide && "changes-body--wide")}>
        <div className="changes-side">{list}{wide && foot}</div>
        {preview}
      </div>
      {!wide && foot}
      {fx === "out" && <div className="changes-fx" aria-hidden><i className="pk-scan" /></div>}
    </div>
  );
};

// ---------- FILES ----------

interface TreeNode { name: string; path: string; dir: boolean; size: number; children: TreeNode[] }
function buildTree(paths: Array<{ path: string; size: number }>): TreeNode[] {
  const root: TreeNode = { name: "", path: "", dir: true, size: 0, children: [] };
  for (const entry of paths) {
    let node = root;
    const parts = entry.path.split("/");
    parts.forEach((part, i) => {
      const path = parts.slice(0, i + 1).join("/");
      let child = node.children.find((c) => c.name === part);
      if (!child) { child = { name: part, path, dir: i < parts.length - 1, size: i < parts.length - 1 ? 0 : entry.size, children: [] }; node.children.push(child); }
      node = child;
    });
  }
  const sort = (n: TreeNode) => { n.children.sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name)); n.children.forEach(sort); };
  sort(root);
  return root.children;
}

const WHY: Record<string, string> = { binary: "二进制文件，不预览。", large: "文件太大，不预览。", missing: "这个文件现在不在了。" };

/** 能切源码 / 预览的几种。 */
const previewable = (path: string) => /\.(md|markdown|html?|svg)$/i.test(path);

/** 项目里的 html 只看样子：沙箱里本来就不跑脚本；再不让它从网上取东西、不让它自己跳走。 */
const htmlPreview = (html: string) =>
  `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:">${html.replace(/<meta[^>]*http-equiv\s*=\s*["']?refresh[^>]*>/gi, "")}`;

/** 一行代码：上了色的片段，没有就原样。 */
const CodeText: React.FC<{ text: string; tokens?: Token[] }> = ({ text, tokens }) => (
  <span className="code-t">{tokens ? tokens.map((t, j) => <span key={j} className={t.cls || undefined}>{t.text}</span>) : text || " "}</span>
);

/** PDF：data URL 换成 blob 地址给 iframe（WebKit 里比 data URL 稳）。 */
function useBlobUrl(dataUrl: string | null | undefined): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!dataUrl) { setUrl(null); return; }
    let made: string | null = null;
    let stop = false;
    void fetch(dataUrl).then((r) => r.blob()).then((b) => { if (stop) return; made = URL.createObjectURL(b); setUrl(made); }).catch(() => { if (!stop) setUrl(dataUrl); });
    return () => { stop = true; if (made) URL.revokeObjectURL(made); };
  }, [dataUrl]);
  return url;
}

/** Word、Excel 这类：Quick Look 的缩略图。 */
const OfficePreview: React.FC<{ workspaceId: string; path: string; native: boolean; say: (m: unknown) => void }> = ({ workspaceId, path, say }) => {
  const [img, setImg] = useState<string | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    let stop = false;
    setImg(null); setErr("");
    void ipc.filesQuicklook(workspaceId, path).then((u) => { if (!stop) setImg(u); }).catch((e) => { if (!stop) setErr(String(e instanceof Error ? e.message : e).replace(/^Error:\s*/, "")); });
    return () => { stop = true; };
  }, [workspaceId, path]);
  return (
    <div className="media-box scroll">
      {img ? <img src={img} alt={path} className="media-img media-img--page" /> : err ? <p className="empty-line">{err}</p> : <p className="empty-line"><Twinkle /> Quick Look…</p>}
      <div className="preview-acts"><Btn v="ghost" sm onClick={() => void ipc.filesQuicklookOpen(workspaceId, path).catch(say)}>QUICK LOOK</Btn></div>
    </div>
  );
};

const Files: React.FC<Props> = ({ project, run, wide, onWide, onTab, onFilePath, say, native }) => {
  const [listing, setListing] = useState<FileListing | null>(null);
  const [tokens, setTokens] = useState<Token[][] | null>(null);
  const [view, setView] = useState<"src" | "view">("view");
  const [closed, setClosed] = useState<Set<string> | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<FilePreview | null>(null);
  const [marks, setMarks] = useState<Set<number>>(new Set());
  const workspaceId = project?.workspace.id;
  // 独立工作区里做的、还没搬回：FILES 列的是项目文件夹，改动不在这里，就不标。
  const elsewhere = !!run?.worktree && !run.worktree.appliedAt && !run.worktree.discardedAt;
  const changed = useMemo(() => new Map(elsewhere ? [] : (run?.files ?? []).map((f) => [f.path, f])), [run?.files, elsewhere]);
  const undone = !!run?.undoneAt;
  // 任务一动文件就重列一遍（接着现有的文件监听推来的事件）。
  const stamp = `${run?.id ?? ""}:${run?.files.length ?? 0}:${run?.status ?? ""}:${run?.undoneAt ?? ""}`;
  useEffect(() => {
    if (!workspaceId) { setListing(null); return; }
    let cancelled = false;
    void ipc.filesList(workspaceId).then((l) => { if (!cancelled) setListing(l); }).catch((e) => { if (!cancelled) say(e); });
    return () => { cancelled = true; };
  }, [workspaceId, stamp, say]);
  useEffect(() => { setClosed(null); setPath(null); }, [workspaceId]);

  // 这个任务删掉的文件也列着（划掉），点开看删之前的样子。
  const entries = useMemo(() => {
    const present = listing?.entries ?? [];
    const names = new Set(present.map((e) => e.path));
    const gone = undone ? [] : [...changed.values()].filter((f) => f.kind === "delete" && !names.has(f.path)).map((f) => ({ path: f.path, size: 0 }));
    return [...present, ...gone];
  }, [listing, changed, undone]);
  const tree = useMemo(() => buildTree(entries), [entries]);
  // 默认：第一层开着，再往下只开有改动的文件夹。
  const isClosed = (node: TreeNode, depth: number) => {
    if (closed?.has(node.path)) return true;
    if (closed?.has(`!${node.path}`)) return false;
    return depth > 0 && ![...changed.keys()].some((p) => p.startsWith(`${node.path}/`));
  };
  const toggle = (node: TreeNode, depth: number) => {
    const next = new Set(closed ?? []);
    const now = isClosed(node, depth);
    next.delete(node.path); next.delete(`!${node.path}`);
    next.add(now ? `!${node.path}` : node.path);
    setClosed(next);
  };

  const mark = path && !undone ? changed.get(path) : undefined;
  useEffect(() => {
    let cancelled = false;
    setPreview(null); setMarks(new Set());
    if (!workspaceId || !path) return;
    void ipc.filesRead(workspaceId, path, run?.id).then((p) => { if (!cancelled) setPreview(p); }).catch((e) => { if (!cancelled) say(e); });
    if (run && mark && mark.kind !== "delete") {
      void ipc.changesGetDiff(run.id, undefined, path).then((d) => { if (!cancelled) setMarks(addedLines(d.unifiedDiff)); }).catch(() => {});
    }
    return () => { cancelled = true; };
    // 只有这个任务改到的文件才跟着任务重读（图片、PDF、Office 不在任务里改就不反复读、不反复起 Quick Look）。
  }, [workspaceId, path, run?.id, mark?.kind, mark?.additions, mark ? stamp : "", say]); // eslint-disable-line react-hooks/exhaustive-deps

  const rows: React.ReactNode[] = [];
  const walk = (nodes: TreeNode[], depth: number) => nodes.forEach((node) => {
    if (node.dir) {
      const shut = isClosed(node, depth);
      rows.push(<FileRow key={node.path} name={node.name} depth={depth} dir={shut ? "closed" : "open"} onClick={() => toggle(node, depth)} />);
      if (!shut) walk(node.children, depth + 1);
    } else {
      rows.push(<FileRow key={node.path} name={node.name} title={node.path} depth={depth} mark={undone ? undefined : changed.get(node.path)} on={node.path === path} onClick={() => setPath(node.path === path ? null : node.path)} />);
    }
  });
  walk(tree, 0);

  const lines = preview?.kind === "text" ? (preview.content ?? "").replace(/\n$/, "").split("\n") : [];
  // 代码上色（按需加载语言包）；换文件先清掉上一份。
  useEffect(() => {
    let stop = false;
    setTokens(null);
    if (preview?.kind === "text" && preview.content) void highlight(preview.content, preview.path).then((t) => { if (!stop) setTokens(t); }).catch(() => {});
    return () => { stop = true; };
  }, [preview?.path, preview?.content, preview?.kind]);
  const pdfUrl = useBlobUrl(preview?.kind === "pdf" ? preview.dataUrl : null);
  const canView = !!path && previewable(path) && preview?.kind === "text";
  const showing = canView && view === "view";
  const body = !preview ? <p className="empty-line">在读…</p>
    : preview.kind === "image" && preview.dataUrl ? <div className="media-box media-box--checker scroll"><img src={preview.dataUrl} alt={preview.path} className="media-img" /></div>
    : preview.kind === "pdf" ? (pdfUrl ? <iframe className="media-frame" src={pdfUrl} title={preview.path} /> : <p className="empty-line">在读…</p>)
    : preview.kind === "office" && workspaceId ? <OfficePreview workspaceId={workspaceId} path={preview.path} native={native} say={say} />
    : preview.kind !== "text" ? <p className="empty-line">{WHY[preview.kind] ?? "预览不了。"}{preview.kind !== "missing" ? `（${fileSize(preview.size)}）` : ""}</p>
    : showing && /\.(md|markdown)$/i.test(preview.path) ? <Md text={preview.content ?? ""} className="files-md scroll" />
    : showing && /\.html?$/i.test(preview.path) ? <iframe className="media-frame media-frame--html" sandbox="" srcDoc={htmlPreview(preview.content ?? "")} title={preview.path} />
    : showing && /\.svg$/i.test(preview.path) ? <div className="media-box media-box--checker scroll"><img className="media-img" alt={preview.path} src={`data:image/svg+xml;utf8,${encodeURIComponent(preview.content ?? "")}`} /></div>
    : (
      <div className={cn("code-lines sel scroll", preview.deleted && "is-dim")}>
        {lines.map((text, i) => (
          <div key={i}>
            <i className={marks.has(i + 1) || (mark?.kind === "create" && !preview.deleted) ? "is-add" : undefined} />
            <span className="code-n">{i + 1}</span>
            <CodeText text={text} tokens={tokens?.[i]} />
          </div>
        ))}
      </div>
    );
  const note = !preview ? "" : elsewhere ? "这个任务的改动还在独立工作区，这里是项目文件夹现在的样子；APPLY 以后才进来。" : preview.deleted ? "这个任务删了它。预览的是删之前的样子；撤销整个任务能放回来。" : mark?.kind === "create" ? "新文件，整份都是这个任务写的。" : mark ? "改过的行左边一条绿；完整的 Diff 在 CHANGES。" : "";
  return (
    <div className="pk-mod col-grow files">
      <ModHead label="FILES" note="只看，不改"><Expand wide={wide} onWide={onWide} /></ModHead>
      {!project ? <p className="empty-line">选一个项目，这里列它的文件。</p> : (
        <div className={cn("files-body", wide && "files-body--wide")}>
          <div className="files-tree">
            <div className="pk-tree scroll">{listing ? rows : <p className="empty-line"><Twinkle /> 在读项目里的文件…</p>}</div>
            {listing && entries.length === 0 && <p className="empty-line">这个文件夹里还没有文件。</p>}
            {listing && (listing.hidden > 0 || listing.truncated) && (
              <p className="hint files-hidden">{listing.hidden > 0 ? `照忽略规则藏了 ${listing.hidden} 个文件和文件夹（node_modules 这些）。` : ""}{listing.truncated ? "文件太多，只列了前 4000 个。" : ""}</p>
            )}
          </div>
          {path && (
            <div className="files-preview">
              <p className="preview-head">
                <span className="t-code preview-path" title={path}>{path}</span>
                <span className="grow" />
                {canView && (
                  <span className="pk-seg pk-seg--sm" role="group" aria-label="怎么看">
                    <button type="button" className={view === "view" ? "is-on" : undefined} onClick={() => setView("view")}>VIEW</button>
                    <button type="button" className={view === "src" ? "is-on" : undefined} onClick={() => setView("src")}>SRC</button>
                  </span>
                )}
                <span className="t-label preview-meta">{preview?.deleted ? "DEL" : mark?.kind === "create" ? `NEW +${mark.additions}` : mark ? `+${mark.additions} −${mark.deletions}` : preview ? fileSize(preview.size) : ""}</span>
              </p>
              {note && <p className="hint">{note}</p>}
              {body}
              {mark && <div className="preview-acts"><Btn v="ghost" sm onClick={() => { onFilePath(path); onTab("changes"); }}>DIFF</Btn></div>}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

// ---------- TERM ----------

const Term: React.FC<Props> = ({ project, wide, onWide, busy, term, onTermRun, onTermStop, onTermClear }) => {
  const [draft, setDraft] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [cursor, setCursor] = useState(-1);
  const endRef = useRef<HTMLDivElement>(null);
  const size = term.entries.reduce((n, e) => n + e.output.length, term.entries.length);
  useEffect(() => { endRef.current?.scrollIntoView({ block: "end" }); }, [size]);
  const running = !!term.runningId;
  const locked = busy;
  const name = project?.workspace.name ?? "";
  const prompt = (cwd: string) => `${cwd ? (cwd.startsWith("/") ? shortenHomePath(cwd) : `${name}/${cwd}`) : name} $`;
  const last = term.entries[term.entries.length - 1];
  const finished = last && last.id !== term.runningId && last.durationMs != null ? last : null;
  const run = () => {
    const command = draft.trim();
    if (!command || running || locked) return;
    onTermRun(command);
    setHistory((h) => [...h.filter((c) => c !== command), command].slice(-50));
    setCursor(-1);
    setDraft("");
  };
  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") { e.preventDefault(); run(); }
    if (e.key === "ArrowUp" && history.length) { e.preventDefault(); const next = cursor < 0 ? history.length - 1 : Math.max(0, cursor - 1); setCursor(next); setDraft(history[next]); }
    if (e.key === "ArrowDown" && cursor >= 0) { e.preventDefault(); const next = cursor + 1; if (next >= history.length) { setCursor(-1); setDraft(""); } else { setCursor(next); setDraft(history[next]); } }
  };
  return (
    <div className="pk-mod col-grow term">
      <ModHead label="TERM" note="你自己的终端">
        <Btn v="ghost" sm onClick={onTermClear} disabled={term.entries.length === 0}>CLEAR</Btn>
        <Expand wide={wide} onWide={onWide} />
      </ModHead>
      {!project ? <p className="empty-line">选一个项目，终端开在它的根目录。</p> : (
        <>
          <div className="term-out sel scroll">
            {term.entries.length === 0 && (
              <div className="empty">
                <p>你自己的终端，开在 {shortenHomePath(project.workspace.path)}<Info text="不经过权限三档，也不进模型上下文；Agent 跑的命令在中间的步骤里看。cd 到哪儿会记着，export 的变量不带到下一条；项目设置里的环境变量都带上。" /></p>
              </div>
            )}
            {term.entries.map((entry) => (
              <div key={entry.id} className="term-entry">
                <p className="term-cmd"><span className="term-p">{prompt(entry.cwd)}</span>{entry.command}</p>
                {entry.output && <pre>{entry.output.replace(/\n$/, "")}</pre>}
              </div>
            ))}
            <div ref={endRef} />
          </div>
          {finished && (
            <div className={cn("term-exit", finished.code !== 0 && "is-bad")}>
              <b className="t-label">{finished.stopped ? "STOPPED" : `EXIT ${finished.code ?? "?"}`} · {span(finished.durationMs)}</b>
              <span className="t-code">{finished.command}</span>
            </div>
          )}
          <div className={cn("term-in", (locked || running) && "is-off")}>
            <span className="t-code term-p">{prompt(term.cwd)}</span>
            <input className="t-code" aria-label="一条命令" autoCorrect="off" autoCapitalize="off" value={locked ? "" : draft} disabled={locked || running} placeholder={locked ? "任务在跑，终端锁着" : running ? "" : "打一条命令"} onChange={(e) => setDraft(e.target.value)} onKeyDown={onKey} spellCheck={false} autoComplete="off" />
            {running ? <Btn v="danger" icon="stop" onClick={onTermStop}>STOP</Btn> : locked ? <span className="term-lock t-label"><Icon name="lock" size={12} />LOCKED</span> : <><span className="kbd">⏎</span><Btn v={draft.trim() ? "primary" : undefined} disabled={!draft.trim()} onClick={run}>RUN</Btn></>}
          </div>
          {locked && (
            <p className="hint term-note">
              {running ? "任务在跑：这条接着跑，STOP 一直能点" : "任务在跑，终端锁着"}
              <Info text={running ? "发任务前就在跑的命令接着跑；它改的项目文件算进这次改动。跑完以后输入行锁着，任务做完才解锁。" : "模型在跑任务，终端先锁着：做完或 STOP 就解锁。输出照样能看。"} />
            </p>
          )}
        </>
      )}
    </div>
  );
};
