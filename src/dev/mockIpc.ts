import {
  MOCK_AGENTS,
  MOCK_DIFFS,
  MOCK_MESSAGES,
  MOCK_RUN_104,
  MOCK_SESSIONS,
  MOCK_WORKSPACES,
} from "@/dev/mockData";
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

const emptyPreview = new URLSearchParams(window.location.search).has("empty");
let workspacesState: Workspace[] = emptyPreview ? [] : [...MOCK_WORKSPACES];
let sessionsState: Session[] = [...MOCK_SESSIONS];
const messagesState: Record<string, Message[]> = structuredClone(MOCK_MESSAGES);
const runsState: Record<string, RunRecord[]> = {
  "sess-104": [structuredClone(MOCK_RUN_104)],
  "sess-103": [],
};
const diffsState: Record<string, DiffResult> = structuredClone(MOCK_DIFFS);
let runCounter = 105;

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
      title: title || "新发令会话",
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
    const flowId = isSingleStep ? `single-${singleAgentId}` : "idea-to-code";
    const flowName = isSingleStep
      ? `单步直达 (@${singleAgentId})`
      : "想法 → 代码";

    const steps: StepDescriptor[] = isSingleStep
      ? [
          {
            id: `${runId}-step-1`,
            stepIndex: 0,
            agentId: singleAgentId!,
            agentName:
              MOCK_AGENTS.find((a) => a.id === singleAgentId)?.name ?? singleAgentId!,
            title: `执行 @${singleAgentId} 指令`,
            status: "pending",
            expects: [],
          },
        ]
      : [
          {
            id: `${runId}-step-intent`,
            stepIndex: 0,
            agentId: "muse",
            agentName: "Muse Code",
            title: "意图与任务生成",
            status: "pending",
            expects: [`.peck/handoff/${runNum}/intent.md`, ".peck/tasks.md"],
          },
          {
            id: `${runId}-step-build`,
            stepIndex: 1,
            agentId: "grok",
            agentName: "Grok Build",
            title: "代码构建与实施",
            status: "pending",
            expects: [],
          },
        ];

    const initialTasks: TaskItem[] = [
      {
        id: `${runId}-t1`,
        text: `生成需求规范文档 (.peck/handoff/${runNum}/intent.md)`,
        completed: false,
      },
      {
        id: `${runId}-t2`,
        text: `拆解可执行任务清单 (.peck/tasks.md)`,
        completed: false,
      },
      {
        id: `${runId}-t3`,
        text: `根据需求实施工作区代码变更并运行静态检查`,
        completed: false,
      },
    ];

    const record: RunRecord = { id: runId, number: runNum, sessionId, flowId, flowName,
      prompt, status: "running", startedAt: Date.now(), steps: structuredClone(steps), tasks: [], files: [], logs: [] };
    runsState[sessionId] = [...(runsState[sessionId] ?? []), record];
    const onEvent = (event: RunEvent) => {
      if (event.type === "StepStarted") record.steps = record.steps.map((s) => s.id === event.payload.stepId ? { ...s, status: "running", startedAt: Date.now(), executionMode: "mock", notice: "浏览器模拟执行，未调用真实 AI。" } : s);
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
      if (event.type === "StepStarted") listener({ type: "StepExecution", payload: { runId, stepId: event.payload.stepId, mode: "mock", notice: "浏览器模拟执行，未调用真实 AI。" } });
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
      content: `已触发「${flowName}」发令流水线，正在调度底层 CLI 并监听工作区文件变动...`,
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
      diffsState[intentPath] = {
        runId,
        stepId: step1.id,
        filePath: intentPath,
        kind: "create",
        language: "markdown",
        additions: 28,
        deletions: 0,
        contentAfter: `# 意图规范文档 (Run #${runNum})\n\n> **用户指令**：${cleanPrompt}\n\n## 实施要点\n1. 解析当前工作区结构并生成变更方案；\n2. 更新 \`.peck/tasks.md\` 复选框任务；\n3. 保持只读边界与影子 Git 快照一致性。\n`,
        unifiedDiff: `diff --git a/${intentPath} b/${intentPath}\nnew file mode 100644\n--- /dev/null\n+++ b/${intentPath}\n@@ -0,0 +1,8 @@\n+# 意图规范文档 (Run #${runNum})\n+\n+> **用户指令**：${cleanPrompt}\n+\n+## 实施要点\n+1. 解析当前工作区结构并生成变更方案；\n+2. 更新 \`.peck/tasks.md\` 复选框任务；\n+3. 保持只读边界与影子 Git 快照一致性。`,
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
                additions: 28,
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
            totalAdditions: 34,
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
            summary: `单步任务 @${step1.agentId} 执行完成。`,
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
            chunk: `[grok] Shadow Git snapshot committed. All 3/3 tasks marked [x].`,
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
            summary: `「想法 → 代码」双步流水线执行成功（Run #${runNum}）。`,
          },
        });
        clearRunTimers(runId);
      });
    }

    return runId;
  },

  async runCancel(runId: string): Promise<void> {
    clearRunTimers(runId);
    activeCallbacks.get(runId)?.({ type: "RunFinished", payload: { runId, status: "cancelled", summary: "模拟任务已停止。" } });
  },

  async runRetryStep(
    runId: string,
    stepId: string,
    onEvent: (event: RunEvent) => void,
  ): Promise<void> {
    const record = Object.values(runsState).flat().find((r) => r.id === runId);
    if (!record) throw new Error("未找到运行记录");
    const step = record.steps.find((s) => s.id === stepId);
    if (!step) throw new Error("未找到步骤");
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
          chunk: "[retry] Step re-executed successfully (exit code 0).",
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
          summary: "步骤重试成功。",
        },
      });
      record.status = "succeeded";
      record.summary = "模拟步骤重试成功。";
      record.steps = record.steps.map((s) => s.stepIndex >= step.stepIndex ? { ...s, status: "succeeded", executionMode: "mock", notice: "浏览器模拟执行，未调用真实 AI。" } : s);
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
    const targetPath = filePath || ".peck/handoff/104/intent.md";
    const found = diffsState[targetPath];
    if (found) {
      return { ...found, runId, stepId };
    }
    return {
      runId,
      stepId,
      filePath: targetPath,
      kind: "modify",
      language: "typescript",
      additions: 4,
      deletions: 1,
      unifiedDiff: `diff --git a/${targetPath} b/${targetPath}\n--- a/${targetPath}\n+++ b/${targetPath}\n@@ -1,3 +1,6 @@\n+// Updated by Peck Run ${runId}\n export const updated = true;\n`,
    };
  },

  async agentsList(): Promise<AgentInfo[]> {
    return [...MOCK_AGENTS];
  },
  async agentGetConfig(agentId: string): Promise<string> {
    const agent = MOCK_AGENTS.find((a) => a.id === agentId);
    if (!agent) throw new Error("未找到 Agent");
    return configState[agentId] ?? `id = "${agent.id}"\nname = "${agent.name}"\ncommand = "${agent.command}"\nargs = ${JSON.stringify(agent.args)}\ncwd = "{{workspace}}"\nstdin = "none"\ntimeout = "${agent.timeout}"\npath = []\n\n[env]\n`;
  },
  async agentSaveConfig(agentId: string, content: string): Promise<void> {
    configState[agentId] = content;
  },
};
