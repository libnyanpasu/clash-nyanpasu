# 启动失败处理实施计划

**日期：** 2026-10-29

**状态：** 已实施，经三轮审查，以 draft PR 交付。计划第二版按 codex 只读审计（gpt-6.1-sol，xhigh）的 9 个阻断项与 5 个文档项修订；实施中的偏离与审查结论见第 6 节。

**依据：** [启动失败分级 spec](../spec/2026-10-29-startup-failure-handling/design.md)（下称 spec）。本计划只写实施方式；分级、对话框选型与各项决定以 spec 为准。

**基线：** app `origin/main@8e1084e4e`（已含 #6045，runtime pin `v2.0.2` = `e73b9f5`）；utils 为 runtime `v2.0.2` 中的 gitlink。

下文路径前缀：`app/` = `backend/tauri/src/`，`utils/` = `backend/nyanpasu-runtime/crates/nyanpasu-utils/`，`manager/` = `backend/nyanpasu-runtime/crates/nyanpasu-core-manager/`，`jobs/` = `backend/nyanpasu-runtime/crates/nyanpasu-jobs/`，`reporter/` = `backend/nyanpasu-crash-reporter/`。

## 1. 交付形态

全部以 draft PR 交付。app 的 5 个 PR 组成 GitHub 原生 stack（加入 stack 后才跑 CI）。runtime 侧 PR 与 app 栈互不依赖：app 栈不使用 runtime 的新接口。

| 卡  | 仓库    | 分支                               | 内容                                                                                | spec 覆盖                  |
| --- | ------- | ---------------------------------- | ----------------------------------------------------------------------------------- | -------------------------- |
| U1  | utils   | `fix/startup-pid-record-degrade`   | `reap_epoch_pid_file` 返回分类错误；记录读写的暂时性 IO 重试；只改 utils 与其测试   | D1 前置                    |
| R1  | runtime | `fix/startup-pid-record-degrade`   | gitlink → U1 并适配全部调用方；孤儿清扫分两类；内核日志落盘重试与降级               | D1、D2                     |
| R2  | runtime | 同 R1 的 PR，第二个提交            | `nyanpasu-jobs` 的 redb 存储保留类型化错误（不再压成 `Journal(String)`）            | D5 前置                    |
| A1  | app     | `docs/startup-failure-handling`    | spec 与本计划                                                                       | —                          |
| A2  | app     | `refactor/setup-domain-errors`     | setup 调用链迁到 snafu 领域错误；迁移子进程结构化结果；行为不变                     | spec §3.2                  |
| A3  | app     | `fix/startup-degrade-local-data`   | 暂时性 IO 重试、损坏文件留证改名；派生与缓存数据隔离重建；悬空 `current`            | D3、D4、D6、D10            |
| A4  | app     | `feat/startup-failure-dialog`      | 报告器 `startup-failed` 模式；setup 失败弹窗后走正常退出流程收尾；迁移失败弹窗迁移  | N2–N4、N7，§3.1 权限与占用 |
| A5  | app     | `fix/startup-environment-failures` | 构建前与平台相关的失败：目录、WebView2、日志、Tauri 构建、deep-link、平台钩子与菜单 | D7–D9、N1、N5、N8          |

不在本次交付内：runtime 发版与 app bump pin；D5 在 app 中的隔离重建（R2 之后另做，见实施记录的"后续：D5 与共享 redb 分类"）。在那之前：

- D1、D2 的触发条件在 app 中按 N4 报告，不再 panic。
- `jobs.redb` 打不开时，app 无法区分权限、占用与损坏，A4 按"任务库不可用"报告：给出 `jobs.redb` 路径，说明该文件只保存任务历史，可以删除或移走后重启。

## 2. 共同约定

以下约定适用于每张卡，审查按此核对。

### 2.1 规范

- 遵循 `docs/development/` 全部指南，重点：[Rust 指南](../development/rust.md) §7（错误、panic）、§8（关停用根 `CancellationToken` 与注入的 `TaskTracker`）、§11（`tracing` 结构化字段，不新增 `log` 宏）；[测试指南](../development/testing.md)。
- runtime 与 utils 是独立工作区，沿用它们现有的错误框架（`thiserror`）与风格，不引入 snafu。deep-link 插件是 vendored fork，保持上游风格。
- 变更最小：不顺手改相邻代码；只删除自己造成的孤儿代码。
- panic 只留给程序不变量（例如内嵌配置缺字段）；环境、文件、用户输入导致的失败一律走错误路径。保留的 `expect` 写明不变量。

