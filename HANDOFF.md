# 当前交接

更新：2026-11-10（本地账号）。**v0.3 完成；Phase 3（三栏外壳）进行中，交给云端账号在 `claude/peck-v0.4` 分支上做。** v0.4 = Phase 3 + Phase 4，做完停在「v0.4 等本地验收」。/Applications/Peck.app 还是 `0.2.0-alpha.4`，代码里是 `0.3.0`。

规则在 [CLAUDE.md](./CLAUDE.md)（云端先读「云端的边界」「云端开发」），Phase 和状态在 [docs/roadmap.md](./docs/roadmap.md)。

## 给云端：Phase 3 开工

- **分支**：从最新的 `origin/main` 切 `claude/peck-v0.4`，只推这一条。
- **视觉都在 `src/vendor/peck/`**（visual-atelier 方案 `peck-v0.1.0` 同步来的，不改）：`DESIGN.md` 是视觉基线，`peck-rules.md` 是外框规则 66 条（三栏、右栏四个页签、卡片、权限三档），`DESIGN-SYSTEM.md` 和 `components/<组件>/preview.html` 是 `pk-` 组件的写法。新外壳引 `bundle.css`，只用 `--color-*` 变量和 `pk-` 类名，不抄色值。`docs/design.md` 有用词对照、布局和「样子」的结论。
- **旧代码现状**：`src/components/` 还是旧外壳，文案是旧口径（Run #、Step、看板），Phase 3 整体换掉、旧组件能删就删。`src/index.css` 开头那段旧变量映射（同时认占位和正式 token 的名字）随旧组件一起删。主题现在固定深色（`src/main.tsx` 里 `applyTheme("dark")`），Phase 3 改成跟随系统（`src/lib/theme.ts` 已经支持 `system`）。
- **假数据**：`src/dev/mockData.ts`，3 个项目、6 个任务，已按新流程和用词（`docs/testing.md`「`?mock`」）。
- **不在 Phase 3**：TERM 的终端进程、发任务和撤销（Phase 4）；ONE SHOT 的内容（Phase 5、7）；额度与花费的实现（站主自己做）。
- **云端能验的**：`pnpm check`、`pnpm build`、无头浏览器开 `?mock` 明暗各看一遍；`cargo test` 编得过就跑，编不过在这里写明。

## 这次做了什么（本地账号）

- 核对了站主的提交：visual-atelier `27c2db4` 和 tag `peck-v0.1.0`（方案 `0.1.0` approved，78 个文件；画布、tooling 的改动都在）；Peck 的 `main` 是 `6b0614b`，已推到 GitHub。
- 按 tag 同步：62 个文件写进 `src/vendor/peck/`（样式、两个字体和 OFL 原文、`tokens.ts`、App 图标、A3、菜单栏模板图、DESIGN.md、外框规则、Design System 说明和 18 个组件示例）。
- roadmap：Phase 2 改 `完成`，Phase 3 改 `进行中`；删了和 CLAUDE.md 重复的「和 Dawn Plan 的关系」。architecture 的「视觉」一节、CLAUDE.md「边界」里 visual-atelier 那条跟着精简。

## 实际验证了什么

- `npm run sync -- --check --tag peck-v0.1.0`：一致。
- `pnpm check` 通过；前端构建通过，两个字体打进了产物。
- `?mock` 在内置浏览器里打开，旧界面接上了新颜色（深海底 `#050816`、青 `#00E5FF`）；只看了深色（界面现在固定深色）。
- 没跑 `cargo test`（没改 Rust）。没有真实 CLI、API 调用。

## 退回项

（没有）

## 下一步

1. **站主**：提交并推送这次的改动（`src/vendor/peck/` 和几份文档），云端才看得到正式的视觉。
2. **站主**：用另一个账号在 Claude Code 网页版连上 `elenskye/peck`，说「照 CLAUDE.md 开始 v0.4」。
3. **云端**：Phase 3 → Phase 4 → 版本号改 `0.4.0` → 在这里写「v0.4 等本地验收」和验收清单草稿。
4. **本地**：站主说「验收 v0.4」时照 CLAUDE.md「本地验收」。

## 留意

- 真实 API 账本：Opus 7/20、DeepSeek 46/100（roadmap「现行授权」），只有本地验收时用。
- Claude Code 状态栏脚本的接入没动（备份在 `~/.claude/settings.json.before-peck-usage-20261107-234334`）。
- 额度监测器（`src/quota/`、`src-tauri/src/quota/`）从 v0.5 起由站主用 Peck 做，Agent 不写。
