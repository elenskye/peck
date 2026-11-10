export type StepStatus = "pending" | "running" | "succeeded" | "failed" | "cancelled";
export type RunStatus = "running" | "succeeded" | "failed" | "cancelled";
export type StreamType = "stdout" | "stderr";
export type FileChangeKind = "create" | "modify" | "delete";

export interface Workspace {
  id: string;
  path: string;
  name: string;
  lastOpenedAt: number;
  gitBranch?: string;
  gitignoreConfigured?: boolean;
}

export interface Session {
  id: string;
  workspaceId: string;
  title: string;
  createdAt: number;
  updatedAt: number;
}

export interface Message {
  id: string;
  sessionId: string;
  role: "user" | "assistant" | "system";
  content: string;
  runId?: string;
  createdAt: number;
}

export interface TaskItem {
  id: string;
  text: string;
  completed: boolean;
}

export interface FileChangeItem {
  path: string;
  kind: FileChangeKind;
  additions: number;
  deletions: number;
  stepId: string;
}

export interface ChangeSetSummary {
  files: FileChangeItem[];
  totalAdditions: number;
  totalDeletions: number;
}

export interface StepDescriptor {
  id: string;
  stepIndex: number;
  agentId: string;
  agentName: string;
  title: string;
  status: StepStatus;
  expects: string[];
  exitCode?: number;
  durationMs?: number;
  startedAt?: number;
  finishedAt?: number;
  executionMode?: "mock" | "cli" | "api-mock" | "api-real";
  notice?: string;
}

export interface RawLogEntry {
  id: string;
  runId: string;
  stepId: string;
  agentId: string;
  stream: StreamType;
  chunk: string;
  timestamp: number;
}

export interface RunRecord {
  id: string;
  number: number;
  sessionId: string;
  flowId: string;
  flowName: string;
  prompt: string;
  status: RunStatus;
  startedAt: number;
  finishedAt?: number;
  steps: StepDescriptor[];
  tasks: TaskItem[];
  files: FileChangeItem[];
  stepFiles?: Record<string, FileChangeItem[]>;
  logs: RawLogEntry[];
  summary?: string;
}

export interface DiffResult {
  runId: string;
  stepId?: string;
  filePath: string;
  kind: FileChangeKind;
  language: string;
  additions: number;
  deletions: number;
  unifiedDiff: string;
  contentAfter?: string;
}

export interface AgentInfo {
  id: string;
  name: string;
  description: string;
  command: string;
  args: string[];
  timeout: string;
  available: boolean;
  resolvedPath?: string;
  version?: string;
  isMock: boolean;
  configPath?: string;
  diagnostic?: string;
}

export type RunEvent =
  | { type: "StepAdded"; payload: { runId: string; step: StepDescriptor } }
  | {
      type: "StepExecution";
      payload: { runId: string; stepId: string; mode: "mock" | "cli" | "api-mock" | "api-real"; notice: string };
    }
  | {
      type: "RunStarted";
      payload: {
        runId: string;
        flowId: string;
        flowName: string;
        steps: StepDescriptor[];
      };
    }
  | {
      type: "StepStarted";
      payload: {
        runId: string;
        stepId: string;
        stepIndex: number;
        agentId: string;
      };
    }
  | {
      type: "StepOutput";
      payload: {
        runId: string;
        stepId: string;
        stream: StreamType;
        chunk: string;
      };
    }
  | {
      type: "TasksUpdated";
      payload: {
        runId: string;
        stepId: string;
        total: number;
        completed: number;
        items: TaskItem[];
      };
    }
  | {
      type: "FileTouched";
      payload: {
        runId: string;
        stepId: string;
        path: string;
        kind: FileChangeKind;
      };
    }
  | {
      type: "StepFinished";
      payload: {
        runId: string;
        stepId: string;
        status: StepStatus;
        exitCode?: number;
        durationMs: number;
        changeset: ChangeSetSummary;
      };
    }
  | {
      type: "RunFinished";
      payload: {
        runId: string;
        status: RunStatus;
        summary: string;
      };
    };
