# Wait

等待的小东西：3×3 点阵转圈（`pk-twinkle`，短的等待、页签上那边在跑）、光标（`pk-caret`，输入框有焦点）、7×7 点阵勾（`pk-dotcheck`，做完了）。

**写法**：
- 转圈：`<span class="pk-twinkle">` + 9 个 `<i></i>`（24 格一圈，steps）。
- 光标：`<i class="pk-caret"></i>`（20 格闪一次）。
- 点阵勾：`<span class="pk-dotcheck">` + 49 个 `<i>`，亮的格加 `is-on`（7×7，从左下到右上的勾，照预览抄）。

**减弱动态效果**：转圈停成满格、光标常亮。
