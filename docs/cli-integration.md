# CLI 接入与无订阅开发

核对日期：2026-11-06。Peck 调用本机 CLI，不绑定两家订阅，也不代购或管理服务额度。

## 先选择一种运行方式

| 条件 | Peck 中的选择 | 能做什么 |
|---|---|---|
| 没有订阅、没有 API 额度 | 模拟 Agent / `@mock` | 检查调度、任务清单、文件看板和历史保存，不进行 AI 推理 |
| 只有一个可用 CLI | 对应的单 Agent / `@muse` 或 `@grok` | 将完整任务交给该 Agent，不调用另一家 |
| 两个 CLI 都可用 | Muse → Grok 流水线 | 第一步生成规范和清单，第二步据此实施 |
| 使用其他 CLI | 增加对应 TOML 并重新检测，输入 `@id` | 使用同一文件交接约定；无需改写 Rust 执行引擎 |

一个 Agent 也可以在一次任务中先规划再编码。默认双步流程只是一个可选编排，不是购买两份订阅的要求。

## 当前默认参数与官方依据

- Muse Code：`muse exec "{{prompt}}"`。Muse Wonder 是模型系列名称，不是 `generate` 子命令。[官方非交互说明](https://dev.meta.ai/docs/muse-code/extending)列出了 `exec` 的输出与退出码；[配置说明](https://dev.meta.ai/docs/muse-code/configuration)区分了交互与非交互参数。
- Grok Build：`grok -p "{{prompt}}" --output-format plain --no-auto-update`。依据[官方非交互说明](https://docs.x.ai/build/cli/headless-scripting)。工作目录由 Peck 的子进程 `cwd` 指定，交接文件要求包含在提示词中。
- Peck Mock：`peck-mock --prompt "{{prompt}}" --handoff "{{handoff}}" --write-files`，为内置实现，不需另行安装。

以上参数已按文档修正并测试模板渲染与本地调度，但尚未安装真实 Muse/Grok CLI、调用模型或验收账户权限。实际接入时先记录本机 `muse --version` / `muse exec --help` / `grok --version` / `grok --help` 的版本与输出，再执行真实工作区验收。

Peck 不默认追加 `--yolo`、`--always-approve` 或关闭沙箱的参数。请在 CLI 中配置适合非交互执行的工作区信任与权限策略；需要人工批准但没有终端界面时，CLI 可能拒绝操作，应根据输出配置后重试。

## 配置位置与编辑

在 Peck 的“设置 → 编辑配置”中修改并保存，下一次运行生效。也可以直接编辑：

- 开发/Debug：`~/Library/Application Support/Peck Dev/agents/*.toml`
- Release：`~/Library/Application Support/Peck/agents/*.toml`
- 测试隔离：`PECK_DATA_DIR` 可覆盖应用数据根目录。

应用首次运行会复制默认清单；对旧版内置参数的精确匹配会升级为新入口，保留其他配置值。自定义的 command/args 不会被替换。修改源码默认清单需重新构建；修改应用数据目录中的清单不需编译。

支持的字段：`id`、`name`、`description`、`command`、`args`、`cwd`、`stdin`（`none` / `prompt`）、`timeout`（如 `30m`）、`path`、`[env]`。

- `command` 支持二进制名或绝对路径；`~/` 会展开。`path` 可补充额外 PATH 目录。
- Agent 参数可使用 `{{prompt}}`、`{{handoff}}`、`{{workspace}}`。Peck 直接传递参数，不经 shell 拼接。
- 子进程继承 Peck 进程环境，再应用 `[env]` 覆盖。Finder 启动的应用通常不会继承某个终端中临时 export 的变量。
- 优先在对应 CLI 中完成登录；不要把真实密钥提交到项目。Grok 官方入门另列有 `XAI_API_KEY` 认证路径；Peck 不检查或声称账户已开通。API 访问也可能产生费用，与 CLI 是否安装是两件事。

自定义 CLI 示例（路径与参数需按实际工具调整）：

```toml
id = "my-agent"
name = "My Agent"
command = "/absolute/path/to/agent"
args = ["--prompt", "{{prompt}}"]
cwd = "{{workspace}}"
stdin = "none"
timeout = "30m"
path = []

[env]
```

自定义流水线保存在同一数据根目录的 `flows/*.toml`，步骤中的 `agent` 引用上述 `id`。可以把两个步骤都指向同一家 CLI。

## 状态、降级与失败

- “CLI 未安装”：找不到可执行文件；当前流程会改用内置模拟执行器，并在步骤和运行卡片中明确标注。
- “已检测到 CLI · 认证未验证”：本地程序存在；不代表登录、额度、模型权限或工作区权限已经通过。
- “模拟”：只验证 Peck 的工作流程。`@mock` 和缺失 CLI 降级都会写入 `.peck/handoff/<number>/intent.md` 与 `.peck/tasks.md`，请在测试项目中体验。
- 已安装的 CLI 返回认证、额度、权限错误或超时：流水线失败暂停，不转换为模拟成功；在原始输出中查看详情，修复后重试。
- 声明在 `expects` 中的文件未生成：即便 CLI 退出码为 0，步骤也会失败暂停。退出成功仍不代表代码功能正确，项目自身检查由对应 Agent 或开发者执行。

## 真实接入验收（具备访问条件后）

在独立测试项目中，每家可以分别验收，无需同时开通：检查版本/帮助、完成认证、运行单 Agent、确认文件产出与 Diff、验证权限不足或失败提示、再视需要测试双步流程。不在真实项目中用付费调用探测额度。
