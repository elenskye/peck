// 输入框：CodeMirror 6 + Markdown，边打边渲染（标题加粗变大、`代码` 等宽、**粗体**、> 引用、列表符号变淡）。
// ↵ 发送，⇧↵ 或 ⌘↵ 换行（列表里换行接着列表）；列表里 Tab 缩进一级、⇧Tab 退一级（v1.1.12）；
// 中文输入法照常（组字时的回车是选字，不发）；不拼写检查、不自动改字。样子照 Peck 的输入格。
import React, { useEffect, useRef } from "react";
import { Compartment, EditorState, Prec, type ChangeSpec } from "@codemirror/state";
import { EditorView, keymap, placeholder as placeholderExt } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, insertNewlineAndIndent } from "@codemirror/commands";
import { insertNewlineContinueMarkup, markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";

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
});

interface Props {
  value: string;
  onChange: (text: string) => void;
  onSubmit: () => void;
  /** ↑ ↓ Esc 先问外面要不要（比如 @ 弹层开着）；回 true 就不交给编辑器。 */
  onKey?: (key: "up" | "down" | "escape") => boolean;
  placeholder: string;
  disabled?: boolean;
  ariaLabel: string;
}

export const MarkdownEditor: React.FC<Props> = ({ value, onChange, onSubmit, onKey, placeholder, disabled, ariaLabel }) => {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const editable = useRef(new Compartment());
  const hint = useRef(new Compartment());
  const latest = useRef({ onChange, onSubmit, onKey });
  latest.current = { onChange, onSubmit, onKey };

  useEffect(() => {
    if (!host.current) return;
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
              if (nav && !e.isComposing && !v.composing && latest.current.onKey?.(nav)) { e.preventDefault(); return true; }
              if (e.key !== "Enter" || e.isComposing || e.keyCode === 229 || v.composing) return false;
              e.preventDefault();
              if (e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) {
                if (!insertNewlineContinueMarkup(v)) insertNewlineAndIndent(v);
              } else {
                latest.current.onSubmit();
              }
              return true;
            },
          })),
          keymap.of([...defaultKeymap.filter((b) => b.key !== "Mod-[" && b.key !== "Mod-]"), ...historyKeymap]),
          Prec.high(keymap.of([{ key: "Tab", run: indentList, shift: outdentList }])),
          markdown(),
          syntaxHighlighting(peckMarkdown),
          EditorView.lineWrapping,
          theme,
          hint.current.of(placeholderExt(placeholder)),
          editable.current.of(EditorView.editable.of(!disabled)),
          EditorView.contentAttributes.of({ spellcheck: "false", autocorrect: "off", autocapitalize: "off", "aria-label": ariaLabel, "aria-multiline": "true" }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) latest.current.onChange(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = v;
    return () => { v.destroy(); view.current = null; };
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
};
