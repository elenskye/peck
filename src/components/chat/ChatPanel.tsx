import React from "react";
import { History, MessageSquarePlus } from "lucide-react";
import { Composer } from "@/components/chat/Composer";
import { MessageList } from "@/components/chat/MessageList";
import type { AgentInfo, Message, RunRecord, Session } from "@/lib/types";
import { cn } from "@/lib/utils";
import type { ApiMode, ApiRole, RunLimits } from "@/lib/api";

interface ChatPanelProps {
  sessions: Session[];
  activeSession: Session | null;
  messages: Message[];
  runs: RunRecord[];
  activeRunId: string | null;
  agents: AgentInfo[];
  isRunning: boolean;
  apiMode: ApiMode | null;
  onSelectSession: (session: Session) => void;
  onNewSession: () => void;
  onSelectRun: (runId: string) => void;
  onSendPrompt: (prompt: string, limits?: RunLimits, scenario?: string, role?: ApiRole) => void;
  onCancelRun: () => void;
  onRetryStep: (runId: string, stepId: string) => void;
}

export const ChatPanel: React.FC<ChatPanelProps> = ({
  sessions,
  activeSession,
  messages,
  runs,
  activeRunId,
  agents,
  isRunning,
  apiMode,
  onSelectSession,
  onNewSession,
  onSelectRun,
  onSendPrompt,
  onCancelRun,
  onRetryStep,
}) => {
  return (
    <div className="flex h-full flex-col bg-[#14171C]">
      {/* Subheader: Session Switcher & New Session */}
      <div className="flex h-10 shrink-0 items-center justify-between border-b border-[#262B34] px-3.5">
        <div className="flex items-center gap-1.5 overflow-x-auto">
          <History className="h-3.5 w-3.5 shrink-0 text-[#8B949E]" />
          {sessions.map((sess) => {
            const active = sess.id === activeSession?.id;
            return (
              <button
                key={sess.id}
                disabled={isRunning}
                type="button"
                onClick={() => onSelectSession(sess)}
                className={cn(
                  "max-w-[165px] truncate rounded px-2 py-1 text-xs transition-colors",
                  active
                    ? "bg-[#1E222A] font-medium text-[#00E5FF]"
                    : "text-[#8B949E] hover:bg-[#181B21] hover:text-[#C9D1D9]",
                )}
              >
                {sess.title}
              </button>
            );
          })}
        </div>

        <button
          type="button"
          onClick={onNewSession}
          title="新建发令会话"
          disabled={isRunning}
          className="inline-flex shrink-0 items-center gap-1 rounded border border-[#262B34] bg-[#181B21] px-2 py-1 text-[11px] text-[#C9D1D9] transition-colors hover:border-[#00E5FF]/50 hover:text-[#00E5FF]"
        >
          <MessageSquarePlus className="h-3 w-3" />
          <span>新会话</span>
        </button>
      </div>

      {/* Conversation Message Stream */}
      <MessageList
        messages={messages}
        runs={runs}
        activeRunId={activeRunId}
        onSelectRun={onSelectRun}
        onQuickPrompt={onSendPrompt}
        onRetryStep={onRetryStep}
      />

      {/* Bottom Composer */}
      <Composer
        agents={agents}
        isRunning={isRunning}
        apiMode={apiMode}
        onSend={onSendPrompt}
        onCancel={onCancelRun}
      />
    </div>
  );
};
