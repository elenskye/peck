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
  /** 系统选文件夹的窗（只返回路径；浏览器里没有）。 */
  async pickFolder(): Promise<string | null> {
    if (isMockMode()) return null;
    return invoke<string | null>("dialog_pick_folder");
  },
  /** Quick Look 的缩略图（Word、Excel 这类，PNG 的 data URL）。只在 Mac 上有。 */
  async filesQuicklook(workspaceId: string, path: string): Promise<string> {
    if (isMockMode()) return mockIpc.filesQuicklook(workspaceId, path);
    return invoke<string>("files_quicklook", { workspaceId, path });
  },
  /** 叫出系统的 Quick Look 窗口。 */
  async filesQuicklookOpen(workspaceId: string, path: string): Promise<void> {
    if (isMockMode()) return mockIpc.filesQuicklookOpen(workspaceId, path);
    return invoke<void>("files_quicklook_open", { workspaceId, path });
  },
  /** CHANGES：拒绝一块改动（在现在的文件里换回任务前的样子）。 */
  async changesHunkRevert(runId: string, stepId: string | undefined, filePath: string, hunk: number): Promise<void> {
    if (isMockMode()) return mockIpc.changesHunkRevert(runId, stepId, filePath, hunk);
    return invoke<void>("changes_hunk_revert", { runId, stepId, filePath, hunk });
  },
  /** 独立工作区：搬回项目（有冲突、没让照样搬时什么都不动，返回冲突的文件）。 */
  async worktreeApply(runId: string, force = false): Promise<WorktreeApply> {
    if (isMockMode()) return mockIpc.worktreeApply(runId, force);
    return invoke<WorktreeApply>("worktree_apply", { runId, force });
  },
  async worktreeDiscard(runId: string): Promise<RunRecord> {
    if (isMockMode()) return mockIpc.worktreeDiscard(runId);
    return invoke<RunRecord>("worktree_discard", { runId });
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
  /** 菜单栏图标的状态；`count` 是在跑几个（两个以上图标旁边写个数，v0.7）。 */
  async setActivity(activity: Activity, count = 0): Promise<void> {
    if (isMockMode()) return;
    return invoke<void>("shell_set_activity", { activity, count });
  },
  async notify(title: string, body: string): Promise<void> {
    if (isMockMode()) return mockIpc.notify(title, body);
    return invoke<void>("notify", { title, body });
  },

  // ── Model Gateway（v0.5 契约，src-tauri/src/gateway/）──
  /** 模型列表：API 模型（配了 Key 的能用）、CLI 引擎（检测到的能用）、接力。 */
  /** 一家 CLI 能选的模型（从 CLI 自己那里读，存着；refresh 才重读）。 */
  async cliModels(engine: string, refresh = false): Promise<CliModels> {
    if (isMockMode()) return mockCliModels(engine);
    return invoke<CliModels>("cli_models", { engine, refresh });
  },
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
  // ── 上下文（v0.6 Phase 7）：记忆 ──
  /** 这个项目站主批过的记忆。 */
  async memoryList(workspaceId: string): Promise<MemoryNote[]> {
    if (isMockMode()) return mockIpc.memoryList(workspaceId);
    return invoke<MemoryNote[]>("memory_list", { workspaceId });
  },
  async memoryDelete(workspaceId: string, id: string): Promise<MemoryNote[]> {
    if (isMockMode()) return mockIpc.memoryDelete(workspaceId, id);
    return invoke<MemoryNote[]>("memory_delete", { workspaceId, id });
  },
  /** 「想记住这几条」：keep 存进项目记忆，skip 不要；返回追加进事件日志的那条 memory.reviewed。 */
  async memoryReview(runId: string, keep: string[], skip: string[]): Promise<AgentEvent> {
    if (isMockMode()) return mockIpc.memoryReview(runId, keep, skip);
    return invoke<AgentEvent>("memory_review", { runId, keep, skip });
  },

  // ── 验证和后台（v0.6 Phase 8）──
  /** .peck/verify.toml：有没有、内容、写错了没有、猜的、站主说过不要没有。 */
  async verifyGet(workspaceId: string): Promise<VerifyState> {
    if (isMockMode()) return mockIpc.verifyGet(workspaceId);
    return invoke<VerifyState>("verify_get", { workspaceId });
  },
  /** 校验后写进 .peck/verify.toml。 */
  async verifySave(workspaceId: string, text: string): Promise<VerifyState> {
    if (isMockMode()) return mockIpc.verifySave(workspaceId, text);
    return invoke<VerifyState>("verify_save", { workspaceId, text });
  },
  /** 不要猜的那几条：中栏不再问。 */
  async verifyDismiss(workspaceId: string): Promise<VerifyState> {
    if (isMockMode()) return mockIpc.verifyDismiss(workspaceId);
    return invoke<VerifyState>("verify_dismiss", { workspaceId });
  },
  /** 这个项目的后台命令和常驻 dev server。 */
  async backgroundList(workspaceId: string): Promise<BgInfo[]> {
    if (isMockMode()) return mockIpc.backgroundList(workspaceId);
    return invoke<BgInfo[]>("background_list", { workspaceId });
  },
  async backgroundOutput(id: string): Promise<string> {
    if (isMockMode()) return mockIpc.backgroundOutput(id);
    return invoke<string>("background_output", { id });
  },
  async backgroundStop(id: string): Promise<void> {
    if (isMockMode()) return mockIpc.backgroundStop(id);
    return invoke<void>("background_stop", { id });
  },
  /** 起 verify.toml 里的一个 dev server（已经在跑就返回它）。 */
  async serverStart(workspaceId: string, name: string): Promise<BgInfo> {
    if (isMockMode()) return mockIpc.serverStart(workspaceId, name);
    return invoke<BgInfo>("server_start", { workspaceId, name });
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
  /** 目标模式：要过的验证命令的名字（* 是全部）。 */
  goal?: string[];
  /** 在独立工作区（git worktree）里做（v0.7）。 */
  worktree?: boolean;
}

export interface WorktreeApply { applied: string[]; conflicts: string[]; record: RunRecord }

/** 站主批过的一条记忆（harness/history.rs）。 */
export interface MemoryNote { id: string; text: string; sourceRun?: string | null; createdAt: number }

/** .peck/verify.toml（harness/verify.rs）。TOML 里是 [[check]]、[[server]]。 */
export interface VerifyCheck { name: string; run: string; when?: string[]; timeout?: number | null }
export interface VerifyServer { name: string; run: string; ready?: string | null; autostart: boolean }
export interface VerifyConfig { check: VerifyCheck[]; server: VerifyServer[] }
export interface VerifyState {
  exists: boolean;
  text?: string | null;
  config?: VerifyConfig | null;
  /** 写错了：原因。 */
  error?: string | null;
  /** 没有文件时猜的（TOML 原文）。 */
  guessed?: string | null;
  dismissed: boolean;
  /** 文件和站主确认过的那一份一样；不一样（项目自带的、改过的）就不替模型跑、不起 server。 */
  approved: boolean;
}

/** 后台命令（harness/background.rs）。 */
export interface BgInfo {
  id: string;
  name: string;
  command: string;
  kind: "command" | "server";
  status: "running" | "exited" | "stopped";
  exitCode?: number | null;
  ready: boolean;
  startedAt: number;
  endedAt?: number | null;
  owner?: string | null;
  log: string;
}

export interface OneshotArgs {
  workspaceId: string;
  prompt: string;
  /** api:… 或 cli:…（Antigravity 不行） */
  who: string;
  /** CLI 引擎里的模型名（AGENTS 里选的；不填用 CLI 默认）。 */
  model?: string;
  effort?: string;
  costCap?: number | null;
}

/** CLI 能选的一个模型（`gateway/cli_models.rs`）。 */
export interface CliModel { id: string; label: string; efforts: string[]; defaultEffort: string | null; isDefault: boolean }
export interface CliModels { engine: string; models: CliModel[]; source: "cli" | "builtin" | "mock"; readAt: number | null; error: string | null }

/** 浏览器演示：照夹具里录到的。 */
function mockCliModels(engine: string): Promise<CliModels> {
  const m = (id: string, efforts: string[], isDefault = false): CliModel => ({ id, label: id, efforts, defaultEffort: null, isDefault });
  const list: Record<string, CliModel[]> = {
    "claude-code": [["claude-opus-5-5", "Opus 5.5"], ["claude-fable-5-2", "Fable 5.2"], ["claude-sonnet-5-5", "Sonnet 5.5"], ["claude-haiku-5-5", "Haiku 5.5"], ["claude-opus-4-8", "Opus 4.8"]].map(([id, label], i) => ({ ...m(id, ["low", "medium", "high", "xhigh", "max"], i === 0), label })),
    codex: [m("gpt-6.1-sol", ["low", "medium", "high", "xhigh", "max"], true), m("gpt-6-luna", ["low", "medium", "high"])],
    muse: [m("muse-wonder-1.0", ["minimal", "low", "medium", "high", "max"], true), m("muse-ember-1.0", ["low", "medium", "high"]), m("muse-spark-1.3", ["low", "medium", "high"])],
    grok: [m("grok-4.6", ["medium", "high", "xhigh"], true), m("grok-4.5", ["medium", "high", "xhigh"])],
    antigravity: [m("gemini-4-argon", ["low", "medium", "high"], true), m("gemini-3.8-flash", ["low", "medium", "high"]), m("claude-opus-5-5", ["low", "medium", "high"])],
  };
  return Promise.resolve({ engine, models: list[engine] ?? [], source: "mock", readAt: Date.now(), error: null });
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
