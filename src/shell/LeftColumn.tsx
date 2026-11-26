import React, { useEffect, useRef, useState } from "react";
import type { ProjectOverview, TaskBrief } from "@/lib/types";
import type { ModelEntry } from "@/lib/agentEvents";
import type { Schedule } from "@/lib/ipc";
import { dayClock, scheduleWhen, scheduleWho } from "@/lib/schedules";
import { ago, clock, cn, plainEnter, plainLine, runState, STATUS_WORD } from "@/lib/utils";
import { Btn, Float, FoldBtn, Field, Icon, ModHead, Twinkle, type IconName } from "@/ui";

/** 全部项目页每个项目带几次最近的任务。 */
const RECENT = 5;

function taskMeta(task: TaskBrief, now: number): string {
  const last = task.lastRun;
  if (!last) return ago(task.updatedAt, now);
  const state = runState(last);
  if (state === "running") return `在跑 · ${clock(last.startedAt)} 开始`;
  const when = ago(last.undoneAt ?? last.finishedAt ?? last.startedAt, now);
  if (state === "succeeded") return `完成 · ${when} · ${last.files.length} 个文件`;
  return `${STATUS_WORD[state]} · ${when}`;
}

const TaskIcon: React.FC<{ task: TaskBrief }> = ({ task }) => {
  const state = task.lastRun ? runState(task.lastRun) : "succeeded";
  if (state === "running") return <Twinkle />;
  return <Icon size={12} name={state === "succeeded" ? "check" : state === "undone" ? "undo" : state === "cancelled" ? "stop" : "x"} className={`task-ic task-ic--${state}`} />;
};

// ---------- 右键菜单 ----------

type MenuEntry =
  | { label: string; icon: IconName; hint?: string; danger?: boolean; disabled?: boolean; confirm?: string; onClick: () => void }
  | "sep";

/** 右键菜单：贴着鼠标出现（位置交给 Float：靠窗口边就往回让、太长就压矮滚动），点外面、Esc、滚动都收起。
 *  带 confirm 的项点第一下只换成确认的字，再点才做。 */
const ContextMenu: React.FC<{ x: number; y: number; items: MenuEntry[]; onClose: () => void }> = ({ x, y, items, onClose }) => {
  const ref = useRef<HTMLDivElement>(null);
  const [armed, setArmed] = useState<number | null>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const onClose = () => closeRef.current();
    ref.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const down = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) onClose(); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    // 菜单太长压矮了能在里面滚：自己里面的滚动不收。
    const scroll = (e: Event) => { if (!(e.target instanceof Node && ref.current?.contains(e.target))) onClose(); };
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    document.addEventListener("scroll", scroll, true);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
      document.removeEventListener("scroll", scroll, true);
    };
  }, [x, y]);
  return (
    <Float point={{ x, y }} gap={0} floatRef={ref} className="ctx-menu" onContextMenu={(e) => e.preventDefault()}>
      <div className="pk-menu" role="menu">
        <div className="pk-menu-scroll">
          {items.map((item, i) =>
            item === "sep" ? <div key={`sep-${i}`} className="pk-menu-sep" /> : (
              <button key={item.label} type="button" role="menuitem" disabled={item.disabled} className={cn("pk-menu-action", item.danger && "ctx-danger", armed === i && "is-armed")}
                onClick={() => {
                  if (item.confirm && armed !== i) { setArmed(i); return; }
                  onClose();
                  item.onClick();
                }}>
                <Icon name={item.icon} size={12} />
                {armed === i ? item.confirm : item.label}
                {item.hint && armed !== i && <em>{item.hint}</em>}
              </button>
            ),
          )}
        </div>
      </div>
    </Float>
  );
};

// ---------- 拖动排顺序 ----------

/**
 * 按住一行往上或往下拖（动了 4 像素以上才算拖，不然还是点）。放手时把新顺序交出去。
 * 用指针事件自己做，不用 HTML 拖放：原生窗口里拖放会被当成拖文件进来。
 */
