import React from "react";
import { CheckSquare, Square } from "lucide-react";

interface MarkdownViewProps {
  content: string;
}

export const MarkdownView: React.FC<MarkdownViewProps> = ({ content }) => {
  const lines = content.split("\n");
  const elements: React.ReactNode[] = [];
  let idx = 0;

  while (idx < lines.length) {
    const line = lines[idx];

    // Table block
    if (line.trim().startsWith("|") && lines[idx + 1]?.includes("---")) {
      const headers = line
        .split("|")
        .map((c) => c.trim())
        .filter(Boolean);
      idx += 2;
      const rows: string[][] = [];
      while (idx < lines.length && lines[idx].trim().startsWith("|")) {
        rows.push(
          lines[idx]
            .split("|")
            .map((c) => c.trim())
            .filter(Boolean),
        );
        idx++;
      }
      elements.push(
        <div key={`tbl-${idx}`} className="my-3 overflow-x-auto">
          <table className="w-full border-collapse rounded-md border border-[#262B34] text-left text-xs">
            <thead>
              <tr className="border-b border-[#262B34] bg-[#14171C]">
                {headers.map((h, i) => (
                  <th key={i} className="px-3 py-2 font-semibold text-[#E6EDF3]">
                    {renderInline(h)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, rIdx) => (
                <tr
                  key={rIdx}
                  className="border-b border-[#262B34]/60 bg-[#181B21]/60"
                >
                  {r.map((cell, cIdx) => (
                    <td key={cIdx} className="px-3 py-2 text-[#C9D1D9]">
                      {renderInline(cell)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    // Headings
    if (line.startsWith("# ")) {
      elements.push(
        <h1
          key={idx}
          className="mb-3 mt-1 border-b border-[#262B34] pb-2 text-base font-bold text-[#E6EDF3]"
        >
          {renderInline(line.slice(2))}
        </h1>,
      );
    } else if (line.startsWith("## ")) {
      elements.push(
        <h2
          key={idx}
          className="mb-2 mt-4 text-sm font-semibold text-[#00E5FF]"
        >
          {renderInline(line.slice(3))}
        </h2>,
      );
    } else if (line.startsWith("> ")) {
      elements.push(
        <blockquote
          key={idx}
          className="my-2.5 border-l-2 border-[#00E5FF] bg-[#14171C] px-3 py-1.5 text-xs text-[#C9D1D9]"
        >
          {renderInline(line.slice(2))}
        </blockquote>,
      );
    } else if (/^\s*-\s*\[([ xX])\]\s+/.test(line)) {
      const checked = /^\s*-\s*\[[xX]\]/.test(line);
      const text = line.replace(/^\s*-\s*\[[ xX]\]\s+/, "");
      elements.push(
        <div
          key={idx}
          className="my-1.5 flex items-center gap-2 rounded bg-[#14171C] px-2.5 py-1.5 text-xs"
        >
          {checked ? (
            <CheckSquare className="h-3.5 w-3.5 shrink-0 text-[#10B981]" />
          ) : (
            <Square className="h-3.5 w-3.5 shrink-0 text-[#6E7681]" />
          )}
          <span
            className={
              checked ? "line-through text-[#8B949E]" : "text-[#E6EDF3]"
            }
          >
            {renderInline(text)}
          </span>
        </div>,
      );
    } else if (/^\s*[-*]\s+/.test(line)) {
      const text = line.replace(/^\s*[-*]\s+/, "");
      elements.push(
        <div key={idx} className="my-1 flex items-start gap-2 pl-2 text-xs text-[#C9D1D9]">
          <span className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full bg-[#00E5FF]" />
          <span>{renderInline(text)}</span>
        </div>,
      );
    } else if (/^\s*\d+\.\s+/.test(line)) {
      const match = line.match(/^\s*(\d+\.)\s+(.*)$/);
      if (match) {
        elements.push(
          <div key={idx} className="my-1 flex items-start gap-2 pl-2 text-xs text-[#C9D1D9]">
            <span className="font-mono-code font-semibold text-[#00E5FF]">
              {match[1]}
            </span>
            <span>{renderInline(match[2])}</span>
          </div>,
        );
      }
    } else if (line.trim().length > 0) {
      elements.push(
        <p key={idx} className="my-1.5 text-xs leading-relaxed text-[#C9D1D9]">
          {renderInline(line)}
        </p>,
      );
    }

    idx++;
  }

  return (
    <div className="selectable-text h-full overflow-y-auto p-5">
      <div className="mx-auto max-w-3xl">{elements}</div>
    </div>
  );
};

function renderInline(text: string): React.ReactNode {
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return parts.map((part, i) => {
    if (part.startsWith("`") && part.endsWith("`")) {
      return (
        <code
          key={i}
          className="font-mono-code rounded border border-[#262B34] bg-[#14171C] px-1.5 py-0.5 text-[11px] text-[#00E5FF]"
        >
          {part.slice(1, -1)}
        </code>
      );
    }
    if (part.startsWith("**") && part.endsWith("**")) {
      return (
        <strong key={i} className="font-semibold text-[#E6EDF3]">
          {part.slice(2, -2)}
        </strong>
      );
    }
    return part;
  });
}
