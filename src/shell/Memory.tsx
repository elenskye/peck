import React, { useEffect, useState } from "react";
import { ipc, type MemoryGroup, type MemoryNote } from "@/lib/ipc";
import { ago, shortenHomePath } from "@/lib/utils";
import { Btn, Field, Row } from "@/ui";

/** 一个项目的记忆（v1.1.4）：每条能改字、删；最下面一行自己记一条。项目设置 → MEMORY 和设置 → MEMORY 共用。 */
export const MemoryList: React.FC<{
  workspaceId: string;
  notes: MemoryNote[];
  off?: boolean;
  onChange: (next: MemoryNote[]) => void;
  say: (message: unknown) => void;
}> = ({ workspaceId, notes, off, onChange, say }) => {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [adding, setAdding] = useState("");
  const [busy, setBusy] = useState(false);
  const act = async (run: () => Promise<MemoryNote[]>, message: string) => {
    setBusy(true);
    try { onChange(await run()); say(message); return true; } catch (e) { say(e); return false; } finally { setBusy(false); }
  };
  const saveEdit = async (id: string) => {
    if (await act(() => ipc.memoryUpdate(workspaceId, id, draft), "改好了，下次发任务带上新的")) setEditing(null);
  };
  const add = async () => {
    if (await act(() => ipc.memoryAdd(workspaceId, adding), "记下了，以后每次发任务都带上")) setAdding("");
  };
  return (
    <div className="set-rows">
      {notes.map((m) => m.id === editing ? (
        <div key={m.id} className="set-row memory-edit">
          <Field aria-label="改这条记忆" value={draft} off={busy} autoFocus onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) void saveEdit(m.id); if (e.key === "Escape") setEditing(null); }} />
          <div className="set-ctl">
            <Btn sm disabled={busy || !draft.trim() || draft.trim() === m.text} onClick={() => void saveEdit(m.id)}>SAVE</Btn>
            <Btn sm v="ghost" disabled={busy} onClick={() => setEditing(null)}>CANCEL</Btn>
          </div>
        </div>
      ) : (
        <Row key={m.id} name={<span className="memory-text">{m.text}</span>} desc={`${m.sourceRun ? `任务 ${m.sourceRun.replace(/^run-/, "")} 提的` : "手工记的"} · ${ago(m.createdAt)}`}>
          <Btn sm v="ghost" disabled={off || busy} onClick={() => { setEditing(m.id); setDraft(m.text); }}>EDIT</Btn>
          <Btn sm v="danger" disabled={off || busy} onClick={() => void act(() => ipc.memoryDelete(workspaceId, m.id), "删掉了，以后不再带上")}>DELETE</Btn>
        </Row>
      ))}
      <div className="set-row memory-edit">
        <Field aria-label="自己记一条" placeholder="记一条：以后每次发任务都带上" value={adding} off={off || busy} onChange={(e) => setAdding(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing && adding.trim()) void add(); }} />
        <div className="set-ctl"><Btn sm disabled={off || busy || !adding.trim()} onClick={() => void add()}>ADD</Btn></div>
      </div>
    </div>
  );
};

/** 设置 → MEMORY（v1.1.4）：全部项目的记忆，一个项目一块。移出了列表的项目有记忆才列，排在最后。 */
export const MemorySettings: React.FC<{ say: (message: unknown) => void }> = ({ say }) => {
  const [groups, setGroups] = useState<MemoryGroup[] | null>(null);
  useEffect(() => {
    let live = true;
    ipc.memoryOverview().then((g) => { if (live) setGroups(g); }).catch(say);
    return () => { live = false; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  if (!groups) return <p className="hint">在读…</p>;
  if (groups.length === 0) return <p className="hint">还没有项目。</p>;
  const replace = (workspaceId: string, notes: MemoryNote[]) => setGroups((all) => all && all.map((g) => (g.workspaceId === workspaceId ? { ...g, notes } : g)));
  return (
    <>
      {groups.map((g) => (
        <div key={g.workspaceId} className="memory-group">
          <p className="lbl">{g.name}<span className="memory-path t-code">{shortenHomePath(g.path)}{g.hidden ? " · 不在列表里" : ""}</span></p>
          <MemoryList workspaceId={g.workspaceId} notes={g.notes} onChange={(notes) => replace(g.workspaceId, notes)} say={say} />
        </div>
      ))}
    </>
  );
};