function useSortable(onReorder: (group: string, ids: string[]) => void) {
  const [drag, setDrag] = useState<{ group: string; id: string; over: number } | null>(null);
  const moved = useRef(false);
  const start = (e: React.PointerEvent, group: string, id: string) => {
    if (e.button !== 0) return;
    const list = (e.currentTarget as HTMLElement).closest<HTMLElement>(`[data-sort-group="${group}"]`);
    if (!list) return;
    const y0 = e.clientY;
    let dragging = false;
    let over = -1;
    const rows = () => [...list.querySelectorAll<HTMLElement>(":scope > [data-sort-id]")];
    const move = (ev: PointerEvent) => {
      if (!dragging && Math.abs(ev.clientY - y0) < 4) return;
      dragging = true;
      const all = rows();
      over = all.length;
      for (let i = 0; i < all.length; i++) {
        const r = all[i].getBoundingClientRect();
        if (ev.clientY < r.top + r.height / 2) { over = i; break; }
      }
      setDrag({ group, id, over });
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setDrag(null);
      if (!dragging) return;
      moved.current = true;
      window.setTimeout(() => { moved.current = false; }, 0);
      const ids = rows().map((r) => r.dataset.sortId!);
      const from = ids.indexOf(id);
      if (from < 0) return;
      const next = ids.filter((x) => x !== id);
      next.splice(over > from ? over - 1 : over, 0, id);
      if (next.join() !== ids.join()) onReorder(group, next);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  /** 刚拖完的那一下「点」不算。 */
  const swallow = () => moved.current;
  const rowClass = (group: string, id: string, index: number, count: number) => cn(
    drag?.group === group && drag.id === id && "is-dragging",
    drag?.group === group && drag.over === index && "drop-before",
    drag?.group === group && index === count - 1 && drag.over === count && "drop-after",
  );
  return { start, swallow, rowClass, dragging: drag != null };
}

// ---------- 改名 ----------

const RenameField: React.FC<{ value: string; label: string; onDone: (next: string | null) => void }> = ({ value, label, onDone }) => {
  const [text, setText] = useState(value);
  const done = useRef(false);
  const finish = (next: string | null) => { if (done.current) return; done.current = true; onDone(next); };
  return (
    <input className="rename-in" aria-label={label} value={text} autoFocus spellCheck={false}
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (plainEnter(e)) finish(text.trim() && text.trim() !== value ? text.trim() : null);
        if (e.key === "Escape") finish(null);
      }}
      onBlur={() => finish(text.trim() && text.trim() !== value ? text.trim() : null)} />
  );
};

// ---------- 待你处理、在跑（v0.7） ----------

/** 左栏上面一块的一行：等你批或答、做完没看、失败停下没看、在跑。跨项目。 */
export interface InboxItem {
  projectId: string;
  taskId: string;
  title: string;
  project: string;
  kind: "waiting" | "done" | "failed" | "running";
  at: number;
}

const INBOX_WORD: Record<InboxItem["kind"], string> = { waiting: "等你", done: "做完了", failed: "停下了", running: "在跑" };

