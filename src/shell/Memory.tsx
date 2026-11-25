import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ipc, type MemoryGroup, type MemoryNote } from "@/lib/ipc";
import { ago, cn, shortenHomePath } from "@/lib/utils";
import { Btn, Modal, Row } from "@/ui";

/** 多行的框，跟着字变高（记忆的改字、记一条；提示词的正文）。Esc 交给 `onEscape`，不关小窗；⌘↵ 交给 `onSubmit`。 */
export const GrowText: React.FC<{ value: string; onChange: (v: string) => void; onSubmit: () => void; onEscape?: () => void; placeholder?: string; label: string; disabled?: boolean; autoFocus?: boolean; className?: string }> = ({ value, onChange, onSubmit, onEscape, placeholder, label, disabled, autoFocus, className }) => {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);
  useEffect(() => {
    const el = ref.current;
    if (autoFocus && el) { el.focus(); el.setSelectionRange(el.value.length, el.value.length); }
  }, [autoFocus]);
  return (
    <textarea ref={ref} className={cn("memo-edit", className)} aria-label={label} placeholder={placeholder} rows={2} spellCheck={false} disabled={disabled} value={value}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.nativeEvent.isComposing) return;
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); onSubmit(); }
        if (e.key === "Escape" && onEscape) { e.stopPropagation(); e.preventDefault(); onEscape(); }
      }} />
  );
};

/** 一个项目的记忆小窗（v1.1.6）：一条一块，字照原样显示；EDIT 就地变成多行的框（⌘↵ 存、Esc 取消），DELETE 点两下；
 *  最下面一个多行的框自己记一条。设置 → MEMORY 和项目设置 → MEMORY 都开这一个。 */
export const MemoryModal: React.FC<{
  workspaceId: string;
  name: string;
  path: string;
  notes: MemoryNote[];
  onChange: (next: MemoryNote[]) => void;
  onClose: () => void;
  say: (message: unknown) => void;
}> = ({ workspaceId, name, path, notes, onChange, onClose, say }) => {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [adding, setAdding] = useState("");
  const [armed, setArmed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const act = async (run: () => Promise<MemoryNote[]>, message: string) => {
    setBusy(true);
    try { onChange(await run()); say(message); return true; } catch (e) { say(e); return false; } finally { setBusy(false); setArmed(null); }
  };
  const saveEdit = async (m: MemoryNote) => {
    if (!draft.trim() || draft.trim() === m.text) { setEditing(null); return; }
    if (await act(() => ipc.memoryUpdate(workspaceId, m.id, draft.trim()), "改好了，下次发任务带上新的")) setEditing(null);
  };
  const add = async () => {
    if (!adding.trim()) return;
    if (await act(() => ipc.memoryAdd(workspaceId, adding.trim()), "记下了，以后每次发任务都带上")) setAdding("");
  };
  return (
    <Modal label="MEMORY" onClose={onClose}>
      <Row name={name} desc={shortenHomePath(path)} />
      {notes.length > 0 ? (
        <div className="memo-list">
          {notes.map((m) => (
            <div key={m.id} className="memo">
              {editing === m.id ? (
                <GrowText label="改这条记忆" value={draft} disabled={busy} autoFocus onChange={setDraft} onSubmit={() => void saveEdit(m)} onEscape={() => setEditing(null)} />
              ) : (
                <p className="memo-text">{m.text}</p>
              )}
              <div className="memo-foot">
                <span className="memo-meta">{m.sourceRun ? `任务 ${m.sourceRun.replace(/^run-/, "")} 提的` : "手工记的"} · {ago(m.createdAt)}</span>
                {editing === m.id ? (
                  <>
                    <Btn sm v="ghost" disabled={busy} onClick={() => setEditing(null)}>CANCEL</Btn>
                    <Btn sm v="primary" disabled={busy || !draft.trim() || draft.trim() === m.text} onClick={() => void saveEdit(m)}>SAVE</Btn>
                  </>
                ) : (
                  <>
                    <Btn sm v="danger" disabled={busy || editing !== null} aria-label={armed === m.id ? "再点一次删掉这条记忆" : "删掉这条记忆"}
                      onClick={() => (armed === m.id ? void act(() => ipc.memoryDelete(workspaceId, m.id), "删掉了，以后不再带上") : setArmed(m.id))}>{armed === m.id ? "CONFIRM" : "DELETE"}</Btn>
                    <Btn sm disabled={busy || editing !== null} onClick={() => { setEditing(m.id); setDraft(m.text); setArmed(null); }}>EDIT</Btn>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <p className="hint memo-add">还没有。</p>
      )}
      <div className="memo-add">
        <GrowText label="自己记一条" placeholder="记一条：以后每次发任务都带上" value={adding} disabled={busy} onChange={setAdding} onSubmit={() => void add()} />
        <div className="memo-foot">
          <span className="memo-meta" />
          <Btn sm disabled={busy || !adding.trim()} onClick={() => void add()}>ADD</Btn>
        </div>
      </div>
    </Modal>
  );
};

/** 几条（项目那一行下面写的）。 */
export const memoryCount = (n: number) => (n ? `${n} 条` : "还没有");

/** 设置 → MEMORY（v1.1.4；v1.1.6 改成一个项目一行 + EDIT 开小窗）：移出了列表的项目有记忆才列，排在最后。 */
export const MemorySettings: React.FC<{ say: (message: unknown) => void }> = ({ say }) => {
  const [groups, setGroups] = useState<MemoryGroup[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    ipc.memoryOverview().then((g) => { if (live) setGroups(g); }).catch(say);
    return () => { live = false; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  if (!groups) return <p className="hint">在读…</p>;
  if (groups.length === 0) return <p className="hint">还没有项目。</p>;
  const replace = (workspaceId: string, notes: MemoryNote[]) => setGroups((all) => all && all.map((g) => (g.workspaceId === workspaceId ? { ...g, notes } : g)));
  const shown = groups.find((g) => g.workspaceId === open);
  return (
    <>
      <div className="set-rows">
        {groups.map((g) => (
          <Row key={g.workspaceId} name={g.name} off={g.notes.length === 0} desc={`${memoryCount(g.notes.length)} · ${shortenHomePath(g.path)}${g.hidden ? " · 不在列表里" : ""}`}>
            <Btn sm onClick={() => setOpen(g.workspaceId)}>EDIT</Btn>
          </Row>
        ))}
      </div>
      {shown && (
        <MemoryModal workspaceId={shown.workspaceId} name={shown.name} path={shown.path} notes={shown.notes}
          onChange={(notes) => replace(shown.workspaceId, notes)} onClose={() => setOpen(null)} say={say} />
      )}
    </>
  );
};
