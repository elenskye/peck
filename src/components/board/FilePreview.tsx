import React, { useEffect, useState } from "react";
import { Check, Columns2, Copy, Eye, FileDiff, Rows3 } from "lucide-react";
import { DiffView } from "@/components/board/DiffView";
import { MarkdownView } from "@/components/board/MarkdownView";
import type { DiffResult } from "@/lib/types";
import { cn, isMarkdownFile } from "@/lib/utils";

interface FilePreviewProps {
  diff: DiffResult | null;
}

export const FilePreview: React.FC<FilePreviewProps> = ({ diff }) => {
  const isMd = diff ? isMarkdownFile(diff.filePath) : false;
  const [viewMode, setViewMode] = useState<"markdown" | "unified" | "split">(
    "unified",
  );
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!diff) return;
    if (isMarkdownFile(diff.filePath) && diff.contentAfter) {
      setViewMode("markdown");
    } else {
      setViewMode("unified");
    }
  }, [diff]);

  if (!diff) {
    return (
      <div className="flex flex-1 items-center justify-center bg-[#181B21] text-xs text-[#8B949E]">
        在左侧列表选择变动文件以预览 Markdown 或代码 Diff
      </div>
    );
  }

  const handleCopy = async () => {
    const text =
      viewMode === "markdown" && diff.contentAfter
        ? diff.contentAfter
        : diff.unifiedDiff;
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#181B21]">
      {/* Header Toolbar */}
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-[#262B34] bg-[#14171C] px-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileDiff className="h-3.5 w-3.5 shrink-0 text-[#00E5FF]" />
          <span className="font-mono-code truncate text-xs font-medium text-[#E6EDF3]">
            {diff.filePath}
          </span>
          <span className="rounded bg-[#1E222A] px-1.5 py-0.5 font-mono-code text-[10px] text-[#8B949E]">
            {diff.kind === "create"
              ? "新增"
              : diff.kind === "delete"
                ? "删除"
                : "修改"}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          {isMd && diff.contentAfter && (
            <button
              type="button"
              onClick={() => setViewMode("markdown")}
              className={cn(
                "inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] transition-colors",
                viewMode === "markdown"
                  ? "bg-[#00E5FF]/15 font-medium text-[#00E5FF]"
                  : "text-[#8B949E] hover:text-[#E6EDF3]",
              )}
            >
              <Eye className="h-3 w-3" />
              预览 Markdown
            </button>
          )}

          <button
            type="button"
            onClick={() => setViewMode("unified")}
            className={cn(
              "inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] transition-colors",
              viewMode === "unified"
                ? "bg-[#00E5FF]/15 font-medium text-[#00E5FF]"
                : "text-[#8B949E] hover:text-[#E6EDF3]",
            )}
          >
            <Rows3 className="h-3 w-3" />
            Unified Diff
          </button>

          <button
            type="button"
            onClick={() => setViewMode("split")}
            className={cn(
              "inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] transition-colors",
              viewMode === "split"
                ? "bg-[#00E5FF]/15 font-medium text-[#00E5FF]"
                : "text-[#8B949E] hover:text-[#E6EDF3]",
            )}
          >
            <Columns2 className="h-3 w-3" />
            分栏对比
          </button>

          <button
            type="button"
            onClick={handleCopy}
            className="ml-1 inline-flex items-center gap-1 rounded border border-[#262B34] bg-[#181B21] px-2 py-0.5 text-[11px] text-[#8B949E] hover:border-[#373E4B] hover:text-[#E6EDF3]"
          >
            {copied ? (
              <>
                <Check className="h-3 w-3 text-[#10B981]" />
                <span className="text-[#10B981]">已复制</span>
              </>
            ) : (
              <>
                <Copy className="h-3 w-3" />
                <span>复制</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Content Area */}
      <div className="min-h-0 flex-1 overflow-hidden">
        {viewMode === "markdown" && diff.contentAfter ? (
          <MarkdownView content={diff.contentAfter} />
        ) : (
          <DiffView
            unifiedDiff={diff.unifiedDiff}
            mode={viewMode === "split" ? "split" : "unified"}
          />
        )}
      </div>
    </div>
  );
};
