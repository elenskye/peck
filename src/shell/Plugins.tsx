// 插件的界面（v1.1.0；v1.1.6 学 Claude Desktop 改成卡片）：设置 → PLUGINS 的 BUILT-IN、LIBRARY 两块，项目设置 → PLUGINS。
// 两栏卡片，每张只有图标、名字、简介；点开一个小窗：全部说明、按项目装和开关、它带的 Skill、命令（怎么打）、工具、Hook、MCP。

import React, { useEffect, useState } from "react";
import { ipc, type PluginInfo, type PluginList, type ProjectPlugin, type ProjectPlugins } from "@/lib/ipc";
import type { ProjectOverview } from "@/lib/types";
import { cn, shortenHomePath } from "@/lib/utils";
import { Btn, Info, Modal, Row, SetHead, Switch } from "@/ui";

/** Hook 什么时候跑（给人看）。 */
const HOOK_WHEN: Record<string, string> = { Stop: "一轮做完", SessionStart: "开场时", UserPromptSubmit: "发话时", PreToolUse: "调工具前", PostToolUse: "调工具后", PreCompact: "压缩前" };

interface DetailItem { name: string; text: string; warn?: boolean }

/** 一个插件带的东西，按种类分组。 */
function detailGroups(info: PluginInfo): Array<[string, DetailItem[]]> {
  const groups: Array<[string, DetailItem[]]> = [
    ["SKILL", info.skills.map((s) => ({ name: s.name, text: s.description }))],
    ["COMMAND", info.commands.map((c) => ({ name: `/${c.name}${c.hint ? ` ${c.hint}` : ""}`, text: c.description }))],
    ["TOOL", info.tools.map((t) => ({ name: t.name, text: t.known ? t.description : [t.description, "Peck 没有这个工具"].filter(Boolean).join(" · "), warn: !t.known }))],
    ["HOOK", info.hooks.map((h) => ({
      name: HOOK_WHEN[h.event] ?? h.event,
      text: h.kind === "peck" ? `问刚干完的模型「${h.ask ?? ""}」，再由 Peck 跑 ${h.run ?? ""}` : h.kind === "command" ? [h.run, "脚本"].filter(Boolean).join(" · ") : [h.run, "Peck 不认这种 Hook"].filter(Boolean).join(" · "),
      warn: h.kind !== "peck" && h.kind !== "command",
    }))],
    ["MCP", info.mcp.map((m) => ({ name: m.name, text: m.command }))],
  ];
  return groups.filter(([, items]) => items.length > 0);
}

/** 它带的东西：一类一组。 */
const PluginDetail: React.FC<{ info: PluginInfo }> = ({ info }) => {
  const groups = detailGroups(info);
  if (groups.length === 0) return <p className="hint">什么都没带。</p>;
  return (
    <dl className="plugin-detail sel">
      {groups.map(([label, items]) => (
        <React.Fragment key={label}>
          <dt className="t-label">{label}</dt>
          <dd>
            {items.map((item, i) => (
              <p key={`${label}-${i}`} className={cn(item.warn && "is-warn")}><code className="t-code">{item.name}</code><span>{item.text}</span></p>
            ))}
          </dd>
        </React.Fragment>
      ))}
    </dl>
  );
};

/** 按名字算出来的 5×5 像素图标（左右对称）：同一个插件哪里都是同一个样子。 */
function iconCells(name: string): boolean[] {
  let h = 2166136261;
  for (const ch of name) { h ^= ch.codePointAt(0) ?? 0; h = Math.imul(h, 16777619); }
  h ^= h >>> 13; h = Math.imul(h, 0x5bd1e995); h ^= h >>> 15;
  const cells: boolean[] = [];
  for (let r = 0; r < 5; r++) for (let c = 0; c < 5; c++) cells.push(((h >>> (r * 3 + (c < 3 ? c : 4 - c))) & 1) === 1);
  if (cells.filter(Boolean).length < 7) for (const i of [2, 7, 12, 17, 22]) cells[i] = true;
  return cells;
}

/** 插件是青色，单独的 Skill 是淡紫。 */
const PixelIcon: React.FC<{ info: PluginInfo; big?: boolean }> = ({ info, big }) => (
  <span className={cn("plug-icon", big && "plug-icon--big", info.kind === "skill" && "is-skill")} aria-hidden="true">
    {iconCells(info.name).map((on, i) => <i key={i} className={on ? "is-on" : undefined} />)}
  </span>
);

/** 一张卡：图标、名字、两行简介；暗的是这个项目里没装 / 关着。 */
const PluginCard: React.FC<{ info: PluginInfo; off?: boolean; badge?: React.ReactNode; onOpen: () => void }> = ({ info, off, badge, onOpen }) => (
  <button type="button" className={cn("plug-card", off && "is-off")} aria-haspopup="dialog" onClick={onOpen} title={info.description}>
    <PixelIcon info={info} />
    <span className="plug-card-text">
      <span className="plug-card-name"><b className="t-name">{info.title}</b>{badge}</span>
      <span className="plug-card-desc">{info.description}</span>
    </span>
  </button>
);

