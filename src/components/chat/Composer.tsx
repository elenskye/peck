import React, { useRef, useState } from "react";
import { AtSign, CornerDownLeft, Square, Workflow } from "lucide-react";
import type { AgentInfo } from "@/lib/types";
import { cn } from "@/lib/utils";

interface ComposerProps {
  agents: AgentInfo[];
  isRunning: boolean;
  onSend: (prompt: string) => void;
  onCancel: () => void;
}

export const Composer: React.FC<ComposerProps> = ({
  agents,
  isRunning,
  onSend,
  onCancel,
}) => {
  const [text, setText] = useState("");
  const [showAgentMenu, setShowAgentMenu] = useState(false);
  const [target, setTarget] = useState("");
  const externalAgents = agents.filter((a) => a.available && !a.isMock);
  const effectiveTarget = target || (externalAgents.length > 1 ? "flow" : externalAgents[0]?.id ?? "mock");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleSubmit = () => {
    const trimmed = text.trim();
    if (!trimmed || isRunning) return;
    onSend(trimmed.startsWith("@") || effectiveTarget === "flow" ? trimmed : `@${effectiveTarget} ${trimmed}`);
    setText("");
    setShowAgentMenu(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      handleSubmit();
    }
  };

  const insertAgentPrefix = (agentId: string) => {
    const cleaned = text.replace(/^@[a-z0-9_-]+\s*/i, "");
    setText(`@${agentId} ${cleaned}`);
    setShowAgentMenu(false);
    textareaRef.current?.focus();
  };

  const singleStepMatch = text.trim().match(/^@[a-z0-9_-]+\b/i);
  const currentModeLabel = singleStepMatch
    ? `单步直达: ${singleStepMatch[0].toLowerCase()}`
    : effectiveTarget === "flow" ? "流水线: Muse → Grok" : `单 Agent: @${effectiveTarget}`;

  return (
    <div className="border-t border-[#262B34] bg-[#14171C] p-3">
      <div className="mb-2 flex items-center gap-2 text-[11px] text-[#8B949E]">
        <label htmlFor="execution-target">运行方式</label>
        <select id="execution-target" aria-label="运行方式" disabled={isRunning} value={effectiveTarget} onChange={(e) => { setTarget(e.target.value); setText((s) => s.replace(/^@[a-z0-9_-]+\s*/i, "")); }} className="min-w-0 flex-1 rounded border border-[#262B34] bg-[#181B21] px-2 py-1 text-[#E6EDF3]">
          <option value="mock">模拟 Agent（无需订阅）</option>
          {agents.filter((a) => !a.isMock).map((a) => <option key={a.id} value={a.id} disabled={!a.available}>{a.name} 单 Agent{!a.available ? "（未安装）" : ""}</option>)}
          <option value="flow">Muse → Grok 流水线（缺失步骤将模拟）</option>
        </select>
      </div>
      <p className="mb-2 text-[11px] leading-relaxed text-[#8B949E]">一个可用的 CLI 即可独立完成任务。模拟运行不调用 AI；已安装 CLI 的认证或额度错误会暂停任务。</p>
      {/* Mode & Quick Agent Selector Row */}
      <div className="mb-2 flex items-center justify-between text-[11px]">
        <div className="inline-flex items-center gap-1.5 text-[#8B949E]">
          <Workflow className="h-3.5 w-3.5 text-[#00E5FF]" />
          <span>{currentModeLabel}</span>
        </div>

        <div className="relative flex items-center gap-1">
          {["muse", "grok", "mock"].map((id) => (
            <button
              key={id}
              type="button"
              onClick={() => insertAgentPrefix(id)}
              className="font-mono-code rounded border border-[#262B34] bg-[#181B21] px-1.5 py-0.5 text-[10px] text-[#8B949E] transition-colors hover:border-[#00E5FF]/50 hover:text-[#00E5FF]"
            >
              @{id}
            </button>
          ))}
          <button
            type="button"
            onClick={() => setShowAgentMenu((v) => !v)}
            title="选择单步 Agent"
            className="rounded border border-[#262B34] bg-[#181B21] p-1 text-[#8B949E] hover:border-[#00E5FF]/50 hover:text-[#E6EDF3]"
          >
            <AtSign className="h-3 w-3" />
          </button>

          {showAgentMenu && (
            <div className="absolute bottom-[calc(100%+6px)] right-0 z-30 w-60 rounded-lg border border-[#262B34] bg-[#1E222A] p-1.5 shadow-xl shadow-black/70">
              <div className="px-2 py-1 text-[10px] font-medium text-[#8B949E]">
                指定单步 Agent（跳过默认双步编排）
              </div>
              {agents.map((ag) => (
                <button
                  key={ag.id}
                  type="button"
                  onClick={() => insertAgentPrefix(ag.id)}
                  className="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-xs text-[#C9D1D9] hover:bg-[#181B21]"
                >
                  <div>
                    <span className="font-mono-code font-semibold text-[#00E5FF]">
                      @{ag.id}
                    </span>
                    <span className="ml-1.5 text-[#E6EDF3]">{ag.name}</span>
                  </div>
                  <span className={`text-[10px] ${ag.available ? "text-[#F59E0B]" : "text-[#FCA5A5]"}`}>{ag.isMock ? "模拟" : ag.available ? "CLI 已检测" : "未安装 · 将模拟"}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Textarea Box */}
      <div className="rounded-lg border border-[#262B34] bg-[#181B21] p-2.5 transition-colors focus-within:border-[#00E5FF]/60">
        <textarea
          ref={textareaRef}
          rows={3}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="描述任务，使用所选 Agent 执行；也可输入 @grok 或 @muse"
          className="selectable-text w-full resize-none bg-transparent text-[13px] leading-relaxed text-[#E6EDF3] placeholder-[#6E7681] focus:outline-none"
        />

        <div className="mt-2 flex items-center justify-between border-t border-[#262B34]/70 pt-2">
          <span className="font-mono-code text-[11px] text-[#6E7681]">
            ⌘ + ↩ 发送发令
          </span>

          {isRunning ? (
            <button
              type="button"
              onClick={onCancel}
              className="inline-flex items-center gap-1.5 rounded-md bg-[#EF4444]/20 border border-[#EF4444]/50 px-3 py-1 text-xs font-medium text-[#EF4444] transition-colors hover:bg-[#EF4444]/30"
            >
              <Square className="h-3 w-3 fill-current" />
              停止运行
            </button>
          ) : (
            <button
              type="button"
              disabled={!text.trim()}
              onClick={handleSubmit}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-semibold transition-all",
                text.trim()
                  ? "bg-[#00E5FF] text-[#0F1115] hover:bg-[#33EBFF] shadow-sm shadow-[#00E5FF]/20"
                  : "cursor-not-allowed bg-[#1E222A] text-[#6E7681]",
              )}
            >
              <span>发令执行</span>
              <CornerDownLeft className="h-3 w-3" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
