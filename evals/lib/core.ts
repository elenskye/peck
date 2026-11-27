// 题目、工作区、判分、从事件里数数。run.ts 用；格式见 evals/README.md。

import { spawnSync, spawn } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, appendFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { parseToml, type Toml, type TomlValue } from "./toml.ts";

export const EVALS = resolve(dirname(import.meta.filename), "..");
export const REPO = resolve(EVALS, "..");
/** 工作区、过程、结果都在仓库外（有代码和提示词，不进 Peck 仓库）。 */
export const HOME = process.env.PECK_EVAL_HOME ?? resolve(REPO, "../projects/eval");
export const ASK_REPLY = "评测中：按你的判断做，不要再问";

export type Group = "regression" | "capability" | "holdout";
export type Test = { name: string; group: "fail_to_pass" | "pass_to_pass"; run: string; timeoutMin: number };
export type Task = {
  id: string;
  dir: string;
  prompt: string;
  mode: string;
  group: Group;
  tags: string[];
  network: boolean;
  maxRounds?: number;
  timeoutMin: number;
  source: { dir?: string; repo?: string; commit?: string; history: boolean; caches: string[] };
  check: { protect: string[]; append: { file: string; from: string }[]; tests: Test[] };
};

const str = (v: TomlValue | undefined, what: string): string => {
  if (typeof v !== "string" || !v.trim()) throw new Error(`缺 ${what}（字符串）`);
  return v;
};
const strs = (v: TomlValue | undefined, what: string): string[] => {
  if (v === undefined) return [];
  if (!Array.isArray(v) || v.some((x) => typeof x !== "string")) throw new Error(`${what} 要是字符串数组`);
  return v as string[];
};
const only = (t: Toml, keys: string[], where: string) => {
  const extra = Object.keys(t).filter((k) => !keys.includes(k));
  if (extra.length) throw new Error(`${where} 里不认：${extra.join("、")}`);
};

/** 读一道题：`tasks/<id>/task.toml` + `check/check.toml`。写错了报哪个文件哪一项。 */
export function loadTask(id: string): Task {
  const dir = join(EVALS, "tasks", id);
  const file = join(dir, "task.toml");
  try {
    const t = parseToml(readFileSync(file, "utf8"));
    only(t, ["prompt", "mode", "group", "tags", "network", "max_rounds", "timeout_min", "source"], "task.toml");
    const s = (t.source ?? {}) as Toml;
    only(s, ["dir", "repo", "commit", "history", "caches"], "[source]");
    if (!s.dir === !s.repo) throw new Error("[source] 写 dir（题目里的起点）或 repo + commit，二选一");
    if (s.repo && typeof s.commit !== "string") throw new Error("[source] 的 repo 要配 commit");
    const group = (t.group ?? "regression") as Group;
    if (!["regression", "capability", "holdout"].includes(group)) throw new Error(`group 只能是 regression / capability / holdout`);
    const c = parseToml(readFileSync(join(dir, "check", "check.toml"), "utf8"));
    only(c, ["protect", "append", "test"], "check.toml");
    const tests = ((c.test ?? []) as Toml[]).map((x, i) => {
      only(x, ["name", "group", "run", "timeout_min"], `[[test]] 第 ${i + 1} 个`);
      const g = x.group;
      if (g !== "fail_to_pass" && g !== "pass_to_pass") throw new Error(`[[test]] ${x.name} 的 group 只能是 fail_to_pass / pass_to_pass`);
      return { name: str(x.name, "test.name"), group: g, run: str(x.run, "test.run"), timeoutMin: Number(x.timeout_min ?? 10) } as Test;
    });
    if (!tests.some((x) => x.group === "fail_to_pass")) throw new Error("check.toml 至少要一个 fail_to_pass");
    return {
      id,
      dir,
      prompt: str(t.prompt, "prompt"),
      mode: String(t.mode ?? "auto"),
      group,
      tags: strs(t.tags, "tags"),
      network: t.network === true,
      maxRounds: typeof t.max_rounds === "number" ? t.max_rounds : undefined,
      timeoutMin: Number(t.timeout_min ?? 30),
      source: { dir: s.dir as string | undefined, repo: s.repo as string | undefined, commit: s.commit as string | undefined, history: s.history === true, caches: strs(s.caches, "caches") },
      check: {
        protect: strs(c.protect, "protect"),
        append: ((c.append ?? []) as Toml[]).map((a) => ({ file: str(a.file, "append.file"), from: str(a.from, "append.from") })),
        tests,
      },
    };
  } catch (e) {
    throw new Error(`题 ${id}：${(e as Error).message}`);
  }
}

export function allTaskIds(): string[] {
  return readdirSync(join(EVALS, "tasks")).filter((d) => existsSync(join(EVALS, "tasks", d, "task.toml"))).sort();
}