const Inbox: React.FC<{ items: InboxItem[]; taskId: string | null; onSelectTask: (projectId: string, taskId: string) => void }> = ({ items, taskId, onSelectTask }) => {
  const todo = items.filter((i) => i.kind !== "running").length;
  return (
    <div className="pk-mod left-inbox">
      <ModHead label="NOW" count={todo > 0 ? todo : undefined} note={items.length - todo > 0 ? `${items.length - todo} 个在跑` : undefined} />
      <ul className="inbox-list">
        {items.slice(0, 8).map((i) => (
          <li key={`${i.kind}:${i.taskId}`}>
            <button type="button" className={cn("task-row inbox-row", `inbox-row--${i.kind}`, i.taskId === taskId && "is-on")} onClick={() => onSelectTask(i.projectId, i.taskId)}>
              <span className="task-si">{i.kind === "running" ? <Twinkle /> : <Icon size={12} name={i.kind === "waiting" ? "ask" : i.kind === "done" ? "check" : "x"} />}</span>
              <span className="task-tt">
                <b>{i.title}</b>
                <em>{INBOX_WORD[i.kind]} · {i.project}</em>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
};

// ---------- 左栏 ----------

export interface LeftActions {
  onRenameProject: (id: string, name: string) => void;
  onProjectSettings: (id: string) => void;
  onRevealProject: (id: string) => void;
  onRelocateProject: (id: string) => void;
  onRemoveProject: (id: string) => void;
  onRenameTask: (taskId: string, title: string) => void;
  onDeleteTask: (taskId: string) => void;
  onArchiveTask: (taskId: string) => void;
  onArchiveProjectTasks: (projectId: string) => void;
  onReorderProjects: (ids: string[]) => void;
  onReorderTasks: (projectId: string, ids: string[]) => void;
}

/** 浏览器演示里没有系统选文件夹的窗：出一行路径。 */
const PathForm: React.FC<{ onOpen: (path: string) => void }> = ({ onOpen }) => {
  const [path, setPath] = useState("");
  return (
    <form className="left-path" onSubmit={(e) => { e.preventDefault(); if (path.trim()) { onOpen(path.trim()); setPath(""); } }}>
      <Field aria-label="项目文件夹的路径" placeholder="~/code/项目文件夹" value={path} onChange={(e) => setPath(e.target.value)} autoFocus />
      <Btn sm type="submit" disabled={!path.trim()}>OPEN</Btn>
    </form>
  );
};

interface ListProps {
  projects: ProjectOverview[];
  projectId: string | null;
  taskId: string | null;
  busyProjects: Set<string>;
  onSelectProject: (id: string) => void;
  onSelectTask: (projectId: string, taskId: string | null) => void;
  native: boolean;
  actions: LeftActions;
  /** 全部项目页：每个项目带最近几次任务；不然是左栏（选中的项目列全部任务，别的只带最近一次）。 */
  all?: boolean;
}

/** 项目和它们的任务：右键菜单、改名、拖动排顺序。左栏和全部项目页共用。 */
const ProjectList: React.FC<ListProps> = ({ projects, projectId, taskId, busyProjects, onSelectProject, onSelectTask, native, actions, all }) => {
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuEntry[] } | null>(null);
  const [renaming, setRenaming] = useState<{ kind: "project" | "task"; id: string } | null>(null);
  const sort = useSortable((group, ids) => (group === "projects" ? actions.onReorderProjects(ids) : actions.onReorderTasks(group.slice(6), ids)));
  const now = Date.now();
  const open = (e: React.MouseEvent, items: MenuEntry[]) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, items }); };
  const projectMenu = (p: ProjectOverview): MenuEntry[] => {
    const id = p.workspace.id;
    const busy = busyProjects.has(id);
    return [
      { label: "重命名", icon: "pen", onClick: () => setRenaming({ kind: "project", id }) },
      { label: "项目设置", icon: "gear", onClick: () => actions.onProjectSettings(id) },
      p.missing
        ? { label: "重新找文件夹", icon: "folder", disabled: busy, onClick: () => actions.onRelocateProject(id) }
        : { label: "在访达里显示", icon: "folder", disabled: !native, onClick: () => actions.onRevealProject(id) },
      { label: "归档全部任务", icon: "archive", disabled: !p.tasks.length, hint: busy ? "在跑的不归档" : undefined, confirm: `再点一次：归档 ${p.tasks.length} 个任务`, onClick: () => actions.onArchiveProjectTasks(id) },
      "sep",
      { label: "移出列表", icon: "trash", danger: true, disabled: busy, hint: "文件夹和记录不动", confirm: "再点一次：移出列表", onClick: () => actions.onRemoveProject(id) },
    ];
  };
  const taskMenu = (p: ProjectOverview, t: TaskBrief): MenuEntry[] => {
    const running = t.lastRun ? runState(t.lastRun) === "running" : false;
    return [
      { label: "重命名", icon: "pen", onClick: () => { if (!all) onSelectProject(p.workspace.id); setRenaming({ kind: "task", id: t.id }); } },
      { label: "归档", icon: "archive", disabled: running, hint: running ? "在跑" : "记录留着", onClick: () => actions.onArchiveTask(t.id) },
      "sep",
      { label: "删除任务", icon: "trash", danger: true, disabled: running, hint: running ? "在跑" : "连同记录", confirm: "再点一次：删掉，不能撤回", onClick: () => actions.onDeleteTask(t.id) },
    ];
  };
  const recent = all ? RECENT : 1;
  return (
    <div className={cn("scroll left-list", sort.dragging && "is-sorting")} data-sort-group="projects">
      {projects.map((p, pi) => {
        const { workspace, tasks, missing } = p;
        const on = !all && workspace.id === projectId;
        const shown = on ? tasks : tasks.slice(0, recent);
        const group = `tasks:${workspace.id}`;
        return (
          <div key={workspace.id} className={cn("pj", sort.rowClass("projects", workspace.id, pi, projects.length))} data-sort-id={workspace.id}>
            {renaming?.kind === "project" && renaming.id === workspace.id ? (
              <div className={cn("pj-head is-renaming", on && "is-on")}>
                <Icon name="folder" />
                <RenameField value={workspace.name} label="项目的新名字" onDone={(next) => { setRenaming(null); if (next) actions.onRenameProject(workspace.id, next); }} />
              </div>
            ) : (
              <button type="button" className={cn("pj-head", on && "is-on", missing && "is-missing")} aria-current={on || undefined} title={workspace.path}
                onPointerDown={(e) => sort.start(e, "projects", workspace.id)}
                onClick={() => { if (!sort.swallow()) onSelectProject(workspace.id); }}
                onContextMenu={(e) => open(e, projectMenu(p))}>
                <Icon name={missing ? "bang" : "folder"} />
                <b className="t-name">{workspace.name}</b>
                <span className={cn(missing && "is-warn")}>{missing ? "文件夹不在了" : busyProjects.has(workspace.id) ? "在跑" : tasks.length ? `${tasks.length} 个任务` : "还没有任务"}</span>
              </button>
            )}
            <ul className="pj-tasks" data-sort-group={group}>
              {on && !missing && (
                <li>
                  <button type="button" className={cn("task-row task-row--new", !taskId && "is-on")} onClick={() => onSelectTask(workspace.id, null)}>
                    <Icon name="plus" size={12} />
                    <span className="t-label">NEW TASK</span>
                  </button>
                </li>
              )}
              {shown.map((task, ti) => (
                <li key={task.id} data-sort-id={on ? task.id : undefined} className={on ? sort.rowClass(group, task.id, ti, shown.length) : undefined}>
                  {renaming?.kind === "task" && renaming.id === task.id ? (
                    <div className="task-row is-on is-renaming">
                      <span className="task-si"><TaskIcon task={task} /></span>
                      <RenameField value={plainLine(task.title)} label="任务的新名字" onDone={(next) => { setRenaming(null); if (next) actions.onRenameTask(task.id, next); }} />
                    </div>
                  ) : (
                    <button type="button" className={cn("task-row", on && task.id === taskId && "is-on", !on && !all && "task-row--quiet")}
                      onPointerDown={on ? (e) => sort.start(e, group, task.id) : undefined}
                      onClick={() => { if (!sort.swallow()) onSelectTask(workspace.id, task.id); }}
                      onContextMenu={(e) => open(e, taskMenu(p, task))}>
                      <span className="task-si"><TaskIcon task={task} /></span>
                      <span className="task-tt">
                        <b>{plainLine(task.title)}</b>
                        {(on || all) && <em>{taskMeta(task, now)}</em>}
                      </span>
                    </button>
                  )}
                </li>
              ))}
              {all && tasks.length > recent && <li className="pj-more">还有 {tasks.length - recent} 个</li>}
            </ul>
          </div>
        );
      })}
      {/* 挂在模块外面：模块的像素切角会裁掉放在里面的弹层。 */}
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
    </div>
  );
};

