# Toast

提示条和状态条。
- **提示条**（`pk-toast`）：反色的一句话（`color-text` 底、`color-page` 字），浮在窗口底部中间，几秒后消失；出现用 `pk-anim-reveal-s`（4 格）。写一句话说清楚为什么：「同一个项目一次跑一个任务」。
- **状态条**（`pk-bar`）：模块顶上一条，44 高，前面图标、中间一句话、右边一个按钮。等你（默认，琥珀边 + `color-warn-soft`）、通过 `pk-bar--ok`、失败 `pk-bar--bad`。

**写法**：`<div class="pk-toast"><i class="pk-i pk-i--bang pk-i--12"></i><span>…</span></div>`；`<div class="pk-bar"><i class="pk-i pk-i--ask"></i><span class="pk-bar-text">…</span><button class="pk-btn pk-btn--warn">BACK</button></div>`。