### 2.2 app 错误模型（A2 起）

- 每个模块自有 snafu 错误类型；变体名是失败的步骤，字段带该步骤的数据（路径、epoch、版本号等），`source` 是下游模块的错误类型。禁止全 crate 大 enum、`Custom(String)`、`{ op: &'static str }` 式字符串上下文。
- 迁移范围：setup 闭包（`app/lib.rs` 的 `.setup(...)`）与 `run` 中构建前阶段经过的函数，凡返回 `anyhow` 的都改为本模块 snafu 错误，直到遇到已有类型化错误的函数为止。同一函数也被非启动路径调用时，一并更新调用方。与启动无关的 `anyhow` 不动。
- **类型化 source 不得在边界上丢失。** 已知两处：profiles actor 启动时的物化恢复（`app/state/profiles/actor.rs:1648-1659` 把错误格式化成新的 `anyhow`，调用方只收到 `SpawnErr`）；迁移子进程（父进程只拿到退出码与 stderr）。前者把可能失败的准备工作移到 actor spawn 之前，以类型化错误返回；后者见 A2 的结构化结果。
- 已经是 snafu 的不重复迁移：`app/core/storage.rs` 的两个 `try_new` 已返回 `StorageOperationError`。

### 2.3 IO 分类、重试与留证改名（A3 起；U1、R1 在 runtime 内各自实现同样的规则）

- **暂时性：** `Interrupted`、`WouldBlock`、`TimedOut`、`ResourceBusy`；Windows `raw_os_error` 为 32（`ERROR_SHARING_VIOLATION`）或 33（`ERROR_LOCK_VIOLATION`）。最多重试 3 次，间隔 50、100、200 ms。耗尽后按"占用"报不可降级错误。
- **权限：** 对我们自己文件的 `PermissionDenied` 不重试、不改名、不降级，按不可降级报告，带路径。查询其他进程身份时的拒绝访问不属于这一条，见 U1。
- **只在"确认是数据问题"时降级：** 解析失败、格式不符或版本更新、节点类型不对（文件与目录互换、符号链接）。无法确认是数据问题的错误一律不降级。数据库被另一进程占用这类错误绝不能当成损坏去改名。
- **留证改名：** `<原名>.corrupt-<UTC>`，UTC 格式 `%Y%m%dT%H%M%SZ`，与原文件同目录；改名失败升级为不可降级。随后 `tracing::warn!`，字段含原路径、新路径、错误链与采取的动作。
- app 侧的分类与重试只实现一份（A3 的新模块），A4 的失败分类复用同一个分类函数。runtime 侧在 U1、R1 内另有实现：app 栈不依赖 runtime 发版，发版后是否收敛为一份另行讨论。
- **重试不得重复副作用：** 只重试文件操作，不重试发信号、不重试已经产生外部效果的步骤。

### 2.4 测试

- 用临时目录构造触发条件（写入垃圾内容、把文件换成目录、未知版本号等），断言"启动所需的构造成功 + 原文件已改名 + 新文件可用"或断言具体错误变体；不断言文本，不 sleep。
- 测试走**生产调用路径**，不只测 helper：helper 单测在生产调用被删除后仍会通过。第 4 节的测试矩阵给出每张卡的具体义务，每条写明"删掉哪段生产代码会让它变红"，审查时逐条核对。
- 权限错误用只读目录或拒绝写入的 ACL 构造；做不到的平台写明并跳过，不伪造。重试逻辑用注入的 IO 结果测试，不依赖真实的文件占用。

### 2.5 校验命令

