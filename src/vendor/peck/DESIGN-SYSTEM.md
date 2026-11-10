Peck 是站主自己 Mac 上的 Coding Agent：三栏（项目和任务 · 对话 · 改动 / ONE SHOT / 文件 / 终端），CLI 和 API 两种模式。样子是**点阵终端 × 深海**：深色的底上一层 LED 点阵孔，青色锐利、会发光，是全屏唯一发光的颜色；其余的颜色清爽，只留给意思；角都是像素台阶，动起来像 LED 一格一格亮。

## 怎么用

- 页面或画板最外层写 `class="pk"`（字体、字号、正文色、页面底都从这里来），要铺 LED 点阵孔再加 `pk-dots`。
- **明暗**：根上写 `data-theme="dark"`（默认，第一主题）或 `"light"`，Peck 跟随系统写这一个属性。变量名两边一样（`--color-panel` 还是 `--color-panel`）。
- **样式**：先 `tokens.css`，再 `components/bundle.css`。挂到 Design 画布上时，画板在 `support.js` 那行后面依次写 `<link rel="stylesheet" href="ds/peck/tokens.css">`、`<link rel="stylesheet" href="ds/peck/components/bundle.css">`。没有 JS 组件：`components/bundle.js` 只声明 `window.Peck`，所有组件都是 HTML + 类名，照每个组件的预览抄。
- **字体**：缝合像素 12（名字、标题）和 Departure Mono（代码）在 `fonts/`；Doto（点阵字）和 IBM Plex Sans（说明）从 Google Fonts 取（`Doto:wght@900`、`IBM+Plex+Sans:wght@400;600`）；中文退到苹方（系统字体，只在 Mac 上有）。在画布上用：把 `fonts/` 的两个文件上传成资产，在 `<helmet><style>` 里给它们写 `@font-face`，Google Fonts 的 `<link>` 也放进 `<helmet>`。
- **类名**都以 `pk-` 开头。修饰写 `pk-btn--primary` 这样；状态写 `is-on`（选中、开着）、`is-off` 或 `disabled`（不能用）。`is-hover`、`is-focus`、`is-press` 只在说明页上把状态摆出来用，真界面交给 `:hover`、`:focus-visible`、`:active`。
- **字体样式**写 `t-` 开头的类（`t-name`、`t-body`、`t-code`……，见下面「字」）。
- 每个组件写在 `components/<名字>/README.md`，预览就是它的标准写法。

## 内容基调

- 中文为主，专有名词保留英文：Muse、Grok、API、Tailwind、pnpm、README.md。不叫「助手」，就叫模型的名字。
- **点阵字只写英文大写和数字**：按钮、分段、标签、模块头都是英文大写——`SEND`、`CANCEL`、`UNDO TASK`、`PLAN / EDIT / AUTO`、`[ CHANGES ]`、`NEW`、`DEL`、`2/3`。中文不进 Doto。
- **名字**（任务名、步骤名、问题、选项、项目名）是中文短句，用缝合像素：「把设置里三个对话框合成一个面板」「Grok 跑 pnpm check」「Tailwind 要不要一起升到 4.1？」。
- **说明**一句话，写发生了什么、谁在干：「第一次有报错，Grok 在改」「读了 12 个文件，列出 6 条要改的」「升级会顺手改 6 个文件的类名；不升也能做完这次任务。」。
- 数字和汉字之间空一格：「6 个文件」「还剩 3 步」；行数写 `+86 −132`。时间 `02:14`、`48s`、`1:02`。
- 不用 emoji，不用感叹号。演示只用编的项目：weather-cli、notes-sync、pomelo-site。

## 颜色

