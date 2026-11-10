# Button

按钮：点阵字、英文大写，两层像素角（外层是边、里层是面）。动词开头的短词：SEND、CANCEL、UNDO TASK。

**写法**：
- 普通：`<button class="pk-btn">CANCEL</button>`，边 `color-line-strong`、面 `color-panel`、字 `color-text`。
- 主按钮：加 `pk-btn--primary`，整块 `color-cyan`，字 `color-on-cyan`（深色带一点光）。**一屏只有一个。**
- 危险：`pk-btn--danger`（红边红字，「撤销整个任务」这类）；真要删的那一下：`pk-btn--fill`（红底，字 `color-on-bad`）。
- 琥珀：`pk-btn--warn`，只在「等你」的提示条和卡片里（BACK、去看看）。
- 没边的：`pk-btn--ghost`（COPY、CLEAR 这类小动作）。小按钮加 `pk-btn--sm`（高 26，模块头、预览头里）；只有图标的加 `pk-btn--square` 并写 `aria-label`。
- 图标放在字前面：`<i class="pk-i pk-i--undo"></i>UNDO TASK`，间距 7（小按钮 6）；只有 SEND 的箭头放在字后面（往前走）。
- 不能用：`disabled`（或 `is-off`）。

**状态**：悬停边亮成青线、面抬起一档（青块、琥珀、红底亮一档）；键盘焦点是里面一圈 1 像素点线；按下沉 1 像素、发光收起。

**尺寸**：高 `size-button` 34（小 26），左右 14（小 8），字 `text-button` 14（小 13），字距 .08em。

**不要**：中文写进按钮；一排两个主按钮；按钮套阴影或圆角。
