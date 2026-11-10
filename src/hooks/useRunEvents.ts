import { useCallback, useEffect, useState } from "react";
import { ipc } from "@/lib/ipc";
import type { RunEvent, RunRecord } from "@/lib/types";

export function useRunEvents(sessionId?: string, initialPrompt?: string) {
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [error, setError] = useState("");

  const reloadRuns = useCallback(async () => {
    if (!sessionId) return;
    const loaded = await ipc.sessionGetRuns(sessionId);
    setRuns(loaded);
    setError("");
  }, [sessionId]);

  useEffect(() => {
    let cancelled = false;
    setRuns([]);
    setActiveRunId(null);
    setError("");
    if (!sessionId) return;
    void (async () => {
      const loaded = await ipc.sessionGetRuns(sessionId);
      if (cancelled) return;
      setRuns(loaded);
      if (loaded.length > 0) {
        setActiveRunId(loaded[loaded.length - 1].id);
      } else {
        setActiveRunId(null);
      }
    })().catch((e) => { if (!cancelled) setError(String(e)); });
    return () => { cancelled = true; };
  }, [sessionId]);

  const handleRunEvent = useCallback(
    (promptText: string, event: RunEvent) => {
      if (event.type === "RunStarted") setActiveRunId(event.payload.runId);
      setRuns((prev) => {
        const next = [...prev];

        if (event.type === "RunStarted") {
          const { runId, flowId, flowName, steps } = event.payload;
          const numMatch = runId.match(/\d+/);
          const number = numMatch ? Number(numMatch[0]) : 100 + next.length + 1;
          const record: RunRecord = {
            id: runId,
            number,
            sessionId: sessionId ?? "sess-default",
            flowId,
            flowName,
            prompt: promptText || initialPrompt || "",
            status: "running",
            startedAt: Date.now(),
            steps,
            tasks: [],
            files: [],
            logs: [],
          };
          return [...next.filter((r) => r.id !== runId), record];
        }

        const idx = next.findIndex((r) => r.id === event.payload.runId);
        if (idx === -1) return prev;
        const current = { ...next[idx] };

        switch (event.type) {
          case "StepAdded": { current.steps = [...current.steps, event.payload.step]; break; }
          case "StepExecution": {
            current.steps = current.steps.map((s) => s.id === event.payload.stepId
              ? { ...s, executionMode: event.payload.mode, notice: event.payload.notice } : s);
            break;
          }
          case "StepStarted": {
            current.status = "running";
            current.steps = current.steps.map((s) =>
              s.id === event.payload.stepId
                ? {
                    ...s,
                    status: "running",
                    startedAt: Date.now(),
                    exitCode: undefined,
                    finishedAt: undefined,
                    durationMs: undefined,
                  }
                : s,
            );
            break;
          }
          case "StepOutput": {
            const step = current.steps.find((s) => s.id === event.payload.stepId);
            current.logs = [
              ...current.logs,
              {
                id: `log-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
                runId: event.payload.runId,
                stepId: event.payload.stepId,
                agentId: step?.agentId ?? "cli",
                stream: event.payload.stream,
                chunk: event.payload.chunk,
                timestamp: Date.now(),
              },
            ];
            break;
          }
          case "TasksUpdated": {
            current.tasks = event.payload.items;
            break;
          }
          case "FileTouched": {
            const existingIdx = current.files.findIndex(
              (f) => f.path === event.payload.path,
            );
            if (existingIdx >= 0) {
              const updatedFiles = [...current.files];
              updatedFiles[existingIdx] = {
                ...updatedFiles[existingIdx],
                kind: event.payload.kind,
                stepId: event.payload.stepId,
              };
              current.files = updatedFiles;
            } else {
              current.files = [
                ...current.files,
                {
                  path: event.payload.path,
                  kind: event.payload.kind,
                  additions: 0,
                  deletions: 0,
                  stepId: event.payload.stepId,
                },
              ];
            }
            break;
          }
          case "StepFinished": {
            current.stepFiles = { ...current.stepFiles, [event.payload.stepId]: event.payload.changeset.files };
            current.steps = current.steps.map((s) =>
              s.id === event.payload.stepId
                ? {
                    ...s,
                    status: event.payload.status,
                    exitCode: event.payload.exitCode,
                    durationMs: event.payload.durationMs,
                    finishedAt: Date.now(),
                  }
                : s,
            );
            if (event.payload.changeset.files.length > 0) {
              const merged = [...current.files];
              for (const f of event.payload.changeset.files) {
                const fIdx = merged.findIndex((m) => m.path === f.path);
                if (fIdx >= 0) {
                  merged[fIdx] = f;
                } else {
                  merged.push(f);
                }
              }
              current.files = merged;
            }
            break;
          }
          case "RunFinished": {
            current.status = event.payload.status;
            current.summary = event.payload.summary;
            current.finishedAt = Date.now();
            break;
          }
        }

        next[idx] = current;
        return next;
      });
    },
    [sessionId, initialPrompt],
  );

  const activeRun = runs.find((r) => r.id === activeRunId) ?? runs[runs.length - 1] ?? null;

  return {
    runs,
    activeRun,
    activeRunId,
    setActiveRunId,
    handleRunEvent,
    reloadRuns,
    error,
  };
}
