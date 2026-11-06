import React, { useEffect, useRef } from "react";
import { Bot, Terminal, User } from "lucide-react";
import { RunCard } from "@/components/chat/RunCard";
import type { Message, RunRecord } from "@/lib/types";
import { formatTimeShort } from "@/lib/utils";

interface MessageListProps {
  messages: Message[];
  runs: RunRecord[];
  activeRunId: string | null;
  onSelectRun: (runId: string) => void;
  onQuickPrompt: (prompt: string) => void;
  onRetryStep?: (runId: string, stepId: string) => void;
}

export const MessageList: React.FC<MessageListProps> = ({
  messages,
  runs,
  activeRunId,
  onSelectRun,
  onQuickPrompt,
  onRetryStep,
}) => {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, runs]);

  if (messages.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
        <div className="flex h-11 w-11 items-center justify-center rounded-xl border border-[#262B34] bg-[#181B21] text-[#00E5FF]">
          <Terminal className="h-5 w-5" />
        </div>
        <h3 className="mt-3 text-sm font-semibold text-[#E6EDF3]">
          自然语言发令枪已就绪
        </h3>
        <p className="mt-1.5 max-w-[300px] text-xs leading-relaxed text-[#8B949E]">
          在下方选择运行方式并描述需求；一个 Agent 就能独立执行，也可使用{" "}
          <code className="font-mono-code rounded bg-[#181B21] px-1 py-0.5 text-[#00E5FF]">
            @grok
          </code>{" "}
          /{" "}
          <code className="font-mono-code rounded bg-[#181B21] px-1 py-0.5 text-[#00E5FF]">
            @muse
          </code>{" "}
          指定单步执行。
        </p>

        <div className="mt-4 flex flex-col gap-1.5 w-full max-w-[320px]">
          {[
            "@mock 体验任务进度与文件看板（不调用 AI）",
            "@muse 梳理当前项目的 API 鉴权升级方案",
            "@grok 为 Captcha 组件补充键盘无障碍支持",
          ].map((sample) => (
            <button
              key={sample}
              type="button"
              onClick={() => onQuickPrompt(sample)}
              className="rounded-md border border-[#262B34] bg-[#181B21]/70 px-3 py-2 text-left text-xs text-[#C9D1D9] transition-colors hover:border-[#00E5FF]/50 hover:bg-[#181B21] hover:text-[#E6EDF3]"
            >
              {sample}
            </button>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="selectable-text flex-1 space-y-4 overflow-y-auto px-4 py-4">
      {messages.map((msg) => {
        const isUser = msg.role === "user";
        const linkedRun = msg.runId
          ? runs.find((r) => r.id === msg.runId)
          : undefined;

        return (
          <div key={msg.id} className="space-y-1.5">
            <div className="flex items-center justify-between text-[11px] text-[#8B949E]">
              <span className="inline-flex items-center gap-1.5 font-medium text-[#C9D1D9]">
                {isUser ? (
                  <>
                    <User className="h-3.5 w-3.5 text-[#8B949E]" />
                    <span>用户指令</span>
                  </>
                ) : (
                  <>
                    <Bot className="h-3.5 w-3.5 text-[#00E5FF]" />
                    <span className="text-[#00E5FF]">Peck 调度器</span>
                  </>
                )}
              </span>
              <span className="font-mono-code text-[10px]">
                {formatTimeShort(msg.createdAt)}
              </span>
            </div>

            <div
              className={
                isUser
                  ? "rounded-lg border border-[#262B34] bg-[#181B21] px-3.5 py-2.5 text-[13px] leading-relaxed text-[#E6EDF3]"
                  : "rounded-lg border border-[#262B34]/60 bg-[#0F1115]/60 px-3.5 py-2.5 text-[12.5px] leading-relaxed text-[#C9D1D9]"
              }
            >
              <p className="whitespace-pre-wrap break-words">{msg.content}</p>
              {linkedRun && (
                <RunCard
                  run={linkedRun}
                  isSelected={linkedRun.id === activeRunId}
                  onSelectRun={onSelectRun}
                  onRetryStep={onRetryStep}
                />
              )}
            </div>
          </div>
        );
      })}
      <div ref={bottomRef} />
    </div>
  );
};
