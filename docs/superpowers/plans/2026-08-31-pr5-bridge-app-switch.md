# PR-5 C-switch / D-switch 实施计划（app 仓，2026-08-31）

- **依据**：`docs/design/2026-08-12-core-actor-v2-app-integration.md`（规范正文）、`docs/design/2026-08-08-core-manager-control-plane-runtime-backend-design.md`（修订 A1–A6）、`docs/audit/2026-08-12-core-actor-audit-verification.md` §3/§4、`docs/audit/2026-08-13-v2-implementation-audit-guide.md`（模块图 + L1–L13 + 测试跑法）、`docs/superpowers/plans/2026-08-13-pr-abcd-implementation-plan.md` §0.2、`docs/design/actor-migration-roadmap.md` §6/§8
- **基线**：app `origin/main` = `3d5a518d`（#5116 squash-merge）；submodule `backend/nyanpasu-runtime` gitlink = `6717e44`（`v2.0.0-rc.1-29-g6717e44`）
- **本阶段定义**（2026-08-13 计划 §0.2 的 "C-switch" + D-switch）：把"并存不接线"的 v2 栈接进组合根与 Tauri 命令，吸收 legacy service ipc 形态，删除被取代的 GUI 核心生命周期形态，并关闭审计明确顺延到 bridge 阶段的结构项（L3 / L9 / L13 / L4）。
- **一句话**：核心生命周期的真相从 app 进程内的 `CoreManager` 单例迁出，改由 `CoreControl`（Local 内嵌 / Service 在 daemon 内）拥有；app 只剩 endpoint 路由、投影与编排。

---

## 0. 范围

### 0.1 本阶段做

| #   | 事项                                                                                                                 | 对应设计                    |
| --- | -------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| 1   | 组合根构造 Local host（`CoreControl`）与 `OsServiceHostAdapter`，spawn `CoreActor` + `ServiceActor`                  | 集成设计 §2、§6.1           |
| 2   | facade 编排落地（`reconcile_core` / `update_core` / `stop_core` / `change_execution_host` / shutdown shared-future） | 集成设计 §3.1、§6.4、§7；L4 |
| 3   | Tauri 命令换线为 `NyanpasuClient` 上的薄适配器                                                                       | AGENTS §12                  |
| 4   | 吸收 `core/service/ipc.rs` 三 statics / 七入口                                                                       | 集成设计 §6.5               |
| 5   | 删除被取代的 GUI 核心生命周期形态（见 §6 删除集）                                                                    | 审计 §4 删除表              |
| 6   | L3 `CoreType` 直通、L9 uninstall 结构性互斥、L13 终态 `ShutDown` slot                                                | 审计入口 §4                 |
| 7   | gitlink 推进 + compat 门从"仅比 major"收紧为最低版本                                                                 | L10                         |
| 8   | ledger 快照按真实计数重算                                                                                            | 本计划 §9                   |

### 0.2 本阶段**不**做（明确划归他处）

- **PR-7 清算**：`Config::global()` / `Draft<T>` / `Handle::global()` / `consts::app_handle()` / `feat.rs` 编排中心 / `bridge/` 运行期 mirror / legacy `IVerge`·`IClashTemp` DTO。roadmap §8.1/§8.2。本阶段只把这些文件里**核心生命周期那几条腿**改指向 facade，文件本体、`patch_verge` 的形状、`LegacyVergeBridge` 一律不动。
- **runtime 仓改动**：版本号提升、v1 endpoint 删除、daemon 侧任何代码。见 §4 跨仓顺序接口。
- **前端 TSX**：本计划只交付 Rust/Tauri 命令 + 事件 + specta bindings 的接口面；前端消费改造见 §8「前端影响面」交给前端计划。
- **macOS DNS 实测**：L1 的 Phase-0 spike 仍未做；Local host 注入 `MacosDnsController` 只是接线，不构成验证。

### 0.3 对上游文档的三处更正（实施代理必须以本节为准）

1. **app 仓没有 `backend/tauri/src/core/actor/` 目录**。审计报告引用的 `core/actor/mod.rs:266-306`（`replace_backend` 等）属于**从未合入的** `refactor/core-manager-actor` 分支。`origin/main` 上被取代的形态是 `backend/tauri/src/core/clash/core.rs` 里的 `CoreManager` 单例 + `Instance` enum + `CoreLifecycleLease`。删除集按 §6 执行，不要去找 `actor/`。
2. **`ipc.rs` 的"七入口"在 `control.rs`**。三个 statics 在 `core/service/ipc.rs:28-30`；七个自由函数入口在 `core/service/control.rs`（`install_service`/`update_service`/`uninstall_service`/`start_service`/`stop_service`/`restart_service`/`status`）。集成设计 §6.5 的表述准确，2026-08-13 计划 §0.2 的"ipc.rs 三 statics / 七入口"是简写。
3. **`ServiceHostAdapter` 没有生产实现**。`core/actor_v2/service_actor.rs:34` 定义了 trait，实现只有测试 fake（`service_actor.rs:759`、`:891`、`:1240`）。本阶段必须新写 `OsServiceHostAdapter`（卡 S2）。

---

## 1. 分支拓扑与提交纪律

| 项目      | 值                                                        |
| --------- | --------------------------------------------------------- |
| 新分支    | `refactor/core-actor-switch`，自 `origin/main` `3d5a518d` |
| submodule | 起始 `6717e44`；gitlink 在卡 S13 推进（见 §4 的顺序约束） |
| 提交数    | 13 个原子提交（S1–S13），每个提交后分支必须绿             |

提交纪律（AGENTS §18，逐条强制）：

- 显式路径 `git add <path>`，禁止 `git add .` / `-A` / `*`；`git diff --cached --stat` 复核后再提交。
- 一个提交做一件事；发现某提交补了前一个提交，用 `git reset --soft HEAD~1` 折叠重提，不做 fix-up 提交。
- 提交信息主题祈使句 ≤72 字符不带句号；非平凡改动必须有正文，写清根因与取舍，不逐文件罗列。
- submodule 内不提交任何内容（本仓无 runtime 写权限范围）；gitlink 的推进单独成 S13 一个提交。
- **不得** `--no-verify`。若 pre-commit 的 clippy 因共享 target 损坏而失败，按 §10 用隔离 target dir 让它真跑通过。

---

## 2. 现状基线核实（全部 file:line，实施前不必重新推导）

### 2.1 组合根与 facade

| 事项                     | 位置                                                                                                                                                      |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 唯一构造点               | `backend/tauri/src/setup.rs:42-55` `NyanpasuClient::try_new_with_args(ClientSetupArgs { .. })`                                                            |
| 注入点                   | `setup.rs:61` `app.manage(client);`（`setup.rs:56` 先 manage `LegacyVergeBridge`）                                                                        |
| setup 调用点             | `lib.rs:248` `setup::setup(app)`；随后 `lib.rs:272` `resolve::resolve_setup(app)`                                                                         |
| 启动参数结构             | `client/mod.rs:75-84` `ClientSetupArgs { paths, runtime_paths, bridges, ui_sink, core: Arc<dyn CoreLifecyclePort>, clash_patch, system_dns }`             |
| 客户端内部状态           | `client/mod.rs:225-251` `NyanpasuClientInner`（17 字段；`core: Arc<dyn CoreLifecyclePort>` 在其中）                                                       |
| 已有 actor（5 个）       | `client/application.rs:51`、`client/session_state.rs:51`、`client/clash_config.rs:51`、`client/profiles.rs:62`、`core/clash/ws.rs:698`                    |
| 命令取客户端的方式       | `tauri::State<'_, NyanpasuClient>`，例：`ipc.rs:109`、`ipc.rs:192`、`ipc.rs:481`                                                                          |
| 首次起核                 | `utils/resolve.rs:200-204` `block_on(client.start_promoted_runtime())` → `client/mod.rs:1275` → `core.begin()` → `client/core_bridge.rs:128`              |
| 退出停核（**两处重复**） | `utils/help.rs:268` `CoreManager::global().stop_core()`；`utils/resolve.rs:288` `block_on(CoreManager::global().stop_core())`                             |
| 退出入口                 | `lib.rs:350-352` `RunEvent::ExitRequested { .. } => utils::help::cleanup_processes(app_handle)`（`lib.rs:347-349` 的 `code.is_none()` 臂 `prevent_exit`） |

### 2.2 被取代的核心生命周期形态

`backend/tauri/src/core/clash/core.rs`（742 行），经 `core/mod.rs:18 pub use self::clash::core::*;` 全局再导出：

| 符号                 | 位置          | 说明                                                                                                                                     |
| -------------------- | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `RunType`            | `core.rs:40`  | `Normal`/`Service`/`Elevated`；`Default` impl（`core.rs:64-82`）读 `Config::verge()` + `get_ipc_state()` —— 集成设计点名要消灭的隐式输入 |
| `RunType::classify`  | `core.rs:55`  | `(enable_service, IpcState) -> RunType`                                                                                                  |
| `Instance`           | `core.rs:84`  | 私有 enum：`Child{..}` / `Service{config_path, core_type}`                                                                               |
| `CoreLifecycleLease` | `core.rs:388` | 5 个方法（`core.rs:394/404/409/413/417`）                                                                                                |
| `CoreManager`        | `core.rs:425` | `global()` `core.rs:434`（`OnceCell`）；16 个方法，签名见下                                                                              |
| `find_binary_path`   | `core.rs:725` | **保留**：`core/manager.rs:8`、`utils/dirs.rs:345`、`core.rs:108/498` 用它                                                               |

