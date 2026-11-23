// 插件的界面（v1.1.0）：设置 → PLUGINS 的 BUILT-IN、LIBRARY 两块，项目设置 → PLUGINS。
// 一个插件一行，点名字展开看它带的 Skill、命令（怎么打）、工具、Hook、MCP。

import React, { useEffect, useState } from "react";
import { ipc, type PluginInfo, type PluginList, type ProjectPlugin, type ProjectPlugins } from "@/lib/ipc";
import type { ProjectSettings } from "@/lib/types";
import { cn } from "@/lib/utils";
import { Btn, Icon, Info, Row, Switch } from "@/ui";

/** Hook 什么时候跑（给人看）。 */
const HOOK_WHEN: Record<string, string> = { Stop: "一轮做完" };

interface DetailItem { name: string; text: string; warn?: boolean }

/** 一个插件带的东西，按种类分组。 */
function detailGroups(info: PluginInfo): Array<[string, DetailItem[]]> {
  const groups: Array<[string, DetailItem[]]> = [
    ["SKILL", info.skills.map((s) => ({ name: s.name, text: s.description }))],
    ["COMMAND", info.commands.map((c) => ({ name: `/${c.name}${c.hint ? ` ${c.hint}` : ""}`, text: c.description }))],
    ["TOOL", info.tools.map((t) => ({ name: t.name, text: t.known ? t.description : [t.description, "Peck 没有这个工具"].filter(Boolean).join(" · "), warn: !t.known }))],
    ["HOOK", info.hooks.map((h) => ({
      name: HOOK_WHEN[h.event] ?? h.event,
      text: h.kind === "peck" ? `问刚干完的模型「${h.ask ?? ""}」，再由 Peck 跑 ${h.run ?? ""}` : [h.run, "任意脚本，Peck 不跑"].filter(Boolean).join(" · "),
      warn: h.kind !== "peck",
    }))],
    ["MCP", info.mcp.map((m) => ({ name: m.name, text: m.command }))],
  ];
  return groups.filter(([, items]) => items.length > 0);
}

