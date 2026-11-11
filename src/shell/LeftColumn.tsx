import React, { useState } from "react";
import type { ProjectOverview, RunRecord, TaskBrief } from "@/lib/types";
import { ago, clock, cn, runState, span, STATUS_WORD, stepName } from "@/lib/utils";
import { Btn, Field, Icon, ModHead, Twinkle } from "@/ui";

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

/** 左栏：项目和任务（没选项目时每个项目带最近几次任务；选了就是它的任务列表），下面是这个任务的记录。 */
export const LeftColumn: React.FC<{
  projects: ProjectOverview[];
  projectId: string | null;
  taskId: string | null;
  busyProjects: Set<string>;
  run: RunRecord | null;
  onSelectProject: (id: string | null) => void;
  onSelectTask: (projectId: string, taskId: string | null) => void;
  onOpen: (path?: string) => void;
  native: boolean;
}> = ({ projects, projectId, taskId, busyProjects, run, onSelectProject, onSelectTask, onOpen, native }) => {
  const [pathOpen, setPathOpen] = useState(false);
  const [path, setPath] = useState("");
  const now = Date.now();
  const submitPath = (e: React.FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    onOpen(path.trim());
    setPath("");
    setPathOpen(false);
  };

  return (
    <section className="col" aria-label="项目和任务">
      <div className="pk-mod left-projects">
        <ModHead label="PROJECTS" note={projectId ? <button type="button" className="link" onClick={() => onSelectProject(null)}>全部项目</button> : undefined} count={projects.length} />
        {projects.length === 0 ? (
          <div className="empty">
            <p>还没有项目。打开一个项目文件夹，Peck 就在里面干活。</p>
          </div>
        ) : (
          <div className="scroll left-list">
            {projects.map(({ workspace, tasks }) => {
              const on = workspace.id === projectId;
              const shown = on ? tasks : projectId ? tasks.slice(0, 1) : tasks.slice(0, RECENT);
              return (
                <div key={workspace.id} className="pj">
                  <button type="button" className={cn("pj-head", on && "is-on")} aria-expanded={on} onClick={() => onSelectProject(on ? null : workspace.id)}>
                    <Icon name="folder" />
                    <b className="t-name">{workspace.name}</b>
                    <span>{busyProjects.has(workspace.id) ? "在跑" : tasks.length ? `${tasks.length} 个任务` : "还没有任务"}</span>
                  </button>
                  <ul className="pj-tasks">
                    {on && (
                      <li>
                        <button type="button" className={cn("task-row task-row--new", !taskId && "is-on")} onClick={() => onSelectTask(workspace.id, null)}>
                          <Icon name="plus" size={12} />
                          <span className="t-label">NEW TASK</span>
                        </button>
                      </li>
                    )}
                    {shown.map((task) => (
                      <li key={task.id}>
                        <button type="button" className={cn("task-row", on && task.id === taskId && "is-on", !on && "task-row--quiet")} onClick={() => onSelectTask(workspace.id, task.id)}>
                          <span className="task-si"><TaskIcon task={task} /></span>
                          <span className="task-tt">
                            <b>{task.title}</b>
                            {(on || !projectId) && <em>{taskMeta(task, now)}</em>}
                          </span>
                        </button>
                      </li>
                    ))}
                    {!on && !projectId && tasks.length > RECENT && <li className="pj-more">还有 {tasks.length - RECENT} 个，点项目名看全部</li>}
                  </ul>
                </div>
              );
            })}
          </div>
        )}
        <div className="left-open">
          {pathOpen || !native ? (
            <form onSubmit={submitPath} className="left-path">
              <Field aria-label="项目文件夹的路径" placeholder="~/code/项目文件夹" value={path} onChange={(e) => setPath(e.target.value)} autoFocus={pathOpen} />
              <Btn sm type="submit" disabled={!path.trim()}>OPEN</Btn>
            </form>
          ) : (
            <div className="left-path">
              <Btn icon="folder" onClick={() => onOpen()}>OPEN FOLDER</Btn>
              <Btn v="ghost" sm onClick={() => setPathOpen(true)}>PATH</Btn>
            </div>
          )}
          <p className="hint">同一个项目一次跑一个任务</p>
        </div>
      </div>
      <TaskLog run={run} />
    </section>
  );
};

/** 这个任务的记录：存快照、每一步、做完、撤销。 */
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
    <div className="pk-mod left-log">
      <ModHead label="LOG" note="这个任务" />
      {items.length === 0 ? (
        <p className="empty-line">发任务以后，存快照、每一步、做完、撤销都记在这里。</p>
      ) : (
        <ul className="scroll log">
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