const sourceTag = (info: PluginInfo) => (info.source === "builtin" ? "BUILT-IN" : info.kind === "skill" ? "SKILL" : "LIBRARY");

/** 小窗：上面图标、名字、版本、全部说明；中间按项目装 / 开关（设置里列左栏的全部项目，库里的多一行「每个项目都装」）；下面它带的东西。 */
const PluginModal: React.FC<{
  info: PluginInfo;
  projects: ProjectOverview[];
  /** 设置里开的：库里的多一行「每个项目都装」。 */
  every?: { on: boolean; busy: boolean; onChange: (on: boolean) => void };
  /** 改了某个项目（装、更新、移除、开关）以后。 */
  onChanged: (workspaceId: string) => Promise<void> | void;
  onClose: () => void;
  say: (m: unknown) => void;
}> = ({ info, projects, every, onChanged, onClose, say }) => {
  const builtin = info.source === "builtin";
  const [views, setViews] = useState<Record<string, ProjectPlugin | null>>({});
  const [busy, setBusy] = useState<string | null>(null);
  // REMOVE 要点两次（删的是项目里的文件）。
  const [armed, setArmed] = useState<string | null>(null);
  const pick = (v: ProjectPlugins) => [...v.builtin, ...v.library].find((p) => p.name === info.name) ?? null;
  const ids = projects.filter((p) => !p.missing).map((p) => p.workspace.id);
  useEffect(() => {
    let live = true;
    for (const id of ids) ipc.projectPlugins(id).then((v) => { if (live) setViews((m) => ({ ...m, [id]: pick(v) })); }).catch(say);
    return () => { live = false; };
  }, [ids.join(","), info.name]); // eslint-disable-line react-hooks/exhaustive-deps
  const run = async (p: ProjectOverview, fn: () => Promise<ProjectPlugins>, message: string) => {
    setBusy(p.workspace.id);
    setArmed(null);
    try {
      const v = await fn();
      setViews((m) => ({ ...m, [p.workspace.id]: pick(v) }));
      await onChanged(p.workspace.id);
      say(message);
    } catch (e) { say(e); } finally { setBusy(null); }
  };
  const toggle = (p: ProjectOverview, on: boolean) => run(p, async () => {
    const settings = p.workspace.settings ?? {};
    const off = settings.pluginsOff ?? [];
    await ipc.workspaceUpdate(p.workspace.id, { settings: { ...settings, pluginsOff: on ? off.filter((n) => n !== info.name) : [...off.filter((n) => n !== info.name), info.name] } });
    return ipc.projectPlugins(p.workspace.id);
  }, on ? `${info.title} 在 ${p.workspace.name} 里开了` : `${p.workspace.name} 里不用 ${info.title} 了：它的 Skill 和工具都不给`);
  return (
    <Modal label={info.kind === "skill" ? "SKILL" : "PLUGIN"} onClose={onClose}>
      <div className="plug-hero">
        <PixelIcon info={info} big />
        <div>
          <p className="plug-hero-name"><b>{info.title}</b><span className="set-ver t-code">{info.version}</span><em className="pk-tag">{sourceTag(info)}</em></p>
          <p className="sel">{info.description}</p>
        </div>
      </div>
      {every && (
        <div className="set-rows">
          <Row name="每个项目都装" off={!every.on}><Switch label={`${info.title} 每个项目都装`} on={every.on} disabled={every.busy} onChange={every.onChange} /></Row>
        </div>
      )}
      <SetHead label={projects.length === 1 && !every ? "THIS PROJECT" : "PROJECTS"} />
      <div className="set-rows set-rows--acts">
        {projects.map((p) => {
          const id = p.workspace.id;
          const v = views[id];
          const path = shortenHomePath(p.workspace.path);
          const locked = busy !== null;
          if (p.missing) return <Row key={id} name={p.workspace.name} off desc={`文件夹不在了 · ${path}`}><em className="pk-tag pk-tag--wait">MISSING</em></Row>;
          if (v === undefined) return <Row key={id} name={p.workspace.name} off desc="在读…" />;
          if (v === null) return <Row key={id} name={p.workspace.name} off desc={`这个项目里没有 · ${path}`} />;
          if (builtin) {
            return (
              <Row key={id} name={p.workspace.name} off={v.off} desc={path}>
                <Switch label={`${info.title} 在 ${p.workspace.name} 里开着`} on={!v.off} disabled={locked} onChange={(on) => void toggle(p, on)} />
              </Row>
            );
          }
          const state = v.installed ? `装着 ${v.installed}${v.update && v.latest ? ` · 库里有 ${v.latest}` : ""}` : !v.latest ? "库里没有了" : "没装";
          return (
            <Row key={id} name={p.workspace.name} off={!v.installed} desc={`${state} · ${path}`}>
              {v.installed && v.update && <Btn sm v="primary" disabled={locked} onClick={() => void run(p, () => ipc.pluginInstall(id, info.name), `${info.title} 在 ${p.workspace.name} 里更新到 ${v.latest}`)}>UPDATE</Btn>}
              {v.installed ? (
                <Btn sm v="danger" disabled={locked} aria-label={armed === id ? `再点一次从 ${p.workspace.name} 移除` : `从 ${p.workspace.name} 移除`}
                  onClick={() => (armed === id ? void run(p, () => ipc.pluginRemove(id, info.name), `${info.title} 从 ${p.workspace.name} 里移除了，以后不再自动装`) : setArmed(id))}>{armed === id ? "CONFIRM" : "REMOVE"}</Btn>
              ) : (
                <Btn sm disabled={locked || !v.latest} onClick={() => void run(p, () => ipc.pluginInstall(id, info.name), `${info.title} 装进 ${p.workspace.name} 了`)}>INSTALL</Btn>
              )}
            </Row>
          );
        })}
        {projects.length === 0 && <p className="hint">左栏还没有项目。</p>}
      </div>
      <SetHead label="CONTENTS" />
      <PluginDetail info={info} />
    </Modal>
  );
};

