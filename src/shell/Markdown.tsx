// 回答和站主发的话都按 Markdown 显示：标题、列表、勾选、引用、表格、行内代码、代码块（代码上色等 v0.7）。
// 链接不在 App 里跳走：显示成一段带下划线的字，悬停看地址。
import React from "react";
import ReactMarkdown, { defaultUrlTransform, type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn, copyText } from "@/lib/utils";

/** 代码块：右上角一个 COPY（v0.8）。 */
const Pre: React.FC<{ children?: React.ReactNode }> = ({ children }) => {
  const ref = React.useRef<HTMLPreElement>(null);
  const [done, setDone] = React.useState(false);
  const copy = () => {
    void copyText(ref.current?.innerText ?? "").then(() => { setDone(true); window.setTimeout(() => setDone(false), 1200); }).catch(() => {});
  };
  return (
    <div className="md-pre">
      <pre ref={ref}>{children}</pre>
      <button type="button" className="md-copy" onClick={copy} aria-label="复制这段代码">{done ? "COPIED" : "COPY"}</button>
    </div>
  );
};

const COMPONENTS: Components = {
  // `[@标题](peck:task/<任务 id>)`：输入框里 @ 进来的旧任务，显示成一个小标记。
  a: ({ href, children }) => href?.startsWith("peck:task/")
    ? <span className="md-task" title="引用的旧任务">{children}</span>
    : <span className="md-link" title={href}>{children}</span>,
  img: ({ alt }) => <span className="md-link">[图片{alt ? `：${alt}` : ""}]</span>,
  pre: ({ children }) => <Pre>{children}</Pre>,
  input: ({ checked }) => <span className={cn("pk-check", checked && "is-on")} aria-checked={!!checked} role="img"><i className="pk-i pk-i--check pk-i--12" /></span>,
};

/** 记住结果：流式的时候整个任务每 40ms 重画一次，字没变的回答不用重新解析。 */
export const Md = React.memo<{ text: string; className?: string }>(({ text, className }) => (
  <div className={cn("md sel", className)}>
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS} urlTransform={(url) => (url.startsWith("peck:task/") ? url : defaultUrlTransform(url))}>
      {text}
    </ReactMarkdown>
  </div>
));
