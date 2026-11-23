export type StepStatus = "pending" | "running" | "succeeded" | "failed" | "cancelled";
export type RunStatus = "running" | "succeeded" | "failed" | "cancelled";
export type FileChangeKind = "create" | "modify" | "delete";

export interface Workspace {
  id: string;
  path: string;
  name: string;
  lastOpenedAt: number;
  gitBranch?: string;
  /** 「移出列表」了（左栏不列）。 */
  hidden?: boolean;
  /** 项目设置：在这个项目里发任务默认谁来干、哪一档权限；空着跟全局。 */
  settings?: ProjectSettings;
}

export interface ProjectSettings {
  who?: string | null;
  permission?: string | null;
  /** 这个项目默认用哪套合作预设（v0.9 B2）；有它就不看 who。 */
  preset?: string | null;
  /** 项目即环境（v0.7）：主文件夹以外的文件夹，能改还是只读。 */
  folders?: ProjectFolder[];
  /** 这个项目的环境变量（v0.7；v0.10 起 CLI 引擎也带）。 */
  env?: EnvVar[];
  /** 这个项目里 Peck Loop 不起的 MCP 服务器（v0.10 A2，按名字）。 */
  mcpOff?: string[];
  /** 这个项目里不用的 Skills（v0.10 C2，按名字）。 */
  skillsOff?: string[];
}

export interface ProjectFolder { path: string; writable: boolean }
export interface EnvVar { name: string; value: string }

export interface Session {
  id: string;
  workspaceId: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  /** 归档的时间（v0.9）：归档的不在左栏列。 */
  archivedAt?: number | null;
}

export interface FileChangeItem {
  path: string;
  kind: FileChangeKind;
  additions: number;
  deletions: number;
}

export interface StepDescriptor {
  id: string;
  stepIndex: number;
  agentId: string;
  agentName: string;
  title: string;
  status: StepStatus;
  exitCode?: number;
  durationMs?: number;
  startedAt?: number;
  finishedAt?: number;
}

export interface RunRecord {
  id: string;
  number: number;
  sessionId: string;
  prompt: string;
  status: RunStatus;
  startedAt: number;
  finishedAt?: number;
  steps: StepDescriptor[];
  files: FileChangeItem[];
  summary?: string;
  /** 谁来干：`api:<供应商>/<模型>`、`cli:<引擎>`、`team:<写规范>|<实施>|<审查>`；v0.8 以前的旧记录没有。 */
  who?: string;
  permission?: Permission;
  /** 「撤销整个任务」的时间。 */
  undoneAt?: number;
  /** 在独立工作区（git worktree）里做的（v0.7）。 */
  worktree?: WorktreeState | null;
}

export interface WorktreeState {
  path: string;
  appliedAt?: number | null;
  discardedAt?: number | null;
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
  /** 归档的时间：只有设置 → ARCHIVE 那份列表里有。 */
  archivedAt?: number | null;
  /** 模型提了、你还没批的 Skill（v0.10 C3）：左栏 NOW 里算「等你」。 */
  skillsWaiting?: number;
  lastRun?: {
    id: string;
    number: number;
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
  /** office：Word、Excel 这类，交给 Quick Look（v0.7）。 */
  kind: "text" | "image" | "pdf" | "office" | "binary" | "large" | "missing";
  content?: string | null;
  size: number;
  deleted: boolean;
  /** 图片、PDF：data URL（v0.7）。 */
  dataUrl?: string | null;
}

export type TermEvent =
  | { type: "Output"; payload: { id: string; chunk: string } }
  | { type: "Exit"; payload: { id: string; code: number | null; durationMs: number; cwd: string; stopped: boolean } };
export interface TermState { cwd: string; running: string | null }
export type Activity = "idle" | "running" | "waiting";
