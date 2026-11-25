import React, { useEffect, useMemo, useRef, useState } from "react";
import { ipc, type PromptItem } from "@/lib/ipc";
import { PROMPT_TITLE_MAX, promptText, tidyPrompt } from "@/lib/prompts";
import { ago, cn, copyText } from "@/lib/utils";
import { Btn, Field, Icon, ModHead, Modal, Twinkle } from "@/ui";
import { GrowText } from "@/shell/Memory";

/** 正文前两行（跳过空行），列表里一行一个、照原样（`{占位}` 不动）。 */
const previewLines = (body: string) => body.split("\n").map((l) => l.trim()).filter(Boolean).slice(0, 2);

/** 标题、正文都搜：空格分开的几个词都要出现（不分大小写）。 */
function matches(p: PromptItem, words: string[]): boolean {
  const hay = `${p.title}\n${p.body}`.toLowerCase();
  return words.every((w) => hay.includes(w));
}

/** 右栏 PROMPT（v1.2.0）：全局一份的提示词库。顶上搜索；一行一条（标题 + 正文前两行），
 *  点这一行复制正文（标题后一个绿勾），笔改、箭头放进中栏输入框；NEW、改都开小窗，删在小窗里点两下。 */
export const PromptPanel: React.FC<{
  /** 选了项目才有中栏输入框：没选时箭头变灰。 */
  canInsert: boolean;
  onToTask: (text: string) => void;
  /** 模块头右边的展开按钮。 */
  tools?: React.ReactNode;
  say: (message: unknown) => void;
}> = ({ canInsert, onToTask, tools, say }) => {
  const [list, setList] = useState<PromptItem[] | null>(null);
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const [editing, setEditing] = useState<PromptItem | "new" | null>(null);
  const timer = useRef(0);
  const rootRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let live = true;
    ipc.promptList().then((l) => { if (live) setList(l); }).catch((e) => { if (live) { setList([]); say(e); } });
    return () => { live = false; window.clearTimeout(timer.current); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const shown = useMemo(() => (list ?? []).filter((p) => matches(p, words)), [list, query]); // eslint-disable-line react-hooks/exhaustive-deps
  const copy = async (p: PromptItem) => {
    try {
      await copyText(promptText(p.body));
      setCopied(p.id);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(null), 1200);
    } catch (e) { say(e); }
  };
  const count = list && list.length > 0 ? (words.length ? `${shown.length}/${list.length}` : list.length) : undefined;
  return (
    <div className="pk-mod col-grow prompts" ref={rootRef}>
      <ModHead label="PROMPT" count={count}>
        <Btn v="ghost" sm icon="plus" onClick={() => setEditing("new")}>NEW</Btn>
        {tools}
      </ModHead>
      {list === null ? <p className="empty-line"><Twinkle /> 在读…</p> : list.length === 0 ? <p className="empty-line">还没有提示词。</p> : (
        <>
          <Field className="prompt-search" aria-label="搜提示词" placeholder="搜标题、正文" value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Escape" && query) { e.preventDefault(); e.stopPropagation(); setQuery(""); } }} />
          <div className="prompt-list scroll">
            <div role="list" aria-label="提示词">
              {shown.map((p) => (
                <div key={p.id} role="listitem" className={cn("prompt-row", copied === p.id && "is-copied")}>
                  <button type="button" className="prompt-main" aria-label={`复制「${p.title}」`} onClick={() => void copy(p)}>
                    <span className="prompt-title">
                      <b>{p.title}</b>
                      {copied === p.id && <Icon name="check" size={12} />}
                    </span>
                    {previewLines(p.body).map((line, i) => <span key={i} className="prompt-line">{line}</span>)}
                  </button>
                  <span className="prompt-acts">
                    <Btn v="ghost" sm square className="prompt-edit-btn" aria-label={`改「${p.title}」`} onClick={() => setEditing(p)}><Icon name="pen" size={12} /></Btn>
                    <Btn v="ghost" sm square className="prompt-to" aria-label={`把「${p.title}」放进输入框`} disabled={!canInsert} onClick={() => onToTask(promptText(p.body))}><Icon name="send" size={12} /></Btn>
                  </span>
                </div>
              ))}
            </div>
            {shown.length === 0 && <p className="empty-line">没有对得上的。</p>}
          </div>
        </>
      )}
      {editing && (
        <PromptModal item={editing === "new" ? null : editing} say={say} onClose={() => setEditing(null)}
          onSaved={(next, deleted) => {
            setList(next);
            setEditing(null);
            // 删掉以后原来的笔不在了：焦点放到搜索框（在小窗把焦点还回去之后）。
            if (deleted) window.setTimeout(() => rootRef.current?.querySelector<HTMLElement>(".prompt-search input")?.focus(), 0);
          }} />
      )}
    </div>
  );
};

