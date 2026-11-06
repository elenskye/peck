import React from "react";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  FileCode2,
  Loader2,
  RotateCcw,
  XCircle,
  Zap,
} from "lucide-react";
import type { RunRecord } from "@/lib/types";
import { cn } from "@/lib/utils";

interface RunCardProps {
  run: RunRecord;
  isSelected: boolean;
  onSelectRun: (runId: string) => void;
  onRetryStep?: (runId: string, stepId: string) => void;
}

export const RunCard: React.FC<RunCardProps> = ({
  run,
  isSelected,
  onSelectRun,
  onRetryStep,
}) => {
  const totalTasks = run.tasks.length;
  const completedTasks = run.tasks.filter((t) => t.completed).length;
  const progressPercent =
    totalTasks > 0
      ? Math.round((completedTasks / totalTasks) * 100)
      : run.status === "succeeded"
        ? 100
        : run.status === "running"
          ? 45
          : 0;

  const runningStep = run.steps.find((s) => s.status === "running");
  const failedStep = run.steps.find((s) => s.status === "failed" || s.status === "cancelled");

  return (
    <div
      onClick={() => onSelectRun(run.id)}
      className={cn(
        "mt-2.5 cursor-pointer rounded-lg border p-3 transition-all",
        isSelected
          ? "border-[#00E5FF]/50 bg-[#181B21] shadow-lg shadow-black/30"
          : "border-[#262B34] bg-[#181B21]/80 hover:border-[#373E4B] hover:bg-[#181B21]",
      )}
    >
      {/* Top Row: Run Badge & Status */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="inline-flex items-center gap-1 rounded bg-[#00E5FF]/12 px-1.5 py-0.5 font-mono-code text-[11px] font-semibold text-[#00E5FF]">
            <Zap className="h-3 w-3" />
            Run #{run.number}
          </span>
          <span className="text-xs font-medium text-[#E6EDF3]">
            {run.flowName}
          </span>
        </div>

        <StatusBadge status={run.status} />
      </div>

      {run.steps.some((s) => s.executionMode === "mock") && <p className="mt-2 rounded border border-[#F59E0B]/30 bg-[#F59E0B]/10 px-2 py-1 text-[11px] text-[#F59E0B]">包含模拟步骤 · 未完成真实 AI 调用验收</p>}

      {/* Active Step or Summary */}
      <div className="mt-2 text-xs text-[#8B949E]">
        {runningStep ? (
          <span className="text-[#3B82F6]">
            正在执行 Step {runningStep.stepIndex + 1} · {runningStep.agentName} ({runningStep.title})
          </span>
        ) : (
          <span>
            {run.summary ||
              `共 ${run.steps.length} 个步骤 · 产出 ${run.files.length} 个文件变动`}
          </span>
        )}
      </div>

      {/* Progress Bar */}
      <div className="mt-2.5">
        <div className="mb-1 flex items-center justify-between text-[11px]">
          <span className="text-[#8B949E]">
            任务进度 (.peck/tasks.md)
          </span>
          <span className="font-mono-code text-[#C9D1D9]">
            {totalTasks > 0
              ? `${completedTasks}/${totalTasks} (${progressPercent}%)`
              : `${progressPercent}%`}
          </span>
        </div>
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-[#0F1115]">
          <div
            style={{ width: `${progressPercent}%` }}
            className={cn(
              "h-full rounded-full transition-all duration-300",
              run.status === "failed"
                ? "bg-[#EF4444]"
                : run.status === "running"
                  ? "bg-[#3B82F6]"
                  : "bg-[#00E5FF]",
            )}
          />
        </div>
      </div>

      {/* Failed Step Recovery Actions */}
      {failedStep && onRetryStep && (
        <div className="mt-2.5 flex items-center justify-between rounded border border-[#EF4444]/30 bg-[#EF4444]/10 px-2.5 py-1.5 text-[11px]">
          <span className="text-[#FCA5A5]">
            Step {failedStep.stepIndex + 1} ({failedStep.title}) 异常暂停
          </span>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onRetryStep(run.id, failedStep.id);
            }}
            className="inline-flex items-center gap-1 rounded bg-[#EF4444] px-2 py-0.5 font-medium text-white hover:bg-[#DC2626]"
          >
            <RotateCcw className="h-3 w-3" />
            重试此步骤
          </button>
        </div>
      )}

      {/* Footer Info */}
      <div className="mt-2.5 flex items-center justify-between border-t border-[#262B34]/70 pt-2 text-[11px]">
        <span className="inline-flex items-center gap-1 text-[#8B949E]">
          <FileCode2 className="h-3 w-3 text-[#00E5FF]" />
          {run.files.length} 个文件变动
        </span>
        <span className="inline-flex items-center gap-1 font-medium text-[#00E5FF]">
          在右侧看板查看详情
          <ArrowRight className="h-3 w-3" />
        </span>
      </div>
    </div>
  );
};

const StatusBadge: React.FC<{ status: RunRecord["status"] }> = ({ status }) => {
  switch (status) {
    case "running":
      return (
        <span className="inline-flex items-center gap-1 rounded-full bg-[#3B82F6]/15 px-2 py-0.5 text-[11px] font-medium text-[#3B82F6]">
          <Loader2 className="h-3 w-3 animate-spin" />
          执行中
        </span>
      );
    case "succeeded":
      return (
        <span className="inline-flex items-center gap-1 rounded-full bg-[#10B981]/15 px-2 py-0.5 text-[11px] font-medium text-[#10B981]">
          <CheckCircle2 className="h-3 w-3" />
          已完成
        </span>
      );
    case "failed":
      return (
        <span className="inline-flex items-center gap-1 rounded-full bg-[#EF4444]/15 px-2 py-0.5 text-[11px] font-medium text-[#EF4444]">
          <XCircle className="h-3 w-3" />
          失败
        </span>
      );
    case "cancelled":
      return (
        <span className="inline-flex items-center gap-1 rounded-full bg-[#F59E0B]/15 px-2 py-0.5 text-[11px] font-medium text-[#F59E0B]">
          <AlertTriangle className="h-3 w-3" />
          已中止
        </span>
      );
  }
};
