# FileRow

列表行：文件树、改动列表的一行。图标 12、文件名等宽（Departure Mono 11）、后面是标签和加减的行数。

**写法**：`<div class="pk-tree"><button class="pk-row"><i class="pk-i pk-i--file pk-i--12"></i><span class="pk-row-name">Composer.tsx</span><span></span><span class="pk-add">+9</span><span class="pk-del">−21</span></button></div>`
- 文件夹 `pk-row--dir`（名字 `color-text-2`，图标 `folder` / `triD`）；删掉的 `pk-row--del`（划掉、提示色）。
- 新文件放 `<em class="pk-tag pk-tag--new">NEW</em>`，删了放 `pk-tag--del`；行数 `pk-add`（绿）、`pk-del`（红）。
- 选中 `is-on`（`color-cyan-soft`，图标变青）。不能用 `disabled`。

**状态**：悬停抬起一档，按下 `color-cyan-soft`。

**尺寸**：行高 `size-row` 23，左右 6；行里的标签高 17。
