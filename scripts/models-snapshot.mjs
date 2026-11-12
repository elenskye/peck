#!/usr/bin/env node
// 模型目录：从 models.dev 取一份价格和上下文的快照，写进 src-tauri/defaults/models.json。
// 列哪些模型在下面的 MODELS 里定（每家挑几个现行的）；价格变了、要加模型，改这里再跑一次：
//   node scripts/models-snapshot.mjs
// 设置里还能改价格、加模型（存在站主的设置里，不改这份文件）。
import { writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const target = path.join(root, "src-tauri/defaults/models.json");
const SOURCE = "https://models.dev/api.json";

// 供应商：id 和钥匙串里的一样；wire 是线协议，auth 是 key 或 adc（gcloud）。
const PROVIDERS = [
  { id: "anthropic", label: "Anthropic", wire: "messages", auth: "key", baseUrl: "https://api.anthropic.com/v1" },
  { id: "openai", label: "OpenAI", wire: "responses", auth: "key", baseUrl: "https://api.openai.com/v1" },
  { id: "deepseek", label: "DeepSeek", wire: "chat", dialect: "deepseek", auth: "key", baseUrl: "https://api.deepseek.com" },
  {
    id: "aliyun",
    label: "阿里云百炼",
    wire: "chat",
    dialect: "qwen",
    auth: "key",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    regionBaseUrls: { singapore: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1" },
  },
  { id: "google", label: "Google Gemini", wire: "gemini", auth: "key", baseUrl: "https://generativelanguage.googleapis.com/v1beta" },
  { id: "vertex-anthropic", label: "Vertex AI · Claude", wire: "messages", auth: "adc" },
  { id: "vertex-google", label: "Vertex AI · Gemini", wire: "gemini", auth: "adc" },
];

// [Peck 的供应商 id, models.dev 的供应商, 模型 id]；百炼另取新加坡的价格。
const MODELS = [
  ["anthropic", "anthropic", "claude-fable-5-2"],
  ["anthropic", "anthropic", "claude-opus-5-5"],
  ["anthropic", "anthropic", "claude-sonnet-5-5"],
  ["anthropic", "anthropic", "claude-haiku-5-5"],
  ["openai", "openai", "gpt-6.1-sol"],
  ["openai", "openai", "gpt-6.1-terra"],
  ["openai", "openai", "gpt-6-astra"],
  ["openai", "openai", "gpt-6-luna"],
  ["deepseek", "deepseek", "deepseek-v4-pro"],
  ["deepseek", "deepseek", "deepseek-flash"],
  ["aliyun", "alibaba-cn", "qwen3.8-max"],
  ["aliyun", "alibaba-cn", "qwen3.8-flash"],
  ["aliyun", "alibaba-cn", "qwen3.7-plus"],
  ["google", "google", "gemini-4-carbon"],
  ["google", "google", "gemini-4-argon"],
  ["google", "google", "gemini-3.8-flash"],
  ["google", "google", "gemini-3.5-flash-lite"],
  ["vertex-anthropic", "google-vertex-anthropic", "claude-opus-5-5@default"],
  ["vertex-anthropic", "google-vertex-anthropic", "claude-sonnet-5-5@default"],
  ["vertex-anthropic", "google-vertex-anthropic", "claude-haiku-5-5@default"],
  ["vertex-google", "google-vertex", "gemini-4-carbon"],
  ["vertex-google", "google-vertex", "gemini-3.8-flash"],
];
const REGION_PRICES = { aliyun: { singapore: "alibaba" } };

// CLI 引擎：command 是可执行文件名（设置里的「CLI 目录」或 PATH 里找）。
const ENGINES = [
  { id: "claude-code", label: "Claude Code", command: "claude", protocol: "stream-json" },
  { id: "codex", label: "Codex", command: "codex", protocol: "app-server" },
  { id: "muse", label: "Muse", command: "muse", protocol: "exec-json" },
  { id: "grok", label: "Grok", command: "grok", protocol: "acp" },
  { id: "antigravity", label: "Antigravity", command: "agy", protocol: "acp" },
];

function price(cost) {
  if (!cost || typeof cost.input !== "number" || typeof cost.output !== "number") return null;
  const p = { input: cost.input, output: cost.output };
  if (typeof cost.cache_read === "number") p.cacheRead = cost.cache_read;
  if (typeof cost.cache_write === "number") p.cacheWrite = cost.cache_write;
  // 超过一定上下文以后换一档价格（OpenAI 272K、Gemini 200K 这类）。
  const tiers = [];
  for (const t of cost.tiers ?? []) {
    if (t.tier?.type === "context" && typeof t.tier.size === "number") {
      const tp = price(t);
      if (tp) tiers.push({ aboveTokens: t.tier.size, ...tp });
    }
  }
  if (!tiers.length && cost.context_over_200k) {
    const tp = price(cost.context_over_200k);
    if (tp) tiers.push({ aboveTokens: 200000, ...tp });
  }
  if (tiers.length) p.tiers = tiers;
  return p;
}

function thinking(m) {
  if (!m.reasoning) return "never";
  const options = m.reasoning_options ?? [];
  const toggle = options.some((o) => o.type === "toggle");
  const effortNone = options.some((o) => o.type === "effort" && o.values?.includes("none"));
  return toggle || effortNone ? "switch" : "always";
}

function efforts(m) {
  const effort = (m.reasoning_options ?? []).find((o) => o.type === "effort");
  return (effort?.values ?? []).filter((v) => v !== "none");
}

const response = await fetch(SOURCE);
if (!response.ok) throw new Error(`models.dev ${response.status}`);
const dev = await response.json();

const models = MODELS.map(([provider, devProvider, id]) => {
  const m = dev[devProvider]?.models?.[id];
  if (!m) throw new Error(`models.dev 里没有 ${devProvider}/${id}`);
  const entry = {
    id: `${provider}/${id}`,
    provider,
    apiModel: id,
    label: m.name,
    contextWindow: m.limit?.context ?? null,
    maxOutput: m.limit?.output ?? null,
    thinking: thinking(m),
    efforts: efforts(m),
    tools: Boolean(m.tool_call),
    vision: Boolean(m.modalities?.input?.includes("image")),
    price: price(m.cost),
  };
  const regions = REGION_PRICES[provider];
  if (regions) {
    entry.regionPrices = {};
    for (const [region, otherProvider] of Object.entries(regions)) {
      const other = dev[otherProvider]?.models?.[id];
      if (other) entry.regionPrices[region] = price(other.cost);
    }
  }
  return entry;
});

const snapshot = {
  snapshot: { source: "models.dev", date: new Date().toISOString().slice(0, 10) },
  providers: PROVIDERS,
  models,
  engines: ENGINES,
};
writeFileSync(target, JSON.stringify(snapshot, null, 2) + "\n");
console.log(`写好了 ${path.relative(root, target)}：${models.length} 个模型、${ENGINES.length} 个引擎`);
