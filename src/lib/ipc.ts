import { Channel, invoke } from "@tauri-apps/api/core";
import { mockIpc } from "@/dev/mockIpc";
import type { AgentEvent, CliAccountStatus, CliLoginState, Control, EngineAccounts, GatewayConfig, GatewaySettings, GatewayRunArgs, ModelEntry, PriceTier } from "@/lib/agentEvents";
import type {
  Activity,
  FileListing,
  FilePreview,
  Permission,
  ProjectOverview,
  RunRecord,
  Session,
  TermEvent,
  TermState,
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
  async workspaceRelocate(id: string): Promise<Workspace | null> {
    if (isMockMode()) return mockIpc.workspaceRelocate(id);
    return invoke<Workspace | null>("workspace_relocate", { id });
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
  /** 归档一个任务：不在左栏列，记录全留着（在跑的不能归档）。 */
  async sessionArchive(id: string): Promise<void> {
    if (isMockMode()) return mockIpc.sessionArchive(id);
    return invoke<void>("session_archive", { id });
  },
  /** 拿回左栏；项目移出列表了就一起放回。返回这个项目。 */
  async sessionUnarchive(id: string): Promise<Workspace> {
    if (isMockMode()) return mockIpc.sessionUnarchive(id);
    return invoke<Workspace>("session_unarchive", { id });
  },
  /** 一个项目里不在跑的任务全归档，返回归档了几个。 */
  async workspaceArchiveTasks(id: string): Promise<number> {
    if (isMockMode()) return mockIpc.workspaceArchiveTasks(id);
    return invoke<number>("workspace_archive_tasks", { id });
  },
  /** 设置 → ARCHIVE：归档了的任务，按项目分组（移出列表的项目也在）。 */
  async archiveList(): Promise<ProjectOverview[]> {
    if (isMockMode()) return mockIpc.archiveList();
    return invoke<ProjectOverview[]>("archive_list");
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

  async sessionCreate(workspaceId: string, title?: string): Promise<Session> {
    if (isMockMode()) {
      return mockIpc.sessionCreate(workspaceId, title);
    }
    return invoke<Session>("session_create", { workspaceId, title });
  },

  async sessionGetRuns(sessionId: string): Promise<RunRecord[]> {
    if (isMockMode()) {
      return mockIpc.sessionGetRuns(sessionId);
    }
    return invoke<RunRecord[]>("session_get_runs", { sessionId });
  },

  async runCancel(runId: string): Promise<void> {
    if (isMockMode()) {
      return mockIpc.runCancel(runId);
    }
    return invoke<void>("run_cancel", { runId });
  },

  /** 这个任务改的某个文件的 Diff（统一格式）：FILES 标出加的行。 */
  async changesGetDiff(runId: string, filePath: string): Promise<string> {
    if (isMockMode()) return mockIpc.changesGetDiff(runId, filePath);
    return invoke<string>("changes_get_diff", { runId, filePath });
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
  /** 独立工作区：搬回项目（有冲突、没让照样搬时什么都不动，返回冲突的文件）。 */
  async worktreeApply(runId: string, force = false): Promise<WorktreeApply> {
    if (isMockMode()) return mockIpc.worktreeApply(runId, force);
    return invoke<WorktreeApply>("worktree_apply", { runId, force });
  },
  async worktreeDiscard(runId: string): Promise<RunRecord> {
    if (isMockMode()) return mockIpc.worktreeDiscard(runId);
    return invoke<RunRecord>("worktree_discard", { runId });
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
  /** API 价格表（v0.10 Refine）：内置的 + 站主改的 + 刷新来的、汇率。 */
  async pricesGet(): Promise<PriceBook> {
    if (isMockMode()) return mockIpc.pricesGet();
    return invoke<PriceBook>("prices_get");
  },
  /** 改一行（没有的就是加）、退回一行、改汇率：一次一样。 */
  async pricesSet(args: { row?: PriceRow; reset?: string; fxRate?: number }): Promise<PriceBook> {
    if (isMockMode()) return mockIpc.pricesSet(args);
    return invoke<PriceBook>("prices_set", { row: args.row ?? null, reset: args.reset ?? null, fxRate: args.fxRate ?? null });
  },
  /** 从 models.dev 取一次美元价（人民币的不刷新）。 */
  async pricesRefresh(): Promise<PriceRefresh> {
    if (isMockMode()) return mockIpc.pricesRefresh();
    return invoke<PriceRefresh>("prices_refresh");
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

  // ── Peck Loop（v0.5 Phase 6，src-tauri/src/harness/）：「谁来干」是 api:<供应商>/<模型> 的走这里 ──
  /** 返回任务 id；过程是统一事件（和 CLI 引擎同一套）。审批、回答、打断照旧用 gatewayControl。 */
  async harnessRunStart(args: HarnessRunArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.harnessRunStart(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("harness_run_start", { ...args, channel });
  },
  /** 外部 MCP 服务器（v0.8，设置里配）。 */
  async mcpServersGet(): Promise<McpServerConfig[]> {
    if (isMockMode()) return mockIpc.mcpServersGet();
    return invoke<McpServerConfig[]>("mcp_servers_get");
  },
  async mcpServersSet(servers: McpServerConfig[]): Promise<McpServerConfig[]> {
    if (isMockMode()) return mockIpc.mcpServersSet(servers);
    return invoke<McpServerConfig[]>("mcp_servers_set", { servers });
  },
  /** 起一次、列工具、停掉。 */
  async mcpServerTest(server: McpServerConfig): Promise<McpToolInfo[]> {
    if (isMockMode()) return mockIpc.mcpServerTest(server);
    return invoke<McpToolInfo[]>("mcp_server_test", { server });
  },
  /** 一台服务器的一个环境变量存进钥匙串（value 空着是删掉）。 */
  async mcpSecretSet(server: string, name: string, value: string | null): Promise<McpServerConfig[]> {
    if (isMockMode()) return mockIpc.mcpSecretSet(server, name, value);
    return invoke<McpServerConfig[]>("mcp_secret_set", { server, name, value });
  },
  /** 输入框打 `/`：命令文件和 Skills（v0.10 C2）。 */
  async slashList(workspaceId: string): Promise<SlashList> {
    if (isMockMode()) return mockIpc.slashList(workspaceId);
    return invoke<SlashList>("slash_list", { workspaceId });
  },
  /** 批一条 Skill 提议（v0.10 C3）：save（draft 是卡片上改过的）或 skip；返回追加的那条 skill.reviewed。 */
  async skillReview(runId: string, proposalId: string, decision: "save" | "skip", draft?: SkillDraft | null): Promise<AgentEvent> {
    if (isMockMode()) return mockIpc.skillReview(runId, proposalId, decision, draft ?? null);
    return invoke<AgentEvent>("skill_review", { runId, proposalId, decision, draft: draft ?? null });
  },
  /** 项目设置的 SKILLS：读到的、用了几次、最近一次、旧版几份。 */
  async skillOverview(workspaceId: string): Promise<SkillRow[]> {
    if (isMockMode()) return mockIpc.skillOverview(workspaceId);
    return invoke<SkillRow[]>("skill_overview", { workspaceId });
  },
  /** 一个 Skill 存过的旧版（新的在前）。 */
  async skillVersions(workspaceId: string, name: string): Promise<SkillVersion[]> {
    if (isMockMode()) return mockIpc.skillVersions(workspaceId, name);
    return invoke<SkillVersion[]>("skill_versions", { workspaceId, name });
  },
  /** 退回一份旧版（现在的也先存一份）；返回新的列表。 */
  async skillRestore(workspaceId: string, name: string, id: string): Promise<SkillRow[]> {
    if (isMockMode()) return mockIpc.skillRestore(workspaceId, name, id);
    return invoke<SkillRow[]>("skill_restore", { workspaceId, name, id });
  },
  // ── 插件（v1.1.0）──
  /** 设置 → PLUGINS：自带的和库里的。 */
  async pluginList(): Promise<PluginList> {
    if (isMockMode()) return mockIpc.pluginList();
    return invoke<PluginList>("plugin_list");
  },
  /** 库里的一项要不要默认每个项目都装（只管以后点开的项目）。 */
  async pluginEveryProject(name: string, on: boolean): Promise<PluginList> {
    if (isMockMode()) return mockIpc.pluginEveryProject(name, on);
    return invoke<PluginList>("plugin_every_project", { name, on });
  },
  /** 点开一个项目：第一次认项目结构，每次补上「每个项目都装」的插件。回这次做了什么。 */
  async projectPrepare(workspaceId: string): Promise<Prepared> {
    if (isMockMode()) return mockIpc.projectPrepare(workspaceId);
    return invoke<Prepared>("project_prepare", { workspaceId });
  },
  /** 项目设置 → PLUGINS：自带的开没开、库里的装没装、有没有新版。 */
  async projectPlugins(workspaceId: string): Promise<ProjectPlugins> {
    if (isMockMode()) return mockIpc.projectPlugins(workspaceId);
    return invoke<ProjectPlugins>("project_plugins", { workspaceId });
  },
  /** INSTALL / UPDATE：把库里的拷进项目（盖掉项目里那一份）。 */
  async pluginInstall(workspaceId: string, name: string): Promise<ProjectPlugins> {
    if (isMockMode()) return mockIpc.pluginInstall(workspaceId, name);
    return invoke<ProjectPlugins>("plugin_install", { workspaceId, name });
  },
  /** REMOVE：删掉项目里那一份，以后不再自动装。 */
  async pluginRemove(workspaceId: string, name: string): Promise<ProjectPlugins> {
    if (isMockMode()) return mockIpc.pluginRemove(workspaceId, name);
    return invoke<ProjectPlugins>("plugin_remove", { workspaceId, name });
  },
  /** 选了一个插件命令：展开成要发出去的这句话。 */
  async slashExpand(workspaceId: string, name: string, args: string): Promise<string> {
    if (isMockMode()) return mockIpc.slashExpand(workspaceId, name, args);
    return invoke<string>("slash_expand", { workspaceId, name, args });
  },
  /** `/compact [重点]`（v1.1.7）：Peck Loop 的手动压缩，落成这个任务的一次新记录（叫一次模型写交接摘要）。返回任务 id。 */
  async harnessCompact(args: CompactArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.harnessCompact(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("harness_compact", { ...args, channel });
  },
  /** `/compact [重点]` 转给 CLI（v1.1.7）：续上 `resume` 那个会话让它自己压。返回任务 id。 */
  async gatewayCompact(args: CompactArgs & { who: string; model?: string; resume: string }, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.gatewayCompact(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("gateway_compact", { ...args, channel });
  },
  /** 项目设置：这个项目的 Peck Loop 会用到的 MCP（设置里的 + 项目 .mcp.json 里的）。 */
  async projectMcpList(workspaceId: string): Promise<ProjectMcpView> {
    if (isMockMode()) return mockIpc.projectMcpList(workspaceId);
    return invoke<ProjectMcpView>("project_mcp_list", { workspaceId });
  },
  async projectMcpTest(workspaceId: string, name: string): Promise<McpToolInfo[]> {
    if (isMockMode()) return mockIpc.projectMcpTest(workspaceId, name);
    return invoke<McpToolInfo[]>("project_mcp_test", { workspaceId, name });
  },
  /** ONE SHOT：只读回答一句（API 模型或 CLI）；不占任务、不锁终端。返回 id（oneshot-…）。 */
  async harnessOneshot(args: OneshotArgs, onEvent: (event: AgentEvent) => void): Promise<string> {
    if (isMockMode()) return mockIpc.harnessOneshot(args, onEvent);
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("harness_oneshot", { ...args, channel });
  },
  // ── 定时任务：在对话里设、改、取消（模型的工具）；界面只列出来看，错过的弹卡片 ──
  async scheduleList(): Promise<Schedule[]> {
    if (isMockMode()) return mockIpc.scheduleList();
    return invoke<Schedule[]>("schedule_list");
  },
  async scheduleDelete(id: string): Promise<void> {
    if (isMockMode()) return mockIpc.scheduleDelete(id);
    return invoke<void>("schedule_delete", { id });
  },
  /** 错过的那张卡片：run 为真是 RUN NOW（补跑一次，返回发出去的那一次），否则是 SKIP。 */
  async scheduleMissed(id: string, run: boolean): Promise<ScheduleFired | null> {
    if (isMockMode()) return mockIpc.scheduleMissed(id, run);
    return invoke<ScheduleFired | null>("schedule_missed", { id, run });
  },
  /** 定时任务发出去的任务：发了一次（fired）、过程里的每条事件（event）；定时任务变了（changed：设了、取消了、跑了、错过了）。
   *  返回取消监听的函数。 */
  async onSchedule(onFired: (f: ScheduleFired) => void, onEvent: (e: ScheduleEvent) => void, onChanged: () => void): Promise<() => void> {
    if (isMockMode()) return mockIpc.onSchedule(onFired, onEvent, onChanged);
    const { listen } = await import("@tauri-apps/api/event");
    const a = await listen<ScheduleFired>("schedule-fired", (e) => onFired(e.payload));
    const b = await listen<ScheduleEvent>("schedule-event", (e) => onEvent(e.payload));
    const c = await listen("schedules-changed", () => onChanged());
    return () => { a(); b(); c(); };
  },
  /** 预览面板（v0.8）：给这个本机地址起代理，返回面板要开的地址。 */
  async previewStart(url: string): Promise<string> {
    if (isMockMode()) return mockIpc.previewStart(url);
    return invoke<string>("preview_start", { url });
  },
  /** 截图、附图（Peck 输出目录里的）→ data URL。 */
  async harnessImageRead(path: string): Promise<string> {
    if (isMockMode()) return mockIpc.harnessImageRead(path);
    return invoke<string>("harness_image_read", { path });
  },
  /** 用编辑器（app 空着用系统默认）或访达打开项目里的文件（v0.8）。 */
  /** 用系统浏览器打开一个网址（v1.1.7，读网页那一步）。只认 http / https。 */
  async openUrl(url: string): Promise<void> {
    if (isMockMode()) { window.open(url, "_blank", "noopener"); return; }
    return invoke<void>("open_url", { url });
  },
  async openPath(workspaceId: string, path: string, how: "editor" | "finder", app?: string): Promise<void> {
    if (isMockMode()) return mockIpc.openPath(workspaceId, path, how, app);
    return invoke<void>("open_path", { workspaceId, path, how, app: app || null });
  },
  /** 打开落盘的完整命令输出。 */
  async harnessOutputRead(path: string): Promise<string> {
    if (isMockMode()) return mockIpc.harnessOutputRead(path);
    return invoke<string>("harness_output_read", { path });
  },
  /** 这个月 Peck Loop（API）花了多少。 */
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
  async memoryOverview(): Promise<MemoryGroup[]> {
    if (isMockMode()) return mockIpc.memoryOverview();
    return invoke<MemoryGroup[]>("memory_overview");
  },
  /** 自己记一条；返回这个项目的记忆。 */
  async memoryAdd(workspaceId: string, text: string): Promise<MemoryNote[]> {
    if (isMockMode()) return mockIpc.memoryAdd(workspaceId, text);
    return invoke<MemoryNote[]>("memory_add", { workspaceId, text });
  },
  async memoryUpdate(workspaceId: string, id: string, text: string): Promise<MemoryNote[]> {
    if (isMockMode()) return mockIpc.memoryUpdate(workspaceId, id, text);
    return invoke<MemoryNote[]>("memory_update", { workspaceId, id, text });
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

  // ── 后台命令 ──
  /** 这个项目的后台命令和留着的 dev server。 */
  async backgroundList(workspaceId: string): Promise<BgInfo[]> {
    if (isMockMode()) return mockIpc.backgroundList(workspaceId);
    return invoke<BgInfo[]>("background_list", { workspaceId });
  },
  async backgroundStop(id: string): Promise<void> {
    if (isMockMode()) return mockIpc.backgroundStop(id);
    return invoke<void>("background_stop", { id });
  },
  /** 换 Dock 里的 App 图标（j2 / a3）。浏览器里不做事。 */
  async appIconSet(icon: "j2" | "a3"): Promise<void> {
    if (isMockMode()) return;
    return invoke<void>("app_icon_set", { icon });
  },
};

export interface CompactArgs {
  sessionId: string;
  /** 要重点留住什么（只有 Peck Loop、Claude Code 收）。 */
  focus?: string;
  permission?: Permission;
  effort?: string;
  costCap?: number | null;
}

export interface HarnessRunArgs {
  sessionId: string;
  prompt: string;
  /** api:<供应商>/<模型> */
  who: string;
  permission?: Permission;
  effort?: string;
  /** 每月花费上限（美元）；空着不管。 */
  costCap?: number | null;
  /** 在独立工作区（git worktree）里做（v0.7）。 */
  worktree?: boolean;
  /** 这一句附的图（v0.8）。 */
  images?: ImageInput[];
}

/** 定时任务（schedule.rs）。时间按本机时区。days：1 是周一 … 7 是周日。 */
export type ScheduleWhen =
  | { kind: "once"; date: string; time: string }
  | { kind: "daily"; time: string; days?: number[] }
  | { kind: "monthly"; day: number; time: string }
  | { kind: "interval"; hours: number };

export interface Schedule {
  id: string;
  workspaceId: string;
  title: string;
  prompt: string;
  who: string;
  model?: string | null;
  effort?: string | null;
  permission: Permission;
  when: ScheduleWhen;
  offsetMin: number;
  enabled: boolean;
  nextAt?: number | null;
  lastRunAt?: number | null;
  lastSession?: string | null;
  lastNote?: string | null;
  createdAt?: number;
  /** 接在哪个对话里跑；空着是每次开新任务。 */
  session?: string | null;
  /** 错过的那一次原本该几点跑（等站主点卡片）。 */
  missedAt?: number | null;
}

export interface ScheduleFired { scheduleId: string; workspaceId: string; sessionId: string; runId: string; prompt: string }
export interface ScheduleEvent { runId: string; workspaceId: string; sessionId: string; prompt: string; event: AgentEvent }

/** 附的图（base64，不带 data: 前缀）。 */
export interface ImageInput {
  mediaType: string;
  data: string;
}

export interface McpServerConfig {
  name: string;
  command: string;
  args: string[];
  env: { name: string; value: string }[];
  enabled: boolean;
  /** 值放在钥匙串里的环境变量（v0.10，只有名字）。 */
  secretEnv?: string[];
  /** 从项目 .mcp.json 读来的：那一项的指纹。 */
  project?: string | null;
}

export type ToolHint = "read" | "write" | "destructive";

/** 一个插件（或库里一个单独的 Skill），v1.1.0。 */
export interface PluginInfo {
  name: string;
  /** 界面上的名字。 */
  title: string;
  version: string;
  description: string;
  kind: "plugin" | "skill";
  /** builtin：嵌在程序里；library：数据目录的插件库；project：装进项目的那一份。 */
  source: "builtin" | "library" | "project";
  /** 库里的：默认每个项目都装。 */
  everyProject: boolean;
  skills: Array<{ name: string; description: string }>;
  /** name 是全名 `<插件名>:<命令>`。 */
  commands: Array<{ name: string; description: string; hint?: string | null }>;
  /** known：Peck 有这个工具。 */
  tools: Array<{ name: string; description: string; known: boolean }>;
  /** kind：peck（问模型一句 ask，再由 Peck 跑 run）、command（任意脚本，Peck 不跑）。 */
  hooks: Array<{ event: string; kind: string; ask?: string | null; run?: string | null }>;
  mcp: Array<{ name: string; command: string }>;
}
export interface PluginList { builtin: PluginInfo[]; library: PluginInfo[] }
/** 项目设置 → PLUGINS 的一行。 */
export interface ProjectPlugin extends PluginInfo {
  /** 自带的：这个项目里关掉了。 */
  off: boolean;
  /** 项目里装的是哪一版（没装是空）。 */
  installed?: string | null;
  /** 库里现在的版本（库里没有了是空）。 */
  latest?: string | null;
  /** 库里的比项目里的新。 */
  update: boolean;
}
export interface ProjectPlugins { builtin: ProjectPlugin[]; library: ProjectPlugin[]; skillsDir: string }
/** 点开一个项目时做了什么。 */
export interface Prepared { created: string[]; installed: string[] }
/** 一个插件带的斜杠命令（v1.1.0）：name 是全名 `<插件名>:<命令>`，plugin 是插件在界面上的名字。 */
export interface CommandFile { name: string; description: string; hint?: string | null; plugin: string }
/** 一个 Skill（SKILL.md，v0.10 C2）。 */
/** scope：项目的 .claude/skills、项目的 skills/、Peck 自带插件的（plugin 是哪个插件）。 */
export interface SkillInfo { name: string; description: string; scope: "project" | "project-skills" | "peck"; plugin?: string | null; off: boolean }
export interface SlashList { commands: CommandFile[]; skills: SkillInfo[] }
/** 一条 Skill 的草稿（v0.10 C3）：模型提的，或者卡片上改过的。 */
export interface SkillDraft { name: string; description: string; body: string; scope: Exclude<SkillInfo["scope"], "peck"> }
/** 项目设置里的一行 Skill（v0.10 C3）。 */
export interface SkillRow { name: string; description: string; scope: SkillInfo["scope"]; plugin?: string | null; off: boolean; loads: number; lastUsed?: number | null; changedAt?: number | null; stale: boolean; versions: number }
/** 一份旧版。 */
export interface SkillVersion { id: string; at: number; description: string; lines: number }
/** 项目设置里的一行 MCP（v0.10 A2）。 */
export interface ProjectMcpRow {
  name: string;
  command: string;
  args: string[];
  source: "project" | "settings";
  on: boolean;
  offInSettings: boolean;
  overridden: boolean;
  allowed: boolean;
  tools: [string, ToolHint][];
}

export interface ProjectMcpView { rows: ProjectMcpRow[]; skipped: [string, string][]; error?: string | null }

export interface McpToolInfo {
  server: string;
  name: string;
  description: string;
  readOnly: boolean;
  destructive: boolean;
}

export interface WorktreeApply { applied: string[]; conflicts: string[]; record: RunRecord }

/** 站主批过的一条记忆（harness/history.rs）。 */
export interface MemoryNote { id: string; text: string; sourceRun?: string | null; createdAt: number }
/** 设置 → MEMORY（v1.1.4）的一块：一个项目和它的记忆（移出列表的项目有记忆才列）。 */
export interface MemoryGroup { workspaceId: string; name: string; path: string; hidden: boolean; notes: MemoryNote[] }

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
  /** 输出里头一个本机地址（预览面板用，v0.8）。 */
  url?: string | null;
}

export interface OneshotArgs {
  workspaceId: string;
  prompt: string;
  /** api:… 或 cli:… */
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
  /** peck（Peck Loop 和 ONE SHOT）或 CLI 引擎 id。 */
  engine: string;
  /** Peck Loop：目录里的模型 id；ONE SHOT：oneshot；CLI：引擎 id。 */
  key: string;
  usd: number | null;
  estimated: boolean;
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite?: number | null;
}

// ── API 价格表（prices.rs）──
export type Currency = "USD" | "CNY";
/** 一个模型的价格：每百万 tokens，币种见 currency。 */
export interface PriceRow {
  model: string;
  label: string;
  vendor: string;
  currency: Currency;
  input: number;
  output: number;
  cacheRead?: number | null;
  cacheWrite?: number | null;
  tiers?: PriceTier[];
  /** 高峰倍数（DeepSeek：北京时间工作日 9–12、14–18 点）。 */
  peak?: number | null;
}
export interface PriceView extends PriceRow { origin: "builtin" | "online" | "edited" | "custom" }
export interface PriceVendor { id: string; label: string; modelsDev?: string | null }
export interface PriceBook { fxRate: number; defaultFx: number; vendors: PriceVendor[]; rows: PriceView[]; builtinDate: string; refreshedAt: number | null }
export interface PriceRefresh { book: PriceBook; report: { changed: string[]; same: number; missing: string[]; skippedCny: number } }
export interface QuotaPoint { at: number; engine: string; label: string; usedPercent: number; resetsAt: number | null }
export interface SpendOverview { since: number; points: SpendPoint[]; quota: QuotaPoint[] }
