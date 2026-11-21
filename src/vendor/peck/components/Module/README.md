# Module

模块：三栏里的每一块、卡片、摘要。两层像素角：外层是边（`color-line`），`:before` 是面（`color-panel`）。状态只换外层的边。

**写法**：
```html
<section class="pk-mod pk-mod--run">
  <p class="pk-mod-head"><span class="pk-mod-label">TASK</span><span class="pk-mod-note">在跑 · 02:14</span><span class="pk-mod-count">3/4</span></p>
  …
</section>
```
- 模块头：`pk-mod-label` 是点阵字的 `[ LABEL ]`，`pk-mod-note` 是右边的小字，`pk-mod-count` 是右边的数。
- 边：在跑 `pk-mod--run`（青线）、等你 `pk-mod--wait`（琥珀）、出错 `pk-mod--err`（红）。**同一时刻只有一块模块描彩边。**
- 内凹的块（摘要、代码、预览）：`pk-mod--sunk`（面 `color-panel-2`）。

**尺寸**：内边距 14 / 18 / 16，模块之间 14–16，台阶 2 级 × 3px。
