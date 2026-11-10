# Menu

菜单：ONE SHOT 左下「谁来答」弹出的单子。分组标题是点阵小字，每项前面是身份色点，名字点阵字，后面是一句小字；最后可以有一条动作（`pk-menu-action`）。

**写法**：
```html
<div class="pk-menu" role="menu">
  <p class="pk-menu-group">CLI</p>
  <button class="pk-menu-item is-on" role="menuitem"><i class="pk-dot pk-dot--muse"></i><b>MUSE</b><span>订阅额度 · 只读</span></button>
  <div class="pk-menu-sep"></div>
  <button class="pk-menu-action"><i class="pk-i pk-i--plus pk-i--12"></i>配 Key 用 API 模型<em>当场填</em></button>
</div>
```
边是青线 `color-cyan-line`，面是 `color-panel-2`。选中的项 `is-on`（`color-cyan-soft` 底、青字），不能用 `disabled`。

**尺寸**：行高 `size-menu-row` 30，左右 8；菜单内边距 8 / 6 / 6。