/** 跑一条命令；`input` 给 stdin。退出码不是 0 抛错（`ok: true` 时返回结果不抛）。 */
export function sh(cmd: string, args: string[], opts: { cwd?: string; env?: NodeJS.ProcessEnv; ok?: boolean; input?: string } = {}) {
  const r = spawnSync(cmd, args, { cwd: opts.cwd, env: opts.env ?? process.env, encoding: "utf8", input: opts.input, maxBuffer: 256 * 1024 * 1024 });
  if (!opts.ok && r.status !== 0) throw new Error(`${cmd} ${args.join(" ")}：${(r.stderr || r.stdout || r.error?.message || "").trim().slice(0, 800)}`);
  return r;
}

const GIT_ID = { GIT_AUTHOR_NAME: "peck-eval", GIT_AUTHOR_EMAIL: "eval@peck.invalid", GIT_COMMITTER_NAME: "peck-eval", GIT_COMMITTER_EMAIL: "eval@peck.invalid", GIT_AUTHOR_DATE: "2026-01-01T00:00:00Z", GIT_COMMITTER_DATE: "2026-01-01T00:00:00Z" };
const git = (ws: string, args: string[], ok = false) => sh("git", ["-C", ws, ...args], { env: { ...process.env, ...GIT_ID }, ok });

/** `repos.json` 里的名字 → 本机路径（相对仓库根，或 `~/`）。别的仓库只读：只 archive / clone 出来。 */
function repoPath(name: string): string {
  const map = JSON.parse(readFileSync(join(EVALS, "repos.json"), "utf8")) as Record<string, string>;
  const p = map[name];
  if (!p) throw new Error(`repos.json 里没有 ${name}`);
  return p.startsWith("~/") ? join(homedir(), p.slice(2)) : resolve(REPO, p);
}

/**
 * 建一个干净的工作区：只有起点那一笔提交（`history` 的题留起点以前的，没有远端、reflog、stash），没有 `evals/`；
 * 再把预热好的缓存（`HOME/cache/<仓库>/<路径>`）用 APFS 克隆拷进来。返回起点的提交号。
 */
export function materialize(task: Task, ws: string): string {
  rmSync(ws, { recursive: true, force: true });
  mkdirSync(ws, { recursive: true });
  const { source } = task;
  if (source.dir) {
    cpSync(join(task.dir, source.dir), ws, { recursive: true });
  } else if (source.history) {
    const from = repoPath(source.repo!);
    sh("git", ["clone", "-q", "--no-local", "--no-checkout", from, ws]);
    git(ws, ["checkout", "-q", "-B", "main", source.commit!]);
    for (const ref of git(ws, ["for-each-ref", "--format=%(refname)"]).stdout.split("\n").filter(Boolean)) {
      if (ref !== "refs/heads/main") git(ws, ["update-ref", "-d", ref]);
    }
    git(ws, ["remote", "remove", "origin"], true);
    git(ws, ["reflog", "expire", "--expire=now", "--all"]);
    git(ws, ["gc", "-q", "--prune=now"]);
  } else {
    const from = repoPath(source.repo!);
    const r = spawnSync("bash", ["-c", `set -o pipefail; git -C "$1" archive --format=tar "$2" | tar -x -C "$3"`, "bash", from, source.commit!, ws]);
    if (r.status !== 0) throw new Error(`导出 ${source.repo}@${source.commit} 失败：${r.stderr}`);
  }
  rmSync(join(ws, "evals"), { recursive: true, force: true });
  if (!source.history) {
    git(ws, ["init", "-q", "-b", "main"]);
    git(ws, ["add", "-A"]);
    git(ws, ["commit", "-q", "--no-verify", "-m", "start"]);
  }
  for (const c of source.caches) {
    const from = join(HOME, "cache", source.repo ?? task.id, c);
    if (!existsSync(from)) continue;
    mkdirSync(dirname(join(ws, c)), { recursive: true });
    sh("cp", ["-c", "-R", from, join(ws, c)]);
  }
  return git(ws, ["rev-parse", "HEAD"]).stdout.trim();
}