- app：在 `backend/` 下 `cargo clippy -p clash-nyanpasu --all-targets --all-features`，`cargo test -p clash-nyanpasu`（只跑相关模块时写明过滤器），涉及报告器时 `cargo test -p nyanpasu-crash-reporter`。改动 `cfg(unix)`/`cfg(target_os = "linux")` 代码时补 `cargo check --target x86_64-unknown-linux-gnu`（能编译到哪一步就报告到哪一步）；macOS 代码本地无法编译，必须在报告里点名。
- runtime：U1 在 utils 自身工作区验证（clippy、测试）；R1、R2 在 `backend/nyanpasu-runtime/` 整个工作区验证。
- 工具链：默认 nightly 若出现 ICE，先原样重跑一次；仍 ICE 则改用 `cargo +nightly-2026-10-08`，不修改 `rust-toolchain.toml`，并在报告中写明。
- 格式：app `pnpm lint:rustfmt`；runtime、utils 用各自目录的 `cargo fmt --check`（utils 有独立的 rustfmt 配置）。

### 2.6 提交

- 实施者不提交、不推送；完成后报告改动、最终范围与验证结果，由我审查后提交。每张卡一个提交（必要时按完整行为拆分，每个提交可编译、可测试）。

## 3. 卡片

### U1 utils：回收错误分类

**目标：** 调用方能区分"记录不可用"、"存活进程无法证明归属"与"IO 失败"。对**未经证明的身份**从不发信号；重试从不重复发信号。

- `reap_epoch_pid_file`（`utils/src/process/pid_file.rs:284`）改为返回带分类的错误类型（`thiserror`），三类：
  - **记录不可用：** 内容无法解析、版本不受支持、文件名与内容 epoch 不符、`runtime_config` 无父目录或父目录不存在（`NotFound`）或越出运行目录、`runtime_config` 文件名 epoch 不符、`validate_runtime_target` 判定无效。
  - **无法证明归属或无法确认退出：** `identity_for_reap` 的进程查询失败（含耗尽后的拒绝访问，`pid_file.rs:417,627-645`）、`IdentityMatch::Uncertain`、对已证明归属的进程发信号后在等待窗口内无法确认退出（`:503,525-534`；此时信号已经发出，错误不能声称"未发信号"）、捕获的子进程无法确认退出。
  - **IO：** 运行目录 canonicalize 失败、读写记录的 IO 错误（含权限）、删除记录失败。
- 记录读取与删除遇到暂时性错误（§2.3）时重试 3 次（50/100/200 ms，`tokio::time::sleep`）。只重试文件操作。
- 只改 utils 与 utils 的测试（含 `tests/process_pid_file.rs:399` 对 `io::Error::kind()` 的断言）。runtime 调用方的适配在 R1。

### R1 runtime：孤儿清扫分两类、日志落盘降级

**依赖：** U1。本卡包含 gitlink 更新与全部调用方适配：`manager/src/manager/quarantine.rs:28,148`、`manager/src/.../instance.rs:334,510,544`（以代码为准）。

- `sweep_orphans`（`manager/src/manager/quarantine.rs`）：
  - **记录不可用：** 把 `core-<N>.pid` 改名为 `core-<N>.pid.corrupt-<UTC>`，`tracing::warn!`，继续清理该 epoch 的控制器与其他产物。改名失败返回错误。
  - **无法证明归属：** 记一条 `QuarantinedEpoch { death_proven: false }`，保留该 epoch 的全部产物（`recover_quarantine` 需要重读记录），继续下一个 epoch。epoch 为 0 时无法构造 `Epoch`，按"记录不可用"处理。
  - **IO：** 照旧返回错误。
  - 现有匹配规则已经排除改名后的文件（`config/runtime_store.rs:313-340,385-408`），补回归测试证明它既不被当作产物发现，也不被清理，不扩大发现规则。
- `build_configured`（`manager/src/manager/mod.rs:305`）：把清扫得到的 quarantine 条目放入 `Ctrl`，并像 `latch_quarantine`（`quarantine.rs:11`）一样发布终止状态。仅让 reconcile 被拒绝不够，状态订阅方必须从构造完成起就看到 quarantine 原因。epoch 分配器种子不变。
- **D2：** 日志落盘的准备与打开（`mod.rs:361-372`，`log_sink.rs:136-175` 中的文件操作）对暂时性错误重试 3 次；耗尽或权限错误照旧返回错误；确认是节点类型等数据问题时 `warn` 后关闭落盘（`log_dir`、`log_sink` 为 `None`），内存日志广播照常。

### R2 runtime：jobs 存储保留类型化错误

**依赖：** R1 之后的同一 PR。