- **底 → 面**：`color-page` 是页面底（深色深海 / 浅色冷白），上面铺 `color-grid` 的 LED 点阵孔；模块、卡片、菜单的面是 `color-panel`；输入框、代码块、预览、菜单里面是 `color-panel-2`（内凹）；悬停抬起一档是 `color-raised`。模块的外层是 `color-line`，控件的边是 `color-line-strong`。
- **字**：名字和正文 `color-text`；说明 `color-text-2`；提示小字、时间、占位 `color-text-3`。
- **青色只表示「在发生」和「主要的那一下」**：主按钮、选中的分段、开着的开关是 `color-cyan`，上面的字和图标用 `color-on-cyan`；青字 `color-cyan-text`（选中、链接、键盘焦点）；进度块、光标、点阵 `color-cyan-block`；在跑的模块边、悬停时控件的边 `color-cyan-line`；选中的行 `color-cyan-soft`；扫描光 `color-cyan-scan`。**同一时刻只有一个青色主按钮。**
- **状态色只留给意思**：绿 `color-ok` 完成、通过、新增；琥珀 `color-warn` 等你批、等你答；红 `color-bad` 删除、出错、危险。各自有 `-soft`（状态条的底）、`-line`（模块、卡片的边）、`on-`（块上的字）、`-hover`。代码不按语法上色。
- **身份色**：`color-muse` 紫、`color-grok` 橙、`color-api` 蓝，只做 7 像素的方点（`pk-dot`）。
- **发光只在深色，只给青色**：`shadow-glow-dot`（点、光标）、`shadow-glow-block`（进度块）、`shadow-glow-text`（大号青字）、`shadow-glow-timer`（计时）、`shadow-glow-on-cyan`（青块上的字）、`shadow-glow-ink`（在跑那一步的白字微光）。浅色里这些都是 `none`，青换成深青 `#007A94`。
- **对比度按 APCA**（Peck 的字只有 12–12.5 号，门槛比常见的高）：名字和正文 ≥ 90；青块上的字 ≥ 75；说明 ≥ 70；青字、状态字、按钮上的字 ≥ 60；提示小字 ≥ 55；进度块、身份色点 ≥ 45；在跑、等你、出错的边 ≥ 30；控件的边 ≥ 15。36 对深浅都过，每个颜色 token 的用途里写了它能放在哪些底上。
- 主要的角色：

| token | 名字 | 深色 | 浅色 | 用在哪 |
| --- | --- | --- | --- | --- |
| `color-page` | 页面底 | `#050816` | `#F2F4FC` | 整页的底，上面是 LED 点阵孔 |
| `color-panel` | 模块面 | `#0A0F24` | `#FFFFFF` | 模块、卡片、菜单的面 |
| `color-panel-2` | 内凹 | `#0F1630` | `#F5F7FD` | 输入框、代码块、预览的底 |
| `color-raised` | 抬起 | `#172040` | `#E7EBF8` | 悬停的底、灰标签、计数小块 |
| `color-line-strong` | 控件边 | `#43528B` | `#B4BDDB` | 按钮、选项片、分段、开关的边；模块里的点线 |
| `color-text` | 正文 | `#E8ECFF` | `#0B1030` | 名字、正文、代码 |
| `color-text-2` | 说明 | `#BEC7F5` | `#4E567A` | 说明、次要的话、没选中的标签 |
| `color-text-3` | 提示 | `#9FAADE` | `#646C92` | 提示小字、时间、占位、模块头右边的话 |
| `color-cyan` | 青块 | `#00E5FF` | `#007A94` | 主按钮、选中的分段、开着的开关 |
| `color-on-cyan` | 青块上的字 | `#02101A` | `#FFFFFF` | 青块上的字和图标 |
| `color-cyan-text` | 青字 | `#00E5FF` | `#007A94` | 选中、链接、在发生；键盘焦点的点线 |
| `color-cyan-block` | 青格 | `#00E5FF` | `#0099B8` | 进度块、光标、点阵 |
| `color-ok` | 完成 | `#4ADE80` | `#12804C` | 完成、通过、新增 |
| `color-warn` | 等你 | `#FFB86B` | `#A35E00` | 等你批、等你答 |
| `color-bad` | 出错 | `#FE9AA4` | `#E11D48` | 删除、出错、失败、危险 |

## 字

四种字各管一件事，都是 OFL：

| 类 | 角色 | 字体 | 字号 / 行高 | 用在哪 |
| --- | --- | --- | --- | --- |
| `t-hero` | 首页大标题 | 缝合像素 12 | 24 / 30 | 首页「weather-cli 要做什么？」 |
| `t-title` | 标题 | 缝合像素 12 | 18 / 24 | 模块里的大标题、任务名 |
| `t-name` | 名字 | 缝合像素 12 | 12 / 18 | 任务名、步骤名、问题、选项、项目名 |
| `t-body` | 正文 | Plex Sans + 苹方 | 12.5 / 19 | 说明、回答、总结 |
| `t-small` | 小字 | Plex Sans + 苹方 | 12 / 18 | 提示、时间、模块头右边的话 |
| `t-code` | 代码 | Departure Mono | 11 / 18 | 代码、路径、命令、文件名（字号不小于 12 的唯一例外） |
| `t-label` | 标签 | Doto 900 | 13 / 16 | 标签、选项片、小按钮：英文大写 |
| `t-button` | 按钮 | Doto 900 | 14 / 16 | 按钮、分段、模块头 [ LABEL ] |
| `t-count` | 计数 | Doto 900 | 17 / 17 | 模块头右边的数 |
| `t-num` | 数字 | Doto 900 | 26 / 26 | 改动的文件数、进度百分比 |
| `t-timer` | 计时 | Doto 900 | 54 / 54 | 在跑的计时 |