/** 设置 → PLUGINS 的上两块：Peck 自带的、插件库里的，两栏卡片；点开按项目装、开关，库里的还有「每个项目都装」。 */
export const PluginsSettings: React.FC<{ projects: ProjectOverview[]; onProjectsChanged: () => Promise<void>; say: (m: unknown) => void }> = ({ projects, onProjectsChanged, say }) => {
  const [list, setList] = useState<PluginList | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { ipc.pluginList().then(setList).catch(say); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const every = async (p: PluginInfo, on: boolean) => {
    setBusy(true);
    try { setList(await ipc.pluginEveryProject(p.name, on)); say(on ? `${p.title}：以后点开的项目都装上` : `${p.title}：不再自动装（装了的不动）`); } catch (e) { say(e); } finally { setBusy(false); }
  };
  const shown = list && [...list.builtin, ...list.library].find((p) => p.name === open);
  const grid = (items: PluginInfo[] | undefined) => (
    items ? (items.length ? <div className="plug-grid">{items.map((p) => <PluginCard key={p.name} info={p} onOpen={() => setOpen(p.name)} />)}</div> : <p className="hint">还没有。</p>)
      : <p className="hint">在读…</p>
  );
  return (
    <>
      <SetHead label="BUILT-IN" />
      {grid(list?.builtin)}
      <SetHead label="LIBRARY" />
      {grid(list?.library)}
      {shown && (
        <PluginModal info={shown} projects={projects.filter((p) => !p.workspace.hidden)} say={say} onClose={() => setOpen(null)}
          every={shown.source === "builtin" ? undefined : { on: shown.everyProject, busy, onChange: (on) => void every(shown, on) }}
          onChanged={() => onProjectsChanged()} />
      )}
    </>
  );
};

/** 项目设置 → PLUGINS：一样的两栏卡片（这个项目里关着、没装的暗一些，库里有新版的多一个青色版本号）；点开小窗在这个项目里装、开关。 */
export const ProjectPluginsSection: React.FC<{
  project: ProjectOverview;
  say: (m: unknown) => void;
  /** 装了、更新了、移除了、开关了：项目设置重读、下面的 SKILLS 重读。 */
  onChanged: () => Promise<void>;
}> = ({ project, say, onChanged }) => {
  const { workspace, missing } = project;
  const [view, setView] = useState<ProjectPlugins | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const load = () => ipc.projectPlugins(workspace.id).then(setView).catch(say);
  useEffect(() => { if (!missing) void load(); }, [workspace.id, missing]); // eslint-disable-line react-hooks/exhaustive-deps
  const shown = view && [...view.builtin, ...view.library].find((p) => p.name === open);
  const badge = (p: ProjectPlugin) => (p.update ? <em className="pk-tag pk-tag--rec">{p.latest}</em> : p.source !== "builtin" && !p.latest ? <em className="pk-tag">NOT IN LIBRARY</em> : undefined);
  return (
    <>
      <p className="lbl">PLUGINS<Info text={`点开一个看它带的 Skill、命令、工具、Hook，在这个项目里装、开关。Peck 自带的按项目开关，关了它的 Skill 和工具都不给。库里的 INSTALL 拷进这个项目（Skill 进 ${view?.skillsDir ?? ".claude/skills"}，其余的进旁边的 plugins/），之后是项目自己的文件、跟着项目的 git 走；库里有新版出 UPDATE，点了才覆盖。REMOVE 删掉项目里那一份，以后不再自动装。设置 → PLUGINS 里能一次看全部项目。`} /></p>
      {view ? (
        <div className="plug-grid">
          {view.builtin.map((p) => <PluginCard key={p.name} info={p} off={p.off} onOpen={() => setOpen(p.name)} />)}
          {view.library.map((p) => <PluginCard key={p.name} info={p} off={!p.installed} badge={badge(p)} onOpen={() => setOpen(p.name)} />)}
        </div>
      ) : (
        <p className="hint">{missing ? "文件夹不在了。" : "在读…"}</p>
      )}
      {shown && (
        <PluginModal info={shown} projects={[project]} say={say} onClose={() => setOpen(null)}
          onChanged={async () => { await load(); await onChanged(); }} />
      )}
    </>
  );
};
