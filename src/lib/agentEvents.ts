// 统一事件（v0.5 Model Gateway 的契约）：和 src-tauri/src/gateway/event.rs、caps.rs、catalog.rs 一一对应。
// 改一边就改另一边（Rust 测试会查这里有没有漏掉事件类型）。v0.6 云端加了上下文、记忆、验证、后台几类（两个 Phase 都是云端的）。
//
// 用量的口径：input 是没走缓存的输入，cacheRead / cacheWrite 另算，三者相加才是整个提示；
// output 包括思考，reasoning 只是其中思考的那部分。

export type PermissionLevel = "plan" | "edit" | "auto";

export type TurnEndReason = "done" | "limit" | "refused" | "cancelled" | "interrupted" | "error";

/** 线协议。思考只在「供应商 + 线协议 + 模型」都一样时回放。 */
export type Wire = "messages" | "responses" | "chat" | "gemini";

export interface Origin {
  provider: string;
  wire: Wire;
  model: string;
}

/** 供应商的私有思考数据（签名、密文），原样挂着。 */
export interface OpaqueReasoning {
  origin: Origin;
  data: unknown;
}

/** 读、改、跑、搜、取网页、MCP、其他。 */
export type ToolKind = "read" | "edit" | "run" | "search" | "fetch" | "mcp" | "other";

export type ToolStatus = "ok" | "failed" | "denied" | "cancelled";

export type FileChange = "added" | "modified" | "deleted" | "renamed";

export interface FileDiff {
  /** 相对项目根。 */
  path: string;
  change: FileChange;
  oldPath?: string;
  additions?: number;
  deletions?: number;
  /** unified diff 原文（引擎给了才有）。 */
  patch?: string;
}

export type PlanStatus = "pending" | "in_progress" | "completed";

export interface PlanStep {
  text: string;
  status: PlanStatus;
}

/** 这次允许、总是允许、这次拒绝、总是拒绝。 */
export type ApprovalDecision = "allow_once" | "allow_always" | "deny_once" | "deny_always";

export type ResolvedBy = "user" | "rule" | "engine";

export interface TokenUsage {
  input?: number | null;
  output?: number | null;
  cacheRead?: number | null;
  cacheWrite?: number | null;
  reasoning?: number | null;
  contextUsed?: number | null;
  contextWindow?: number | null;
}

/** 花费从哪来：实报、按价格表算、CLI 估、不知道。 */
export type CostSource = "reported" | "priced" | "estimated" | "unknown";

export interface Cost {
  /** 美元；unknown 时为 null，不是 0。 */
  usd?: number | null;
  source: CostSource;
}

export type UsageScope = "turn" | "cumulative";

export interface QuotaWindow {
  label: string;
  usedPercent?: number | null;
  /** 毫秒时间戳。 */
  resetsAt?: number | null;
}

export type NoticeLevel = "info" | "warn";

export interface McpServer {
  name: string;
  status: string;
}

// ── 上下文、验证、后台（v0.6）──

/** system：Peck 的说明和工具；project：项目说明和记忆；conversation：对话和思考；tools：工具的输出。 */
export interface ContextPart {
  key: "system" | "project" | "conversation" | "tools" | string;
  tokens: number;
}

/** 自动压缩三步：落盘、清旧工具输出、交接摘要。 */
export type CompactStep = "spill" | "clear" | "summary";

/** edit：改了文件后台先跑；end：模型停下后收尾前跑；goal：目标模式。 */
export type VerifyTrigger = "edit" | "end" | "goal";

/** stale：跑的时候文件又变了，结果不算。 */
export type VerifyStatus = "passed" | "failed" | "stale" | "cancelled";

/** passed：都过了不再叫模型；fixing：交回模型修；stopped：停下。 */
export type VerifyOutcome = "passed" | "fixing" | "stopped";

export type BackgroundKind = "command" | "server";
export type BackgroundStatus = "running" | "exited" | "stopped";

// ── 能力标记（caps.rs）──

export type ToolOwner = "peck" | "engine";
export type ApprovalMode = "none" | "preset" | "live";
export type UsageGranularity = "per_turn" | "cumulative" | "none";

export interface AgentCaps {
  streaming: boolean;
  reasoningReplay: boolean;
  tools: ToolOwner;
  approvals: ApprovalMode;
  askUser: boolean;
  steer: boolean;
  interrupt: boolean;
  resume: boolean;
  mcpInject: boolean;
  usage: UsageGranularity;
  cost: CostSource;
  quota: boolean;
  switchModel: boolean;
  needsTty: boolean;
  /** 只做直接调用（Antigravity）：不进 ONE SHOT、接力、委派、审查、定时任务、自动修。 */
  directOnly: boolean;
}

