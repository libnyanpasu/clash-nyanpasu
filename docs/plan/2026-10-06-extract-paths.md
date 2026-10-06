# 独立提取 nyanpasu-paths

## 目标与边界

在 `backend/nyanpasu-paths` 建立独立 crate，让目录与 binary 解析不依赖 Tauri，供 GUI、后续 core 和 CLI 共用。开发要求以 [development guides](../development/README.md)、[架构规范](../development/architecture.md)、[RPC](../development/rpc.md) 和 [测试规范](../development/testing.md) 为准。

- crate 负责：OS 默认目录、Windows portable／registry 的 config 目录优先级、目录创建、binary 搜索。
- GUI 负责：Tauri executable 与资源探测、应用身份、portable 标记、开发 sidecar 的源码树和 target-triple 命名、窗口／tray／widget／dialog。
- service 身份与命令模板、单实例名称不进入路径库：前者留在 `core/service/control.rs`，目录由调用方作为 `&Path` 传入；后者在 `utils/init`，签名为 `(app_name, config_dir)`。
- 不新增全局 service singleton、万能 locator、GUI 反向回调、仅用于取路径的 `NyanpasuClient` State 或 facade 接口。不开始 version/device、updater、effects、transactions 或 facade 的其他迁移。

## 设计

```text
HostInputs              GUI 提供的原始宿主事实，只采集一次
   │  PathResolver::discover(inputs)   唯一的发现步骤；不建目录
   ▼
PathResolver            Arc<Context>：config／data 根目录与 install dir 各一份；clone 只增引用计数
   │  纯函数：app_config_dir()、profiles_path()、app_logs_dir() …
   ▼
Utf8PathBuf             调用方拿到的路径值
```

- **单一上下文**：`PathResolver` 是系统中传递的唯一路径类型。不存在 resolved 容器类型，叶路径是对根目录的纯 join，不再重新发现根目录。
- **发现**：`discover` 在 `run()` 开头调用一次。失败（平台没有默认目录、目录不是 UTF-8、portable 布局缺 install dir）时 `panic_dialog` 后 `exit(1)`，因为之后每个阶段都需要两个根目录。
- **registry**：`registry` 模块（仅 Windows）读写“下次启动使用的”配置目录，按需读写不缓存；本进程只使用 `discover` 时的快照。discover 时 registry 读取失败按未设置处理。
- **创建目录**是显式 IO：`create_base_dirs` 先 config 再 data（传播错误），再 profiles／logs／cache（best-effort），由 `run()` 在单实例锁之后调用，抢锁失败的进程不会创建 data 根目录。`check_singleton` 自行先建 config 目录。CLI 子命令按原隐式创建的目录各自调用：`migrate` 两个根目录，`migrate-home-dir` 与 `is_fresh_install_instance` 只 config，`collect` 只 data。
- **动态查找**不缓存：`find_binary_path` 按 data → install → 开发候选搜索；`data_or_sidecar_path` 的 data 命中返回目录、未命中返回 install 下未检查存在性的文件路径，两者不合并。
- **UTF-8**：不是 UTF-8 的路径只在边界检查一次（`discover`、`installation_dir`），其后整个上下文都是 UTF-8。需要 `&Path`／`PathBuf` 的 std API 和既有字段用 `as_std_path()`／`into_std_path_buf()` 转换，不借此改成 Utf8 类型。
- **错误**用 snafu 领域错误（`InstallDirError`、`DiscoverError`、`CreateDirError`、Windows 的 `RegistryError`），路径库不使用 `anyhow`。`ServiceCommandError::ResolveServiceDirs` 的 source 是装箱的 `InstallDirError`，变体和 bindings 不变。

### 注入点

| 消费者                                                                                                                                                              | 获得方式                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `run()` 的 dhat、`cmds::parse`、`check_singleton`、迁移、迁移失败对话框、`logging::init`、`init_config`、`BundleMetadata` 的 install dir                            | `run()` 局部变量，按引用传入                                                                          |
| `setup::setup`                                                                                                                                                      | 参数；`app.manage(paths.clone())` 供桌面命令使用                                                      |
| 桌面命令：打开 config／data／logs 目录、`collect_logs`、`set_tray_icon`                                                                                             | `State<'_, PathResolver>`                                                                             |
| HTTP 命令：`collect_envs`、`get_cached_icon`、`get_tray_icon`、`set_tray_icon_from_bytes`、`is_tray_icon_set`、`get_service_install_prompt`；`/bridge/logs/archive` | `RpcDependencies.paths`；`nyanpasu-macro` 将 `PathResolver` 的 State 参数解析为 `&dependencies.paths` |
| 托盘图标                                                                                                                                                            | `tray/icon.rs` 的函数接收 `&PathResolver`；托盘与缩放回调从托管 state 取                              |
| widget                                                                                                                                                              | `widget::setup` 参数，`ProcessWidgetHost` 持有                                                        |
| 日志 reload 闭包                                                                                                                                                    | `init` 捕获 resolver 克隆                                                                             |
| bundle 资源目录                                                                                                                                                     | `utils::init::bundled_resources_dir`：setup 与 `invoke_uwp_tool` 共用                                 |
| client 的 core spec 查找                                                                                                                                            | `ClientSetupArgs.core_specs`；生产由 setup 注入真实查找，测试注入固定 spec                            |

