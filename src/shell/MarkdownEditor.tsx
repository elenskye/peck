// 输入框：CodeMirror 6 + Markdown，边打边渲染（标题加粗变大、`代码` 等宽、**粗体**、> 引用、列表符号变淡）。
// ↵ 发送，⇧↵ 或 ⌘↵ 换行（列表里换行接着列表）；列表里 Tab 缩进一级、⇧Tab 退一级（v1.1.12）；
// 插进来的提示词里的 `{占位}` 选中第一个、Tab 跳下一个（v1.2.0）；
// 中文输入法照常（组字时的回车是选字，不发）；不拼写检查、不自动改字。样子照 Peck 的输入格。
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { Compartment, EditorState, Prec, StateEffect, StateField, type ChangeSpec } from "@codemirror/state";
import { Decoration, EditorView, keymap, placeholder as placeholderExt, type DecorationSet } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, insertNewlineAndIndent } from "@codemirror/commands";
import { insertNewlineContinueMarkup, markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { expandPrompt } from "@/lib/prompts";

const peckMarkdown = HighlightStyle.define([
  { tag: tags.heading1, fontWeight: "700", fontSize: "1.18em" },
  { tag: tags.heading2, fontWeight: "700", fontSize: "1.1em" },
  { tag: [tags.heading3, tags.heading4, tags.heading5, tags.heading6], fontWeight: "700" },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.monospace, fontFamily: "var(--font-code)", color: "var(--color-cyan-text)" },
  { tag: tags.link, color: "var(--color-cyan-text)", textDecoration: "underline" },
  { tag: tags.url, color: "var(--color-text-3)" },
  { tag: tags.quote, color: "var(--color-text-2)" },
  { tag: [tags.processingInstruction, tags.contentSeparator, tags.list], color: "var(--color-text-3)" },
]);

// ---------- 列表缩进（v1.1.12） ----------

/** 列表项那一行（`- `、`* `、`+ `、`1. `、`1) `）：标记前面几个空格、正文从第几格起、有序列表的编号（和它几位）。 */
const LIST_ITEM = /^( *)([-*+]|(\d{1,9})[.)])( +|$)/;
interface ListItem { indent: number; content: number; num: number | null; digits: number }
function listItem(text: string): ListItem | null {
  const m = LIST_ITEM.exec(text);
  if (!m) return null;
  const gap = m[4].length;
  return { indent: m[1].length, content: m[1].length + m[2].length + (gap >= 1 && gap <= 4 ? gap : 1), num: m[3] ? Number(m[3]) : null, digits: m[3]?.length ?? 0 };
}

/** 有序列表项换了一级以后的编号：那一级上一项是有序的就接着它，是这一级的第一项就从 1 起。无序的不动。位置算改之前的。 */
function renumber(line: { from: number }, item: ListItem, prev: ListItem | null): ChangeSpec[] {
  if (item.num == null) return [];
  const next = prev?.num != null ? prev.num + 1 : prev ? null : 1;
  if (next == null || next === item.num) return [];
  const from = line.from + item.indent;
  return [{ from, to: from + item.digits, insert: String(next) }];
}

/** 选区碰到的行（选区停在下一行行首时不算那一行），按顺序、不重复。 */
function touchedLines(state: EditorState) {
  const seen = new Map<number, ReturnType<EditorState["doc"]["line"]>>();
  for (const r of state.selection.ranges) {
    const first = state.doc.lineAt(r.from).number;
    let last = state.doc.lineAt(r.to).number;
    if (r.to > r.from && last > first && state.doc.lineAt(r.to).from === r.to) last--;
    for (let n = first; n <= last; n++) if (!seen.has(n)) seen.set(n, state.doc.line(n));
  }
  return [...seen.values()].sort((a, b) => a.number - b.number);
}

/** 第一个列表项及其上面的列表项（空行、续行跳过）。 */
function listContext(state: EditorState) {
  const lines = touchedLines(state);
  for (const line of lines) {
    const item = listItem(line.text);
    if (!item) continue;
    const above = function* () {
      for (let n = line.number - 1; n >= 1; n--) {
        const it = listItem(state.doc.line(n).text);
        if (it) yield it;
      }
    };
    return { lines, line, item, above };
  }
  return null;
}

/** Tab：选到的行里有列表项就整体缩进一级——缩到同一级上一项的正文那一格（成为它的子项，`1. ` 下面是 3 格、`- ` 下面 2 格），
 *  没有上一项就按自己的标记宽；有序的顺手改编号。不在列表里交给别人（跟以前一样）。 */
