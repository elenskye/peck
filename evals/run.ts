// PLOOP 评测：pnpm eval <run | check | report> …（说明见 evals/README.md）。
// 一组 = 题 × 臂 × 次数；每次尝试从干净状态开始、单独落盘，重跑同一条命令跳过做完的。

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, openSync, closeSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { ASK_REPLY, EVALS, HOME, REPO, allTaskIds, grade, loadTask, materialize, metrics, sh, type Grade, type Metrics, type Task } from "./lib/core.ts";
import { diffsOf, minDetectable, paired, passAt1, passHatK, type Interval, type Runs } from "./lib/stats.ts";

const HELP = `pnpm eval run <组.json> [--keys <Key 目录>] [--no-build] [--keep]
  跑一组：题 × 臂 × 次数，A B 交替、固定并发；做完的跳过（中途杀掉再跑接着做）；花到 budgetUsd 就停。
  结果在 ${"projects/eval/runs/<组名>/<题>/<臂>/<第几次>/"}（result.json、events.jsonl、final.diff、grade.json）。
pnpm eval check [<题> …]
  校验题：参考答案要过、空改动要不过、alt/ 里每份换写法的解要过。不给题号就查全部。
pnpm eval report <组.json> [--out evals/reports/<版本>-<名字>] [--baseline <旧报告.json>]
  汇总成一页中文报告（pass@1 带区间、pass^3、按题配对的差、噪声底、花费、计数、没读过的失败）。`;

type Arm = { name: string; ui: Record<string, unknown>; script?: string; peck?: string };
type Suite = { name: string; tasks: string[]; arms: string[]; attempts: number; concurrency: number; budgetUsd: number; noise?: [string, string] };
type Outcome = { runId?: string; baseCommit?: string; reason: string; error?: string; model: string; effort?: string; startedAt: number; endedAt: number; approvalsDenied: number; highRiskDenied: number; asksAnswered: number; costUsd: number };
type Result = {
  task: string; arm: string; n: number; group: string; network: boolean;
  startedAt: number; durationMs: number; peck: string;
  outcome: Outcome | null; metrics: Metrics; grade: Grade | null;
  passed: boolean; score: number; costUsd: number; infra: string | null; retries: number;
};

const die = (msg: string): never => { console.error(msg); process.exit(2); };
const readJson = <T>(p: string): T => JSON.parse(readFileSync(p, "utf8")) as T;
const writeJson = (p: string, v: unknown) => writeFileSync(p, JSON.stringify(v, null, 2) + "\n");

function loadSuite(path: string): Suite {
  const s = readJson<Suite>(resolve(path));
  for (const k of ["name", "tasks", "arms", "attempts"] as const) if (s[k] === undefined) die(`${path} 缺 ${k}`);
  return { ...s, concurrency: s.concurrency ?? 1, budgetUsd: s.budgetUsd ?? 0 };
}

function loadArm(name: string): Arm {
  const file = join(EVALS, "arms", `${name}.json`);
  if (!existsSync(file)) die(`没有臂 ${name}（evals/arms/${name}.json）`);
  return { name, ...readJson<Omit<Arm, "name">>(file) };
}

/** `all`（不含 holdout）、`group:<组>`、`tag:<标签>`，或者题号。 */
function selectTasks(sel: string[]): Task[] {
  const all = allTaskIds().map(loadTask);
  const out = new Map<string, Task>();
  for (const s of sel) {
    const pick = s === "all" ? all.filter((t) => t.group !== "holdout")
      : s.startsWith("group:") ? all.filter((t) => t.group === s.slice(6))
      : s.startsWith("tag:") ? all.filter((t) => t.tags.includes(s.slice(4)))
      : [loadTask(s)];
    for (const t of pick) out.set(t.id, t);
  }
  return [...out.values()];
}

/** 假模型的剧本：题自己的 `scripts/<名>.json`，没有就用 `arms/scripts/<名>.json`。 */
function scriptFor(arm: Arm, task: Task): string | undefined {
  if (!arm.script) return undefined;
  const own = join(task.dir, "scripts", `${arm.script}.json`);
  const shared = join(EVALS, "arms", "scripts", `${arm.script}.json`);
  if (existsSync(own)) return own;
  if (existsSync(shared)) return shared;
  return die(`臂 ${arm.name} 要剧本 ${arm.script}：${task.id} 没有`);
}

