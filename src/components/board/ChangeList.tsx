import React from "react";
import { FileCode, FilePlus2, FileText, FileX2 } from "lucide-react";
import type { FileChangeItem } from "@/lib/types";
import { cn, isMarkdownFile } from "@/lib/utils";

interface ChangeListProps {
  files: FileChangeItem[];
  selectedFilePath: string | null;
  onSelectFile: (path: string) => void;
}

export const ChangeList: React.FC<ChangeListProps> = ({
  files,
  selectedFilePath,
  onSelectFile,
}) => {
  const totalAdditions = files.reduce((sum, f) => sum + f.additions, 0);
  const totalDeletions = files.reduce((sum, f) => sum + f.deletions, 0);

  return (
    <div className="flex h-full w-[260px] shrink-0 flex-col border-r border-[#262B34] bg-[#14171C]">
      <div className="flex h-9 items-center justify-between border-b border-[#262B34] px-3">
        <span className="text-xs font-semibold text-[#E6EDF3]">
          变动文件 ({files.length})
        </span>
        <div className="font-mono-code flex items-center gap-1.5 text-[11px]">
          <span className="text-[#10B981]">+{totalAdditions}</span>
          <span className="text-[#EF4444]">-{totalDeletions}</span>
        </div>
      </div>

      <div className="flex-1 space-y-0.5 overflow-y-auto p-1.5">
        {files.map((file) => {
          const isSelected = file.path === selectedFilePath;
          const isMd = isMarkdownFile(file.path);

          return (
            <button
              key={file.path}
              type="button"
              onClick={() => onSelectFile(file.path)}
              className={cn(
                "flex w-full items-center justify-between gap-2 rounded-md px-2.5 py-2 text-left text-xs transition-colors",
                isSelected
                  ? "bg-[#00E5FF]/12 text-[#E6EDF3] ring-1 ring-[#00E5FF]/40"
                  : "text-[#C9D1D9] hover:bg-[#181B21]",
              )}
            >
              <div className="flex min-w-0 items-center gap-2">
                {file.kind === "create" ? (
                  <FilePlus2 className="h-3.5 w-3.5 shrink-0 text-[#10B981]" />
                ) : file.kind === "delete" ? (
                  <FileX2 className="h-3.5 w-3.5 shrink-0 text-[#EF4444]" />
                ) : isMd ? (
                  <FileText className="h-3.5 w-3.5 shrink-0 text-[#00E5FF]" />
                ) : (
                  <FileCode className="h-3.5 w-3.5 shrink-0 text-[#3B82F6]" />
                )}
                <span
                  title={file.path}
                  className="font-mono-code truncate text-[11.5px]"
                >
                  {file.path}
                </span>
              </div>

              <div className="font-mono-code flex shrink-0 items-center gap-1 text-[10px]">
                {file.additions > 0 && (
                  <span className="text-[#10B981]">+{file.additions}</span>
                )}
                {file.deletions > 0 && (
                  <span className="text-[#EF4444]">-{file.deletions}</span>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
};
