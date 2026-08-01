# PR-5a 实施计划 — 最小 CoreActor + `OperationId` + `CoreBackend` enum

**日期：** 2026-08-02
**分支基线：** `refactor/core-manager-actor` @ `4583048b5`（含 PR-5-pre 三提交：`4f22eaddb` 依赖切换 / `cca7f654f` 兼容门 / `4583048b5` ledger 同步）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/design.md` §3–§6、同目录 `task.md` 卡 A1/A2/A3
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.1；必答项 §6.4 RQ-02 / RQ-04
**平台：** Windows 11 / PowerShell

---

## 0. 本阶段的边界

**做（= task.md A1/A2/A3）：**

1. 封闭 `CoreBackend` enum，`Local` 包装 `nyanpasu_core_manager::CoreManager`，`Service` 持有实例化的 `nyanpasu_ipc::client::Client`；
2. 取消安全的 `OperationId` / `OperationGate` / `CoreOperationGuard`，client 侧预分配 ID；
3. `CoreClient` 通过既有 `CoreLifecyclePort` / `CoreLifecycleLease` seam 接入，组合根注入，start/stop/restart/status 改走 actor；
4. 删除 legacy `CoreManager::lifecycle_lock` 与裸线程递归 recover。

**不做（越界即返工）：**

- 不改 apply 管线语义（`check_and_promote` / `apply_candidate` / `apply_promoted` 的**实现路径**保持现状，见决策点 D3）；
- 不动 `rebuild_gate` / `clash_patch_gate` / `RuntimeLifecycleStore` / `publish_promoted` / `publish_applied` / `restore_promoted`（全部是 B1/B2/B3 的范围）；
- 不改 `change_core` 的编排与回滚（B4）；
- 不删除 `RunningConfigPatchPort` / `LegacyRunningConfigPatchBridge`（B3）；
- 不迁移 Updater 的 `CoreManager::global()`（design §9 明确留给 PR-6d 的单一 residual）；
- 不做日志 ring / watch 投影 / `set_mode` / `reconcile_mode` / macOS DNS 归位（全部 C1–C3）；
- 不删除 5 s 健康轮询线程与 `IPC_STATE` static（C2）；
- 不改 `get_core_status` 的 wire 形状（C1 才做 additive 扩展）。

---

## 1. 已核验事实

> 全部为本次会话直接读源码所得。`nyanpasu-runtime` submodule 确认在 tag `v2.0.0-rc.1`（`git -C backend/nyanpasu-runtime describe --tags HEAD`）。
> 下表 `RT/` = `backend/nyanpasu-runtime/`，`APP/` = `backend/tauri/src/`。

### 1.1 runtime 侧（`nyanpasu-core-manager` @ v2.0.0-rc.1）

| ID  | 事实                                                                                                                                                                                                                                                                                      | 锚点                                                                                                               |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| R1  | 构造是 **async** 且 **runtime_dir 必填**：`pub async fn new(options: ManagerOptions) -> Result<Self, Error>`；缺 `runtime_dir` 返回 `Error::InvalidManagerOptions("runtime_dir is required")`                                                                                             | `RT/crates/nyanpasu-core-manager/src/manager/mod.rs:212`、`:218-221`                                               |
| R2  | **构造会取运行目录独占锁**，失败返回 `Error::RuntimeDirectoryOwned(path)`；同时校验 controller 模板、拒绝零超时、清扫孤儿 epoch、拉起 JSONL log sink                                                                                                                                      | `manager/mod.rs:223`、`:229-274`                                                                                   |
| R3  | `ManagerOptions::default()` 的 `local_ipc_policy` **已经是 `LocalIpcPolicy::Disable`**（`spec.rs:114`，`spec.rs:154-162` 有单测钉住）。A1 要求的"显式写出"是可审性要求，不是行为修复                                                                                                      | `RT/crates/nyanpasu-core-manager/src/spec.rs:65-128`                                                               |
| R4  | **`CoreManager` 不是 `Clone`**（`struct CoreManager { inner: Arc<Inner> }`，未派生 Clone）→ app 侧必须 `Arc<CoreManager>`                                                                                                                                                                 | `manager/mod.rs:87-89`                                                                                             |
| R5  | 生命周期方法全部 `&self` + async，错误类型 `nyanpasu_core_manager::Error`：`start(InstanceSpec)`、`stop()`、`check_config(&InstanceSpec)`、`shutdown()`、`restart() -> SwitchOutcome`、`switch(InstanceSpec) -> SwitchOutcome`                                                            | `manager/mod.rs:319,438,509,541`；`manager/switching.rs:45,52`                                                     |
| R6  | **没有 `recover()`**；实际名字是 `recover_quarantine() -> Result<(), Error>`，语义是"清除 quarantine 闩锁"，不是"重启核心"                                                                                                                                                                | `manager/quarantine.rs:18`                                                                                         |
| R7  | `apply_config(&self, input: InstanceSpec, expected_revision: Option<RevisionId>) -> Result<ApplyOutcome, Error>`——参数是**整个 `InstanceSpec` 按值**，不是配置路径；要求核心已在跑，否则 `Error::NotStarted`                                                                              | `manager/apply.rs:19-23`、`:26-29`                                                                                 |
| R8  | CAS 失败返回 `Error::RevisionConflict { expected: RevisionId, actual: Option<RevisionId> }`，且**不应用任何东西**                                                                                                                                                                         | `manager/apply.rs:30-38`；`src/error.rs:43-47`                                                                     |
| R9  | `RevisionId { epoch: u64, generation: u64, effective_hash: String }`，**不是 `Copy`**；由 `ConfigRevision::id()` 取得，`ConfigRevision` 挂在 `CoreStatus.revision: Option<ConfigRevision>` 上；**没有 `CoreManager::revision()` 访问器**                                                  | `src/state.rs:143-167`、`:189`                                                                                     |
| R10 | 状态与日志订阅：`subscribe() -> watch::Receiver<CoreStatus>`、`subscribe_logs() -> broadcast::Receiver<Arc<LogFrame>>`（容量 256，可在首次 `start()` 前调用）、`status() -> CoreStatus`                                                                                                   | `manager/mod.rs:292,296,308`；`src/log.rs:16`                                                                      |
| R11 | 本地 `ApplyOutcome` 有 7 个分支：`Noop` / `Patched` / `Reloaded` / `Restarted` / `Switched` / `RolledBack{failed_apply}` / `DurabilityUncertain{outcome, warning}`。**`Warning` 是包装器不是 outcome**（印证 RQ-03）                                                                      | `manager/mod.rs:57-85`                                                                                             |
| R12 | 供抄写的 outcome 映射参考实现：`map_apply_outcome`（把 `DurabilityUncertain` 拆成 `CoreApplyData.warning`，可嵌套两层、用 `"; "` 拼接）                                                                                                                                                   | `RT/crates/nyanpasu-service-runtime/src/server/manager_bridge.rs:605-639`                                          |
| R13 | **manager 自身已做有界重启 + 指数退避**（委托 `nyanpasu_utils::process::Supervisor`）：`InstanceOptions.restart_policy` 默认 `OnFailure{max_restarts: 5}`、`backoff` 默认 `exponential(1s, 30s).with_jitter()`；另有**不可通过 `InstanceOptions` 配置**的 storm guard（默认 5 次/5 分钟） | `src/spec.rs:31-50`；`src/instance.rs:229-237`；`RT/crates/nyanpasu-utils/src/process/supervisor.rs:36-55,431-466` |
| R14 | **恢复耗尽没有 typed 信号**——唯一机器可读线索是 `CoreState::Stopped { reason: Some(StopReason::Error(msg)) }` 且 `msg` 以 `"core kept crashing; restart budget exhausted\n"` 开头；上游自己的测试也在字符串匹配                                                                           | `src/instance.rs:631-641`；`RT/.../tests/instance_lifecycle.rs:246-252`                                            |
| R15 | 第二个不可恢复闩锁是 **quarantine**：`Error::StopUnconfirmed` → `latch_quarantine`，此后所有受门操作（start/switch/restart/apply）都被拒，直到 `recover_quarantine()` 成功。`stop()`/`shutdown()` 故意绕过该门                                                                            | `manager/quarantine.rs:8-13`；`manager/mod.rs:434-437,537-540`                                                     |
| R16 | **`Error::kind()` 在本 tag 不存在**（整个 crate 没有 `impl Error` 块）。可用的机器可读分类是：ipc 侧的 `nyanpasu_ipc::api::error_kind` 字符串常量表 + 一个**私有**映射函数 `map_error_kind`（位于 `nyanpasu-service-runtime`，app 不依赖该 crate）                                        | `RT/nyanpasu_ipc/src/api/mod.rs:38-66`；`manager_bridge.rs:646-665`                                                |
| R17 | 可抄的测试脚手架：`tests/common/mod.rs` 的 `fast_options()`（`max_restarts: 2`、50ms→200ms backoff、50ms 健康间隔）、`wait_for_state()`、`wait_for_health()`、`utf8_tempdir()`。fake core 是**该 crate 自己的 `[[bin]]`**，app 侧**拿不到** `CARGO_BIN_EXE_*`                             | `RT/crates/nyanpasu-core-manager/tests/common/mod.rs:18-147`；`Cargo.toml:12-15`                                   |

### 1.2 runtime 侧（`nyanpasu-ipc` v2 client）

| ID  | 事实                                                                                                                                                                                                                                            | 锚点                                                       |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| R18 | 可实例化构造：`Client::new(placeholder: &str) -> Result<Self>`；`Client` 是 `Clone + Debug`。`service_default()` 仍在（`OnceLock`），A1 禁止使用它                                                                                              | `RT/nyanpasu_ipc/src/client/mod.rs:67-99`                  |
| R19 | 默认端点常量 `pub const SERVICE_PLACEHOLDER: &str = "nyanpasu_ipc"`；placeholder 映射为 Windows `\\.\pipe\{p}` / Unix `/var/run/{p}.sock`                                                                                                       | `RT/nyanpasu_ipc/src/lib.rs:12`；`client/mod.rs:76-79`     |
| R20 | `apply_config(&CoreApplyReq) -> Result<CoreApplyData>`；`CoreApplyReq { core_type, config_file, expected_revision: Option<RevisionIdInfo> }`，`None` = 无条件                                                                                   | `client/shortcuts.rs:51-62`；`src/api/core/apply.rs:21-31` |
| R21 | `check_config(&CoreCheckReq)`、`start_core(&CoreStartReq)`、`stop_core()`、`restart_core()`、`recover_core()`、`status() -> StatusResBody`、`events() -> EventStream`                                                                           | `client/shortcuts.rs:26-110`                               |
| R22 | `Client::recover_core()` 的语义是"清 quarantine 闩锁，幂等"——与 Local 的 `recover_quarantine()` **对称**，不是"重启核心"                                                                                                                        | `client/shortcuts.rs:69`                                   |
| R23 | `ApplyOutcomeKind` 六个 wire 分支含 **`Noop`**；`Warning` 是 `CoreApplyData.warning: Option<String>` **字段**不是分支                                                                                                                           | `src/api/core/apply.rs:34-60,70-85`                        |
| R24 | `/ws/events` 的 `Event` 三个分支：`CoreStateChanged(CoreState)`（有损两值）、`CoreStatusChanged(CoreInfos)`（**完整快照，连接即推一次、丢事件恢复后再推、每次转换都推**）、`CoreLog(Arc<LogFrame>)`。日志与状态**无顺序保证**；连接时不重放日志 | `src/api/ws/events.rs:26-68`                               |
| R25 | Service 侧 revision 载体是 `ConfigRevisionInfo { epoch, generation, source_hash, effective_hash }`，CAS token 是 `RevisionIdInfo { epoch, generation, effective_hash }`，由 `ConfigRevisionInfo::id()` 取得                                     | `RT/nyanpasu_ipc/src/api/status.rs:69-105`                 |
| R26 | 错误分类在 client 侧以 `error_kind: Option<String>` 到达                                                                                                                                                                                        | `RT/nyanpasu_ipc/src/client/mod.rs:51-53`                  |

### 1.3 app 侧现状

| ID   | 事实                                                                                                                                                                                                                                                                                                              | 锚点                                                                                                                             |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| A1f  | 既有 seam：`CoreLifecyclePort { begin() -> Box<dyn CoreLifecycleLease>, status() -> CoreStatusSnapshot, on_profile_change() }`；`CoreLifecycleLease: Send`（**不是 Sync**）有 5 个方法 `check_and_promote` / `apply_candidate` / `apply_promoted` / `restart` / `stop`                                            | `APP/client/core_bridge.rs:43-74`                                                                                                |
| A2f  | 组合根在 `setup.rs:42-55`，`core: Arc::new(LegacyCoreBridge::new(runtime_paths))` 从这里注入；`NyanpasuClient::try_new_with_args` 是 **sync fn**，内部 `tauri::async_runtime::block_on`；四个 typed actor 各自在自己的 `Client::new()` 里 `Actor::spawn`                                                          | `APP/setup.rs:42-61`；`APP/client/mod.rs:255-332,273`                                                                            |
| A3f  | **`begin()` 共 10 个生产调用点**，`rebuild_gate` 在**全部 10 处都先于** `core.begin()` 获取；`patch_running_config` 是最严格的三层顺序 `clash_patch_gate → rebuild_gate → begin()`                                                                                                                                | `APP/client/mod.rs:1231,1276,1351-1353,1413,1429`；`APP/client/rebuild.rs:232,257,268,282,406`                                   |
| A4f  | lease 以 **`&mut dyn`** 跨函数传递（4 处签名），并且 `patch_running_config` 把 lease **move 进 `async move` 闭包**交给 `feat::patch_clash_with_rebuild` → guard 必须 `Send` 且可移动进 boxed future                                                                                                               | `APP/client/mod.rs:1285-1291,1371-1400,1436-1439,1452-1459`；`APP/client/rebuild.rs:239-242`                                     |
| A5f  | `CoreLifecyclePort::status()` **没有生产调用者**；生产状态读取仍直连 `CoreManager::global().status()`                                                                                                                                                                                                             | `APP/ipc.rs:403`；`APP/feat.rs:292,385`；`APP/core/service/ipc.rs:83`                                                            |
| A6f  | `CoreLifecycleLease::stop()` **没有生产调用者**；生产停核走 `CoreManager::global().stop_core()`                                                                                                                                                                                                                   | `APP/utils/help.rs:268`；`APP/utils/resolve.rs:288`                                                                              |
| A7f  | typed actor client 房规：`Clone` via `Arc<…Inner>`；每个 client 一个手写 `call` helper，把 `CallResult::{SenderError, Timeout}` 映射成显式错误；**读用 `Some(5s)`，写用 `None`**；`Drop for …Inner` 调 `actor_ref.stop(None)`                                                                                     | `APP/client/application.rs:17-162`                                                                                               |
| A8f  | 测试注入钩子已存在：`test_client_args_with_lifecycle(dir, core: Arc<dyn CoreLifecyclePort>) -> ClientSetupArgs`——**5a 直接复用**，无需新建测试图                                                                                                                                                                  | `APP/client/mod.rs:2087-2106`                                                                                                    |
| A9f  | 需要更新的 lease 测试替身共 6 个：`MockRunningCoreBridge`/`MockCoreLease`、`TestCorePort`/`TestCoreLease`、`CompensationLease`、`BarrierCompensationLease`、`BarrierCore`/`BarrierLease`，外加 trait 上的 `#[cfg_attr(test, mockall::automock)]`                                                                  | `APP/client/mod.rs:1589-1919`；`APP/client/rebuild.rs:814-928`；`core_bridge.rs:53`                                              |
| A10f | 测试全程**零 sleep**：oneshot barrier / `AtomicUsize` / `mockall::Sequence` / `tokio::time::pause()` / `Notify`。5a 必须延续                                                                                                                                                                                      | `APP/client/rebuild.rs:930-1000`                                                                                                 |
| A11f | `NyanpasuClient::shutdown()` 目前**只**关 rebuild worker；生产顺序是 `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`                                                                                                                                                                      | `APP/client/mod.rs:392-404`；`APP/utils/help.rs:249-272`                                                                         |
| A12f | `RunType::classify(enable_service, ipc_state)` 已由 PR-5-pre 抽出为纯函数；`IpcState` 只由 `health_check()` 翻转，兼容门在 `target_ipc_state()`                                                                                                                                                                   | `APP/core/clash/core.rs:50-67`；`APP/core/service/ipc.rs:143`                                                                    |
| A13f | `Instance::try_new` 展示了构造核心进程所需的全部输入：core_type、app_data_dir、binary（`find_binary_path`）、config_path、pid_path                                                                                                                                                                                | `APP/core/clash/core.rs:83-124,711-728`                                                                                          |
| A14f | `ractor = "0.16"`；`nyanpasu-core-manager` / `nyanpasu-core-metadata` **当前不在** workspace 依赖里（PR-5-pre 的 D1=A 推迟到本阶段）                                                                                                                                                                              | `backend/tauri/Cargo.toml:63`；`backend/Cargo.toml:27-41`                                                                        |
| A15f | 本仓 `backend/fake-core` 对 `backend/tauri` 是 **dev-dependency**，因此**既不构建该 binary 也不设置 `CARGO_BIN_EXE_fake-core`**。既有做法是预构建（`cargo build -p fake-core`）+ 运行时定位 `fake_core::require_bin_path()`（current_exe profile/triple 查找 → 非空 `NYANPASU_FAKE_CORE` 覆盖 → target 目录回退） | `backend/tauri/Cargo.toml:273-281`；`backend/fake-core/src/lib.rs:399-418`；消费者示例 `APP/client/process_core_bridge.rs:18-20` |

