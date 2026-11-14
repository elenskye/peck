import React, { useEffect, useState } from "react";
import type { ModelEntry } from "@/lib/agentEvents";
import { ipc, type AllowRule, type MemoryNote, type VerifyState } from "@/lib/ipc";
import type { Permission, ProjectOverview } from "@/lib/types";
import { ago, PERMISSIONS, shortenHomePath } from "@/lib/utils";
import { Btn, Field, Info, ModHead, Row, Seg } from "@/ui";
import { WhoPicker, pickable } from "@/shell/ModelPicker";

const RULE_WORD: Record<string, string> = { "install-deps": "装依赖", "long-running": "起常驻进程" };

/** 项目设置（右键项目 → 项目设置）：名字、文件夹、在这个项目里默认谁来干和哪一档、验证命令、记忆、「总是允许」过的、移出列表。 */
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
  const [memories, setMemories] = useState<MemoryNote[] | null>(null);
  const [verify, setVerify] = useState<VerifyState | null>(null);
  const [verifyText, setVerifyText] = useState("");
  useEffect(() => setName(workspace.name), [workspace.name]);
  useEffect(() => {
    let live = true;
    ipc.memoryList(workspace.id).then((m) => { if (live) setMemories(m); }).catch(say);
    if (!missing) ipc.verifyGet(workspace.id).then((v) => { if (live) { setVerify(v); setVerifyText(v.text ?? v.guessed ?? ""); } }).catch(say);
    return () => { live = false; };
  }, [workspace.id, missing]); // eslint-disable-line react-hooks/exhaustive-deps
  const saveVerify = async () => {
    try { const v = await ipc.verifySave(workspace.id, verifyText); setVerify(v); setVerifyText(v.text ?? ""); say("验证命令存好了"); } catch (e) { say(e); }
  };
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
  return (
    <section className="col page" aria-label="项目设置">
      <div className="pk-mod col-grow page-mod">
        <ModHead label="PROJECT" note={workspace.name}><Btn v="ghost" sm icon="x" onClick={onClose}>CLOSE</Btn></ModHead>
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
              <Row name="文件夹" off={missing} desc={<><span className="t-code">{shortenHomePath(workspace.path)}</span>{missing ? " · 文件夹不在了" : ""}</>}>
                {missing ? <em className="pk-tag pk-tag--wait">MISSING</em> : <em className="pk-tag pk-tag--ok">OK</em>}
              </Row>
              <div className="set-inline">
                <Btn sm icon="folder" disabled={!native || missing} onClick={() => onReveal(workspace.id)}>SHOW IN FINDER</Btn>
                <Btn sm onClick={() => onRelocate(workspace.id)}>CHANGE FOLDER</Btn>
                <Info text="换文件夹：任务记录、快照、「总是允许」都跟过去。" />
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

          <p className="lbl">FOLDERS<Info text="项目即环境：主文件夹以外的文件夹，每个选只读（READ）或能改（WRITE）。Peck 循环的工具用绝对路径读写它们，命令的沙箱放开能改的那几个；能改的文件夹的改动也进 CHANGES、能撤销。CLI 引擎还只认主文件夹。" /></p>
          <div className="set-rows">
            <Row name={<span className="t-code">{shortenHomePath(workspace.path)}</span>} desc="主文件夹 · 能改">
              <em className="pk-tag pk-tag--ok">MAIN</em>
            </Row>
            {folders.map((f, i) => (
              <Row key={f.path} name={<span className="t-code">{shortenHomePath(f.path)}</span>} desc={f.writable ? "能改" : "只读"}>
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

          <p className="lbl">ENV<Info text="一行一个 NAME=value。Peck 循环的命令、验证命令、dev server、TERM 都带上（同名的盖过你终端里的）；值只存在 Peck 的数据库里，不进模型上下文（模型看得到名字）。" /></p>
          <textarea className="verify-text" aria-label="这个项目的环境变量" spellCheck={false} disabled={missing} value={envDraft}
            placeholder={"API_BASE=http://localhost:8080\nRUST_LOG=info"} rows={Math.min(12, Math.max(3, envDraft.split("\n").length + 1))} onChange={(e) => setEnvDraft(e.target.value)} />
          <div className="set-inline">
            <Btn sm disabled={missing || envDraft === envText} onClick={saveEnv}>SAVE</Btn>
            {envDraft !== envText && <Btn sm v="ghost" onClick={() => setEnvDraft(envText)}>RESET</Btn>}
          </div>

          <p className="lbl">VERIFY · .peck/verify.toml<Info text="模型改了文件，Peck 在后台替它跑这些；模型停下时没过，就把失败交回去修（最多 3 次）。[[check]] 里 name、run、when（改了哪些文件才跑）、timeout（秒）；[[server]] 是常驻的 dev server，在输入框旁的 DEV 里起停。项目自带的、改过的要 SAVE 确认过才用。" /></p>
          {!verify?.exists && verify?.guessed && <p className="hint">还没有：下面是 Peck 猜的，SAVE 就写进去。</p>}
          {verify?.error && <p className="hint is-bad">{verify.error}</p>}
          {verify?.exists && !verify.approved && !verify.error && <p className="hint is-bad">还没确认（项目自带的，或者改过了）：看一遍，SAVE 以后 Peck 才替模型跑、才起 dev server。</p>}
          <textarea className="verify-text" aria-label="verify.toml" spellCheck={false} disabled={missing} value={verifyText}
            placeholder={'[[check]]\nname = "check"\nrun = "pnpm check"\nwhen = ["**/*.ts"]'} rows={Math.min(18, Math.max(6, verifyText.split("\n").length + 1))} onChange={(e) => setVerifyText(e.target.value)} />
          <div className="set-inline">
            <Btn sm disabled={missing || !verifyText.trim() || (verifyText === (verify?.text ?? "") && !!verify?.approved)} onClick={() => void saveVerify()}>SAVE</Btn>
            {verify?.exists && verifyText !== (verify.text ?? "") && <Btn sm v="ghost" onClick={() => setVerifyText(verify.text ?? "")}>RESET</Btn>}
          </div>

          <p className="lbl">MEMORY<Info text="Peck 循环觉得值得记的，任务结束时弹一张「想记住这几条」，你批了才记在这里；每次发任务都带上。" /></p>
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
                <Row key={r.rule} name={RULE_WORD[r.rule] ?? r.rule} desc={`EDIT 档里不再问 · ${ago(r.createdAt)}`}>
                  <Btn sm v="danger" onClick={() => void ipc.harnessAllowRemove(r.workspaceId, r.rule).then((all) => { setRules(all.filter((x) => x.workspaceId === workspace.id)); say("撤掉了，下次又会问"); }).catch(say)}>REMOVE</Btn>
                </Row>
              ))}
            </div>
          ) : (
            <p className="hint">{rules ? "还没有。" : "在读…"}</p>
          )}

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