/** `*` 不跨 `/`，`**` 跨，`?` 一个字。 */
export function globMatch(pattern: string, path: string): boolean {
  let re = "";
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern[i];
    if (c === "*" && pattern[i + 1] === "*") { re += ".*"; i++; if (pattern[i + 1] === "/") i++; }
    else if (c === "*") re += "[^/]*";
    else if (c === "?") re += "[^/]";
    else re += c.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${re}$`).test(path);
}

export type TestResult = { name: string; group: Test["group"]; ok: boolean; timedOut: boolean; ms: number; tail: string };
export type Grade = { passed: boolean; score: number; healthy: boolean; health: string; changed: string[]; protectedChanged: string[]; tests: TestResult[]; infra?: string };

/** 一条判分命令（`sh -c`），到时间杀掉整组。 */
function runTest(t: Test, ws: string): Promise<TestResult> {
  return new Promise((done) => {
    const start = Date.now();
    const child = spawn("sh", ["-c", t.run], { cwd: ws, detached: true, env: { ...process.env, CI: "1" } });
    let out = "";
    const keep = (b: Buffer) => { out = (out + b.toString()).slice(-4000); };
    child.stdout.on("data", keep);
    child.stderr.on("data", keep);
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; try { process.kill(-child.pid!, "SIGKILL"); } catch { /* 已经退了 */ } }, t.timeoutMin * 60_000);
    child.on("close", (code) => {
      clearTimeout(timer);
      done({ name: t.name, group: t.group, ok: code === 0 && !timedOut, timedOut, ms: Date.now() - start, tail: out });
    });
  });
}

/**
 * 判分：先记下最终的 diff（对比 `base`；Git Plan 中途提交的也算）、查 `.git` 和不该改的文件，
 * 再放进隐藏测试（`check/files/` 拷进去、`append` 接到文件末尾），跑 check.toml 里的命令。
 */
export async function grade(task: Task, ws: string, base: string, diffFile?: string): Promise<Grade> {
  const head = git(ws, ["rev-parse", "HEAD"], true);
  const fsck = git(ws, ["fsck", "--no-progress", "--connectivity-only"], true);
  const ancestor = git(ws, ["merge-base", "--is-ancestor", base, "HEAD"], true);
  const healthy = head.status === 0 && fsck.status === 0 && ancestor.status === 0;
  const health = healthy ? "ok" : head.status !== 0 ? "HEAD 读不到" : fsck.status !== 0 ? `git fsck：${fsck.stderr.trim().slice(0, 300)}` : "起点不在 HEAD 的历史里";
  let changed: string[] = [];
  if (head.status === 0) {
    git(ws, ["add", "-A"], true);
    changed = git(ws, ["diff", "--cached", "--name-only", base], true).stdout.split("\n").filter(Boolean);
    if (diffFile) {
      const d = git(ws, ["diff", "--cached", "--binary", base], true).stdout;
      appendFileSync(diffFile, d);
    }
  }
  const protectedChanged = changed.filter((f) => task.check.protect.some((p) => globMatch(p, f)));
  const files = join(task.dir, "check", "files");
  if (existsSync(files)) cpSync(files, ws, { recursive: true });
  for (const a of task.check.append) appendFileSync(join(ws, a.file), "\n" + readFileSync(join(task.dir, "check", a.from), "utf8"));
  const tests: TestResult[] = [];
  for (const t of task.check.tests) tests.push(await runTest(t, ws));
  const okCount = tests.filter((t) => t.ok).length;
  const infra = tests.find((t) => t.timedOut) ? `判分命令超时：${tests.find((t) => t.timedOut)!.name}` : undefined;
  return {
    passed: healthy && protectedChanged.length === 0 && okCount === tests.length,
    score: tests.length ? okCount / tests.length : 0,
    healthy,
    health,
    changed,
    protectedChanged,
    tests,
    infra,
  };
}

export type Metrics = {
  requests: number;
  rounds: number;
  costUsd: number;
  tokens: { input: number; cacheRead: number; cacheWrite: number; output: number };
  subagents: number;
  queued: number;
  blockedWrites: number;
  loopWarnings: number;
  compactions: number;
};

/** 从 `events.jsonl` 数：请求、轮数、花费、token、子代理、排队、越界写被拦、原地打转、压缩。 */
export function metrics(eventsFile: string): Metrics {
  const m: Metrics = { requests: 0, rounds: 0, costUsd: 0, tokens: { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 }, subagents: 0, queued: 0, blockedWrites: 0, loopWarnings: 0, compactions: 0 };
  if (!existsSync(eventsFile)) return m;
  const queued = new Set<string>();
  for (const line of readFileSync(eventsFile, "utf8").split("\n")) {
    if (!line.trim()) continue;
    const e = JSON.parse(line);
    switch (e.type) {
      case "usage.updated":
        m.requests++;
        m.costUsd += e.cost?.usd ?? 0;
        for (const k of ["input", "cacheRead", "cacheWrite", "output"] as const) m.tokens[k] += e.usage?.[k] ?? 0;
        break;
      case "round.started":
        if (!e.subagentId) m.rounds = Math.max(m.rounds, e.round ?? 0);
        break;
      case "subagent.started":
        m.subagents++;
        break;
      case "board.updated":
        for (const it of e.items ?? []) if (it.status === "queued") queued.add(it.id);
        break;
      case "tool_call.completed":
        if (e.status === "denied" && /不在划给你的范围里|现在划给了/.test(e.output ?? "")) m.blockedWrites++;
        break;
      case "notice":
        if (/同样的参数、同样的结果/.test(e.text ?? "")) m.loopWarnings++;
        break;
      case "context.compacted":
        if (e.step === "summary") m.compactions++;
        break;
    }
  }
  m.queued = queued.size;
  return m;
}

export const rel = (p: string) => relative(REPO, p);