### 1.4 与 spec 正文的偏差（必须按事实实现，spec 措辞为准的地方已注明）

| 偏差 | design 措辞                                                                       | 实际                                                                                                       | 处理                                                                                 |
| ---- | --------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| DV-A | §4 `async fn recover(&self) -> Result<()>`                                        | Local 是 `recover_quarantine()`，Service 是 `recover_core()`，二者语义都是**清 quarantine 闩锁**（R6/R22） | `CoreBackend::recover()` 保留此名，doc 注明真实语义是 clear-quarantine，**不是**重启 |
| DV-B | §4.1 "只保留机器可读 kind + 原始 message/source"                                  | 本 tag 无 `Error::kind()`（R16），R0 才加                                                                  | 见决策点 D4：隔离一个 `core_error_kind` 模块，bump 后一步替换                        |
| DV-C | §5 "Supervisor/daemon 最终放弃后，发布一次 `core_recovery_exhausted` degradation" | 无 typed 信号，只有字符串前缀（R14）                                                                       | 见决策点 D5                                                                          |
| DV-D | §4 `async fn apply(&self, request: &CoreRequest) -> Result<CoreApplyData>`        | Local 返回本地 `ApplyOutcome`（7 分支），需要按 R12 映射成 `CoreApplyData`                                 | backend 内转换，app 侧不新增 outcome 类型                                            |
| DV-E | §6 `CoreActorState` 含 `runtime: RuntimeLifecycleState` 与 `logs: VecDeque`       | 那是 **B1/C1** 的字段                                                                                      | 5a 的 actor state **不含**这两项，见 §0 不做清单                                     |

