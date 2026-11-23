import React, { useEffect, useState } from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { ipc, type AllowRule, type MemoryNote, type ProjectMcpView, type SkillRow, type SkillVersion, type SlashList, type ToolHint } from "@/lib/ipc";
import type { Permission, ProjectOverview, ProjectSettings } from "@/lib/types";
import { ago, cn, PERMISSIONS, ruleWord, shortenHomePath } from "@/lib/utils";
import { Btn, Dot, Field, Icon, Info, MenuChip, ModHead, Row, Seg, Switch } from "@/ui";
import { WhoPicker, pickable } from "@/shell/ModelPicker";
import { presetProblems, presetSummary, type Preset } from "@/lib/presets";


/** 项目设置（右键项目 → 项目设置）：名字、文件夹、在这个项目里默认谁来干和哪一档、MCP、Skills、记忆、「总是允许」过的、归档全部任务、移出列表。 */
export const ProjectPage: React.FC<{
  project: ProjectOverview;
  entries: ModelEntry[];
  native: boolean;
  onChanged: () => Promise<void>;
  onRelocate: (id: string) => void;
  onReveal: (id: string) => void;
  onRemove: (id: string) => void;
  onArchiveAll: (id: string) => void;
  /** 存着的合作预设（v0.9 B2）：给项目指定默认用哪套。 */
  presets: Preset[];
  onClose: () => void;
  say: (message: unknown) => void;
}> = ({ project, entries, native, onChanged, onRelocate, onReveal, onRemove, onArchiveAll, presets, onClose, say }) => {
  const { workspace, tasks, missing } = project;
  const [name, setName] = useState(workspace.name);
  const [rules, setRules] = useState<AllowRule[] | null>(null);
  const [armed, setArmed] = useState(false);
  const [archiveArmed, setArchiveArmed] = useState(false);
  const [memories, setMemories] = useState<MemoryNote[] | null>(null);
  useEffect(() => setName(workspace.name), [workspace.name]);
  useEffect(() => {
    let live = true;
    ipc.memoryList(workspace.id).then((m) => { if (live) setMemories(m); }).catch(say);
    return () => { live = false; };
  }, [workspace.id]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    let live = true;
    ipc.harnessAllowList().then((all) => { if (live) setRules(all.filter((r) => r.workspaceId === workspace.id)); }).catch(say);
    return () => { live = false; };
  }, [workspace.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const settings = workspace.settings ?? {};
  // 存设置、读回来以前开关都不让再拨：每一下都是拿 props 里的设置改的，连着拨会把前一下盖掉（v0.10 收尾审查）。
  const [saving, setSaving] = useState(false);
  const save = async (patch: Parameters<typeof ipc.workspaceUpdate>[1], message: string) => {
    setSaving(true);
    try { await ipc.workspaceUpdate(workspace.id, patch); await onChanged(); say(message); } catch (e) { say(e); } finally { setSaving(false); }
  };
  // 项目的默认谁来干：一个 API 模型或一个 CLI（接力、审查在 AGENTS 里搭）。
  const options = pickable(entries).filter((e) => e.group !== "flow");
  const folders = settings.folders ?? [];
  const [folderPath, setFolderPath] = useState("");
  const envText = (settings.env ?? []).map((v) => `${v.name}=${v.value}`).join("\n");
  const [envDraft, setEnvDraft] = useState(envText);
  useEffect(() => setEnvDraft(envText), [workspace.id, envText]);
  const saveFolders = (next: typeof folders, message: string) => save({ settings: { ...settings, folders: next } }, message);
  const addFolder = async () => {
    const path = folderPath.trim() || (native ? await ipc.pickFolder().catch((e) => { say(e); return null; }) : null);
    if (!path) return;
    setFolderPath("");
    await saveFolders([...folders, { path, writable: false }], "加好了（默认只读）");
  };
  const saveEnv = () => {
    const env = envDraft.split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("#")).map((l) => {
      const at = l.indexOf("=");
      return at < 0 ? { name: l, value: "" } : { name: l.slice(0, at).trim().replace(/^export\s+/, ""), value: l.slice(at + 1).trim().replace(/^"(.*)"$/, "$1") };
    });
    void save({ settings: { ...settings, env } }, "环境变量存好了");
  };
  const who = options.find((e) => e.id === settings.who) ?? null;
  const preset = presets.find((p) => p.id === settings.preset) ?? null;
  return (
    <section className="col page" aria-label="项目设置">
      <div className="pk-mod col-grow page-mod is-big">
        <ModHead label="PROJECT"><Btn v="ghost" sm icon="x" onClick={onClose}>CLOSE</Btn></ModHead>
        <div className="set-pane scroll">
          <h2 className="t-title">项目设置<Info text={`只管这一个项目；空着的跟设置里的全局默认。${tasks.length ? `这个项目有 ${tasks.length} 个任务。` : ""}`} /></h2>
          <div className="set-rows">
            <div className="set-block">
              <Row name="名字" info="左栏和顶栏显示的名字；文件夹本身不改名。" />
              <div className="set-inline">
                <Field aria-label="项目的名字" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && name.trim() && name.trim() !== workspace.name) void save({ name }, "名字改好了"); }} />
                <Btn sm disabled={!name.trim() || name.trim() === workspace.name} onClick={() => void save({ name }, "名字改好了")}>SAVE</Btn>
              </div>
            </div>
            <div className="set-block">
              <Row name="文件夹" off={missing} desc={<span className="t-code">{shortenHomePath(workspace.path)}</span>}>
                {missing ? <em className="pk-tag pk-tag--wait">MISSING</em> : <em className="pk-tag pk-tag--ok">OK</em>}
              </Row>
              <div className="set-inline">
                <Btn sm icon="folder" disabled={!native || missing} onClick={() => onReveal(workspace.id)}>SHOW IN FINDER</Btn>
                <Btn sm onClick={() => onRelocate(workspace.id)}>CHANGE FOLDER</Btn>
                <Info text="换文件夹：任务记录、快照、「总是允许」都跟过去。" />
              </div>
            </div>
            <Row name="默认预设" desc={preset ? presetSummary(preset, entries) : settings.preset ? "选的预设删掉了" : "不用"} info="打开这个项目时，AGENTS 套用这套合作预设（写规范、实施、审查、委派）；有它就不看下面的「默认谁来干」。预设在左栏 AGENTS 最上面那行管。">
              <MenuChip sm disabled={saving} ariaLabel="这个项目的默认预设" label={preset?.name ?? "不用"}>
                {(close) => (
                  <>
                    <button type="button" role="menuitem" className={cn("pk-menu-item", !settings.preset && "is-on")} onClick={() => { void save({ settings: { ...settings, preset: null } }, "这个项目不用预设了"); close(); }}>
                      <Dot kind="" /><b>不用</b><span>看「默认谁来干」</span>
                    </button>
                    {presets.map((p) => {
                      const problems = presetProblems(p, entries);
                      return (
                        <button key={p.id} type="button" role="menuitem" className={cn("pk-menu-item", settings.preset === p.id && "is-on", problems.length > 0 && "is-off")} title={problems.join("；") || undefined}
                          onClick={() => { void save({ settings: { ...settings, preset: p.id } }, `这个项目默认用「${p.name}」`); close(); }}>
                          <Dot kind="" /><b>{p.name}</b><span>{problems[0] ?? presetSummary(p, entries)}</span>
                        </button>
                      );
                    })}
                  </>
                )}
              </MenuChip>
            </Row>
            <Row name="默认谁来干" off={!!preset} desc={preset ? "用了预设，这一行不看" : settings.who ? undefined : "跟全局"} info="打开这个项目时，左栏 AGENTS 换成它；空着就用上一次选的那个。">
              <WhoPicker entries={options} value={who} up={false} onChange={(id) => void save({ settings: { ...settings, who: id } }, "这个项目默认用它了")} />
              {settings.who && <Btn sm v="ghost" onClick={() => void save({ settings: { ...settings, who: null } }, "改回跟全局")}>CLEAR</Btn>}
            </Row>
            <Row name="默认权限" info="GLOBAL 跟设置 → 权限里的默认档。">
              <Seg<Permission | "global"> label="这个项目的默认权限" sm
                items={[{ id: "global", label: "GLOBAL" }, ...PERMISSIONS.map((p) => ({ id: p.id, label: p.label }))]}
                value={(settings.permission as Permission | undefined) ?? "global"}
                onChange={(p) => void save({ settings: { ...settings, permission: p === "global" ? null : p } }, p === "global" ? "改回跟全局" : `这个项目默认 ${p.toUpperCase()}`)} />
            </Row>
          </div>

          <p className="lbl">FOLDERS<Info text="项目即环境：主文件夹以外的文件夹，每个选只读（READ）或能改（WRITE）。 Peck Loop 的工具用绝对路径读写它们，命令的沙箱放开能改的那几个；能改的文件夹的改动也进 CHANGES、能撤销。CLI 引擎也认：能改的用它自己的开关挂上（Claude Code、Codex），只读的在每一句后面写明。" /></p>
          <div className="set-rows">
            <Row name={<span className="t-code">{shortenHomePath(workspace.path)}</span>}>
              <em className="pk-tag pk-tag--ok">MAIN</em>
            </Row>
            {folders.map((f, i) => (
              <Row key={f.path} name={<span className="t-code">{shortenHomePath(f.path)}</span>}>
                <Seg<"rw" | "ro"> label="这个文件夹能不能改" sm items={[{ id: "ro", label: "READ" }, { id: "rw", label: "WRITE" }]} value={f.writable ? "rw" : "ro"}
                  onChange={(v) => void saveFolders(folders.map((x, j) => (j === i ? { ...x, writable: v === "rw" } : x)), v === "rw" ? "这个文件夹能改了" : "这个文件夹只读了")} />
                <Btn sm v="danger" onClick={() => void saveFolders(folders.filter((_, j) => j !== i), "拿掉了")}>REMOVE</Btn>
              </Row>
            ))}
          </div>
          <div className="set-inline">
            <Field aria-label="要加的文件夹的路径" placeholder={native ? "空着点 ADD 选文件夹" : "~/code/shared-lib"} value={folderPath} onChange={(e) => setFolderPath(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") void addFolder(); }} />
            <Btn sm icon="folder" disabled={missing || (!native && !folderPath.trim())} onClick={() => void addFolder()}>ADD</Btn>
          </div>

          <p className="lbl">ENV<Info text="一行一个 NAME=value。 Peck Loop 的命令（包括它起的 dev server）、TERM、CLI 引擎的进程都带上（同名的盖过你终端里的；PATH 不带进 CLI）；值只存在 Peck 的数据库里，不进模型上下文（Peck Loop 看得到名字）。" /></p>
          <textarea className="text-edit" aria-label="这个项目的环境变量" spellCheck={false} disabled={missing} value={envDraft}
            placeholder={"API_BASE=http://localhost:8080\nRUST_LOG=info"} rows={Math.min(12, Math.max(3, envDraft.split("\n").length + 1))} onChange={(e) => setEnvDraft(e.target.value)} />
          <div className="set-inline">
            <Btn sm disabled={missing || envDraft === envText} onClick={saveEnv}>SAVE</Btn>
            {envDraft !== envText && <Btn sm v="ghost" onClick={() => setEnvDraft(envText)}>RESET</Btn>}
          </div>

          <ProjectMcp workspaceId={workspace.id} settings={settings} missing={!!missing} saving={saving} save={save} say={say} />

          <ProjectSkills workspaceId={workspace.id} settings={settings} missing={!!missing} saving={saving} save={save} say={say} />

          <p className="lbl">MEMORY<Info text="Peck Loop 觉得值得记的，任务结束时弹一张「想记住这几条」，你批了才记在这里；每次发任务都带上。" /></p>
          {memories && memories.length > 0 ? (
            <div className="set-rows">
              {memories.map((m) => (
                <Row key={m.id} name={<span className="memory-text">{m.text}</span>} desc={`${m.sourceRun ? `任务 ${m.sourceRun.replace(/^run-/, "")} 提的` : "手工记的"} · ${ago(m.createdAt)}`}>
                  <Btn sm v="danger" onClick={() => void ipc.memoryDelete(workspace.id, m.id).then((next) => { setMemories(next); say("删掉了，以后不再带上"); }).catch(say)}>DELETE</Btn>
                </Row>
              ))}
            </div>
          ) : (
            <p className="hint">{memories ? "还没有。" : "在读…"}</p>
          )}

          <p className="lbl">ALWAYS ALLOWED<Info text="EDIT 档装依赖、起常驻进程时弹卡，点 ALWAYS 就记在这里。" /></p>
          {rules && rules.length > 0 ? (
            <div className="set-rows">
              {rules.map((r) => (
                <Row key={r.rule} name={ruleWord(r.rule)} desc={ago(r.createdAt)}>
                  <Btn sm v="danger" onClick={() => void ipc.harnessAllowRemove(r.workspaceId, r.rule).then((all) => { setRules(all.filter((x) => x.workspaceId === workspace.id)); say("撤掉了，下次又会问"); }).catch(say)}>REMOVE</Btn>
                </Row>
              ))}
            </div>
          ) : (
            <p className="hint">{rules ? "还没有。" : "在读…"}</p>
          )}

          <p className="lbl">ARCHIVE</p>
          <div className="set-inline">
            <Btn icon="archive" disabled={!tasks.length} onClick={() => { if (!archiveArmed) { setArchiveArmed(true); return; } setArchiveArmed(false); onArchiveAll(workspace.id); }}>{archiveArmed ? "CONFIRM" : "ARCHIVE ALL TASKS"}</Btn>
            {archiveArmed ? <span className="hint">再点一次：归档这 {tasks.length} 个任务</span> : <Info text="左栏里这个项目的任务全收进归档（在跑的除外）：记录都在，设置 → ARCHIVE 里看、拿回来。项目还留在左栏。" />}
          </div>

          <p className="lbl">REMOVE</p>
          <div className="set-inline">
            <Btn v="danger" icon="trash" onClick={() => { if (!armed) { setArmed(true); return; } onRemove(workspace.id); }}>{armed ? "CONFIRM" : "REMOVE FROM LIST"}</Btn>
            {armed ? <span className="hint">再点一次就移出</span> : <Info text="只是从左栏拿掉：文件夹、任务记录、快照都不动，重新打开这个文件夹就回来。" />}
          </div>
        </div>
      </div>
    </section>
  );
};