`CoreManager` 方法与其全部调用点（`CoreManager::global()` 共 18 处，与 ledger `service_globals.CoreManager::global()` = 18 一致）：

| 方法                                                                                          | 调用点                                                                                                             |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `status()` `core.rs:451`                                                                      | `core/service/ipc.rs:86`、`client/core_bridge.rs:136`、`feat.rs:292`、`feat.rs:385`、`ipc.rs:403`                  |
| `run_core()` `core.rs:560`                                                                    | `core.rs:472`(`init`)、`core/service/ipc.rs:91`、`feat.rs:58`、`ipc.rs:503`、`ipc.rs:960/977/994`                  |
| `run_core_from()` `core.rs:552`                                                               | `core.rs:563`；经 lease：`core/updater/instance.rs:279`、`client/core_bridge.rs:221`                               |
| `stop_core()` `core.rs:611`                                                                   | `utils/help.rs:268`、`utils/resolve.rs:288`；经 lease：`core/updater/instance.rs:216`、`client/core_bridge.rs:225` |
| `recover_core()` `core.rs:567`                                                                | `core.rs:234`（`Instance::start` 崩溃自恢复）、`core.rs:579`（自重试）。**无 app 侧调用方**                        |
| `begin_lifecycle()` `core.rs:444`                                                             | `client/core_bridge.rs:128`、`core/updater/instance.rs:205`                                                        |
| `change_default_network_dns()` `core.rs:667` (macOS)                                          | `core.rs:540`、`core.rs:619`、`feat.rs:392`                                                                        |
| `init()` `core.rs:469` / `check_config()` `core.rs:480` / `apply_config_from()` `core.rs:636` | **零调用点**（`#[allow(dead_code)]`）                                                                              |

v1 IPC `/core/*` 调用点（runtime 仓删 v1 后会编译失败的全集，共 5 处，全在本文件）：`core.rs:270` `start_core`、`core.rs:299` `stop_core`、`core.rs:329` `status`、`core.rs:366` `status`、`core.rs:703` `set_dns`。
`core/service/control.rs:326 status()` 是 shell 出 `nyanpasu-service status --json`（`control.rs:327-328`），**不是 IPC 路由**，不受 v1 删除影响。

### 2.3 legacy service 形态

`core/service/ipc.rs`（302 行）——三个进程级 statics 与它们的全部读写点：

| static                 | 位置        | 类型             | 读写点                                                                                                   |
| ---------------------- | ----------- | ---------------- | -------------------------------------------------------------------------------------------------------- |
| `IPC_STATE`            | `ipc.rs:28` | `AtomicIpcState` | `ipc.rs:33/37/45/59`（文件私有）；经 `get_ipc_state()` 外泄到 `core.rs:73`、`feat.rs:359`、`feat.rs:377` |
| `KILL_FLAG`            | `ipc.rs:29` | `AtomicBool`     | `ipc.rs:101/110`；`control.rs:175`（`uninstall_service`）、`control.rs:270`（`stop_service`）            |
| `HEALTH_CHECK_RUNNING` | `ipc.rs:30` | `AtomicBool`     | `ipc.rs:103/112`；`control.rs:96/224/319`；`core/service/mod.rs:33`                                      |

`ipc.rs` 其余函数：`IpcState::is_connected` `:23`、`get_ipc_state` `:32`、`set_ipc_state` `:36`、`dispatch_disconnected` `:41`、`dispatch_connected` `:58`、`on_ipc_state_changed` `:74`（**隐式起核触发器**：`:86` `status()` + `:91` `run_core()`）、`spawn_health_check` `:100`（`std::thread::spawn` + 5s 轮询）、`next_incompatible_warning_state` `:137`、`health_check` `:153`、`target_ipc_state` `:195`（fail-closed 分类，带 `TODO(actor-migration)` 于 `:192-194`）。

`core/service/control.rs` 七入口与调用点：

| 入口                       | 定义             | 调用点                                                                        |
| -------------------------- | ---------------- | ----------------------------------------------------------------------------- |
| `get_service_install_args` | `control.rs:10`  | `control.rs:55`、`ipc.rs:1015`                                                |
| `install_service`          | `control.rs:54`  | `ipc.rs:938`                                                                  |
| `update_service`           | `control.rs:102` | `utils/init/mod.rs:259`（**唯一**，启动期自动升级）                           |
| `uninstall_service`        | `control.rs:145` | `ipc.rs:945`                                                                  |
| `start_service`            | `control.rs:184` | `ipc.rs:952`                                                                  |
| `stop_service`             | `control.rs:230` | `ipc.rs:969`                                                                  |
| `restart_service`          | `control.rs:279` | `ipc.rs:986`                                                                  |
| `status`                   | `control.rs:326` | `ipc.rs:154`、`core/service/mod.rs:29`、`ipc.rs:924`、`utils/init/mod.rs:244` |

`control.rs` 对 statics 的副作用（本阶段要摘除的）：`control.rs:96-97`、`:224-225`、`:319-320`（`HEALTH_CHECK_RUNNING` + `spawn_health_check`）；`control.rs:175`、`:270`（`KILL_FLAG`）。
启动期入口：`utils/resolve.rs:152 init::init_service()` → `utils/init/mod.rs:232`（`:244` status、`:251-252` `parse_service_version`、`:255` 版本比较、`:259` `update_service()`、`:274` `core::service::init_service()`）→ `core/service/mod.rs:18`（`:32` `spawn_health_check` + `:33-35` 忙等）。

### 2.4 v2 侧可直接消费的签名（不要重新发明）

```rust
// core/actor_v2/mod.rs
pub struct CoreActorArgs { pub initial: EndpointHandle, pub status_tx: watch::Sender<CoreStatusProjection>,
                           pub events_tx: broadcast::Sender<CoreStatusProjection>,
                           pub status_timeout: Duration, pub stop_wait: Duration }        // mod.rs:214-228
impl CoreClient {                                                                          // mod.rs:889
    pub async fn spawn(initial: EndpointHandle) -> Result<Self, ractor::SpawnErr>;         // mod.rs:891
    pub fn status(&self) -> CoreStatusProjection;                                          // mod.rs:933
    pub fn subscribe(&self) -> watch::Receiver<CoreStatusProjection>;                      // mod.rs:937
    pub fn subscribe_events(&self) -> broadcast::Receiver<CoreStatusProjection>;           // mod.rs:941
    pub async fn submit(&self, envelope: CoreCommandEnvelope) -> Result<SubmitTicket, CoreError>; // mod.rs:945
    pub async fn change_host(&self, target: EndpointHandle) -> Result<HandoffReport, CoreError>;  // mod.rs:967
    pub async fn shutdown(&self) -> Result<ShutdownReport, CoreError>;                     // mod.rs:985
}
// core/actor_v2/endpoint.rs
pub type EndpointHandle = Arc<dyn ControlEndpoint>;                                        // endpoint.rs:428
pub trait ControlEndpoint { fn host(&self) -> ExecutionHost;                               // endpoint.rs:60-79
    fn submit<'a>(&'a self, envelope: CoreCommandEnvelope) -> BoxFuture<'a, Result<OperationInfo, CoreError>>;
    fn wait_operation<'a>(&'a self, id: OperationId, timeout: Duration) -> BoxFuture<'a, Option<OperationInfo>>;
    fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>>; }
impl LocalEndpoint  { pub fn new(control: CoreControl) -> Self }                            // endpoint.rs:85,90
impl ServiceEndpoint{ pub fn new(client: nyanpasu_ipc::client::Client) -> Self }            // endpoint.rs:251,256
// core/actor_v2/service_actor.rs
pub trait ServiceHostAdapter { probe/install/uninstall/start_daemon/stop_daemon/update/endpoint } // :34-43
impl ServiceClient { pub async fn spawn(adapter: Arc<dyn ServiceHostAdapter>, restart_budget: u8)
                       -> Result<Self, ractor::SpawnErr>;                                   // :538-543
    pub fn status/subscribe/ensure_ready/install/update/uninstall/start_daemon/stop_daemon/probe/report_endpoint_down } // :580-644
// core/actor_v2/intent.rs
impl RuntimeIntentBuilder { pub fn build(core_type: CoreType, document: &serde_yaml::Mapping,
                                         expected_applied: Option<RevisionIdInfo>) -> Result<RuntimeIntent, serde_yaml::Error> }
```

runtime 侧（submodule `6717e44`，本阶段用到的全部）：

```rust
nyanpasu_core_manager::{CoreManager, CoreManagerBuilder, CoreControl, ControlOptions,
                        CoreCommand, CoreCommandEnvelope, ReconcileRequest, ConfigInput,
                        CoreSpec, InstanceOptions, ManagerOptions, CoreKind, CoreError,
                        CoreErrorKind, OperationId, DnsController, payload_digest};
CoreManager::builder(options: ManagerOptions) -> CoreManagerBuilder      // manager/mod.rs:233
CoreManagerBuilder::{readiness_probe, liveness_probe, liveness_with_readiness_probe,
                     runtime_backend, dns_controller, build}             // manager/mod.rs:194-228
CoreControl::spawn(manager: CoreManager, options: ControlOptions) -> CoreControl // control/mod.rs:358
ControlOptions::new(source_dir: Utf8PathBuf, working_dir: Utf8PathBuf)   // control/mod.rs:318
ManagerOptions { runtime_dir: Option<Utf8PathBuf>, local_ipc_policy, controller_template,
                 control_timeout, reconcile_timeout, stop_timeout, dns_timeout,
                 cancel_token, log_sink_enabled, log_max_bytes, log_max_files }  // spec.rs:80-109
                 // Default 见 spec.rs:111-132（local_ipc_policy = Disable）
CoreSpec { kind: CoreKind, binary_path: Utf8PathBuf, version: Option<String>, features: Vec<String> } // spec.rs:12
ConfigInput::Inline { bytes, expected_digest }                            // control/mod.rs:127-135
#[cfg(target_os="macos")] nyanpasu_core_manager::dns::macos::MacosDnsController::new(store_key: String) // dns.rs:130
```