---

## 2. RQ 必答（roadmap §6.4）

### RQ-04 — `begin_operation` 的调用侧有限超时

**答：** 超时加在 **client 的 RPC 等待**上，不是 actor 里。actor 的 `AcquireOperation` handler 永远立即返回（design §3.3），真正在等的是调用方手里的 reply future。

```rust
/// 排队等待 operation 的上限。不是单次核心操作的上限——
/// 排在前面的一次 apply/start 本身就可能跑满 runtime 的 startup_timeout(30s)
/// + reconcile_timeout(30s)（见事实 R13），所以这个值必须显著大于单次操作。
/// 它存在的唯一目的是：actor 卡死时调用方能失败返回，而不是永久挂起。
const CORE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(120);
```

取值依据：runtime 默认 `startup_timeout` 30 s、`reconcile_timeout` 30 s、`stop_timeout` 10 s、`control_timeout` 10 s（R1/R13），单次最坏 ≈ 80 s；120 s 留余量且仍然有限。

超时后的行为**复用既有 drop 路径**，不新增机制：

1. `.call(..., Some(CORE_ACQUIRE_TIMEOUT))` 返回 `CallResult::Timeout`；
2. `begin_operation` 返回 `Err(OperationError::AcquireTimeout)`；
3. **pending guard 随栈展开被 drop**，其 `Drop` 用 `cast`（fire-and-forget，可在同步 `Drop` 里安全调用）发出 `ReleaseOperation { id }`；
4. actor 收到后：若该 ID 在 waiters 里 → 删除（等价取消）；若刚好已被提升为 active → 清空 active 并放行下一个 waiter；若未知 → 幂等 no-op + debug 日志。

第 4 步的三分支正是 design §3.3 的规则，**不需要为超时新增任何状态**。

**明确不实现**：TTL、auto-steal、watchdog self-message、续期、心跳。

读操作（status）不走 mailbox 排队，沿用房规 `Some(5s)`（A7f）。

### RQ-02 — engine revision 的 app 侧处理与 `expected_revision` CAS

**表示法（不新增 app 镜像类型）。** design §4.1 禁止在 app 侧复制 `EngineRevision`。两侧结构完全同构：

- Local：`RevisionId { epoch, generation, effective_hash }`（R9），完整信息在 `ConfigRevision`（多一个 `runtime_path`，是 0o700 私有目录，app 读不到，**丢弃**）；
- Service：`RevisionIdInfo { epoch, generation, effective_hash }`（R25），完整信息在 `ConfigRevisionInfo`。

**结论：统一采用 IPC 的 `RevisionIdInfo` / `ConfigRevisionInfo` 作为 app 侧唯一表示**，理由：(a) 它已经是 app 依赖的 wire 类型且已 derive specta，C1 要 additive 暴露时零成本；(b) `LocalBackend` 内做一次 `RevisionId → RevisionIdInfo` 的字段搬运即可，转换点收敛在 backend 内部，符合 §4.1"在 backend 内转换"。

**存储。** actor state 持有 `last_revision: Option<ConfigRevisionInfo>`，作为 `CoreStatusView` 的一个字段。它是**观察到的事实缓存**，不是权威——权威永远在 runtime 那边。

**刷新来源（三条，最后写入者赢）：**

| 来源     | Local                                                 | Service                                                      |
| -------- | ----------------------------------------------------- | ------------------------------------------------------------ |
| 主动查询 | `CoreManager::status().revision`（R10）               | `Client::status().core_infos.revision`（R21）                |
| 推送     | `subscribe()` 的 `watch::Receiver<CoreStatus>`（R10） | `/ws/events` 的 `Event::CoreStatusChanged(CoreInfos)`（R24） |
| 操作返回 | `ApplyOutcome::*{revision}`（R11）                    | `CoreApplyData.revision`（R23）                              |

**重连对账。** Service 的 `/ws/events` 断线后，`Event::CoreStatusChanged` 会在**连接建立瞬间**再推一次完整快照（R24 明确保证），因此重连不需要额外的对账 RPC——**以重连后收到的第一个快照为准，直接覆盖**。Local 的 `watch` 通道不会断（与 manager 同生命周期），无需对账。两侧都遵循"快照幂等、最后一帧赢"（R24 原文建议）。

**冲突处理。** `expected_revision` 语义：