## 必须保留的行为

- Windows config：portable → registry → OS default；data 不走 registry。目录名在 Windows／macOS 为 Title 大小写。
- 单实例占位字符串与 main 逐字相同；未获得锁就退出的进程不创建 data 根目录。
- managed binary 搜索 data → install → 开发候选，不缓存；`data_or_sidecar_path` 的不对称保留。
- `create_dir_all`：路径上是文件时先删除再创建。
- 日志导出先打开输出 writer／reopen tempfile，再处理目录；HTTP body 保持 tempfile 的持有和清理时机。
- service 安装参数的顺序和 `OsString` 处理、`update` 参数的平台差异与 main 相同。
- Serde／Specta／RPC 能力、owner、错误和事件契约不变；bindings 由原导出流程生成。

## 决策

- 死代码：本次必须修改才能编译的删除（`check_core_permission`、`grant_permission`），不涉及的不动（`PREVIOUS_APP_NAME`）。
- `CoreSpecError::CoreBinaryPathNotUtf8` 在 binary 路径改为 UTF-8 后不可达，删除；bindings 只因此变化。
- 非 UTF-8 的 data 根目录原来能部分工作，现在在 `discover` 时失败。实际上只有 Unix 上非 UTF-8 的 `HOME`／XDG 变量，或 Windows 上的非配对代理项会触发。

## 验证

在 Windows（MSVC，nightly）上验证最终提交：

| 检查                                                                             | 结果                                                                                                     |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `cargo clippy --workspace --all-targets --all-features`                          | 通过，无 error；GUI／依赖既有 warnings 保留，新 crate 无 warning                                         |
| `cargo test -p nyanpasu-paths`                                                   | 8 通过、1 原 ignored（`test_dir_placeholder`）                                                           |
| GUI lib 全量（`-p clash-nyanpasu --lib`）                                        | 1194 通过、4 ignored、0 失败；含 Specta 导出                                                             |
| `nyanpasu-macro` 测试（与 GUI 一起编译以统一 syn feature）                       | 8 通过，含 `PathResolver` 状态解析和桌面 dispatcher 检查                                                 |
| bindings                                                                         | 只有 `CoreSpecError` 去掉 `core_binary_path_not_utf8`                                                    |
| `nyanpasu` 前端 `tsc --noEmit`、`lint:ts:node`、`lint:ts:perf`、`lint:ts:tests`  | 通过                                                                                                     |
| `cargo fmt --all --check`                                                        | 通过                                                                                                     |
| `deno task lint:architecture-ledger`／`test:architecture-ledger`                 | 通过，36 个 allowlisted statics（移除 `IS_PORTABLE`），45 个 ledger 测试                                 |
| `cargo clippy -p nyanpasu-paths --target x86_64-unknown-linux-gnu --all-targets` | 通过（Unix 分支与非 UTF-8 测试仅编译检查）                                                               |
| `cargo tree -p nyanpasu-paths -e normal`                                         | 无 tauri／egui，无直接 anyhow／thiserror／whoami 依赖                                                    |
| 文本检查                                                                         | `host_paths` 只在 `run()` 调用一次；无 `ResolvedPaths`；路径库源码无 `Command::new`、`service`、`whoami` |

未验证：

- GUI 的 Linux 交叉检查被 `glib-sys` 缺 pkg-config 阻塞；macOS 无法在本地检查。Unix／macOS 分支的 GUI 代码（`cfg(not(windows))` 的 stub、托盘图标块）只经人工核对，交给 CI。
- 没有运行应用。需要在 verge-dev 身份下并行冒烟：首次启动创建目录；第二实例退出且不创建 data 根目录（用新的 portable 目录）；打开 config／data／logs 目录；桌面与 HTTP 的日志收集；自定义托盘图标的设置与读取；registry 自定义目录的读取与设置；service 安装提示与 main 一致；widget 启动后写入状态文件。
