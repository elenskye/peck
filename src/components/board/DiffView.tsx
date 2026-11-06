import React, { useMemo } from "react";
import { cn } from "@/lib/utils";

interface DiffViewProps {
  unifiedDiff: string;
  mode: "unified" | "split";
}

interface ParsedDiffLine {
  type: "header" | "hunk" | "add" | "del" | "context";
  content: string;
  oldLine?: number;
  newLine?: number;
}

export const DiffView: React.FC<DiffViewProps> = ({ unifiedDiff, mode }) => {
  const parsedLines = useMemo(() => parseUnifiedDiff(unifiedDiff), [unifiedDiff]);

  if (mode === "split") {
    return (
      <div className="selectable-text font-mono-code h-full overflow-auto bg-[#0F1115] text-[12px] leading-5">
        <div className="grid min-w-[680px] grid-cols-2 divide-x divide-[#262B34]">
          {/* Left Column: Old / Deletions */}
          <div>
            {parsedLines
              .filter((l) => l.type !== "add")
              .map((line, idx) => (
                <div
                  key={`left-${idx}`}
                  className={cn(
                    "flex px-2 py-0.5",
                    line.type === "del"
                      ? "bg-[#EF4444]/15 text-[#FCA5A5]"
                      : line.type === "hunk"
                        ? "bg-[#1E222A] text-[#00E5FF]"
                        : line.type === "header"
                          ? "bg-[#14171C] text-[#8B949E]"
                          : "text-[#C9D1D9]",
                  )}
                >
                  <span className="w-9 shrink-0 select-none pr-2 text-right text-[11px] text-[#6E7681]">
                    {line.oldLine ?? ""}
                  </span>
                  <span className="whitespace-pre-wrap break-all">
                    {line.content}
                  </span>
                </div>
              ))}
          </div>

          {/* Right Column: New / Additions */}
          <div>
            {parsedLines
              .filter((l) => l.type !== "del")
              .map((line, idx) => (
                <div
                  key={`right-${idx}`}
                  className={cn(
                    "flex px-2 py-0.5",
                    line.type === "add"
                      ? "bg-[#10B981]/15 text-[#6EE7B7]"
                      : line.type === "hunk"
                        ? "bg-[#1E222A] text-[#00E5FF]"
                        : line.type === "header"
                          ? "bg-[#14171C] text-[#8B949E]"
                          : "text-[#C9D1D9]",
                  )}
                >
                  <span className="w-9 shrink-0 select-none pr-2 text-right text-[11px] text-[#6E7681]">
                    {line.newLine ?? ""}
                  </span>
                  <span className="whitespace-pre-wrap break-all">
                    {line.content}
                  </span>
                </div>
              ))}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="selectable-text font-mono-code h-full overflow-auto bg-[#0F1115] py-2 text-[12px] leading-5">
      {parsedLines.map((line, idx) => (
        <div
          key={idx}
          className={cn(
            "flex px-3 py-0.5",
            line.type === "add" && "bg-[#10B981]/14 text-[#A7F3D0]",
            line.type === "del" && "bg-[#EF4444]/14 text-[#FECACA]",
            line.type === "hunk" && "my-1 bg-[#1E222A]/90 py-1 text-[#00E5FF]",
            line.type === "header" && "text-[#8B949E]",
            line.type === "context" && "text-[#C9D1D9]",
          )}
        >
          <span className="w-9 shrink-0 select-none pr-2 text-right text-[11px] text-[#6E7681]">
            {line.oldLine ?? ""}
          </span>
          <span className="w-9 shrink-0 select-none pr-3 text-right text-[11px] text-[#6E7681]">
            {line.newLine ?? ""}
          </span>
          <span className="whitespace-pre-wrap break-all">{line.content}</span>
        </div>
      ))}
    </div>
  );
};

function parseUnifiedDiff(raw: string): ParsedDiffLine[] {
  const lines = raw.split("\n");
  const result: ParsedDiffLine[] = [];
  let oldLine = 1;
  let newLine = 1;

  for (const line of lines) {
    if (
      line.startsWith("diff --git") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ") ||
      line.startsWith("new file mode")
    ) {
      result.push({ type: "header", content: line });
      continue;
    }

    if (line.startsWith("@@")) {
      const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (match) {
        oldLine = parseInt(match[1], 10);
        newLine = parseInt(match[2], 10);
      }
      result.push({ type: "hunk", content: line });
      continue;
    }

    if (line.startsWith("+")) {
      result.push({
        type: "add",
        content: line,
        newLine: newLine++,
      });
    } else if (line.startsWith("-")) {
      result.push({
        type: "del",
        content: line,
        oldLine: oldLine++,
      });
    } else {
      result.push({
        type: "context",
        content: line,
        oldLine: oldLine++,
        newLine: newLine++,
      });
    }
  }

  return result;
}
