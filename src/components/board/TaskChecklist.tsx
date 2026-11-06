import React, { useState } from "react";
import { CheckSquare, ChevronDown, ChevronRight, ListTodo, Square } from "lucide-react";
import type { TaskItem } from "@/lib/types";
import { cn } from "@/lib/utils";

interface TaskChecklistProps {
  tasks: TaskItem[];
}

export const TaskChecklist: React.FC<TaskChecklistProps> = ({ tasks }) => {
  const [expanded, setExpanded] = useState(true);

  if (tasks.length === 0) return null;

  const completed = tasks.filter((t) => t.completed).length;
  const total = tasks.length;
  const percent = Math.round((completed / total) * 100);

  return (
    <div className="rounded-lg border border-[#262B34] bg-[#181B21] p-3">
      <div
        onClick={() => setExpanded((v) => !v)}
        className="flex cursor-pointer items-center justify-between"
      >
        <div className="flex items-center gap-2">
          {expanded ? (
            <ChevronDown className="h-3.5 w-3.5 text-[#8B949E]" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 text-[#8B949E]" />
          )}
          <ListTodo className="h-3.5 w-3.5 text-[#00E5FF]" />
          <span className="text-xs font-semibold text-[#E6EDF3]">
            任务清单进度 (.peck/tasks.md)
          </span>
        </div>

        <div className="flex items-center gap-2.5">
          <div className="h-1.5 w-24 overflow-hidden rounded-full bg-[#0F1115]">
            <div
              style={{ width: `${percent}%` }}
              className="h-full rounded-full bg-[#10B981] transition-all duration-300"
            />
          </div>
          <span className="font-mono-code text-[11px] font-medium text-[#10B981]">
            {completed}/{total} ({percent}%)
          </span>
        </div>
      </div>

      {expanded && (
        <ul className="selectable-text mt-2.5 grid grid-cols-1 gap-1.5 border-t border-[#262B34]/70 pt-2.5">
          {tasks.map((task) => (
            <li
              key={task.id}
              className={cn(
                "flex items-start gap-2 rounded px-2 py-1 text-xs",
                task.completed
                  ? "bg-[#14171C]/60 text-[#8B949E]"
                  : "bg-[#14171C] text-[#E6EDF3]",
              )}
            >
              {task.completed ? (
                <CheckSquare className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[#10B981]" />
              ) : (
                <Square className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[#6E7681]" />
              )}
              <span className={cn(task.completed && "line-through opacity-85")}>
                {task.text}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};