const flag = (args: string[], name: string) => args.includes(name);
const opt = (args: string[], name: string) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const expand = (p: string) => (p.startsWith("~/") ? join(homedir(), p.slice(2)) : resolve(p));

function peckCommit(): { commit: string; dirty: boolean } {
  const commit = sh("git", ["-C", REPO, "rev-parse", "--short", "HEAD"]).stdout.trim();
  const dirty = sh("git", ["-C", REPO, "status", "--porcelain", "--", "src-tauri"]).stdout.trim() !== "";
  return { commit, dirty };
}

/** 这一组已经花了多少：所有尝试（连 infra 重跑的）的 outcome.json。 */
function spentSoFar(dir: string): number {
  let total = 0;
  const walk = (d: string) => {
    if (!existsSync(d)) return;
    for (const f of readdirSync(d, { withFileTypes: true })) {
      if (f.isDirectory() && f.name !== "data") walk(join(d, f.name));
      else if (f.name === "outcome.json") total += readJson<Outcome>(join(d, f.name)).costUsd ?? 0;
    }
  };
  walk(dir);
  return total;
}

/** infra：不算模型没做对，自动重跑（最多 2 次）。 */
function infraOf(outcome: Outcome | null, killed: boolean): string | null {
  if (killed) return "到时间没停，杀掉了";
  if (!outcome) return "peck eval 没写出结果（崩了）";
  if (outcome.reason === "setup") return `没发出去：${outcome.error ?? ""}`;
  if (outcome.reason === "error" && /network|timed? ?out|connection|broken pipe|overloaded|reset|5\d\d|429|断|超时|连不上/i.test(outcome.error ?? "")) return `API：${outcome.error}`;
  return null;
}

/** 在跑的 `peck eval`（各自一个进程组）：Ctrl-C 时一起停。 */
const live = new Set<number>();
const killGroup = (pid: number) => { try { process.kill(-pid, "SIGKILL"); } catch { /* 已经退了 */ } };
for (const sig of ["SIGINT", "SIGTERM"] as const) process.on(sig, () => { for (const pid of live) killGroup(pid); process.exit(130); });

/** 跑一次 `peck eval`；`peck.pid` 留着：上一回被杀掉时丢下的，重跑前先停掉。返回是不是到时间杀掉的。 */
function runPeck(bin: string, dir: string, env: NodeJS.ProcessEnv, hardMs: number): Promise<boolean> {
  return new Promise((done) => {
    const fd = openSync(join(dir, "peck.log"), "a");
    const child = spawn(bin, ["eval", join(dir, "attempt.json")], { env, stdio: ["ignore", fd, fd], detached: true });
    live.add(child.pid!);
    writeFileSync(join(dir, "peck.pid"), String(child.pid));
    let killed = false;
    const timer = setTimeout(() => { killed = true; killGroup(child.pid!); }, hardMs);
    child.on("close", () => { clearTimeout(timer); closeSync(fd); live.delete(child.pid!); rmSync(join(dir, "peck.pid"), { force: true }); done(killed); });
  });
}

/** 上一回留下的 `peck eval` 还在跑就停掉（只认这个尝试文件夹记着的进程组）。 */
function stopLeftover(dir: string) {
  const file = join(dir, "peck.pid");
  if (!existsSync(file)) return;
  const pid = Number(readFileSync(file, "utf8"));
  // 进程号会被别人重用：确认还是这个尝试的 peck eval 才停。
  const cmd = spawnSync("ps", ["-o", "command=", "-p", String(pid)], { encoding: "utf8" }).stdout ?? "";
  if (pid > 1 && cmd.includes(" eval ") && cmd.includes(join(dir, "attempt.json"))) killGroup(pid);
}

type Ctx = { suite: Suite; bin: string; peck: string; keys?: string; keep: boolean; runs: string; work: string };

