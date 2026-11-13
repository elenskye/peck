import React, { useEffect, useState } from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { ipc, type AllowRule } from "@/lib/ipc";
import type { Permission, ProjectOverview } from "@/lib/types";
import { ago, PERMISSIONS, shortenHomePath } from "@/lib/utils";
import { Btn, Field, ModHead, Row, Seg } from "@/ui";
import { WhoPicker, pickable } from "@/shell/ModelPicker";

const RULE_WORD: Record<string, string> = { "install-deps": "装依赖", "long-running": "起常驻进程" };

/** 项目设置（右键项目 → 项目设置）：名字、文件夹、在这个项目里默认谁来干和哪一档、「总是允许」过的、移出列表。 */
export const ProjectPage: React.FC<{
  project: ProjectOverview;
  entries: ModelEntry[];
  fakeAgent: boolean;
  native: boolean;
  onChanged: () => Promise<void>;
  onRelocate: (id: string) => void;
  onReveal: (id: string) => void;
  onRemove: (id: string) => void;
  onClose: () => void;
  say: (message: unknown) => void;
}> = ({ project, entries, fakeAgent, native, onChanged, onRelocate, onReveal, onRemove, onClose, say }) => {
  const { workspace, tasks, missing } = project;
  const [name, setName] = useState(workspace.name);
  const [rules, setRules] = useState<AllowRule[] | null>(null);
  const [armed, setArmed] = useState(false);
  useEffect(() => setName(workspace.name), [workspace.name]);
  useEffect(() => {
    let live = true;
    ipc.harnessAllowList().then((all) => { if (live) setRules(all.filter((r) => r.workspaceId === workspace.id)); }).catch(say);
    return () => { live = false; };
  }, [workspace.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const settings = workspace.settings ?? {};
  const save = async (patch: Parameters<typeof ipc.workspaceUpdate>[1], message: string) => {
    try { await ipc.workspaceUpdate(workspace.id, patch); await onChanged(); say(message); } catch (e) { say(e); }
  };
  const options = pickable(entries, fakeAgent);
  const who = options.find((e) => e.id === settings.who) ?? null;
  return (
    <section className="col page" aria-label="项目设置">
      <div className="pk-mod col-grow page-mod">
        <ModHead label="PROJECT" note={workspace.name}><Btn v="ghost" sm icon="x" onClick={onClose}>CLOSE</Btn></ModHead>
        <div className="set-pane scroll">
          <h2 className="t-title">项目设置</h2>
          <p className="lead">只管这一个项目；空着的跟设置里的全局默认。{tasks.length ? `这个项目有 ${tasks.length} 个任务。` : ""}</p>
          <div className="set-rows">
            <div className="set-block">
              <Row name="名字" desc="左栏和顶栏显示的名字；文件夹本身不改名。" />
              <div className="set-inline">
                <Field aria-label="项目的名字" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && name.trim() && name.trim() !== workspace.name) void save({ name }, "名字改好了"); }} />
                <Btn sm disabled={!name.trim() || name.trim() === workspace.name} onClick={() => void save({ name }, "名字改好了")}>SAVE</Btn>
              </div>
            </div>
            <div className="set-block">
              <Row name="文件夹" off={missing} desc={<><span className="t-code">{shortenHomePath(workspace.path)}</span>{missing ? " · 文件夹不在了" : ""}</>}>
                {missing ? <em className="pk-tag pk-tag--wait">MISSING</em> : <em className="pk-tag pk-tag--ok">OK</em>}
              </Row>
              <div className="set-inline">
                <Btn sm icon="folder" disabled={!native || missing} onClick={() => onReveal(workspace.id)}>SHOW IN FINDER</Btn>
                <Btn sm onClick={() => onRelocate(workspace.id)}>CHANGE FOLDER</Btn>
                <span className="hint">换文件夹：任务记录、快照、「总是允许」都跟过去。</span>
              </div>
            </div>
            <Row name="默认谁来干" desc={settings.who ? "打开这个项目时，输入框下的模型列表换成它。" : "跟全局：上一次选的那个。"}>
              <WhoPicker entries={options} value={who} up={false} onChange={(id) => void save({ settings: { ...settings, who: id } }, "这个项目默认用它了")} />
              {settings.who && <Btn sm v="ghost" onClick={() => void save({ settings: { ...settings, who: null } }, "改回跟全局")}>CLEAR</Btn>}
            </Row>
            <Row name="默认权限" desc={settings.permission ? PERMISSIONS.find((p) => p.id === settings.permission)?.does : "跟全局：设置 → 权限里的默认档。"}>
              <Seg<Permission | "global"> label="这个项目的默认权限" sm
                items={[{ id: "global", label: "GLOBAL" }, ...PERMISSIONS.map((p) => ({ id: p.id, label: p.label }))]}
                value={(settings.permission as Permission | undefined) ?? "global"}
                onChange={(p) => void save({ settings: { ...settings, permission: p === "global" ? null : p } }, p === "global" ? "改回跟全局" : `这个项目默认 ${p.toUpperCase()}`)} />
            </Row>
          </div>

          <p className="lbl">ALWAYS ALLOWED</p>
          {rules && rules.length > 0 ? (
            <div className="set-rows">
              {rules.map((r) => (
                <Row key={r.rule} name={RULE_WORD[r.rule] ?? r.rule} desc={`EDIT 档里不再问 · ${ago(r.createdAt)}`}>
                  <Btn sm v="danger" onClick={() => void ipc.harnessAllowRemove(r.workspaceId, r.rule).then((all) => { setRules(all.filter((x) => x.workspaceId === workspace.id)); say("撤掉了，下次又会问"); }).catch(say)}>REMOVE</Btn>
                </Row>
              ))}
            </div>
          ) : (
            <p className="hint">{rules ? "这个项目还没有。EDIT 档装依赖、起常驻进程时弹卡，点 ALWAYS 就记在这里。" : "在读…"}</p>
          )}

          <p className="lbl">REMOVE</p>
          <div className="set-inline">
            <Btn v="danger" icon="trash" onClick={() => { if (!armed) { setArmed(true); return; } onRemove(workspace.id); }}>{armed ? "CONFIRM" : "REMOVE FROM LIST"}</Btn>
            <span className="hint">{armed ? "再点一次就移出。" : "只是从左栏拿掉：文件夹、任务记录、快照都不动，重新打开这个文件夹就回来。"}</span>
          </div>
        </div>
      </div>
    </section>
  );
};