- 字号不小于 12；唯一的例外是代码用的 Departure Mono 11。缝合像素只用 12、18、24（12 的整数倍才清楚）。
- 说明和回答用 Plex Sans（中文退到苹方），读长段不累；不要把整段话写成像素字或等宽字。

## 形状

- **像素阶梯角，没有圆角**：模块、卡片、菜单切 2 级 × 3px（`--pk-clip`）；按钮、选项片、分段、输入框 2 级 × 2px（`--pk-clip2`）；标签、单选多选、开关的钮 1 级 × 2px（`--pk-notch`）。都是 `clip-path`，所以边框画成两层：外层是边色，`:before` 是面。
- **模块两层**（`pk-mod`）：外层 `color-line`，里层 `color-panel`。状态只换外层：在跑 `pk-mod--run`（青线）、等你 `pk-mod--wait`（琥珀）、出错 `pk-mod--err`（红）。**同一时刻只有一块模块描彩边。**
- **LED 点阵孔**：页面底上 8 像素一颗（`pk-dots`）。
- **没有投影**；分开东西靠面的深浅、点阵和留白。
- **间距**只用 `space-1`…`space-12`（2、4、6、8、10、12、14、16、18、22、28、30）。三栏 `width-left` 288 / 自适应 / `width-right` 336，栏间 `width-gap` 16；窗口边上 22、左右 30、下 28。
- **尺寸**：`size-button` 34、`size-button-sm` 26、`size-chip` 28、`size-seg` 28、`size-input` 30、`size-field` 38、`size-row` 23、`size-menu-row` 30、`size-tag` 19、`size-check` 16、`size-switch-w` 34、`size-switch-h` 18、`size-icon` 16、`size-icon-sm` 12、`size-icon-lg` 24、`size-header` 44。

## 控件的状态

- **悬停**：有边的控件边亮成青线（`color-cyan-line`）、里面抬起一档（`color-raised`）；青块、琥珀、红底亮一档（`-hover`）；没边的行抬起一档。状态立刻变，不过渡。
- **键盘焦点**：控件里面内缩 3 像素一圈 1 像素点线（`color-focus`）。不画在外面：像素角会把外面的光圈切掉。青块、琥珀、红底上用块上的字色。
- **按下**：往下沉 1 像素，发光收起；有边的按钮里面凹一档（`color-panel-2`）。
- **不能用**：提示色字、内凹底、没有发光；为什么不能用写在旁边或悬停时说。

## 图标

- **界面图标**：8×8 像素格，一格 2 像素画成 16（列表行、小按钮、提示条里 12，空状态 24）。写 `<i class="pk-i pk-i--send"></i>`，颜色跟着字（`currentColor`），大小加 `pk-i--12` / `pk-i--24`。共 26 个：`check` 勾、通过、`x` 关掉、失败、`play` 开始、`ask` 问你、`bang` 注意、`term` 终端、`file` 文件、`folder` 文件夹、`shield` 权限、`globe` 联网、`trash` 删除、`pen` 改、`undo` 撤销、`bell` 通知、`send` 发出、`out` 展开输出、`clock` 时间、`lock` 锁着、`plus` 加、`expand` 展开到宽栏、`collapse` 收回、`chat` ONE SHOT、`triR` 收起的树、`triD` 展开的树、`stop` 停、`gear` 设置。不混别的图标集，不用 emoji。
- **App 图标 J2**（资产 App 组）：32 格的像素鸟，青 `#00E5FF`、冠羽 `#A8F6FF`，眼睛空着；深海渐变的底板占画面 7/8（第 1 步的比例），圆角 22.5%。每个尺寸一格都是整像素：`peck-j2.svg` 给 256 以上（1024、512、256 上一格 28、14、7 像素）；`peck-j2-128.svg`、`peck-j2-64.svg`、`peck-j2-32.svg` 是照 J2 缩的 28 格鸟（一格 4、2、1）；`peck-j2-16.svg` 是 14 格的。`peck-a3.png` 是现在的图标润色过的（A3），设置里能换成它。
- **菜单栏模板图**（资产 MenuBar 组）：18pt，14 格的鸟（和 `peck-j2-16.svg` 同一只），一格 1pt，@1x、@2x 都是整像素；黑 + 透明，由系统按明暗上色。空闲、在跑（`run-1`…`run-3` 每帧 300ms 轮流）、等你（问号）、在跑 N 个。