- `jobs/src/storage/redb.rs:10-17` 把 redb、IO、JSON 错误压成 `Error::Journal(String)`；改为保留类型化 source 的变体（打开、建表、提交、记录解码分开），`jobs/src/lib.rs:75-85` 的恢复与修剪路径原样向上传递。沿用该 crate 的错误框架。
- 调用方（含 app 无关的 runtime 内部调用）只做编译适配，行为不变。
- app 侧 D5 的隔离重建在 runtime 发版、app bump pin 之后另做。

### A1 docs

提交 spec 与本计划。

### A2 setup 调用链迁到 snafu

**目标：** 启动失败时，调用方拿到的错误能按变体匹配，跨 actor 与子进程边界不丢类型；行为不变（setup 仍把错误交给 Tauri）。

- `app/setup.rs`：模块错误 `Error`，变体按 setup 的步骤命名（shutdown hook、迁移准备、迁移执行、service IPC 客户端、安装目录、内核宿主、内核 actor、service actor、jobs、effect owner、storage、debug HTTP 前端、client、widget 启动、widget 安装），`source` 为下游类型化错误。`setup_unified_rpc` 同样改为自己的错误类型。
- 按 §2.2 迁移下游，至少包括：`core/actor_v2/local_host.rs::build`、`core/migration/runner.rs` 的 `with_paths`/`run_pending`、`client/jobs.rs::start`、`client/system_proxy/mod.rs::spawn`、`client/hotkey/mod.rs::spawn`、`client/ui_effects/adapters.rs::install`、`shutdown_hook.rs::setup_shutdown_hook`、`client/runtime.rs::cleanup_stale_candidates`、`client/mod.rs` 的 `try_new_with_args`、`new_typed_config_clients`、`with_parts`，以及 `ApplicationClient::new`、`SessionStateClient::new`、`ClashConfigClient::new`、`ProfilesClient::new_with_jobs`、`widget::setup` 中返回 `anyhow` 的部分。以代码为准补全或删减，在报告中列出最终清单。
- **profiles 物化恢复：** 把 `state/profiles/actor.rs:1648-1659` 中可能失败的准备工作移到 actor spawn 之前，以类型化错误经 `ProfilesClient` 构造返回。
- **迁移错误：** 为"由更新版本写入"的三种情况建类型化变体：store schema 版本（`migration/store.rs:69`，区分高于与低于支持版本）、文档修订号（`runner.rs:242`）、已应用模块修订号（`runner.rs:335`），字段带读到的与支持的修订号。文案只说"由更新版本写入"，不承诺具体的应用最低版本号。
- **迁移子进程的结构化结果：** 父进程（`app/utils/init/mod.rs`）给 `migrate` 子命令传一个结果文件路径；子进程（`app/cmds/migrate.rs`）失败时写入 JSON（失败种类、相关路径、错误链文本、备份是否可用），覆盖 runner 构造失败、备份失败与迁移失败；父进程读取它构造类型化错误，读不到时退回现有的"退出码 + stderr"。备份失败目前靠 `BACKUP_FAILED_EXIT_CODE` 区分，改由结果文件表达后删除该退出码约定（若无其他用途）。
- `app/lib.rs` 的 setup 闭包仍把错误交给 Tauri（本卡不改行为）。

### A3 本地数据隔离重建

**依赖：** A2。

