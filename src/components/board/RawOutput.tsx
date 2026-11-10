import React, { useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  TerminalSquare,
} from "lucide-react";
import type { RawLogEntry, StreamType } from "@/lib/types";
import { cn, formatTimeShort } from "@/lib/utils";

interface RawOutputProps {
  logs: RawLogEntry[];
}

export const RawOutput: React.FC<RawOutputProps> = ({ logs }) => {
  // Default collapsed as required by Peck design spec
  const [expanded, setExpanded] = useState(false);
  const [filter, setFilter] = useState<"all" | StreamType>("all");
  const [copied, setCopied] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const filteredLogs = logs.filter((l) =>
    filter === "all" ? true : l.stream === filter,
  );
  const stderrCount = logs.filter((l) => l.stream === "stderr").length;

  useEffect(() => {
    if (expanded) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [expanded, filteredLogs.length]);

  const handleCopyAll = async (e: React.MouseEvent) => {
    e.stopPropagation();
    const text = filteredLogs.map((l) => l.chunk).join("\n");
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="shrink-0 border-t border-[#262B34] bg-[#14171C]">
      {/* Drawer Header */}
      <div
        onClick={() => setExpanded((v) => !v)}
        className="flex h-9 cursor-pointer items-center justify-between px-3.5 transition-colors hover:bg-[#181B21]"
      >
        <div className="flex items-center gap-2 text-xs">
          {expanded ? (
            <ChevronDown className="h-3.5 w-3.5 text-[#8B949E]" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 text-[#8B949E]" />
          )}
          <TerminalSquare className="h-3.5 w-3.5 text-[#00E5FF]" />
          <span className="font-medium text-[#C9D1D9]">
            运行原始输出 (Stdout / Stderr)
          </span>
          <span className="font-mono-code rounded bg-[#1E222A] px-1.5 py-0.5 text-[10px] text-[#8B949E]">
            {logs.length} 行
          </span>
          {stderrCount > 0 && (
            <span className="font-mono-code rounded bg-[#F59E0B]/15 px-1.5 py-0.5 text-[10px] text-[#F59E0B]">
              {stderrCount} 警告/Stderr
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {expanded && (
            <div
              onClick={(e) => e.stopPropagation()}
              className="flex items-center gap-1"
            >
              {(["all", "stdout", "stderr"] as const).map((f) => (
                <button
                  key={f}
                  type="button"
                  onClick={() => setFilter(f)}
                  className={cn(
                    "rounded px-2 py-0.5 text-[10px] uppercase transition-colors",
                    filter === f
                      ? "bg-[#00E5FF]/15 font-semibold text-[#00E5FF]"
                      : "text-[#8B949E] hover:text-[#E6EDF3]",
                  )}
                >
                  {f}
                </button>
              ))}
              <button
                type="button"
                onClick={handleCopyAll}
                className="ml-1 inline-flex items-center gap-1 rounded border border-[#262B34] bg-[#181B21] px-2 py-0.5 text-[10px] text-[#8B949E] hover:text-[#E6EDF3]"
              >
                {copied ? (
                  <Check className="h-3 w-3 text-[#10B981]" />
                ) : (
                  <Copy className="h-3 w-3" />
                )}
                <span>复制全部</span>
              </button>
            </div>
          )}
          <span className="text-[11px] text-[#8B949E]">
            {expanded ? "点击收起" : "默认收起 · 点击展开"}
          </span>
        </div>
      </div>

      {/* Collapsible Terminal Body */}
      {expanded && (
        <div className="selectable-text font-mono-code h-44 overflow-y-auto border-t border-[#262B34] bg-[#0B0D10] px-4 py-2.5 text-[11.5px] leading-relaxed">
          {filteredLogs.length === 0 ? (
            <div className="text-[#6E7681]">暂无底层输出日志...</div>
          ) : (
            filteredLogs.map((entry) => (
              <div key={entry.id} className="flex items-start gap-2.5 py-0.5">
                <span className="shrink-0 select-none text-[10px] text-[#6E7681]">
                  {formatTimeShort(entry.timestamp)}
                </span>
                <span
                  className={cn(
                    "shrink-0 rounded px-1 text-[10px] uppercase",
                    entry.stream === "stderr"
                      ? "bg-[#F59E0B]/15 text-[#F59E0B]"
                      : "bg-[#1E222A] text-[#00E5FF]",
                  )}
                >
                  {entry.agentId}
                </span>
                <span
                  className={cn(
                    "whitespace-pre-wrap break-all",
                    entry.stream === "stderr"
                      ? "text-[#FCD34D]"
                      : "text-[#D1D5DB]",
                  )}
                >
                  {entry.chunk}
                </span>
              </div>
            ))
          )}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  );
};