### 2.5 L3 有损映射的事实

`CoreKind` = `nyanpasu_core_metadata::ClashCoreKind`（`nyanpasu-core-manager/src/kind.rs:10` 再导出），只有四个变体 `{Mihomo, ClashRust, ClashPremium, Meow}`（`nyanpasu-core-metadata/src/kind.rs:20-29`）。app 的 `ClashCore` 有六个变体（`backend/nyanpasu-config/src/application/clash_core.rs:24-37`：`ClashPremium/ClashRs/Mihomo/MihomoAlpha/ClashRsAlpha/Meow`），`From<&ClashCore> for CoreType`（同文件 `:48-71`）是**无损**的。
`endpoint.rs:392-408 app_core_kind_to_type` 从 `CoreKind` 反推 `CoreType`：alpha 通道已在 `ClashCore → CoreKind` 一步塌缩，`Meow` 干脆返回 `Internal`。所以 Service host 上永远起不了 alpha 核与 Meow。修复必须是 facade 携带 intent 自己的 `CoreType`（`RuntimeIntent.core_type` 已经有，`intent.rs:19`），而不是修 `app_core_kind_to_type`。

---

## 3. 目标形态

### 3.1 组合根新拓扑

```text
setup::setup(app)                                   // setup.rs:19
  ├─ PathResolver::from_env()                       // setup.rs:31（不变）
  ├─ RuntimePaths::from_resolver(&paths)            // setup.rs:38（不变）
  ├─ local_host::build(&paths)  ──► CoreControl     // 新，卡 S1
  ├─ LocalEndpoint::new(control) ──► EndpointHandle
  ├─ CoreClient::spawn(local_endpoint)              // 卡 S6
  ├─ OsServiceHostAdapter::new(..) ──► Arc<dyn ServiceHostAdapter>  // 新，卡 S2
  ├─ ServiceClient::spawn(adapter, RESTART_BUDGET)  // 卡 S6
  └─ NyanpasuClient::try_new_with_args(ClientSetupArgs { .., core: CoreClient, service: ServiceClient, .. })
```

`ClientSetupArgs.core` 的类型从 `Arc<dyn CoreLifecyclePort>` 换成 `CoreClient`，新增 `service: ServiceClient`；`NyanpasuClientInner` 同步换字段。**不保留** `CoreLifecyclePort`，不加 `Option` 双轨（AGENTS §11）。

### 3.2 facade 新方法面（`NyanpasuClient`）

按集成设计 §3.1 落地，签名固定如下（实施代理照抄）：

```rust
impl NyanpasuClient {
    pub async fn reconcile_core(&self) -> Result<ReconcileReport, CoreError>;
    pub async fn update_core(&self, core: nyanpasu_config::application::ClashCore) -> Result<ReconcileReport, CoreError>;
    pub async fn stop_core(&self) -> Result<StopReport, CoreError>;
    pub async fn recover_core(&self) -> Result<RecoverReport, CoreError>;
    pub async fn change_execution_host(&self, host: ExecutionHost) -> Result<HandoffReport, CoreError>;
    pub fn core_status(&self) -> CoreStatusProjection;                       // watch 同步读，零 mailbox
    pub fn subscribe_core_events(&self) -> broadcast::Receiver<CoreStatusProjection>;
    pub fn service_status(&self) -> ServiceHostStatus;
    pub async fn shutdown_core(&self) -> ShutdownReport;                     // 幂等 shared-future
}
```

- `reconcile_core` 内部：`regenerate_runtime_inner`（现有 enhance 管线，纯计算）→ 写 product 文件（UI 读它）→ `RuntimeIntentBuilder::build(core_type, &document, expected_applied)` → 组 `CoreCommandEnvelope{ operation_id: OperationId::generate(), command: CoreCommand::Reconcile(Box::new(ReconcileRequest{ core: CoreSpec{ kind, binary_path: find_binary_path(&core_type)?, version: None, features: vec![] }, config: ConfigInput::Inline{ bytes, expected_digest: Some(digest) }, options: InstanceOptions::default(), expected_applied })) }` → `core.submit(..)` → `wait_operation`。
- `expected_applied` 的唯一来源是 `self.core_status().snapshot.and_then(|s| s.revision)`（`endpoint.rs:52`），**不再**从 app 侧 `RuntimeLifecycleStore.applied` 取。
- `update_core(core)`：提交 typed/legacy 的 `clash_core` 选择（commit-first），再 `reconcile_core()`。**没有**专用换核事务、没有 app 侧回滚补偿——回滚归 orchestrator（控制面设计 §11 ⑨）。
- `shutdown_core`：`OnceCell<Shared<BoxFuture<ShutdownReport>>>` 形态的 shared-future latch（集成设计 §7）；第二次调用 await 同一 future，不经 `Notify`。

`ReconcileReport` / `StopReport` / `RecoverReport` 定义在新文件 `core/actor_v2/facade.rs`，只包裹 `OperationOutputInfo` + `CoreStatusProjection`，不新增第二份真相。

### 3.3 命令映射表（Tauri 命令 → v2 替代）

`#[tauri::command]` 只存在于 `backend/tauri/src/ipc.rs`（全仓 grep `#[tauri::command]` 无第二个文件）；注册表在 `specta_export.rs:11-124` `collect_commands!`，挂载于 `lib.rs:236`。

| #     | 命令                                                                             | 位置                                                 | 现调用                                                    | 换线后                                                      |
| ----- | -------------------------------------------------------------------------------- | ---------------------------------------------------- | --------------------------------------------------------- | ----------------------------------------------------------- |
| 1     | `get_core_status`                                                                | `ipc.rs:399`                                         | `CoreManager::global().status()` `:403`                   | `client.core_status()` → 新 DTO（§8 破坏性）                |
| 2     | `restart_sidecar`                                                                | `ipc.rs:502`                                         | `CoreManager::global().run_core()` `:503`                 | `client.reconcile_core()`                                   |
| 3     | `change_clash_core`                                                              | `ipc.rs:482`                                         | `client.change_core(..)` `:493` → `rebuild.rs:281`        | `client.update_core(core)`                                  |
| 4     | `patch_clash_config`                                                             | `ipc.rs:439`                                         | `client.patch_running_config(..)` `:457` → `mod.rs:1350`  | commit clash config → `client.reconcile_core()`             |
| 5     | `enhance_profiles`                                                               | `ipc.rs:139`                                         | `client.rebuild_running_config()` `:140` → `mod.rs:1412`  | `client.reconcile_core()`                                   |
| 6     | `update_core`                                                                    | `ipc.rs:639`                                         | `UpdaterManager` → `core/updater/instance.rs:205/216/279` | 下载后 `client.reconcile_core()`（updater 不再持 lease）    |
| 7     | `cleanup_processes`                                                              | `ipc.rs:1029`                                        | `help::cleanup_processes` → 两处 `stop_core`              | `client.shutdown_core()`（单一路径）                        |
| 8     | `restart_application`                                                            | `ipc.rs:855`                                         | 同上                                                      | 同上                                                        |
| 9     | `quit_application`                                                               | `ipc.rs:1317`                                        | `app_handle.exit(0)` → `lib.rs:351`                       | 不变（退出路径在 S8 内换）                                  |
| 10    | `set_custom_app_dir`(win)                                                        | `ipc.rs:816`                                         | `quit_application` `:842`                                 | 不变                                                        |
| 11    | `patch_verge_config`                                                             | `ipc.rs:475`                                         | → `feat::patch_verge` `feat.rs:336`                       | 只换 `feat.rs` 内的核心腿（`:359/361/377/378/385/392/398`） |
| 12    | `service::status_service`                                                        | `ipc.rs:923`                                         | `control::status()` + `ServiceCompat::classify`           | `client.service_status()` → `ServiceHostStatus` DTO         |
| 13    | `service::install_service`                                                       | `ipc.rs:937`                                         | `control::install_service()`                              | `service.install()`                                         |
| 14    | `service::uninstall_service`                                                     | `ipc.rs:944`                                         | `control::uninstall_service()`                            | facade `uninstall_service()`（§5 卡 S5 双层守卫）           |
| 15    | `service::start_service`                                                         | `ipc.rs:951`                                         | `control::start_service()` + `run_core()` `:960`          | `service.start_daemon()`；起核归 §6.4 编排                  |
| 16    | `service::stop_service`                                                          | `ipc.rs:968`                                         | `control::stop_service()` + `run_core()` `:977`           | `service.stop_daemon()`；同上                               |
| 17    | `service::restart_service`                                                       | `ipc.rs:985`                                         | `control::restart_service()` + `run_core()` `:994`        | `service.stop_daemon()` + `start_daemon()`                  |
| 18    | `get_service_install_prompt`                                                     | `ipc.rs:1014`                                        | `control::get_service_install_args()`                     | 不变（纯字符串拼装，`control.rs:10` 保留）                  |
| 19–30 | profile 变更 12 命令（`ipc.rs:146/193/204/214/223/233/242/251/260/269/279/289`） | `after_commit` → `mod.rs:852 rebuild_running_config` | 落到 `reconcile_core()`，逐条不改命令签名                 |

命令自身一律收缩为 AGENTS §12 的薄适配器：解析 DTO → 调 `NyanpasuClient` → 映射错误。