async function attempt(ctx: Ctx, task: Task, arm: Arm, n: number, costCap: number): Promise<Result> {
  const dir = join(ctx.runs, task.id, arm.name, String(n));
  const ws = join(ctx.work, task.id, arm.name, String(n));
  const script = scriptFor(arm, task);
  if (!script && !ctx.keys) die("真跑要 --keys <Key 目录>（或环境变量 PECK_EVAL_KEYS）");
  for (let k = 0; ; k++) {
    stopLeftover(dir);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    const startedAt = Date.now();
    const ui = structuredClone(arm.ui) as Record<string, unknown>;
    if (task.maxRounds) ui.maxRounds = Math.min(Number(ui.maxRounds ?? 40), task.maxRounds);
    materialize(task, ws);
    const attemptFile = join(dir, "attempt.json");
    writeJson(attemptFile, {
      workspace: ws, prompt: task.prompt, permission: task.mode, ui, script, keys: script ? undefined : ctx.keys,
      costCap, timeoutSecs: task.timeoutMin * 60, askReply: ASK_REPLY, out: dir,
    });
    const data = join(dir, "data");
    const env: NodeJS.ProcessEnv = { ...process.env, PECK_DATA_DIR: data, PECK_SHADOW_ROOT: join(data, "shadow") };
    if (script) delete env.PECK_API_MODE; else env.PECK_API_MODE = "real";
    const killed = await runPeck(ctx.bin, dir, env, (task.timeoutMin * 60 + 180) * 1000);
    const outcome = existsSync(join(dir, "outcome.json")) ? readJson<Outcome>(join(dir, "outcome.json")) : null;
    let infra = infraOf(outcome, killed);
    let g: Grade | null = null;
    if (!infra && outcome?.baseCommit) {
      g = await grade(task, ws, outcome.baseCommit, join(dir, "final.diff"));
      writeJson(join(dir, "grade.json"), g);
      infra = g.infra ?? null;
    } else if (!infra) {
      infra = "工作区不是 git 仓库，判不了";
    }
    const result: Result = {
      task: task.id, arm: arm.name, n, group: task.group, network: task.network,
      startedAt, durationMs: Date.now() - startedAt, peck: ctx.peck,
      outcome, metrics: metrics(join(dir, "events.jsonl")), grade: g,
      passed: !infra && !!g?.passed, score: g?.score ?? 0, costUsd: outcome?.costUsd ?? 0, infra, retries: k,
    };
    if (infra && k < 2) {
      const keepAs = `${dir}.infra-${k + 1}`;
      rmSync(keepAs, { recursive: true, force: true });
      writeJson(join(dir, "infra.json"), result);
      renameSync(dir, keepAs);
      continue;
    }
    writeJson(join(dir, "result.json"), result);
    if (!ctx.keep) rmSync(ws, { recursive: true, force: true });
    return result;
  }
}

