import { test } from "node:test";
import assert from "node:assert/strict";
import { parseToml } from "../lib/toml.ts";
import { globMatch } from "../lib/core.ts";
import { bootstrap, diffsOf, minDetectable, paired, passAt1, passHatK } from "../lib/stats.ts";

test("toml: the subset tasks use", () => {
  const t = parseToml(`# 注释
prompt = """
第一行
第二行 "引号"
"""
mode = "auto" # 行尾注释
tags = ["a", 'b',
  "c"]
max_rounds = 1_2
network = false

[source]
dir = "repo"
caches = []

[[test]]
name = "一"
run = 'cargo test --lib eval_check::'

[[test]]
name = "二"
env = { A = "1", B = 2 }
`);
  assert.equal(t.prompt, '第一行\n第二行 "引号"\n');
  assert.deepEqual(t.tags, ["a", "b", "c"]);
  assert.equal(t.max_rounds, 12);
  assert.equal(t.network, false);
  assert.deepEqual(t.source, { dir: "repo", caches: [] });
  assert.equal((t.test as { name: string }[]).length, 2);
  assert.deepEqual((t.test as Record<string, unknown>[])[1].env, { A: "1", B: 2 });
});

test("toml: mistakes name the line", () => {
  assert.throws(() => parseToml(`a = 1\nb = nope`), /第 2 行/);
  assert.throws(() => parseToml(`a = 1\na = 2`), /写了两遍/);
  assert.throws(() => parseToml(`a = "open`), /没有结尾/);
});

test("glob", () => {
  assert.ok(globMatch("tests/**", "tests/a/b.rs"));
  assert.ok(globMatch("src/*.ts", "src/a.ts"));
  assert.ok(!globMatch("src/*.ts", "src/x/a.ts"));
  assert.ok(globMatch("**/Cargo.toml", "src-tauri/Cargo.toml"));
  assert.ok(globMatch("Cargo.toml", "Cargo.toml") && !globMatch("Cargo.toml", "x/Cargo.toml"));
});

test("pass@1 averages per task, pass^k needs every run", () => {
  const tasks = [[true, true, true], [true, false, true], [false, false, false]];
  assert.ok(Math.abs(passAt1(tasks).mean - (1 + 2 / 3 + 0) / 3) < 1e-9);
  assert.ok(Math.abs(passHatK(tasks, 3).mean - 1 / 3) < 1e-9);
  const i = passAt1(tasks);
  assert.ok(i.lo <= i.mean && i.mean <= i.hi);
  assert.deepEqual(bootstrap([0.5, 0.5]), bootstrap([0.5, 0.5]), "固定种子");
});

test("paired difference and the noise floor", () => {
  const a = new Map([["t1", [true, true]], ["t2", [true, false]], ["t3", [false, false]]]);
  const b = new Map([["t1", [true, false]], ["t2", [true, false]], ["t4", [true, true]]]);
  assert.deepEqual(diffsOf(a, b), [0.5, 0]);
  assert.equal(paired(a, b).tasks, 2);
  assert.ok(Number.isNaN(minDetectable([0.1])));
  assert.ok(Math.abs(minDetectable([0.1, -0.1, 0.1, -0.1]) - (2.8 * Math.sqrt(0.04 / 3)) / 2) < 1e-9);
});
