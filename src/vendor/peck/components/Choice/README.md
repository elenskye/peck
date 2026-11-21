# Choice

单选和多选：ASK 卡片里的选项、设置里的一组。一行一个，前面是像素圆（单选）或像素方（多选），名字用缝合像素，说明用小字。

**写法**：
- `<button class="pk-opt" role="radio" aria-checked="true"><span class="pk-radio"></span><span><b class="pk-opt-name">一起升到 4.1</b><span class="pk-opt-desc">顺手改 6 个文件的类名</span></span><span class="pk-opt-key">1</span></button>`
- 多选把 `role` 换成 `checkbox`，圆换成 `<span class="pk-check"><i class="pk-i pk-i--check pk-i--12"></i></span>`。
- 推荐的那一项在名字后面放 `<em class="pk-tag pk-tag--rec">DEFAULT</em>`；选了会怎样写在说明里。
- 选中：`aria-checked="true"`（或 `is-on`），整行 `color-cyan-soft`。不能用 `disabled`。

**状态**：悬停抬起一档；按下 `color-cyan-soft`；焦点是里面的点线。

**尺寸**：格 `size-check` 16，行内边距 3 / 10，格和字之间 12。
