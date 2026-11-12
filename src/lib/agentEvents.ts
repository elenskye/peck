// 统一事件（v0.5 Model Gateway 的契约）：和 src-tauri/src/gateway/event.rs、caps.rs、catalog.rs 一一对应。
// 本地写；云端要加字段先写进 HANDOFF，由本地改两边（Rust 测试会查这里有没有漏掉事件类型）。
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
