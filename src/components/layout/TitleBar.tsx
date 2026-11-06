import React, { useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Cpu,
  FolderGit2,
  FolderOpen,
  GitBranch,
  Plus,
  Settings,
  Sparkles,
} from "lucide-react";
import { isMockMode } from "@/lib/ipc";
import type { AgentInfo, Workspace } from "@/lib/types";
import { cn, shortenHomePath } from "@/lib/utils";

interface TitleBarProps {
  workspaces: Workspace[];
  activeWorkspace: Workspace | null;
  agents: AgentInfo[];
  onSelectWorkspace: (path: string) => void;
  onOpenAgentsDialog: () => void;
  onChooseWorkspace: () => void;
  disabled: boolean;
}

export const TitleBar: React.FC<TitleBarProps> = ({
  workspaces,
  activeWorkspace,
  agents,
  onSelectWorkspace,
  onOpenAgentsDialog,
  onChooseWorkspace,
  disabled,
}) => {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [customPathInput, setCustomPathInput] = useState("");
  const [showCustomInput, setShowCustomInput] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const readyAgentsCount = agents.filter((a) => a.available && !a.isMock).length;
  const mockMode = isMockMode();

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(e.target as Node)
      ) {
        setDropdownOpen(false);
        setShowCustomInput(false);
      }
    };
    window.addEventListener("mousedown", handleClickOutside);
    return () => window.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const handleCustomSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!customPathInput.trim()) return;
    onSelectWorkspace(customPathInput.trim());
    setCustomPathInput("");
    setShowCustomInput(false);
    setDropdownOpen(false);
  };

  return (
    <header className="app-drag-region relative z-30 flex h-[46px] w-full shrink-0 items-center justify-between border-b border-[#262B34] bg-[#0F1115] px-4 select-none">
      {/* Left: Traffic Lights Reserve + Brand + Workspace Selector */}
      <div className="flex items-center gap-3">
        {/* Traffic lights placeholder (visible in browser mock mode, native overlay in Tauri) */}
        <div className="flex w-[64px] items-center gap-2 pl-0.5">
          {mockMode && (
            <>
              <span className="h-3 w-3 rounded-full bg-[#FF5F56]/85 ring-1 ring-black/20" />
              <span className="h-3 w-3 rounded-full bg-[#FFBD2E]/85 ring-1 ring-black/20" />
              <span className="h-3 w-3 rounded-full bg-[#27C93F]/85 ring-1 ring-black/20" />
            </>
          )}
        </div>

        {/* Peck Identity Pill */}
        <div className="flex items-center gap-1.5 rounded-md border border-[#262B34] bg-[#14171C] px-2 py-1">
          <Sparkles className="h-3.5 w-3.5 text-[#00E5FF]" />
          <span className="text-xs font-semibold tracking-wide text-[#E6EDF3]">
            PECK
          </span>
        </div>

        {/* Workspace Dropdown Trigger */}
        <div ref={dropdownRef} className="app-no-drag relative">
          <button
            type="button"
            disabled={disabled}
            onClick={() => setDropdownOpen((v) => !v)}
            className={cn(
              "group flex items-center gap-2 rounded-md border px-2.5 py-1 text-xs transition-all",
              dropdownOpen
                ? "border-[#00E5FF]/60 bg-[#1E222A] text-[#E6EDF3]"
                : "border-[#262B34] bg-[#14171C] text-[#C9D1D9] hover:border-[#373E4B] hover:bg-[#181B21]",
            )}
          >
            <FolderGit2 className="h-3.5 w-3.5 text-[#00E5FF]" />
            <span className="font-mono-code max-w-[280px] truncate text-[12px] text-[#E6EDF3]">
              {activeWorkspace
                ? shortenHomePath(activeWorkspace.path)
                : "打开项目文件夹"}
            </span>
            {activeWorkspace?.gitBranch && (
              <span className="inline-flex items-center gap-1 rounded bg-[#1E222A] px-1.5 py-0.5 text-[10px] text-[#8B949E]">
                <GitBranch className="h-2.5 w-2.5 text-[#00E5FF]" />
                {activeWorkspace.gitBranch}
              </span>
            )}
            <ChevronDown className="h-3.5 w-3.5 text-[#8B949E] transition-transform group-hover:text-[#E6EDF3]" />
          </button>

          {dropdownOpen && (
            <div className="absolute left-0 top-[calc(100%+6px)] w-[380px] rounded-lg border border-[#262B34] bg-[#1E222A] p-1.5 shadow-2xl shadow-black/70">
              <div className="px-2.5 py-1.5 text-[11px] font-medium text-[#8B949E] flex items-center justify-between">
                <span>最近打开的工作区</span>
                <span className="text-[10px] text-[#10B981]">
                  {activeWorkspace?.gitignoreConfigured ? ".peck/ 已加入 .gitignore" : "选择一个本地文件夹"}
                </span>
              </div>
              <div className="max-h-[220px] space-y-0.5 overflow-y-auto">
                {workspaces.map((ws) => {
                  const isCurrent = ws.id === activeWorkspace?.id;
                  return (
                    <button
                      key={ws.id}
                      type="button"
                      onClick={() => {
                        onSelectWorkspace(ws.path);
                        setDropdownOpen(false);
                      }}
                      className={cn(
                        "flex w-full items-center justify-between rounded-md px-2.5 py-2 text-left text-xs transition-colors",
                        isCurrent
                          ? "bg-[#00E5FF]/12 text-[#E6EDF3]"
                          : "text-[#C9D1D9] hover:bg-[#181B21]",
                      )}
                    >
                      <div className="min-w-0 flex-1 pr-2">
                        <div className="flex items-center gap-1.5 font-medium">
                          <FolderOpen className="h-3.5 w-3.5 shrink-0 text-[#00E5FF]" />
                          <span className="truncate">{ws.name}</span>
                          {ws.gitBranch && (
                            <span className="rounded bg-[#14171C] px-1.5 py-0.5 font-mono-code text-[10px] text-[#8B949E]">
                              {ws.gitBranch}
                            </span>
                          )}
                        </div>
                        <div className="mt-0.5 truncate font-mono-code text-[11px] text-[#8B949E]">
                          {shortenHomePath(ws.path)}
                        </div>
                      </div>
                      {isCurrent && (
                        <Check className="h-3.5 w-3.5 shrink-0 text-[#00E5FF]" />
                      )}
                    </button>
                  );
                })}
              </div>

              <div className="my-1.5 border-t border-[#262B34]" />
              {!mockMode && <button type="button" onClick={() => { setDropdownOpen(false); onChooseWorkspace(); }} className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-xs text-[#00E5FF] hover:bg-[#181B21]"><FolderOpen className="h-3.5 w-3.5" />选择项目文件夹...</button>}

              {!showCustomInput ? (
                <button
                  type="button"
                  onClick={() => setShowCustomInput(true)}
                  className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-xs text-[#00E5FF] hover:bg-[#181B21]"
                >
                  <Plus className="h-3.5 w-3.5" />
                  <span>输入路径打开其他本地工作区...</span>
                </button>
              ) : (
                <form onSubmit={handleCustomSubmit} className="p-1">
                  <div className="flex items-center gap-1.5">
                    <input
                      type="text"
                      value={customPathInput}
                      onChange={(e) => setCustomPathInput(e.target.value)}
                      placeholder="~/Documents/my-project"
                      autoFocus
                      className="flex-1 rounded border border-[#262B34] bg-[#14171C] px-2 py-1 font-mono-code text-xs text-[#E6EDF3] placeholder-[#6E7681] focus:border-[#00E5FF] focus:outline-none"
                    />
                    <button
                      type="submit"
                      className="rounded bg-[#00E5FF] px-2.5 py-1 text-xs font-medium text-[#0F1115] hover:bg-[#33EBFF]"
                    >
                      打开
                    </button>
                  </div>
                </form>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Right: Mock Mode Indicator + Agent Status Pill + Settings */}
      <div className="app-no-drag flex items-center gap-2">
        {mockMode && (
          <span
            title="当前处于浏览器 ?mock 高仿真演示模式"
            className="inline-flex items-center gap-1.5 rounded-full border border-[#F59E0B]/40 bg-[#F59E0B]/10 px-2.5 py-0.5 text-[11px] font-medium text-[#F59E0B]"
          >
            <span className="h-1.5 w-1.5 rounded-full bg-[#F59E0B]" />
            ?mock 预览模式
          </span>
        )}

        <button
          type="button"
          onClick={onOpenAgentsDialog}
          className="flex items-center gap-2 rounded-md border border-[#262B34] bg-[#14171C] px-2.5 py-1 text-xs text-[#C9D1D9] transition-colors hover:border-[#00E5FF]/50 hover:bg-[#181B21] hover:text-[#E6EDF3]"
        >
          <span className="relative flex h-2 w-2">
            <span className={cn("relative inline-flex h-2 w-2 rounded-full", readyAgentsCount ? "bg-[#F59E0B]" : "bg-[#3B82F6]")} />
          </span>
          <Cpu className="h-3.5 w-3.5 text-[#00E5FF]" />
          <span>{mockMode ? "演示 Agent" : readyAgentsCount ? `${readyAgentsCount} 个 CLI 已检测` : "模拟 Agent 可用"}</span>
        </button>

        <button
          type="button"
          onClick={onOpenAgentsDialog}
          title="Agent 清单与运行设置"
          className="flex items-center gap-1.5 rounded-md border border-[#262B34] bg-[#14171C] px-2.5 py-1 text-xs text-[#8B949E] transition-colors hover:border-[#373E4B] hover:bg-[#181B21] hover:text-[#E6EDF3]"
        >
          <Settings className="h-3.5 w-3.5" />
          <span>设置</span>
        </button>
      </div>
    </header>
  );
};