function indentList(view: EditorView): boolean {
  const { state } = view;
  const ctx = listContext(state);
  if (!ctx) return false;
  let target = ctx.item.content;
  let parent = false;
  const deeper: ListItem[] = [];
  for (const it of ctx.above()) {
    if (it.indent === ctx.item.indent) { target = it.content; parent = true; break; }
    if (it.indent < ctx.item.indent) break;
    deeper.push(it);
  }
  // 新的那一级里的上一项：父项下面已经有的子项里离得最近、缩进一样的。
  let prev: ListItem | null = null;
  for (const it of deeper) {
    if (it.indent === target) { prev = it; break; }
    if (it.indent < target) break;
  }
  const pad = " ".repeat(Math.max(1, target - ctx.item.indent));
  const changes: ChangeSpec[] = ctx.lines.filter((l) => l.text.trim() || l.number === ctx.line.number).map((l) => ({ from: l.from, insert: pad }));
  if (parent) changes.push(...renumber(ctx.line, ctx.item, prev));
  const set = state.changes(changes);
  view.dispatch({ changes: set, selection: state.selection.map(set, 1), scrollIntoView: true, userEvent: "input.indent" });
  return true;
}

/** ⇧Tab：退到父项那一级（上面缩进更少的那一项，有序的接着它的编号），没有就退到最左；已经在最外层不动。 */
function outdentList(view: EditorView): boolean {
  const { state } = view;
  const ctx = listContext(state);
  if (!ctx) return false;
  if (ctx.item.indent === 0) return true;
  let target = 0;
  let parent: ListItem | null = null;
  for (const it of ctx.above()) if (it.indent < ctx.item.indent) { target = it.indent; parent = it; break; }
  const by = ctx.item.indent - target;
  const changes: ChangeSpec[] = [];
  for (const l of ctx.lines) {
    const cut = Math.min(by, /^ */.exec(l.text)![0].length);
    if (cut) changes.push({ from: l.from, to: l.from + cut });
  }
  if (parent) changes.push(...renumber(ctx.line, ctx.item, parent));
  view.dispatch({ changes, scrollIntoView: true, userEvent: "delete.dedent" });
  return true;
}

// ---------- 占位（v1.2.0） ----------

interface Span { from: number; to: number }
/** 插一段提示词时给的一组占位（换掉旧的）；空数组 = 退出。位置按这次改完以后的算。 */
const setSlots = StateEffect.define<Span[]>();
/** Tab 跳到第几个。 */
const toSlot = StateEffect.define<number>();
const openMark = Decoration.mark({ class: "cm-slot" });

/** 一组占位：`fields` 是每个占位现在的范围（填了字跟着长，两头含；自己按改动映射，不用标记装饰——空了的标记 CodeMirror
 *  下一次改动就丢掉，清空一个占位再在旁边删字，后面的就跳不了了），`open` 是还没动过的（标琥珀），`active` 是光标在第几个。 */
interface Slots { fields: Span[]; open: DecorationSet; active: number }
const NO_SLOTS: Slots = { fields: [], open: Decoration.none, active: -1 };

/** 插进来的提示词里的占位，照 snippet 的做法：选中、打字替换、Tab 跳下一个。光标走出所有占位（点别处、方向键）、Esc、
 *  一次改动吞掉整个占位还多（撤销插入、全选重打、发出去清空）就整组退出，Tab 回到列表缩进那套。 */
const slotField = StateField.define<Slots>({
  create: () => NO_SLOTS,
  update(s, tr) {
    let set: Span[] | null = null;
    for (const e of tr.effects) if (e.is(setSlots)) set = e.value;
    if (!s.fields.length && !set) return s;
    let { fields, open, active } = s;
    if (set) {
      if (!set.length) return NO_SLOTS;
      fields = set.map((r) => ({ from: r.from, to: r.to }));
      open = Decoration.set(set.map((r) => openMark.range(r.from, r.to)), true);
      active = 0;
    } else {
      if (tr.docChanged) {
        const hit: Span[] = [];
        tr.changes.iterChangedRanges((from, to) => hit.push({ from, to }));
        // 清空了的占位（长度 0）旁边删字不算吞掉它；整个输入框清空（发出去了）一定退出。
        if (tr.state.doc.length === 0) return NO_SLOTS;
        if (fields.some((f) => f.to > f.from && hit.some((h) => h.from <= f.from && h.to >= f.to && h.to - h.from > f.to - f.from))) return NO_SLOTS;
        // 动过的不再标：删的一段和它重叠，或者插在它中间（紧挨着两头插不算）。
        open = open.update({ filter: (from, to) => !hit.some((h) => h.from < to && h.to > from) }).map(tr.changes);
        fields = fields.map((f) => ({ from: tr.changes.mapPos(f.from, -1), to: tr.changes.mapPos(f.to, 1) }));
      }
      for (const e of tr.effects) if (e.is(toSlot)) active = e.value;
    }
    if (tr.docChanged || tr.selection) {
      const list = fields;
      const { from, to } = tr.state.selection.main;
      const inside = (f: Span | undefined) => !!f && f.from <= from && to <= f.to;
      if (!inside(list[active])) {
        active = list.findIndex(inside);
        if (active < 0) return NO_SLOTS;
      }
    }
    return { fields, open, active };
  },
  provide: (f) => EditorView.decorations.from(f, (s) => s.open),
});