/** 新加、改一条的小窗：标题一行、正文多行（⌘↵ 存）；改的时候多一个 DELETE（点两下）。 */
const PromptModal: React.FC<{
  item: PromptItem | null;
  onSaved: (next: PromptItem[], deleted: boolean) => void;
  onClose: () => void;
  say: (message: unknown) => void;
}> = ({ item, onSaved, onClose, say }) => {
  const [title, setTitle] = useState(item?.title ?? "");
  const [body, setBody] = useState(item?.body ?? "");
  const [busy, setBusy] = useState(false);
  const [armed, setArmed] = useState(false);
  const formRef = useRef<HTMLDivElement>(null);
  const bodyBox = () => formRef.current?.querySelector<HTMLTextAreaElement>("textarea") ?? null;
  // 小窗打开时先把焦点给了自己（Modal 的 effect）；这里在它后面跑：新加的先写标题，改的接着写正文。
  useEffect(() => {
    const box = bodyBox();
    if (item && box) { box.focus(); box.setSelectionRange(box.value.length, box.value.length); }
    else formRef.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  // 和后端一样规整过再比：只在末尾多了空格、空行不算改了（不然存一下就跳到最上）。
  const tidy = tidyPrompt(title, body);
  const changed = !item || tidy.title !== item.title || tidy.body !== item.body;
  const ready = !!tidy.title && !!tidy.body.trim() && changed && !busy;
  const run = async (fn: () => Promise<PromptItem[]>, deleted = false) => {
    setBusy(true);
    try { onSaved(await fn(), deleted); } catch (e) { say(e); setBusy(false); setArmed(false); }
  };
  const save = () => { if (ready) void run(() => (item ? ipc.promptUpdate(item.id, title, body) : ipc.promptAdd(title, body))); };
  return (
    <Modal label={item ? "EDIT PROMPT" : "NEW PROMPT"} onClose={onClose} className="prompt-modal">
      <div className="prompt-form" ref={formRef}>
        <Field aria-label="标题" placeholder="标题" value={title} maxLength={PROMPT_TITLE_MAX} disabled={busy}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); if (e.metaKey || e.ctrlKey) save(); else bodyBox()?.focus(); } }} />
        <GrowText className="prompt-body-edit" label="正文" placeholder="正文" value={body} disabled={busy} onChange={setBody} onSubmit={save} />
        <div className="memo-foot">
          <span className="memo-meta">{item ? `${ago(item.createdAt)} 建 · ${ago(item.updatedAt)} 改` : ""}</span>
          {item && (
            <Btn sm v="danger" disabled={busy} aria-label={armed ? "再点一次删掉这条提示词" : "删掉这条提示词"}
              onClick={() => (armed ? void run(() => ipc.promptDelete(item.id), true) : setArmed(true))}>{armed ? "CONFIRM" : "DELETE"}</Btn>
          )}
          <Btn sm v="primary" disabled={!ready} onClick={save}>SAVE</Btn>
        </div>
      </div>
    </Modal>
  );
};