async function runSuite(args: string[]) {
  const suitePath = args[0] ?? die(HELP);
  const suite = loadSuite(suitePath);
  const tasks = selectTasks(suite.tasks);
  const arms = suite.arms.map(loadArm);
  const keys = opt(args, "--keys") ?? process.env.PECK_EVAL_KEYS;
  const real = arms.filter((a) => !a.script);
  if (real.length && !(suite.budgetUsd > 0)) die(`有真跑的臂（${real.map((a) => a.name).join("、")}）：组里要写 budgetUsd（站主批的总预算）`);
  if (!flag(args, "--no-build")) {
    console.log("编译 peck …");
    const b = spawnSync("cargo", ["build", "-q", "--manifest-path", join(REPO, "src-tauri/Cargo.toml"), "--bin", "peck"], { stdio: "inherit" });
    if (b.status !== 0) die("peck 编译不过");
  }
  const { commit, dirty } = peckCommit();
  const peck = dirty ? `${commit}+dirty` : commit;
  for (const a of arms) if (a.peck && !commit.startsWith(a.peck) && !a.peck.startsWith(commit)) die(`臂 ${a.name} 要 Peck ${a.peck}，现在是 ${commit}`);
  const ctx: Ctx = { suite, bin: join(REPO, "src-tauri/target/debug/peck"), peck, keys: keys ? expand(keys) : undefined, keep: flag(args, "--keep"), runs: join(HOME, "runs", suite.name), work: join(HOME, "work", suite.name) };
  mkdirSync(ctx.runs, { recursive: true });
  const metaFile = join(ctx.runs, "meta.json");
  const meta = existsSync(metaFile) ? readJson<{ sessions: unknown[] }>(metaFile) : { sessions: [] };
  meta.sessions.push({ startedAt: new Date().toISOString(), peck, concurrency: suite.concurrency, node: process.version, tasks: tasks.map((t) => t.id), arms: arms.map((a) => a.name), attempts: suite.attempts });
  writeJson(metaFile, meta);

  // A B 交替：第 1 次每题各臂，再第 2 次……
  const queue: [Task, Arm, number][] = [];
  for (let n = 1; n <= suite.attempts; n++) for (const t of tasks) for (const a of arms) queue.push([t, a, n]);
  let spent = spentSoFar(ctx.runs);
  let reserved = 0;
  let running = 0;
  let stopped: string | null = null;
  let done = 0;
  const total = queue.length;
  const results: Result[] = [];
  const already = queue.filter(([t, a, n]) => existsSync(join(ctx.runs, t.id, a.name, String(n), "result.json"))).length;
  if (already) console.log(`接着做：${already}/${total} 次已经做完，跳过`);
  const worker = async () => {
    for (;;) {
      const next = queue.shift();
      if (!next || stopped) return;
      const [t, a, n] = next;
      const resultFile = join(ctx.runs, t.id, a.name, String(n), "result.json");
      if (existsSync(resultFile)) { results.push(readJson<Result>(resultFile)); done++; continue; }
      const free = suite.concurrency - running;
      const cap = a.script ? 0 : (suite.budgetUsd - spent - reserved) / Math.max(1, free);
      if (!a.script && cap <= 0.001) {
        stopped = `花到预算了（$${spent.toFixed(2)} / $${suite.budgetUsd}）：停在 ${t.id} / ${a.name} / 第 ${n} 次`;
        return;
      }
      running++;
      reserved += cap;
      const r = await attempt(ctx, t, a, n, a.script ? 1 : cap);
      reserved -= cap;
      running--;
      spent += r.costUsd;
      done++;
      results.push(r);
      const mark = r.infra ? `infra（${r.infra}）` : r.passed ? "过" : `没过（${r.outcome?.reason ?? "?"}，${Math.round(r.score * 100)}%）`;
      console.log(`[${done}/${total}] ${t.id} · ${a.name} · 第 ${n} 次：${mark}，$${r.costUsd.toFixed(4)}，${Math.round(r.durationMs / 1000)}s`);
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, suite.concurrency) }, worker));
  if (stopped) {
    writeJson(join(ctx.runs, "stopped.json"), { at: new Date().toISOString(), reason: stopped, spentUsd: spent });
    console.log(stopped);
  } else {
    rmSync(join(ctx.runs, "stopped.json"), { force: true });
  }
  console.log(`做完 ${done}/${total}，这一组一共花了 $${spent.toFixed(4)}。报告：pnpm eval report ${suitePath}`);
}

// ───────────────────────── 校验题 ─────────────────────────