type Shared = ListProps & {
  /** 待你处理、在跑（空着不显示）。 */
  inbox: InboxItem[];
  onOpen: (path?: string) => void;
  /** 设好的定时任务（PROJECTS 头上一个钟，点开看）。 */
  schedules: ScheduleBundle;
};

interface ScheduleBundle {
  list: Schedule[];
  entries: ModelEntry[];
  /** 去它跑的地方（错过的卡片在那里）。 */
  onGo: (s: Schedule) => void;
  onRemove: (id: string) => void;
}

/** PROJECTS 头上的钟：设好的定时任务有几个，点开一个个看（什么时候、发哪句话、谁来干、上一次怎么样）。
 *  设、改、取消都在对话里跟模型说；这里只留一个 REMOVE（到点没人看着自己跑的东西，要有个不用开对话就能关掉的地方）。 */
const ScheduleChip: React.FC<ScheduleBundle & { projects: ProjectOverview[]; end?: boolean }> = ({ list, entries, onGo, onRemove, projects, end }) => {
  const [open, setOpen] = useState(false);
  const [shown, setShown] = useState<string | null>(null);
  const [armed, setArmed] = useState<string | null>(null);
  const ref = useRef<HTMLSpanElement>(null);
  if (list.length === 0) return null;
  const missed = list.filter((s) => s.missedAt).length;
  const sorted = [...list].sort((a, b) => Number(!!b.missedAt) - Number(!!a.missedAt) || (a.nextAt ?? Infinity) - (b.nextAt ?? Infinity));
  const projectOf = (s: Schedule) => projects.find((p) => p.workspace.id === s.workspaceId);
  const close = () => { setOpen(false); setArmed(null); };
  return (
    <span className="menu-anchor" ref={ref}>
      <button type="button" className={cn("head-icon sched-chip", end && "head-icon--end", open && "is-on")} aria-haspopup="dialog" aria-expanded={open}
        aria-label={`定时任务 ${list.length} 个${missed ? `，${missed} 个错过了` : ""}`} title={`定时任务 ${list.length} 个${missed ? `，${missed} 个错过了` : ""}`} onClick={() => (open ? close() : setOpen(true))}>
        <Icon name="clock" size={12} /><em>{list.length}</em>{missed > 0 && <i className="pk-badge pk-badge--wait" />}
      </button>
      {open && (
        <Float anchor={ref} side="bottom" onClose={close} className="pk-anim-reveal-s menu-pop sched-pop">
          <div className="pk-menu" role="dialog" aria-label="定时任务">
            <div className="pk-menu-scroll">
              <p className="pk-menu-group">定时任务<em className="menu-group-note">在对话里跟模型说，就能设、改、取消</em></p>
              {sorted.map((s) => {
                const p = projectOf(s);
                const target = s.session ? p?.tasks.find((t) => t.id === s.session) : undefined;
                const last = s.lastSession ? p?.tasks.find((t) => t.id === s.lastSession) : undefined;
                return (
                  <div key={s.id} className={cn("sched-item", shown === s.id && "is-open")}>
                    <button type="button" className={cn("pk-menu-item", shown === s.id && "is-on")} aria-expanded={shown === s.id} onClick={() => { setShown(shown === s.id ? null : s.id); setArmed(null); }}>
                      <Icon name={shown === s.id ? "triD" : "triR"} size={12} />
                      <b>{s.title}</b>
                      <span>{s.missedAt ? "错过了" : s.nextAt ? dayClock(s.nextAt) : "没有下一次"}</span>
                    </button>
                    {shown === s.id && (
                      <div className="sched-detail">
                        <p className="sched-say sel">{s.prompt}</p>
                        <dl>
                          <dt>项目</dt><dd>{p?.workspace.name ?? "不在列表里了"}</dd>
                          <dt>什么时候</dt><dd>{scheduleWhen(s)}</dd>
                          <dt>下一次</dt><dd>{s.nextAt ? dayClock(s.nextAt) : "没有了"}</dd>
                          <dt>谁来干</dt><dd>{scheduleWho(s, entries)} · {s.permission.toUpperCase()}</dd>
                          <dt>在哪跑</dt><dd>{target ? `接在「${plainLine(target.title)}」里` : "每次开一个新任务"}</dd>
                          {(s.lastNote || s.lastRunAt) && <><dt>上一次</dt><dd>{[s.lastNote, s.lastRunAt ? dayClock(s.lastRunAt) : null].filter(Boolean).join(" · ")}</dd></>}
                          {s.missedAt && <><dt>错过的</dt><dd>原定 {dayClock(s.missedAt)}</dd></>}
                        </dl>
                        <div className="set-inline">
                          {s.missedAt && p && <Btn sm v="primary" onClick={() => { onGo(s); close(); }}>去点卡片</Btn>}
                          {!s.missedAt && last && p && <Btn sm v="ghost" onClick={() => { onGo({ ...s, session: last.id }); close(); }}>上一次的任务</Btn>}
                          <span className="grow" />
                          <Btn sm v="danger" onClick={() => { if (armed !== s.id) { setArmed(s.id); return; } setArmed(null); setShown(null); onRemove(s.id); }}>{armed === s.id ? "CONFIRM" : "REMOVE"}</Btn>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </Float>
      )}
    </span>
  );
};

/** 左栏：PROJECTS（选中的项目列全部任务，别的只带最近一次）和下面的 AGENTS（谁来干）。两块都能收起，收起的只剩头，另一块占满。
 *  在全部项目页（`all`）时项目列表在中间，这里一整块都是 AGENTS。 */
export const LeftColumn: React.FC<Omit<Shared, "all" | "onSelectProject"> & {
  all: boolean;
  /** `null` 是去全部项目页。 */
  onSelectProject: (id: string | null) => void;
  /** 下面那一块（AGENTS）。 */
  agents: React.ReactNode;
  folded: boolean;
  onFold: (folded: boolean) => void;
  /** 顶栏上收起了左栏（v1.1.12）：藏起来不卸掉。 */
  hidden?: boolean;
}> = ({ inbox, all, agents, folded, onFold, onOpen, schedules, hidden, ...list }) => {
  const [pathOpen, setPathOpen] = useState(false);
  const cls = cn("col", hidden && "is-hidden");
  if (all) return <section className={cls} aria-label="谁来干" aria-hidden={hidden || undefined}>{agents}</section>;
  const { projects, native, onSelectProject, onSelectTask } = list;
  return (
    <section className={cls} aria-label="项目和任务" aria-hidden={hidden || undefined}>
      {inbox.length > 0 && !folded && <Inbox items={inbox} taskId={list.taskId} onSelectTask={onSelectTask} />}
      <div className={cn("pk-mod left-projects", folded && "is-folded")}>
        <ModHead label="PROJECTS" after={<button type="button" className="head-icon" aria-label="新增环境：打开一个项目文件夹" title="新增环境：打开一个项目文件夹" onClick={() => (native ? onOpen() : setPathOpen((v) => !v))}><Icon name="folder" size={12} /></button>}>
          <ScheduleChip {...schedules} projects={projects} end />
          <button type="button" className="head-icon head-icon--end" aria-label="全部项目" title="全部项目" onClick={() => onSelectProject(null)}><Icon name="expand" size={12} /></button>
          <FoldBtn name="PROJECTS" folded={folded} onFold={onFold} />
        </ModHead>
        {pathOpen && !native && <PathForm onOpen={(path) => { onOpen(path); setPathOpen(false); }} />}
        <ProjectList {...list} />
      </div>
      {agents}
    </section>
  );
};

/** 全部项目页（中栏）：所有项目，每个带最近几次任务；点一个项目或任务就进去。 */
export const ProjectsPage: React.FC<Omit<Shared, "all" | "projectId">> = ({ inbox, onOpen, schedules, ...list }) => {
  const [pathOpen, setPathOpen] = useState(false);
  const { projects, native } = list;
  return (
    <section className="col" aria-label="全部项目">
      {inbox.length > 0 && <Inbox items={inbox} taskId={list.taskId} onSelectTask={list.onSelectTask} />}
      <div className="pk-mod col-grow all-projects">
        <ModHead label="PROJECTS">
          <ScheduleChip {...schedules} projects={projects} />
          <Btn sm icon="folder" onClick={() => (native ? onOpen() : setPathOpen((v) => !v))}>OPEN FOLDER</Btn>
        </ModHead>
        {pathOpen && !native && <PathForm onOpen={(path) => { onOpen(path); setPathOpen(false); }} />}
        {projects.length === 0 ? <p className="empty-line">还没有项目。</p> : <ProjectList {...list} projectId={null} all />}
      </div>
    </section>
  );
};
