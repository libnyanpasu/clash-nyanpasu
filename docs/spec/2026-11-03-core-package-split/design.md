# Core 包族拆分、Shell 与 RPC 目标 设计

**日期：** 2026-11-03
**基线：** `main @ 5ed3c019e`（含 #6116 路线图）
**来源：** [`application-platform-extraction.md`](../../design/application-platform-extraction.md) 的 Target ownership 与 Next stage；代码事实由 Claude 与 codex 联合探索、逐条附源码位置核对（§1），并经 codex 审计。
**范围：** Next stage 的 A–E 五个阶段。Phase A 写到可直接执行的 PR 粒度；B–E 写到里程碑粒度（前置条件、关键决策、风险、可机械验证的完成判据），开始各阶段前再细化成同等粒度的任务。
**不在范围：** 见 §9。
**权威顺序：** `AGENTS.md` > `docs/development/*` > `application-platform-extraction.md` > 本设计 > `tasks.md`。本设计与路线图的偏差集中列在 §8，已在引入本 spec 的同一提交中同步修订路线图、`backend-packages.md` 与 `architecture.md`。

---

## 0. 结论摘要

1. **先修门禁与测量，再动代码（T0）。** `lint:backend-boundaries` 现在把四个包名和禁止规则硬编码在一条 `if` 链里（`scripts/src/architecture-ledger/backend-boundaries.ts:4-43`），新 crate 出现时无法表达分层规则。T0 把它改成按包集合声明的规则表，审全部 feature 边，并记录构建基线。
2. **分三层：基础层、core 族、实现 crate。** 基础层（`nyanpasu-config`、`nyanpasu-contracts`、`nyanpasu-persistence`、`nyanpasu-profile-transform`）是纯值、端口与通用机制，谁都可以依赖；core 族是 facade、应用与 owner；实现 crate（`nyanpasu-scripts`、`nyanpasu-profile-io`、`nyanpasu-net`）只依赖基础层，由 shell 注入 core。
3. **A1 不新建描述符 crate。** `ScriptType` 与 config 已有的 `profile::ScriptRuntime` 完全重复，且 `ScriptRunner::run` 已经只收 `ScriptRuntime`。A1 删除 `ScriptType`，`ScriptWrapper` 变为脚本模块私有。`BundleMetadata` 是 `nyanpasu-contracts` 的第一个成员。`owner_shutdown` 先移到 core 根模块，等第一个 owner crate 离开 core（B3）时再按需建 crate。
4. **脚本 runner 的构造由应用边界拥有。** config 的 `ScriptRunner` 没有 `Send + Sync`，runner 内部是 `Rc<RefCell<_>>`，不能把 `Arc<dyn ScriptRunner>` 传过 `spawn_blocking`。消费者是运行时构建编排，所以由它定义一个线程安全的构造闭包类型；shell 传入调用 scripts 构造函数的闭包，runner 在阻塞闭包内创建、用完即销毁。config 的纯执行器 API 不变。
5. **运行时构建编排上移到 core，不下放到 profile-io。** 它构造应用层快照、依赖 core-manager 与 `ProfilesActor` 的闭包算法（`nyanpasu-platform/src/enhance/runtime_build.rs:7-26,44,65-107`）。编排（输入捕获、调用 builder、严格校验、产物转快照、发布）归 core 应用层，下层只提供内容读取。
6. **contracts 按"谁在 core 外实现"增长。** A4 只搬 Phase A 的实现 crate 需要的端口：元数据、profile 与 materialization、网络三组。能力端口随 C2/C3 的目标 crate 搬；运行时值（receipt、degradation、mutation outcome）连着 `RuntimeError`、`ProfilesError`、`CommitAborted`，随 D2 处理。
7. **契约错误用装箱的 source 替代库类型。** `SubscriptionFetchError`、`ProfileFileError` 的 `reqwest::Error`、`atomicwrites::Error`、YAML 错误、`corrupt::Error` 字段改为 `Box<dyn Error + Send + Sync>`，serde 照旧跳过。contracts 不依赖这些库，变体、结构字段、`Display` 与完整 source 链都不变。
8. **订阅 HTTP 进 net，profile-io 只管文件。** `ProfileFilePorts` 本来就把 `fs`、`fetcher`、`materialization` 分开（`C/client/host_adapters.rs:17-23`），shell 分别用 profile-io 与 net 的实现组装，实现 crate 之间没有依赖。
9. **测试归属。** 应用事务集成测试移入 core 的测试，dev-dependency profile-io；core 导出的 `test-support` 仍按 `ClientHostAdapters` 参数化，不链接实现 crate；golden 测试放在 transform，dev-dependency scripts。门禁只检查 normal/build 边。
10. **B–E 的依赖图见 §5 开头**；E 的完整协议 DTO 依赖 D2，CLI 因此也在 D2 之后。

---

## 1. 现状事实（已核对源码，`main @ 5ed3c019e`）

路径缩写：`C/` = `backend/nyanpasu-core/src/`，`P/` = `backend/nyanpasu-platform/src/`，`Q/` = `backend/nyanpasu-config/src/`，`T/` = `backend/tauri/src/`，`S/` = `scripts/src/architecture-ledger/`。表中位置是代表性锚点；完整移动清单在 `tasks.md` 各卡里。

### 1.1 规模与门禁

| 事实                                                                                                                                                           | 位置                                                                 |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| core 100,456 行（`src/**/*.rs`，含测试）；platform 9,046 行                                                                                                    | `wc -l`                                                              |
| 门禁只检查 core、platform、config、diagnose 四个包，规则是包名硬编码的 `if` 链；platform → core 由策略测试显式允许；解析 `cargo tree` 输出时根包自身也在名单内 | `S/backend-boundaries.ts:4-43`；`S/backend-boundaries_test.ts:34-39` |
| 门禁跑 `cargo tree --all-features --target all --edges normal,build`，检查传递依赖，不含 dev 边；只看包名，不看 feature 名                                     | `S/backend-boundaries.ts:46-76`                                      |
| 禁止 `nyanpasu-local-channel` 即可间接拦住 widget 协议的 `ipc` feature                                                                                         | `backend/nyanpasu-widget-protocol/Cargo.toml:12`                     |
| 静态单例门禁只覆盖 `backend/tauri/src/`、`backend/nyanpasu-core/src/`；允许名单按路径加名字，`utils/hwid.rs` 的 static 在名单里                                | `S/policy.ts:80-84,159-162`；`S/scan.ts:337-366`                     |
| 新 `backend/**` crate 已被 CI 的 backend 规则覆盖                                                                                                              | `scripts/src/ci/changed-parts.ts:76-80`                              |
| specta 导出写三个文件                                                                                                                                          | `T/specta_export.rs:443-455,484-500`                                 |

### 1.2 Feature 接线

| 事实                                                                                                                                                              | 位置                                                                                                                                                                                                   |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| tauri → core 未关默认 feature，`default-meta` 转发无法关闭                                                                                                        | `backend/tauri/Cargo.toml:39,285,289`                                                                                                                                                                  |
| platform → core 同样隐式开默认 feature                                                                                                                            | `backend/nyanpasu-platform/Cargo.toml:16`                                                                                                                                                              |
| 直连 config 并开默认 feature 的边：core、platform、tauri、diagnose（core 又依赖 diagnose）；config 默认含 `default-meta`、`nesting`                               | `backend/nyanpasu-core/Cargo.toml:33,77`；`backend/nyanpasu-platform/Cargo.toml:19`；`backend/tauri/Cargo.toml:41`；`backend/nyanpasu-diagnose/Cargo.toml:10`；`backend/nyanpasu-config/Cargo.toml:10` |
| dev-dependency 也开默认 feature，并开 `test-support`（路线图写的 `:266` 已过时）                                                                                  | `backend/tauri/Cargo.toml:268,271`；`backend/nyanpasu-platform/Cargo.toml:13,65`                                                                                                                       |
| core `test-support` 打开可选的 `mockall`、`axum`；core 的测试支撑门控为 `cfg(any(test, feature = "test-support"))`，下游 crate 只能经 `test-support` feature 拿到 | `backend/nyanpasu-core/Cargo.toml:13`；`C/client/mod.rs:45-46`                                                                                                                                         |

### 1.3 A1 涉及的三个项

| 事实                                                                                                                                                                                                                    | 位置                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `owner_shutdown` 39 行，闭包只有 `ractor::ActorCell`、`tokio_util` 的 token/tracker、`tokio::select!`                                                                                                                   | `C/client/owner_shutdown.rs:1-39`                                                  |
| 它的下层使用者除路线图列出的 logs、traffic、geo、clash、actor_v2 外，还有 `proxies`、`updater`；tauri 直接导入 `track_until_shutdown`                                                                                   | `C/proxies.rs:492`；`C/updater/mod.rs:445`；`T/setup.rs:4,306,467,704,721,735,742` |
| `BundleMetadata` 只依赖 config 的 `ReleaseChannel`，无 serde/specta；测试常量 `TEST_BUNDLE_METADATA` 在 `cfg(test)` 或 `test-support` 下可用，core 单测直接用它                                                         | `C/client/bundle_metadata.rs:3-35`；`C/backup.rs:383`                              |
| 元数据的下层使用者除 migration、backup、utils、service 外还有 `updater`                                                                                                                                                 | `C/updater/instance.rs:26,60,68`                                                   |
| `ScriptType` 与 `Q/profile/definition.rs:89-96` 的 `ScriptRuntime` 变体、默认值、线上拼写相同，只多一个 `strum::EnumString`（唯一使用者是它自己的测试 `C/enhance/mod.rs:49`）；生成的 bindings 导出的是 `ScriptRuntime` | `C/enhance/mod.rs:8-53`；`T/specta_export.rs:216-224`                              |
| `ScriptType`/`ScriptWrapper` 在 core 内除定义与测试外无生产使用者；platform 有三处                                                                                                                                      | `P/enhance/script/{adapter.rs:16,runner.rs:7,mod.rs:8}`                            |

### 1.4 脚本与运行时构建（A2、A3、A5a）

