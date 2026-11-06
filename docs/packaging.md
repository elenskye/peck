# 本机打包、安装与数据位置

Peck 的本机开发和打包不依赖 Muse/Grok 订阅，也不强制要求创建自签名证书。

## 命令

```bash
# 只检查工具与签名配置，不写入安装目录
bash scripts/install-app.sh --check

# Release .app，仅生成到项目 target 目录
pnpm app:package

# 较快的 Debug .app；使用 Peck Dev 数据目录
PECK_BUILD_PROFILE=debug pnpm app:package

# 显式安装到 /Applications；已有版本会保留为带时间戳的备份
pnpm app:install

# 安装到指定目录，可用于测试安装流程
PECK_INSTALL_DIR=/absolute/local/directory pnpm app:install

# 只检查缓存大小，不删除文件
bash scripts/clean-build.sh --check
```

默认签名身份为 `-`（ad-hoc 本机签名），生成后自动运行 `codesign --verify --deep --strict`。要使用已有证书，设置 `APPLE_SIGNING_IDENTITY="证书名称"` 后执行上述命令；脚本会先检查证书是否存在。[Tauri 官方签名说明](https://v2.tauri.app/distribute/sign/macos/)列出了该环境变量与 ad-hoc 方式。

Release 产物：`src-tauri/target/release/bundle/macos/Peck.app`。
Debug 产物：`src-tauri/target/debug/bundle/macos/Peck.app`。

本机 ad-hoc 签名不等于 Apple Developer ID 签名或公证。向其他用户分发前需另行配置相应证书与公证凭据；本次不创建证书、不修改钥匙串，也不将此状态标记为公开发布完成。

安装脚本先在目标目录临时复制并验证新包，再保留旧包并替换。仅执行 `app:package` 不会写入 `/Applications`。打包时不自动删除 Rust 缓存。

## 历史与运行产物

- Release 数据根目录：`~/Library/Application Support/Peck/`
- Debug 数据根目录：`~/Library/Application Support/Peck Dev/`
- `peck.db`：SQLite 数据库（工作区、会话、消息、运行与步骤）；运行记录还包含日志、任务清单、文件清单、执行计划与影子 Git 快照索引。
- `shadow/`：历史文件内容与差异所需的 Git 对象。保留历史差异需要同时保留数据库和此目录。
- `agents/`、`flows/`：可编辑的 Agent 与流水线配置。
- 各项目的 `.peck/handoff/<number>/intent.md` 与 `.peck/tasks.md`：CLI 间交接产物。

`PECK_DATA_DIR` 可将数据根目录改为独立测试目录。`PECK_SHADOW_ROOT` 可单独覆盖影子 Git 根目录。无需修改 HOME。

应用重开会将上次未完成的运行标记为中断失败；可查看历史、修复配置后从失败步骤重试，不会自动发起新的付费 AI 调用。历史差异读取保存的快照，不会把当前文件冒充成原运行结果。

退出应用后再备份数据库和 shadow 目录；数据库使用 WAL 模式，运行期间不要只复制一个正在使用的 `peck.db` 文件。
