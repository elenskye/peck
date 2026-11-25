import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { TermView } from "@/App";
import type { AgentEvent, ModelEntry } from "@/lib/agentEvents";
import { foldEvents } from "@/lib/agentView";
import { ipc, type BgInfo } from "@/lib/ipc";
import type { AppSettings } from "@/lib/settings";
import type { FileChangeItem, FileListing, FilePreview, IgnoredEntry, ProjectOverview, RunRecord } from "@/lib/types";
import { cn, copyText, fileSize, runState, shortenHomePath, span } from "@/lib/utils";
import { Btn, Icon, Info, ModHead, Twinkle, type IconName } from "@/ui";
import { AgentView } from "@/shell/AgentView";
import { parseDiff } from "@/lib/diff";
import { Md } from "@/shell/Markdown";
import { highlight, type Token } from "@/lib/highlight";
import { MarkdownEditor } from "@/shell/MarkdownEditor";
import { EffortPicker, WhoPicker, cliModelOf, defaultEffort, sendEffort } from "@/shell/ModelPicker";
import { PromptPanel } from "@/shell/PromptLibrary";

export type RightTab = "oneshot" | "files" | "prompt" | "term";

interface Props {
  project: ProjectOverview | null;
  run: RunRecord | null;
  tab: RightTab;
  onTab: (tab: RightTab) => void;
  wide: boolean;
  onWide: (wide: boolean) => void;
  /** 这个项目有任务在跑：终端锁着 */
  busy: boolean;
  /** 对话里点了一个改过的文件：FILES 打开它。 */
  filePath: string | null;
  onFilePath: (path: string | null) => void;
  term: TermView;
  onTermRun: (command: string) => void;
  onTermStop: () => void;
  onTermClear: () => void;
  say: (message: unknown) => void;
  /** 模型列表（ONE SHOT 选谁来答） */
  entries: ModelEntry[];
  settings: AppSettings;
  onSettings: (patch: Partial<AppSettings>) => void;
  /** 把 ONE SHOT 的回答、PROMPT 的一条放进中栏输入框 */
  onToTask: (text: string) => void;
  /** 顶栏上收起了右栏（v1.1.12）：藏起来不卸掉，ONE SHOT 的状态留着。 */
  hidden?: boolean;
}

const Expand: React.FC<{ wide: boolean; onWide: (wide: boolean) => void }> = ({ wide, onWide }) => (
  <Btn v="ghost" sm square aria-label={wide ? "收回右栏" : "展开到宽栏"} onClick={() => onWide(!wide)}>
    <Icon name={wide ? "collapse" : "expand"} size={12} />
  </Btn>
);

/** 右栏：顶上一条页签，一次开一个；从不自己切走正在看的页签。 */
export const RightColumn: React.FC<Props> = (props) => {
  const { run, tab, onTab, wide, onWide, busy, term, hidden } = props;
  const state = run ? runState(run) : null;
  // 页签只有图标，选中的那个带名字（站主 2026-11-13 定）；悬停看名字。
  const tabs: Array<{ id: RightTab; label: string; icon: IconName; extra?: React.ReactNode }> = [
    { id: "files", label: "FILES", icon: "files", extra: run && run.files.length > 0 && !run.undoneAt ? <em>{run.files.length}</em> : null },
    { id: "oneshot", label: "ONE SHOT", icon: "oneshot" },
    { id: "prompt", label: "PROMPT", icon: "prompt" },
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
    <section className={cn("col right", wide && "right--wide", hidden && "is-hidden")} aria-label="右栏" aria-hidden={hidden || undefined}>
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
      {/* ONE SHOT 一直挂着，只是藏起来：切去别的页签再回来，问到一半的、答完的都还在，STOP 也还能点。 */}
      <OneShot key={props.project?.workspace.id ?? "none"} {...props} shown={tab === "oneshot"} />
      {tab === "files" && <Files {...props} />}
      {tab === "prompt" && (
        // 放进输入框时收回宽栏，不然看不到中栏的输入框。
        <PromptPanel canInsert={!!props.project} say={props.say} tools={<Expand wide={wide} onWide={onWide} />}
          onToTask={(text) => { onWide(false); props.onToTask(text); }} />
      )}
      {tab === "term" && <Term {...props} />}
    </section>
  );
};

// ---------- ONE SHOT ----------