## 动效

一格 50ms，一步一格（`steps()`）；东西一格一格跳，不用平滑缓动；时长都是整格。光（扫描、亮块呼吸）可以平滑。

| 动作 | 什么时候 | 时长 | 走法 | 档位 | 类 |
| --- | --- | --- | --- | --- | --- |
| 控件状态 | 悬停、按下、选中、开关 | 0 格（0ms） | 立刻变，不过渡 | calm | （各控件） |
| 小东西出现 | 提示条、标签、菜单 | 4 格（200ms） | steps(4) | calm | `pk-anim-reveal-s` |
| 露出 | 卡片内容、回答一段段出来 | 8 格（400ms） | steps(8) | calm | `pk-anim-reveal` |
| 溶掉 | 撤销时改动一行行消失 | 8 格（400ms） | steps(8) | calm | `pk-anim-dissolve` |
| 卡片的边点亮 | 停下来问你、高危卡片出现 | 16 格（800ms） | steps(16)：上 → 右 → 下 → 左 | expressive | `pk-card-edge` |
| 发出 | 发任务：一格光从输入框飞到任务 | 12 格（600ms） | steps(6) | expressive | — |
| 扫一次 | 发出、等你、完成、撤销 | 18 格（900ms） | 平滑（光不是东西） | expressive | `pk-scan` |
| 做完闪两下 | 任务做完 | 18 格（900ms） | steps(1) | expressive | `pk-anim-flash` |
| 头一格变琥珀 | 停下来等你 | 20 格（1000ms） | steps(1) | expressive | `pk-blocks` 的 `is-wait` |
| 数字滚 | 撤销时文件数往下滚 | 18 格（900ms） | steps(n)，n 是滚过的数 | expressive | — |
| 光标闪 | 输入框有焦点 | 20 格（1000ms） | steps(1) | expressive，循环 | `pk-caret` |
| 点阵转圈 | 短的等待（3×3 点阵） | 24 格（1200ms） | steps(1) | expressive，循环 | `pk-twinkle` |
| 头一格闪 | 进度块正在走的那一格 | 16 格（800ms） | steps(1) | expressive，循环 | `pk-blocks` 的 `is-head` |
| 亮块呼吸 | 进度块亮着的格 | 48 格（2400ms） | 缓入缓出（只变亮度） | expressive，循环 | `pk-blocks` 的 `is-on` |
| 在跑那一步 | 弹扣一下 + 一圈光 | 28 格（1400ms） | 回弹 cubic-bezier(.3,1.6,.5,1) | expressive，循环 | `pk-run` |
| 在跑时扫描 | 在跑的那一步每 2.8 秒扫一次 | 56 格（2800ms） | 平滑 | expressive，循环 | `pk-scan--loop` |

- `calm`：控件的小动静，单次 ≤ 400ms，不循环。`expressive`：只给发出、等你、做完、撤销这几个时刻，单次 ≤ 1 秒；在跑和等待时才循环。
- **减弱动态效果**：只留最后的样子——光标常亮，转圈停成满格，扫描不出现，溶掉的直接不见。`bundle.css` 已经按 `prefers-reduced-motion` 写好；说明页、截图上要定格就在外层加 `pk-still`。

## 不要

- 不要做成 VSCode 那样：一层层灰面板 + 1 像素分隔线 + 满屏等宽字，青色只剩几个小点。
- 不要圆角、柔和阴影、渐变底（App 图标的底板除外）。
- 不要装饰性的循环动效；不要两个青色主按钮同时出现。
- 不要给浅色加发光；不要用纯青 `#00E5FF` 在浅色里写字。
