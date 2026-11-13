// 回答和站主发的话都按 Markdown 显示：标题、列表、勾选、引用、表格、行内代码、代码块（代码上色等 v0.7）。
// 链接不在 App 里跳走：显示成一段带下划线的字，悬停看地址。
import React from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "@/lib/utils";

const COMPONENTS: Components = {
  a: ({ href, children }) => <span className="md-link" title={href}>{children}</span>,
  img: ({ alt }) => <span className="md-link">[图片{alt ? `：${alt}` : ""}]</span>,
  input: ({ checked }) => <span className={cn("pk-check", checked && "is-on")} aria-checked={!!checked} role="img"><i className="pk-i pk-i--check pk-i--12" /></span>,
};

export const Md: React.FC<{ text: string; className?: string }> = ({ text, className }) => (
  <div className={cn("md sel", className)}>
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
      {text}
    </ReactMarkdown>
  </div>
);
