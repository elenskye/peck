# Segmented

分段和页签：几样里选一样（权限三档 PLAN / EDIT / AUTO、CLI / API），右栏的四个页签。选中的那块也切台阶角。

**写法**：
- 分段：`<div class="pk-seg" role="group"><button class="is-on">PLAN</button><button>EDIT</button>…</div>`；铺满加 `pk-seg--full`，矮一号加 `pk-seg--sm`（26）。
- 页签：`pk-seg pk-seg--full pk-tabs` + `role="tablist"`，每个 `role="tab"` 写 `aria-selected`。页签上的提示：数字 `<em>5</em>`（青字）、锁 `pk-i pk-i--lock`、点阵转圈 `pk-twinkle`（那边在跑）、角标 `pk-badge`（有新东西；`--ok` 做完、`--bad` 出错）。从不自己切走。

**状态**：没选中的悬停抬起一档；选中的悬停亮一档；焦点是里面的点线（选中的那块用 `color-on-cyan`）。

**尺寸**：一格高 `size-seg` 28，左右 12，格间 3，外框内边距 3；页签一格左右 7、字 13。
