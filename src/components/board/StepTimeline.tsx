import React from "react";
import {
  CheckCircle2,
  Circle,
  Clock,
  Loader2,
  RotateCcw,
  XCircle,
} from "lucide-react";
import type { StepDescriptor } from "@/lib/types";
import { cn, formatDuration } from "@/lib/utils";

interface StepTimelineProps {
  steps: StepDescriptor[];
  canRetry: boolean;
  selectedStepId: string | null;
  onSelectStep: (stepId: string | null) => void;
  onRetryStep: (stepId: string) => void;
}

export const StepTimeline: React.FC<StepTimelineProps> = ({
  steps,
  canRetry,
  selectedStepId,
  onSelectStep,
  onRetryStep,
}) => {
  return (
    <div className="rounded-lg border border-[#262B34] bg-[#181B21] p-3">
      <div className="mb-2.5 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold tracking-wide text-[#E6EDF3]">
            步骤时间线 (StepTimeline)
          </span>
          <span className="rounded bg-[#1E222A] px-1.5 py-0.5 font-mono-code text-[10px] text-[#8B949E]">
            {steps.filter((s) => s.status === "succeeded").length}/{steps.length} 完成
          </span>
        </div>

        {selectedStepId && (
          <button
            type="button"
            onClick={() => onSelectStep(null)}
            className="text-[11px] text-[#00E5FF] hover:underline"
          >
            显示全部步骤改动
          </button>
        )}
      </div>

      <div className={`grid grid-cols-1 gap-2 ${steps.length > 1 ? "md:grid-cols-2" : ""}`}>
        {steps.map((step) => {
          const isSelected = selectedStepId === step.id;
          return (
            <div
              key={step.id}
              onClick={() =>
                onSelectStep(isSelected ? null : step.id)
              }
              className={cn(
                "cursor-pointer rounded-md border p-2.5 transition-all",
                isSelected
                  ? "border-[#00E5FF] bg-[#00E5FF]/10"
                  : "border-[#262B34] bg-[#14171C] hover:border-[#373E4B]",
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 min-w-0">
                  <StepStatusIcon status={step.status} />
                  <span className="font-mono-code text-[11px] font-semibold text-[#00E5FF]">
                    Step {step.stepIndex + 1}
                  </span>
                  <span className="truncate text-xs font-medium text-[#E6EDF3]">
                    {step.agentName} · {step.title}
                  </span>
                </div>

                <div className="flex shrink-0 items-center gap-1.5">
                  {step.durationMs != null && (
                    <span className="inline-flex items-center gap-1 font-mono-code text-[11px] text-[#8B949E]">
                      <Clock className="h-3 w-3" />
                      {formatDuration(step.durationMs)}
                    </span>
                  )}
                  {canRetry && (step.status === "failed" || step.status === "cancelled") && (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onRetryStep(step.id);
                      }}
                      className="inline-flex items-center gap-1 rounded bg-[#EF4444]/20 px-1.5 py-0.5 text-[10px] font-medium text-[#EF4444] hover:bg-[#EF4444]/30"
                    >
                      <RotateCcw className="h-2.5 w-2.5" />
                      {step.agentId.startsWith("api-") ? "从当前文件继续" : "重试"}
                    </button>
                  )}
                </div>
              </div>

              {step.notice && <p className={`mt-2 rounded px-2 py-1.5 text-[11px] leading-relaxed ${step.executionMode === "mock" || step.executionMode === "api-mock" ? "bg-[#F59E0B]/10 text-[#F59E0B]" : step.status === "failed" ? "bg-[#EF4444]/10 text-[#FCA5A5]" : "bg-[#3B82F6]/10 text-[#93C5FD]"}`}>{step.notice}</p>}

              {step.expects.length > 0 && (
                <div className="mt-1.5 flex flex-wrap items-center gap-1 text-[10px] text-[#8B949E]">
                  <span>期待产物:</span>
                  {step.expects.map((exp) => (
                    <code
                      key={exp}
                      className="font-mono-code rounded bg-[#181B21] px-1.5 py-0.5 text-[#C9D1D9]"
                    >
                      {exp}
                    </code>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};

const StepStatusIcon: React.FC<{ status: StepDescriptor["status"] }> = ({
  status,
}) => {
  switch (status) {
    case "succeeded":
      return <CheckCircle2 className="h-4 w-4 shrink-0 text-[#10B981]" />;
    case "running":
      return (
        <Loader2 className="h-4 w-4 shrink-0 animate-spin text-[#3B82F6]" />
      );
    case "failed":
      return <XCircle className="h-4 w-4 shrink-0 text-[#EF4444]" />;
    case "cancelled":
      return <XCircle className="h-4 w-4 shrink-0 text-[#F59E0B]" />;
    default:
      return <Circle className="h-4 w-4 shrink-0 text-[#6E7681]" />;
  }
};