async function checkTasks(ids: string[]) {
  const tasks = (ids.length ? ids : allTaskIds()).map(loadTask);
  const outDir = join(HOME, "runs", "_check");
  mkdirSync(outDir, { recursive: true });
  let bad = 0;
  for (const task of tasks) {
    const ws = join(HOME, "work", "_check", task.id);
    const problems: string[] = [];
    const tryWith = async (patch?: string) => {
      const base = materialize(task, ws);
      if (patch) sh("git", ["-C", ws, "apply", "--binary", patch]);
      return grade(task, ws, base);
    };
    const empty = await tryWith();
    if (empty.passed) problems.push("空改动也过了");
    if (empty.tests.some((t) => t.group === "pass_to_pass" && !t.ok)) problems.push("空改动时 pass_to_pass 就没过（测试本身坏了）");
    if (empty.tests.some((t) => t.group === "fail_to_pass" && t.ok)) problems.push("有 fail_to_pass 改前就过（没在考要修的）");
    const refFile = join(task.dir, "reference.patch");
    const reference = existsSync(refFile) ? await tryWith(refFile) : null;
    if (!reference) problems.push("缺 reference.patch");
    else if (!reference.passed) problems.push(`参考答案没过：${reference.tests.filter((t) => !t.ok).map((t) => t.name).join("、") || reference.health}`);
    const altDir = join(task.dir, "alt");
    const alts = existsSync(altDir) ? readdirSync(altDir).filter((f) => f.endsWith(".patch")).sort() : [];
    if (!alts.length) problems.push("缺换写法的解（alt/*.patch）");
    const altResults: { file: string; passed: boolean }[] = [];
    for (const f of alts) {
      const g = await tryWith(join(altDir, f));
      altResults.push({ file: f, passed: g.passed });
      if (!g.passed) problems.push(`换写法的解 ${f} 没过：${g.tests.filter((t) => !t.ok).map((t) => t.name).join("、")}`);
    }
    rmSync(ws, { recursive: true, force: true });
    writeJson(join(outDir, `${task.id}.json`), { task: task.id, at: new Date().toISOString(), ok: problems.length === 0, problems, empty, reference, alts: altResults });
    if (problems.length) bad++;
    console.log(`${problems.length ? "✗" : "✓"} ${task.id}${problems.length ? "：" + problems.join("；") : ""}`);
  }
  console.log(`${tasks.length - bad}/${tasks.length} 题合格`);
  if (bad) process.exit(1);
}

// ───────────────────────── 报告 ─────────────────────────

type Review = Record<string, { verdict: "model" | "task" | "grader" | "infra"; note: string }>;

function collect(runs: string): Result[] {
  const out: Result[] = [];
  if (!existsSync(runs)) return out;
  for (const t of readdirSync(runs, { withFileTypes: true })) {
    if (!t.isDirectory() || t.name.startsWith("_")) continue;
    for (const a of readdirSync(join(runs, t.name), { withFileTypes: true })) {
      if (!a.isDirectory()) continue;
      for (const n of readdirSync(join(runs, t.name, a.name))) {
        const f = join(runs, t.name, a.name, n, "result.json");
        if (existsSync(f)) out.push(readJson<Result>(f));
      }
    }
  }
  return out;
}

const pct = (x: number) => (Number.isFinite(x) ? `${Math.round(x * 100)}%` : "—");
const pp = (x: number) => (Number.isFinite(x) ? `${x >= 0 ? "+" : ""}${Math.round(x * 100)}` : "—");
const ci = (i: Interval) => (Number.isFinite(i.lo) ? `${pct(i.mean)}（${pct(i.lo)}–${pct(i.hi)}）` : pct(i.mean));
const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
const mean = (xs: number[]) => (xs.length ? sum(xs) / xs.length : NaN);

function byTask(results: Result[], arm: string, filter: (r: Result) => boolean = () => true): Map<string, Runs> {
  const m = new Map<string, Runs>();
  for (const r of results.filter((x) => x.arm === arm && !x.infra && filter(x)).sort((a, b) => a.n - b.n)) {
    m.set(r.task, [...(m.get(r.task) ?? []), r.passed]);
  }
  return m;
}

