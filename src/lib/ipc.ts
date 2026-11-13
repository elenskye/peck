import { Channel, invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import type { AgentEvent, CliAccountStatus, CliLoginState, Control, EngineAccounts, GatewayConfig, GatewaySettings, GatewayRunArgs, ModelEntry } from "@/lib/agentEvents";
import type {
  Activity,
  AgentInfo,
  DiffResult,
  FileListing,
  FilePreview,
  Message,
  Permission,
  ProjectOverview,
  RunEvent,
  RunRecord,
  Session,
  TermEvent,
  TermState,
  UndoPreview,
  ProjectSettings,
  Workspace,
} from "@/lib/types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isMockMode(): boolean {
  if (typeof window === "undefined") return true;
  // A native window always uses Rust IPC, including native API mock mode.
  return typeof window.__TAURI_INTERNALS__ === "undefined";
}

export const ipc = {
  async workspaceChoose(): Promise<Workspace | null> {
    if (isMockMode()) return null;
    return invoke<Workspace | null>("workspace_choose");
  },
  async workspaceOpen(path: string): Promise<Workspace> {
    if (isMockMode()) {
      return mockIpc.workspaceOpen(path);
    }
    return invoke<Workspace>("workspace_open", { path });
  },

  /** 改项目的名字、项目设置。 */
  async workspaceUpdate(id: string, patch: { name?: string; settings?: ProjectSettings }): Promise<Workspace> {
    if (isMockMode()) return mockIpc.workspaceUpdate(id, patch);
    return invoke<Workspace>("workspace_update", { id, name: patch.name ?? null, settings: patch.settings ?? null });
  },
  /** 移出列表：文件夹、任务、快照都不动，重新打开这个文件夹就回来。 */
  async workspaceRemove(id: string): Promise<void> {
    if (isMockMode()) return mockIpc.workspaceRemove(id);
    return invoke<void>("workspace_remove", { id });
  },
  /** 文件夹不在了：换一个文件夹（不给路径就弹选文件夹的窗）。 */
  async workspaceRelocate(id: string, path?: string): Promise<Workspace | null> {
    if (isMockMode()) return mockIpc.workspaceRelocate(id, path);
    return invoke<Workspace | null>("workspace_relocate", { id, path: path ?? null });
  },
  async workspaceReveal(id: string): Promise<void> {
    if (isMockMode()) return;
    return invoke<void>("workspace_reveal", { id });
  },
  async sessionRename(id: string, title: string): Promise<void> {
    if (isMockMode()) return mockIpc.sessionRename(id, title);
    return invoke<void>("session_rename", { id, title });
  },
  /** 删一个任务和它的全部记录（项目里的文件不动）。 */
  async sessionDelete(id: string): Promise<void> {
    if (isMockMode()) return mockIpc.sessionDelete(id);
    return invoke<void>("session_delete", { id });
  },
  /** 存拖动排好的顺序。 */
  async layoutSave(args: { projects?: string[]; workspaceId?: string; tasks?: string[] }): Promise<void> {
    if (isMockMode()) return mockIpc.layoutSave(args);
    return invoke<void>("layout_save", { projects: args.projects ?? null, workspaceId: args.workspaceId ?? null, tasks: args.tasks ?? null });
  },
  /** QUOTA 页的图表：近几天的花费和额度读数。 */
  async spendOverview(days: number): Promise<SpendOverview> {
    if (isMockMode()) return mockIpc.spendOverview(days);
    return invoke<SpendOverview>("spend_overview", { days });
  },

  async workspaceListRecent(): Promise<Workspace[]> {
    if (isMockMode()) {
      return mockIpc.workspaceListRecent();
    }
    return invoke<Workspace[]>("workspace_list_recent");
  },

  async sessionCreate(workspaceId: string, title?: string): Promise<Session> {
    if (isMockMode()) {
      return mockIpc.sessionCreate(workspaceId, title);
    }
    return invoke<Session>("session_create", { workspaceId, title });
  },

  async sessionList(workspaceId: string): Promise<Session[]> {
    if (isMockMode()) {
      return mockIpc.sessionList(workspaceId);
    }
    return invoke<Session[]>("session_list", { workspaceId });
  },

  async sessionGetMessages(sessionId: string): Promise<Message[]> {
    if (isMockMode()) {
      return mockIpc.sessionGetMessages(sessionId);
    }
    return invoke<Message[]>("session_get_messages", { sessionId });
  },

  async sessionGetRuns(sessionId: string): Promise<RunRecord[]> {
    if (isMockMode()) {
      return mockIpc.sessionGetRuns(sessionId);
    }
    return invoke<RunRecord[]>("session_get_runs", { sessionId });
  },

  /** 发任务。`who` 是「谁来干」（muse、grok、muse>grok、grok>muse、mock）。 */
  async runDispatch(
    sessionId: string,
    prompt: string,
    who: string,
    permission: Permission,
    onEvent: (event: RunEvent) => void,
  ): Promise<string> {
    if (isMockMode()) {
      return mockIpc.runDispatch(sessionId, prompt, who, permission, onEvent);
    }
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("run_dispatch", { sessionId, prompt, who, permission, channel });
  },

  async runCancel(runId: string): Promise<void> {
    if (isMockMode()) {
      return mockIpc.runCancel(runId);
    }
    return invoke<void>("run_cancel", { runId });
  },

  async runRetryStep(
    runId: string,
    stepId: string,
    onEvent: (event: RunEvent) => void,
  ): Promise<void> {
    if (isMockMode()) {
      return mockIpc.runRetryStep(runId, stepId, onEvent);
    }
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("run_retry_step", {
      runId,
      stepId,
      channel,
    });
  },

  async changesGetDiff(
    runId: string,
    stepId?: string,
    filePath?: string,
  ): Promise<DiffResult> {
    if (isMockMode()) {
      return mockIpc.changesGetDiff(runId, stepId, filePath);
    }
    return invoke<DiffResult>("changes_get_diff", {
      runId,
      stepId,
      filePath,
    });
  },

  async agentsList(): Promise<AgentInfo[]> {
    if (isMockMode()) {
      return mockIpc.agentsList();
    }
    return invoke<AgentInfo[]>("agents_list");
  },
  async agentGetConfig(agentId: string): Promise<string> {
    if (isMockMode()) return mockIpc.agentGetConfig(agentId);
    return invoke<string>("agent_get_config", { agentId });
  },
  async agentSaveConfig(agentId: string, content: string): Promise<void> {
    if (isMockMode()) return mockIpc.agentSaveConfig(agentId, content);
    return invoke<void>("agent_save_config", { agentId, content });
  },

  async overview(): Promise<ProjectOverview[]> {
    if (isMockMode()) return mockIpc.overview();
    return invoke<ProjectOverview[]>("projects_overview");
  },
  async filesList(workspaceId: string): Promise<FileListing> {
    if (isMockMode()) return mockIpc.filesList(workspaceId);
    return invoke<FileListing>("files_list", { workspaceId });
  },
  async filesRead(workspaceId: string, path: string, runId?: string): Promise<FilePreview> {
    if (isMockMode()) return mockIpc.filesRead(workspaceId, path, runId);
    return invoke<FilePreview>("files_read", { workspaceId, path, runId });
  },
  async runUndoPreview(runId: string): Promise<UndoPreview> {
    if (isMockMode()) return mockIpc.runUndoPreview(runId);
    return invoke<UndoPreview>("run_undo_preview", { runId });
  },
  async runUndo(runId: string): Promise<RunRecord> {
    if (isMockMode()) return mockIpc.runUndo(runId);
    return invoke<RunRecord>("run_undo", { runId });
  },
  async settingsGet(): Promise<string | null> {
    if (isMockMode()) return mockIpc.settingsGet();
    return invoke<string | null>("settings_get");
  },
  async settingsSet(value: string): Promise<void> {
    if (isMockMode()) return mockIpc.settingsSet(value);
    return invoke<void>("settings_set", { value });
  },
  async termRun(workspaceId: string, command: string, onEvent: (event: TermEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.termRun(workspaceId, command, onEvent);
    const channel = new Channel<TermEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("term_run", { workspaceId, command, channel });
  },
  async termStop(workspaceId: string): Promise<void> {
    if (isMockMode()) return mockIpc.termStop(workspaceId);
    return invoke<void>("term_stop", { workspaceId });
  },
  async termState(workspaceId: string): Promise<TermState> {
    if (isMockMode()) return mockIpc.termState(workspaceId);
    return invoke<TermState>("term_state", { workspaceId });
  },
  /** 菜单栏小图标跟着变。浏览器里没有菜单栏，不做事。 */
  async setActivity(activity: Activity): Promise<void> {
    if (isMockMode()) return;
    return invoke<void>("shell_set_activity", { activity });
  },
  async notify(title: string, body: string): Promise<void> {
    if (isMockMode()) return mockIpc.notify(title, body);
    return invoke<void>("notify", { title, body });
  },

  // ── Model Gateway（v0.5 契约，src-tauri/src/gateway/）──
  /** 模型列表：API 模型（配了 Key 的能用）、CLI 引擎（检测到的能用）、接力。 */
  async gatewayModels(): Promise<ModelEntry[]> {
    if (isMockMode()) return mockIpc.gatewayModels();
    return invoke<ModelEntry[]>("gateway_models");
  },
  async gatewayConfigGet(): Promise<GatewaySettings> {
    if (isMockMode()) return mockIpc.gatewayConfigGet();
    return invoke<GatewaySettings>("gateway_config_get");
  },
  /** 存 Vertex 的 Project ID / Location、CLI 目录；格式不对会报错（中文一句）。 */
  async gatewayConfigSet(config: GatewayConfig): Promise<GatewayConfig> {
    if (isMockMode()) return mockIpc.gatewayConfigSet(config);
    return invoke<GatewayConfig>("gateway_config_set", { config });
  },
  /** 一个任务的统一事件（事件日志）。旧任务为空。 */
  async gatewayEvents(runId: string): Promise<AgentEvent[]> {
    if (isMockMode()) return mockIpc.gatewayEvents(runId);
    return invoke<AgentEvent[]>("gateway_events", { runId });
  },

  // ── CLI 账号（v0.5，src-tauri/src/gateway/accounts.rs）──
  async cliAccountsList(checkAll: boolean): Promise<EngineAccounts[]> {
    if (isMockMode()) return mockIpc.cliAccountsList(checkAll);
    return invoke<EngineAccounts[]>("cli_accounts_list", { checkAll });
  },
  async cliAccountAdd(engine: string, name: string): Promise<EngineAccounts[]> {
    if (isMockMode()) return mockIpc.cliAccountAdd(engine, name);
    return invoke<EngineAccounts[]>("cli_account_add", { engine, name });
  },
  async cliAccountUse(engine: string, id: string): Promise<EngineAccounts[]> {
    if (isMockMode()) return mockIpc.cliAccountUse(engine, id);
    return invoke<EngineAccounts[]>("cli_account_use", { engine, id });
  },
  async cliAccountRemove(engine: string, id: string): Promise<EngineAccounts[]> {
    if (isMockMode()) return mockIpc.cliAccountRemove(engine, id);
    return invoke<EngineAccounts[]>("cli_account_remove", { engine, id });
  },
  async cliAccountCheck(engine: string, id: string): Promise<CliAccountStatus> {
    if (isMockMode()) return mockIpc.cliAccountCheck(engine, id);
    return invoke<CliAccountStatus>("cli_account_check", { engine, id });
  },
  async cliAccountLogin(engine: string, id: string): Promise<CliLoginState> {
    if (isMockMode()) return mockIpc.cliAccountLogin(engine, id);
    return invoke<CliLoginState>("cli_account_login", { engine, id });
  },
  async cliAccountLoginState(engine: string, id: string): Promise<CliLoginState> {
    if (isMockMode()) return mockIpc.cliAccountLoginState(engine, id);
    return invoke<CliLoginState>("cli_account_login_state", { engine, id });
  },
  async cliAccountLoginInput(engine: string, id: string, text: string): Promise<void> {
    if (isMockMode()) return mockIpc.cliAccountLoginInput(engine, id, text);
    return invoke<void>("cli_account_login_input", { engine, id, text });
  },
  async cliAccountLoginCancel(engine: string, id: string): Promise<void> {
    if (isMockMode()) return mockIpc.cliAccountLoginCancel(engine, id);
    return invoke<void>("cli_account_login_cancel", { engine, id });
  },

  // ── 发任务（Gateway 版，src-tauri/src/gateway/runner.rs）：「谁来干」是 cli:<引擎> 的走这里 ──
  /** 返回任务 id；过程是统一事件，经 onEvent 来（第一条 session.started，最后一条 turn.completed）。 */
  async gatewayRunStart(args: GatewayRunArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.gatewayRunStart(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("gateway_run_start", { ...args, channel });
  },
  /** 审批、回答、追加、打断。任务已经结束会报错。 */
  async gatewayControl(runId: string, control: Control): Promise<void> {
    if (isMockMode()) return mockIpc.gatewayControl(runId, control);
    return invoke<void>("gateway_control", { runId, control });
  },

  // ── Peck 循环（v0.5 Phase 6，src-tauri/src/harness/）：「谁来干」是 api:<供应商>/<模型> 的走这里 ──
  /** 返回任务 id；过程是统一事件（和 CLI 引擎同一套）。审批、回答、打断照旧用 gatewayControl。 */
  async harnessRunStart(args: HarnessRunArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.harnessRunStart(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("harness_run_start", { ...args, channel });
  },
  /** ONE SHOT：只读回答一句（API 模型或 CLI，Antigravity 除外）；不占任务、不锁终端。返回 id（oneshot-…）。 */
  async harnessOneshot(args: OneshotArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.harnessOneshot(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("harness_oneshot", { ...args, channel });
  },
  /** 打开落盘的完整命令输出。 */
  async harnessOutputRead(path: string): Promise<string> {
    if (isMockMode()) return mockIpc.harnessOutputRead(path);
    return invoke<string>("harness_output_read", { path });
  },
  /** 这个月 Peck 循环（API）花了多少。 */
  async harnessSpend(): Promise<{ monthUsd: number; since: number }> {
    if (isMockMode()) return mockIpc.harnessSpend();
    return invoke<{ monthUsd: number; since: number }>("harness_spend");
  },
  /** 设置 → 权限：哪些项目「总是允许」了哪一类（装依赖、起常驻进程）。 */
  async harnessAllowList(): Promise<AllowRule[]> {
    if (isMockMode()) return mockIpc.harnessAllowList();
    return invoke<AllowRule[]>("harness_allow_list");
  },
  /** 撤掉一条「总是允许」，返回剩下的。 */
  async harnessAllowRemove(workspaceId: string, rule: string): Promise<AllowRule[]> {
    if (isMockMode()) return mockIpc.harnessAllowRemove(workspaceId, rule);
    return invoke<AllowRule[]>("harness_allow_remove", { workspaceId, rule });
  },
  /** 换 Dock 里的 App 图标（j2 / a3）。浏览器里不做事。 */
  async appIconSet(icon: "j2" | "a3"): Promise<void> {
    if (isMockMode()) return;
    return invoke<void>("app_icon_set", { icon });
  },
};

export interface HarnessRunArgs {
  sessionId: string;
  prompt: string;
  /** api:<供应商>/<模型> */
  who: string;
  permission?: Permission;
  effort?: string;
  /** 每月花费上限（美元）；空着不管。 */
  costCap?: number | null;
}

export interface OneshotArgs {
  workspaceId: string;
  prompt: string;
  /** api:… 或 cli:…（Antigravity 不行） */
  who: string;
  effort?: string;
  costCap?: number | null;
}

/** 「这个项目总是允许」的一条。 */
export interface AllowRule {
  workspaceId: string;
  /** 项目文件夹（家目录写成 ~）；项目不在列表里了为空。 */
  workspace?: string | null;
  /** install-deps（装依赖）、long-running（起常驻进程）。 */
  rule: string;
  createdAt: number;
}

/** 一次请求花的（spend.rs）。 */
export interface SpendPoint {
  at: number;
  /** peck（Peck 循环和 ONE SHOT）或 CLI 引擎 id。 */
  engine: string;
  /** Peck 循环：目录里的模型 id；ONE SHOT：oneshot；CLI：引擎 id。 */
  key: string;
  usd: number | null;
  estimated: boolean;
  input: number | null;
  output: number | null;
  cacheRead: number | null;
}
export interface QuotaPoint { at: number; engine: string; label: string; usedPercent: number; resetsAt: number | null }
export interface SpendOverview { since: number; points: SpendPoint[]; quota: QuotaPoint[] }