- `None` = 无条件应用（R7/R20）。**只允许在一种情况下出现：actor 尚未观察到任何 revision**——而 `apply` 要求核心已在跑（R7 `Error::NotStarted`），核心跑起来必然产生 revision，所以正常路径下 `None` 不会出现。因此规则是：**apply 一律传 `Some(last_revision.id())`；若 `last_revision` 为 `None`，视为内部不变量被破坏，返回错误而不是降级为无条件应用。**
- CAS 失败：Local `Error::RevisionConflict { expected, actual }`（R8），Service `error_kind = "revision_conflict"`（R20）。两侧都**没有应用任何东西**，所以处理方式是：用 `actual`（或重新查一次 status）刷新 `last_revision`，然后把冲突作为可重试错误上报。

**5a / 5b 分工（重要）：**

- **5a 只做"观察与存储"**：建立表示法、订阅刷新、重连对账、暴露给 `CoreClient` 的读接口，并用测试钉住。
- **5b 才做"CAS 应用"**：因为 5a 不改 apply 管线（决策点 D3），`expected_revision` 在 5a 没有生产调用点。上面的冲突规则属于 B1 的实现契约，5a 只负责让 `last_revision` 在那时是可信的。

> **前向引用（不在本阶段作答）：** RQ-01 完整 post-commit 失败矩阵、RQ-03 apply parity 含 `Noop`——均由 **PR-5b 计划**回答。事实 R11/R23 已为其备好素材。

---

## 3. 需 leader 裁定的决策点

> 未另行裁定则按推荐项执行。
>
> **Leader 裁定（2026-08-02）：D1=A、D2=A、D3=A、D4=A、D5=A、D6=120s。** 全部按推荐项执行。D5 附带建议（上游 `StopReason` 加 `RestartBudgetExhausted` typed 变体）**不并入已收口的 R0 分支**——记入待用户授权的上游事项，授权后作独立小 PR；本阶段按字符串前缀实现。

### D1 — workspace 依赖加哪几个 crate？

PR-5-pre 的 D1=A 把 `nyanpasu-core-manager` / `nyanpasu-core-metadata` 推迟到"首个真实消费者"，那就是本阶段。

- **推荐 A**：只加 `nyanpasu-core-manager` 与 `nyanpasu-core-metadata` 两条。前者是 `LocalBackend` 的直接依赖；后者提供 `LogFrame` / `ClashCoreKind`，且 R0 之后的 `CoreErrorKind` 也在其中——虽然 5a 的日志 ring 是 C1 范围，`ClashCoreKind`（= `CoreKind`）在构造 `CoreSpec` 时就要用到，属真实消费。
- **选项 B**：再加 `clash-api`。**不推荐**——app 侧无直接消费者（`clash_api::Host` 只在 `CoreStatus.controller` 里出现，5a 不读它），加了就是死条目。

### D2 — backend 的构造时机

`CoreManager::new()` 会取运行目录独占锁（R2），所以"两个 backend 都常驻"不可行。

- **推荐 A：pre_start 只构造当前模式匹配的那一个；`SetBackend` 时 `shutdown()` 旧的 → 构造新的。** 释放锁靠 `shutdown()` + drop。`SetBackend` 必须在 operation guard 下执行（design §3.4 已列入必须持 guard 的清单）。
- **选项 B**：完全惰性，首次用到才构造。省一次启动开销，但把构造失败推迟到第一次操作，错误定位更差。

配套顺序事实：组合根（`setup.rs`）先于 `init_service()` 运行，此时 `IpcState` 仍是 `Disconnected`，`RunType::classify` 必然给出 `Normal`（A12f）。所以**启动时总是先建 Local backend**，随后健康检查若判定兼容再发 `SetBackend(Service)`。这与 fail-closed 语义一致，是期望行为。

### D3 — 5a 是否把 apply 改道到 `CoreBackend::apply`？（**本计划最关键的范围裁定**）

- **推荐 A：不改道。** `check_and_promote` / `apply_candidate` / `apply_promoted` 在新的 lease 适配器里**保持现有实现**（候选检查 + 原子 promote + `api::put_configs` 推送）。理由：(1) task.md A3 列的是 "start/stop/restart/status 改走 actor"，apply **不在其中**；(2) 统一 full-config apply 是 design §7 白纸黑字的 **5b/B3** 范围；(3) `put_configs` 是对核心 external-controller 的 HTTP 调用，与"谁拉起了进程"无关，因此 CoreActor 接管进程所有权后它照常工作；(4) 让 5a 保持"纯所有权搬移"，diff 可审。
  - 代价：5a 结束时短暂存在两条配置应用路径（legacy PUT 用于生产，`CoreBackend::apply` 仅被测试覆盖）。这正是 B3 要消灭的。
- **选项 B**：5a 就改道。会把 B1（Promoted/Applied 入 actor）和 B3（删补偿层）的一部分提前，且 `expected_revision` 的失败矩阵（RQ-01）尚未定义，风险明显更高。

### D4 — `CoreBackend::apply` 是否在 5a 实现？

与 D3 配套。task.md A1 明确写了 "只实现 check/start/apply/stop/restart/recover"。

- **推荐 A：实现，但只被 Test backend 与 parity 测试消费，生产不接线。** 依据是 A1 卡的显式要求；同时它让 R12 的 outcome 映射（`DurabilityUncertain` 拆 warning、可嵌套两层）在 5a 就被测试钉住，5b 接线时只剩接线。
- **选项 B**：5a 不实现 apply，5b 再加。更严格地遵守 CLAUDE.md §2（不写投机代码），但违反 A1 卡的字面要求，且把 outcome 映射的风险全压到 5b。

### D5 — `core_recovery_exhausted` 怎么判定？

事实 R14：没有 typed 信号，只有 `StopReason::Error` 里的字符串前缀。

- **推荐 A：定义一个常量 + 一个纯函数，集中在一处字符串匹配，并用测试钉住前缀。** Local 从 `CoreStatus.state` 取，Service 从 `CoreState::Stopped(Some(msg))` / `CoreStatusChanged` 的 detail 取（同一字符串，因为 daemon 内跑的是同一个 manager）。
  ```rust
  /// 上游 `instance.rs:631-641` 在重启预算耗尽时写入的前缀。
  /// 这是目前唯一的机器可读线索——上游没有 typed 信号（见计划 §1.1 R14）。
  /// 若上游后续加了 typed 变体，这里连同 `is_recovery_exhausted` 一并删除。
  const RECOVERY_EXHAUSTED_PREFIX: &str = "core kept crashing; restart budget exhausted";
  ```
- **选项 B**：不实现该 degradation，等上游加 typed 信号。design §5 明确要求"发布一次 degradation"，跳过会留下功能缺口。
- **附带建议（需用户授权，不在本阶段执行）**：给上游提一个小改动，在 `StopReason` 上加 `RestartBudgetExhausted` 变体。R0 的 PR 尚未推送，可以合并进同一批上游改动——**请 leader 决定是否纳入**。

### D6 — `CORE_ACQUIRE_TIMEOUT` 取值

推荐 **120 s**，依据见 RQ-04。若 leader 认为应更激进（例如 60 s），需同时接受"前序 apply 跑满 runtime 超时时后续操作可能误超时"的代价。

---

## 4. 实施步骤

> 每步给出编辑内容 → 验证命令 → 通过判据。全程**不要**跑 `cargo clippy -- -D warnings`（仓库本就红）。
> 记忆事项：本仓共享 target 曾出现 kache 污染导致本地 clippy 假红；若遇到，用独立 target 目录复验再判定。

### S1 — workspace 依赖（按 D1=A）

`backend/Cargo.toml` 的 `[workspace.dependencies]` `# --- nyanpasu ---` 段追加两条，沿用既有注释风格：

```toml
nyanpasu-core-manager = { path = "nyanpasu-runtime/crates/nyanpasu-core-manager" }
nyanpasu-core-metadata = { path = "nyanpasu-runtime/crates/nyanpasu-core-metadata" }
```

`backend/tauri/Cargo.toml` 的 `# Local Dependencies` 段追加：

```toml
nyanpasu-core-manager = { workspace = true }
nyanpasu-core-metadata = { workspace = true }
```

**验证：**

