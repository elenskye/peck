// 统计：pass@1、pass^k、按题配对的差，区间都用按题重抽（bootstrap，种子固定，同样的结果出同样的数）。
// 题是抽样的单位：同一道题的几次不独立，按次数算区间会偏窄。

/** 一道题在一个臂上：每次过没过（按第几次排好）。 */
export type Runs = boolean[];

export type Interval = { mean: number; lo: number; hi: number };

const B = 4000;

/** 固定种子的随机数（mulberry32）。 */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const avg = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : NaN);
const rate = (r: Runs) => avg(r.map((x) => (x ? 1 : 0)));

/** 按题重抽 `B` 次，取 2.5% 和 97.5% 分位。 */
export function bootstrap(values: number[], seed = 1): Interval {
  const mean = avg(values);
  if (values.length < 2) return { mean, lo: NaN, hi: NaN };
  const next = rng(seed);
  const means: number[] = [];
  for (let b = 0; b < B; b++) {
    let s = 0;
    for (let i = 0; i < values.length; i++) s += values[Math.floor(next() * values.length)];
    means.push(s / values.length);
  }
  means.sort((x, y) => x - y);
  return { mean, lo: means[Math.floor(B * 0.025)], hi: means[Math.ceil(B * 0.975) - 1] };
}

/** pass@1：每题的通过率再按题平均。 */
export function passAt1(tasks: Runs[]): Interval {
  return bootstrap(tasks.filter((r) => r.length).map(rate));
}

/** pass^k：前 k 次全过的题占多少（次数不到 k 的题不算）。 */
export function passHatK(tasks: Runs[], k: number): Interval {
  return bootstrap(tasks.filter((r) => r.length >= k).map((r) => (r.slice(0, k).every(Boolean) ? 1 : 0)));
}

/** 同一批题上 A − B 每题通过率的差（两边都有的题才算）。 */
export function diffsOf(a: Map<string, Runs>, b: Map<string, Runs>): number[] {
  const out: number[] = [];
  for (const [task, ra] of a) {
    const rb = b.get(task);
    if (ra.length && rb?.length) out.push(rate(ra) - rate(rb));
  }
  return out;
}

/** A − B：均值和区间。 */
export function paired(a: Map<string, Runs>, b: Map<string, Runs>): Interval & { tasks: number } {
  const diffs = diffsOf(a, b);
  return { ...bootstrap(diffs, 7), tasks: diffs.length };
}

/**
 * 噪声底：同一个配置跑两遍（A/A）每题的差 → 这么多题能看出的最小差别。
 * 双侧 5%、功效 80%：≈ (1.96 + 0.84) × 差的标准差 / √题数。
 */
export function minDetectable(diffs: number[]): number {
  if (diffs.length < 2) return NaN;
  const m = avg(diffs);
  const sd = Math.sqrt(diffs.reduce((s, d) => s + (d - m) ** 2, 0) / (diffs.length - 1));
  return (2.8 * sd) / Math.sqrt(diffs.length);
}