---

## 4. 跨仓顺序接口（硬约束）

| 接口 | 内容                                                                                                                                      | 约束                                                                                                                                                                                                                                 |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| I-1  | runtime 仓提升 `backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml` 的 `version`（现为 `2.0.0-rc.1`，见该文件 `:3`）并打 tag 发 release | 必须**先于**本仓 S13。`scripts/check.ts:679-692 getNyanpasuServiceVersion()` 按该字段推导下载 URL，不提升就会拉到缺 `/v2/core/*` 的 rc.1 daemon（L10 的活陷阱）                                                                      |
| I-2  | runtime 仓删除 v1 endpoint                                                                                                                | 必须**后于**本仓 S11（S11 删掉 `core/clash/core.rs`，即本仓最后 5 个 v1 调用点）                                                                                                                                                     |
| I-3  | 本仓 gitlink 推进（S13）                                                                                                                  | 必须在 I-1 之后；若 I-2 也已落地，S13 一次推到含两者的 commit。**S13 必须排在任何依赖新 runtime API 的提交之前**——本计划 S1–S12 全部只用 `6717e44` 已有的 API，因此 S13 排在末尾是安全的，实施代理不得为了"顺手"把新 API 用进 S1–S12 |
| I-4  | compat 门最低版本常量的取值                                                                                                               | 取 I-1 提升后的版本；I-1 未定版本号前 S12 不可实施（见 §11 待裁定 D-3）                                                                                                                                                              |
| I-5  | 前端 bindings 消费                                                                                                                        | 本仓 S8/S9 改动命令与事件后 `frontend/interface/src/ipc/bindings.ts` 会被重新生成；前端计划据 §8 表接手                                                                                                                              |

---

## 5. 任务卡

顺序即提交顺序。每卡执行完必须先跑本卡的「验证」，绿了再提交。

### S1 — Local host 构造

| 项        | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 新文件    | `backend/tauri/src/core/actor_v2/local_host.rs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| 内容      | `pub async fn build(paths: &crate::utils::path::PathResolver) -> anyhow::Result<CoreControl>`（`PathResolver` 定义在 `utils/path.rs:40`，`setup.rs:14` 与 `client/mod.rs:40` 已按此路径导入）：<br>① `ManagerOptions { runtime_dir: Some(<app_config_dir>/runtime/control), local_ipc_policy: <默认 Disable，保持现状>, ..Default::default() }`；<br>② `CoreManager::builder(options)`，macOS 上 `.dns_controller(Arc::new(MacosDnsController::new("State:/Network/Service/nyanpasu-dns/DNS".into())))`；<br>③ `.build().await?`；<br>④ `CoreControl::spawn(manager, ControlOptions::new(source_dir, working_dir))`，`source_dir` = `<app_config_dir>/runtime/staging`，`working_dir` = `dirs::app_data_dir()`（geo 资产所在，与 `core.rs:105-107` 现行为一致）。<br>另导出 `pub fn core_spec(core: &ClashCore) -> anyhow::Result<CoreSpec>`，`binary_path` 走 `crate::core::clash::find_binary_path`（S11 后的新路径，本卡先用 `core::clash::core::find_binary_path`）。 |
| 声明      | `core/actor_v2/mod.rs:32-34` 的 `pub mod` 块加 `pub mod local_host;`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 依赖      | 无新增 crate（`nyanpasu-core-manager` 已在 `backend/tauri/Cargo.toml:28`）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| 验证      | `cd backend && cargo build -p clash-nyanpasu --lib`；新增 `#[tokio::test] the_local_host_spawns_under_a_temp_root`（用 `tempfile::TempDir` 造 `PathResolver`，断言 `control.status()` 可读且 `executor_is_closed() == false`）+ `core_spec_maps_every_clash_core_variant`（六个 `ClashCore` 变体全覆盖，断言 `binary_path` 非空）。测试**不得**访问真实用户目录（roadmap §1.6）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| fake/真核 | 纯 fake：不起真核（只 spawn executor）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |

### S2 — `OsServiceHostAdapter`（生产适配器）

| 项 | 内容 |
| --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --- | --------------------------------------------------- |
| 新文件 | `backend/tauri/src/core/actor_v2/service_host_adapter.rs` |
| 内容 | `pub struct OsServiceHostAdapter;` 实现 `ServiceHostAdapter`（`service_actor.rs:34-43`）：<br>`probe` → `crate::core::service::control::status()`（`control.rs:326`）；`install/uninstall/start_daemon/stop_daemon/update` → `control.rs:54/145/184/230/102`；`endpoint()` → `Arc::new(ServiceEndpoint::new(nyanpasu_ipc::client::Client::service_default().clone()))`。<br>错误一律 `map_err( | e   | e.to*string())`（trait 要求 `Result<*, String>`）。 |
| 同卡改动 | 摘除 `control.rs` 对 `ipc.rs` statics 的副作用：删 `control.rs:96-97`、`:224-225`、`:319-320`（`HEALTH_CHECK_RUNNING` + `spawn_health_check`）与 `control.rs:175`、`:270`（`KILL_FLAG`）。这些语义由 `ServiceActor` 的观察循环与相位机替代（`service_actor.rs:143/168-214`） |
| 不做 | 不动 `control.rs` 的 OS 机制本体（SCM / launchd / systemd 调用），它就是 AGENTS §8 说的边界适配器 |
| 验证 | `cargo build -p clash-nyanpasu --lib`；`cargo test -p clash-nyanpasu --lib`（`core::service::*` 14 个测试中依赖被删 statics 的必须同步删除/改写，其余保持绿）。**本卡无单元测试**——纯 OS 边界透传，可测的部分（compat 分类、相位判定）已在 `service_actor.rs` 的 fake 测试里；这一点如实记录在文件头 doc 注释 |
| fake/真核 | 不需要真 daemon；行为验证留到 §7 冒烟 |

### S3 — L3：facade 携带原 `CoreType`

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 改动 | `core/actor_v2/endpoint.rs`：新增 `pub struct CoreSubmission { pub envelope: CoreCommandEnvelope, pub core_type: Option<nyanpasu_utils::core::CoreType> }`；`ControlEndpoint::submit` 的参数由 `CoreCommandEnvelope` 改为 `CoreSubmission`（`endpoint.rs:65-68`）。`LocalEndpoint::submit` 忽略 `core_type`（它用 `CoreSpec.binary_path`）；`ServiceEndpoint::submit` 把 `core_type` 传进 `wire_submit_request`，`Some` 时直接用，`None` 时回落 `app_core_kind_to_type`（`endpoint.rs:392`）。`app_core_kind_to_type` **保留**为回落，doc 注释改写为"仅在调用方未携带 `CoreType` 时使用"。 |
| 连带 | `core/actor_v2/mod.rs:176-210` 的 `CoreActorMessage::Submit` 载荷改为 `CoreSubmission`；`CoreClient::submit`（`mod.rs:945`）签名改为 `pub async fn submit(&self, submission: CoreSubmission) -> Result<SubmitTicket, CoreError>`；`stop_and_confirm` 等内部构造点补 `core_type: None`                                                                                                                                                                                                                                                                                                      |
| 验证 | `cargo test -p clash-nyanpasu --lib`；新增回归测试 `an_alpha_core_reaches_the_service_wire_intact`：用 fake `ControlEndpoint` 捕获 `CoreSubmission`，断言 `core_type == Some(CoreType::Clash(ClashCoreType::MihomoAlpha))`，并断言在旧路径（`core_type: None` + `CoreKind::Mihomo`）下会塌缩为 `Mihomo` —— 该测试在改动前必须失败                                                                                                                                                                                                                                                          |
| fake | 纯 fake                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |

### S4 — L13：终态 `ShutDown` slot

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 改动 | `core/actor_v2/mod.rs:230-246` 的 `EndpointSlot` 增加变体 `ShutDown { report: ShutdownReport }`；`CoreActorMessage::Shutdown` 处理（`mod.rs:642-676`）改为：结算后把 slot 置为 `ShutDown{report}`、**不再** `myself.stop(..)`；第二次 `Shutdown` 直接回放已存的 report。`ShutdownReport` 需 `Clone`（`mod.rs:143-154`，其 `stop: Result<Option<OperationInfo>, CoreError>` 两侧均可 Clone）。<br>`Submit` / `ChangeHost` 在 `ShutDown` 下返回 `CoreErrorKind::ShuttingDown`（不可重试）。<br>`project()`（`mod.rs:283-306`）为 `ShutDown` 增一条投影分支，`connectivity` 复用 `Degraded{desired, reason:"shut down"}` 还是新增变体，取后者：`EndpointConnectivity::ShutDown`（`mod.rs:119-132`），因为把关停投影成 degraded 又是一次合成状态（I-R3）。 |
| 验证 | 新增 `a_second_shutdown_replays_the_same_report`（改动前必失败：现在第二次会拿到 `mod.rs:1019` 的 "the core router is gone"）；`a_submit_after_shutdown_is_refused_as_shutting_down`。`cargo test -p clash-nyanpasu --lib`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| fake | 纯 fake                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

### S5 — L9：uninstall 结构性互斥

