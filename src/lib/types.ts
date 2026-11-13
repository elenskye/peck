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
  /** 「移出列表」了（左栏不列）。 */
  hidden?: boolean;
  /** 项目设置：在这个项目里发任务默认谁来干、哪一档权限；空着跟全局。 */
  settings?: ProjectSettings;
}

export interface ProjectSettings {
  who?: string | null;
  permission?: string | null;
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
  /** 谁来干：muse、grok、muse>grok、grok>muse、mock；API 模式和旧记录没有。 */
  who?: string;
  permission?: Permission;
  /** 「撤销整个任务」的时间。 */
  undoneAt?: number;
}

export type Permission = "plan" | "edit" | "auto";

/** 左栏的任务摘要：一次对话一行，带最近一次记录的状态。 */
export interface TaskBrief {
  id: string;
  workspaceId: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  runCount: number;
  lastRun?: {
    id: string;
    number: number;
    flowId: string;
    flowName: string;
    status: RunStatus;
    startedAt: number;
    finishedAt?: number;
    files: FileChangeItem[];
    who?: string;
    undoneAt?: number;
  } | null;
}

export interface ProjectOverview {
  workspace: Workspace;
  tasks: TaskBrief[];
  /** 项目文件夹不在了（每次读列表都看一遍）。 */
  missing?: boolean;
}

export interface FileEntry { path: string; size: number }
export interface FileListing { entries: FileEntry[]; hidden: number; truncated: boolean }
export interface FilePreview {
  path: string;
  kind: "text" | "image" | "binary" | "large" | "missing";
  content?: string | null;
  size: number;
  deleted: boolean;
}

export interface UndoFile { path: string; kind: FileChangeKind; changedSince: boolean }
export interface UndoPreview { runId: string; files: UndoFile[] }

export type TermEvent =
  | { type: "Output"; payload: { id: string; chunk: string } }
  | { type: "Exit"; payload: { id: string; code: number | null; durationMs: number; cwd: string; stopped: boolean } };
export interface TermState { cwd: string; running: string | null }
export type Activity = "idle" | "running" | "waiting";

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
