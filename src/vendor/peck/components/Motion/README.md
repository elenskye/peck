# Motion

动效零件：一格 50ms，一步一格。类名直接加在元素上，减弱动态效果时自动只留最后的样子。

| 类 | 动作 | 时长 |
| --- | --- | --- |
| `pk-anim-reveal-s` | 小东西出现（提示条、标签、菜单），从上往下露 | 4 格 200ms |
| `pk-anim-reveal` | 露出（卡片内容、回答一段段出来） | 8 格 400ms |
| `pk-anim-dissolve` | 溶掉（撤销时改动一行行消失），从左往右收 | 8 格 400ms |
| `pk-anim-flash` | 做完闪两下 | 18 格 900ms |
| `pk-scan` | 扫一次：放在 `position:relative; overflow:hidden` 的模块里，一道光从左扫到右 | 18 格 900ms |
| `pk-scan--loop` | 在跑时扫描，2.8 秒一次 | 56 格 |
| `pk-card-edge` | 卡片的边点亮：上 → 右 → 下 → 左 | 16 格 800ms |

在跑的那一步（`pk-run`）、方块进度（`pk-blocks`）、转圈和光标见 Steps、Progress、Wait。同一时刻只有一道扫描。
