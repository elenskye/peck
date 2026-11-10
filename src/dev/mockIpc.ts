// 浏览器 `?mock` 里替代 Rust IPC：数据来自 mockData.ts，只在内存，刷新重置。
import {
  MOCK_AGENTS,
  MOCK_API_TASKS,
  MOCK_DIFFS,
  MOCK_MESSAGES,
  MOCK_RUNS,
  MOCK_SESSIONS,
  MOCK_WORKSPACES,
} from "@/dev/mockData";
import type { ApiTask } from "@/lib/api";
import type {
  AgentInfo,
  DiffResult,
  Message,
  RunEvent,
  RunRecord,
  Session,
  StepDescriptor,
  TaskItem,
  Workspace,
} from "@/lib/types";

const DEMO_NOTICE = "浏览器演示数据，没有调用真实 AI。";
const emptyPreview = new URLSearchParams(window.location.search).has("empty");
let workspacesState: Workspace[] = emptyPreview ? [] : [...MOCK_WORKSPACES];
let sessionsState: Session[] = [...MOCK_SESSIONS];
const messagesState: Record<string, Message[]> = structuredClone(MOCK_MESSAGES);
const runsState: Record<string, RunRecord[]> = structuredClone(MOCK_RUNS);
// 键是 `${runId}:${path}`；新发的任务也按这个键存。
const diffsState: Record<string, DiffResult> = structuredClone(MOCK_DIFFS);
const apiTasksState: Record<string, ApiTask[]> = structuredClone(MOCK_API_TASKS);
let runCounter = 13;

const activeTimers = new Map<string, number[]>();
const activeCallbacks = new Map<string, (event: RunEvent) => void>();
const configState: Record<string, string> = {};

function clearRunTimers(runId: string) {
  const timers = activeTimers.get(runId);
  if (timers) {
    timers.forEach((id) => window.clearTimeout(id));
    activeTimers.delete(runId);
  }
}

