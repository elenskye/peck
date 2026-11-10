# Steps

任务的步骤：一行一步，前面一格是状态，后面是名字（缝合像素）、时间（点阵字）和一句说明。

**写法**：`<ol>` 里每一步 `<li class="pk-step pk-step--run">`：`<span class="pk-step-node">`（状态）+ `<p class="pk-step-head"><b class="pk-step-name">…</b><span class="pk-step-time">…</span></p>` + `<p class="pk-step-desc">…</p>`。
- `--ok` 做完（绿勾 `pk-i--check`）、`--run` 在跑（`<span class="pk-run"></span>`：弹扣一下 + 一圈光，28 格一次；名字带一点白光，时间是青字）、`--wait` 等你（琥珀问号，闪）、`--todo` 还没到（空心方格，提示色）、`--fail` 失败（红叉）。
- 只有在跑的那一步动。