/** 边干活边问：选一个引擎（API 模型和 CLI），按 PLAN 档只读回答，不锁终端，答完一行花费。 */
const OneShot: React.FC<Props & { shown: boolean }> = ({ project, entries, settings, onSettings, onToTask, wide, onWide, say, shown }) => {
  const hide = shown ? undefined : { display: "none" };
  const options = entries.filter((e) => e.available);
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
      <ModHead label="ONE SHOT" note={<>只读<Info text="边干活边问一句。它能读整个项目，但不改文件；任务进度和终端输出它看不到，要带上就贴进问题里。API 模型按次算钱，算进花费上限；CLI 用它们的订阅额度。" /></>}><Expand wide={wide} onWide={onWide} /></ModHead>
      <div className="col-grow scroll oneshot-body">
        {!asked ? null : (
          <>
            <div className="you sel"><span className="t-label">ASK</span><span className="you-text">{asked}</span></div>
            <AgentView events={events} running={running} who={pick?.label.toUpperCase() ?? ""} onControl={(c) => (runId ? ipc.gatewayControl(runId, c) : Promise.resolve())} onShowFile={() => {}} say={say} />
            {!running && answer && answer.kind === "text" && (
              <div className="oneshot-acts">
                <Btn v="ghost" sm onClick={() => void copyText(answer.text).catch(say)}>COPY</Btn>
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
        <div className="oneshot-controls">
          <WhoPicker entries={options} value={pick} disabled={running} remembered={settings.picks}
            onChange={(oneshotPick) => onSettings({ oneshotPick })}>
            <EffortPicker entry={pick} value={effort} disabled={running} onChange={(e) => pick && onSettings({ efforts: { ...settings.efforts, [pick.id]: e } })} />
          </WhoPicker>
          {running ? <Btn v="danger" sm icon="stop" onClick={stop}>STOP</Btn> : <Btn v="primary" sm disabled={!question.trim() || !pick} onClick={() => void ask()}>ASK</Btn>}
        </div>
      </div>
    </div>
  );
};

// ---------- Diff ----------

/** 这个文件里任务加的行号（FILES 预览左边那条绿）。 */
function addedLines(raw: string): Set<number> {
  return new Set(parseDiff(raw).filter((l) => l.type === "add").map((l) => l.newNo!));
}

/** 浏览器里存一份（只是这台机器上的方便：刷新、重开还在；读不到就从空的开始）。换了 key 当场读新的，不先露一下旧的。 */
function useStored<T>(key: string, init: T): [T, (next: T) => void] {
  const read = (): T => { try { const raw = localStorage.getItem(key); return raw ? (JSON.parse(raw) as T) : init; } catch { return init; } };
  const [held, setHeld] = useState<{ key: string; value: T }>(() => ({ key, value: read() }));
  const value = held.key === key ? held.value : read();
  useEffect(() => { if (held.key !== key) setHeld({ key, value: read() }); }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (next: T) => { setHeld({ key, value: next }); try { localStorage.setItem(key, JSON.stringify(next)); } catch { /* 存不了就只在这次 */ } };
  return [value, set];
}

// ---------- FILES ----------

/** 文件行（这次任务改到的标 NEW、DEL、+ −）。 */
const FileRow: React.FC<{ name: string; title?: string; mark?: FileChangeItem; on?: boolean; depth?: number; dir?: "open" | "closed"; dim?: boolean; onClick: () => void }> = ({ name, title, mark, on, depth = 0, dir, dim, onClick }) => (
  <button type="button" title={title} className={cn("pk-row", dir && "pk-row--dir", mark?.kind === "delete" && "pk-row--del", dim && "pk-row--dim", on && "is-on")} style={depth ? { paddingLeft: 6 + depth * 14 } : undefined} onClick={onClick}>
    <Icon name={dir === "open" ? "triD" : dir === "closed" ? "triR" : "file"} size={12} />
    <span className="pk-row-name">{name}</span>
    {mark?.kind === "create" ? <em className="pk-tag pk-tag--new">NEW</em> : mark?.kind === "delete" ? <em className="pk-tag pk-tag--del">DEL</em> : <span />}
    {mark && mark.kind !== "delete" && mark.additions > 0 ? <span className="pk-add">+{mark.additions}</span> : <span />}
    {mark && mark.deletions > 0 ? <span className="pk-del">−{mark.deletions}</span> : <span />}
  </button>
);

/** 用编辑器、访达打开这个文件（v0.8）。 */
const OpenBtns: React.FC<{ workspaceId: string; path: string; app: string; say: (m: unknown) => void }> = ({ workspaceId, path, app, say }) => (
  <>
    <Btn v="ghost" sm title={app ? `用 ${app} 打开` : "用默认的 App 打开"} onClick={() => void ipc.openPath(workspaceId, path, "editor", app).catch(say)}>OPEN</Btn>
    <Btn v="ghost" sm title="在访达里选中" onClick={() => void ipc.openPath(workspaceId, path, "finder").catch(say)}>FINDER</Btn>
  </>
);

interface TreeNode { name: string; path: string; dir: boolean; size: number; children: TreeNode[]; /** 照忽略规则藏着的（ALL 开着才列，变暗；文件夹点开才读一层）。 */ ignored?: boolean }
function buildTree(paths: Array<{ path: string; size: number; dir?: boolean; ignored?: boolean }>): TreeNode[] {
  const root: TreeNode = { name: "", path: "", dir: true, size: 0, children: [] };
  for (const entry of paths) {
    let node = root;
    const parts = entry.path.split("/");
    parts.forEach((part, i) => {
      const path = parts.slice(0, i + 1).join("/");
      const leaf = i === parts.length - 1;
      let child = node.children.find((c) => c.name === part);
      if (!child) { child = { name: part, path, dir: !leaf || !!entry.dir, size: leaf && !entry.dir ? entry.size : 0, children: [], ignored: entry.ignored && (leaf || node.ignored) }; node.children.push(child); }
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
const OfficePreview: React.FC<{ workspaceId: string; path: string; say: (m: unknown) => void }> = ({ workspaceId, path, say }) => {
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

const Files: React.FC<Props> = ({ project, run, wide, onWide, filePath, onFilePath, settings, say }) => {
  const [listing, setListing] = useState<FileListing | null>(null);
  const [tokens, setTokens] = useState<Token[][] | null>(null);
  const [view, setView] = useState<"src" | "view">("view");
  const [closed, setClosed] = useState<Set<string> | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<FilePreview | null>(null);
  const [marks, setMarks] = useState<Set<number>>(new Set());
  const workspaceId = project?.workspace.id;
  // ALL（v1.1.8）：照忽略规则藏着的也列出来（变暗）；藏着的文件夹点开才读一层，读过的记着。
  const [all, setAll] = useStored<boolean>("peck.files.all", false);
  const [inside, setInside] = useState<Record<string, IgnoredEntry[]>>({});
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
  useEffect(() => { setClosed(null); setPath(null); setInside({}); }, [workspaceId]);
  // 对话里点了一个改过的文件：打开它（用完就清掉，之后在这里自己点别的不受它管）。
  useEffect(() => { if (filePath) { setPath(filePath); onFilePath(null); } }, [filePath, onFilePath]);

  // 这个任务删掉的文件也列着（划掉），点开看删之前的样子。
  const entries = useMemo(() => {
    const present = listing?.entries ?? [];
    const names = new Set(present.map((e) => e.path));
    const gone = undone ? [] : [...changed.values()].filter((f) => f.kind === "delete" && !names.has(f.path)).map((f) => ({ path: f.path, size: 0 }));
    const hidden = all ? [...(listing?.ignored ?? []), ...Object.values(inside).flat()].map((e) => ({ ...e, ignored: true })) : [];
    return [...present, ...gone, ...hidden];
  }, [listing, changed, undone, all, inside]);
  const tree = useMemo(() => buildTree(entries), [entries]);
  // 默认：第一层开着，再往下只开有改动的文件夹。
  const isClosed = (node: TreeNode, depth: number) => {
    if (closed?.has(node.path)) return true;
    if (closed?.has(`!${node.path}`)) return false;
    if (node.ignored) return true;
    return depth > 0 && ![...changed.keys()].some((p) => p.startsWith(`${node.path}/`));
  };
  const toggle = (node: TreeNode, depth: number) => {
    const next = new Set(closed ?? []);
    const now = isClosed(node, depth);
    next.delete(node.path); next.delete(`!${node.path}`);
    next.add(now ? `!${node.path}` : node.path);
    setClosed(next);
    if (now && node.ignored && workspaceId && !inside[node.path]) {
      void ipc.filesChildren(workspaceId, node.path).then((list) => setInside((prev) => ({ ...prev, [node.path]: list }))).catch(say);
    }
  };

  const mark = path && !undone ? changed.get(path) : undefined;
  useEffect(() => {
    let cancelled = false;
    setPreview(null); setMarks(new Set());
    if (!workspaceId || !path) return;
    void ipc.filesRead(workspaceId, path, run?.id).then((p) => { if (!cancelled) setPreview(p); }).catch((e) => { if (!cancelled) say(e); });
    if (run && mark && mark.kind !== "delete") {
      void ipc.changesGetDiff(run.id, path).then((d) => { if (!cancelled) setMarks(addedLines(d)); }).catch(() => {});
    }
    return () => { cancelled = true; };
    // 只有这个任务改到的文件才跟着任务重读（图片、PDF、Office 不在任务里改就不反复读、不反复起 Quick Look）。
  }, [workspaceId, path, run?.id, mark?.kind, mark?.additions, mark ? stamp : "", say]); // eslint-disable-line react-hooks/exhaustive-deps

  const rows: React.ReactNode[] = [];
  const walk = (nodes: TreeNode[], depth: number) => nodes.forEach((node) => {
    if (node.dir) {
      const shut = isClosed(node, depth);
      rows.push(<FileRow key={node.path} name={node.name} depth={depth} dim={node.ignored} dir={shut ? "closed" : "open"} onClick={() => toggle(node, depth)} />);
      if (!shut) walk(node.children, depth + 1);
    } else {
      rows.push(<FileRow key={node.path} name={node.name} title={node.path} depth={depth} dim={node.ignored} mark={undone ? undefined : changed.get(node.path)} on={node.path === path} onClick={() => setPath(node.path === path ? null : node.path)} />);
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
    : preview.kind === "office" && workspaceId ? <OfficePreview workspaceId={workspaceId} path={preview.path} say={say} />
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
  const note = !preview ? "" : elsewhere ? "改动还在独立工作区，这里是项目文件夹现在的样子" : preview.deleted ? "这个任务删了它，这里是删之前的样子" : "";
  return (
    <div className="pk-mod col-grow files">
      <ModHead label="FILES">
        {!!listing?.hidden && (
          <button type="button" className={cn("pk-chip pk-chip--sm", all && "is-on")} aria-pressed={all} title={`照 .gitignore 藏着的 ${listing.hidden} 个`} onClick={() => setAll(!all)}>
            <span className="pk-chip-label">ALL</span>
          </button>
        )}
        <Expand wide={wide} onWide={onWide} />
      </ModHead>
      {!project ? <p className="empty-line">选一个项目，这里列它的文件。</p> : (
        <div className={cn("files-body", wide && "files-body--wide")}>
          <div className="files-tree">
            <div className="pk-tree scroll">{listing ? rows : <p className="empty-line"><Twinkle /> 在读项目里的文件…</p>}</div>
            {listing && entries.length === 0 && <p className="empty-line">这个文件夹里还没有文件。</p>}
            {listing?.truncated && <p className="hint files-hidden">只列了前 4000 个</p>}
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
                {!preview?.deleted && <OpenBtns workspaceId={project.workspace.id} path={path} app={settings.editorApp} say={say} />}
              </p>
              {note && <p className="hint">{note}</p>}
              {body}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

// ---------- TERM ----------

const Term: React.FC<Props> = ({ project, wide, onWide, busy, term, onTermRun, onTermStop, onTermClear, say }) => {
  const [draft, setDraft] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [cursor, setCursor] = useState(-1);
  const endRef = useRef<HTMLDivElement>(null);
  const size = term.entries.reduce((n, e) => n + e.output.length, term.entries.length);
  useEffect(() => { endRef.current?.scrollIntoView({ block: "end" }); }, [size]);
  const workspaceId = project?.workspace.id ?? "";
  const [servers, setServers] = useState<BgInfo[]>([]);
  // 现在看的是哪个项目：读数回来时对不上就丢掉（切项目时上一个项目的不写进来，点了也就不会停错）。
  const shownId = useRef(workspaceId);
  shownId.current = workspaceId;
  const later = useRef(0);
  // 留着在跑的 dev server（3 秒看一次）。点一下停；要再起，在对话里跟模型说。
  const readServers = useCallback(() => {
    if (!workspaceId || document.visibilityState !== "visible") return;
    const id = workspaceId;
    ipc.backgroundList(id).then((b) => { if (shownId.current === id) setServers(b.filter((x) => x.kind === "server" && x.status === "running")); }).catch(() => {});
  }, [workspaceId]);
  useEffect(() => {
    setServers([]);
    if (!workspaceId) return;
    readServers();
    const t = window.setInterval(readServers, 3000);
    return () => { window.clearInterval(t); window.clearTimeout(later.current); };
  }, [workspaceId, readServers]);
  const stopServer = async (server: BgInfo) => {
    try { await ipc.backgroundStop(server.id); say(`dev server「${server.name}」停了`); } catch (e) { say(e); }
    window.clearTimeout(later.current);
    later.current = window.setTimeout(readServers, 300);
  };
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
      <ModHead label="TERM">
        {project && servers.map((server) => (
          <button key={server.id} type="button" className="pk-chip pk-chip--sm is-on" aria-pressed
            title={`dev server「${server.name}」在跑（${server.command}）：任务结束不停，退出 Peck 才停。点一下停掉`}
            onClick={() => void stopServer(server)}>
            <i className="pk-dot dot-live" />
            <span className="pk-chip-label">{server.name.toUpperCase()}</span>
          </button>
        ))}
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
            </div>
          )}
          <div className={cn("term-in", (locked || running) && "is-off")}>
            <span className="t-code term-p">{prompt(term.cwd)}</span>
            <input className="t-code" aria-label="一条命令" autoCorrect="off" autoCapitalize="off" value={locked ? "" : draft} disabled={locked || running} placeholder={locked || running ? "" : "打一条命令"} onChange={(e) => setDraft(e.target.value)} onKeyDown={onKey} spellCheck={false} autoComplete="off" />
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
