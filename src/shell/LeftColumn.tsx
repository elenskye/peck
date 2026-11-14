import React, { useEffect, useRef, useState } from "react";
import type { ProjectOverview, TaskBrief } from "@/lib/types";
import { ago, clock, cn, plainLine, runState, STATUS_WORD } from "@/lib/utils";
import { Btn, FoldBtn, Field, Icon, Info, ModHead, Twinkle, type IconName } from "@/ui";

const RECENT = 3;

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

export type MenuEntry =
  | { label: string; icon: IconName; hint?: string; danger?: boolean; disabled?: boolean; confirm?: string; onClick: () => void }
  | "sep";

/** 右键菜单：贴着鼠标出现，点外面、Esc、滚动都收起。带 confirm 的项点第一下只换成确认的字，再点才做。 */
export const ContextMenu: React.FC<{ x: number; y: number; items: MenuEntry[]; onClose: () => void }> = ({ x, y, items, onClose }) => {
  const ref = useRef<HTMLDivElement>(null);
  const [armed, setArmed] = useState<number | null>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const onClose = () => closeRef.current();
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      setPos({ left: Math.max(8, Math.min(x, window.innerWidth - r.width - 8)), top: Math.max(8, Math.min(y, window.innerHeight - r.height - 8)) });
      el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    }
    const down = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) onClose(); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    document.addEventListener("scroll", onClose, true);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
      document.removeEventListener("scroll", onClose, true);
    };
  }, [x, y]);
  return (
    <div ref={ref} className="pk-menu ctx-menu" role="menu" style={{ left: pos.left, top: pos.top }} onContextMenu={(e) => e.preventDefault()}>
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
        if (e.key === "Enter" && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229) finish(text.trim() && text.trim() !== value ? text.trim() : null);
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
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onDeleteTask: (projectId: string, taskId: string) => void;
  onReorderProjects: (ids: string[]) => void;
  onReorderTasks: (projectId: string, ids: string[]) => void;
}