| 事实                                                                                                                                                                                                                                                                      | 位置                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `P/enhance/script/` 共 7 个文件 2,626 行；对 core 的导入只有描述符三处                                                                                                                                                                                                    | `P/enhance/script/*`                                                                                                      |
| 脚本相关依赖：`boa_utils`、`nyanpasu-log-preview`、引擎与 oxc，引擎无选择 feature                                                                                                                                                                                         | `backend/nyanpasu-platform/Cargo.toml:20-21,39-55`                                                                        |
| JS 模块加载器写死为 `CombineModuleLoader(Simple, Http)`，HTTP 加载器在 `boa_utils` 内直接构造 reqwest 客户端                                                                                                                                                              | `P/enhance/script/js.rs:134-145`；`backend/boa_utils/src/module/http.rs:107-158`                                          |
| `ScriptRunner`、`ProfileContentSource` 都无 `Send + Sync`；runner 内部为 `Rc<RefCell<_>>`；`EnhanceScriptRunner` 自带 current-thread runtime，只能在阻塞上下文执行                                                                                                        | `Q/runtime/executor/ports.rs:16,28-43`；`P/enhance/script/runner.rs:12-22`；`P/enhance/script/adapter.rs:1-31`            |
| config 执行器消费已创建的 `ScriptRunner`，不负责构造                                                                                                                                                                                                                      | `Q/runtime/executor/mod.rs:46-59`                                                                                         |
| `FsRuntimeBuildAdapter` 存 `ScriptDirs`；`capture_content` 在 async 方法里直接读内容；阻塞闭包内自建 runner、调用 builder、严格校验、转快照、构造 `RuntimeSnapshot`；`RuntimeBuildPort` 要求 `Send + Sync`                                                                | `P/enhance/runtime_build.rs:28-33,43-51,65-100`；`C/client/application_workflow/ports.rs:10`                              |
| `P/enhance/runtime_build/tests.rs` 也直接构造该适配器与平台的内容源、runner                                                                                                                                                                                               | `P/enhance/runtime_build/tests.rs:60-65,89-93`                                                                            |
| `C/enhance/` 共 970 行；`runtime_builder.rs`（576 行）、`chain.rs`、`utils.rs`、`test_support.rs` 不引用 `crate::`；`artifact_snapshot.rs` 构造 `client::runtime::RuntimeSnapshotData` 与 `client::runtime_inspection::RuntimeInspectionData`，并用私有模块里的 `LogSpan` | `C/enhance/artifact_snapshot.rs:23-24,95-126`；`C/enhance/mod.rs:57`                                                      |
| transform 测试夹具 `metadata` 是 `pub(crate)`，但被 `state/profiles/actor.rs` 的测试调用                                                                                                                                                                                  | `C/enhance/test_support.rs:16`；`C/state/profiles/actor.rs:2616,2633`                                                     |
| `RuntimeBuildError` 定义在 builder 旁，被 core 的 `RuntimeError` 与 `RuntimeBuildPort` 引用                                                                                                                                                                               | `C/enhance/runtime_builder.rs:25-60`；`C/client/runtime_error.rs:156-159`；`C/client/application_workflow/ports.rs:22-30` |

### 1.5 core 外实现或实现在 core、被 core 外消费的端口（A4、C）

| 端口                                                                          | 定义                                                                                                                                                                                  | 生产实现                                                                                                                   |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `ProfileFsPort`、`SubscriptionFetcher`、`ProfileMaterializationPort`          | `C/state/profiles/ports.rs:14,57,167`                                                                                                                                                 | 同一个 `ProfileFileService`（platform，`P/profiles/fs_service.rs:1527,2029,1590`），经 `ProfileFilePorts` 三个字段分别注入 |
| `RuntimeBuildPort`                                                            | `C/client/application_workflow/ports.rs:10`                                                                                                                                           | `FsRuntimeBuildAdapter`（platform）                                                                                        |
| `ChangelogHttp`                                                               | `C/client/app_update/changelog.rs:48`                                                                                                                                                 | `ReqwestChangelogHttp`（platform）；解析测试用 core 内 `FakeHttp`（`C/client/app_update/changelog.rs:318-335`）            |
| `SelfProxyPortSource`                                                         | `C/service/profile_file/ports.rs:6`                                                                                                                                                   | `SessionPortResolver`（core，`C/client/ports.rs:232`），被 platform 消费                                                   |
| `DirectEgressProbe`                                                           | `C/client/direct_egress.rs:28`                                                                                                                                                        | core 内 HTTP 实现（常量 `:35-39`，实现 `:90-105`）                                                                         |
| `AppUpdateBackend`、`AppUpdateEventSink`、`UiEventSink`、`MainThreadExecutor` | `C/client/app_update/mod.rs:99,120`；`C/client/event_sink.rs:28`；`C/client/main_thread.rs:82`                                                                                        | tauri                                                                                                                      |
| 热键 4 个、系统代理 3 个、效果 4 个、`DebugHttpControl`、`CoreVersionReader`  | `C/client/hotkey/ports.rs:315-351`；`C/client/system_proxy/ports.rs:66,115,186`；`C/client/effects/ports.rs:26,67,87,149`；`C/client/debug_http.rs:16`；`C/client/core_version.rs:31` | tauri                                                                                                                      |

| 错误                                | 事实                                                                                                                                                                                                                            | 位置                                                                                                                                                                                       |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `SubscriptionFetchError`            | `Debug, Snafu, Serialize, specta::Type`，内部标签 snake_case；三个变体带 serde 跳过的 `reqwest::Error`，一个带 `status: u16`；重试、jobs、前端本地化按变体区分；测试构造器 `mock` 是 `cfg(test)` 的 `pub(crate)`，core 测试在用 | `C/state/profiles/error.rs:270-306`；`P/profiles/fs_service.rs:2098-2107`；`C/state/profiles/jobs.rs:214-226`；`frontend/nyanpasu/src/utils/ipc-error.ts:155`；`C/client/mod.rs:2974,3134` |
| `ProfileFileError`                  | 带 serde 跳过的 `corrupt::Error`、`atomicwrites::Error`、YAML 错误与 IO 错误                                                                                                                                                    | `C/state/profiles/error.rs:84-192`                                                                                                                                                         |
| `ProfileDegradationCode::retryable` | `pub(crate)`，core 生产代码调用                                                                                                                                                                                                 | `C/state/profiles/ports.rs:134`；`C/state/profiles/actor.rs:1445`；`C/client/application_workflow/profiles.rs:22`                                                                          |
| `WidgetError`                       | 只有 `Debug, Snafu`；`CreateIpcServer`、`ConnectWidget` 带 `WidgetIpcError`；对外只暴露 `EffectFailureCode` 与 source 链文本                                                                                                    | `C/client/effects/ports.rs:102-143`；`C/client/effects/executor.rs:127-162`                                                                                                                |
| `DegradationReason`                 | 直接内嵌 `Arc<RuntimeError>`、`Arc<ProfilesError>`、`InterruptFailure`；`RuntimeError` 再连到 core-manager、service、build、state 的错误；RPC 响应直接返回 `MutationOutcome`                                                    | `C/client/runtime.rs:770-807`；`C/client/runtime_error.rs:9-252`；`C/state/mutation.rs:232-299`；`T/ipc.rs:433,491,503`                                                                    |

### 1.6 platform 与 core 的 HTTP、文件辅助（A5）

| 组                                                                                                                                                                                                                                                                                                                                                  | 生产位置                                                                                                                                      | 去向                          |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| 元数据                                                                                                                                                                                                                                                                                                                                              | `P/profiles/fs_service.rs:26`；`P/changelog_http.rs:7`                                                                                        | contracts（A1c）              |
| profile 端口、值、错误                                                                                                                                                                                                                                                                                                                              | `P/profiles/fs_service.rs:23,28-35`                                                                                                           | contracts（A4a）              |
| 损坏文件恢复：`retry_io`、`retry`、`retry_with_delay`、`retry_async`、`is_link`、`preserve` 与 `Error`，依赖 `backon`、`nyanpasu_utils::io`、snafu、`time`、tracing、`atomicwrites::move_atomic`、macOS `renamex_np`；`error_io` 另外向下转型 redb 与 core-manager 错误                                                                             | `C/corrupt.rs:11-17,20-44,60-152`；platform 调用 `P/profiles/fs_service.rs:27,182-484`；core 读点 `C/session_state.rs:48`、`C/storage.rs:277` | 通用部分进 persistence（A5b） |
| 订阅抓取：字段 `self_proxy_port`、`http_timeout`、`bundle_metadata`；代理优先级 self proxy → 实时读 `sysproxy` → 直连；默认 UA `clash-nyanpasu/v{app_version}`；四个设备头 `x-hwid`、`x-device-os`、`x-ver-os`、`x-device-model`（后三个经 `sanitize_for_header`）；响应头解析在 `subscription.rs`，用 core 的 13 行 key/value 解析器（唯一使用者） | `P/profiles/fs_service.rs:43-48,2029-2120`；`P/profiles/subscription.rs:34`；`C/utils/key_value.rs:3-15`                                      | net（A5c）                    |
| 客户端策略 `get_reqwest_client`（self proxy + 实时系统代理 + UA）与 `NyanpasuReqwestProxyExt`；使用者：图标、网络诊断（`url_delay_test`、ip.sb），以及订阅抓取的 `swift_set_proxy`；tauri 的更新适配器直接调 `get_self_proxy`/`get_system_proxy`                                                                                                    | `C/utils/candy.rs:8-19`；`C/utils/config.rs:7-50`；`C/service/icon.rs:67`；`C/utils/net.rs:14,35`；`T/adapters/app_update.rs:88-90`           | net（A5c）                    |
| updater 自建客户端（120 秒超时、`package_version` UA；有 mixed 端口时显式走 self proxy，否则保留 reqwest 默认的系统代理自动检测，工作区开了 reqwest `system-proxy`，`backend/Cargo.toml:82`），不用上面的策略；download 接收该客户端；B4 须保持两个分支                                                                                             | `C/updater/instance.rs:78-89`；`C/download/mod.rs:97-98`                                                                                      | 留在 core，B4 处理            |
| 描述符、builder、输入、快照、`RuntimePaths`、core-spec 解析、`ProfilesActor::current_closure`、`blocking::join`                                                                                                                                                                                                                                     | `P/enhance/runtime_build.rs:7-26,44,58-107`                                                                                                   | 编排上移 core（A5a）          |

