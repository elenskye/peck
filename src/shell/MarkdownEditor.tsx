// 输入框：CodeMirror 6 + Markdown，边打边渲染（标题加粗变大、`代码` 等宽、**粗体**、> 引用、列表符号变淡）。
// ↵ 发送，⇧↵ 或 ⌘↵ 换行（列表里换行接着列表）；中文输入法照常（组字时的回车是选字，不发）；
// 不拼写检查、不自动改字。样子照 Peck 的输入格。
import React, { useEffect, useRef } from "react";
import { Compartment, EditorState, Prec } from "@codemirror/state";
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

const theme = EditorView.theme({
  "&": { backgroundColor: "transparent", color: "var(--color-text)", fontSize: "inherit", maxHeight: "132px" },
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
          keymap.of([...defaultKeymap, ...historyKeymap]),
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