function report(args: string[]) {
  const suitePath = args[0] ?? die(HELP);
  const suite = loadSuite(suitePath);
  const runs = join(HOME, "runs", suite.name);
  const results = collect(runs);
  if (!results.length) die(`${suite.name} 还没有结果`);
  const review: Review = existsSync(join(runs, "review.json")) ? readJson<Review>(join(runs, "review.json")) : {};
  const key = (r: Result) => `${r.task}/${r.arm}/${r.n}`;
  const k = Math.min(3, suite.attempts);
  const arms = suite.arms.filter((a) => results.some((r) => r.arm === a));
  const stats = arms.map((arm) => {
    const rs = results.filter((r) => r.arm === arm);
    const ok = rs.filter((r) => !r.infra);
    const tok = (f: keyof Metrics["tokens"]) => sum(ok.map((r) => r.metrics.tokens[f]));
    const input = tok("input"), cacheRead = tok("cacheRead"), cacheWrite = tok("cacheWrite"), output = tok("output");
    const tasks = byTask(results, arm);
    const net = byTask(results, arm, (r) => r.network);
    return {
      arm,
      attempts: ok.length,
      infra: rs.length - ok.length,
      passAt1: passAt1([...tasks.values()]),
      passHatK: passHatK([...tasks.values()], k),
      network: net.size ? passAt1([...net.values()]) : null,
      costUsd: sum(rs.map((r) => r.costUsd)),
      costPerAttempt: mean(ok.map((r) => r.costUsd)),
      minutes: mean(ok.map((r) => r.durationMs / 60_000)),
      rounds: mean(ok.map((r) => r.metrics.rounds)),
      tokens: { input, cacheRead, cacheWrite, output, hit: cacheRead / Math.max(1, input + cacheRead + cacheWrite) },
      counts: {
        subagents: sum(ok.map((r) => r.metrics.subagents)), queued: sum(ok.map((r) => r.metrics.queued)), blockedWrites: sum(ok.map((r) => r.metrics.blockedWrites)),
        approvalsDenied: sum(ok.map((r) => r.outcome?.approvalsDenied ?? 0)), highRiskDenied: sum(ok.map((r) => r.outcome?.highRiskDenied ?? 0)), asks: sum(ok.map((r) => r.outcome?.asksAnswered ?? 0)),
        loopWarnings: sum(ok.map((r) => r.metrics.loopWarnings)), compactions: sum(ok.map((r) => r.metrics.compactions)),
      },
      perTask: Object.fromEntries([...tasks].map(([t, r]) => [t, r.filter(Boolean).length / r.length])),
    };
  });
  const base = arms[0];
  const pairs = arms.slice(1).map((arm) => ({ arm, vs: base, ...paired(byTask(results, arm), byTask(results, base)) }));
  const noise = suite.noise && arms.includes(suite.noise[0]) && arms.includes(suite.noise[1])
    ? { pair: suite.noise, ...paired(byTask(results, suite.noise[0]), byTask(results, suite.noise[1])), minDetectable: minDetectable(diffsOf(byTask(results, suite.noise[0]), byTask(results, suite.noise[1]))) }
    : null;
  const failures = results.filter((r) => !r.passed && !r.infra).map((r) => ({ key: key(r), path: `runs/${suite.name}/${key(r)}`, reason: r.outcome?.reason, score: r.score, review: review[key(r)] ?? null }));
  const unread = failures.filter((f) => !f.review);
  const baselineFile = opt(args, "--baseline");
  const baseline = baselineFile ? readJson<{ stats: typeof stats; peck: string[] }>(resolve(baselineFile)) : null;
  const pecks = [...new Set(results.map((r) => r.peck))];
  const taskCount = new Set(results.map((r) => r.task)).size;
  const json = { suite: suite.name, at: new Date().toISOString(), peck: pecks, tasks: taskCount, attempts: suite.attempts, arms, stats, pairs, noise, failures, unread: unread.length, totalUsd: sum(results.map((r) => r.costUsd)) };

  const L: string[] = [];
  L.push(`# PLOOP 评测 · ${suite.name}`, "");
  L.push("> 结论：（读完失败的过程以后写一句）", "");
  L.push(`Peck ${pecks.join("、")}；${taskCount} 题 × ${suite.attempts} 次 × ${arms.length} 臂，并发 ${suite.concurrency}；一共花了 $${json.totalUsd.toFixed(2)}。${unread.length ? `**还有 ${unread.length} 次失败没读过过程，分数先不算数。**` : "失败的过程都读过了。"}`, "");
  L.push("## 各臂", "", `| 臂 | pass@1（95% 区间） | pass^${k} | 每次花费 | 每次耗时 | 轮数 | 缓存命中 | infra |`, "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |");
  for (const s of stats) L.push(`| ${s.arm} | ${ci(s.passAt1)} | ${pct(s.passHatK.mean)} | $${s.costPerAttempt.toFixed(3)} | ${s.minutes.toFixed(1)} 分 | ${s.rounds.toFixed(1)} | ${pct(s.tokens.hit)} | ${s.infra} |`);
  if (stats.some((s) => s.network)) L.push("", `联网的题另算：${stats.filter((s) => s.network).map((s) => `${s.arm} ${ci(s.network!)}`).join("；")}。`);
  if (pairs.length) {
    L.push("", `## 按题配对（和 ${base} 比）`, "", "| 臂 | 差（点） | 95% 区间 | 题数 |", "| --- | ---: | --- | ---: |");
    for (const p of pairs) L.push(`| ${p.arm} | ${pp(p.mean)} | ${pp(p.lo)} 到 ${pp(p.hi)} | ${p.tasks} |`);
  }
  if (noise) L.push("", `噪声底：${noise.pair[0]} 和 ${noise.pair[1]} 是同一个配置，差 ${pp(noise.mean)} 点（${pp(noise.lo)} 到 ${pp(noise.hi)}）；${noise.tasks} 题、每题 ${suite.attempts} 次能看出的最小差别约 ${Math.round(noise.minDetectable * 100)} 个点。`);
  if (baseline) {
    L.push("", `## 和上一份基线比（Peck ${baseline.peck.join("、")}）`, "", "| 臂 | 上一份 | 这一份 | 差（点） |", "| --- | ---: | ---: | ---: |");
    for (const s of stats) {
      const old = baseline.stats.find((x) => x.arm === s.arm);
      if (old) L.push(`| ${s.arm} | ${pct(old.passAt1.mean)} | ${pct(s.passAt1.mean)} | ${pp(s.passAt1.mean - old.passAt1.mean)} |`);
    }
  }
  const told = failures.filter((f) => f.review?.verdict === "model").slice(0, 3);
  L.push("", "## 典型失败", "");
  if (told.length) for (const f of told) L.push(`- **${f.key}**：${f.review!.note}`);
  else L.push(failures.length ? "（还没读过过程：读完在 review.json 写 verdict 和一句人话）" : "没有失败。");
  L.push("", "## 下一步建议", "", "（读完过程以后写）", "");
  L.push("<details><summary>每题、计数</summary>", "", `| 题 | ${arms.join(" | ")} |`, `| --- | ${arms.map(() => "---:").join(" | ")} |`);
  for (const t of [...new Set(results.map((r) => r.task))].sort()) {
    L.push(`| ${t} | ${arms.map((a) => { const r = results.filter((x) => x.task === t && x.arm === a && !x.infra); return `${r.filter((x) => x.passed).length}/${r.length}`; }).join(" | ")} |`);
  }
  L.push("", "| 臂 | 子代理 | 排队 | 越界写被拦 | 卡片拒（高危） | ASK | 原地打转 | 压缩 | token 输入 / 读缓存 / 写缓存 / 输出 |", "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
  for (const s of stats) {
    const c = s.counts;
    L.push(`| ${s.arm} | ${c.subagents} | ${c.queued} | ${c.blockedWrites} | ${c.approvalsDenied}（${c.highRiskDenied}） | ${c.asks} | ${c.loopWarnings} | ${c.compactions} | ${s.tokens.input} / ${s.tokens.cacheRead} / ${s.tokens.cacheWrite} / ${s.tokens.output} |`);
  }
  L.push("", "</details>", "");
  const md = L.join("\n");
  writeFileSync(join(runs, "report.md"), md);
  writeJson(join(runs, "report.json"), json);
  const out = opt(args, "--out");
  if (out) {
    writeFileSync(resolve(`${out}.md`), md);
    writeJson(resolve(`${out}.json`), json);
  }
  console.log(md);
  console.log(`\n写在 ${join(runs, "report.md")}${out ? `、${out}.md` : ""}`);
}

const [cmd, ...rest] = process.argv.slice(2);
if (cmd === "run") await runSuite(rest);
else if (cmd === "check") await checkTasks(rest);
else if (cmd === "report") report(rest);
else { console.log(HELP); if (cmd && cmd !== "--help" && cmd !== "-h") process.exit(2); }
