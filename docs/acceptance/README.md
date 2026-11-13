# 验收清单页

每个版本的 Phase 都在云端做完以后，**本地账号**装好正式包，出一个云端页面给站主验收：像问卷一样，每一项勾「通过」或「退回」、写意见，答案自动保存。云端账号不出页（`CLAUDE.md`「云端的边界」），也读不到本地账号的页。

## 出页

云端分支压成一份改动合进 `main`、装好正式包以后（`CLAUDE.md`「本地验收」）：

1. 复制 `template.html` 为 `v0.x.html`（同目录；上一版的源文件删掉，发出去的页面还在，链接在 roadmap），把顶部 `<script id="acceptance-data">` 里的 JSON 换成这个版本的内容：
   - `version`：`"v0.4"`；`title`：一句话说这个版本；`installedAt`：装进 /Applications 的时间；`doc`：`"acceptance/v0.4"`（存答案的文档路径，每个版本一个）。
   - `groups`：按 Phase 分组，每组 `title` 和 `items`。每项 `id`（稳定、不重复）、`what`（验什么，站主看得懂的一句话）、`how`（在 App 里怎么做，一两步）、`agent`（验过的结果：云端的 `?mock`、本地的原生、真实服务分开写；没验的写「没验」）。从云端在 HANDOFF 里写的验收清单草稿出发。
   - 额度监测器（站主 v0.6 起用 Peck 开发）单独一组 `kind: "eval"`：写 Agent 的评估（能不能跑、数据对不对、代码质量、Peck 哪里不顺手），站主只写意见；还没开工的版本不放这一组。
   - 本地验收时 Agent 自己修过的问题单独一组，写清改了什么，让站主知道合进来的不只是云端分支。
2. 发到云端（claude-artifacts）：`publish_files` 把 `v0.x.html` 发成 `index.html`，标题「Peck v0.x 验收」，图标 `checklist`；再 `declare_capabilities {"db":{},"user":{}}`。
3. 把链接填进 `docs/roadmap.md`「版本与验收清单」表，HANDOFF 记一行。

## 读回

站主填完：`db_get`（slug 是那个页面的，文档 `acceptance/v0.x`）。`items[id].verdict` 是 `pass` / `return`，`note` 是意见；`overall` 是整体意见；`submittedAt` 有值表示站主填完了。退回的项写进 HANDOFF「退回项」（哪里不对、站主的原话），交给云端修；不改验收页。

页面只在云端跑（`window.claude.use('db')`）；本地打开只存在浏览器里，用来看版面。