路线图"Known gaps"里的"YAML parsing"已过时：platform 生产代码直接调用 serde YAML（`P/profiles/fs_service.rs:559,578`），剩下的 core 解析器只有 key/value。

platform 的应用集成测试：`P/profiles/fs_service/integration_tests/{profiles,facade,workflow}.rs` 构造 `ProfilesClient`、facade、workflow 图；`P/enhance/runtime_build/tests.rs` 测应用的严格候选策略；`P/profiles/test_support.rs` 用真实 `ProfileFileService` 构造 `NyanpasuClient`（`:2,22`），被 `T/unified_rpc.rs:486,875,964,1087,1151` 与 `T/adapters/hotkey/tests.rs:27` 使用。core 已有按 `ClientHostAdapters` 参数化的测试装配（`C/client/test_support.rs:751-755`）。

### 1.7 core 顶层模块图

按全部 `.rs`（含测试与所有 `cfg` 分支）解析 `crate::`/`super::` 引用：21 个顶层模块、65 条有向边，最大强连通分量是 **16 个模块**（路线图写 14）：`actor_v2, backup, clash, client, connections, enhance, geo, logs, migration, proxies, service, session_state, state, traffic, updater, utils`。`corrupt`、`download`、`format`、`profile_import`、`storage` 不在环内。

即使把路线图环表里的边整条删掉，仍剩三个环：

| 剩余环                                  | 回边                                                                                                                  | 归属阶段 |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | -------- |
| `client → proxies → client`             | `C/proxies.rs:11` 用运行时 outcome/degradation 值                                                                     | D2       |
| `client → updater → client`             | `C/updater/instance.rs:26,672` 元数据与版本解析；`C/updater/ports.rs:8` 引用 `client::core_lifecycle` 的制品/进度类型 | A1c + B4 |
| `migration → session_state → migration` | `C/session_state.rs:27,72` 用 `migration::fs` 的错误与原子写                                                          | B1a      |

`state → client` 不只是值：字面 `crate::client` 有 24 行，展开分组导入后是 7 个文件 45 行 62 个路径，含操作性依赖——效果计划（`C/state/application.rs:93`）、热键校验错误（`C/state/config_error.rs:16`）、提交通知（`C/state/mutation.rs:23`）、jobs 等待与 facade 错误（`C/state/profiles/jobs.rs:71,93,112`）、收敛健康（`C/state/profiles/sources.rs:10`）。`effects ↔ hotkey/system_proxy` 与 `state ↔ application_workflow` 在 `client` 内部，顶层图看不到。

因此"断环"按符号与目标归属写判据，不按箭头写；模块级环在 crate 拆出时由 Cargo 本身禁止，不另写模块图检查脚本。

### 1.8 B–E 的阻断点