- 新增 app 模块（建议 `app/core/corrupt.rs`，命名以审查为准）：§2.3 的暂时性判断、同步与异步的有限重试、留证改名。
- **D3** `session-state.yaml` 解析失败 → 改名，默认值启动。**所有读取点用同一个函数处理**：迁移的 typed_config 校验（`migration/modules/typed_config.rs:214,218,344`，迁移子进程与进程内两次执行都经过，`runner.rs:114` 在迁移完成后仍会校验）以及 `SessionStateClient::new`。改名必须发生在迁移的部分文件检查之前。
- **D4** `storage.db`：数据库损坏、格式不符或版本更新 → 改名后新建；`storage.rs:239` 的 `remove_file(...).unwrap()` 与 `:251` 的 `remove_dir_all(...).unwrap()` 改为返回错误（失败时按 §2.3 判断）。启动期所有打开 `storage.db` 的地方行为一致：`Storage::try_new` 与迁移的 baseline/check/run（`migration/modules/storage.rs:42,109,240`）。丢弃旧 KV 会丢失旧安装中由它承载的热键配置（`:190-194`），在 warn 与实施记录中写明。
- **D6** `runtime/.candidates`、`profiles/.profile-materialization-v1` 等派生目录节点类型不对 → 改名后重建；单个过期候选文件删除失败只 `warn` 并跳过。profiles 的物化目录经 A2 移到 spawn 之前的准备步骤处理。
- **D10** `profiles.yaml` 的 `current` 指向不存在的配置：只清空该悬空选择并写回文件，`warn` 中记下原 ID。不用 `sanitize_top_level`（它还会删掉全局 transform，`nyanpasu-config/src/profile/profiles.rs:159-168`）。`CurrentNotConfig` 等其他校验失败仍为不可降级。
- 每项遵守 §2.3：权限错误、无法确认的数据问题、重试耗尽都返回错误，由 A4 报告。

### A4 启动失败对话框与不 panic 的退出

**依赖：** A3。

- **分类：** 新增 `StartupFailure { kind, path, detail }` 与 `FailureKind`（目录、迁移失败、由更新版本写入、用户配置、内核运行目录、任务库、WebView2、日志、权限、占用、内部错误）。由 `setup::Error`、`setup_unified_rpc` 的错误与 A2 的迁移结果映射得到，映射走类型化 source，IO 判断复用 A3 的分类函数。`detail` 为完整错误链（`snafu::Report`）。用户配置错误带文件路径与行列号（解析错误提供时）。
- **报告器：** `reporter/` 新增启动失败窗口（新的 Slint 组件与 `show_startup_failure` 入口）。复用 `platform.rs` 中已有的后端选择、居中与字体（`:8,53,76`）；剪贴板与打开目录目前写在 `lib.rs` 的窗口逻辑里（`:95,177`），按相同做法实现或抽成共用函数。内容：标题、说明、恢复步骤、路径、可复制的详情；按钮"打开所在目录 / 打开备份目录 / 打开日志目录"按 kind 出现，点击后**不关窗**；"复制详情"；"退出"关闭。文案由宿主按语言传入，与 `ReporterText` 的做法一致。
- **宿主子命令：** `app/cmds` 新增 `startup-failed --language <tag>`，从 stdin 读取 JSON 输入（与 `panic-dialog` 相同，读到 EOF，`app/cmds/panic_dialog.rs:24`）。新增文案放入 app 的 rust-i18n 语言文件，覆盖全部语言（`en`、`ja`、`ko`、`ru`、`zh-cn`、`zh-tw`）。
- **弹窗：** 宿主函数拉起子进程、写入 JSON、**显式关闭子进程 stdin**、等待其退出。序列化、写入、等待失败，或子进程非零退出，都退回 rfd 一次性错误对话框，正文含路径与日志位置。
- **setup 失败不再交给 Tauri，收尾交给正常退出流程：**
  - setup 在每个 owner 构造成功后立即登记其清理：tracker 内的任务由根 token 与 `TaskTracker` 覆盖；内核 actor、service actor 等不在 tracker 内的 owner（`actor_v2/mod.rs:1399`、`service_actor.rs:1052` 丢弃了 join handle；内核 pump 持有 actor 引用，`:659,681-685`）在构造后立即用 `drain_on_shutdown` 或等价方式纳入 tracker，并保留内核句柄以便像 `NyanpasuClient::wait_shutdown` 那样在最后停止内核。覆盖 service 构造失败但内核已启动、client 构造失败等每个失败点。
  - setup 失败时：`tracing::error!` 记录完整错误链 → 弹窗（阻塞）→ 把"部分启动的 owner"（根 token、tracker、内核句柄）交给 Tauri 托管状态 → `app.handle().exit(1)` → 返回 `Ok(())`。
  - `utils/exit.rs` 的 `request_shutdown`/`start_shutdown` 在没有 `NyanpasuClient` 时改用该部分启动状态：取消、关闭、等待 tracker，再停止内核。收尾在事件循环继续运行时进行，所以需要主线程的清理（热键 `post_stop` 的 `unregister_all`，`client/hotkey/actor.rs:108`）照常执行。**不在 setup 内阻塞主线程等待收尾**：那会与需要主线程的清理互相等待而死锁。不加超时。
  - Windows shutdown hook 线程与日志线程不在 tracker 内：确认它们在退出时不需要额外清理，并保证 setup 失败的错误日志在进程结束前落盘；在报告中说明依据。