```powershell
cargo metadata --manifest-path .\backend\Cargo.toml --format-version 1 | Out-Null; $LASTEXITCODE
Select-String -Path .\backend\Cargo.lock -Pattern '^name = "nyanpasu-core-manager"' -Context 0,2
cargo tree --manifest-path .\backend\Cargo.toml --duplicates --edges normal | Out-String
```

**通过判据：** exit 0；两个 crate 以 path 依赖入 lock（无 `source =` 行）；`--duplicates` **没有新增重复组**（出现新重复组 → 停止并上报，不要自行改 feature）。

### S2 — 请求与状态投影类型（纯类型，零行为）

新文件 `APP/core/actor/types.rs`（模块树见 S5）。

```rust
/// 一次核心操作的完整输入。两个 backend 各自把它翻译成自己的形状：
/// Local → `InstanceSpec`（事实 R7），Service → `CoreStartReq` / `CoreApplyReq`（R20）。
#[derive(Debug, Clone)]
pub struct CoreRequest {
    pub core_type: nyanpasu_utils::core::CoreType,
    pub binary_path: camino::Utf8PathBuf,
    pub config_path: camino::Utf8PathBuf,
    pub working_dir: camino::Utf8PathBuf,
    pub pid_path: Option<camino::Utf8PathBuf>,
}

/// 给 UI 的最小状态投影。刻意**不**镜像 runtime 的 `CoreStatus`：
/// 只保留 5a 的调用点真正需要的字段（见事实 A5f 的四个读取点）。
#[derive(Debug, Clone)]
pub struct CoreStatusView {
    pub state: nyanpasu_ipc::api::status::CoreState,
    pub state_changed_at: i64,
    pub run_type: crate::core::RunType,
    /// 观察到的运行配置 revision（RQ-02）。权威在 runtime，这里只是缓存。
    pub revision: Option<nyanpasu_ipc::api::status::ConfigRevisionInfo>,
    /// runtime 侧重启预算耗尽（事实 R14）。
    pub recovery_exhausted: bool,
}
```

`CoreRequest` 的构造沿用 `Instance::try_new` 的既有取值方式（A13f：`find_binary_path` + `dirs::app_data_dir` + `dirs::clash_pid_path`），**不改变这些取值逻辑**。

**验证：** `cargo check -p nyanpasu --all-features`（或 `pnpm lint:clippy`）。

### S3 — `CoreBackend` 封闭 enum

新文件 `APP/core/actor/backend.rs`。

```rust
pub enum CoreBackend {
    Local(LocalBackend),
    Service(ServiceBackend),
    #[cfg(test)]
    Test(TestBackend),
}

impl CoreBackend {
    pub async fn check(&self, request: &CoreRequest) -> Result<(), CoreBackendError>;
    pub async fn start(&self, request: &CoreRequest) -> Result<(), CoreBackendError>;
    pub async fn apply(&self, request: &CoreRequest, expected: Option<RevisionIdInfo>)
        -> Result<CoreApplyData, CoreBackendError>;
    pub async fn stop(&self) -> Result<(), CoreBackendError>;
    pub async fn restart(&self) -> Result<(), CoreBackendError>;
    /// 清除 runtime 的 quarantine 闩锁。**不是**"重启核心"——
    /// Local 走 `recover_quarantine()`、Service 走 `recover_core()`（事实 R6/R22/DV-A）。
    pub async fn recover(&self) -> Result<(), CoreBackendError>;
    pub fn status(&self) -> CoreStatusView;
}
```

`LocalBackend`：

- 持 `Arc<nyanpasu_core_manager::CoreManager>`（事实 R4：非 Clone）+ `watch::Receiver<CoreStatus>`（R10）；
- 构造时 `ManagerOptions` **显式写出** `local_ipc_policy: LocalIpcPolicy::Disable`，并加注释说明"上游默认值已是 Disable（`spec.rs:114`），显式化是为了让这条安全门在 app 侧可见可审"（A1 卡要求，事实 R3）；
- `runtime_dir` 来自注入的 `RuntimePaths`，**不得**用 `dirs::*` 自行解析（roadmap §1.6 测试禁真实目录）；
- `apply` 把本地 `ApplyOutcome`（7 分支，R11）映射成 `CoreApplyData`，映射规则**照抄** `manager_bridge.rs:605-639`（R12），含 `DurabilityUncertain` 可嵌套两层、warning 用 `"; "` 拼接。

`ServiceBackend`：

- 持 `nyanpasu_ipc::client::Client`（实例，`Client::new(SERVICE_PLACEHOLDER)`，**禁止 `service_default()`**——A1 卡要求，事实 R18/R19）；
- 状态来自 `/status` + `/ws/events`（R21/R24）。

`TestBackend`（`#[cfg(test)]`）：脚本化返回值 + 调用计数 + oneshot barrier 钩子，供 A2/A3 的并发测试使用。

**验证：** `cargo check`；`rg 'service_default' backend/tauri/src` 只命中 legacy `core/clash/core.rs`（5a 不动那 5 处，见 §0）。

### S4 — error kind 分类（R0 条件化，隔离在一个模块）

新文件 `APP/core/actor/error_kind.rs`，**整个文件就是 R0 的适配层**。

```rust
//! 机器可读的核心错误分类。
//!
//! 当前 submodule pin（v2.0.0-rc.1）**没有** `Error::kind()`（见计划 §1.1 R16）；
//! 上游的映射函数 `map_error_kind` 是 `nyanpasu-service-runtime` 的私有函数，
//! 本 app 不依赖该 crate。因此这里维护一份等价映射作为过渡。
//!
//! TODO(actor-migration): 过渡实现，等 R0 合并并 bump submodule。
//! Reason: `nyanpasu_core_manager::Error::kind()` 在 v2.0.0-rc.1 不存在。
//! Remove when: submodule 指向含 R0 的 tag —— 届时本文件退化为
//! `err.kind()` 的一次调用，Service 侧的字符串解析也改成解析 typed kind。
```

- Local：`fn local_error_kind(err: &nyanpasu_core_manager::Error) -> Option<&'static str>`，逐条对齐 `nyanpasu_ipc::api::error_kind` 的 12 个常量（R16），并像上游一样递归进 `DurabilityUncertain`；
- Service：直接读 `ClientError` 的 `error_kind: Option<String>`（R26）。

**R0 落地后的替换步骤（一步，独立提交）：** 删除 `local_error_kind` 的 match 体，改为 `err.kind()`；Service 侧把字符串解析成同一个 enum。**本计划不执行该步**（见 §0 与 out-of-scope）。

**验证：** 单测断言 12 个常量的映射与 `nyanpasu_ipc::api::error_kind` 一一对应（用常量本身而非字面量，防止上游改字符串时静默漂移）。

### S5 — `OperationId` / `OperationGate` / actor 骨架

新文件 `APP/core/actor/mod.rs`（`pub mod backend; pub mod types; mod error_kind; pub mod client;`）与 `APP/core/actor/gate.rs`。

按 design §3.1–§3.3 逐字实现：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(NonZeroU64);   // 0 保留为无效值

struct OperationGate { active: Option<ActiveOperation>, waiters: VecDeque<OperationWaiter> }
struct ActiveOperation { id: OperationId, acquired_at: Instant }
struct OperationWaiter { id: OperationId, reply: RpcReplyPort<Result<(), OperationError>> }
```

消息枚举（**5a 只放本阶段用得到的**，design §3.2 里属于 B1 的 `CheckAndPromote` / `ApplyPromoted` / `StartPromoted` **不在 5a**）：

```rust
pub enum CoreActorMessage {
    AcquireOperation { id: OperationId, reply: RpcReplyPort<Result<(), OperationError>> },
    ReleaseOperation { id: OperationId },

    Check   { operation: OperationId, request: CoreRequest, reply: ... },
    Start   { operation: OperationId, request: CoreRequest, reply: ... },
    Stop    { operation: OperationId, reply: ... },
    Restart { operation: OperationId, reply: ... },
    Recover { operation: OperationId, reply: ... },
    SetBackend { operation: OperationId, mode: RunType, reply: ... },