// ── 事件（event.rs 的 EventBody）──

export type EventBody =
  | {
      type: "session.started";
      engine: string;
      model?: string | null;
      caps: AgentCaps;
      tools: string[];
      mcp: McpServer[];
      sessionRef?: string | null;
      version?: string | null;
    }
  | {
      type: "session.config_changed";
      model?: string | null;
      permission?: PermissionLevel | null;
      effort?: string | null;
    }
  | { type: "turn.started"; turnId: string }
  | { type: "turn.completed"; turnId: string; reason: TurnEndReason; error?: string | null }
  | { type: "message.delta"; itemId: string; text: string }
  | { type: "message.completed"; itemId: string; text: string }
  | { type: "reasoning.delta"; itemId: string; text: string }
  | { type: "reasoning.completed"; itemId: string; text: string; opaque?: OpaqueReasoning | null }
  | {
      type: "tool_call.started";
      callId: string;
      /** 工具原名（read_file、Bash、commandExecution……）。 */
      name: string;
      kind: ToolKind;
      /** 给人看的一行。 */
      title: string;
      input: unknown;
    }
  | { type: "tool_call.output_delta"; callId: string; text: string }
  | {
      type: "tool_call.completed";
      callId: string;
      status: ToolStatus;
      output?: string | null;
      exitCode?: number | null;
      diff?: FileDiff[];
      durationMs?: number | null;
    }
  | { type: "diff.updated"; files: FileDiff[] }
  | { type: "plan.updated"; steps: PlanStep[]; explanation?: string | null }
  | {
      type: "approval.requested";
      requestId: string;
      callId?: string | null;
      kind: ToolKind;
      title: string;
      detail?: string | null;
      /** 高危：不管哪档都弹卡片。 */
      highRisk: boolean;
      options: ApprovalDecision[];
    }
  | { type: "approval.resolved"; requestId: string; decision: ApprovalDecision; by: ResolvedBy }
  | {
      type: "user_input.requested";
      requestId: string;
      question: string;
      options: string[];
      freeText: boolean;
    }
  | { type: "user_input.resolved"; requestId: string; answer: string }
  | { type: "subagent.started"; subagentId: string; parentCallId?: string | null; title: string }
  | { type: "subagent.completed"; subagentId: string; status: ToolStatus }
  | { type: "usage.updated"; usage: TokenUsage; cost: Cost; scope: UsageScope }
  | { type: "quota.updated"; provider: string; windows: QuotaWindow[] }
  // ── 上下文（v0.6）──
  | { type: "context.updated"; used: number; window?: number | null; compactAt?: number | null; parts: ContextPart[] }
  | { type: "context.compacted"; step: CompactStep; before: number; after: number; text: string; summary?: string | null }
  | { type: "memory.proposed"; notes: string[] }
  | { type: "memory.reviewed"; saved: string[]; skipped: string[] }
  // ── 验证和后台（v0.6）──
  | { type: "verify.started"; checkId: string; name: string; command: string; trigger: VerifyTrigger; attempt: number }
  | { type: "verify.completed"; checkId: string; name: string; status: VerifyStatus; exitCode?: number | null; durationMs?: number | null; output?: string | null }
  | { type: "verify.round"; attempt: number; maxAttempts: number; outcome: VerifyOutcome; reason?: string | null; goal: boolean }
  | { type: "background.updated"; id: string; name: string; command: string; kind: BackgroundKind; status: BackgroundStatus; exitCode?: number | null }
  | { type: "notice"; level: NoticeLevel; text: string }
  | { type: "retry"; attempt: number; delayMs: number; reason: string }
  | { type: "error"; message: string; fatal: boolean }
  /** 不认识的事件：原件在事件日志里。 */
  | { type: "raw"; source: string; kind?: string | null };

export type EventType = EventBody["type"];

/** 一条事件：seq 在一个任务里从 0 递增；子任务里的带 subagentId。 */
export type AgentEvent = EventBody & {
  seq: number;
  at: number;
  subagentId?: string;
};

/** 控制通道：界面 → 引擎（mod.rs 的 Control）。 */
export type Control =
  | { type: "approve"; requestId: string; decision: ApprovalDecision }
  | { type: "answer"; requestId: string; answer: string }
  | { type: "append"; text: string }
  | { type: "interrupt" };