- **迁移失败（构建前）：** `lib.rs:221-232` 改用报告器（迁移失败、由更新版本写入两种 kind；备份不可用时不提供"打开备份目录"），之后从 `run` 返回失败退出码；`main` 改为返回 `ExitCode`，不再 `run().unwrap()`（`mobile_entry_point` 接受非单元返回类型，已核实）。`migration_failed_dialog` 无调用方后删除。
- **任务库：** `StartJobs` 映射到"任务库不可用"，见第 1 节。

### A5 构建前与平台相关的失败

**依赖：** A4。

- **D7** Windows shutdown hook 创建失败 → `warn` 并跳过。
- **D8** macOS Edit 菜单三处 `unwrap`（`lib.rs:362-364`）→ `warn` 并跳过。
- **D9** deep-link 插件：Linux 监听线程 `bind(...).expect`（`linux.rs:110`）、macOS debug（`macos.rs:152`）、Windows 的监听创建（`windows.rs:98`）、运行时创建（`:145`）、对 `argv[1]` 的 `expect`（`:166`）、socket 写入与 flush（`:170,174,175`）→ 记录错误并返回，深链接不可用。
- **N1** 目录解析与创建失败（`lib.rs:155-161`、`:213-219`）以及安装目录解析（`:313`）：rfd 一次性对话框补充恢复步骤（路径、检查权限与剩余空间、便携版移到可写位置），从 `run` 返回失败退出码。
- **N5** 固定版 WebView2 不完整（`bundle.rs:33-37` → `lib.rs:316`）：便携版 → rfd 一次性对话框"重新下载便携版"，返回失败退出码；安装版 → 若系统装有 WebView2（`tauri::webview_version()`，`tauri-2.12.5/src/lib.rs:209`），不设置固定运行时路径，`warn` 后继续，`is_fixed_webview` 仍用于选择更新包；系统也没有时按便携版同样报错，文案改为"重新安装"。`metadata.setup`（`:319`）只依赖内嵌配置，保留为写明不变量的 `expect`。
- **Tauri 构建失败**（`lib.rs:405`）：按内部错误报告（报告器，带详情），返回失败退出码。
- **N8** 日志初始化（`lib.rs:235`）与 `migration.log` 创建（`app/utils/init/mod.rs:37`，位于 `app_data_dir` 而非日志目录）失败 → **rfd 一次性对话框**（spec 已定），报告实际失败的文件或目录，返回失败退出码。`migration.log` 创建失败与迁移子进程失败分开报告。

## 4. 测试矩阵

| 卡  | 必须有的测试                                                                                                                                                                                                                                                                               | 删掉哪段生产代码会变红                             |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| U1  | 经 `reap_epoch_pid_file` 公开入口：每类错误各一例，含进程查询拒绝访问与发信号后无法确认退出；记录读取、删除的暂时性错误重试成功与耗尽（注入）                                                                                                                                              | 分类映射；重试调用点（不只是重试 helper）          |
| R1  | 损坏记录、`version=3`、`runtime_config` 父目录不存在：构造成功、记录已改名、无信号；存活且无法证明归属：构造后**首个状态**即为 quarantine 原因、reconcile 被拒、产物保留、recover 可重试；改名文件不被发现也不被清理；日志落盘：暂时性成功、暂时性耗尽、权限、节点类型错误后内存日志仍送达 | 两类分支；构造时的状态发布；D2 的降级分支与重试    |
| R2  | 损坏的 `jobs.redb`、损坏的记录：错误为对应的类型化变体                                                                                                                                                                                                                                     | 每个新变体的构造点                                 |
| A1  | 无生产测试；核对文档引用、基线与卡片依赖                                                                                                                                                                                                                                                   | —                                                  |
| A2  | 损坏的 `application.yaml`；store schema 高于与低于支持版本；文档修订号过新；已应用模块修订号过新；profiles 物化准备失败经 `ProfilesClient` 构造返回类型化 source；迁移子进程结果文件往返（含备份失败）                                                                                     | 每个新变体；子进程结果写入与读取                   |
| A3  | 按真实启动顺序（迁移 → client 构造）测试 D3、D4：证据文件保留、重建的数据可用；D6 两个目录；D10 悬空 `current` 与 `CurrentNotConfig` 分开测；权限错误不改名（可构造的平台）；重试经生产调用点                                                                                              | 每个降级分支；迁移路径中的降级调用点               |
| A4  | 注入一个在 owner 启动之后的真实 setup 失败：断言 owner 收到清理、弹窗函数被调用、退出被请求、闭包返回 `Ok(())`；每个 kind 的分类映射；`cmds` 分派到新子命令（不只是解析）；报告器渲染测试断言文案与按钮回调存在（不只是尺寸）                                                              | setup 失败分支；部分启动状态的登记与收尾；分类映射 |
| A5  | WebView2：断言最终运行时配置与保留的更新包选择（不只是纯函数）；deep-link 失败路径返回错误而非 panic（能在本平台构造的部分）；其余按可测性补充并在报告中说明                                                                                                                               | 回退分支的生产调用；deep-link 各降级分支           |