    Status(RpcReplyPort<CoreStatusView>),
    Shutdown(RpcReplyPort<()>),

    BackendStatus(CoreStatusView),   // 来自订阅任务的内部投递
}
```

actor state（对照 DV-E：**不含** runtime lifecycle 与 log ring）：

```rust
struct CoreActorState {
    backend: CoreBackend,
    mode: RunType,
    operation: OperationGate,
    status: CoreStatusView,
}
```

gate 规则（design §3.3，逐条）：无 active → 立即登记并回复成功；有 active → 入 FIFO，**handler 立即返回，绝不等待**；`ReleaseOperation(active_id)` → 清 active 并放行下一个仍有接收方的 waiter；`ReleaseOperation(waiting_id)` → 从 waiters 删除；未知 ID → 幂等 no-op + debug 日志；mutation 的 ID 与 active 不符 → `StaleOperation`；shutdown → 拒绝全部 waiters、清 active、关 backend。

**验证：** S12 的 T-OP-01…07。

### S6 — `CoreClient` + `CoreOperationGuard`

新文件 `APP/client/core.rs`（与 `application.rs` / `profiles.rs` 同级，遵循 A7f 房规）。

```rust
#[derive(Clone)]
pub struct CoreClient { inner: Arc<CoreClientInner> }

struct CoreClientInner {
    actor_ref: ActorRef<CoreActorMessage>,
    next_operation: AtomicU64,   // clones 共享（design §3.1）
}

pub struct CoreOperationGuard { id: OperationId, client: CoreClient, acquired: bool }
```

关键实现点：

- `begin_operation()` **先分配 ID、先构造 pending guard、再发 `AcquireOperation`**（design §3.1 的取消窗口论证）：
  ```rust
  async fn begin_operation(&self) -> Result<CoreOperationGuard> {
      let mut operation = CoreOperationGuard::pending(self.clone(), self.allocate_operation_id()?);
      operation.acquire().await?;   // 内部用 Some(CORE_ACQUIRE_TIMEOUT)
      Ok(operation)
  }
  ```
- `Drop for CoreOperationGuard` 用 **`cast`**（fire-and-forget）发 `ReleaseOperation { id }`——`Drop` 是同步的，不能 await；发送失败只意味着 actor 已终止，此时不存在需要让位的 active operation（design §3.3 原文）；
- 正常路径允许显式 `release().await`，`Drop` 只作兜底；
- `allocate_operation_id` 用 `fetch_add`，`0` 视为无效；溢出 → 进程生命周期内不可恢复的内部错误（design §3.1），**不持久化、不跨进程**；
- `status()` 用 `Some(CORE_READ_TIMEOUT /* 5s */)`，与四个既有 client 一致（A7f）；
- `Drop for CoreClientInner` 调 `actor_ref.stop(None)`（房规）。

### S7 — A3 兼容 seam 适配

在 `APP/client/core.rs` 里为 `CoreClient` 实现 `CoreLifecyclePort`，为一个包住 `CoreOperationGuard` 的适配器实现 `CoreLifecycleLease`。

约束（来自 A1f/A4f，**编译期硬约束，不可妥协**）：

- `CoreLifecycleLease: Send`（不要求 Sync）→ `CoreOperationGuard` 必须 `Send`；
- lease 以 `&mut dyn` 跨 4 处函数签名传递，并被 **move 进 `async move` 闭包**（`patch_running_config`）→ 适配器必须是 `Box<dyn CoreLifecycleLease>` 且可移动进 boxed future；
- `begin()` 返回的 `Box<dyn CoreLifecycleLease>` 拥有 guard，作用域结束即 drop → 自动 `ReleaseOperation`。这正好把"借用式 lease"与"RAII guard"对上，**不需要改任何调用点**。

五个 lease 方法在 5a 的路由（按 D3=A）：

| 方法                | 5a 路由                                                                    | 去向 |
| ------------------- | -------------------------------------------------------------------------- | ---- |
| `check_and_promote` | 维持现有实现（候选校验 + 原子 promote），核心检查改调 `CoreBackend::check` | B1   |
| `apply_candidate`   | 维持现有实现                                                               | B3   |
| `apply_promoted`    | 维持现有实现（`api::put_configs`）                                         | B3   |
| `restart`           | **改走** `CoreBackend::start`（对应 legacy `run_core_from`）               | —    |
| `stop`              | **改走** `CoreBackend::stop`                                               | —    |

`CoreLifecyclePort::status()` 在 5a **第一次有了生产实现**（A5f 此前无生产调用者），返回值由 `CoreStatusView` 投影成既有 `CoreStatusSnapshot`，**wire 与结构都不变**。

`on_profile_change()` 维持现有实现（连接中断服务，PR-6 范围）。

**顺序不变式（必须在代码注释里写明）：** A3f 已确认 `rebuild_gate` 在全部 10 处都先于 `core.begin()`，`patch_running_config` 是 `clash_patch_gate → rebuild_gate → begin()`。5a 引入 `OperationGate` 后变成**三层嵌套且全局顺序一致**，因此不产生死锁。B2 会把前两层吸收掉。

### S8 — 组合根接线

`APP/setup.rs:42-55`：把 `core: Arc::new(LegacyCoreBridge::new(runtime_paths))` 换成新建的 `CoreClient`。

- actor 的 spawn 位置：跟随四个既有 typed client 的房规，**在 `CoreClient::new()` 内部 `Actor::spawn`**（A2f）。`try_new_with_args` 是 sync fn + 内部 `block_on`，所以 `CoreClient::new()` 保持 async 即可自然嵌入；
- 启动模式：`RunType::classify(enable_service, get_ipc_state())`。按 D2 的顺序事实，此刻必然是 `Normal` → 先建 Local backend；
- `LegacyCoreBridge` 在 5a **删除**（它是 `CoreManager::global()` 的两个残留调用点之一，`core_bridge.rs:128,136`）；
- `core/service/ipc.rs:83-88`（IpcState 翻转后重启核心）改为：`SetBackend(mode)` + `Restart`，两者在**同一个** `CoreOperationGuard` 下顺序执行。

### S9 — 迁移直接调用点

按 A3 卡"start/stop/restart/status 改走 actor"，逐点替换（全部来自 `CoreManager::global()` 的生产命中）：

| 调用点                                                   | 现状                             | 5a 改为                                                                                                |
| -------------------------------------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `APP/ipc.rs:403` `get_core_status`                       | `CoreManager::global().status()` | `CoreClient::status()`，**wire 形状不变**（同一 `(Cow<CoreState>, i64, RunType)` 元组），删除该处 TODO |
| `APP/ipc.rs:503` `restart_sidecar`                       | `run_core()`                     | guard + `Restart`                                                                                      |
| `APP/ipc.rs:960,977,994` service start/stop/restart 命令 | `run_core()`                     | guard + `SetBackend` + `Restart`                                                                       |
| `APP/feat.rs:58`                                         | `run_core()`                     | guard + `Start`                                                                                        |
| `APP/feat.rs:292,385`                                    | `status()`                       | `CoreClient::status()`                                                                                 |
| `APP/core/service/ipc.rs:83,88`                          | `status()` + `run_core()`        | 见 S8                                                                                                  |
| `APP/utils/help.rs:268`                                  | `stop_core()`                    | `CoreClient::shutdown()`（见 S11）                                                                     |
| `APP/utils/resolve.rs:288`                               | `stop_core()`                    | guard + `Stop`                                                                                         |
| `APP/feat.rs:392` macOS DNS                              | `change_default_network_dns`     | **不动**（C3 范围）                                                                                    |
| `APP/core/updater/instance.rs:201`                       | `CoreManager::global()`          | **不动**（design §9 的单一 residual，owner = PR-6d）                                                   |

Updater residual 必须补一条标记：

```rust
// TODO(actor-migration): 保留的单一 core residual。
// Reason: Updater 的完整注入是 PR-6d；design §9 明确禁止为归零指标加 attach_core_port 半迁移桥。
// Remove when: PR-6d 把 UpdaterActor 接上 CoreClient。
```

### S10 — 删除 legacy 生命周期

- 删除 `CoreManager::lifecycle_lock` 字段、`begin_lifecycle()`、`CoreLifecycleLease<'a>`（legacy 的那个）以及所有 `*_with_lease` 变体（`APP/core/clash/core.rs:386-422,428,444-449,486-651`）；
- 删除 `recover_core()` 里的**裸线程递归重试**（`core.rs:567-585` 的 `sleep(5s)` + `std::thread::spawn` + 自调用）——这正是 design §5 禁止的第二层恢复；
- **保留**：`Instance`、`RunType`、`find_binary_path`、`change_default_network_dns`、`status()`（Updater residual 与 C3 仍需要）。