/** 左栏：项目和任务（没选项目时每个项目带最近几次任务；选了就是它的任务列表），下面是 AGENTS（谁来干）。两块都能收起，收起的只剩头，另一块占满。 */
export const LeftColumn: React.FC<{
  /** 待你处理、在跑（空着不显示）。 */
  inbox: InboxItem[];
  projects: ProjectOverview[];
  projectId: string | null;
  taskId: string | null;
  busyProjects: Set<string>;
  /** 下面那一块（AGENTS）。 */
  agents: React.ReactNode;
  folded: boolean;
  onFold: (folded: boolean) => void;
  onSelectProject: (id: string | null) => void;
  onSelectTask: (projectId: string, taskId: string | null) => void;
  onOpen: (path?: string) => void;
  native: boolean;
  actions: LeftActions;
}> = ({ inbox, projects, projectId, taskId, busyProjects, agents, folded, onFold, onSelectProject, onSelectTask, onOpen, native, actions }) => {
  const [pathOpen, setPathOpen] = useState(false);
  const [path, setPath] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuEntry[] } | null>(null);
  const [renaming, setRenaming] = useState<{ kind: "project" | "task"; id: string } | null>(null);
  const sort = useSortable((group, ids) => (group === "projects" ? actions.onReorderProjects(ids) : actions.onReorderTasks(group.slice(6), ids)));
  const now = Date.now();
  const submitPath = (e: React.FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    onOpen(path.trim());
    setPath("");
    setPathOpen(false);
  };
  const open = (e: React.MouseEvent, items: MenuEntry[]) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, items }); };
  const projectMenu = (p: ProjectOverview): MenuEntry[] => {
    const id = p.workspace.id;
    const busy = busyProjects.has(id);
    return [
      { label: "重命名", icon: "pen", onClick: () => setRenaming({ kind: "project", id }) },
      { label: "项目设置", icon: "gear", onClick: () => actions.onProjectSettings(id) },
      p.missing
        ? { label: "重新找文件夹", icon: "folder", hint: "文件夹不在了", disabled: busy, onClick: () => actions.onRelocateProject(id) }
        : { label: "在访达里显示", icon: "folder", disabled: !native, onClick: () => actions.onRevealProject(id) },
      "sep",
      { label: "移出列表", icon: "trash", danger: true, disabled: busy, hint: "文件夹和记录不动", confirm: "再点一次：移出列表", onClick: () => actions.onRemoveProject(id) },
    ];
  };
  const taskMenu = (p: ProjectOverview, t: TaskBrief): MenuEntry[] => {
    const running = t.lastRun ? runState(t.lastRun) === "running" : false;
    return [
      { label: "重命名", icon: "pen", onClick: () => { onSelectProject(p.workspace.id); setRenaming({ kind: "task", id: t.id }); } },
      "sep",
      { label: "删除任务", icon: "trash", danger: true, disabled: running, hint: running ? "在跑" : "连同记录", confirm: "再点一次：删掉，不能撤回", onClick: () => actions.onDeleteTask(p.workspace.id, t.id) },
    ];
  };

  return (
    <section className="col" aria-label="项目和任务">
      {inbox.length > 0 && !folded && <Inbox items={inbox} taskId={taskId} onSelectTask={(p, t) => onSelectTask(p, t)} />}
      <div className={cn("pk-mod left-projects", folded && "is-folded")}>
        <ModHead label="PROJECTS" after={<button type="button" className="head-icon" aria-label="新增环境：打开一个项目文件夹" title="新增环境：打开一个项目文件夹" onClick={() => (native ? onOpen() : setPathOpen((v) => !v))}><Icon name="folder" size={12} /></button>} note={folded ? projects.find((p) => p.workspace.id === projectId)?.workspace.name : projectId ? <button type="button" className="link" onClick={() => onSelectProject(null)}>全部项目</button> : undefined} count={projects.length}>
          <FoldBtn name="PROJECTS" folded={folded} onFold={onFold} />
        </ModHead>
        {/* 浏览器演示里没有系统选文件夹的窗：点文件夹图标出一行路径。 */}
        {pathOpen && !native && (
          <form onSubmit={submitPath} className="left-path">
            <Field aria-label="项目文件夹的路径" placeholder="~/code/项目文件夹" value={path} onChange={(e) => setPath(e.target.value)} autoFocus />
            <Btn sm type="submit" disabled={!path.trim()}>OPEN</Btn>
          </form>
        )}
        {projects.length === 0 ? (
          <div className="empty">
            <p>还没有项目<Info text="点 PROJECTS 旁边的文件夹图标，打开一个项目文件夹，Peck 就在里面干活。" /></p>
          </div>
        ) : (
          <div className={cn("scroll left-list", sort.dragging && "is-sorting")} data-sort-group="projects">
            {projects.map((p, pi) => {
              const { workspace, tasks, missing } = p;
              const on = workspace.id === projectId;
              const shown = on ? tasks : projectId ? tasks.slice(0, 1) : tasks.slice(0, RECENT);
              const group = `tasks:${workspace.id}`;
              return (
                <div key={workspace.id} className={cn("pj", sort.rowClass("projects", workspace.id, pi, projects.length))} data-sort-id={workspace.id}>
                  {renaming?.kind === "project" && renaming.id === workspace.id ? (
                    <div className={cn("pj-head is-renaming", on && "is-on")}>
                      <Icon name="folder" />
                      <RenameField value={workspace.name} label="项目的新名字" onDone={(next) => { setRenaming(null); if (next) actions.onRenameProject(workspace.id, next); }} />
                    </div>
                  ) : (
                    <button type="button" className={cn("pj-head", on && "is-on", missing && "is-missing")} aria-expanded={on} title={workspace.path}
                      onPointerDown={(e) => sort.start(e, "projects", workspace.id)}
                      onClick={() => { if (!sort.swallow()) onSelectProject(on ? null : workspace.id); }}
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
                            <RenameField value={plainLine(task.title)} label="任务的新名字" onDone={(next) => { setRenaming(null); if (next) actions.onRenameTask(workspace.id, task.id, next); }} />
                          </div>
                        ) : (
                          <button type="button" className={cn("task-row", on && task.id === taskId && "is-on", !on && "task-row--quiet")}
                            onPointerDown={on ? (e) => sort.start(e, group, task.id) : undefined}
                            onClick={() => { if (!sort.swallow()) onSelectTask(workspace.id, task.id); }}
                            onContextMenu={(e) => open(e, taskMenu(p, task))}>
                            <span className="task-si"><TaskIcon task={task} /></span>
                            <span className="task-tt">
                              <b>{plainLine(task.title)}</b>
                              {(on || !projectId) && <em>{taskMeta(task, now)}</em>}
                            </span>
                          </button>
                        )}
                      </li>
                    ))}
                    {!on && !projectId && tasks.length > RECENT && <li className="pj-more">还有 {tasks.length - RECENT} 个，点项目名看全部</li>}
                  </ul>
                </div>
              );
            })}
          </div>
        )}
      </div>
      {agents}
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
    </section>
  );
};

