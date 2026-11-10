# Ask

停下来问你的卡片：模型跑到一半要你定的时候，卡片占住输入框的位置，答完输入框才回来。学 Claude Code：一张卡装得下问题、说明、选项（单选、多选、判断）、推荐；要用到的权限放进同一张卡一次批。

**写法**：一块 `pk-mod pk-mod--wait pk-ask`，里面依次是：
- 模块头 `[ ASK ]` + 谁在问（`pk-who` + 身份色点）+ 第几题 `1/3`；
- 问题 `pk-ask-q`（缝合像素 18）、说明 `pk-ask-d`；
- 选项 `pk-ask-opts` 里放 Choice（`pk-opt`），推荐的带 `DEFAULT`；
- 底下 `pk-ask-foot`：左边 `LATER`（没边的按钮），右边主按钮 `NEXT` / `SUBMIT`。
- 出现时边一格一格点亮：卡片里放 `<span class="pk-card-edge"><i></i><i></i><i></i><i></i></span>`（上 → 右 → 下 → 左，16 格 800ms），内容 `pk-anim-reveal` 露出。

**不要**：一条条弹窗问权限；高危的权限默认勾上。