| 项       | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 问题     | `service_actor.rs:328-400` 的 uninstall 守卫读一次 probe 再发卸载；探测与卸载之间被 daemon 自己的 control plane 放行的一次 `Reconcile` 不被排除（L9）                                                                                                                                                                                                                                                                                                                          |
| 改动     | 在**同一个 mailbox turn 内**改为三段，全部有界（`state.bounded(..)`）：<br>① 现有守卫 probe（保留 F5 的 fail-closed 语义不变）；<br>② `adapter.stop_daemon()`——daemon 停了就再也接不到 `Reconcile`，这才是结构性排除，而不是缩小窗口；<br>③ 复探，必须读到 `ServiceStatus::Stopped \| NotInstalled` 才继续，否则 `Err(kind=AlreadyRunning)` 并把相位复位；<br>④ `adapter.uninstall()`。<br>相位序列：`Ready/DaemonStopped → Uninstalling → NotInstalled`（失败回 `Probing`）。 |
| 第二层   | facade 侧：`NyanpasuClient` 增私有 `host_transition: tokio::sync::Mutex<()>`，`change_execution_host` 与 `uninstall_service` 共享它，且 `uninstall_service` 进入前断言 `self.core_status().host != ExecutionHost::Service`，否则 `Err(kind=OperationConflict)`。两层都过才发提权命令（集成设计 §6.4）                                                                                                                                                                          |
| 诚实边界 | 本互斥只覆盖**本 app 进程**。第三方（另一实例 / CLI）直接向 daemon 提交 `Reconcile` 不在排除范围内——写进 `service_actor.rs` doc 注释，不假装解决                                                                                                                                                                                                                                                                                                                               |
| 验证     | 新增 `uninstall_stops_the_daemon_before_removing_it`（fake adapter 记录调用序列，断言 `stop_daemon` 在 `uninstall` 之前且中间有一次 `probe`）；`uninstall_refuses_when_the_daemon_will_not_stop`（fake 的复探仍报 Running → `AlreadyRunning`，且 `uninstall` 从未被调用）。两者在改动前必失败                                                                                                                                                                                  |
| fake     | 纯 fake（`service_actor.rs` 已有 fake adapter 基建）                                                                                                                                                                                                                                                                                                                                                                                                                           |

### S6 — facade 编排（L4）+ 组合根 spawn（并存，不换线）

| 项     | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 新文件 | `backend/tauri/src/core/actor_v2/facade.rs`：`ReconcileReport` / `StopReport` / `RecoverReport` 定义 + `CoreFacade { core: CoreClient, service: ServiceClient, shutdown: OnceCell<Shared<..>> }`                                                                                                                                                                                                                                                                                            |
| 改动   | ① `client/mod.rs:75-84` `ClientSetupArgs` 增字段 `pub core_v2: crate::core::actor_v2::CoreClient` 与 `pub service: crate::core::actor_v2::service_actor::ServiceClient`；`NyanpasuClientInner`（`client/mod.rs:225-251`）同步增字段；`with_parts`（`:335-350`）同步。**`core: Arc<dyn CoreLifecyclePort>` 本卡保留**。<br>② `NyanpasuClient` 上实现 §3.2 的九个方法。<br>③ `setup.rs:42-55` 之前构造 Local host / adapter / 两个 client 并传入（`block_on`，与 `client/mod.rs:273` 同款）。 |
| 连带   | 每个构造 `ClientSetupArgs` 的测试都要补两个新字段：`bridge/verge.rs:1081` 与 `client/mod.rs` 内的测试 helper（`grep -rn 'ClientSetupArgs' backend/tauri/src` 取全集）。新增测试用 fake `ControlEndpoint` 起 `CoreClient::spawn`、fake `ServiceHostAdapter` 起 `ServiceClient::spawn`                                                                                                                                                                                                        |
| 说明   | 本卡结束时新旧两条核心路径并存，但**没有任何生产代码调用新路径**（只有测试调）。这不是 AGENTS §11 意义上的兼容层：并存窗口仅存在于本分支的 S6→S11 之间，随 S11 一同合入，永不发版，因此不加 `TODO(actor-migration)` 标记。实施代理不得在此窗口外扩大并存范围                                                                                                                                                                                                                                |
| 验证   | `cargo build -p clash-nyanpasu --lib`；`cargo test -p clash-nyanpasu --lib`。新增 facade 测试：`reconcile_builds_an_inline_intent_with_the_status_revision_as_cas_token`、`a_second_shutdown_awaits_the_same_future`（两个并发 `shutdown_core()` 只触发一次 `CoreClient::shutdown`）、`change_execution_host_to_service_ensures_ready_first`（fake ServiceHostAdapter 断言 `EnsureReady` 在 `ChangeHost` 之前）                                                                             |
| fake   | 纯 fake（facade 测试用 fake `ControlEndpoint` + fake `ServiceHostAdapter`）                                                                                                                                                                                                                                                                                                                                                                                                                 |

### S7 — 运行时变更路径迁到 facade

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 改动 | 把下列全部改为 `reconcile_core()` / `stop_core()`：<br>`client/mod.rs:1228 promote_existing_runtime_product`、`:1275 start_promoted_runtime`、`:1350 patch_running_config`、`:1412 rebuild_running_config`、`:1428 regenerate_runtime`；<br>`client/rebuild.rs:229 regenerate_runtime_for_legacy`、`:254 regenerate_and_apply_for_legacy`、`:267 regenerate_and_restart_for_legacy`、`:281 change_core`、`:405 promote_default_runtime_config`。 |
| 删除 | 同卡删掉因此失去理由的 app 侧补偿机制：`client/mod.rs:1287 restore_applied_after_patch_failure`、`client/runtime.rs` 的 `compensation_for` / `PatchCompensationPlan` / `RuntimeTransactionSnapshot`、`client/core_bridge.rs restore_product`、`rebuild.rs:281-395 change_core` 的整段回滚编排。**理由**：控制面设计 §11 ⑨ 的 rollback 与 §5.3 ③ 的 CAS 是同一职责的唯一实现点，app 侧再留一份就是第二个事务所有者（审计 P0-2 的同构病）          |
| 保留 | `RuntimeSnapshot` / `promoted` / product 文件写入（`get_runtime_config` `ipc.rs:343`、`get_runtime_yaml` `:361`、`get_runtime_exists` `:375`、`get_postprocessing_output` `:386` 仍读它）；`RuntimeLifecycleStore.applied` 字段与其发布路径删除，`expected_applied` 改读 `core_status()`                                                                                                                                                         |
| 验证 | `cargo test -p clash-nyanpasu --lib`；`client/mod.rs`（31）+`client/rebuild.rs`（16）+`client/runtime.rs`（13）共 60 个测试中，依赖 `MockCoreLifecycleLease` 的全部改写为 fake `ControlEndpoint`；改写后计数不得下降（下降即说明覆盖被悄悄丢掉，必须在提交正文说明）                                                                                                                                                                             |
| fake | 纯 fake                                                                                                                                                                                                                                                                                                                                                                                                                                          |

### S8 — 核心命令 / feat / 启动 / 退出换线

| 项     | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 命令   | §3.3 表 #1–#8、#11：`ipc.rs:399/482/439/139/502/639/855/1029/1317`。`get_core_status` 返回类型换为新 DTO（§8）                                                                                                                                                                                                                                                                                                                                                                       |
| feat   | `feat.rs:58 restart_clash_core` → `client.reconcile_core()`；`feat.rs:292` / `:385` 的 `status()` → `client.core_status()`；`feat.rs:392` 的 macOS `change_default_network_dns` **整段删除**（DNS 归 host 的 `MacosDnsController`，集成设计 §8：app 进程零 DNS 职责）；`feat.rs:359/361/377/378` 的 `get_ipc_state()` → `client.core_status().host == ExecutionHost::Service`。`patch_verge` 的形状、`patch_verge_entrypoint`（`feat.rs:111`）、`LegacyVergeBridge` 一律不动（PR-7） |
| 启动   | `utils/resolve.rs:178/181-192/200-204` 三段合并为：写 product → `client.reconcile_core()`。`resolve.rs:152 init::init_service()` 改为 `service.probe()` 的启动期对账（`update_service` 的 UAC 语义由 `ServiceActor` 启动对账保留，见 `service_actor.rs:288-291`）                                                                                                                                                                                                                    |
| 退出   | `lib.rs:350-352` 不变；`utils/help.rs:261-269` 里两处 `stop_core` 合并为一次 `client.shutdown_core()`；`utils/resolve.rs:288` 的重复停核**删除**（保留 `:287` 的 `reset_sysproxy`）；`client/mod.rs:402 shutdown()` 的 doc（`:396-399`）改写——它现在确实停核了                                                                                                                                                                                                                       |
| 更新器 | `core/updater/instance.rs:201/205/216/279`：`replace_core` 不再 `begin_lifecycle()`，改为下载完成后 `client.reconcile_core()`（新 artifact 由 `find_binary_path` 解析，`CoreSpec.binary_path` 自然指向新二进制）                                                                                                                                                                                                                                                                     |
| 验证   | `cargo test -p clash-nyanpasu --lib`；`cargo build -p fake-core` 后 `process_core_bridge::s09_*` 仍绿（本卡尚未删它）；specta bindings 重新生成后 `git diff --exit-code frontend/interface/src/ipc/bindings.ts` **预期非零**——把重新生成的 bindings 一并纳入本提交                                                                                                                                                                                                                   |
| fake   | 纯 fake                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |

