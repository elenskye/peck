// `?mock` 用的 Gateway 假数据：都从仓库里的夹具来，和 Rust 那边同一份（src-tauri/tests/fixtures/gateway/）。
// - MOCK_MODEL_ENTRIES：模型列表（Rust 的 catalog 测试生成，全部当能用）。
// - MOCK_ENGINE_REPLAYS：真实录下来的 CLI 引擎过程，转成统一事件；回放时碰到 approval / user_input 要停下等回答。
//   每条可能带 subagentId（子任务里的事件）。
import type { EventBody, ModelEntry } from "@/lib/agentEvents";

/** 夹具里的一条：AgentEvent 去掉 seq、at。 */
export type ReplayEvent = EventBody & { subagentId?: string };
import modelsRaw from "../../src-tauri/tests/fixtures/gateway/models-mock.json?raw";
import codexRaw from "../../src-tauri/tests/fixtures/gateway/codex-small-task/events.jsonl?raw";
import claudeRaw from "../../src-tauri/tests/fixtures/gateway/claude-code-small-task/events.jsonl?raw";

function jsonl(text: string): ReplayEvent[] {
  return text
    .split("\n")
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line) as ReplayEvent);
}

export const MOCK_MODEL_ENTRIES: ModelEntry[] = JSON.parse(modelsRaw) as ModelEntry[];

export const MOCK_ENGINE_REPLAYS: Record<string, ReplayEvent[]> = {
  codex: jsonl(codexRaw),
  "claude-code": jsonl(claudeRaw),
};