export const mockIpc = {
  async workspaceOpen(path: string): Promise<Workspace> {
    const existing = workspacesState.find((w) => w.path === path);
    if (existing) {
      existing.lastOpenedAt = Date.now();
      return { ...existing };
    }
    const name = path.split("/").filter(Boolean).pop() || "workspace";
    const created: Workspace = {
      id: `ws-${Date.now()}`,
      path,
      name,
      lastOpenedAt: Date.now(),
      gitBranch: "main",
      gitignoreConfigured: true,
    };
    workspacesState = [created, ...workspacesState];
    return created;
  },

  async workspaceListRecent(): Promise<Workspace[]> {
    return [...workspacesState].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  },

  async sessionCreate(workspaceId: string, title?: string): Promise<Session> {
    const session: Session = {
      id: `sess-${Date.now()}`,
      workspaceId,
      title: title || "新任务",
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    sessionsState = [session, ...sessionsState];
    messagesState[session.id] = [];
    runsState[session.id] = [];
    return session;
  },

  async sessionList(workspaceId: string): Promise<Session[]> {
    return sessionsState
      .filter((s) => s.workspaceId === workspaceId)
      .sort((a, b) => b.updatedAt - a.updatedAt);
  },

  async sessionGetMessages(sessionId: string): Promise<Message[]> {
    return [...(messagesState[sessionId] ?? [])];
  },

  async sessionGetRuns(sessionId: string): Promise<RunRecord[]> {
    return [...(runsState[sessionId] ?? [])];
  },

  async runDispatch(
    sessionId: string,
    prompt: string,
    listener: (event: RunEvent) => void,
  ): Promise<string> {
    const runNum = runCounter++;
    const runId = `run-${runNum}`;

    // Parse optional @agent prefix
    const trimmed = prompt.trim();
    const singleAgentMatch = trimmed.match(/^@(muse|grok|mock)\b\s*(.*)$/i);
    const singleAgentId = singleAgentMatch ? singleAgentMatch[1].toLowerCase() : null;
    const cleanPrompt = singleAgentMatch ? singleAgentMatch[2] || trimmed : trimmed;

    const isSingleStep = Boolean(singleAgentId);
    const singleAgentName =
      MOCK_AGENTS.find((a) => a.id === singleAgentId)?.name ?? singleAgentId ?? "";
    const flowId = isSingleStep ? `single-${singleAgentId}` : "muse-grok";
    const flowName = isSingleStep ? singleAgentName : "Muse → Grok";

    const steps: StepDescriptor[] = isSingleStep
      ? [
          {
            id: `${runId}-step-1`,
            stepIndex: 0,
            agentId: singleAgentId!,
            agentName: singleAgentName,
            title: "跑完整个任务",
            status: "pending",
            expects: [],
          },
        ]
      : [
          {
            id: `${runId}-step-intent`,
            stepIndex: 0,
            agentId: "muse",
            agentName: "Muse",
            title: "写规范",
            status: "pending",
            expects: [`.peck/handoff/${runNum}/intent.md`, ".peck/tasks.md"],
          },
          {
            id: `${runId}-step-build`,
            stepIndex: 1,
            agentId: "grok",
            agentName: "Grok",
            title: "实施",
            status: "pending",
            expects: [],
          },
        ];

    const initialTasks: TaskItem[] = [
      {
        id: `${runId}-t1`,
        text: `写规范（.peck/handoff/${runNum}/intent.md）`,
        completed: false,
      },
      {
        id: `${runId}-t2`,
        text: `列任务清单（.peck/tasks.md）`,
        completed: false,
      },
      {
        id: `${runId}-t3`,
        text: `按规范改代码并自检`,
        completed: false,
      },
    ];

    const record: RunRecord = { id: runId, number: runNum, sessionId, flowId, flowName,
      prompt, status: "running", startedAt: Date.now(), steps: structuredClone(steps), tasks: [], files: [], logs: [] };
    runsState[sessionId] = [...(runsState[sessionId] ?? []), record];
    const onEvent = (event: RunEvent) => {
      if (event.type === "StepStarted") record.steps = record.steps.map((s) => s.id === event.payload.stepId ? { ...s, status: "running", startedAt: Date.now(), executionMode: "mock", notice: DEMO_NOTICE } : s);
      if (event.type === "StepOutput") record.logs.push({ id: `log-${record.logs.length}`, runId, stepId: event.payload.stepId, agentId: "mock", stream: event.payload.stream, chunk: event.payload.chunk, timestamp: Date.now() });
      if (event.type === "TasksUpdated") record.tasks = event.payload.items;
      if (event.type === "StepFinished") {
        record.steps = record.steps.map((s) => s.id === event.payload.stepId ? { ...s, status: event.payload.status, durationMs: event.payload.durationMs, exitCode: event.payload.exitCode } : s);
        for (const file of event.payload.changeset.files) {
          const index = record.files.findIndex((f) => f.path === file.path);
          if (index < 0) record.files.push(file); else record.files[index] = file;
        }
      }
      if (event.type === "RunFinished") {
        record.status = event.payload.status; record.summary = event.payload.summary; record.finishedAt = Date.now();
        if (event.payload.status === "cancelled") record.steps = record.steps.map((s) => s.status === "running" || s.status === "pending" ? { ...s, status: "cancelled" } : s);
        activeCallbacks.delete(runId);
      }
      listener(event);
      if (event.type === "StepStarted") listener({ type: "StepExecution", payload: { runId, stepId: event.payload.stepId, mode: "mock", notice: DEMO_NOTICE } });
    };
    activeCallbacks.set(runId, onEvent);

    const userMsg: Message = {
      id: `msg-u-${Date.now()}`,
      sessionId,
      role: "user",
      content: prompt,
      createdAt: Date.now(),
    };
    const assistantMsg: Message = {
      id: `msg-a-${Date.now() + 1}`,
      sessionId,
      role: "assistant",
      content: isSingleStep
        ? `交给 ${flowName} 单个跑完整个任务。改动在右栏。`
        : `交给 ${flowName}：Muse 先写规范，Grok 按规范实施。改动在右栏。`,
      runId,
      createdAt: Date.now() + 1,
    };

    messagesState[sessionId] = [
      ...(messagesState[sessionId] ?? []),
      userMsg,
      assistantMsg,
    ];

    const timers: number[] = [];
    const schedule = (delayMs: number, fn: () => void) => {
      const tid = window.setTimeout(fn, delayMs);
      timers.push(tid);
    };
    activeTimers.set(runId, timers);

    // Emit RunStarted immediately
    schedule(40, () => {
      onEvent({
        type: "RunStarted",
        payload: {
          runId,
          flowId,
          flowName,
          steps,
        },
      });
    });

    const step1 = steps[0];
    const intentPath = `.peck/handoff/${runNum}/intent.md`;

    schedule(180, () => {
      onEvent({
        type: "StepStarted",
        payload: {
          runId,
          stepId: step1.id,
          stepIndex: 0,
          agentId: step1.agentId,
        },
      });
      onEvent({
        type: "StepOutput",
        payload: {
          runId,
          stepId: step1.id,
          stream: "stdout",
          chunk: `[${step1.agentId}] Dispatching prompt: "${cleanPrompt}"`,
        },
      });
    });

    schedule(700, () => {
      diffsState[`${runId}:${intentPath}`] = {
        runId,
        stepId: step1.id,
        filePath: intentPath,
        kind: "create",
        language: "markdown",
        additions: 8,
        deletions: 0,
        contentAfter: `# 规范（任务 ${runNum}）\n\n> ${cleanPrompt}\n\n## 要点\n1. 先看项目结构，定改法；\n2. 任务清单写在 \`.peck/tasks.md\`，做一条勾一条；\n3. 只改项目里的文件，任务前有快照。\n`,
        unifiedDiff: `diff --git a/${intentPath} b/${intentPath}\nnew file mode 100644\n--- /dev/null\n+++ b/${intentPath}\n@@ -0,0 +1,8 @@\n+# 规范（任务 ${runNum}）\n+\n+> ${cleanPrompt}\n+\n+## 要点\n+1. 先看项目结构，定改法；\n+2. 任务清单写在 \`.peck/tasks.md\`，做一条勾一条；\n+3. 只改项目里的文件，任务前有快照。`,
      };

      onEvent({
        type: "FileTouched",
        payload: {
          runId,
          stepId: step1.id,
          path: intentPath,
          kind: "create",
        },
      });

      const updatedTasks = initialTasks.map((t, idx) =>
        idx === 0 ? { ...t, completed: true } : t,
      );
      onEvent({
        type: "TasksUpdated",
        payload: {
          runId,
          stepId: step1.id,
          total: updatedTasks.length,
          completed: 1,
          items: updatedTasks,
        },
      });
    });

    schedule(1300, () => {
      const updatedTasks = initialTasks.map((t, idx) =>
        idx <= 1 || isSingleStep ? { ...t, completed: true } : t,
      );
      onEvent({
        type: "TasksUpdated",
        payload: {
          runId,
          stepId: step1.id,
          total: updatedTasks.length,
          completed: updatedTasks.filter((t) => t.completed).length,
          items: updatedTasks,
        },
      });
      onEvent({
        type: "StepOutput",
        payload: {
          runId,
          stepId: step1.id,
          stream: "stdout",
          chunk: `[${step1.agentId}] Generated ${intentPath} and updated .peck/tasks.md`,
        },
      });
      onEvent({
        type: "StepFinished",
        payload: {
          runId,
          stepId: step1.id,
          status: "succeeded",
          exitCode: 0,
          durationMs: 1120,
          changeset: {
            files: [
              {
                path: intentPath,
                kind: "create",
                additions: 8,
                deletions: 0,
                stepId: step1.id,
              },
              {
                path: ".peck/tasks.md",
                kind: "modify",
                additions: 6,
                deletions: 0,
                stepId: step1.id,
              },
            ],
            totalAdditions: 14,
            totalDeletions: 0,
          },
        },
      });

      if (isSingleStep) {
        onEvent({
          type: "RunFinished",
          payload: {
            runId,
            status: "succeeded",
            summary: `${flowName} 跑完了：改了 2 个文件。验证：演示数据，没有真实验证。`,
          },
        });
        clearRunTimers(runId);
      }
    });

    if (!isSingleStep && steps[1]) {
      const step2 = steps[1];
      schedule(1500, () => {
        onEvent({
          type: "StepStarted",
          payload: {
            runId,
            stepId: step2.id,
            stepIndex: 1,
            agentId: step2.agentId,
          },
        });
        onEvent({
          type: "StepOutput",
          payload: {
            runId,
            stepId: step2.id,
            stream: "stdout",
            chunk: `[grok] Reading ${intentPath} & .peck/tasks.md ...`,
          },
        });
      });

      schedule(2200, () => {
        onEvent({
          type: "FileTouched",
          payload: {
            runId,
            stepId: step2.id,
            path: "src/pages/Login.tsx",
            kind: "modify",
          },
        });
        const allDone = initialTasks.map((t) => ({ ...t, completed: true }));
        onEvent({
          type: "TasksUpdated",
          payload: {
            runId,
            stepId: step2.id,
            total: allDone.length,
            completed: allDone.length,
            items: allDone,
          },
        });
      });

      schedule(2700, () => {
        onEvent({
          type: "StepOutput",
          payload: {
            runId,
            stepId: step2.id,
            stream: "stdout",
            chunk: `[grok] 任务清单 3/3 已勾。`,
          },
        });
        onEvent({
          type: "StepFinished",
          payload: {
            runId,
            stepId: step2.id,
            status: "succeeded",
            exitCode: 0,
            durationMs: 1200,
            changeset: {
              files: [
                {
                  path: "src/pages/Login.tsx",
                  kind: "modify",
                  additions: 12,
                  deletions: 3,
                  stepId: step2.id,
                },
              ],
              totalAdditions: 12,
              totalDeletions: 3,
            },
          },
        });
        onEvent({
          type: "RunFinished",
          payload: {
            runId,
            status: "succeeded",
            summary: `Muse → Grok 跑完了：Muse 写了规范，Grok 改了 1 个文件。验证：演示数据，没有真实验证。`,
          },
        });
        clearRunTimers(runId);
      });
    }

    return runId;
  },

  async runCancel(runId: string): Promise<void> {
    clearRunTimers(runId);
    activeCallbacks.get(runId)?.({ type: "RunFinished", payload: { runId, status: "cancelled", summary: "任务已停止。" } });
  },

  async runRetryStep(
    runId: string,
    stepId: string,
    onEvent: (event: RunEvent) => void,
  ): Promise<void> {
    const record = Object.values(runsState).flat().find((r) => r.id === runId);
    if (!record) throw new Error("没找到这个任务");
    const step = record.steps.find((s) => s.id === stepId);
    if (!step) throw new Error("没找到这个步骤");
    record.status = "running";
    step.status = "running";
    activeCallbacks.set(runId, (event) => {
      if (event.type === "RunFinished") { record.status = event.payload.status; record.summary = event.payload.summary; activeCallbacks.delete(runId); }
      onEvent(event);
    });
    onEvent({
      type: "StepStarted",
      payload: {
        runId,
        stepId,
        stepIndex: 0,
        agentId: "grok",
      },
    });
    const retryTimer = window.setTimeout(() => {
      onEvent({
        type: "StepOutput",
        payload: {
          runId,
          stepId,
          stream: "stdout",
          chunk: "[grok] 这一步重跑完成（退出码 0）。",
        },
      });
      onEvent({
        type: "StepFinished",
        payload: {
          runId,
          stepId,
          status: "succeeded",
          exitCode: 0,
          durationMs: 850,
          changeset: {
            files: [],
            totalAdditions: 0,
            totalDeletions: 0,
          },
        },
      });
      onEvent({
        type: "RunFinished",
        payload: {
          runId,
          status: "succeeded",
          summary: "这一步重跑完成。",
        },
      });
      record.status = "succeeded";
      record.summary = "这一步重跑完成。验证：演示数据，没有真实验证。";
      record.steps = record.steps.map((s) => s.stepIndex >= step.stepIndex ? { ...s, status: "succeeded", executionMode: "mock", notice: DEMO_NOTICE } : s);
      record.tasks = record.tasks.map((t) => ({ ...t, completed: true }));
      activeCallbacks.delete(runId);
      activeTimers.delete(runId);
    }, 850);
    activeTimers.set(runId, [retryTimer]);
  },

  async changesGetDiff(
    runId: string,
    stepId?: string,
    filePath?: string,
  ): Promise<DiffResult> {
    const record = Object.values(runsState).flat().find((r) => r.id === runId);
    const targetPath = filePath || record?.files[0]?.path || ".peck/tasks.md";
    const found = diffsState[`${runId}:${targetPath}`];
    if (found) {
      return { ...found, runId, stepId };
    }
    // 新发的任务里没单独造 Diff 的文件，给一段通用的改动。
    return {
      runId,
      stepId,
      filePath: targetPath,
      kind: "modify",
      language: "typescript",
      additions: 1,
      deletions: 0,
      unifiedDiff: `diff --git a/${targetPath} b/${targetPath}\n--- a/${targetPath}\n+++ b/${targetPath}\n@@ -1,2 +1,3 @@\n+// 任务 ${record?.number ?? runId} 改过这里（演示数据）\n export const updated = true;\n`,
    };
  },

  /** API 模式任务的调用与审查历史（右栏展开看）。 */
  async apiTaskHistory(runId: string): Promise<ApiTask[]> {
    return structuredClone(apiTasksState[runId] ?? []);
  },

  async agentsList(): Promise<AgentInfo[]> {
    return [...MOCK_AGENTS];
  },
  async agentGetConfig(agentId: string): Promise<string> {
    const agent = MOCK_AGENTS.find((a) => a.id === agentId);
    if (!agent) throw new Error("没找到这个 CLI");
    return configState[agentId] ?? `id = "${agent.id}"\nname = "${agent.name}"\ncommand = "${agent.command}"\nargs = ${JSON.stringify(agent.args)}\ncwd = "{{workspace}}"\nstdin = "none"\ntimeout = "${agent.timeout}"\npath = []\n\n[env]\n`;
  },
  async agentSaveConfig(agentId: string, content: string): Promise<void> {
    configState[agentId] = content;
  },
};