### S9 — service-mode 命令 + §6.4 编排

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 命令 | §3.3 表 #12–#17（`ipc.rs:923/937/944/951/968/985`）。`start_service`/`stop_service`/`restart_service` 里的隐式 `run_core()`（`ipc.rs:960/977/994`）**删除**——daemon 生命周期与核生命周期是两件事（集成设计 §6.1 边界表）                                                                                                                                                                                                                                                                                          |
| 编排 | `enable_service_mode` 的 true/false 两向按集成设计 §6.4 落地在 facade：<br>**true**：commit（typed patch，commit-first）→ `service.ensure_ready()` → `core.change_host(service_endpoint)` → 失败即 `CommittedDegraded`，**不回滚 state**；<br>**false**：`core.change_host(local_endpoint)`（handoff 内含 Service 侧 Stop + StopProof，此刻 daemon 必须还活着）→ 按需 `service.stop_daemon()`。<br>入口：`feat.rs:358-366`（现为 `regenerate_and_restart_for_legacy`）改为调 facade 的 `set_execution_host(bool)` |
| 状态 | `status_service` 返回 `ServiceHostStatus`（`service_actor.rs:70-74`：`phase` / `compat` / `restart_attempts`）+ 保留原 `StatusInfo` 镜像字段。`ServicePhase`（`service_actor.rs:46-65`）需加 `specta::Type` derive                                                                                                                                                                                                                                                                                                |
| 验证 | `cargo test -p clash-nyanpasu --lib`；新增 facade 编排测试 `enabling_service_mode_commits_before_ensure_ready`、`disabling_service_mode_hands_off_before_stopping_the_daemon`（fake 记录调用序）；bindings 重新生成并纳入提交                                                                                                                                                                                                                                                                                     |
| fake | 纯 fake                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |

### S10 — 吸收 `core/service/ipc.rs`

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 删除 | `core/service/ipc.rs` **整文件**（302 行）：三 statics（`:28/29/30`）、`IpcState`（`:17`）、`get_ipc_state`（`:32`）、`set_ipc_state`（`:36`）、`dispatch_*`（`:41/58`）、`on_ipc_state_changed`（`:74`，隐式起核触发器）、`spawn_health_check`（`:100`）、`health_check`（`:153`）、`next_incompatible_warning_state`（`:137`）、`target_ipc_state`（`:195`）及其测试块（`:214-302`）                                       |
| 去向 | 逐项对照（集成设计 §6.5）：观察循环 → `ServiceActor` 单一循环；`IPC_STATE` → `ServiceHostStatus.phase`（`service_actor.rs:70`）；fail-closed 分类（`target_ipc_state`）→ `ServiceActor::classify_probe`（`service_actor.rs:168-190`）；告警锁存（`next_incompatible_warning_state`）→ F9 的相位锁存；`KILL_FLAG` → 相位机（`Uninstalling`/`DaemonStopped`）；`HEALTH_CHECK_RUNNING` 忙等 → `ensure_ready()` 的 request/reply |
| 连带 | `core/service/mod.rs:18-37 init_service()` 删除；`core/service/mod.rs:9 pub mod ipc;` 删除；`utils/init/mod.rs:232-280 init_service()` 删除（`:259 update_service()` 的产品语义由 `ServiceActor` 启动对账承接）；`utils/resolve.rs:152` 的调用点删除                                                                                                                                                                         |
| 保留 | `core/service/compat.rs` 全部（`ServiceCompat` 是版本门的唯一实现点，`service_actor.rs:26` 已在消费）；`core/service/control.rs` 的 8 个 OS 机制函数                                                                                                                                                                                                                                                                         |
| 验证 | `cargo build -p clash-nyanpasu --lib`；`cargo test -p clash-nyanpasu --lib`。`core::service::*` 原 14 个测试里 `ipc.rs` 的那部分随文件删除，`compat.rs` 的必须全绿；提交正文如实写明删了几个测试及其覆盖去向                                                                                                                                                                                                                 |
| fake | 纯 fake                                                                                                                                                                                                                                                                                                                                                                                                                      |

### S11 — 删除被取代的生命周期形态

| 项   | 内容                                                                                                                                                                                                                                                                           |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 删除 | 见 §6 删除集全表                                                                                                                                                                                                                                                               |
| 迁移 | `find_binary_path`（`core.rs:725-742`）移入 `core/clash/mod.rs`；`core/mod.rs:18 pub use self::clash::core::*;` 改为 `pub use self::clash::find_binary_path;`；更新 `core/manager.rs:8`、`utils/dirs.rs:345` 的路径引用                                                        |
| 验证 | `cargo build -p clash-nyanpasu --lib` + `cargo test -p clash-nyanpasu --lib`；`grep -rn 'CoreManager::global()' backend/tauri/src` 必须**零命中**；`grep -rn 'nyanpasu_ipc::client::shortcuts::Client::service_default' backend/tauri/src` 只剩 `service_host_adapter.rs` 一处 |
| fake | 纯 fake                                                                                                                                                                                                                                                                        |

### S12 — compat 门收紧为最低版本

| 项   | 内容                                                                                                                                                                                                                                                                                                                                         |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 问题 | `core/service/compat.rs:11 REQUIRED_SERVICE_MAJOR = 2` 只比 major。换线后 `/v2/core/*` 是唯一路径，而一台残留的 `2.0.0-rc.1` daemon（major == 2）会被判为 `Compatible` 却没有该路由——门禁不再 fail-closed                                                                                                                                    |
| 改动 | 增 `pub const REQUIRED_SERVICE_MIN: semver::Version`（取值见 I-4）；`ServiceCompat::classify`（`compat.rs:31`）在 major 相等后追加 `version >= REQUIRED_SERVICE_MIN` 判据，不满足即 `Incompatible`。`Incompatible` 变体增 `required_min: String` 字段供 UI 展示                                                                              |
| 验证 | 新增 `an_rc_daemon_with_the_right_major_is_still_incompatible`（用 `compat.rs:73` 的 `STATUS_V2_0_0_RC1_FIXTURE`，该 fixture 现被 `compat.rs:127/141/159` 三个用例断言为 `Compatible`，本卡必须一并改判——改动前新测试必失败）；既有 `STATUS_V1_4_5_FIXTURE` 用例保持 `Incompatible`。`cargo test -p clash-nyanpasu --lib`；bindings 重新生成 |
| 前置 | **I-1 完成且版本号已定**，否则本卡不可实施                                                                                                                                                                                                                                                                                                   |

### S13 — gitlink 推进

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                                 |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 改动 | `git -C backend/nyanpasu-runtime fetch && git -C backend/nyanpasu-runtime checkout <目标 commit>`，然后 `git add backend/nyanpasu-runtime`（**只 add gitlink**）；同步修正 `backend/Cargo.toml` 里关于 pin 的注释（L10 的 "Leader-found" 项已改过一次口径，本次再核对）                                                                                                                              |
| 验证 | 干净 checkout 可构建：`git worktree add <tmp> refactor/core-actor-switch && cd <tmp> && git submodule update --init --recursive && cargo build -p clash-nyanpasu --lib`（按 AGENTS §17 建 worktree，符号链接 sidecar/resources）；`pnpm prepare:check` 拉到的 daemon 版本 == I-1 的版本；`cd backend/nyanpasu-runtime && cargo test --workspace --all-features --config build.rustc-wrapper=''` 全绿 |
| 顺序 | 必须晚于 S11（本仓最后的 v1 调用点在 S11 删除，见 I-2）                                                                                                                                                                                                                                                                                                                                              |

### S14 — ledger 快照 + 审计文档

| 项   | 内容                                                                                                                                                                                                                                                                                                                                                                                       |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 步骤 | ① `deno run -A scripts/architecture-ledger.ts --mode=report` 记录换线后的真实计数；② `deno run -A scripts/architecture-ledger.ts --write-snapshot --snapshot scripts/architecture-ledger.snapshot.json`；③ `pnpm lint:architecture-ledger` 必须通过；④ `pnpm test:architecture-ledger` 必须通过                                                                                            |
| 纪律 | **不得预测数字**。当前快照为 `config_calls` 120 / `service_globals` 80（其中 `CoreManager::global()` 18）/ `migration_markers` 19（`scripts/architecture-ledger.snapshot.json`）。#5070 已修好词法器（注释、转义、生命周期撇号、原始字符串 `r`/`br`/`cr`、嵌套块注释），旧的 116/74 伪影不会再出现，所以**新计数就是真计数**；提交正文里逐条写明每一项减少的来源，不得用"清理"一词一笔带过 |
| 文档 | `docs/audit/2026-08-13-v2-implementation-audit-guide.md`：L3/L4/L9/L13 标记为已闭并指向本分支；L10 标记为由 S12+S13 闭合；§2 的测试计数更新为本轮实测值                                                                                                                                                                                                                                    |
| 验证 | 上述四条命令全绿；`git diff --exit-code frontend/interface/src/ipc/bindings.ts` 为零（前面的卡已把 bindings 纳入各自提交）                                                                                                                                                                                                                                                                 |

---

## 6. 删除集（S11 一卡内完成，共约 3.4k 行）

