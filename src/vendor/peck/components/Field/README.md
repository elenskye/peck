# Field

输入：单独的一格输入框（填 Key 这类，`pk-field`），和中栏、ONE SHOT 底下的输入行（`pk-input`）。前面都有一个青色的 `›`（`pk-prompt`），光标是一格青块（`pk-caret`）。

**写法**：
- 输入框：`<label class="pk-field"><span class="pk-prompt">›</span><input placeholder="粘贴 DeepSeek 的 Key"></label>`；填好以后显示成 `<span class="pk-field-value">sk-••••••••3f2a</span>`（等宽、字距 .06em）；不能用加 `is-off`，旁边写为什么。
- 输入行：`<div class="pk-input"><span class="pk-prompt">›</span><span class="pk-input-text">…</span><i class="pk-caret"></i></div>`，没有边，放在模块里。

**样子**：输入框的边是青（`color-cyan`），面是内凹 `color-panel-2`，悬停面抬起一档。

**尺寸**：输入框高 `size-field` 38，左右 12，字 13；输入行高 `size-input` 30。
