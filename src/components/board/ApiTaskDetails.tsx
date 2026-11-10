import { useEffect, useState } from "react";
import { api, type ApiTask } from "@/lib/api";
import type { RunRecord } from "@/lib/types";

const statusNames: Record<string,string> = {running:"执行中",completed:"已完成",failed:"失败",paused:"预算暂停",cancelled:"已停止",interrupted:"已中断"};
const reviewNames = {accept:"验收通过",rework:"要求返工",takeover:"Supervisor 接管"};

export function ApiTaskDetails({ run, onSelectTask }: { run: RunRecord; onSelectTask: (task: ApiTask) => void }) {
  const [tasks, setTasks] = useState<ApiTask[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    setTasks([]); setError("");
    const refresh = async () => {
      try { const next = await api.taskHistory(run.id); if (!stopped) { setTasks(next); setError(""); } }
      catch (e) { if (!stopped) setError(String(e)); }
      if (!stopped && run.status === "running") timer = setTimeout(() => void refresh(), 1200);
    };
    void refresh();
    return () => { stopped = true; clearTimeout(timer); };
  }, [run.id, run.status]);
  if (error) return <p role="alert" className="text-xs text-[#FCA5A5]">任务历史读取失败：{error}</p>;
  const root = tasks.find(t => !t.parentId);
  if (!root) return <p className="text-xs text-[#8B949E]">正在读取 API 任务历史…</p>;
  const rounds = tasks.reduce((sum,t) => sum+t.round,0);
  const tools = tasks.reduce((sum,t) => sum+t.tools.length,0);
  const input = tasks.every(t => t.inputTokens != null) ? tasks.reduce((sum,t) => sum+t.inputTokens!,0) : "未知";
  const output = tasks.every(t => t.outputTokens != null) ? tasks.reduce((sum,t) => sum+t.outputTokens!,0) : "未知";
  return <details className="rounded-lg border border-[#262B34] bg-[#181B21] p-3 text-xs">
    <summary className="cursor-pointer text-[#C9D1D9]">{root.mode === "real" ? "真实 API" : "模拟模型"} · 整次运行 {rounds}/{root.limits.maxRounds} 轮 · {tools}/{root.limits.maxToolCalls} 工具 · {tasks.length-1} 个子任务 · 调用与审查历史</summary>
    <div className="mt-2 max-h-72 space-y-2 overflow-y-auto selectable-text">
      <p className="text-[11px] text-[#8B949E]">累计输入 {input} / 输出 {output} tokens · 委派上限 {root.limits.maxDelegations ?? 0} · 每项返工上限 {root.limits.maxReworks ?? 0}</p>
      {tasks.map(task => <details key={task.id} className={`rounded border p-2 ${task.parentId ? "ml-3 border-[#373E4B]" : "border-[#00E5FF]/30"}`}>
        <summary className="cursor-pointer">{task.settings.role === "supervisor" ? "Supervisor" : "Worker"} · {task.model.name} · {task.title || "任务"} · {statusNames[task.status] ?? task.status}{task.parentId ? ` · 尝试 ${task.attempt}` : ""}{task.review ? ` · ${reviewNames[task.review.decision]}` : task.parentId ? " · 待审查" : ""}</summary>
        <p className="mt-2 text-[11px] text-[#8B949E]">{task.model.apiModel} · 思考 {task.settings.thinking ? task.settings.effort ?? "默认" : "关闭"} · 输出上限 {task.settings.maxOutputTokens} · 本角色 {task.round} 轮 · 输入 {task.inputTokens ?? "未知"} / 输出 {task.outputTokens ?? "未知"}</p>
        {task.paths?.length > 0 && <p className="mt-1 break-all text-[11px] text-[#8B949E]">写入范围：{task.paths.join("、")} · 验收：{task.acceptance}</p>}
        {task.review && <p className="mt-2 text-[11px] text-[#93C5FD]">{reviewNames[task.review.decision]}：{task.review.feedback}</p>}
        {task.summary && <p className="mt-2 whitespace-pre-wrap text-[11px]">{task.summary}</p>}
        {!!task.changeset?.files.length && <button onClick={() => onSelectTask(task)} className="mt-2 text-[11px] text-[#00E5FF]">查看本任务 {task.changeset.files.length} 个文件的真实 Diff</button>}
        <p className="my-2 break-all text-[10px] text-[#6E7681]">Task {task.id} · 请求：{task.callIds.join(" · ") || "尚未发送"}</p>
        {task.tools.map((t,i) => <details key={`${t.callId}-${t.tool.id}-${i}`} className="mt-1 rounded border border-[#373E4B] p-2">
          <summary className={`cursor-pointer ${t.status === "failed" || t.status === "interrupted" ? "text-[#FCA5A5]" : "text-[#C9D1D9]"}`}>{i+1}. {t.tool.name} · {statusNames[t.status] ?? t.status} · {new Date(t.startedAt).toLocaleTimeString()}</summary>
          <pre className="mt-2 whitespace-pre-wrap break-all text-[11px] text-[#8B949E]">{JSON.stringify(t.tool.arguments,null,2)}</pre>
          <pre className="mt-2 whitespace-pre-wrap break-words text-[11px]">{t.output || "等待结果…"}</pre>
        </details>)}
      </details>)}
      {root.status === "interrupted" && <p className="text-[#F59E0B]">中断操作可能已修改文件。继续会新建运行并先核对文件，不重放旧工具。</p>}
    </div>
  </details>;
}