// ── 模型列表（catalog.rs 的 ModelEntry，IPC gateway_models）──

export type Thinking = "never" | "switch" | "always";

export interface PriceTier {
  aboveTokens: number;
  input: number;
  output: number;
  cacheRead?: number | null;
  cacheWrite?: number | null;
}

/** 每百万 tokens 的美元价格。 */
export interface Price {
  input: number;
  output: number;
  cacheRead?: number | null;
  cacheWrite?: number | null;
  tiers?: PriceTier[];
}

export type EntryGroup = "peck" | "cli" | "flow";

export interface ModelEntry {
  /** api:anthropic/claude-opus-5-5、cli:codex、flow:muse>grok。发任务时的「谁来干」。 */
  id: string;
  group: EntryGroup;
  label: string;
  provider: string;
  providerLabel: string;
  available: boolean;
  /** 没配 Key、没检测到、先跑 gcloud auth application-default login…… */
  unavailableReason?: string | null;
  directOnly: boolean;
  thinking?: Thinking | null;
  efforts: string[];
  contextWindow?: number | null;
  price?: Price | null;
  caps: AgentCaps;
  version?: string | null;
  /** CLI 引擎当前用的账号（设置里起的名字）。API 和没配账号的为空。 */
  account?: string | null;
  /** 转换器按哪个版本的 CLI 写的；和 version 里的版本号不一样就提示「格式可能变了」。 */
  testedVersion?: string | null;
  /** CLI 在哪找到的（家目录写成 ~）。 */
  location?: string | null;
}

// ── Gateway 设置（config.rs，IPC gateway_config_get / gateway_config_set）──

export interface VertexConfig {
  projectId: string;
  /** global 或 us-east5 这样的区域。 */
  location: string;
}

export interface GatewayConfig {
  vertex: VertexConfig;
  /** 放 CLI 的目录；空：照 PATH 找。 */
  cliDir: string;
}

/** 设置页读的：Gateway 设置 + gcloud 的 ADC 在不在。 */
export interface GatewaySettings {
  config: GatewayConfig;
  adc: boolean;
}

/** 列表里的「CLI 引擎」能不能进自动化（ONE SHOT、接力、委派……）。 */
export function canAutomate(entry: Pick<ModelEntry, "directOnly">): boolean {
  return !entry.directOnly;
}

// ── CLI 账号（accounts.rs，IPC cli_account_*）──
// 一个账号 = 一个独立的配置目录（Claude Code 的 CLAUDE_CONFIG_DIR、Codex 的 CODEX_HOME）。
// 「默认」账号是 CLI 自己的默认位置，和客户端共用；新增的放在 CLI 目录旁边的 accounts/。

export interface CliAccountStatus {
  loggedIn: boolean;
  /** Claude Code 的 max、pro；Codex 是 ChatGPT。 */
  plan?: string | null;
  email?: string | null;
  /** 一句话：已登录（Claude 订阅）、没登录、状态命令超时了…… */
  detail: string;
}

export interface CliAccount {
  /** default 是默认账号。 */
  id: string;
  label: string;
  isDefault: boolean;
  current: boolean;
  /** 没验过为空。 */
  status?: CliAccountStatus | null;
}

export interface EngineAccounts {
  /** claude-code、codex */
  engine: string;
  label: string;
  /** 默认账号和谁共用登录（界面写明）。 */
  defaultNote: string;
  accounts: CliAccount[];
}

export interface CliLoginState {
  running: boolean;
  /** 登录链接：站主复制到自己挑的浏览器里打开（Peck 不自动开浏览器）。 */
  url?: string | null;
  /** 登录命令的输出（Codex 的一次性代码在这里）。 */
  output: string;
  /** 结束了：成功为 true。 */
  ok?: boolean | null;
}

// ── 发任务（Gateway 版，runner.rs，IPC gateway_run_start / gateway_control）──

export interface GatewayRunArgs {
  sessionId: string;
  prompt: string;
  /** 模型列表里的 id：cli:codex、cli:claude-code、cli:muse、cli:grok、cli:antigravity。 */
  who: string;
  /** plan | edit | auto，不填是 edit。 */
  permission?: "plan" | "edit" | "auto";
  /** 引擎里的模型名（不填用引擎默认）。 */
  model?: string;
  effort?: string;
  /** 上一次的会话号（session.started 的 sessionRef），续接用。 */
  resume?: string;
}