/** 展开的那一块。 */
export const PluginDetail: React.FC<{ info: PluginInfo }> = ({ info }) => {
  const groups = detailGroups(info);
  if (groups.length === 0) return <p className="hint plugin-detail">这个插件什么都没带。</p>;
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

/** 一个插件一行：名字（点了展开）、版本、说明，右边放开关或按钮。 */
export const PluginRow: React.FC<{ info: PluginInfo; note?: React.ReactNode; off?: boolean; children?: React.ReactNode }> = ({ info, note, off, children }) => {
  const [open, setOpen] = useState(false);
  return (
    <div className="set-block">
      <Row off={off} desc={info.description}
        name={<>
          <button type="button" className="plugin-name" aria-expanded={open} onClick={() => setOpen(!open)}>
            <Icon name={open ? "triD" : "triR"} size={12} />{info.title}
          </button>
          <span className="set-ver t-code">{info.version}</span>
          {info.kind === "skill" && <em className="pk-tag">SKILL</em>}
          {note}
        </>}>
        {children}
      </Row>
      {open && <PluginDetail info={info} />}
    </div>
  );
};

/** 设置 → PLUGINS 的上两块：Peck 自带的、插件库里的（「每个项目都装」的开关）。 */
export const PluginsSettings: React.FC<{ say: (m: unknown) => void }> = ({ say }) => {
  const [list, setList] = useState<PluginList | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { ipc.pluginList().then(setList).catch(say); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const every = async (p: PluginInfo, on: boolean) => {
    setBusy(true);
    try { setList(await ipc.pluginEveryProject(p.name, on)); say(on ? `${p.title}：以后点开的项目都装上` : `${p.title}：不再自动装（装了的不动）`); } catch (e) { say(e); } finally { setBusy(false); }
  };
  return (
    <>
      <p className="lbl">BUILT-IN</p>
      <div className="set-rows">
        {list?.builtin.map((p) => <PluginRow key={p.name} info={p} />)}
        {!list && <p className="hint">在读…</p>}
      </div>
      <p className="lbl">LIBRARY</p>
      <div className="set-rows">
        {list?.library.map((p) => (
          <PluginRow key={p.name} info={p}>
            <span className="t-label plugin-every">EVERY PROJECT</span>
            <Switch label={`${p.title} 每个项目都装`} on={p.everyProject} disabled={busy} onChange={(on) => void every(p, on)} />
          </PluginRow>
        ))}
        {list && list.library.length === 0 && <p className="hint">还没有。</p>}
        {!list && <p className="hint">在读…</p>}
      </div>
    </>
  );
};

/** 项目设置 → PLUGINS：自带的按项目开关；库里的 INSTALL / UPDATE / REMOVE。 */
export const ProjectPluginsSection: React.FC<{
  workspaceId: string;
  settings: ProjectSettings;
  missing: boolean;
  /** 上一下还在存。 */
  saving?: boolean;
  save: (patch: { settings?: ProjectSettings }, message: string) => Promise<void>;
  say: (m: unknown) => void;
  /** 装了、更新了、移除了：项目里的 Skill、命令变了。 */
  onChanged: () => void;
}> = ({ workspaceId, settings, missing, saving, save, say, onChanged }) => {
  const [view, setView] = useState<ProjectPlugins | null>(null);
  const [busy, setBusy] = useState(false);
  // REMOVE 要点两次（删的是项目里的文件）。
  const [armed, setArmed] = useState<string | null>(null);
  const off = settings.pluginsOff ?? [];
  useEffect(() => {
    if (missing) return;
    let live = true;
    ipc.projectPlugins(workspaceId).then((v) => { if (live) setView(v); }).catch(say);
    return () => { live = false; };
  }, [workspaceId, missing, off.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (p: ProjectPlugin, on: boolean) =>
    save({ settings: { ...settings, pluginsOff: on ? off.filter((n) => n !== p.name) : [...off, p.name] } }, on ? `${p.title} 在这个项目里开了` : `这个项目里不用 ${p.title} 了：它的 Skill 和工具都不给`);
  const act = async (p: ProjectPlugin, what: "install" | "update" | "remove") => {
    setBusy(true);
    setArmed(null);
    try {
      setView(what === "remove" ? await ipc.pluginRemove(workspaceId, p.name) : await ipc.pluginInstall(workspaceId, p.name));
      say(what === "install" ? `${p.title} 装进这个项目了` : what === "update" ? `${p.title} 更新到 ${p.latest}` : `${p.title} 从这个项目里移除了，以后不再自动装`);
      onChanged();
    } catch (e) { say(e); } finally { setBusy(false); }
  };
  const locked = missing || busy || saving;
  return (
    <>
      <p className="lbl">PLUGINS<Info text={`点名字看它带的 Skill、命令、工具、Hook。Peck 自带的按项目开关，关了它的 Skill 和工具都不给。库里的点 INSTALL 拷进这个项目（Skill 进 ${view?.skillsDir ?? ".claude/skills"}，其余的进旁边的 plugins/），之后是项目自己的文件、跟着项目的 git 走；库里有新版出 UPDATE，点了才覆盖。REMOVE 删掉项目里那一份，以后不再自动装。`} /></p>
      {view ? (
        <div className="set-rows">
          {view.builtin.map((p) => (
            <PluginRow key={p.name} info={p} off={p.off} note={<em className="pk-tag">BUILT-IN</em>}>
              <Switch label={`${p.title} 开着`} on={!p.off} disabled={locked} onChange={(on) => void toggle(p, on)} />
            </PluginRow>
          ))}
          {view.library.map((p) => (
            <PluginRow key={p.name} info={p} off={!p.installed}
              note={p.update ? <em className="pk-tag pk-tag--rec">{p.latest}</em> : !p.latest ? <em className="pk-tag">NOT IN LIBRARY</em> : undefined}>
              {!p.installed && <Btn sm disabled={locked} onClick={() => void act(p, "install")}>INSTALL</Btn>}
              {p.installed && p.update && <Btn sm v="primary" disabled={locked} onClick={() => void act(p, "update")}>UPDATE</Btn>}
              {p.installed && <Btn sm v="danger" disabled={locked} onClick={() => (armed === p.name ? void act(p, "remove") : setArmed(p.name))}>{armed === p.name ? "CONFIRM" : "REMOVE"}</Btn>}
            </PluginRow>
          ))}
        </div>
      ) : (
        <p className="hint">{missing ? "文件夹不在了。" : "在读…"}</p>
      )}
    </>
  );
};