## 5. 风险与待核实

- spec §9 中出自 codex、未逐条复核的行号与触发条件，在对应卡开始前核实。
- 报告器窗口的视觉效果需要用户确认后再推送（参照 crash reporter 的做法）。
- macOS 代码本地无法编译，依赖 CI。

## 6. 实施记录

实施由 codex 按卡完成（U1、R1、A4 用 gpt-6-astra，其余用 gpt-6.1-sol），每张卡经我审查后提交。

**各卡的偏离与原因**

- **U1：** `OrphanReapError` 去掉了 `#[non_exhaustive]`，让 runtime 调用方穷尽匹配。测试通过 `task_local` 注入进程身份与 IO 故障，不依赖真实进程时序。
- **R1：** 改名后的记录与同秒内已有的证据重名时报错，不覆盖证据。
- **R2：** `nyanpasu_jobs::Error` 因携带 redb、IO、JSON 的 source，不再派生 `Clone`、`PartialEq`；app 侧 `ClientError::Jobs` 不受影响。
- **A2：** 迁移范围比卡面更大：启动链上各 actor 的 `spawn`、迁移模块内部的步骤函数都改成了各自模块的 snafu 错误。保留 `anyhow` 的只有三处既有边界：`Format` 序列化端口、已类型化的状态错误内部、CLI 报告器子命令。无法失败的 `widget::setup` 与 debug 前端选择去掉了 `Result`。
- **A3：** D10 的悬空 `current` 清理也作用于迁移前的 legacy 校验，否则迁移会先失败。macOS 上目录改名用 `renamex_np(RENAME_EXCL)`：`atomicwrites` 在非 Linux Unix 上靠硬链接，移动不了目录。`StorageOperationError` 新增的两个变体经 RPC 暴露，前端错误映射与绑定同步更新。
- **A4：** 实施会话在上下文耗尽时中断，由新会话接着完成。审查时撤回了两处影响**正常退出**的改动：内核关停改成无上限等待，以及 service actor 退出时等待未完成的提权命令（UAC 未关时会挂住退出）。部分启动的收尾沿用带预算的内核关停。启动失败记录在弹窗前同步写入日志文件并确认。
- **A5：** deep-link 的 Windows 监听改为先同步创建再起线程，创建失败返回错误。安装版回退系统 WebView2 时 `is_fixed_webview` 仍决定更新包。debug 下导出绑定失败也走内部错误报告。

**审查（`/ccg:review`，codex 后端 + kimi 界面，三轮）**

- **第一轮修复（折叠进 A3、A4、A5）：**
  - 正常退出不再等待日志线程。
  - shutdown hook 的线程创建失败返回错误，不再 panic。
  - 迁移读取 YAML、文档与迁移状态时走暂时性 IO 重试。
  - 退出时选择 client 还是部分启动 owner 的逻辑抽成 `drain_shutdown_owners`，测试走这条生产路径。
  - 日语、俄语按钮文案修正。
  - 失败状态提示改用警示色。
  - 初始焦点改到"复制详情"。
  - 俄语三个按钮在 620px 下被截断，最小宽度提到 660px。