| 文件 / 符号                                                                                                             | 位置                   | 行数 | 处置                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ----------------------------------------------------------------------------------------------------------------------- | ---------------------- | ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core/clash/core.rs` 整文件（`find_binary_path` 迁出后）                                                                | `core.rs:1-724`        | ~724 | 删。`RunType`/`Instance`/`CoreLifecycleLease`/`CoreManager` 全在内                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `client/core_bridge.rs` 整文件                                                                                          | `core_bridge.rs:1-242` | 242  | 删。`CoreLifecyclePort`(`:44`) / `CoreLifecycleLease`(`:55`) / `LegacyCoreBridge` / `CoreStatusSnapshot`(`:36`) / `restore_product` 随核心生命周期一并消失；`RunningConfigPatchPort`(`:19`) + `LegacyRunningConfigPatchBridge`(`:25`) 在 D-2 取默认（commit-first）时同步失去唯一消费者（`client/mod.rs:1366`），连同 `ClientSetupArgs.clash_patch`(`:82`)、`NyanpasuClientInner.clash_patch`(`:236`)、`clash_patch_gate`(`:239`) 与 `client/mod.rs:266-268` 的 `TODO(actor-migration)` 一起删。**`core/clash/api.rs:150 patch_configs` 本体保留**——`feat.rs:79 change_clash_mode` 直接用它，那条路属 PR-7。若 D-2 被裁定为保留 API-first patch，则本行只删前五个符号 |
| `client/process_core_bridge.rs` 整文件                                                                                  | 全文件                 | 1570 | 删（是被删端口的 test-only 实现）。覆盖去向见 §11 D-1                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `client/mod.rs:8 mod process_core_bridge;`                                                                              | 1 行                   | 1    | 删                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `core/service/ipc.rs` 整文件                                                                                            | 全文件                 | 302  | 删（S10）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `core/service/mod.rs:9,18-37`                                                                                           | ~21                    | 21   | 删 `pub mod ipc;` + `init_service()`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `utils/init/mod.rs:232-280 init_service()`                                                                              | ~49                    | 49   | 删（S10）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `client/mod.rs` 补偿路径（`restore_applied_after_patch_failure` 等）                                                    | `:1287-1345`           | ~59  | 删（S7）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `client/runtime.rs` 的 `compensation_for` / `PatchCompensationPlan` / `RuntimeTransactionSnapshot` / `applied` 发布路径 | 分散                   | ~180 | 删（S7）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `client/rebuild.rs:281-395 change_core` 回滚编排                                                                        | ~115                   | ~115 | 删（S7），由 `update_core` 取代                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `utils/resolve.rs:288` 重复停核                                                                                         | 1 行                   | 1    | 删（S8）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `feat.rs:392` macOS `change_default_network_dns` 调用                                                                   | ~7                     | 7    | 删（S8）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `core/updater/instance.rs:205/216/279` 的 lease 用法                                                                    | ~20                    | 20   | 改写（S8）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

审计 §4 删除表逐条对账（该表以从未合入的 `refactor/core-manager-actor` 为基线，本仓对应物）：

| 审计删除项                                                                        | 本仓对应物                                                                                          | 处置                                                |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `OperationGate` / `Acquire`-`ReleaseOperation` / `CoreOperationGuard`             | 本仓无（未合入）。同构物 = `CoreManager.lifecycle_lock` + `CoreLifecycleLease`（`core.rs:389-425`） | S11 删                                              |
| `CoreLifecyclePort`-`Lease`                                                       | `client/core_bridge.rs:44/55`                                                                       | S11 删                                              |
| GUI `CoreActor` 生命周期所有权                                                    | `CoreManager`（`core.rs:425`）                                                                      | S11 删                                              |
| `CoreBackend::{Local,Service}` / `BackendSlot` / `replace_backend` / `SetBackend` | 同构物 = `Instance::{Child,Service}`（`core.rs:84-95`）+ `RunType`（`core.rs:40`）                  | S11 删                                              |
| `running` shadow / `CoreStatusView`+`FaithfulLifecycle`                           | 同构物 = `CoreManager.instance: Mutex<Option<Arc<Instance>>>`（`core.rs:427`）                      | S11 删                                              |
| `RefreshStatus`/`RefreshHint`/`hint_pending`                                      | 本仓无                                                                                              | 无需动作                                            |
| `PublishPromoted`/`PublishApplied`                                                | `client/mod.rs` 的 `publish_promoted`/`publish_applied`                                             | `applied` 侧 S7 删；`promoted` 保留（product 产物） |
| transport retry（5×250ms 盲重试）                                                 | 本仓无（在 runtime 侧，已由 PR-A 收口）                                                             | 无需动作                                            |
| legacy DNS singleton                                                              | `CoreManager.previous_dns`（`core.rs:429`）+ `change_default_network_dns`（`core.rs:667`）          | S11 删                                              |
| `RunType::default()`                                                              | `core.rs:64-82`                                                                                     | S11 删                                              |
| S1–S4 接缝 / 旧 wire 兼容桥                                                       | `core.rs:270/299/329/366/703` 五处 v1 `/core/*`                                                     | S11 删                                              |

审计 §4 保留表在本仓的落点：`CoreErrorKind` / epoch-revision-apply 分类-rollback-quarantine / `stop_and_confirm_dead` / fake-core 基建 / Desired-Applied 分离 —— 全部已在 submodule 内，本仓不复制。

---

## 7. 验证矩阵

### 7.1 命令（逐字，均可在本仓跑）

```bash
# 编译检查（cargo check 在本 workspace 于 boa_engine 内确定性 ICE，见 §10）
cd backend && cargo build -p clash-nyanpasu --lib

# app 单测（process_core_bridge 的 11 个测试需要 fake-core，否则必红；S11 后不再需要）
cd backend && cargo build -p fake-core
cd backend && cargo test -p clash-nyanpasu --lib

# runtime 仓（S13 后必跑）
cd backend/nyanpasu-runtime && cargo test --workspace --all-features --config build.rustc-wrapper=''

# ledger 门
deno run -A scripts/architecture-ledger.ts --mode=report
deno run -A scripts/architecture-ledger.ts --write-snapshot --snapshot scripts/architecture-ledger.snapshot.json
pnpm lint:architecture-ledger
pnpm test:architecture-ledger

# specta bindings 新鲜度（测试在 specta_export.rs:185 export_typescript_bindings，随 --lib 跑）
git diff --exit-code frontend/interface/src/ipc/bindings.ts

# lint 门（.lintstagedrc.js:31-35，注意没有 -D warnings）
cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features
cargo fmt --manifest-path ./backend/Cargo.toml --all
```

基线计数（`docs/audit/2026-08-13-v2-implementation-audit-guide.md` §2，2026-08-30 实测）：app 447 passed / 0 failed / 1 ignored，其中 `core::actor_v2::*` 54、`core::service::*` 14；runtime 484 passed / 24 ignored。本轮结束时计数会变（S10/S11 删测试、S3–S9 增测试），提交正文如实记录增减来源。

### 7.2 纯 fake 可测 vs 需真 daemon

| 卡           | 纯 fake 可测                              | 需真 daemon / 真核           |
| ------------ | ----------------------------------------- | ---------------------------- |
| S1           | ✅（temp dir，只 spawn executor，不起核） | —                            |
| S2           | —                                         | ✅ 只能靠 §7.3 冒烟          |
| S3 / S4 / S5 | ✅                                        | —                            |
| S6 / S7      | ✅                                        | —                            |
| S8 / S9      | ✅（命令层用 fake client）                | 起核路径需 §7.3 冒烟         |
| S10 / S11    | ✅                                        | —                            |
| S12          | ✅（fixture）                             | —                            |
| S13          | —                                         | ✅ `pnpm prepare:check` 实拉 |

### 7.3 冒烟清单（手动，需真 daemon 与真核；非 CI）

1. Local 模式冷启动 → 核起来 → `get_core_status` 报 Running。
2. 切换核（mihomo → mihomo-alpha → clash-rs）：**alpha 通道必须在 Service 模式下也能起**（S3 的 L3 修复判据）。
3. `enable_service_mode` true：daemon 安装/启动 → handoff → 核在 daemon 下运行；false：handoff 回 Local。
4. 卸载 service：核在 Service 下运行时必须被拒（`AlreadyRunning`）；handoff 回 Local 后可卸载（S5 判据）。
5. 退出 app：核停；Service 模式下 daemon 存活（OQ-2 现行为，见 §11 D-4）。
6. 装一台 `2.0.0-rc.1` daemon：必须被判 `Incompatible` 且不进 Service（S12 判据）。

---

## 8. 前端影响面（本计划只交付 Rust 侧接口，前端改造归前端计划）

### 8.1 破坏性变更

| 命令 / 类型                                                   | 现状                                                                             | 换线后                                                                                                                                                                                                        |
| ------------------------------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `get_core_status`                                             | `(Cow<CoreState>, i64, RunType)`（`ipc.rs:399`）                                 | `CoreStatusInfo { host: ExecutionHost, connectivity: EndpointConnectivity, generation: u64, state: Option<CoreStateDetail>, state_changed_at: i64, revision: Option<RevisionIdInfo>, healthy: Option<bool> }` |
| `RunType`                                                     | 导出到 bindings（`core.rs:40`）                                                  | **消失**。语义替代 = `host: ExecutionHost`（`Local`/`Service`）                                                                                                                                               |
| `CoreState`（粗态）                                           | `get_core_status` 的主字段                                                       | 换成 `CoreStateDetail`（`nyanpasu_ipc::api::status::CoreStateDetail`）。粗态会把 Starting/Restarting 塌缩成 Stopped 形状（`endpoint.rs:412-414`），前端不得再据它判"已停"                                     |
| `service::status_service`                                     | `ServiceStatusInfo{ name, version, status, server, compat }`（`ipc.rs:913-919`） | 增 `phase: ServicePhase`、`restart_attempts: u8`；`compat` 语义不变但来源改为 `ServiceActor`                                                                                                                  |
| `ServiceCompat::Incompatible`                                 | `{ server_version, required_major }`（`compat.rs:22-25`）                        | 增 `required_min: String`（S12）                                                                                                                                                                              |
| `service::start_service` / `stop_service` / `restart_service` | 内部隐式重启核（`ipc.rs:960/977/994`）                                           | 只操作 daemon，不再动核。前端若依赖"点了就会重启核"需显式调 `restart_sidecar`                                                                                                                                 |

### 8.2 新增

| 项                                                                            | 说明                                                                                                                                                                 |
| ----------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 事件 `CoreStatusChangedEvent`                                                 | 注册进 `specta_export.rs:125-132` 的 `collect_events!`；由 facade 订阅 `CoreClient::subscribe_events()`（`mod.rs:941`）转发。前端可从轮询 `get_core_status` 改为订阅 |
| 事件 `ServiceStatusChangedEvent`                                              | 同上，源为 `ServiceClient::subscribe()`（`service_actor.rs:584`）                                                                                                    |
| `ExecutionHost` / `EndpointConnectivity` / `ServicePhase` 需加 `specta::Type` | `endpoint.rs:36`、`mod.rs:119`、`service_actor.rs:46`                                                                                                                |

### 8.3 未变

`patch_clash_config`、`enhance_profiles`、`change_clash_core`、`restart_sidecar`、12 个 profile 命令的**签名**不变（只换内部实现）。`get_runtime_config` / `get_runtime_yaml` / `get_runtime_exists` / `get_postprocessing_output` 不变（product 文件保留）。

### 8.4 交付纪律

bindings 由 `specta_export.rs:185 export_typescript_bindings` 在 `cargo test --lib` 时重新生成到 `frontend/interface/src/ipc/bindings.ts`（路径常量 `specta_export.rs:154`），CI 的 `test_unit` job 用 `git diff --exit-code` 卡新鲜度。**每一张改动命令/事件的卡都必须把重新生成的 bindings 纳入同一个提交**，否则该提交的 CI 会红——这违反"每个提交都可构建"。

注：`export_typescript_bindings` 的具名导出循环（`specta_export.rs:211-235`）只钉了 profile 域类型，`ServiceStatusInfo`/`RunType`/`CoreState` 的变化不会被它断言到，只会经 `git diff` 暴露。本轮建议顺手把 `CoreStatusInfo` 与 `ServiceStatusInfo` 加进那个循环——但这属于加固，不属于本阶段必须项，若做则单独在 S14 提交。

---

## 9. 现状与目标的 ledger 对照

| 指标                                    | 当前快照 | 预期方向                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| --------------------------------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `service_globals.CoreManager::global()` | 18       | → 0（S11）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `service_globals` 合计                  | 80       | 下降（`Logger::global()` 在 `rebuild.rs:313` 的一处随 `change_core` 删除；其余 global 属 PR-6/PR-7）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `config_calls` 合计                     | 120      | 下降（`core.rs` 与 `ipc.rs` 内的 `Config::verge()` 随文件删除；`feat.rs` 的保留）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `migration_markers`                     | 19       | 下降。全仓 19 处已逐一核对（`grep -rn 'TODO(actor-migration)\|FIXME(actor-migration)' backend/tauri/src`）。**随本阶段消失的 7 处**：`client/core_bridge.rs:125`、`client/core_bridge.rs:145`、`client/mod.rs:265`（D-2 取默认时）、`core/clash/core.rs:557`、`core/service/ipc.rs:190`、`core/updater/instance.rs:202`、`ipc.rs:400`。**明确存活的**：`client/rebuild.rs:297`（换核仍 draft legacy verge——typed app config 接管属 PR-7）、`client/rebuild.rs:311`（换核清 legacy log sink——LogSink 注入属 PR-6/7），以及 `bridge/*`、`feat.rs:112`、`core/hotkey.rs:200`、`lib.rs:277`、`utils/resolve.rs:154`、`client/rebuild.rs:194/412` |
| `legacy_dto_refs`                       | 300      | 基本不变（属 PR-7）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `bridgeFiles`                           | 8 项     | 少 2 项（`client/core_bridge.rs` 保留但缩小、`client/process_core_bridge.rs` 删除）→ 快照需重算                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |

**再次强调**：以上是方向不是数字。S14 只写实测值。

---

## 10. 实施环境注意（逐条照做，别复现已知坑）

- **满并行构建会 OOM**：全量依赖重建在 `-j 32` 下会以 `STATUS_STACK_BUFFER_OVERRUN` 崩掉。处置：`cargo clean -p <崩掉的 crate>`，然后用 `-j 6` + `CARGO_INCREMENTAL=0` 重跑。
- **`cargo check` 不可用**：本 workspace 在 `boa_engine` 内确定性 ICE。要纯编译检查一律用 `cargo build -p clash-nyanpasu --lib`。
- **PowerShell 下跑 submodule 测试**必须加 `--config build.rustc-wrapper=''`（禁掉 kache），否则失败。
- **共享 `backend/target` 会增量损坏**：症状为 `拒绝访问 (os error 5)`、`tungstenite` 增量数据损坏、`nyanpasu-core-metadata` rmeta 损坏。处置：用干净的隔离 `CARGO_TARGET_DIR` 让 pre-commit 的 clippy 真跑通过。**不要** `--no-verify`。隔离目录跑完记得删（上一轮遗留过 12G）。
- **lint 门是 `.lintstagedrc.js:31-35` 的 clippy，没有 `-D warnings`**：warning 不会挡提交，但也别因此留 warning。
- **`process_core_bridge` 的 11 个测试需要 `cargo build -p fake-core` 先跑**，否则必红（S11 之后此条失效）。
- **高机器负载会假红**：`process_core_bridge::s09_*` 与 `profiles::caller_aborted_refresh_*` 是超时型测试，负载高时会误判。单独重跑该测试确认，不要据一次失败改代码。
- **ICE 先重跑**：不要据一次 ICE 推断工具链不兼容，更不要自作主张 pin toolchain 日期。

---

## 11. 待用户裁定

| #   | 事项                                                                                                                                                                                                                                                                                                                                                                                                | 本计划的默认假设                                                                                                                                                 |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D-1 | **删除 `client/process_core_bridge.rs`（1570 行 / 14 个测试）**。它是被删端口 `CoreLifecyclePort` 的 test-only 进程实现，承载 PR-5d/5e 的 S09 失败矩阵。审计 §4 保留表把这套 fake-core/barrier/failure-injection 基建判给 runtime 仓的 backend contract tests，runtime 侧确有 `fake_backend` / `process` 套件。但那不是**逐条**等价：app 侧矩阵覆盖的是 check/promote/apply/restart 的 app 编排组合 | 默认**删除**，并在提交正文点名 runtime 侧的替代套件。若要保留，唯一诚实的做法是把它改写到 `ControlEndpoint` 上，这会给 S7 增加约一天工作量                       |
| D-2 | **`patch_clash_config` 从 API-first patch 改为 commit-first reconcile**。现路径先经 clash API 热改运行中的核（`client/mod.rs:1366 .clash_patch.patch(&patch)`）再提交；v2 下 Patch/Reload/Restart 的分类归 orchestrator 内部（控制面设计 §11 ⑥），app 不再自己热改。**用户可感知**：某些原本瞬时生效的开关会走一次 reconcile                                                                        | 默认**改**（这是 v2 的结构，保留 app 侧热改就是留第二个事务所有者）。若要保留即时性，须由 orchestrator 的 classify 保证 Noop/Patch 路径足够快，属 runtime 侧调优 |
| D-3 | **S12 的 `REQUIRED_SERVICE_MIN` 取值**。取决于 I-1（runtime 仓版本号提升到多少）。未定则 S12 阻塞                                                                                                                                                                                                                                                                                                   | 无默认；等 runtime 计划给出版本号                                                                                                                                |
| D-4 | **OQ-2：Service 模式下 app 退出是否停核**。集成设计 §9 OQ-2 与 §7 都写"默认保留现行为（停核）"，审计 §6 第 4 项仍列为待裁定，两处未合并结论                                                                                                                                                                                                                                                         | 默认**保留现行为：退出停核**。"退出保活"作为产品选项不在本阶段实现                                                                                               |
| D-5 | **OQ-5：`check_config` 是否开 Tauri 命令**。集成设计 §9 OQ-5 说"仅 profile 编辑器'验证配置'，不接任何自动化路径"；本仓今天 `CoreManager::check_config`（`core.rs:480`）零调用点，前端也没有入口                                                                                                                                                                                                     | 默认**不开命令**。等前端确有"验证配置"按钮需求时单独加                                                                                                           |
| D-6 | **`ManagerOptions.local_ipc_policy`**。runtime 默认 `Disable`（`spec.rs:119`），即不改写源配置的 HTTP controller；app 现行为是让核监听 `external-controller`，clash API 客户端（`core/clash/api.rs`）据此连接。若改成 `Prefer`/`Require` 会引入 epoch-scoped 本地传输，clash API 侧要一起改                                                                                                         | 默认 **`Disable`**（保持现状，最小改动）。改与不改不影响本阶段其余卡                                                                                             |

---

## 12. 完成判据

1. `grep -rn 'CoreManager::global()' backend/tauri/src` 零命中；
2. `backend/tauri/src/core/service/ipc.rs` 与 `client/process_core_bridge.rs` 不存在；
3. 每个 Tauri 命令的函数体只做「解析 DTO → 调 `NyanpasuClient` → 映射错误」（AGENTS §12）；
4. `cargo build -p clash-nyanpasu --lib`、`cargo test -p clash-nyanpasu --lib`、`cargo clippy --all-targets --all-features`、`cargo fmt --check` 全绿；
5. `pnpm lint:architecture-ledger` 与 `pnpm test:architecture-ledger` 全绿，快照为实测重算值；
6. `git diff --exit-code frontend/interface/src/ipc/bindings.ts` 为零；
7. 干净 worktree + `git submodule update --init --recursive` 后每个提交都能构建；
8. §7.3 六项冒烟全部通过（含 alpha 核在 Service 模式下起核、rc.1 daemon 被 fail-closed 拒绝）；
9. 审计入口文档的 L3 / L4 / L9 / L10 / L13 标为已闭并指向本分支的具体提交。
