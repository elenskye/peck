import React, { useEffect, useState } from "react";
import { Layers, ShieldCheck } from "lucide-react";
import { ChangeList } from "@/components/board/ChangeList";
import { FilePreview } from "@/components/board/FilePreview";
import { RawOutput } from "@/components/board/RawOutput";
import { StepTimeline } from "@/components/board/StepTimeline";
import { TaskChecklist } from "@/components/board/TaskChecklist";
import { ipc } from "@/lib/ipc";
import type { DiffResult, RunRecord } from "@/lib/types";

interface ArtifactBoardProps {
  run: RunRecord | null;
  onRetryStep: (stepId: string) => void;
}

export const ArtifactBoard: React.FC<ArtifactBoardProps> = ({
  run,
  onRetryStep,
}) => {
  const [selectedStepId, setSelectedStepId] = useState<string | null>(null);
  const [selectedFilePath, setSelectedFilePath] = useState<string | null>(null);
  const [currentDiff, setCurrentDiff] = useState<DiffResult | null>(null);

  useEffect(() => { setSelectedStepId(null); setSelectedFilePath(null); }, [run?.id]);

  // Filter files by step if a step is selected
  const visibleFiles = run
    ? selectedStepId
      ? run.files.filter((f) => f.stepId === selectedStepId)
      : run.files
    : [];

  useEffect(() => {
    if (!run || visibleFiles.length === 0) {
      setSelectedFilePath(null);
      setCurrentDiff(null);
      return;
    }
    const stillExists = visibleFiles.some((f) => f.path === selectedFilePath);
    const nextPath = stillExists
      ? selectedFilePath!
      : visibleFiles[0].path;
    setSelectedFilePath(nextPath);
  }, [run, selectedStepId, visibleFiles.length]);

  useEffect(() => {
    let cancelled = false;
    if (!run || !selectedFilePath) {
      setCurrentDiff(null);
      return;
    }
    void (async () => {
      const diff = await ipc.changesGetDiff(
        run.id,
        selectedStepId ?? undefined,
        selectedFilePath,
      );
      if (!cancelled) setCurrentDiff(diff);
    })().catch(() => { if (!cancelled) setCurrentDiff(null); });
    return () => { cancelled = true; };
  }, [run?.id, run?.status, run?.files, selectedStepId, selectedFilePath]);

  if (!run) {
    return (
      <div className="flex h-full flex-col items-center justify-center bg-[#0F1115] p-8 text-center">
        <div className="flex h-12 w-12 items-center justify-center rounded-xl border border-[#262B34] bg-[#181B21] text-[#00E5FF]">
          <Layers className="h-6 w-6" />
        </div>
        <h2 className="mt-3 text-sm font-semibold text-[#E6EDF3]">
          右侧产物与文件变动看板
        </h2>
        <p className="mt-1 max-w-md text-xs leading-relaxed text-[#8B949E]">
          当您在左侧发出自然语言指令后，这里将实时呈现多步流水线状态、
          <code className="font-mono-code mx-1 text-[#00E5FF]">.peck/tasks.md</code>
          勾选进度、意图文档预览及影子 Git 生成的精确代码 Diff。
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden bg-[#0F1115]">
      {/* Board Header Bar */}
      <div className="flex h-10 shrink-0 items-center justify-between border-b border-[#262B34] bg-[#14171C] px-4">
        <div className="flex items-center gap-2.5">
          <span className="font-mono-code rounded bg-[#00E5FF]/12 px-2 py-0.5 text-xs font-semibold text-[#00E5FF]">
            Run #{run.number}
          </span>
          <span className="text-xs font-semibold text-[#E6EDF3]">
            {run.flowName}
          </span>
          <span className="max-w-[360px] truncate text-xs text-[#8B949E]">
            — {run.prompt}
          </span>
        </div>

        <div className="flex items-center gap-3 text-[11px] text-[#8B949E]">
          <span className="inline-flex items-center gap-1 text-[#10B981]">
            <ShieldCheck className="h-3.5 w-3.5" />
            Shadow Git 快照隔离已开启
          </span>
        </div>
      </div>

      {/* Upper Section: StepTimeline & TaskChecklist */}
      <div className="shrink-0 space-y-2.5 border-b border-[#262B34] bg-[#0F1115] p-3">
        <StepTimeline
          steps={run.steps}
          selectedStepId={selectedStepId}
          onSelectStep={setSelectedStepId}
          onRetryStep={onRetryStep}
        />
        <TaskChecklist tasks={run.tasks} />
      </div>

      {/* Middle Section: ChangeList + FilePreview (Markdown / Diff) */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <ChangeList
          files={visibleFiles}
          selectedFilePath={selectedFilePath}
          onSelectFile={setSelectedFilePath}
        />
        <FilePreview diff={currentDiff} />
      </div>

      {/* Bottom Collapsible Drawer: RawOutput */}
      <RawOutput logs={run.logs} />
    </div>
  );
};