> 判定原则：只删"被 CoreActor 取代的排他与恢复机制"，不删"尚有 owner 的功能代码"。

### S11 — shutdown 接线

- `NyanpasuClient::shutdown()` 追加：rebuild worker 关闭**之后**调 `CoreClient::shutdown()`（发 `Shutdown` 消息 → 拒绝全部 waiters → 关 backend）；
- `APP/utils/help.rs:249-272` 的 `cleanup_processes` 顺序保持 `client.shutdown()` → widget stop → 停核，第三步由 `CoreManager::global().stop_core()` 改为已经在第一步完成（因为 CoreActor 的 shutdown 会关 backend），**该行删除**；
- 更新 `shutdown()` 的契约 doc comment（`client/mod.rs:392-401` 现在明写"不停 CoreManager globals"，5a 后不再成立）。

### S12 — 测试

全部 TempDir + barrier/RPC 同步，**零 sleep 断言**（A10f）。

#### A2 gate 测试（对应 A-Exit 六项）

| ID      | 名称                                                 | 断言                                                                                                                                                              | A-Exit 项         |
| ------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| T-OP-01 | `waiters_are_granted_in_fifo_order`                  | 三个 waiter 依次入队；释放 active 后按入队顺序放行（用 oneshot 记录放行次序）                                                                                     | FIFO              |
| T-OP-02 | `dropping_a_waiting_guard_cancels_it`                | waiter guard drop → 从 waiters 移除；后续 release 不会把它误认为 active                                                                                           | 等待取消          |
| T-OP-03 | `guard_dropped_right_after_grant_releases_active`    | 用 barrier 让 release 与 grant 竞争：guard 在刚被提升为 active 后 drop → active 清空且下一个 waiter 被放行                                                        | 刚获批取消        |
| T-OP-04 | `stale_release_is_idempotent_noop`                   | 对已完成/未知 ID 发 `ReleaseOperation` → 不影响当前 active，不 panic                                                                                              | stale release     |
| T-OP-05 | `mutation_with_wrong_id_returns_stale`               | 持 A 的 id 时用 B 的 id 发 `Restart` → `StaleOperation`，且 backend **零调用**（TestBackend 计数）                                                                | wrong-id mutation |
| T-OP-06 | `shutdown_drains_all_waiters`                        | 一个 active + 两个 waiter 时 shutdown → 两个 waiter 都收到错误，backend 收到 stop                                                                                 | shutdown drain    |
| T-OP-07 | `acquire_times_out_and_releases_the_waiter`（RQ-04） | 用 `tokio::time::pause()` 推进到 `CORE_ACQUIRE_TIMEOUT` → `begin_operation` 返回 `AcquireTimeout`，且 waiter 已从队列移除（后续 release active 时直接放行第三个） | RQ-04             |

#### A1 backend parity 测试

| ID      | 断言                                                                                                                                                                                 |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| T-BK-01 | `CoreBackend::{Local, Service}` 对 check/start/stop/restart/recover 的**成功路径**产生一致的 `CoreStatusView` 转换（Service 用本地 mock IPC endpoint，Local 用 TempDir + fake core） |
| T-BK-02 | `LocalBackend` 构造出的 `ManagerOptions.local_ipc_policy == LocalIpcPolicy::Disable`（显式化的回归钉）                                                                               |
| T-BK-03 | apply outcome 映射：7 个本地 `ApplyOutcome` 分支 → `CoreApplyData`，含 `DurabilityUncertain` 单层与**双层嵌套**（warning 以 `"; "` 拼接），`Noop` 不丢失                             |
| T-BK-04 | `local_error_kind` 对 12 个 `nyanpasu_ipc::api::error_kind` 常量的映射（断言用常量而非字面量）                                                                                       |
| T-BK-05 | `is_recovery_exhausted` 对上游前缀命中/不命中；`CoreStatusView.recovery_exhausted` 随之置位（D5）                                                                                    |

#### RQ-02 revision 测试

| ID      | 断言                                                                                                     |
| ------- | -------------------------------------------------------------------------------------------------------- |
| T-RV-01 | 三个刷新来源（status 查询 / 推送 / 操作返回）都能更新 `last_revision`，最后写入者赢                      |
| T-RV-02 | Service 重连后收到的第一个 `CoreStatusChanged` 快照直接覆盖旧 revision（模拟断线重连，不发额外对账 RPC） |
| T-RV-03 | `RevisionId → RevisionIdInfo` 转换保字段（epoch / generation / effective_hash），`runtime_path` 被丢弃   |

#### seam 回归

| ID      | 断言                                                                                                                                                                                         |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-SM-01 | 既有 `client/mod.rs` 与 `rebuild.rs` 的全部 lease 测试在 6 个替身更新后**继续通过**（A9f 列的替身逐个适配，不改测试语义）                                                                    |
| T-SM-02 | `rebuild.rs:930-1000` 的 `s04_concurrent_restart_waits_until_change_core_rollback_completes` 在三层嵌套（clash_patch_gate → rebuild_gate → OperationGate）下仍然通过——这是顺序不变式的回归钉 |

**Local backend 测试的 fake core 问题（事实 R17）。** 两侧的 fake core 都**不能**靠 `CARGO_BIN_EXE_*` 拿到：

- `nyanpasu-core-manager` 的 fake core 是**它自己 package 的 `[[bin]]`**，`CARGO_BIN_EXE_*` 只在同 package 内可见，app 侧取不到（R17）；
- 本仓的 `backend/fake-core` 对 `backend/tauri` 是 **dev-dependency**，而 dev-dependency **既不构建该 binary 也不设置 `CARGO_BIN_EXE_fake-core`**——这一点在 `backend/tauri/Cargo.toml:275-280` 有明确注释。既有做法是**预构建 + 运行时定位**：`cargo build -p fake-core`（或 `cargo test -p fake-core`），然后 `fake_core::require_bin_path()` 按 `current_exe` 的 profile/triple 查找，支持非空 `NYANPASU_FAKE_CORE` 覆盖，最后回退 target 目录（`backend/fake-core/src/lib.rs:399-418`）。现成消费者示例：`APP/client/process_core_bridge.rs:18-20`。

**因此：** 若 S12 要用真实 `LocalBackend` 驱动真实 `CoreManager`，必须沿用同一套预构建 + `require_bin_path()` 流程，并在计划的验证命令里显式加上 `cargo build -p fake-core` 前置步骤。

**取舍（S12 开工时先花 5 分钟实测，不要盲写）：** 本仓 fake-core 是为 legacy `Instance` 的进程矩阵设计的，未必满足 `nyanpasu-core-manager` 的 readiness ack 协议与 `-t` 检查语义（R13 的 Supervisor 用 `ReadinessProbe::Acknowledged`）。**若不满足，就不要为它改造 fake-core**——改用 `TestBackend` 覆盖 actor 层语义，把真实 manager 的行为留给 runtime 自己的测试（`RT/crates/nyanpasu-core-manager/tests/`，那里已有完整覆盖）。T-BK-01 的 parity 断言相应降级为"两个 backend 对同一 `CoreRequest` 产生同构的 `CoreStatusView` 转换"，用 mock/Test 双端验证，而非拉起真实进程。

### S13 — 门禁