| 阶段 | 事实                                                                                                                                                                                                                                                                                                          | 位置                                                                                                                                    |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| B1   | backup 导入具体的 `Storage`、`StorageOperationError`，暴露 `StorageSource::Live(&Storage)`；migration 直接导入 `Storage`/`WebStorage`                                                                                                                                                                         | `C/backup.rs:11,43,62`；`C/migration/modules/storage.rs:4,29-37`                                                                        |
| B1   | migration 还需要 `PathResolver`、`BundleMetadata`、备份、会话状态恢复、YAML merge 展开                                                                                                                                                                                                                        | `C/migration/mod.rs:83`；`C/migration/runner.rs:6`；`C/utils/yaml.rs:44`                                                                |
| B2   | `ClientSetupArgs` 两处 redb：`traffic_store` 显式写 `RedbStorage`；`storage: Storage` 内部是 `redb::Database` 且 `get_instance()` 公开                                                                                                                                                                        | `C/client/mod.rs:109,127-128`；`C/storage.rs:96,150,317`                                                                                |
| B2   | `nyanpasu-traffic` 无条件依赖 redb，legacy 导入器无 feature 门控；导入的持久进度与放弃语义有专门逻辑                                                                                                                                                                                                          | `backend/nyanpasu-traffic/Cargo.toml:18`；`C/traffic/storage.rs:49-75`                                                                  |
| B3   | traffic 吃的是 `clash_api::ConnectionsSnapshot`，归一化在 `traffic/source.rs`；geo 直接用 `CoreClient` 拿 API 租约读 `/configs`；`CountryIndexSource::watch` 的错误泄漏 `notify`                                                                                                                              | `C/traffic/actor.rs:66`；`C/traffic/source.rs:15-104`；`C/geo/actor.rs:135-160`；`C/geo/ports.rs:48,72`                                 |
| B4   | facade 在 `C/client/mod.rs:502-515` 构造 `HttpUpdaterBackend`；updater 依赖 `client::core_lifecycle` 的制品类型                                                                                                                                                                                               | `C/updater/ports.rs:8`                                                                                                                  |
| C1   | 无头装配除 `WindowControl`、`AcceleratorValidator`、`DebugHttpFactory` 外，还必须给 `ApplicationEffectsPort`（其执行器要热键、locale、widget、托盘）、`UiEventSink`、日志的前端 sink                                                                                                                          | `C/client/mod.rs:111,123-125`；`C/client/host_adapters.rs:35-48`；`C/client/effects/executor.rs:57`；`C/client/logs.rs:19`              |
| C2   | core 里直接弹 OS 授权的有三处：服务控制、DNS 刷新、**core 二进制安装失败后的 `runas` 复制回退**（路线图漏了第三处）                                                                                                                                                                                           | `C/service/control.rs:176-190`；`C/client/system_dns.rs:50-77`；`C/client/core_lifecycle/adapters.rs:60-80`                             |
| C2   | `update_core` 开了 `rpc(http)`，可经下载安装走到上一行的提权回退，与"WebUI 不含提权"的目标不符（[#6157](https://github.com/libnyanpasu/clash-nyanpasu/issues/6157)）                                                                                                                                          | `T/ipc.rs:1110-1114`；`C/updater/mod.rs:316-318`                                                                                        |
| C3   | 一个效果计划同时含 locale、logger、core 日志、自启动、系统代理、代理守卫、热键、widget、托盘；执行器要求 locale 先于托盘                                                                                                                                                                                      | `C/client/effects/plan.rs:339-350`；`C/client/effects/executor.rs:173,209-258`                                                          |
| D2   | `MutationCoordinator` 存具体的 `ApplicationWorkflowClient`；四个源 owner 调用点                                                                                                                                                                                                                               | `C/state/mutation.rs:31-36`；`C/state/application.rs:159`；`C/state/clash_config.rs:123`；`C/state/profiles/actor.rs:480,1286`          |
| E    | `TauriRpcHandler` 是 `fn(AppHandle, Window, Webview, Value)`；宏两个分支都生成 Tauri 处理器，选项只有 `http`、`result`、`owner`；161 个声明、104 个 `http`、16 个注入 `AppHandle`；HTTP 无前端资源拒绝启动；事件从 Tauri 事件桥接；`RpcError::application_ref` 向下转型 logging、core-manager、Tauri IPC 错误 | `T/unified_rpc.rs:28-69,99-105,144-168`；`backend/nyanpasu-macro/src/unified_command.rs:5-21,176-279`；`T/server/debug_http.rs:138-151` |
| E    | `nyanpasu-local-channel` 是父子进程一次性通道（u32 长度 + postcard、令牌握手、只接受一个子进程、同构建类型指纹），可复用分帧与 OS 权限，不能直接当长期 RPC 监听                                                                                                                                               | `backend/nyanpasu-local-channel/src/{lib.rs:37-50,78,wire.rs:4-50,tokio.rs:24-35}`                                                      |

---

## 2. 目标与成功判据

| #   | 判据                                                                                                                                                                 | 验证方式                                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| G1  | 实现 crate（scripts、profile-io、net）只依赖基础层，不依赖 core 族与其他实现 crate                                                                                   | `pnpm run lint:backend-boundaries`，规则表含该禁止边与正反策略测试                                                                            |
| G2  | contracts 的直接依赖不含 `ractor`、`reqwest`、`redb`、`atomicwrites`、`nyanpasu-core-manager`、`nyanpasu-local-channel`、Tauri；传递依赖经 config 到达的 YAML 等允许 | 门禁（传递禁止 `ractor`、`reqwest`、`redb`、`nyanpasu-core-manager`、`nyanpasu-local-channel`）+ 检查 `backend/nyanpasu-contracts/Cargo.toml` |
| G3  | `nyanpasu-platform` 从 workspace 删除，无旧路径 re-export                                                                                                            | `cargo metadata --no-deps` 不含该包；`rg nyanpasu_platform backend` 退出码为 1（无匹配；退出码 2 视为检查失败）                               |
| G4  | 行为不变：JS/Lua 单测、7 个 golden、严格/宽松转换测试、profile 事务集成测试照旧通过，且测试名清单移动前后一致                                                        | 移动前后各导出 `cargo test -p <包> -- --list --format terse` 的排序清单并比较                                                                 |
| G5  | 线上形状不变                                                                                                                                                         | specta 导出后三个生成文件无 diff；每个改动的错误变体有线上形状测试                                                                            |
| G6  | 编译时间按测量判断边界值不值得保留                                                                                                                                   | T0 基线与 A、B 结束时的复测（§6）                                                                                                             |
| G7  | B–E 每一步的完成判据可机械检查                                                                                                                                       | §5 每个里程碑的"完成判据"列                                                                                                                   |

---

## 3. 目标包与依赖规则

### 3.1 Phase A 结束时的包

```text
backend/tauri（shell，构造并注入一切）
  ├─ nyanpasu-core（facade + 应用 + 其余 owner，仍是一个 crate）
  ├─ nyanpasu-scripts      ┐
  ├─ nyanpasu-profile-io   ├─ 实现 crate：只依赖基础层
  └─ nyanpasu-net          ┘
基础层（谁都可依赖，自身只依赖更低的基础层）：
  nyanpasu-profile-transform → nyanpasu-config
  nyanpasu-contracts         → nyanpasu-config
  nyanpasu-persistence       → nyanpasu-config（通用文件恢复，不含 redb、core-manager）

dev-only：core → profile-io（应用集成测试）；transform → scripts（golden）
```

core → 基础层是下行依赖。persistence 在 A5b 只装通用文件恢复，B1a 再收格式与通用原子写；依赖 redb 的存储、备份、migration 不进 persistence（§5.1）。

### 3.2 门禁规则表（T0a 引入，各卡追加）

规则按包集合写；检查某个包时，跳过 `cargo tree` 输出中的根包自身。

| 集合             | 成员（Phase A 结束时）                                                                                                              |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `GUI`            | Tauri 族、Slint 族、winit、egui/eframe、gtk、webkit2gtk、`clash-nyanpasu`、`nyanpasu-widget`、`nyanpasu-crash-reporter`（现有规则） |
| `FOUNDATION`     | `nyanpasu-config`、`nyanpasu-contracts`、`nyanpasu-persistence`、`nyanpasu-profile-transform`                                       |
| `CORE_FAMILY`    | `nyanpasu-core`（B/D 拆出的 core 包加入此集合）                                                                                     |
| `IMPLEMENTATION` | `nyanpasu-scripts`、`nyanpasu-profile-io`、`nyanpasu-net`                                                                           |

| 被检查的包                                                                       | 禁止（normal/build，传递）                                                          |
| -------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| 所有中立包（`FOUNDATION`、`CORE_FAMILY`、`IMPLEMENTATION`、`nyanpasu-diagnose`） | `GUI`                                                                               |
| `FOUNDATION` 每个成员                                                            | `CORE_FAMILY`、`IMPLEMENTATION`                                                     |
| `nyanpasu-contracts`                                                             | 另加 `ractor`、`reqwest`、`redb`、`nyanpasu-core-manager`、`nyanpasu-local-channel` |
| `nyanpasu-persistence`                                                           | 另加 `redb`、`nyanpasu-core-manager`                                                |
| `nyanpasu-profile-transform`                                                     | 另加 `nyanpasu-contracts`、`nyanpasu-persistence`、`nyanpasu-scripts`               |
| `nyanpasu-config`                                                                | 另加 `FOUNDATION` 的其他成员                                                        |
| `nyanpasu-contracts`、`nyanpasu-persistence`（互相）                             | 对方与 `nyanpasu-profile-transform`                                                 |
| `IMPLEMENTATION` 每个成员                                                        | `CORE_FAMILY`、`IMPLEMENTATION` 的其他成员                                          |
| `CORE_FAMILY` 每个成员                                                           | `IMPLEMENTATION`、`nyanpasu-platform`（存在期间）                                   |
| `nyanpasu-diagnose`                                                              | `CORE_FAMILY`、`IMPLEMENTATION`、`nyanpasu-platform`                                |

基础层内部方向因此也被强制：config 在最底；contracts、persistence、transform 都只依赖 config。每个集合成员在它所在的提交里加入，新成员自动继承集合规则；每条规则有一正一反的策略测试（含 transform → contracts、transform → persistence 的反例），并保留"禁止 local-channel 能拦住 widget `ipc` feature"的回归用例。

---

## 4. Phase A 设计

### 4.0 T0：门禁、feature 接线、基线

- **规则表。** 见 §3.2；T0a 只表达现有规则（GUI 族、core → platform、diagnose → core/platform），集合中尚不存在的包不出现。表化后在 `main` 上跑仍通过。
- **feature 接线。** 审全部到 core 与 config 的边：tauri、platform 的 core 依赖（normal 与 dev），core、platform、tauri、diagnose 的 config 依赖。每条边加 `default-features = false` 并显式列出所需 feature（dev 边保留 `test-support`），使 tauri 的 `default-meta`、`nesting` 转发真正可控。`test-support` 经 dev-dependency 打开导致 `cargo build` 与 `cargo test` 编出两份 core，这一点 T0 只测量，修法按数据定（§8 O-3）。之后新增的 crate 依赖 core 或 config 时同样关默认 feature。
- **基线。** 见 §6，结果写进 T0 的 PR 描述。

### 4.1 A1：拆环的三个小项

1. **`owner_shutdown` → `crate::owner_shutdown`。** 从 `client/` 移到 core 根模块，更新 core 内 18 个文件与 `T/setup.rs:4`。只移动不改实现。
2. **删除 `ScriptType`，统一用 `ScriptRuntime`。** platform 的 runner 注册表与分派改用 `nyanpasu_config::profile::ScriptRuntime`；`ScriptWrapper` 改为 `P/enhance/script/` 内私有类型，`RunnerManager::process_script` 随之收窄可见性。`strum::EnumString` 只被被删的测试使用，不给 config 加 strum。
3. **新建 `nyanpasu-contracts`，移入 `BundleMetadata`。** 依赖只有 `nyanpasu-config`（关默认 feature）。`TEST_BUNDLE_METADATA` 在 contracts 的 `test-support` feature 后；core 的 `test-support` 转发 `nyanpasu-contracts/test-support`，core 另加 `[dev-dependencies] nyanpasu-contracts = { features = ["test-support"] }`，使单独 `cargo test -p nyanpasu-core` 的单测仍能用它。

### 4.2 A2：`nyanpasu-scripts`

- **移动。** `P/enhance/script/` 七个文件整体移入新 crate，带上它们实际使用的依赖（`boa_utils`、`nyanpasu-log-preview`、引擎、oxc，以及 serde、tokio、tracing 等共享依赖）。依赖只有 config 与这些库，不依赖 contracts 与 core。
- **构造闭包。** 消费者是运行时构建编排（A2 时在 platform，A5a 后在 core），所以构造类型由 core 在 `client/application_workflow/ports.rs`（`RuntimeBuildPort` 旁）定义：

  ```rust
  /// Builds a script runner on the blocking thread that will use and drop it.
  pub type ScriptRunnerFactory =
      dyn Fn() -> Result<Box<dyn ScriptRunner>, PortError> + Send + Sync;
  ```

  scripts 只暴露现有构造函数 `EnhanceScriptRunner::new(dirs)`；tauri 传入 `Arc::new(move || Ok(Box::new(EnhanceScriptRunner::new(dirs.clone())?)))`。scripts 不知道这个类型，也就不依赖 core。

- **注入。** `FsRuntimeBuildAdapter` 不再存 `ScriptDirs`，改存 `Arc<ScriptRunnerFactory>`，在原阻塞闭包里调用；`StartScriptRunner` 错误的 source 从 `io::Error` 改为 `PortError`。`P/enhance/runtime_build/tests.rs` 的构造同提交更新。
- **语义保持。** 阻塞上下文执行、失败前日志保留、严格/宽松转换、有序映射、panic 经 `blocking::join` 传播，全部不变。
- **不在 A2 内：** 扩展定义的注入与引擎 feature 开关（A2b，§5.0）。

### 4.3 A3：`nyanpasu-profile-transform`

- **移入：** `runtime_builder.rs`、`chain.rs`、`utils.rs`（`PostProcessingOutput`、`Logs`、`LogSpan`，后者改为公开导出）、三个 builtin 脚本、`test_support.rs`（模块门控为 `#[cfg(any(test, feature = "test-support"))]`，使 transform 自己的单测不依赖别的包请求 feature；`metadata` 等被 core 测试使用的夹具改为 `pub`）。`RuntimeBuildError` 与 builder 同行移入。
- **留在 core：** `artifact_snapshot.rs` 的产物转快照，移到 `client/` 下 `runtime.rs` 旁。
- **依赖方向：** core → transform（下行）；transform 只依赖 config。`PostProcessingOutput` 的类型名与 specta 名不变。core 的 `test-support` 转发 transform 的 `test-support`，并加 dev-dependency 开启它。
- **golden：** A3 时仍留在 platform（它同时需要 scripts 与内容源），A5a 时移到 transform 的测试。

### 4.4 A4：`nyanpasu-contracts` 的两组端口

contracts 只收"core 外有生产实现或生产消费者、且该实现要在 Phase A 离开 core 依赖"的端口及其签名闭包。

**A4a：profile 与 materialization。**

- 移入 `C/state/profiles/ports.rs` 全部端口与值类型（路径、`FetchedSubscription`、`SubscriptionInfo`、materialization 资源/报告/清理结果、`ProfileDegradation*`），以及 `error.rs` 里只属于这些端口的错误（`ProfileFileError`、`SubscriptionFetchError`、`ErrorPath`、`ExpectedNode`）。`ProfilePathError` 已在 config，不动。`ProfilesError` 与事务错误留在 core。
- 跨 crate 后仍被 core 调用的 `pub(crate)` 项改为 `pub`：`ProfileDegradationCode::retryable`；`SubscriptionFetchError::mock` 改为 contracts `test-support` 下的公开构造器。
- **错误去库类型。** contracts 定义 `pub type BoxedCause = Box<dyn std::error::Error + Send + Sync + 'static>;`。`ProfileFileError` 的 `corrupt::Error`、`atomicwrites::Error`、YAML 错误字段与 `SubscriptionFetchError` 的三个 `reqwest::Error` 字段改为 `BoxedCause`；`std::io::Error` 字段不变；`#[serde(skip)]` 不变。实现方在产生库错误的调用处显式装箱（`.map_err(|e| Box::new(e) as BoxedCause)` 再接 snafu context；contracts 的错误定义里不出现库类型，因此不能在定义上写 `source(from(库类型, …))`），原错误对象整体保留，`Display`、`Debug` 与多级 source 链与改动前一致。
- **回归测试：** 对每个被替换的变体，用真实库错误构造，断言序列化 JSON、`Display`、逐级 `source()` 文本与改动前相同（先在改动前的提交上生成期望值写入测试）。

**A4b：网络端口。** `ChangelogHttp`、`SelfProxyPortSource`、`DirectEgressProbe` 移入 contracts；另新增三个由 core 消费的端口：图标抓取（`C/service/icon.rs:35-83`）、网络诊断（`C/utils/net.rs` 的 `url_delay_test` 与 `get_ipsb_asn`，调用方 `C/client/mod.rs:793,803`）、系统代理读取 `SystemProxySource`（每次调用实时读取）。闭包只有 URL、`String`、IP 地址、`bytes::Bytes`（图标返回值）、`serde_json::Value` 与 `anyhow`；现有 `anyhow::Result` 不改。现有函数的 `PathResolver` 与元数据改为实现的构造依赖；self proxy 端口仍是每次调用的参数，由 facade 照旧以 `self.clash_info().port` 实时取得（已确认绑定优先、否则配置的起始端口，`C/client/clash_info.rs:37`），不能在构造时固定，也不能换成只返回已确认绑定的 `SessionPortResolver`。端口方法保留端口、URL、期望状态码等每次调用变化的参数。A4b 时这些实现先在 core 内集中到一个适配器模块（工作名 `C/net_adapters.rs`），由 tauri 注入；A5c 整体移入 net。`SystemProxySource` 的实现在 C2 前留在 core，C2 移入 `nyanpasu-os`。

**不在 A4：** 能力端口随 C2/C3 搬；运行时值随 D2 搬（§8 V-2、V-3）。

### 4.5 A5：拆掉 platform

**A5a：运行时构建编排上移。**

- core 新增应用层服务（工作名 `RuntimeBuildService`，在 `client/` 下），持有 `Arc<ScriptRunnerFactory>`、`Arc<dyn ProfileContentSource + Send + Sync>`、`RuntimePaths` 与 core-spec 解析器，实现现有 `RuntimeBuildPort`。逻辑从 `P/enhance/runtime_build.rs:37-107` 原样迁入，包括 `capture_content` 在 async 方法内直接读内容的现状（改到阻塞线程是行为改动，不在本卡）。
- `RuntimeBuildPort` 留作 core 内的测试接缝；生产实现变为 core 自有。
- 内容源 `P/enhance/content_source.rs` 进入 profile-io（A5b），A5a 时仍由 platform 提供。
- 测试：`P/enhance/runtime_build/tests.rs` 随服务进入 core，其内容源换成测试内的内存实现，runner 换成测试替身（严格候选策略不依赖真实脚本引擎；若某用例必须真实引擎，则 core 加 scripts 的 dev-dependency 并在 PR 说明）；golden 与 `backend/nyanpasu-platform/tests/runtime_builder.rs` 进入 transform 的测试，transform dev-dependency scripts，内容源用测试内实现读 fixture 目录。

**A5b：`nyanpasu-persistence` 与 `nyanpasu-profile-io`（文件部分）。**

- `C/corrupt.rs` 的通用恢复整体进 persistence：`retry_io`、`retry`、`retry_with_delay`（由 `pub(crate)` 改为 `pub`，core 的生产调用点 `C/session_state.rs:48`、`C/storage.rs:277` 直接导入，不留转发函数）、`retry_async`、`is_link`、`preserve`、`Error`、时间戳格式化与独占重命名辅助，连同依赖（`backon`、`nyanpasu-utils` 的 `io` 部分、snafu、`time`、tracing、`atomicwrites`、macOS `nix`）与测试。`nyanpasu-utils` 不得开 `redb` feature（门禁禁止 redb 即可检查）。`error_io` 与 redb、core-manager 分类留在 core。
- profile-io 收 `P/profiles/{fs_service,materialization}.rs` 中 `ProfileFsPort` 与 `ProfileMaterializationPort` 的实现及内容源；依赖 contracts、config、persistence。订阅抓取在本步仍留在 platform（它依赖 core 的客户端策略，见 A5c）。
- tauri 组装 `ProfileFilePorts`：`fs`、`materialization` 来自 profile-io，`fetcher` 暂来自 platform。
- 同一提交迁完仍在 platform 的测试装配：`P/profiles/test_support.rs:22-30` 现在用一个 `ProfileFileService` 填满三个字段，改为 profile-io 的实现填 `fs`、`materialization`，platform 剩下的抓取类型填 `fetcher`；`P/profiles/fs_service/integration_tests/profiles.rs:143` 等把该类型当作文件/materialization 端口的地方改用 profile-io 的类型（platform dev-dependency profile-io）。
- adapter 本地测试随文件进入 profile-io，夹具改用 contracts 的 `test-support`。

**A5c：`nyanpasu-net`（一个提交完成整条 HTTP 策略路径）。** 订阅抓取与共享客户端策略必须一起走，否则要么 net 依赖 core，要么复制策略。本步一次移入：

- 订阅抓取拆成独立类型（工作名 `SubscriptionHttpFetcher`），实现 `SubscriptionFetcher`；连同 `P/profiles/subscription.rs` 与 `C/utils/key_value.rs` 的解析器。代理优先级、默认 UA、超时、重试保持原样；系统代理改经 `SystemProxySource`；四个设备头由 shell 准备好（含 `sanitize_for_header`）以值传入构造函数，测试用确定的夹具断言四个头的精确值。
- `P/changelog_http.rs`、`C/client/direct_egress.rs:35-105` 的探测实现、A4b 集中在 `C/net_adapters.rs` 的图标与网络诊断实现、客户端策略 `get_reqwest_client` 与 `NyanpasuReqwestProxyExt`（系统代理经 `SystemProxySource`）。
- tauri 的更新适配器（`T/adapters/app_update.rs:88-90`）改用 net 的策略；tauri 组装 `ProfileFilePorts` 的 `fetcher` 改为 net；platform 的测试装配与集成测试同一提交改用 net 的抓取类型（platform dev-dependency net）。
- core 删除这些实现与 `utils/candy.rs`、`utils/config.rs`、`utils/net.rs` 中已无使用者的部分；`SystemProxySource` 的 core 实现保留到 C2。updater 的独立客户端（`C/updater/instance.rs:78-89`）不动，B4 处理。

**A5d：迁出 platform 的测试并退役 platform。**

- 应用集成测试 `integration_tests/{profiles,facade,workflow}.rs` 移入 core 的测试（`backend/nyanpasu-core/tests/` 或 `cfg(test)` 模块），core dev-dependency profile-io 与 net；真实适配器在这些测试本地组装。core 导出的 `test-support` 不变（仍按 `ClientHostAdapters` 参数化，不链接实现 crate）；`P/profiles/test_support.rs` 的组装职责移到 tauri 的测试本地支撑，六处使用者改用它。
- 删除 crate、workspace 项、tauri 的两处依赖、门禁表里的 platform 与"platform → core"特例、文档中现存的 platform 描述（`backend-packages.md`、`architecture.md`、`AGENTS.md` §5、路线图的"Delivered baseline"与"Known gaps"）。

栈顺序：A5a → A5b → A5c → A5d，每步独立可编译。

### 4.6 每个 Phase A PR 的固定清单

1. workspace `members`、依赖项（关默认 feature）、门禁集合与策略测试、`STATIC_GATE_PREFIXES` 与其覆盖测试在同一提交里更新；静态允许名单随代码移动改路径。
2. 调用方同一提交迁完，不留旧路径 re-export 或别名。
3. 涉及 specta 类型的移动，重新导出 bindings，三个生成文件无 diff。
4. 本地跑：`cargo clippy --workspace --all-targets --all-features`、动到的包的 `cargo test`、`pnpm run lint:backend-boundaries`、`pnpm run lint:architecture-ledger`、`pnpm run test:scripts`。
5. 旧路径检查按所属 crate 限定范围，`rg` 退出码 2 视为检查失败而不是"无匹配"。
6. 动到 `cfg(unix)`/`cfg(target_os = "macos")` 代码时用 `cargo check --target x86_64-unknown-linux-gnu` 补 Linux，macOS 交给 CI。

---

## 5. Phase B–E 设计（里程碑粒度）

依赖图（唯一版本，`tasks.md` 与此一致）：

```text
A2 ── A2b
A5b ── B1a ── B1b ── B2 ── B3
        └──────────────────── D1
B3a ── B4
A ──┬─ C1 ── C3
    ├─ C2
    └─ E1a ── E2 ── E3
B4 ── C2
C2 ── E2
B3、B4、C1、C2、C3、D1 ── D2
E1a、D2 ── E1b
C2、C3、E1b、E3 ── E4 ── E5
```

A2b 只需 A2；B1a 只需 A5b，B1b、B2、B3 沿图中链条依次跟进，不必等 A5c、A5d；B4 需要 B3a 建好的 owner 支撑 crate（V-13）；C1、C2、E1a 需要 A 完成。C2 的安装请求由 core 从 B4 下移到 updater 的制品转换而来，制品的生命周期仍由 workflow 持有，contracts 与 OS 不依赖 updater；C3 只需要 C1 的展示接缝，不需要 C2；E2 把 C2 的审计切面随注册表迁走。D1 只需 B1a 的格式与持久化依赖；D2 需要 B、C 全部完成；E 的完整协议 DTO（E1b）需要 E1a 与 D2 下移的运行时值，CLI（E4）因此在 D2 之后；E4 的 server 注入 C2 的拒绝策略。每个阶段开始前，按 §4 的粒度把该阶段写成独立任务，先审再做。

### 5.0 A2b：脚本扩展注入

| 项       | 内容                                                                                                                                                                                                                              |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 决策     | 脚本可触达的能力（模块加载器、HTTP 模块、文件系统、console）属于扩展定义，由 shell 注入；引擎由 feature 选择                                                                                                                      |
| 前置     | A2                                                                                                                                                                                                                                |
| 风险     | HTTP 模块与缓存过期是 `ScriptRunner::run` 之外的输入，确定性契约（`Q/runtime/executor/ports.rs:27`）对它们不成立；去掉 Lua 时 overlay 的 `eval_item_*` 总是走 Lua（`P/enhance/script/adapter.rs:75-103`），需要明确的"不支持"结果 |
| 完成判据 | scripts 的构造参数显式列出扩展；`boa_utils` 的 HTTP 加载器不再自建 reqwest 客户端（`rg 'reqwest::Client' backend/boa_utils/src` 无匹配）；关闭某引擎 feature 时相关脚本返回结构化不支持错误（测试）                               |

### 5.1 Phase B：叶 owner 与重依赖

| 步  | 关键决策                                                                                                                                                                           | 前置 | 风险最高的不变量                                                                                                 | 完成判据（机械检查）                                                                                                                                                                                                                                                                                                     |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| B1a | persistence 收 `format.rs` 与从 `migration::fs` 拆出的通用原子写；`session_state` 改用它，不再依赖 migration；core-manager 的 IO 分类移到其消费边界                                | A5b  | 恢复时保全证据（两份都不覆盖）；瞬时 IO 在生产读点重试                                                           | `rg 'migration::' backend/nyanpasu-core/src/session_state.rs`（同时覆盖 `crate::` 与 `super::`，在基线上有匹配）无匹配；门禁禁止 persistence → redb/core-manager；`evidence_collision_never_overwrites_either_copy`、`session_read_retries_transient_io_at_its_production_read_call_site`（`C/session_state.rs:81`）通过 |
| B1b | 为 backup 与 migration 引入存储快照/迁移端口（实现留在存储 owner），再把 backup 与 migration 移成 core 族的 migration crate                                                        | B1a  | 在线备份与迁移的错误闭包、子进程结果分类不变                                                                     | migration crate 不依赖 `redb`；在线备份与迁移测试通过                                                                                                                                                                                                                                                                    |
| B2  | `Storage` 与 traffic 存储以 owner 级端口注入，facade 签名不出现具体存储类型；legacy 导入器放到 feature 后或单独 crate，持久进度与放弃语义不变                                      | B1b  | 导入在暴露会话前完成、不确定失败后重开、失败导入先持久放弃、句柄关闭后才删旧文件（`C/traffic/storage.rs:49-75`） | `ClientSetupArgs` 的字段类型不含 `Storage`、`RedbStorage`、`redb`（检查 `C/client/mod.rs:101-134` 及其类型闭包）；`cargo tree -p nyanpasu-traffic --no-default-features -e normal` 不含 redb                                                                                                                             |
| B3  | 原始 Clash 帧的归一化留在 owner 之上；geo 改吃中立的 mode/instance 观测，不再持有 `CoreClient`；watcher、缓存、存储的具体实现与错误在下层端口后；`owner_shutdown` 在此按需建 crate | B2   | traffic 泵的"一次一个 owner 调用、合并更新、转换时采样索引、区分断开、真实回复"（`C/traffic/actor.rs:306-354`）  | owner crate 生产代码不含 `clash_api`、`clash::ws`、`CoreClient`、`redb`、`notify_debouncer_full`（门禁 + 限定范围的 `rg`）；`restart_with_the_same_instance_does_not_recount`、`recovery_between_core_changing_close_groups_preserves_instance_and_events`（`C/traffic/tests.rs:1539,677`）通过                          |
| B4  | updater 后端在 shell 构造后注入（会话代理源存在之后）；`core_lifecycle` 的制品/进度类型下移到 updater 与应用都能用的位置；updater 的 HTTP 客户端随后端走                           | B3a  | 已开始的解压/安装在关闭时跑完；下载在关闭时取消；去重与终态进度                                                  | `rg HttpUpdaterBackend backend/nyanpasu-core/src/client` 无匹配；facade crate 的直接依赖不含 `zip`、`tar`、`flate2`、`bolt-load`；`the_shutdown_waits_for_an_install_in_progress`、`…_extraction_in_progress`（`C/updater/tests.rs:281,346`）通过                                                                        |

B 结束时复测编译时间（§6）。B1a–B4 已在 `tasks.md` §2 细化为卡（行号记录于 `5aa056c1f`），偏差见 §8.1 V-10–V-15。

### 5.2 Phase C：能力与目标 crate

| 步  | 关键决策                                                                                                                                                                                                                                                                                                                                                                                                               | 前置  | 风险最高的不变量                                                                                                                        | 完成判据                                                                                                                                                                                                                                                                                                                                |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | window 控制、加速键校验、展示效果执行、debug HTTP 在 `ClientSetupArgs` 中可选；缺失时依赖它的操作返回结构化"不支持"，显式热键编辑在提交前被拒，期望的展示效果报告 `Unsupported`。只观察的 sink（`UiEventSink`、`FrontendLogSink`）保持必需，没有 GUI 的 shell 传中立实现（V-17）                                                                                                                                       | A     | 不支持不得报告为已应用，也不推进 applied revision；过期修订不得覆盖新期望；源提交与效果收敛分开                                         | core 测试里有一个不传任何展示能力的装配，断言依赖操作返回不支持、热键编辑不落盘；`stale_completion_does_not_publish_success_for_new_desired`（`C/client/effects/tests.rs:304`）通过                                                                                                                                                     |
| C2  | 新建实现 crate `nyanpasu-os`：服务进程控制、DNS 刷新、core 二进制安装、设备标识、系统代理读写与 PAC、自启动、sidecar 版本探测；它实现的端口进 contracts，服务端口只含进程控制与原始探测，安装端口收窄为不含 updater 类型的请求（V-18）。新增提权同意端口，OS 的提权机制先经它再启动命令；桌面注入的策略保持现状（OS 授权框即同意），拒绝策略留给 E4 的 server。`update_core` 去掉 `http`；RPC 层加审计切面（O-5、O-6） | A、B4 | 服务安装与恢复、DNS 守卫的恢复语义不变；staging 生命周期与 panic 传播不变；拒绝时不启动任何提权命令；审计不记载荷、不改变命令结果与取消 | core 无提权执行点（`rg 'runas\|osascript\|with administrator privileges' backend/nyanpasu-core/src`）；门禁：`CORE_FAMILY` ↛ `nyanpasu-os`、`runas`、`sysproxy`，`nyanpasu-os` ↛ `CORE_FAMILY`；拒绝策略下三类提权机制都不启动命令、经 HTTP 调用 `update_core` 在入队前得到不支持、IPC 与 HTTP 调用各产生一条不含载荷的审计记录（测试） |
| C3  | 不建 `nyanpasu-desktop`：热键 owner、main-thread 交接、展示效果执行（locale、托盘、widget 与整托盘刷新的记忆）移到 `T/desktop/`；core 保留消费端口、提交前的热键校验、热键动作分发与效果的共享收敛（修订、源切片、重试、状态与 receipt）（V-20）                                                                                                                                                                       | C1    | locale 先于托盘、失败的整托盘刷新优先于之后的部分刷新（`C/client/effects/executor.rs:174-188,216-219`）；关闭时释放热键、恢复代理       | core 族不含上述机制（迁移时的 `rg` 检查），core 不再直接依赖 `nyanpasu-widget-protocol`；`commit_receipt_and_status_keep_source_separate_from_pending_notifications`（`C/client/effects/tests.rs:803`）、`stale_revision_is_superseded`（`C/client/hotkey/tests.rs:437`）、main-thread 丢失测试（`C/client/main_thread.rs:178`）通过    |

C2d 已实现同步 `ElevationConsent` 与 `RefuseElevation`：服务、DNS flush 和复制回退先检查策略，拒绝不启动提权命令，服务自动恢复暂停。桌面显式注入允许原 OS 授权框的策略，不加确认框；E4 的独立 server 必须向这三个 OS 构造器注入拒绝策略，并验证 V-19 的剩余路径。C2e 已在 job admission 前拒绝 HTTP `update_core`（unsupported / 501），桌面更新不变；C2f 的审计仍独立实施。

C1–C3 已在 `tasks.md` §3 细化为卡（行号记录于 `e004ef566`），偏差见 §8.1 V-16–V-22，决策见 §8.2 O-5–O-9。

### 5.3 Phase D：事务图

| 步  | 关键决策                                                                                                                                                                                                                         | 前置     | 风险最高的不变量                                                                                                                                          | 完成判据                                                                                                                                                                                                                                                                                                    |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | 状态引擎按完整泛型闭包一起拆：路线图列的 7 个文件（3,253 行）加 `state/{ack,builder,coordinator,decision,error,snapshot,version}.rs` 与 `state/tests/`，共约 7,000 行；引擎不导入应用的 mutation receipt 或 workflow 类型        | B1a      | 必需投票、CAS、回滚、drop 结算、权威结算、持久不确定                                                                                                      | `coordinator.rs:769,829,906,1112,1272`、`persistent_state.rs:710,880,940`、`transaction.rs:581,604`、`state/tests/{single_shot_participant,persistence_settlement}.rs` 的测试名清单移动前后一致且通过；新 crate 不依赖 `CORE_FAMILY` 其他成员                                                               |
| D2  | 先反转 mutation 参与：源 owner 收按域的参与者工厂（工作形状见下），`MutationCoordinator` 不再存具体 workflow client；运行时值（receipt、degradation、outcome）与其错误闭包下移到应用层之下的值 owner；然后拆 core-control 与应用 | D1、B、C | 准备投票在持久化之前、终态结算在权威决定之后；调用方离开不停止 owner 工作，丢失的判决不当作拒绝（`C/client/application_workflow/participant.rs:103-117`） | 状态引擎与源 owner 包内 `rg ApplicationWorkflowClient` 无匹配；门禁禁止下层 → facade；`cancelled_profile_waiter_keeps_admission_and_reconcile_does_not_replay_interruption`（`…/tests/connection_policy.rs:718`）、`tests/mutations.rs:1257,2527`、`C/actor_v2/facade.rs:1424`、`tests/closing.rs:113` 通过 |

D2 参与者工厂的工作形状（落地时以代码为准）：

```rust
trait MutationParticipantFactory<T>: Send + Sync {
    fn make(
        &self,
        intent: MutationIntent,
        decision: DecisionHandle,
        settle: oneshot::Sender<MutationReceipt>,
    ) -> StateParticipant<T>;
}
```

它属于应用/源 owner 边界，在状态引擎之上；不放进 contracts（否则 contracts 要依赖状态引擎的 `DecisionHandle`）。

### 5.4 Phase E：RPC 目标与 shell

| 步  | 关键决策                                                                                                                                                                                                                                              | 前置            | 风险最高的不变量                                            | 完成判据                                                                                                                                                             |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1a | 宏改为中立声明加按传输的包装与按目标 opt-in（`rpc(http)` 之外加 `rpc(local)` 与能力声明）；`CommandEntry` 记录每目标暴露与能力元数据；命令名、事件名、`RpcError` 信封进 `nyanpasu-rpc-protocol`，`RpcError::application_ref` 的向下转型留在注册表一侧 | A               | 线上形状与 bindings 不变                                    | protocol 的 `cargo tree` 不含 `CORE_FAMILY`、Tauri；三个生成文件无 diff；宏展开测试（`unified_command.rs:443,483,514,552`）通过                                      |
| E1b | 请求/响应 DTO 的完整闭包进 protocol（`MutationOutcome` 等随 D2 已下移的值）                                                                                                                                                                           | E1a、D2         | 同上                                                        | protocol 的类型闭包不经 `CORE_FAMILY`；bindings 无 diff                                                                                                              |
| E2  | 拆 `nyanpasu-rpc`：中立注册表与事件总线；观测先发布到总线，Tauri 事件、SSE、local socket 都是订阅者；Tauri IPC 绑定留在 shell                                                                                                                         | E1a、C2         | 结构化失败、目标授权、owner 隔离；传输断开不取消 owner 工作 | 注册表无 Tauri 类型；事件测试证明发布不经 Tauri                                                                                                                      |
| E3  | HTTP 支持纯 API 模式（静态资源可选，凭据与 Host/Origin 中间件不变）；local socket 传输（服务端/客户端分 feature，可复用 local-channel 的分帧与权限，但需要可发现端点、重复 accept、版本协商、请求关联、连接拥有的资源与订阅）                         | E2              | 两个允许名单由注册表执行；`RpcOwner` 增加连接身份           | 无前端资源时 API 启动通过；未 opt-in 的命令被拒（测试）；跨传输的 owner/错误一致性测试通过                                                                           |
| E4  | 组装 `nyanpasu-server`（一个长期进程持有 `NyanpasuClient`），再做 `nyanpasu-cli`（只链接 protocol 与 local-socket 客户端）                                                                                                                            | C2、C3、E1b、E3 | CLI 不按调用构造应用图；结构化结果不丢                      | 门禁：CLI ↛ `CORE_FAMILY`、Tauri；server ↛ Tauri；server 注入拒绝提权策略，#6157 余下的 HTTP 路径不触发提权（测试）；连续 CLI 调用观察到同一 server 会话（集成测试） |
| E5  | OpenWrt，等用户明确恢复                                                                                                                                                                                                                               | E4              | —                                                           | 单独验收 LuCI → CLI → server 授权、procd、打包与硬件                                                                                                                 |

需要 shell 能力的命令（E1a 的能力元数据据此标注）：剪贴板（`T/ipc.rs:202,212,2147`）、原生通知（`:223`）、原生对话框（`:239,292,324,1091,1337`）、调用方窗口/webview 身份与 Channel（`:1915,1937,2180`）、窗口管理（`:2107,2117,2125,2165`）、shell 生命周期（`:1367,2135,2158`）、opener/编辑器/报告器（`:683,971,979,1004,1012,1048,1056,1278,1290`）、托盘与 widget 图标（`:1432,1452,1513,1527`）、深链队列（`:479`）、桌面包更新（`:2022-2100`）。提权：DNS 刷新（`:963`）、服务五个命令（`:1576-1608`）、Windows `set_custom_app_dir`（`:1344`）与 `invoke_uwp_tool`（`:1417`）、经回退的 `update_core`（`:1114`）。`patch_app_config`、`set_hotkeys`、`retry_configuration_effect` 按载荷可能触发展示或 OS 效果，能力元数据要按可达效果标注，不能只看签名。

---

## 6. 构建测量

A、B 结束的复测都没有桌面编译收益，增量构建仍由 `clash-nyanpasu` 库与链接主导。C 因此只为 server 需要的依赖方向建 `nyanpasu-os`，桌面机制不另建 crate（O-7、V-20）。

- **环境固定：** 提交与 submodule 状态、`rust-toolchain.toml` 的工具链、目标三元组、profile、feature、rustflags（`.cargo/config.toml` 的 AES 标志）、target 目录状态、`backend/tauri/tmp/dist` 前置物都写进记录。`boa_utils` 的 builtin 在构建期取远程资源，先完成一次构建把网络准备与编译计时分开。
- **场景：** 干净的 `cargo build --timings -p clash-nyanpasu`（dev profile）；在热缓存上分别对下列文件改一行后的增量重建时间：`C/client/mod.rs`（facade）、`C/client/application_workflow/` 中任一文件（workflow）、脚本 runner（A2 前在 platform，之后在 scripts）、`T/adapters/` 中任一文件（桌面适配器）。每个场景跑 3 次取中位数。
- **时机：** T0（feature 接线修正后）一次；A 结束、B 结束各一次。结果写进对应阶段末 PR 的描述，不另建报告文件。
- **用途：** 收益趋平处停止加边界；总是一起改的 crate 合并。`test-support` 双份编译的修法在 A 结束复测时按数据决定（§8 O-3）。

---

## 7. 迁移不变量

1. 不加全局单例；依赖经构造参数、actor 启动参数传入；`NyanpasuClient` 仍是 facade。
2. actor 所有权、generation fence、实例 API 租约、调用方离开不取消、进程内真实回复、有序关闭（根 `CancellationToken` + 各 owner `post_stop`，`owner_shutdown` 的 `drain_and_wait(None)`）不变。
3. panic 策略不变：release `panic = 'unwind'`（`backend/Cargo.toml:112`），GUI panic hook 弹框后退出，`blocking::join` 重新抛出任务 panic（`backend/nyanpasu-owner-support/src/blocking.rs:6`）；不新增 `catch_unwind`。
4. profile/配置 owner 与其运行时 mutation 参与者作为一个事务边界移动；投票、提交、materialization、提升/补偿、降级、重试、重启协调语义不变。
5. 结构化错误与序列化不变；改动 specta 类型时重新导出 bindings。
6. 每个 PR 迁完调用方，不留 re-export、别名或兼容层；确需兼容层时先在 PR 中说明，再按 `architecture.md` 的格式标注原因与移除条件。
7. 每个 PR 可编译、测试通过、只做一件事。
8. 移动不夹带行为改动；发现的现存问题（如 async 方法内的阻塞读）记下，单独处理。

---

## 8. 与路线图的偏差和决策

### 8.1 偏差（已同步修订路线图与开发规范）

| #    | 路线图                                                                           | 本设计                                                                                                                                                                                                                         | 理由                                                                                                                                                                                                                                                 |
| ---- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| V-1  | A1 把脚本描述符移到 core 之下                                                    | 删除 `ScriptType`，用 config 已有的 `ScriptRuntime`；`ScriptWrapper` 私有                                                                                                                                                      | 两个枚举完全重复（§1.3）                                                                                                                                                                                                                             |
| V-2  | A4 包含"capabilities and update"组                                               | 能力端口随 C2/C3 的目标 crate 搬；A4 只搬 profile 与网络两组                                                                                                                                                                   | 这些端口在 Phase A 只有 tauri 实现，tauri 本来就依赖 core                                                                                                                                                                                            |
| V-3  | A4 包含"runtime values"组，"keep operations and errors in application"           | 运行时值随 D2 下移；E 的完整 DTO 协议（E1b）与 CLI 因此排在 D2 之后                                                                                                                                                            | `DegradationReason` 内嵌 `RuntimeError`、`ProfilesError`（§1.5），值与错误分不开；`state → client` 还有大量操作性依赖（§1.7）                                                                                                                        |
| V-4  | runtime-build 适配器在只用 contracts 后移到 profile-io                           | 编排上移 core 应用层，下层只留内容读取                                                                                                                                                                                         | 适配器构造应用快照并依赖 core-manager 与 `ProfilesActor` 的闭包算法（§1.4、§1.6）                                                                                                                                                                    |
| V-5  | owner shutdown 移到 actor 支撑 crate                                             | A1 先移到 core 根模块，B3 按需建 crate                                                                                                                                                                                         | 39 行；跨 crate 使用者 B3 才出现                                                                                                                                                                                                                     |
| V-6  | persistence 属于 core 族；实现 crate 只依赖 contracts 与 config                  | 分出基础层（config、contracts、persistence、transform），实现 crate 可依赖整个基础层；persistence 只放不含 redb、core-manager 的通用机制，依赖 redb 的存储、备份、migration 留在 core 族                                       | 通用文件恢复被 profile-io 与 core 共用（§1.6），又不含任何应用状态；按"谁能依赖它"划层比按主题划层更准确                                                                                                                                             |
| V-7  | 订阅 HTTP 进 profile-io                                                          | 订阅 HTTP 与其响应头解析进 net，profile-io 只管文件与 materialization                                                                                                                                                          | `ProfileFilePorts` 已按三个端口分开（`C/client/host_adapters.rs:17-23`）；这样实现 crate 之间无依赖，HTTP 策略只有一处                                                                                                                               |
| V-8  | 环表与"14 个模块的 SCC"                                                          | 16 个模块；补 `proxies`、`updater`、`migration ↔ session_state`；`state → client` 按符号列                                                                                                                                     | §1.7                                                                                                                                                                                                                                                 |
| V-9  | Known gaps 的"YAML parsing"、`Cargo.toml:266`、提权位置、B1 的顺序、E1 独立于 D  | 删除 YAML；改为 `:268,271`；补"二进制安装的提权回退"与"`update_core` 可经 WebUI 触达提权"；B1 拆成 B1a/B1b；E1 拆成 E1a/E1b                                                                                                    | §1.2、§1.6、§1.8、§5                                                                                                                                                                                                                                 |
| V-10 | B1a 让 persistence 收格式与通用原子写，`session_state` 改用它                    | B1a 只收 `format` 与通用写入（`atomic_write`、`write_document` 与其错误）；`session_state` 的类型化读取随 migration 进 `nyanpasu-migration`（B1b-2）                                                                           | 它只读一个 config 文档类型，放进 persistence 会让基础层依赖 config 并含领域读取；migration 与 client 都用它，migration crate 在 core 之下                                                                                                            |
| V-11 | B1a 把 core-manager 的 IO 分类移到其消费边界                                     | `corrupt::error_io` 留在 core；`Failure::from_error` 改为接收调用方算好的 `Option<IoFailure>`，tauri 三处调用方传入（B1b-1）                                                                                                   | migration 唯一的分类调用就是这一处；分类含 redb 与 core-manager，下移会让 migration 依赖 redb                                                                                                                                                        |
| V-12 | B3 把 logs、traffic、geo 各建 owner crate                                        | 三个 owner 进一个 `nyanpasu-observers`；具体适配器（redb 日志库、traffic 文件库与 legacy 导入、geo 监视与缓存、Clash 原始帧归一化、core 模式跟随）留在 core，由 core 实现 owner 的端口                                         | 三者没有独立的消费者；A 结束的复测显示边界不带来桌面编译收益（§6）；适配器留在 core，server 可直接复用                                                                                                                                               |
| V-13 | `owner_shutdown` 在 B3 按需建 crate                                              | B3a 建 `nyanpasu-owner-support`，同时收 `utils::blocking::join`                                                                                                                                                                | geo、traffic 的 owner 都用 `join`；contracts 禁止 ractor，不能放进基础层                                                                                                                                                                             |
| V-14 | B4 的 updater 后端与 HTTP 客户端由 shell 注入                                    | 后端与下载、解包实现随 owner 进 `nyanpasu-updater`；facade 不再构造后端，改由 shell 经工厂注入                                                                                                                                 | 完成判据是 facade 的**直接**依赖不含归档库；server 同样需要这个后端，另建实现 crate 需要把 updater 的端口与值搬进 contracts，收益不抵成本                                                                                                            |
| V-15 | B4 把 `core_lifecycle` 的制品、进度类型下移                                      | 另外下移 `parse_version` 与 `CoreVersionError`（进 contracts）和 `utils::candy`（进 updater）                                                                                                                                  | updater 还经 `client::core_version` 与 `utils::candy` 回到 core（`C/updater/instance.rs:27,111,670`）；版本解析也被 shell 的 `CoreVersionReader` 实现使用，C2 的 `nyanpasu-os` 也要用                                                                |
| V-16 | §1.5、§1.8 的 C 相关端口位置与行号                                               | A、B 之后多数已移动：profile 与网络端口在 contracts，实现在 profile-io 与 net；`update_core` 在 `T/ipc.rs:1125-1129`，安装经 `U/updater/mod.rs:315-320`；C 的卡按 `e004ef566` 重新定位                                         | §1 保留为 `5ed3c019e` 的历史事实；§5.2 引用的四个测试锚点仍有效                                                                                                                                                                                      |
| V-17 | C1 把前端日志 sink 当作展示能力，并假定展示能力都是 `ClientSetupArgs` 的直接字段 | 只有执行操作的能力可选（window、加速键校验、展示效果执行、debug HTTP）；`UiEventSink` 与 `FrontendLogSink` 保持必需，没有 GUI 时传中立实现；app-update 的后端与 sink 维持现有的可选性                                          | `FrontendLogSink` 把前端诊断写进后端日志（`C/client/frontend_events.rs:96-105`），不是 GUI 输出；debug HTTP 在 `ClientHostAdapters` 里（`C/client/host_adapters.rs:49`），locale、托盘、widget 是效果执行器的依赖，main-thread 执行器只在 shell 装配 |
| V-18 | C2 把能力端口整体搬进 contracts                                                  | 服务端口拆开：contracts 只放进程控制与原始探测，endpoint 构造与状态解析留 core；二进制安装端口改收不含 updater 类型的窄请求（含 staging 租约），制品与进度留 updater；OS 自带一个与 `owner-support` 同语义的私有 blocking join | `ServiceHostAdapter::endpoint` 返回 core 的 `EndpointHandle`（`C/actor_v2/service_actor.rs:52`）；`BinaryInstaller` 收 updater 的 `PreparedCoreBinary`（`C/client/core_lifecycle/ports.rs:45`）；OS 不能依赖 updater 与 owner-support                |
| V-19 | C2 让 WebUI 完全不能触发提权                                                     | 只去掉 `update_core` 的 `http`；`patch_app_config` 开关服务模式、`retry_configuration_runtime`、事务补偿仍经共享 client 走到桌面的提权策略，记为 #6157 的已知缺口，由 E4 独立装配的 server 注入拒绝策略关闭                    | 嵌入式 WebUI 与桌面 IPC 共用一个 `NyanpasuClient`（`T/setup.rs:586-596`），组合根注入的策略区分不了调用方；按调用携带授权上下文要贯穿 facade、actor 消息、排队操作与补偿，属于 E1a 的能力元数据                                                      |
| V-20 | C3 新建 `nyanpasu-desktop`                                                       | 不建 crate，机制移到 `T/desktop/`                                                                                                                                                                                              | A、B 两次复测都没有桌面编译收益（§6）；server 不需要这些机制；规范要求目标 crate 只依赖基础层，建 crate 还得先把热键与展示效果的端口和值搬进 contracts                                                                                               |
| V-21 | §9：除 bindings 外不改前端                                                       | 行为卡可改 `ipc-error.ts` 中新变体、新 code 的分支与对应 i18n 文案，并在 `ipc-error.browser.test.ts` 补回归测试；不改页面与组件                                                                                                | 前端的错误映射对导出的错误枚举穷举（`frontend/nyanpasu/src/utils/ipc-error.ts:201,672`），加变体不补分支 typecheck 失败                                                                                                                              |
| V-22 | 路线图没有 RPC 审计                                                              | C2 加审计切面：一处覆盖所有传输，只记元数据；E2 随注册表迁走，E3 的 local socket 接入同一切面                                                                                                                                  | 维护者要求（2026-11-05），为之后记录 HTTP 与 CLI 调用                                                                                                                                                                                                |

`K/`、`U/` 分别指 `backend/nyanpasu-contracts/src/`、`backend/nyanpasu-updater/src/`。V-16–V-22 的源码锚点均记录于 `e004ef566`，新符号与测试见 tasks §3。

### 8.2 决策

| #   | 问题                                                  | 选项                                                                             | 结论                                                                                                                                         |
| --- | ----------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| O-1 | 通用文件恢复放哪                                      | a. A5b 建 `nyanpasu-persistence` 作为基础层成员；b. 并入 `nyanpasu-paths`        | **已定（2026-11-03）：a。** 通用恢复放 `nyanpasu-persistence`，作为基础层成员；门禁禁止它依赖 redb、core-manager、core 族                    |
| O-2 | 订阅 HTTP 放哪                                        | a. profile-io 并允许 profile-io → net；b. profile-io 内自带 HTTP 策略；c. 进 net | **已定（2026-11-03）：c。** 订阅 HTTP 与其响应头解析放进 net（V-7）                                                                          |
| O-3 | `test-support` 经 dev-dependency 打开导致 core 编两份 | a. 拆独立测试支撑 crate；b. 接受现状                                             | 待定：按 A 结束的复测数据定，T0 只记录                                                                                                       |
| O-4 | crate 正式名                                          | 工作名沿用路线图                                                                 | 待定：创建该 crate 的 PR 确定                                                                                                                |
| O-5 | WebUI 触发提权（#6157）在 C 处理到什么范围            | a. 去掉 `update_core` 的 `http`，加同意端口；b. 按调用携带授权上下文             | **已定（2026-11-05）：a。** 其余路径见 V-19                                                                                                  |
| O-6 | RPC 审计                                              | 维护者要求                                                                       | **已定（2026-11-05）：** C2 预留审计切面，只记传输、命令、调用方、耗时与结果；HTTP 调用记 INFO，Tauri IPC 记 DEBUG；不存储、不轮转、不做界面 |
| O-7 | 桌面交互机制放哪                                      | a. 新建 `nyanpasu-desktop`；b. tauri 外壳模块；c. 留在 core                      | **已定（2026-11-05）：b。** 见 V-20                                                                                                          |
| O-8 | 没有热键能力时的显式热键编辑                          | a. 提交前一律拒绝（含清空）；b. 只允许清空；c. 只做语法校验后提交                | **已定（2026-11-05）：a。** 其他配置修改不受影响；已有期望的热键效果报告 `Unsupported`                                                       |
| O-9 | 行为卡能否改前端                                      | a. 允许窄范围改动；b. 不加新变体，只用传输层的不支持；c. 行为改动推迟到 E        | **已定（2026-11-05）：a。** 见 V-21                                                                                                          |

---

## 9. 不在范围

- 任何行为改动：本设计只移动代码、注入依赖、收窄依赖方向。行为改动（A2b 扩展注入、C1 不支持结果、C2 的同意端口、`update_core` 的 HTTP 限制与 RPC 审计）各自是独立评审的提交，同一 PR 里也不夹进移动提交。
- 前端：移动卡只重新导出 bindings，三个生成文件无 diff。C 的行为卡另可改 `frontend/nyanpasu/src/utils/ipc-error.ts` 中新变体与新 code 的分支、对应 i18n 文案，以及 `frontend/nyanpasu/tests/ipc-error.browser.test.ts` 中对应的回归测试（V-21），不改页面与组件。E 阶段需要的前端传输改动在 E 细化时另写。
- OpenWrt（E5）在用户明确恢复前不做。
- [#6157](https://github.com/libnyanpasu/clash-nyanpasu/issues/6157)：C2e 只在 admission 前拒绝 HTTP `update_core`，其余嵌入式 WebUI 提权路径见 V-19，由 E4 关闭。
- runtime submodule（`nyanpasu-core-manager`、`clash-api` 等）的内部结构。