- **第二轮修复：**
  - 成功打开目录后清除失败提示。
  - 文档与迁移状态读取各补注入式重试测试。
- **第三轮：** codex 结论 PASS，无剩余 Critical/Major。
- **记为已知限制，未修：**
  - quarantine 恢复只重查父进程。父进程被杀后子进程无法确认退出时，`recover_quarantine` 会放行。根因是 pid 记录不持久化子进程身份，v2.0.2 下次启动重新清扫时同样放行。追踪：libnyanpasu/nyanpasu-runtime#465。
  - 文件名为 u64 最大值的日志分片或配置产物会导致计数溢出。属既有问题，需人为构造文件名。
  - shutdown hook 线程创建失败没有测试：需要为测试在生产代码中加注入点。

**验证**

- 本机默认 nightly 与 nightly-2026-10-08 在块克隆的 target 上编译 ICE，工具链文件经逐一哈希核对无损坏。app 侧验证与提交钩子改用 `RUSTUP_TOOLCHAIN=nightly-2026-09-23`；runtime 侧用新装的 nightly-2026-10-08 与独立 target。均未修改 `rust-toolchain.toml`。
- 最终结果：app 测试 1369 通过、报告器 12 通过、deep-link 3 通过；clippy 无新增警告；rustfmt、architecture-ledger、backend-boundaries 通过。
- Linux、macOS 本地未编译（缺目标标准库或 SDK），依赖 CI。
- runtime 的 `Cargo.lock` 中 `nyanpasu-log-preview` 记为 0.1.1，而清单是 0.1.0。这是 v2.0.2 发版时就有的不一致，构建会改写它，未纳入本次提交。

- 计划审计（codex gpt-6.1-sol xhigh，只读）：9 个阻断项全部采纳。主要修订：D5 的类型化错误移到 runtime（R2），app 侧 D5 推迟；setup 失败的收尾从"setup 内阻塞等待"改为交给正常退出流程，避免与需要主线程的清理死锁；不在 tracker 内的 actor 在构造后立即纳入；迁移子进程增加结构化结果；D3、D4 覆盖迁移路径；deep-link 与构建前的其余 panic 点纳入 A5。

**后续：D5 与共享 redb 分类**

R2 保留了类型化错误，但 app 判断 redb 错误仍是三处手写匹配（KV 存储、启动失败分类、流量存储），且互不一致：只有 KV 存储把截断文件当作损坏，流量存储只认魔数不符。为此不在 jobs 里单独加分类，而是把分类放进 utils，供所有 redb 使用方共用：

- **utils（libnyanpasu/nyanpasu-utils#212）：** `io::classify` 给出暂时性、权限、其他三类；`redb` feature 下的 `redb_ext` 把 redb 4.4 的全部错误类型分成损坏、格式不符、缺表、被占用、IO、内部六类，`find_failure` 沿使用方的错误链查找。只给事实，不定策略。`IoFailure` 在 `serde` feature 下可序列化，供迁移子进程报告使用。
- **runtime（libnyanpasu/nyanpasu-runtime#466）：** 日志落盘改用 `io::classify`；redb 锁定版本从 4.2.0 升到 4.4.0。
- **app：** submodule 暂钉 runtime#466 的提交，合入前须改到发布 tag。KV 存储、启动失败分类、流量存储改用共享分类，删掉 app 自带的 `IoFailure` 与 `classify_io`。流量统计文件被截断时也会丢弃重建。
- **D5：** `jobs.redb` 不是常规文件、redb 判为损坏或格式不符、恢复或修剪时记录解码失败，都改名留证后新建，任务历史丢失。被占用与暂时性 IO 按 50/100/200 ms 重试；权限与其余 IO 失败不改名，按权限或占用报告。启动失败分类不再对任务库特判，权限与占用按对应类型提示。
- **审查：** utils 的分类经 `/ccg:review` 三轮（codex 后端 + kimi，均为后端审查），终审 PASS。第一轮两方都指出 redb 4.4 的 `CursorError`、`BackendError` 未纳入，已补并把下限提到 4.4。未采纳：Unix 上 `EACCES` 锁冲突的说明（没有使用方自加 fcntl 锁）、自定义存储后端的说明（没有使用方使用）。
