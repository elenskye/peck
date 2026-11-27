# evals

PLOOP 的回归评测：固定的、带坑的编程题，一条命令跑完一组、自动判分、每次尝试的过程都留着。改 PLOOP 策略前后各跑一轮，用数字比。

## 命令

```bash
pnpm eval run evals/suites/<组>.json [--keys <Key 目录>] [--no-build] [--keep]
pnpm eval check [<题> …]
pnpm eval report evals/suites/<组>.json [--out evals/reports/<版本>-<名字>] [--baseline <旧报告.json>]
node --test evals/test/*.test.ts
```

- `run`：先编译 `peck`（debug），再按「第 1 次每题各臂、第 2 次……」排队，固定并发跑；有 `result.json` 的跳过，中途杀掉再跑同一条命令接着做；真跑的臂花到 `budgetUsd` 就停，停在哪写进 `stopped.json`。断线、没写出结果、到时间杀掉、判分命令超时记成 infra，自动重跑（最多 2 次，留在 `<第几次>.infra-<k>/`），不算模型没做对。
- `check`：每题三条都过才算合格：参考答案过、空改动不过、`alt/` 里每份换写法的解都过。
- `report`：一页中文（pass@1 带 95% 区间、pass^3、和第一个臂按题配对的差、噪声底、花费、计数）。结论、典型失败、下一步建议由读过过程的人写。

## 在哪

| 位置 | 放什么 |
| --- | --- |
| `evals/tasks/<题>/` | 一题一个文件夹（见「题目格式」） |
| `evals/arms/<臂>.json`、`arms/scripts/` | 臂的配置；共用的假模型剧本 |
| `evals/suites/<组>.json` | 一组跑哪些题、哪些臂、几次 |
| `evals/repos.json` | 题目来源的仓库名 → 本机路径（相对仓库根或 `~/`）；别的仓库只 archive / clone |
| `evals/reports/` | 发版的报告（`.md` + 同名 `.json`） |
| `projects/eval/runs/<组>/<题>/<臂>/<第几次>/` | 一次尝试的全部过程（仓库外，不进 Peck 仓库） |
| `projects/eval/work/`、`projects/eval/cache/<仓库>/<路径>` | 临时工作区；预热好的 `target/`、`node_modules`（`cp -c -R` 克隆进工作区） |

一次尝试的文件夹：`attempt.json`（给 `peck eval` 的参数）、`outcome.json`（怎么结束的、卡片和 ASK 次数、花费）、`events.jsonl`（`agent_events` 原样导出）、`final.diff`（对比起点）、`grade.json`（每条判分命令的结果和输出尾巴）、`result.json`（汇总：过没过、分数、从事件数的计数）、`peck.log`、`data/`（这一次的 Peck 数据目录）。

## 题目格式

```
tasks/<题>/
  task.toml        给模型的那句话、档位、来源、上限、分组
  check/check.toml 判分：两组命令、不能改的文件、要接到文件末尾的隐藏测试
  check/…          隐藏测试（files/ 下的拷进工作区同样的位置；append 指的接到文件末尾）
  reference.patch  参考答案（git diff，对比起点）
  alt/*.patch      换一种写法的正确解（至少一份）
  notes.md         坑在哪、为什么这样判分
  repo/            起点（source.dir 用）
  scripts/<名>.json  假模型剧本（臂的 script 用）
```

`task.toml`：

```toml
prompt = """站主会说的那句话（原样发，前面不加别的）"""
mode = "auto"              # plan / edit / auto
group = "regression"       # regression（该接近全过）/ capability（现在常做不对）/ holdout（只在发版跑）
tags = ["坑的类型"]
network = false            # 要联网的题报告里另算
max_rounds = 30            # 和臂的 maxRounds 取小的
timeout_min = 20           # 到时间打断，记 timeout

[source]
repo = "peck"              # repos.json 里的名字；或者 dir = "repo"（题目里的起点）
commit = "<要修的那笔的上一笔>"
history = false            # true：留起点以前的历史（没有远端、reflog）；默认只有一笔提交
caches = ["src-tauri/target", "node_modules"]
```

`check/check.toml`（命令在工作区根目录 `sh -c` 跑，退出码 0 算过）：

```toml
protect = ["src-tauri/Cargo.lock"]   # 改了就不过（glob；HANDOFF.md 别放：Git Plan 会让模型更新它）

[[append]]                          # Rust 的隐藏测试：写成 #[cfg(test)] mod eval_check { use super::*; … }
file = "src-tauri/src/harness/compact.rs"
from = "append/compact.rs"

[[test]]
name = "要修的"
group = "fail_to_pass"              # 改前失败、改后要过；至少一个
run = "cargo test -q --manifest-path src-tauri/Cargo.toml --lib harness::compact::eval_check::"
timeout_min = 10

[[test]]
name = "别的没改坏"
group = "pass_to_pass"              # 改前就过、改后还要过
run = "pnpm check"
```

TOML 只认这几种写法：注释、`[表]`、`[[表数组]]`、字符串（四种引号）、数、布尔、数组、行内表；别的报错带行号。只跑相关模块，别跑全量。测试只看行为，不卡实现细节。

## 加一道题

1. 建文件夹，写 `task.toml`、`notes.md`；起点用 `repo` + `commit`（真实提交的上一笔）或 `repo/`。
2. 写隐藏测试和 `check.toml`；在临时工作区里改好，`git diff` 存成 `reference.patch`；再写一份不同写法的存进 `alt/`。
3. `pnpm eval check <题>` 三条都过才算入库。连续很多次 0% 的题先查题。

## 臂和组

臂（`arms/<名>.json`）：`ui` 原样存进 Peck 的界面设置（`ploop.main` 是主角色 `api:<供应商>/<模型>`，`efforts`、`subagents`、`maxRounds` 照设置 → AGENTS 的格式）；`script` 有就用假模型照剧本答（每一步都要带 `usage`，不然 Peck 停下）、不花钱；`peck` 可选，写了提交号就只在那个提交上跑。

组（`suites/<名>.json`）：`name`、`tasks`（题号，或 `all`（不含 holdout）、`group:<组>`、`tag:<标签>`）、`arms`（第一个是对比的基准）、`attempts`（至少 3）、`concurrency`、`budgetUsd`（站主批的总预算，真跑必填）、`noise`（可选：两个同配置的臂，测噪声底）。

## 跑一组对比

1. 真跑前请站主批总预算（见 roadmap「现行授权」）；Key 目录用 `--keys` 或 `PECK_EVAL_KEYS` 给，不进任何文件。
2. `pnpm eval run`，再 `pnpm eval report`。
3. **读失败的过程**：报告列出没读过的失败，逐个看 `events.jsonl`、`final.diff`、`grade.json`，在 `projects/eval/runs/<组>/review.json` 记 `{"<题>/<臂>/<第几次>": {"verdict": "model" | "task" | "grader" | "infra", "note": "一句人话"}}`。题坏了、判分坏了的先修题再重跑；没读完的分数不算数。
4. 发版的报告：`--out evals/reports/<版本>-<名字>`，`--baseline` 指上一份的 `.json`，手写结论、典型失败、下一步建议。
