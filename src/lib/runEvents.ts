// 把任务跑的时候推来的事件并进记录里（和后端存进数据库的是同一套规则）。
import type { RunEvent, RunRecord } from "@/lib/types";

/** `sent` 是发任务时就知道、事件里不带的那几样（谁来干、权限档）。 */
export function applyRunEvent(runs: RunRecord[], sessionId: string, prompt: string, event: RunEvent, sent?: Pick<RunRecord, "who" | "permission">): RunRecord[] {
  if (event.type === "RunStarted") {
    const { runId, flowId, flowName, steps } = event.payload;
    const existing = runs.find((r) => r.id === runId);
    const record: RunRecord = {
      id: runId,
      number: Number(runId.match(/\d+/)?.[0] ?? 0),
      sessionId,
      flowId,
      flowName,
      prompt,
      startedAt: Date.now(),
      tasks: [],
      files: [],
      logs: [],
      ...sent,
      ...existing,
      status: "running",
      steps,
    };
    return [...runs.filter((r) => r.id !== runId), record];
  }
  const index = runs.findIndex((r) => r.id === event.payload.runId);
  if (index === -1) return runs;
  const current: RunRecord = { ...runs[index] };
  switch (event.type) {
    case "StepAdded":
      current.steps = [...current.steps, event.payload.step];
      break;
    case "StepExecution":
      current.steps = current.steps.map((s) => (s.id === event.payload.stepId ? { ...s, executionMode: event.payload.mode, notice: event.payload.notice } : s));
      break;
    case "StepStarted":
      current.status = "running";
      current.finishedAt = undefined;
      current.summary = undefined;
      current.undoneAt = undefined;
      current.steps = current.steps.map((s) =>
        s.id === event.payload.stepId ? { ...s, status: "running", startedAt: Date.now(), exitCode: undefined, finishedAt: undefined, durationMs: undefined } : s,
      );
      break;
    case "StepOutput": {
      const step = current.steps.find((s) => s.id === event.payload.stepId);
      current.logs = [
        ...current.logs,
        { id: `log-${current.logs.length}-${Date.now()}`, runId: current.id, stepId: event.payload.stepId, agentId: step?.agentId ?? "cli", stream: event.payload.stream, chunk: event.payload.chunk, timestamp: Date.now() },
      ];
      break;
    }
    case "TasksUpdated":
      current.tasks = event.payload.items;
      break;
    case "FileTouched": {
      const { path, kind, stepId } = event.payload;
      current.files = current.files.some((f) => f.path === path)
        ? current.files.map((f) => (f.path === path ? { ...f, kind, stepId } : f))
        : [...current.files, { path, kind, additions: 0, deletions: 0, stepId }];
      break;
    }
    case "StepFinished": {
      const { stepId, status, exitCode, durationMs, changeset } = event.payload;
      current.stepFiles = { ...current.stepFiles, [stepId]: changeset.files };
      current.steps = current.steps.map((s) => (s.id === stepId ? { ...s, status, exitCode, durationMs, finishedAt: Date.now() } : s));
      const merged = [...current.files];
      for (const file of changeset.files) {
        const at = merged.findIndex((m) => m.path === file.path);
        if (at >= 0) merged[at] = file;
        else merged.push(file);
      }
      current.files = merged;
      break;
    }
    case "RunFinished":
      current.status = event.payload.status;
      current.summary = event.payload.summary;
      current.finishedAt = Date.now();
      if (event.payload.status === "cancelled") {
        current.steps = current.steps.map((s) => (s.status === "running" || s.status === "pending" ? { ...s, status: "cancelled" } : s));
      }
      break;
  }
  const next = [...runs];
  next[index] = current;
  return next;
}