/** Tab：在占位里、后面还有就选中下一个；不然交给列表缩进、再交给浏览器（跟以前一样）。不绕回前面。 */
function nextSlot(view: EditorView): boolean {
  if (view.compositionStarted) return false;
  const s = view.state.field(slotField);
  if (s.active < 0) return false;
  const list = s.fields;
  const next = list[s.active + 1];
  if (!next) {
    // 最后一个占位里的 Tab：退出占位、光标放到它后面，不交给列表缩进（不然这一项会被缩成上一项的子项）。
    const end = list[s.active]?.to ?? view.state.selection.main.head;
    view.dispatch({ selection: { anchor: end }, effects: setSlots.of([]), userEvent: "select" });
    return true;
  }
  view.dispatch({ selection: { anchor: next.from, head: next.to }, effects: toSlot.of(s.active + 1), scrollIntoView: true, userEvent: "select" });
  return true;
}

/** Esc：退出这组占位（没填的留着当普通字）。 */
function leaveSlots(view: EditorView): boolean {
  if (!view.state.field(slotField).fields.length) return false;
  view.dispatch({ effects: setSlots.of([]) });
  return true;
}

const theme = EditorView.theme({
  // 最高多高外面定（`--md-max`）：中栏的输入框放到半个窗口（v1.1.9，学 Claude Code Desktop），ONE SHOT 的矮一点。
  "&": { backgroundColor: "transparent", color: "var(--color-text)", fontSize: "inherit", maxHeight: "var(--md-max, 132px)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "inherit", lineHeight: "20px", overflow: "auto" },
  ".cm-content": { padding: "5px 0", caretColor: "var(--color-cyan)" },
  ".cm-line": { padding: "0" },
  ".cm-placeholder": { color: "var(--color-text-3)" },
  ".cm-cursor": { borderLeftColor: "var(--color-cyan)", borderLeftWidth: "2px" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--color-cyan-soft)" },
  // 没填的占位：琥珀（等你）字加一道点线；选中时照常是青色底。退出占位就不标了。
  ".cm-slot": { color: "var(--color-warn)", borderBottom: "1px dotted var(--color-warn)" },
});

/** 光标在哪（连同那一稿，外面对得上才用）；`composing`：输入法正在组字。 */
export interface Caret { text: string; from: number; to: number; composing: boolean }

/** 外面能对输入框做的事。 */
export interface EditorHandle {
  /** 把 [from, to) 换成提示词正文（规则见 `expandPrompt`）：占位标出来、选中第一个，没有占位就把光标放在后面。
   *  给了 `expect` 就先核对这一段现在是不是它（外面按上一次渲染算的位置），对不上不动、回 false。 */
  insertPrompt: (from: number, to: number, body: string, expect?: string) => boolean;
}

interface Props {
  value: string;
  onChange: (text: string) => void;
  onSubmit: () => void;
  /** ↑ ↓ Esc 先问外面要不要（比如 @ 弹层开着）；回 true 就不交给编辑器。 */
  onKey?: (key: "up" | "down" | "escape") => boolean;
  /** 光标、选区、组字变了（`/` 弹层按光标算）。 */
  onCaret?: (caret: Caret) => void;
  placeholder: string;
  disabled?: boolean;
  ariaLabel: string;
}

export const MarkdownEditor = forwardRef<EditorHandle, Props>(({ value, onChange, onSubmit, onKey, onCaret, placeholder, disabled, ariaLabel }, ref) => {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const editable = useRef(new Compartment());
  const hint = useRef(new Compartment());
  const latest = useRef({ onChange, onSubmit, onKey, onCaret });
  latest.current = { onChange, onSubmit, onKey, onCaret };

  useImperativeHandle(ref, () => ({
    insertPrompt(from, to, body, expect) {
      const v = view.current;
      if (!v) return false;
      const len = v.state.doc.length;
      const a = Math.max(0, Math.min(from, len));
      let b = Math.max(a, Math.min(to, len));
      if (expect != null) {
        if (v.state.doc.sliceString(a, b) !== expect) return false;
        // 外面按上一次渲染算的位置：这一拍里又多打了几个字（`/re` → `/rev`）就把整个词一起换掉，不留尾巴。
        const rest = v.state.doc.sliceString(b, Math.min(len, b + 200));
        b += rest.match(/^\S*/)?.[0].length ?? 0;
      }
      const { text, slots } = expandPrompt(body);
      const first = slots[0];
      v.dispatch({
        changes: { from: a, to: b, insert: text },
        selection: first ? { anchor: a + first.from, head: a + first.to } : { anchor: a + text.length },
        effects: setSlots.of(slots.map((s) => ({ from: a + s.from, to: a + s.to }))),
        scrollIntoView: true,
        userEvent: "input.complete",
      });
      v.focus();
      return true;
    },
  }), []);

  useEffect(() => {
    if (!host.current) return;
    const report = (v: EditorView, composing = v.compositionStarted) => {
      const { from, to } = v.state.selection.main;
      latest.current.onCaret?.({ text: v.state.doc.toString(), from, to, composing });
    };
    let ended = 0;
    const v = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          history(),
          // 回车自己接：比 Markdown 和默认按键都先。输入法组字时（isComposing、keyCode 229）交给输入法。
          Prec.highest(EditorView.domEventHandlers({
            keydown: (e, v) => {
              const nav = e.key === "ArrowUp" ? "up" : e.key === "ArrowDown" ? "down" : e.key === "Escape" ? "escape" : null;
              if (nav && !e.isComposing && e.keyCode !== 229 && !v.compositionStarted && latest.current.onKey?.(nav)) { e.preventDefault(); return true; }
              // 弹层没开时 Esc 退出占位（没有占位就照常交出去）。
              if (nav === "escape" && !e.isComposing && e.keyCode !== 229 && !v.compositionStarted && leaveSlots(v)) { e.preventDefault(); return true; }
              if (e.key !== "Enter" || e.isComposing || e.keyCode === 229 || v.composing) return false;
              e.preventDefault();
              if (e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) {
                if (!insertNewlineContinueMarkup(v)) insertNewlineAndIndent(v);
              } else {
                latest.current.onSubmit();
              }
              return true;
            },
            // 组字时外面不弹、不插；组完等 CodeMirror 把字收进来再报一次。
            compositionstart: (_e, v) => { window.clearTimeout(ended); report(v, true); return false; },
            compositionend: (_e, v) => { window.clearTimeout(ended); ended = window.setTimeout(() => report(v), 0); return false; },
          })),
          keymap.of([...defaultKeymap.filter((b) => b.key !== "Mod-[" && b.key !== "Mod-]"), ...historyKeymap]),
          Prec.high(keymap.of([{ key: "Tab", run: (v) => nextSlot(v) || indentList(v), shift: outdentList }])),
          slotField,
          markdown(),
          syntaxHighlighting(peckMarkdown),
          EditorView.lineWrapping,
          theme,
          hint.current.of(placeholderExt(placeholder)),
          editable.current.of(EditorView.editable.of(!disabled)),
          EditorView.contentAttributes.of({ spellcheck: "false", autocorrect: "off", autocapitalize: "off", "aria-label": ariaLabel, "aria-multiline": "true" }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) latest.current.onChange(u.state.doc.toString());
            if (u.docChanged || u.selectionSet) report(u.view);
          }),
        ],
      }),
    });
    view.current = v;
    return () => { window.clearTimeout(ended); v.destroy(); view.current = null; };
    // 只建一次；value、disabled、placeholder 的变化在下面同步。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 外面改了内容（发出去以后清空、TO TASK 放进来的）：同步进编辑器。
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    const current = v.state.doc.toString();
    if (current !== value) v.dispatch({ changes: { from: 0, to: current.length, insert: value }, selection: { anchor: value.length } });
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({ effects: editable.current.reconfigure(EditorView.editable.of(!disabled)) });
  }, [disabled]);

  useEffect(() => {
    view.current?.dispatch({ effects: hint.current.reconfigure(placeholderExt(placeholder)) });
  }, [placeholder]);

  return <div ref={host} className="md-editor" />;
});
MarkdownEditor.displayName = "MarkdownEditor";
