import React, { useEffect, useMemo, useRef, useState } from "react";
import type { TermView } from "@/App";
import { ipc } from "@/lib/ipc";
import type { DiffResult, FileChangeItem, FileListing, FilePreview, ProjectOverview, RunRecord, UndoPreview } from "@/lib/types";
import { clock, cn, fileSize, isMarkdownFile, runState, shortenHomePath, span, stepName } from "@/lib/utils";
import { Btn, Icon, ModHead, Twinkle } from "@/ui";

export type RightTab = "changes" | "oneshot" | "files" | "term";

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
  const tabs: Array<{ id: RightTab; label: string; extra?: React.ReactNode }> = [
    { id: "changes", label: "CHANGES", extra: run && run.files.length > 0 ? <em>{run.files.length}</em> : null },
    { id: "oneshot", label: "ONE SHOT" },
    { id: "files", label: "FILES" },
    {
      id: "term",
      label: "TERM",
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
      <div className="pk-seg pk-seg--full pk-tabs" role="tablist">
        {tabs.map((t) => (
          <button key={t.id} type="button" role="tab" aria-selected={t.id === tab} className={t.id === tab ? "is-on" : undefined} onClick={() => onTab(t.id)}>
            {t.label}{t.extra}
          </button>
        ))}
      </div>
      {tab === "changes" && <Changes {...props} />}
      {tab === "oneshot" && (
        <div className="pk-mod col-grow">
          <ModHead label="ONE SHOT" note="只读"><Expand wide={wide} onWide={onWide} /></ModHead>
          <div className="empty">
            <p>边干活边问，不打断任务。它能读整个项目，但不改文件、不跑命令；任务进度和终端输出它看不到，要带上才行。</p>
            <p className="empty-s">Muse、Grok 用它们的订阅额度；API 模型按次算钱，算进花费上限。</p>
            <p className="empty-s">这一版先放着这个页签：Muse、Grok 来答从 v0.5 起，API 模型来答从 v0.6 起。</p>
          </div>
        </div>
      )}
      {tab === "files" && <Files {...props} />}
      {tab === "term" && <Term {...props} />}
    </section>
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

const DiffView: React.FC<{ diff: DiffResult; split: boolean }> = ({ diff, split }) => {
  const lines = useMemo(() => parseDiff(diff.unifiedDiff), [diff.unifiedDiff]);
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
  return <div className="diff sel scroll">{lines.map((l, i) => row(l, i))}</div>;
};

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

const Changes: React.FC<Props> = ({ project, run, wide, onWide, filePath, onFilePath, onUndo, busy }) => {
  const [stepId, setStepId] = useState<string | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [view, setView] = useState<"diff" | "split" | "view">("diff");
  const [confirm, setConfirm] = useState<UndoPreview | null>(null);
  const [undoing, setUndoing] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => { setStepId(null); setConfirm(null); }, [run?.id]);

  const files = !run ? [] : stepId ? run.stepFiles?.[stepId] ?? run.files.filter((f) => f.stepId === stepId) : run.files;
  const selected = files.find((f) => f.path === filePath) ?? null;
  const state = run ? runState(run) : null;
  const adds = files.reduce((n, f) => n + f.additions, 0);
  const dels = files.reduce((n, f) => n + f.deletions, 0);

  useEffect(() => {
    let cancelled = false;
    setDiff(null);
    if (!run || !selected) return;
    void ipc.changesGetDiff(run.id, stepId ?? undefined, selected.path).then((d) => { if (!cancelled) setDiff(d); }).catch(() => { if (!cancelled) setDiff(null); });
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
    try { await onUndo(run); setConfirm(null); } catch { /* 提示条里说了为什么 */ } finally { setUndoing(false); }
  };
  const copy = () => {
    if (!diff) return;
    void navigator.clipboard.writeText(view === "view" && diff.contentAfter ? diff.contentAfter : diff.unifiedDiff).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1400); });
  };

  const canView = !!diff && isMarkdownFile(diff.filePath) && !!diff.contentAfter;
  const list = (
    <div className="changes-list">
      <p className="snap"><Icon name="clock" size={12} /><span>{run ? `任务前快照 · ${clock(run.startedAt)}` : "发任务时先存一份快照"}</span></p>
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
        <div className={cn("pk-tree scroll", state === "undone" && "is-undone")}>
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
      {!diff ? <p className="empty-line">在读这个文件的改动…</p> : view === "view" && canView ? <Markdown text={diff.contentAfter!} /> : <DiffView diff={diff} split={view === "split"} />}
    </div>
  );
  const changed = confirm?.files.filter((f) => f.changedSince).length ?? 0;
  const restored = confirm?.files.filter((f) => f.kind === "delete").length ?? 0;
  const foot = run && (
    <div className="changes-foot">
      {state === "undone" ? (
        <p className="back"><Icon name="check" size={12} /><span>已撤销。要再做一次，点左边 NEW TASK。</span></p>
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
          <Btn v="danger" icon="undo" disabled={busy} onClick={() => void askUndo()}>UNDO TASK</Btn>
          <p className="hint">{busy ? "这个项目有任务在跑，等它做完再撤销。" : `回到 ${clock(run.startedAt)} 开始前的样子，你的 Git 不动。`}</p>
        </>
      ) : null}
    </div>
  );
  return (
    <div className="pk-mod col-grow changes">
      <ModHead label="CHANGES" note={project ? "直接写进项目" : undefined}><Expand wide={wide} onWide={onWide} /></ModHead>
      <div className={cn("changes-body", wide && "changes-body--wide")}>
        <div className="changes-side">{list}{wide && foot}</div>
        {preview}
      </div>
      {!wide && foot}
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

const WHY: Record<string, string> = { image: "图片不在这里预览。", binary: "二进制文件，不预览。", large: "文件太大（超过 400 KB），不预览。", missing: "这个文件现在不在了。" };

const Files: React.FC<Props> = ({ project, run, wide, onWide, onTab, onFilePath, say }) => {
  const [listing, setListing] = useState<FileListing | null>(null);
  const [closed, setClosed] = useState<Set<string> | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<FilePreview | null>(null);
  const [marks, setMarks] = useState<Set<number>>(new Set());
  const workspaceId = project?.workspace.id;
  const changed = useMemo(() => new Map((run?.files ?? []).map((f) => [f.path, f])), [run?.files]);
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
  }, [workspaceId, path, run?.id, mark?.kind, mark?.additions, stamp, say]); // eslint-disable-line react-hooks/exhaustive-deps

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
  const note = !preview ? "" : preview.deleted ? "这个任务删了它。预览的是删之前的样子；撤销整个任务能放回来。" : mark?.kind === "create" ? "新文件，整份都是这个任务写的。" : mark ? "改过的行左边一条绿；完整的 Diff 在 CHANGES。" : "";
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
                <span className="t-label preview-meta">{preview?.deleted ? "DEL" : mark?.kind === "create" ? `NEW +${mark.additions}` : mark ? `+${mark.additions} −${mark.deletions}` : preview ? fileSize(preview.size) : ""}</span>
              </p>
              {note && <p className="hint">{note}</p>}
              {!preview ? <p className="empty-line">在读…</p> : preview.kind !== "text" ? <p className="empty-line">{WHY[preview.kind]}{preview.kind !== "missing" ? `（${fileSize(preview.size)}）` : ""}</p> : (
                <div className={cn("code-lines sel scroll", preview.deleted && "is-dim")}>
                  {lines.map((text, i) => (
                    <div key={i}>
                      <i className={marks.has(i + 1) || (mark?.kind === "create" && !preview.deleted) ? "is-add" : undefined} />
                      <span className="code-n">{i + 1}</span>
                      <span className="code-t">{text || " "}</span>
                    </div>
                  ))}
                </div>
              )}
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
                <p>你自己的终端，开在 {shortenHomePath(project.workspace.path)}。</p>
                <p className="empty-s">不经过权限三档，也不进模型上下文；Agent 跑的命令在中间的步骤里看。</p>
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
            <input className="t-code" aria-label="一条命令" value={locked ? "" : draft} disabled={locked || running} placeholder={locked ? "任务在跑，终端锁着" : running ? "" : "打一条命令"} onChange={(e) => setDraft(e.target.value)} onKeyDown={onKey} spellCheck={false} autoComplete="off" />
            {running ? <Btn v="danger" icon="stop" onClick={onTermStop}>STOP</Btn> : locked ? <span className="term-lock t-label"><Icon name="lock" size={12} />LOCKED</span> : <><span className="kbd">⏎</span><Btn v={draft.trim() ? "primary" : undefined} disabled={!draft.trim()} onClick={run}>RUN</Btn></>}
          </div>
          <p className="hint term-note">
            {locked
              ? running ? "发任务前就在跑的命令接着跑，STOP 一直能点；它改的项目文件算进这次改动。跑完以后输入行锁着，任务做完才解锁。" : "模型在跑任务，终端先锁着：做完或 STOP 就解锁。输出照样能看。"
              : `在 ${shortenHomePath(project.workspace.path)} 里跑；cd 到哪儿会记着，export 的变量不带到下一条。`}
          </p>
        </>
      )}
    </div>
  );
};
