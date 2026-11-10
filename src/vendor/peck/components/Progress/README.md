# Progress

任务的进度和读数：方块进度（`pk-blocks`）、状态字（`pk-state`：RUN / WAIT / DONE）、大号读数（`pk-readout`：计时、文件数）。

**写法**：
- 方块：`<div class="pk-blocks" style="--pk-n:28"><i class="is-on"></i>…<i class="is-head"></i><i></i>…</div>`。亮着的 `is-on` 慢慢呼吸（亮度，48 格），正在走的 `is-head` 闪（16 格）；停下来等你时头那一格换成 `is-wait`（琥珀），并加 `pk-blocks--still` 让亮块不再呼吸；做完也加 `pk-blocks--still`。亮块之间错开 70ms 呼吸（`animation-delay`）。
- 状态字：`<span class="pk-state">RUN</span>`，`--wait`、`--done`、`--err`、`--off`。深色是发光的字；浅色自动变成一块底（青块、琥珀淡底……）。
- 读数：`<b class="pk-readout">02:14</b>`（计时 54），`pk-readout--num`（26，文件数、百分比）；等你、做完时加 `--wait`、`--done`。

**样子**：亮格 `color-cyan-block` 带 `shadow-glow-block`（深色），没亮的格 `color-block-off`，格间 3，高 14（小的 12）。