// ---------- MCP（v0.10 A2） ----------

const HINT_WORD: Record<ToolHint, string> = { read: "只读", write: "会改", destructive: "会删改" };

/** 这个项目的 Peck Loop 会起哪些 MCP：项目 .mcp.json 里的和设置里的放一起，能对这个项目一个个关；TEST 记下工具标注。 */
const ProjectMcp: React.FC<{
  workspaceId: string;
  settings: ProjectSettings;
  missing: boolean;
  /** 上一下还在存。 */
  saving?: boolean;
  save: (patch: { settings?: ProjectSettings }, message: string) => Promise<void>;
  say: (m: unknown) => void;
}> = ({ workspaceId, settings, missing, saving, save, say }) => {
  const [view, setView] = useState<ProjectMcpView | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const off = settings.mcpOff ?? [];
  const load = () => ipc.projectMcpList(workspaceId).then(setView).catch(say);
  useEffect(() => { if (!missing) void load(); }, [workspaceId, missing, off.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (name: string, on: boolean) =>
    save({ settings: { ...settings, mcpOff: on ? off.filter((n) => n !== name) : [...off, name] } }, on ? `${name} 在这个项目里开了` : `这个项目里不起 ${name} 了`);
  const test = async (name: string) => {
    setTesting(name);
    try { const tools = await ipc.projectMcpTest(workspaceId, name); await load(); say(`${name} 起来了：${tools.length} 个工具，标注记下了`); } catch (e) { say(e); } finally { setTesting(null); }
  };
  return (
    <>
      <p className="lbl">MCP<Info text="Peck Loop 在这个项目里起的 MCP 服务器：项目根 .mcp.json 里的（Claude Code 的写法，一份四家都用）和设置 → MCP 里的，同名的以项目的为准。项目里的是项目里的程序，第一次起之前问你一次，.mcp.json 改了再问。TEST 起一次、记下每个工具是只读还是会改：Grok、Antigravity 问要不要调这些工具时，Peck 照这个替它们批只读的。CLI 引擎自己读 .mcp.json，这里关掉只管 Peck Loop。" /></p>
      {view?.error && <p className="hint is-bad">{view.error}</p>}
      <div className="set-rows">
        {view?.rows.map((r) => (
          <div key={`${r.source}-${r.name}`} className="set-block">
            <Row name={<span className="t-code">{r.name}</span>} off={!r.on}
              desc={<>{r.source === "project" ? ".mcp.json" : "设置里的"}{r.overridden ? " · 被 .mcp.json 里同名的盖过" : r.offInSettings ? " · 设置里关着" : ""}{r.allowed ? " · 总是允许过" : ""} · <span className="t-code set-cmd" title={[r.command, ...r.args].join(" ")}>{[r.command.split("/").pop(), ...r.args].join(" ")}</span></>}>
              {!r.overridden && !r.offInSettings && <Switch label={`${r.name} 在这个项目里开着`} on={r.on} disabled={missing || saving} onChange={(on) => void toggle(r.name, on)} />}
              {!r.overridden && <Btn sm disabled={missing || testing !== null} onClick={() => void test(r.name)}>{testing === r.name ? "在起…" : "TEST"}</Btn>}
            </Row>
            {r.tools.length > 0 && (
              <div className="chips">
                {r.tools.map(([t, h]) => <span key={t} className="chip-quiet" title={HINT_WORD[h]}>{h === "read" ? <Icon name="lock" size={12} /> : null}{t}{h === "destructive" ? " · 会删改" : ""}</span>)}
              </div>
            )}
          </div>
        ))}
      </div>
      {view && view.rows.length === 0 && <p className="hint">没有。设置 → MCP 里加，或者在项目根写 .mcp.json。</p>}
      {!view && !missing && <p className="hint">在读…</p>}
      {view?.skipped.map(([n, why]) => <p key={n} className="hint">{n}：{why}</p>)}
    </>
  );
};

// ---------- Skills 和命令文件（v0.10 C2） ----------

const SKILL_WHERE: Record<string, string> = { project: ".claude/skills", "project-skills": "skills/", user: "Peck/skills（全局）", peck: "Peck 自带" };

/** 这个项目读到的 Skills（能一个个关）和斜杠命令文件。 */
const ProjectSkills: React.FC<{
  workspaceId: string;
  settings: ProjectSettings;
  missing: boolean;
  /** 上一下还在存。 */
  saving?: boolean;
  save: (patch: { settings?: ProjectSettings }, message: string) => Promise<void>;
  say: (m: unknown) => void;
}> = ({ workspaceId, settings, missing, saving, save, say }) => {
  const [list, setList] = useState<SlashList | null>(null);
  const [rows, setRows] = useState<SkillRow[] | null>(null);
  // 展开了哪个 Skill 的旧版（v0.10 C3）。
  const [history, setHistory] = useState<{ name: string; versions: SkillVersion[] } | null>(null);
  const off = settings.skillsOff ?? [];
  useEffect(() => {
    if (missing) return;
    let live = true;
    ipc.slashList(workspaceId).then((l) => { if (live) setList(l); }).catch(say);
    ipc.skillOverview(workspaceId).then((r) => { if (live) setRows(r); }).catch(say);
    return () => { live = false; };
  }, [workspaceId, missing, off.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (name: string, on: boolean) =>
    save({ settings: { ...settings, skillsOff: on ? off.filter((n) => n !== name) : [...off, name] } }, on ? `Skill ${name} 开了` : `这个项目里不用 Skill ${name} 了`);
  const showHistory = (name: string) => {
    if (history?.name === name) { setHistory(null); return; }
    ipc.skillVersions(workspaceId, name).then((versions) => setHistory({ name, versions })).catch(say);
  };
  const restore = (name: string, v: SkillVersion) =>
    ipc.skillRestore(workspaceId, name, v.id).then((next) => { setRows(next); setHistory(null); say(`Skill ${name} 退回到 ${ago(v.at)} 那一版了（现在的也存了一份，能再退回来）`); }).catch(say);
  const used = (k: SkillRow) => [
    k.loads > 0 ? `用了 ${k.loads} 次${k.lastUsed ? `，最近 ${ago(k.lastUsed)}` : ""}` : "还没用过",
    k.stale ? "30 天没用了" : null,
  ].filter(Boolean).join(" · ");
  const shown: SkillRow[] = rows ?? (list?.skills.map((k) => ({ ...k, loads: 0, stale: false, versions: 0 })) ?? []);
  return (
    <>
      <p className="lbl">SKILLS<Info text="项目的 .claude/skills/、和 AGENTS.md 同级的 skills/ 里的 SKILL.md，Peck 的全局 Skill（数据目录的 skills/，只有 Peck 读），加上 Peck 自带的两个（快照回退、定时任务：教模型用 Peck 自己的工具，只给 Peck Loop）。Peck Loop 把名字和一句描述放进系统提示，要用时读全文（过程里写「读 Skill …」）。CLI 自己读项目里的；这里关掉只管 Peck Loop。模型提议、你批了的 Skill 也写在这几处；替换前的旧版 Peck 存着，点「旧版」能退回。用了几次按 Peck Loop 和 Claude Code 读它的次数算（项目的只算这个项目，全局的算所有项目）。" /></p>
      {list || rows ? (shown.length > 0 ? (
        <div className="set-rows">
          {shown.map((k) => (
            <React.Fragment key={k.name}>
              <Row name={<span className="t-code">{k.name}</span>} off={k.off} desc={<>{SKILL_WHERE[k.scope] ?? k.scope} · {k.description}{rows && <><br /><span className={cn(k.stale && "is-warn")}>{used(k)}</span></>}</>}>
                {k.versions > 0 && <Btn sm v="ghost" aria-expanded={history?.name === k.name} onClick={() => showHistory(k.name)}>旧版 {k.versions}</Btn>}
                <Switch label={`Skill ${k.name} 开着`} on={!k.off} disabled={missing || saving} onChange={(on) => void toggle(k.name, on)} />
              </Row>
              {history?.name === k.name && (
                <div className="skill-history">
                  {history.versions.length === 0 ? <p className="hint">没有存着的旧版。</p> : history.versions.map((v) => (
                    <Row key={v.id} name={<span className="t-code">{ago(v.at)}</span>} desc={<>{v.description || "（没有描述）"} · {v.lines} 行</>}>
                      <Btn sm disabled={missing} onClick={() => void restore(k.name, v)}>RESTORE</Btn>
                    </Row>
                  ))}
                </div>
              )}
            </React.Fragment>
          ))}
        </div>
      ) : <p className="hint">没有。</p>) : (
        <p className="hint">{missing ? "文件夹不在了。" : "在读…"}</p>
      )}
      <p className="lbl">COMMANDS<Info text="输入框打 / 列出来：Peck 自己的几个（/plan、/review、/undo…），加上 Peck 命令库（数据目录的 commands/，所有项目共用）里的 .md。选了以后把内容展开进这句话发出去，哪家引擎都能用（$ARGUMENTS 换成后面写的）。" /></p>
      {list && list.commands.length > 0 ? (
        <div className="chips">
          {list.commands.map((c) => <span key={c.name} className="chip-quiet" title={c.description}>/{c.name}</span>)}
        </div>
      ) : (
        <p className="hint">{list ? "命令库里还没有命令。" : "在读…"}</p>
      )}
    </>
  );
};