```powershell
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
pnpm test:backend
git diff --stat frontend/interface/src/ipc/bindings.ts   # 期望：空
pnpm lint:ts
pnpm architecture-ledger
pnpm lint:architecture-ledger
```

**bindings 预期：无变化。** 5a 不改任何 wire（`get_core_status` 保持元组形状，S9）。若 bindings 有 diff → 说明范围溢出，停下核查。

**ledger 预期变化（必须逐条核对后再 `--write-snapshot`）：**

| 指标                                       | 方向                | 原因                                                                                                         |
| ------------------------------------------ | ------------------- | ------------------------------------------------------------------------------------------------------------ |
| `service_globals["CoreManager::global()"]` | **下降**（当前 18） | S9/S10 迁移与删除；Updater residual 保留 ≥1                                                                  |
| `migration_markers`                        | 净变化需解释        | S4 加 1（R0 过渡）、S9 加 1（Updater residual）、S10 删除 `core_bridge.rs:125-127` 与 `core.rs:557-559` 两条 |
| `config_calls`                             | 不应上升            | 新代码禁止调 `Config::*()`；依赖全部注入                                                                     |
| `test_real_dirs`                           | **必须仍为 0**      | 新测试只用 TempDir + 注入的 `RuntimePaths`                                                                   |
| `bridgeFiles`                              | 减少 1              | `core_bridge.rs` 若整文件删除则移除；若保留 `RunningConfigPatchPort`（B3 才删）则**保留**                    |

> `config_calls` 或 `test_real_dirs` 变差 → **回头改代码，不要靠改 snapshot 掩盖。**

---

## 5. Exit 判据映射

task.md A-Exit 三条：

| Exit                                                                                              | 交付步骤    | 验证                                                                                                                                |
| ------------------------------------------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| operation 测试：FIFO / 等待取消 / 刚获批取消 / stale release / wrong-id mutation / shutdown drain | S5、S6、S12 | T-OP-01…06 全绿                                                                                                                     |
| Local/Service 基本生命周期 parity 测试                                                            | S3、S12     | T-BK-01…05 全绿                                                                                                                     |
| legacy core 生命周期不再被新调用点使用                                                            | S9、S10     | `rg 'begin_lifecycle\|lifecycle_lock\|_with_lease' backend/tauri/src` 为 0；`CoreManager::global()` 仅剩 Updater residual（带标记） |

roadmap §6.1 附加项：

| §6.1 判据                                          | 对应                                                                   |
| -------------------------------------------------- | ---------------------------------------------------------------------- |
| 封闭 enum，不定义 `CoreEngine` trait/factory       | S3；`rg 'CoreEngine\|EngineFactory' backend/tauri/src` 为 0            |
| `LocalIpcPolicy::Disable` 显式写出                 | S3 + T-BK-02                                                           |
| 禁用 `service_default()`                           | S3；ServiceBackend 用 `Client::new`                                    |
| client 预分配 `OperationId` + pending guard        | S6 + T-OP-03/07                                                        |
| 不实现 TTL / auto-steal / watchdog                 | S5；`rg 'ttl\|auto_steal\|watchdog' backend/tauri/src/core/actor` 为 0 |
| actor 无第二层恢复，只发 `core_recovery_exhausted` | S3(D5) + S10 + T-BK-05                                                 |
| A3 兼容 seam 保留，旧 trait 名不扩散               | S7；新代码里 `CoreLifecycle*` 只出现在适配 impl 中                     |
| RQ-02 / RQ-04 已作答                               | 本计划 §2                                                              |

---

## 6. 风险与回滚

| 风险                                                       | 概率 | 影响                                  | 缓解                                                                                                                                                         |
| ---------------------------------------------------------- | ---- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `CoreManager::new()` 的运行目录独占锁与 daemon 冲突        | 中   | Service 模式下建 Local backend 会失败 | D2=A 只建当前模式匹配的那个；且 app 与 daemon 的 runtime_dir 本就不同（daemon 用 `service_data_dir`）。**S3 开工时先写一个断言两路径不相等的测试**，不要假设 |
| lease 被 move 进 `async move` 闭包导致 guard 不满足 `Send` | 中   | 编译失败                              | A4f 已定位为编译期硬约束；`CoreOperationGuard` 的字段（`OperationId` + `CoreClient` + `bool`）全部 `Send`，`ActorRef` 亦然                                   |
| 三层嵌套锁引入死锁                                         | 低   | 挂起                                  | A3f 证明全局顺序一致（10/10 处 `rebuild_gate` 先于 `begin`）；T-SM-02 作回归钉                                                                               |
| 6 个 lease 测试替身适配引发大面积测试改动                  | 高   | diff 变大、review 困难                | 适配只改**构造**不改**语义**；`MockRunningCoreBridge` 的 4 方法 mockall 面保持不变，只在 lease 侧多包一层 guard                                              |
| 上游"重启预算耗尽"字符串前缀漂移                           | 中   | `recovery_exhausted` 静默失效         | D5 集中在一处 + T-BK-05 钉住；**并建议把 typed 变体并入 R0 的上游 PR**（需 leader 裁定）                                                                     |
| 本地 clippy 假红（共享 target kache 污染）                 | 中   | 误判                                  | 已知问题：用独立 `--target-dir` 复验再下结论                                                                                                                 |
| `apply` 实现但不接线被审查判为投机代码                     | 中   | review 争议                           | D4 已把 A1 卡的字面要求与 CLAUDE.md §2 的张力摆到台面上，请 leader 裁定                                                                                      |

**回滚：** 改动集中在新增目录 `APP/core/actor/`、新增 `APP/client/core.rs`、以及 `setup.rs` / `ipc.rs` / `feat.rs` / `utils/{help,resolve}.rs` / `core/clash/core.rs` / `core/service/ipc.rs` / `client/{mod,core_bridge}.rs` 的定点修改。S1–S6 可独立成一个 commit（新增代码，零调用点改动，可编译可测），S7–S11 为第二个 commit（接线与删除）。第一个 commit 单独回滚不影响生产路径。

---

## 7. 提交切分建议

1. `feat(core): add CoreBackend enum and cancellation-safe OperationId protocol` —— S1–S6 + S12 的 T-OP/T-BK/T-RV（纯新增，生产路径未变）；
2. `refactor(core): own the core lifecycle in CoreActor` —— S7–S11 + T-SM 回归 + S13。

---

## 8. 明确 out-of-scope（登记去向）

| 项                                                              | 去向                                                             |
| --------------------------------------------------------------- | ---------------------------------------------------------------- |
| typed `CoreErrorKind` 消费（替换 S4 的过渡映射）                | R0 合并 + submodule bump 之后的**独立一步**；bump 本身待用户授权 |
| `StopReason::RestartBudgetExhausted` typed 变体                 | 建议并入 R0 的上游 PR，**待 leader 裁定**（D5 附带建议）         |
| apply 管线统一到 `CoreBackend::apply`                           | **PR-5b / B3**（D3=A）                                           |
| Promoted / Applied 入 actor、删 `RuntimeLifecycleStore`         | **PR-5b / B1**                                                   |
| 删 `rebuild_gate` / `clash_patch_gate`                          | **PR-5b / B2**                                                   |
| `change_core` 简化为 commit-first                               | **PR-5b / B4**                                                   |
| post-commit 失败矩阵（RQ-01）、apply parity 含 `Noop`（RQ-03）  | **PR-5b 计划**                                                   |
| watch snapshot 投影、100 条 `LogFrame` ring、删 `Logger` global | **PR-5c / C1**                                                   |
| `set_mode` / `reconcile_mode`、删 5 s 轮询线程与 statics        | **PR-5c / C2**                                                   |
| macOS DNS 归入 actor（`MacosDnsGuard`）                         | **PR-5c / C3**                                                   |
| Updater 的 `CoreManager::global()`                              | **PR-6d**（design §9 允许的单一 residual）                       |
| `clash-api` workspace 条目                                      | 无 app 侧消费者，暂不加（D1=A）                                  |
