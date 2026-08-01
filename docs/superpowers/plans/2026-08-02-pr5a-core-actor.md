# PR-5a 实施计划 — 最小 CoreActor + `OperationId` + `CoreBackend` enum

**日期：** 2026-08-02
**分支基线：** `refactor/core-manager-actor` @ `4583048b5`（含 PR-5-pre 三提交：`4f22eaddb` 依赖切换 / `cca7f654f` 兼容门 / `4583048b5` ledger 同步）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/design.md` §3–§6、同目录 `task.md` 卡 A1/A2/A3
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.1；必答项 §6.4 RQ-02 / RQ-04
**平台：** Windows 11 / PowerShell
**版本：** v2（2026-08-02）——v1 经 codex 对抗审查 **REJECT**（3 High + 5 Medium + 1 Low），本版逐条修订

---

## 0.1 审查处置表（v1 → v2）

九条发现**全部经本人复核源码确认成立**，逐条修订如下。另有一条审查未发现、由本人自查发现的错误（#10）。

| #   | 级别   | 问题                                                                          | 处置                                                                                                                                 | 落点                                    |
| --- | ------ | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------- |
| 1   | High   | Updater 依赖 S10 要删的 lease API → **必然编译失败**                          | 改用注入的 `CoreClient` + guard 迁移 `replace_core`；不加全局桥；UpdaterActor 仍归 6d                                                | 事实 A16f；**S9.1**；§0；§8             |
| 2   | High   | `start` / `restart` 都不是 request-bearing 原语，全新 backend 上 restart 无效 | 收敛为**单一 `run(request)`** 迁移原语；Local 用 `switch(spec)` 一次调用覆盖两种情形，Service 用 stop-then-start                     | 事实 R5b/R5c、DV-F；S3、S5、S7、S8      |
| 3   | High   | 组合根注入图不可实现（`core` 在 `block_on` 前就被解构）                       | `CoreClient` 在 typed 快照后于 block 内构造；typed 字段入 `NyanpasuClientInner`；`args.core` 改 `Option`；两个无注入点消费者显式穿线 | 事实 A17f；**S8** 全节重写              |
| 4   | Medium | `events()` 不重连；过期 `/status` 可回退 revision；start 不返回 revision      | actor 自管可取消重连循环 + **generation 栅栏** + start 后同步回填                                                                    | **RQ-02** 重写；T-RV-02/04/05           |
| 5   | Medium | `shutdown()` 不释放目录锁，换槽会拿不到锁                                     | 槽位改 `Option`；六步换槽协议（取消→join→shutdown→drop 全部 `Arc`→构造→写回）；定义无 backend 失败态                                 | 事实 R6b；**D2** 重写；T-BK-08/09       |
| 6   | Medium | 漏第二条裸线程恢复；DNS 是第二个 residual；`resolve_reset` 已先停一次核       | S10 删两条路径；DNS **allowlist 到 5c**（leader 裁定）；`resolve_reset` 移除停核（已验证仅一个调用者）                               | 事实 A18f/A20f；S9.2、S10、**S11** 重写 |
| 7   | Medium | 只算 flag，未满足 design §5 的"发布一次"                                      | 新增窄 `CoreDegradationSink`（单方法、可 mock）+ per-episode latch + 复位规则                                                        | **D5** 重写；T-BK-06/07                 |
| 8   | Medium | parity 允许降级为 TestBackend，等于架空 A-Exit                                | 承诺真实双端：fake-core 补 `GET /version`（或新增探针 bin）+ 真实 IPC roundtrip harness；**禁止降级**                                | 事实 A19f；**S12.1 / S12.2**            |
| 9   | Low    | `ConfigRevision` 字段描述、seam 覆盖遗漏、DV-B 指错                           | T-RV-03 拆成 03a/03b 两个转换；A9f 补第 4 个 seam 实现；DV-B 改指 S4                                                                 | A9f、DV-B、T-RV-03a/03b                 |
| 10  | —      | **（自查）** S13 的 ledger 基线用了 5-pre **之前**的快照数字                  | 改用 2026-08-02 实测基线 116/74/19/300/0，并给出逐项加减预期（19 → 17）                                                              | **S13**                                 |

> 对 #9 的一点澄清：审查说"`ConfigRevision` 比 `RevisionId` 多了 `source_hash` 和 `runtime_path`"——这是拿 `ConfigRevision` 与 `RevisionId` 比。原文 RQ-02 里"多一个 `runtime_path`"是拿 `ConfigRevision` 与 **`ConfigRevisionInfo`** 比，那句本身成立。**但结论一致**：原 T-RV-03 把两个不同的转换混成一句，断言不成立，必须拆开。已按建议拆分。

---

## 0. 本阶段的边界

**做（= task.md A1/A2/A3）：**

1. 封闭 `CoreBackend` enum，`Local` 包装 `nyanpasu_core_manager::CoreManager`，`Service` 持有实例化的 `nyanpasu_ipc::client::Client`；
2. 取消安全的 `OperationId` / `OperationGate` / `CoreOperationGuard`，client 侧预分配 ID；
3. `CoreClient` 通过既有 `CoreLifecyclePort` / `CoreLifecycleLease` seam 接入，组合根注入，start/stop/restart/status 改走 actor；
4. 删除 legacy `CoreManager::lifecycle_lock` 与**两条**裸线程递归 recover；
5. **把 Updater 的核心依赖显式化**（`replace_core` 改用注入的 `CoreClient` + operation guard）——这不是范围扩张，是 S10 删除 legacy lease API 的**编译必要条件**，见 S9.1。

**不做（越界即返工）：**

- 不改 apply 管线语义（`check_and_promote` / `apply_candidate` / `apply_promoted` 的**实现路径**保持现状，见决策点 D3）；
- 不动 `rebuild_gate` / `clash_patch_gate` / `RuntimeLifecycleStore` / `publish_promoted` / `publish_applied` / `restore_promoted`（全部是 B1/B2/B3 的范围）；
- 不改 `change_core` 的编排与回滚（B4）；
- 不删除 `RunningConfigPatchPort` / `LegacyRunningConfigPatchBridge`（B3）；
- **不做 UpdaterActor**，也不加 `attach_core_port` 全局桥——S9.1 只把 core 依赖显式注入，`UpdaterManager::global()` 仍是 PR-6d 的 residual；
- **不迁移 macOS DNS**（`feat.rs:392`）——allowlist 到 PR-5c / C3，见 S9.2；
- 不做日志 ring / watch 投影 / `set_mode` / `reconcile_mode`（C1–C2）；
- 不删除 5 s 健康轮询线程与 `IPC_STATE` static（C2）；
- 不改 `get_core_status` 的 wire 形状（C1 才做 additive 扩展）。

---

## 1. 已核验事实

> 全部为本次会话直接读源码所得。`nyanpasu-runtime` submodule 确认在 tag `v2.0.0-rc.1`（`git -C backend/nyanpasu-runtime describe --tags HEAD`）。
> 下表 `RT/` = `backend/nyanpasu-runtime/`，`APP/` = `backend/tauri/src/`。

### 1.1 runtime 侧（`nyanpasu-core-manager` @ v2.0.0-rc.1）

| ID  | 事实                                                                                                                                                                                                                                                                                                                                                                                                       | 锚点                                                                                                               |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| R1  | 构造是 **async** 且 **runtime_dir 必填**：`pub async fn new(options: ManagerOptions) -> Result<Self, Error>`；缺 `runtime_dir` 返回 `Error::InvalidManagerOptions("runtime_dir is required")`                                                                                                                                                                                                              | `RT/crates/nyanpasu-core-manager/src/manager/mod.rs:212`、`:218-221`                                               |
| R2  | **构造会取运行目录独占锁**，失败返回 `Error::RuntimeDirectoryOwned(path)`；同时校验 controller 模板、拒绝零超时、清扫孤儿 epoch、拉起 JSONL log sink                                                                                                                                                                                                                                                       | `manager/mod.rs:223`、`:229-274`                                                                                   |
| R3  | `ManagerOptions::default()` 的 `local_ipc_policy` **已经是 `LocalIpcPolicy::Disable`**（`spec.rs:114`，`spec.rs:154-162` 有单测钉住）。A1 要求的"显式写出"是可审性要求，不是行为修复                                                                                                                                                                                                                       | `RT/crates/nyanpasu-core-manager/src/spec.rs:65-128`                                                               |
| R4  | **`CoreManager` 不是 `Clone`**（`struct CoreManager { inner: Arc<Inner> }`，未派生 Clone）→ app 侧必须 `Arc<CoreManager>`                                                                                                                                                                                                                                                                                  | `manager/mod.rs:87-89`                                                                                             |
| R5  | 生命周期方法全部 `&self` + async，错误类型 `nyanpasu_core_manager::Error`：`start(InstanceSpec)`、`stop()`、`check_config(&InstanceSpec)`、`shutdown()`、`restart() -> SwitchOutcome`、`switch(InstanceSpec) -> SwitchOutcome`                                                                                                                                                                             | `manager/mod.rs:319,438,509,541`；`manager/switching.rs:45,52`                                                     |
| R5b | **`switch(spec)` 同时覆盖"未跑"与"在跑"两种情形**，是唯一 request-bearing 的运行迁移原语：`switch_locked` 先判 running；**不在跑** → 清理 stale epoch 后 `start_locked(ctrl, spec)`，返回 `SwitchOutcome::Hard { reason: NotRunning }`；**在跑** → 按能力选 graceful（零停机）或 hard switch。这与 legacy `run_core_with_lease` 的"在跑就先停再按请求启动"语义**完全对应**，且更优                         | `manager/switching.rs:52,58-103`                                                                                   |
| R5c | 反之 `start(spec)` 在核心已在跑时**直接返回 `Error::AlreadyRunning`**；`restart()` 依赖 `ctrl.last_spec`，全新 backend 上返回 `Error::NotStarted`。二者都**不能**充当"按请求运行"原语                                                                                                                                                                                                                      | `manager/mod.rs:319-328`；`manager/switching.rs:45-49`                                                             |
| R6  | **没有 `recover()`**；实际名字是 `recover_quarantine() -> Result<(), Error>`，语义是"清除 quarantine 闩锁"，不是"重启核心"                                                                                                                                                                                                                                                                                 | `manager/quarantine.rs:18`                                                                                         |
| R5d | **wire `CoreState` 是有损两值投影**：上游 doc 原文"it reports `Starting` and `Restarting` as `Stopped(None)`, so a crash loop is indistinguishable from a real stop"。**忠实视图只在 `CoreStateDetail` 里**（`Stopped` / `Starting` / `Running` / `Restarting` / `Switching` / `Stopping`），经 `CoreInfos.detail` 送达。因此 Service 的"是否在跑"判定**必须读 `detail`**，读 `state` 会把过渡态误判成已停 | `RT/nyanpasu_ipc/src/api/status.rs:107-122,133-141`                                                                |
| R6b | **`shutdown()` 不释放运行目录锁**。锁是 `RuntimeDirectoryLock { _lock: atomic_fs::DirLock }`（文件系统目录锁），作为 `_runtime_lock` 字段挂在 `Inner` 上并**声明在最后**（注释："so ordinary Inner destruction drops instances/tasks before releasing directory ownership"）。因此它只在**最后一个 `Arc<Inner>` 被 drop 时**才释放                                                                         | `manager/mod.rs:118-124`；`config/runtime_store.rs:24-26,338-340`                                                  |
| R7  | `apply_config(&self, input: InstanceSpec, expected_revision: Option<RevisionId>) -> Result<ApplyOutcome, Error>`——参数是**整个 `InstanceSpec` 按值**，不是配置路径；要求核心已在跑，否则 `Error::NotStarted`                                                                                                                                                                                               | `manager/apply.rs:19-23`、`:26-29`                                                                                 |
| R8  | CAS 失败返回 `Error::RevisionConflict { expected: RevisionId, actual: Option<RevisionId> }`，且**不应用任何东西**                                                                                                                                                                                                                                                                                          | `manager/apply.rs:30-38`；`src/error.rs:43-47`                                                                     |
| R9  | `RevisionId { epoch: u64, generation: u64, effective_hash: String }`，**不是 `Copy`**；由 `ConfigRevision::id()` 取得，`ConfigRevision` 挂在 `CoreStatus.revision: Option<ConfigRevision>` 上；**没有 `CoreManager::revision()` 访问器**                                                                                                                                                                   | `src/state.rs:143-167`、`:189`                                                                                     |
| R10 | 状态与日志订阅：`subscribe() -> watch::Receiver<CoreStatus>`、`subscribe_logs() -> broadcast::Receiver<Arc<LogFrame>>`（容量 256，可在首次 `start()` 前调用）、`status() -> CoreStatus`                                                                                                                                                                                                                    | `manager/mod.rs:292,296,308`；`src/log.rs:16`                                                                      |
| R11 | 本地 `ApplyOutcome` 有 7 个分支：`Noop` / `Patched` / `Reloaded` / `Restarted` / `Switched` / `RolledBack{failed_apply}` / `DurabilityUncertain{outcome, warning}`。**`Warning` 是包装器不是 outcome**（印证 RQ-03）                                                                                                                                                                                       | `manager/mod.rs:57-85`                                                                                             |
| R12 | 供抄写的 outcome 映射参考实现：`map_apply_outcome`（把 `DurabilityUncertain` 拆成 `CoreApplyData.warning`，可嵌套两层、用 `"; "` 拼接）                                                                                                                                                                                                                                                                    | `RT/crates/nyanpasu-service-runtime/src/server/manager_bridge.rs:605-639`                                          |
| R13 | **manager 自身已做有界重启 + 指数退避**（委托 `nyanpasu_utils::process::Supervisor`）：`InstanceOptions.restart_policy` 默认 `OnFailure{max_restarts: 5}`、`backoff` 默认 `exponential(1s, 30s).with_jitter()`；另有**不可通过 `InstanceOptions` 配置**的 storm guard（默认 5 次/5 分钟）                                                                                                                  | `src/spec.rs:31-50`；`src/instance.rs:229-237`；`RT/crates/nyanpasu-utils/src/process/supervisor.rs:36-55,431-466` |
| R14 | **恢复耗尽没有 typed 信号**——唯一机器可读线索是 `CoreState::Stopped { reason: Some(StopReason::Error(msg)) }` 且 `msg` 以 `"core kept crashing; restart budget exhausted\n"` 开头；上游自己的测试也在字符串匹配                                                                                                                                                                                            | `src/instance.rs:631-641`；`RT/.../tests/instance_lifecycle.rs:246-252`                                            |
| R15 | 第二个不可恢复闩锁是 **quarantine**：`Error::StopUnconfirmed` → `latch_quarantine`，此后所有受门操作（start/switch/restart/apply）都被拒，直到 `recover_quarantine()` 成功。`stop()`/`shutdown()` 故意绕过该门                                                                                                                                                                                             | `manager/quarantine.rs:8-13`；`manager/mod.rs:434-437,537-540`                                                     |
| R16 | **`Error::kind()` 在本 tag 不存在**（整个 crate 没有 `impl Error` 块）。可用的机器可读分类是：ipc 侧的 `nyanpasu_ipc::api::error_kind` 字符串常量表 + 一个**私有**映射函数 `map_error_kind`（位于 `nyanpasu-service-runtime`，app 不依赖该 crate）                                                                                                                                                         | `RT/nyanpasu_ipc/src/api/mod.rs:38-66`；`manager_bridge.rs:646-665`                                                |
| R17 | 可抄的测试脚手架：`tests/common/mod.rs` 的 `fast_options()`（`max_restarts: 2`、50ms→200ms backoff、50ms 健康间隔）、`wait_for_state()`、`wait_for_health()`、`utf8_tempdir()`。fake core 是**该 crate 自己的 `[[bin]]`**，app 侧**拿不到** `CARGO_BIN_EXE_*`                                                                                                                                              | `RT/crates/nyanpasu-core-manager/tests/common/mod.rs:18-147`；`Cargo.toml:12-15`                                   |

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

| ID   | 事实                                                                                                                                                                                                                                                                                                                                                                                                          | 锚点                                                                                                                                 |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| A1f  | 既有 seam：`CoreLifecyclePort { begin() -> Box<dyn CoreLifecycleLease>, status() -> CoreStatusSnapshot, on_profile_change() }`；`CoreLifecycleLease: Send`（**不是 Sync**）有 5 个方法 `check_and_promote` / `apply_candidate` / `apply_promoted` / `restart` / `stop`                                                                                                                                        | `APP/client/core_bridge.rs:43-74`                                                                                                    |
| A2f  | 组合根在 `setup.rs:42-55`，`core: Arc::new(LegacyCoreBridge::new(runtime_paths))` 从这里注入；`NyanpasuClient::try_new_with_args` 是 **sync fn**，内部 `tauri::async_runtime::block_on`；四个 typed actor 各自在自己的 `Client::new()` 里 `Actor::spawn`                                                                                                                                                      | `APP/setup.rs:42-61`；`APP/client/mod.rs:255-332,273`                                                                                |
| A3f  | **`begin()` 共 10 个生产调用点**，`rebuild_gate` 在**全部 10 处都先于** `core.begin()` 获取；`patch_running_config` 是最严格的三层顺序 `clash_patch_gate → rebuild_gate → begin()`                                                                                                                                                                                                                            | `APP/client/mod.rs:1231,1276,1351-1353,1413,1429`；`APP/client/rebuild.rs:232,257,268,282,406`                                       |
| A4f  | lease 以 **`&mut dyn`** 跨函数传递（4 处签名），并且 `patch_running_config` 把 lease **move 进 `async move` 闭包**交给 `feat::patch_clash_with_rebuild` → guard 必须 `Send` 且可移动进 boxed future                                                                                                                                                                                                           | `APP/client/mod.rs:1285-1291,1371-1400,1436-1439,1452-1459`；`APP/client/rebuild.rs:239-242`                                         |
| A5f  | `CoreLifecyclePort::status()` **没有生产调用者**；生产状态读取仍直连 `CoreManager::global().status()`                                                                                                                                                                                                                                                                                                         | `APP/ipc.rs:403`；`APP/feat.rs:292,385`；`APP/core/service/ipc.rs:83`                                                                |
| A6f  | `CoreLifecycleLease::stop()` **没有生产调用者**；生产停核走 `CoreManager::global().stop_core()`                                                                                                                                                                                                                                                                                                               | `APP/utils/help.rs:268`；`APP/utils/resolve.rs:288`                                                                                  |
| A7f  | typed actor client 房规：`Clone` via `Arc<…Inner>`；每个 client 一个手写 `call` helper，把 `CallResult::{SenderError, Timeout}` 映射成显式错误；**读用 `Some(5s)`，写用 `None`**；`Drop for …Inner` 调 `actor_ref.stop(None)`                                                                                                                                                                                 | `APP/client/application.rs:17-162`                                                                                                   |
| A8f  | 测试注入钩子已存在：`test_client_args_with_lifecycle(dir, core: Arc<dyn CoreLifecyclePort>) -> ClientSetupArgs`——**5a 直接复用**，无需新建测试图                                                                                                                                                                                                                                                              | `APP/client/mod.rs:2087-2106`                                                                                                        |
| A9f  | 需要更新的 lease 测试替身共 6 个：`MockRunningCoreBridge`/`MockCoreLease`、`TestCorePort`/`TestCoreLease`、`CompensationLease`、`BarrierCompensationLease`、`BarrierCore`/`BarrierLease`，外加 trait 上的 `#[cfg_attr(test, mockall::automock)]`。**另有第 4 个 `CoreLifecyclePort` 生产形态实现** `ProcessCoreLifecycleAdapter`（S09 的进程背书 test-only adapter），seam 若改签名它也必须跟着改             | `APP/client/mod.rs:1589-1919`；`APP/client/rebuild.rs:814-928`；`core_bridge.rs:53`；**`APP/client/process_core_bridge.rs:206,256`** |
| A16f | **Updater 直接依赖 S10 要删的 legacy lease API**：`replace_core()` 里 `CoreManager::global()` → `begin_lifecycle()` → `lifecycle.stop_core()` → `lifecycle.run_core_from(product)`。不迁移它则 5a **无法编译**；且即便能编译，legacy 单例也停不掉 CoreActor 拥有的新 manager 实例                                                                                                                             | `APP/core/updater/instance.rs:201,205,216,279`                                                                                       |
| A16g | **Updater 的真实构造链是四段**：`ipc::update_core(core_type)` → `UpdaterManager::global().write().await.update_core(&core_type)` → `instance::UpdaterBuilder::new().set_*(..).build().await` → `Updater`。**类型名是 `Updater`，不是 `UpdaterInstance`**（v2 计划写错了）；`UpdaterBuilder` 现有 5 个 `Option` 字段：`client` / `core_type` / `mirror` / `artifact` / `tag`                                   | `APP/ipc.rs:639-646`；`APP/core/updater/mod.rs:222-244`；`APP/core/updater/instance.rs:31-38,51-58`                                  |
| A21f | `spawn_health_check()` 的调用者共 **4 处**：`core/service/control.rs:97`（install）、`:225`（start）、`:320`（restart），以及 `core/service/mod.rs:32`（`init_service`）。v2 计划只列了最后一处                                                                                                                                                                                                               | 上述四处                                                                                                                             |
| A22f | `ClientSetupArgs` 的**字面构造点共 5 处**（改字段时全部要动）：`setup.rs:42`（生产）、`bridge/verge.rs:1072`、`client/mod.rs:2093`、`client/mod.rs:2325`、`client/rebuild.rs:955`（后四处为测试）                                                                                                                                                                                                             | 上述五处                                                                                                                             |
| A23f | **`Degradation` 是 mutation 响应体上的字段，不是推送通道**：前端在 `provider/index.tsx:46-53` 拦截 mutation 结果里的 `degradations` 数组再交给 handler。`Message` 枚举只有 `SetConfig(Result<(), String>)` 一个变体（`core/handle.rs:25-27`），也不适合承载它。`DegradationPhase` 的 10 个变体里**没有**核心生命周期相关项；前端 `localizeDegradationPhase` 有 `default` 分支，故新增变体不会打断 TS 类型检查 | `APP/client/runtime.rs:446-470`；`frontend/interface/src/provider/index.tsx:46-53`；`frontend/nyanpasu/src/pages/__root.tsx:123-145` |
| A24f | fake-core 的长驻启动**强制要求 `FAKE_CORE_READY_ADDR`**（TCP ready/release barrier）：doc 原文 "Long-running start requires `FAKE_CORE_READY_ADDR`… Without either `START_EXIT` or a barrier, the process fails fast with exit 2"。HTTP 端口由 `FAKE_CORE_HTTP_PORT` 决定。**只加 `GET /version` 路由不足以让它被 manager 拉起来**                                                                            | `backend/fake-core/src/main.rs:13-18,137-147`；`backend/fake-core/src/lib.rs:144,147`                                                |
| A17f | **组合根注入图不可实现（如原 S8 所写）**：`try_new_with_args` 在**进入 `block_on` 之前**就把 `core` 从 `ClientSetupArgs` 解构出来；而 typed `application` client（`enable_service_mode` 的来源）在 block 内部第 279 行才被创建。因此 `setup.rs` 无法构造一个依赖 typed 快照的 `CoreClient` 再传进来                                                                                                           | `APP/client/mod.rs:255-264,273,279`；`APP/setup.rs:42-55`                                                                            |
| A18f | **`cleanup_processes` 的真实顺序**是 `save_window_state` → **`resolve_reset()`（内部已经停核）** → `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`（第二次停核）。`resolve_reset` 全仓**只有这一个调用者**                                                                                                                                                                            | `APP/utils/help.rs:249-271`；`APP/utils/resolve.rs:286-289`                                                                          |
| A19f | **本仓 fake-core 无法满足 manager 的默认就绪探针**：manager 默认 readiness 是 `ControllerVersionProbe`——"healthy iff `GET /version` succeeds"；而 fake-core 的内置 HTTP 只对**精确** `PUT /configs` / `PATCH /configs` 作答，其余一律 404，且长驻启动强制要求 `READY_ADDR` barrier                                                                                                                            | `RT/crates/nyanpasu-core-manager/src/health/probe.rs:109-125`；`backend/fake-core/src/main.rs:22-23,137-147,305-344`                 |
| A20f | 第二条**裸线程递归恢复**路径在 `Instance` 事件循环内（核心异常退出且 `tx.send` 失败时 `std::thread::spawn` → `CoreManager::global().recover_core()`），与 `recover_core()` 自身的 5 s 重试是**两处独立**的实现                                                                                                                                                                                                | `APP/core/clash/core.rs:228-238`（另一处 `:577-582`）                                                                                |
| A10f | 测试全程**零 sleep**：oneshot barrier / `AtomicUsize` / `mockall::Sequence` / `tokio::time::pause()` / `Notify`。5a 必须延续                                                                                                                                                                                                                                                                                  | `APP/client/rebuild.rs:930-1000`                                                                                                     |
| A11f | `NyanpasuClient::shutdown()` 目前**只**关 rebuild worker；生产顺序是 `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`                                                                                                                                                                                                                                                                  | `APP/client/mod.rs:392-404`；`APP/utils/help.rs:249-272`                                                                             |
| A12f | `RunType::classify(enable_service, ipc_state)` 已由 PR-5-pre 抽出为纯函数；`IpcState` 只由 `health_check()` 翻转，兼容门在 `target_ipc_state()`                                                                                                                                                                                                                                                               | `APP/core/clash/core.rs:50-67`；`APP/core/service/ipc.rs:143`                                                                        |
| A13f | `Instance::try_new` 展示了构造核心进程所需的全部输入：core_type、app_data_dir、binary（`find_binary_path`）、config_path、pid_path                                                                                                                                                                                                                                                                            | `APP/core/clash/core.rs:83-124,711-728`                                                                                              |
| A14f | `ractor = "0.16"`；`nyanpasu-core-manager` / `nyanpasu-core-metadata` **当前不在** workspace 依赖里（PR-5-pre 的 D1=A 推迟到本阶段）                                                                                                                                                                                                                                                                          | `backend/tauri/Cargo.toml:63`；`backend/Cargo.toml:27-41`                                                                            |
| A15f | 本仓 `backend/fake-core` 对 `backend/tauri` 是 **dev-dependency**，因此**既不构建该 binary 也不设置 `CARGO_BIN_EXE_fake-core`**。既有做法是预构建（`cargo build -p fake-core`）+ 运行时定位 `fake_core::require_bin_path()`（current_exe profile/triple 查找 → 非空 `NYANPASU_FAKE_CORE` 覆盖 → target 目录回退）                                                                                             | `backend/tauri/Cargo.toml:273-281`；`backend/fake-core/src/lib.rs:399-418`；消费者示例 `APP/client/process_core_bridge.rs:18-20`     |

### 1.4 与 spec 正文的偏差（必须按事实实现，spec 措辞为准的地方已注明）

| 偏差 | design 措辞                                                                       | 实际                                                                                                                                                                                      | 处理                                                                                                                                             |
| ---- | --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| DV-A | §4 `async fn recover(&self) -> Result<()>`                                        | Local 是 `recover_quarantine()`，Service 是 `recover_core()`，二者语义都是**清 quarantine 闩锁**（R6/R22）                                                                                | `CoreBackend::recover()` 保留此名，doc 注明真实语义是 clear-quarantine，**不是**重启                                                             |
| DV-B | §4.1 "只保留机器可读 kind + 原始 message/source"                                  | 本 tag 无 `Error::kind()`（R16），R0 才加                                                                                                                                                 | 见 **S4**：隔离一个 `error_kind` 模块作为过渡；R0 合并并 bump submodule 后一步替换（去向登记在 §8）                                              |
| DV-C | §5 "Supervisor/daemon 最终放弃后，发布一次 `core_recovery_exhausted` degradation" | 无 typed 信号，只有字符串前缀（R14）                                                                                                                                                      | 见决策点 D5                                                                                                                                      |
| DV-D | §4 `async fn apply(&self, request: &CoreRequest) -> Result<CoreApplyData>`        | Local 返回本地 `ApplyOutcome`（7 分支），需要按 R12 映射成 `CoreApplyData`                                                                                                                | backend 内转换，app 侧不新增 outcome 类型                                                                                                        |
| DV-E | §6 `CoreActorState` 含 `runtime: RuntimeLifecycleState` 与 `logs: VecDeque`       | 那是 **B1/C1** 的字段                                                                                                                                                                     | 5a 的 actor state **不含**这两项，见 §0 不做清单                                                                                                 |
| DV-F | §4 把 `start` 与 `restart` 列为两个平级 backend 方法                              | 二者都**不是** request-bearing 的运行原语：`start` 在跑时报 `AlreadyRunning`，`restart` 在全新 backend 上报 `NotStarted`（R5c）。legacy `run_core` 的真实语义是"在跑就先停，再按请求启动" | backend 暴露**一个** `run(request)` 迁移原语：Local 用 `switch(spec)`（R5b 一次调用覆盖两种情形），Service 用 stop-then-start 组合。详见 S3 / S7 |

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

**重连：`Client::events()` 自己不重连——必须由 actor 自管。** `events()` 只做一次握手并返回**单条** `EventStream`（`RT/nyanpasu_ipc/src/client/shortcuts.rs:110-150`），流结束或出错就没有下文了。服务端只保证"每条连接建立时先推一个快照"（`RT/crates/nyanpasu-service-runtime/src/server/routing/ws.rs:44`），这是**每连接**的保证，不是断线自愈。因此 `ServiceBackend` 必须持有一个 **actor 拥有、可取消的重连循环**：

- 用 `tokio::spawn` + `CancellationToken`（或 `AbortHandle`）启动订阅任务，句柄存进 backend，`SetBackend` / `Shutdown` 时**先取消再 join**（与 D2 的换槽协议同一把手，见 S3）；
- 断线后按有界指数退避重连；每次重连成功后收到的**第一个 `CoreStatusChanged` 即为权威**，直接覆盖 `last_revision`；
- 重连**不需要**额外的对账 RPC。

Local 的 `watch::Receiver` 与 manager 同生命周期，不会断，无需重连——但它的转发任务同样要被取消并 join（同一协议）。

**并发栅栏：两级标记，缺一不可。** "最后一帧赢"只在**单一有序来源**内成立，而实际有三个并发来源（上表）。**只有 generation 是不够的**：generation 只在换 backend / ws 重连时变，所以**同一条连接内**一个更早发出的 `/status` 回包仍可能晚于一个更新的 ws 帧到达，把 `last_revision` 回退。因此每个异步结果携带两个标记：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObservationToken {
    /// 换 backend 或 ws 重连成功时 +1。用于丢弃"上一条连接/上一个 backend"的在途回包。
    generation: u64,
    /// **每次接受一个权威更新就 +1**。用于丢弃同一条连接内乱序到达的旧回包。
    version: u64,
}
```

规则：

- 每个异步刷新在**发起时**捕获当时的 `ObservationToken`；
- 回来时若 `captured.generation != current.generation` **或** `captured.version < current.version` → **整包丢弃**；
- 只有被接受的权威更新才 `version += 1`；
- **所有**异步结果都必须带 token，**包括** `BackendStatus(..)` 这条内部投递（v2 的消息定义漏了它）：

```rust
BackendStatus { token: ObservationToken, view: CoreStatusView },
```

**post-run 状态必须原子提交。** `run()` 返回 `CoreStatusView`（见 S3），actor 在**同一个消息处理内**用它提交状态并 `version += 1`，然后才回复调用方。这样"run 成功但 `last_revision` 还是 `None`/旧值"的窗口在协议层就不存在——不依赖任何后续推送到达。

依据：`start_core` 的响应体是 `()`（`CoreStartRes<'a> = R<'a, ()>`），`switch` 返回 `SwitchOutcome`，两者都不带 revision，所以刷新必须由 backend 在 `run()` 内部完成并回传。

**冲突处理。** `expected_revision` 语义：

- `None` = 无条件应用（R7/R20）。**只允许在一种情况下出现：actor 尚未观察到任何 revision**——而 `apply` 要求核心已在跑（R7 `Error::NotStarted`），核心跑起来必然产生 revision，所以正常路径下 `None` 不会出现。因此规则是：**apply 一律传 `Some(last_revision.id())`；若 `last_revision` 为 `None`，视为内部不变量被破坏，返回错误而不是降级为无条件应用。**
- CAS 失败：Local `Error::RevisionConflict { expected, actual }`（R8），Service `error_kind = "revision_conflict"`（R20）。两侧都**没有应用任何东西**，所以处理方式是：用 `actual`（或重新查一次 status）刷新 `last_revision`，然后把冲突作为可重试错误上报。

**5a / 5b 分工（重要）：**

- **5a 只做"观察与存储"**：建立表示法、订阅刷新、可取消重连循环、generation 栅栏、start 后同步回填、暴露给 `CoreClient` 的读接口，并用测试钉住（T-RV-01…05）。
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

**裁定 A：pre_start 只构造当前模式匹配的那一个；`SetBackend` 时换槽。** `SetBackend` 必须在 operation guard 下执行（design §3.4 已列入必须持 guard 的清单）。

**换槽协议（必须逐字实现——`shutdown()` 并不释放目录锁）。** 事实 R6b：锁是 `atomic_fs::DirLock`，挂在 `Inner` 的最后一个字段上，**只在最后一个 `Arc<Inner>` drop 时才释放**。所以"shutdown 旧的 → 构造新的"如果还有任何一个 `Arc<CoreManager>` 存活（例如订阅任务里那份 clone），新 manager 会拿不到锁并以 `Error::RuntimeDirectoryOwned` 失败。因此：

1. **backend 槽位改成 `Option<CoreBackend>`**——换槽期间必然存在一个短暂空位，状态里必须能表达它；
2. 取消订阅任务的 `CancellationToken` 并 **`join` 到它真正退出**（不 join 就无法保证它持有的 `Arc` 已经 drop）；
3. 调 `backend.shutdown()`（优雅停核 + 归档 sink）；
4. **显式 drop 掉 backend 值本身以及所有 `Arc<CoreManager>` 克隆**，让 `Inner` 的引用计数归零、目录锁释放；
5. 再构造替换 backend；
6. 只有第 5 步成功才把 `Some(new)` 写回槽位。

**无 backend 失败态。** 第 5 步失败时槽位保持 `None`。此时：所有 mutation 消息返回 `CoreBackendError::NoBackend { last_error }`；`Status` 返回最后已知的 `CoreStatusView` 并把 `state` 置为 `Stopped`；`observation.generation` 已在第 2 步递增，因此旧 backend 的在途回包会被栅栏丢弃（见 RQ-02）。后续 `SetBackend` 可以重试装载。**不做**自动重试循环。

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

**裁定 A：常量 + 纯函数集中一处字符串匹配，并用测试钉住前缀。** Local 从 `CoreStatus.state` 取，Service 从 `CoreState::Stopped(Some(msg))` / `CoreStatusChanged` 的 detail 取（同一字符串，因为 daemon 内跑的是同一个 manager）。

```rust
/// 上游 `instance.rs:631-641` 在重启预算耗尽时写入的前缀。
/// 这是目前唯一的机器可读线索——上游没有 typed 信号（见计划 §1.1 R14）。
/// 若上游后续加了 typed 变体，这里连同 `is_recovery_exhausted` 一并删除。
const RECOVERY_EXHAUSTED_PREFIX: &str = "core kept crashing; restart budget exhausted";
```

**但"算出一个 flag"不等于满足 design §5。** design.md:227 的原文要求是"Supervisor/daemon 最终放弃后，**发布一次** `core_recovery_exhausted` degradation"——这是一个**恰好一次的发布行为**，不是一个可反复读取的布尔位。而状态推送是幂等重放的（R24 明确说 `CoreStatusChanged` 可能因重连/丢事件恢复而重复），朴素实现会对同一次耗尽事件重复发布。因此必须补两样东西：

**（a）注入式 degradation sink（窄接口、可 mock）。** 既有 `UiEventSink`（`APP/client/event_sink.rs:11-31`）只有 `state_changed` / `notice_message` / `update_systray*`，**没有** degradation 通道，不要往里塞。新建一个单方法 trait：

```rust
/// CoreActor 向应用层发布的核心降级事件。
/// 刻意只有一个方法：5a 只需要 `core_recovery_exhausted` 一种。
#[cfg_attr(test, mockall::automock)]
pub trait CoreDegradationSink: Send + Sync + 'static {
    fn publish(&self, degradation: crate::client::runtime::Degradation);
}
```

复用既有 `Degradation { phase, code, message, retryable }`（`APP/client/runtime.rs:448-470`），`code = "core_recovery_exhausted"`、`retryable = true`。sink 从 actor 启动参数注入（不是全局）。

**（b）per-episode latch。** actor state 持 `recovery_exhausted_published: bool`：

- 观察到耗尽前缀且 latch 为 `false` → `publish()` 一次并置 `true`；
- latch 为 `true` 时再观察到同一状态 → **不发布**；
- **latch 复位条件（只有两条）**：观察到核心重新进入非耗尽的活跃状态（`Starting` / `Running`），或 backend 身份变化（`SetBackend` 换槽）。**`recover` 成功本身不复位**——它只清 quarantine、不拉起核心，若复位则重放的同一耗尽快照会二次发布同一 episode。（`run` 成功会经由观察到 `Starting`/`Running` 间接复位，不需要独立条件。）

测试见 T-BK-06 / T-BK-07（恰好一次 + 复位后可再次发布）；T-BK-06 须含 recover-后重放分支：耗尽 → 发布 1 次 → `recover` 成功（核心未拉起）→ 重放同一耗尽快照 → `publish` 仍 `== 1`。

**附带建议（需用户授权，不在本阶段执行）**：给上游提一个小改动，在 `StopReason` 上加 `RestartBudgetExhausted` 变体。**Leader 已裁定不并入已收口的 R0 分支**，授权后作独立小 PR。

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

    /// **唯一的 request-bearing 运行迁移原语**（DV-F）。语义 = legacy `run_core_from`：
    /// 核心未跑就按 `request` 启动；已在跑就迁移到 `request` 指定的 spec。
    /// 刻意**不**暴露 `start` / `restart` 两个平级方法——它们各自只在半边状态空间有效
    /// （`start` 在跑时报 `AlreadyRunning`，`restart` 在全新 backend 上报 `NotStarted`，见 R5c），
    /// 把选择权交给调用方就是把 bug 交给调用方。
    /// 返回**刷新后**的状态投影，而不是 `()`：`switch` / `start_core` 都不带 revision，
    /// 由 backend 在同一次调用内同步补一次 `status()`，让 actor 能原子地提交（RQ-02）。
    pub async fn run(&self, request: &CoreRequest) -> Result<CoreStatusView, CoreBackendError>;

    pub async fn apply(&self, request: &CoreRequest, expected: Option<RevisionIdInfo>)
        -> Result<CoreApplyData, CoreBackendError>;
    pub async fn stop(&self) -> Result<(), CoreBackendError>;
    /// 清除 runtime 的 quarantine 闩锁。**不是**"重启核心"——
    /// Local 走 `recover_quarantine()`、Service 走 `recover_core()`（事实 R6/R22/DV-A）。
    pub async fn recover(&self) -> Result<(), CoreBackendError>;
    pub fn status(&self) -> CoreStatusView;
    /// 取消并 join 订阅任务 → 优雅停核 → 归档 sink。调用后此值必须被立即 drop（D2 换槽协议第 4 步）。
    pub async fn shutdown(self) -> Result<(), CoreBackendError>;
}
```

**`run()` 的两侧实现（不对称，且必须如此）：**

| backend   | 实现                                                                                                                                                                       | 依据                                                                                           |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `Local`   | **一次** `CoreManager::switch(spec)` 调用即可 —— `switch_locked` 自己判 running：未跑则清理 stale epoch 后 `start_locked`；在跑则按能力做 graceful（零停机）或 hard switch | R5b（`switching.rs:52,58-103`）                                                                |
| `Service` | IPC **没有** `/core/switch`，只能组合：判定"是否需要先停" → 需要则 `stop_core()` → `start_core(CoreStartReq)`                                                              | R21；`api/core/apply.rs` 的文档说明 switch 语义只存在于 `/core/apply`，而 apply 要求核心已在跑 |

**Service 的"是否在跑"判定必须读 `detail`，不能读 `state`（事实 R5d）。** wire 上的 `CoreState` 是有损两值投影，把 `Starting` 和 `Restarting` 都压成 `Stopped(None)`——若按它判定，一个正在启动/重启中的核心会被误判为"已停"，于是跳过 `stop_core()` 直接 `start_core()`，而 daemon 侧仍会返回 already-running，`run()` 就失败了。正确判定：

```rust
/// 只有 `Stopped` 是终止态；其余五个 detail 变体都意味着"有东西在跑或正在动"，
/// 必须先 stop。`detail` 缺失（老 daemon 或字段未送达）时**保守当作在跑**——
/// 多发一次 stop 是幂等的，漏发会导致 start 失败。
fn service_needs_stop(infos: &CoreInfos) -> bool {
    match infos.detail.as_ref() {
        Some(CoreStateDetail::Stopped { .. }) => false,
        Some(_) => true,
        None => true,   // fail-safe
    }
}
```

**兜底：`stop_core()` 的 not-started 错误必须被吞掉。** 即便判定正确，daemon 侧仍可能在两次 RPC 之间自行停掉核心。因此 `stop_core()` 返回 typed `not_started`（`error_kind` 常量表，R16）时视为成功继续；**只抑制这一种 kind**，其余错误照常上报。

两侧 `run()` 成功后都必须**同步刷新一次状态**——`switch` 返回 `SwitchOutcome`、`start_core` 返回 `()`，都不带 revision。`run()` 因此**返回刷新后的 `CoreStatusView`**（提交规则见 RQ-02 的 post-run 段），而不是 `Result<()>`。

`LocalBackend`：

- 持 `Arc<nyanpasu_core_manager::CoreManager>`（事实 R4：非 Clone）+ `watch::Receiver<CoreStatus>`（R10）；
- 构造时 `ManagerOptions` **显式写出** `local_ipc_policy: LocalIpcPolicy::Disable`，并加注释说明"上游默认值已是 Disable（`spec.rs:114`），显式化是为了让这条安全门在 app 侧可见可审"（A1 卡要求，事实 R3）；
- `runtime_dir` 来自注入的 `RuntimePaths`，**不得**用 `dirs::*` 自行解析（roadmap §1.6 测试禁真实目录）；
- `apply` 把本地 `ApplyOutcome`（7 分支，R11）映射成 `CoreApplyData`，映射规则**照抄** `manager_bridge.rs:605-639`（R12），含 `DurabilityUncertain` 可嵌套两层、warning 用 `"; "` 拼接。

`ServiceBackend`：

- 持 `nyanpasu_ipc::client::Client`（实例，`Client::new(SERVICE_PLACEHOLDER)`，**禁止 `service_default()`**——A1 卡要求，事实 R18/R19）；
- 状态来自 `/status` + `/ws/events`（R21/R24）。

**两侧共有的订阅任务契约（D2 换槽协议依赖它）。** 每个 backend 持有一个 `SubscriptionHandle { token: CancellationToken, join: JoinHandle<()> }`：

- Local：转发 `watch::Receiver<CoreStatus>` 的变更；
- Service：**actor 自管的可取消重连循环**（`Client::events()` 不重连，见 RQ-02），有界指数退避，每次重连成功后 `revision_epoch += 1`；
- 任务体内**只**持有向 actor 回投 `BackendStatus(..)` 所需的 `ActorRef` 与一份 backend 句柄；`shutdown()` 必须 `token.cancel()` 然后 `join.await`，**join 完成才算订阅任务真正释放了它那份 `Arc<CoreManager>`**（这是目录锁能否释放的关键，R6b）。

`TestBackend`（`#[cfg(test)]`）：脚本化返回值 + 调用计数 + oneshot barrier 钩子，供 A2/A3 的**并发与 gate 语义**测试使用。**注意范围：`TestBackend` 不得用于 A-Exit 要求的 Local/Service parity**（见 S12 的说明）。

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
    /// 唯一的运行迁移消息（DV-F）。**没有** `Start` / `Restart` 两条平级消息：
    /// 它们各自只在半边状态空间有效，拆开就是把状态判断推给调用方。
    Run     { operation: OperationId, request: CoreRequest, reply: ... },
    Stop    { operation: OperationId, reply: ... },
    Recover { operation: OperationId, reply: ... },
    /// 换槽（D2 协议）。**成功后不自动启动核心**——调用方需要核心跑起来时
    /// 必须在**同一个** guard 下紧接着发 `Run { request }`，不能发 `Restart`
    /// （新 backend 没有 `last_spec`，见 R5c）。
    SetBackend { operation: OperationId, mode: RunType, reply: ... },

    Status(RpcReplyPort<CoreStatusView>),
    Shutdown(RpcReplyPort<()>),

    /// 来自订阅任务的内部投递。**必须带 token**，否则同一条连接内的乱序帧无法拒绝（RQ-02）。
    BackendStatus { token: ObservationToken, view: CoreStatusView },
}
```

actor state（对照 DV-E：**不含** runtime lifecycle 与 log ring）：

```rust
struct CoreActorState {
    /// `None` 仅出现在换槽失败后（D2 无 backend 失败态）。
    backend: Option<CoreBackend>,
    mode: RunType,
    operation: OperationGate,
    status: CoreStatusView,
    /// 两级观察标记（RQ-02 栅栏）：generation 随换 backend / ws 重连递增，
    /// version 随每次被接受的权威更新递增。异步结果凭它判定是否过期。
    observation: ObservationToken,
    /// `core_recovery_exhausted` 的 per-episode 发布闩锁（D5）。
    recovery_exhausted_published: bool,
    /// 注入的降级发布端（D5）。
    degradation: Arc<dyn CoreDegradationSink>,
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

| 方法                | 5a 路由                                                                                                                                                                                                                                                                | 去向 |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---- |
| `check_and_promote` | 维持现有实现（候选校验 + 原子 promote），核心检查改调 `CoreBackend::check`                                                                                                                                                                                             | B1   |
| `apply_candidate`   | 维持现有实现                                                                                                                                                                                                                                                           | B3   |
| `apply_promoted`    | 维持现有实现（`api::put_configs`）                                                                                                                                                                                                                                     | B3   |
| `restart`           | **改走 `Run { request }`**（**不是** `CoreBackend::start`）。legacy `run_core_from(product)` 的语义就是"在跑先停、再按 product 启动"，恰好等于 `run()`（DV-F / R5b）。`request` 由 lease 适配器按 A13f 的既有取值方式就地构造，`config_path = runtime_paths.product()` | —    |
| `stop`              | **改走** `CoreBackend::stop`                                                                                                                                                                                                                                           | —    |

> **注意**：`CoreLifecycleLease::restart` 这个**名字**保持不变（A3 要求兼容 seam 不动），但它背后是 `Run` 而非任何 "restart" 原语。适配器上必须写明这一点，否则后续读者会误以为可以直接映射到 `CoreManager::restart()`。

`CoreLifecyclePort::status()` 在 5a **第一次有了生产实现**（A5f 此前无生产调用者），返回值由 `CoreStatusView` 投影成既有 `CoreStatusSnapshot`，**wire 与结构都不变**。

`on_profile_change()` 维持现有实现（连接中断服务，PR-6 范围）。

**顺序不变式（必须在代码注释里写明）：** A3f 已确认 `rebuild_gate` 在全部 10 处都先于 `core.begin()`，`patch_running_config` 是 `clash_patch_gate → rebuild_gate → begin()`。5a 引入 `OperationGate` 后变成**三层嵌套且全局顺序一致**，因此不产生死锁。B2 会把前两层吸收掉。

### S8 — 组合根接线

**不能**沿用"在 `setup.rs` 里构造好再从 `ClientSetupArgs.core` 传进去"的写法：事实 A17f——`try_new_with_args` 在进入 `block_on` **之前**就解构了 `core`（`client/mod.rs:255-264`），而 `CoreClient` 需要的 `enable_service_mode` 来自 typed `application` client，后者到 block 内第 279 行才存在。

**正确的所有权图（三步）：**

**（1）`CoreClient` 在 block 内、typed 快照之后构造。** 位置紧跟 `new_typed_config_clients(...)`（`client/mod.rs:279`）之后：

```rust
let application_snapshot = application.get().await?.state;
let mode = crate::core::RunType::classify(
    application_snapshot.enable_service_mode,
    crate::core::service::ipc::get_ipc_state(),
);
let core_client = CoreClient::new(CoreActorArgs {
    mode,
    runtime_paths: runtime_paths_for_setup.clone(),
    degradation: degradation_sink.clone(),   // D5，由 ClientSetupArgs 注入
}).await?;
```

按 D2 的顺序事实此刻必然是 `Normal`（`init_service()` 尚未跑），先建 Local backend；健康检查随后再发 `SetBackend(Service)`。

**（2）`CoreClient` 成为 `NyanpasuClientInner` 的 typed 字段；legacy trait 适配器内部产出。**

```rust
struct NyanpasuClientInner {
    // ...
    core_client: CoreClient,              // 新增 typed 依赖
    core: Arc<dyn CoreLifecyclePort>,     // 保留，但生产路径由 core_client 内部产出
}
```

`ClientSetupArgs.core` 由 `Arc<dyn CoreLifecyclePort>` 改为 **`Option<Arc<dyn CoreLifecyclePort>>`**：

- 生产（`setup.rs`）传 `None` → block 内 `core = core_client.as_lifecycle_port()`；
- 测试传 `Some(mock)` → 沿用既有 `test_client_args_with_lifecycle`（A8f），**测试图零改动**。

这**完全复刻**同文件既有的 `clash_patch: Option<...>` 分阶段注入模式（`client/mod.rs:265-268`），是本仓已确立的房规而非新发明。`setup.rs` 相应删掉 `core: Arc::new(LegacyCoreBridge::new(runtime_paths))` 一行，并新增 degradation sink 的注入。

**（3）无注入点的消费者——显式 clone 穿线。** 两处 `pub fn` 既没有参数也拿不到 Tauri state：

| 消费者                                                   | 现状                                                                                  | 5a 穿线方式                                                                                                                                                                                                                                                                                                               |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `APP/feat.rs:56 restart_clash_core()`                    | 无参 `pub fn`，`spawn` 后调 `CoreManager::global().run_core()`。调用方是托盘/菜单动作 | 改签名为 `restart_clash_core(client: NyanpasuClient)`，由调用点（托盘链）把已 `manage` 的 client clone 传入。`NyanpasuClient` 是 `Arc` newtype，clone 开销为零（`client/mod.rs:93-96`）                                                                                                                                   |
| `APP/core/service/ipc.rs:74 on_ipc_state_changed(state)` | 健康检查线程内调用，读 `Config::verge()` + `CoreManager::global()`                    | `spawn_health_check` 接受一个 `CoreClient`（**不是**整个 `NyanpasuClient`——这里只需要核心生命周期），逐层往下传到 `on_ipc_state_changed`。`init_service()` / `spawn_health_check` 的签名相应加参数；`APP/core/service/mod.rs::init_service` 与 `APP/utils/init/mod.rs::init_service` 两个同名函数都在调用链上，一并加参数 |

> 这两处都**不是**"再加一个全局"——是把已有的实例显式传下去。若某个调用点确实拿不到 client（例如极早期启动路径），**停下来上报**，不要退回全局查找。

**（4）`LegacyCoreBridge` 删除**（`core_bridge.rs:107-153`，含其 `CoreManager::global()` 两处与那条 TODO 标记）。

**（5）IpcState 翻转的处理**（`core/service/ipc.rs:83-91`）改为在**同一个** `CoreOperationGuard` 下顺序执行 `SetBackend(mode)` 然后 `Run { request }`——**不是** `Restart`（新 backend 没有 `last_spec`，R5c）。

### S9 — 迁移直接调用点

按 A3 卡"start/stop/restart/status 改走 actor"，逐点替换（全部来自 `CoreManager::global()` 的生产命中）：

| 调用点                                                   | 现状                             | 5a 改为                                                                                                |
| -------------------------------------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `APP/ipc.rs:403` `get_core_status`                       | `CoreManager::global().status()` | `CoreClient::status()`，**wire 形状不变**（同一 `(Cow<CoreState>, i64, RunType)` 元组），删除该处 TODO |
| `APP/ipc.rs:503` `restart_sidecar`                       | `run_core()`                     | guard + `Run { request }`                                                                              |
| `APP/ipc.rs:960,977,994` service start/stop/restart 命令 | `run_core()`                     | guard + `SetBackend` + `Run { request }`                                                               |
| `APP/feat.rs:58`                                         | `run_core()`                     | guard + `Start`                                                                                        |
| `APP/feat.rs:292,385`                                    | `status()`                       | `CoreClient::status()`                                                                                 |
| `APP/core/service/ipc.rs:83,88`                          | `status()` + `run_core()`        | 见 S8                                                                                                  |
| `APP/utils/help.rs:268`                                  | `stop_core()`                    | `CoreClient::shutdown()`（见 S11）                                                                     |
| `APP/utils/resolve.rs:288`                               | `stop_core()`                    | guard + `Stop`                                                                                         |
| `APP/feat.rs:392` macOS DNS                              | `change_default_network_dns`     | **不动，但必须 allowlist**（见下 §S9.2）                                                               |
| `APP/core/updater/instance.rs:201,205,216,279`           | `begin_lifecycle` + lease 方法   | **必须迁移**（见下 §S9.1）——原计划"不动"是**错的**                                                     |

#### S9.1 — Updater 必须在 5a 迁移（原计划此处有错）

事实 A16f：`replace_core()` 依赖的是 **S10 要删掉的那一整套 API**——`CoreManager::global().begin_lifecycle()`、`lifecycle.stop_core()`、`lifecycle.run_core_from(product)`。原计划"Updater 不动 + S10 删 lease"两条**互相矛盾，会直接编译失败**。而且即便设法保住编译，legacy 单例也停不掉 CoreActor 拥有的新 manager 实例——那是**两个不同的进程所有者**，会出现"更新器以为停了核、实际没停"的静默损坏。

**Leader 裁定：在 5a 用显式注入的 `CoreClient` + operation guard 改造 `replace_core`，不加 `attach_core_port` 全局桥，UpdaterActor 的完整迁移仍归 PR-6d。**

**注入必须穿透真实的四段构造链**（事实 A16g；v2 计划把类型名写成了不存在的 `UpdaterInstance`，且只说"构造点会接受它"，不够可执行）：

```text
ipc::update_core(core_type)                       ← Tauri 命令，注入起点
  → UpdaterManager::update_core(&core_type, core) ← 加一个参数，**不存入 manager 自身**
    → UpdaterBuilder::set_core(core).build()      ← builder 加一个 Option 字段
      → Updater { core, .. }                      ← 真实类型名是 `Updater`
```

**逐点签名（四处，全部要改）：**

```rust
// 1) APP/ipc.rs:639 — 从 Tauri managed state 取 client，clone 出 CoreClient
#[tauri::command]
#[specta::specta]
pub async fn update_core(
    client: tauri::State<'_, crate::client::NyanpasuClient>,
    core_type: nyanpasu::ClashCore,
) -> Result<usize> {
    // facade 方法，不暴露 actor ref（CLAUDE.md §7：NyanpasuClient 是 facade 不是 service locator）
    let core = client.core_client();
    let event_id = (updater::UpdaterManager::global()
        .write().await
        .update_core(&core_type, core).await)?;
    Ok(event_id)
}

// 2) APP/core/updater/mod.rs:222
pub async fn update_core(&mut self, core_type: &ClashCore, core: CoreClient) -> Result<usize>

// 3) APP/core/updater/instance.rs — builder 新增字段与 setter
pub(super) struct UpdaterBuilder {
    client: Option<reqwest::Client>,
    core_type: Option<ClashCore>,
    mirror: Option<String>,
    artifact: Option<String>,
    tag: Option<CoreTypeMeta>,
    core: Option<CoreClient>,        // 新增
}
impl UpdaterBuilder {
    pub fn set_core(mut self, core: CoreClient) -> Self { self.core = Some(core); self }
    // build() 里 `core` 缺失时按既有风格 bail
}

// 4) APP/core/updater/instance.rs:31 — Updater 持有它
pub(super) struct Updater {
    // ...既有字段不动...
    core: CoreClient,
}

async fn replace_core(&self) -> anyhow::Result<()> {
    self.dispatch_state(UpdaterState::Replacing);
    // 整个 stop → swap binary → run 事务持有同一个 guard，
    // 与 rebuild/change-core 互斥（这正是 legacy begin_lifecycle 原本提供的保证）。
    let operation = self.core.begin_operation().await?;

    let current_core = /* 现有取值逻辑不变 */;
    let runtime_paths = if current_core == self.core_type {
        let paths = /* 现有取值逻辑不变 */;
        self.core.stop(&operation).await?;        // 取代 lifecycle.stop_core()
        Some(paths)
    } else { None };

    /* 现有的下载 / 校验 / 复制二进制逻辑逐字不动 */

    if let Some(paths) = runtime_paths.as_ref() {
        self.dispatch_state(UpdaterState::Restarting);
        let request = CoreRequest::for_product(paths.product(), current_core)?;
        self.core.run(&operation, &request).await?;   // 取代 lifecycle.run_core_from()
    }
    Ok(())
}
```

**注入原则（三条，逐条约束）：**

1. **`CoreClient` 只随调用传递，绝不存进 `UpdaterManager`。** 把它挂到全局 manager 上等同于新建一个全局服务定位器（CLAUDE.md §7 禁止），也正是 design §9 拒绝的 `attach_core_port` 半迁移桥。它只从 `update_core` 的参数流向 builder 再流向 `Updater` 实例。
2. **facade 方法而非裸 actor ref**：`NyanpasuClient` 新增 `pub fn core_client(&self) -> CoreClient`（返回 clone，`CoreClient` 是 `Arc` newtype）。**不**暴露 `ActorRef`（CLAUDE.md §7 的 facade 约束）。
3. `UpdaterManager::global()` 本身仍是 PR-6d 的 residual——**本阶段只把 core 依赖显式化，不动 updater 自身的全局性**。

原来那条 `TODO(actor-migration): temporary bridge to the legacy global core manager`（instance.rs:202-204）**删除**。

> **`update_core` 的 Tauri 命令签名变化会动 bindings**：新增的 `client: tauri::State<'_, NyanpasuClient>` 参数**不进入** TS 签名（Tauri 的 managed state 参数在 specta 导出时被跳过，与既有 `patch_verge_config` 等命令同理）。因此 `updateCore` 的 TS 形状不变——但**实施时必须核对** `git diff frontend/interface/src/ipc/bindings.ts` 为空来证明这一点，不要假设。

#### S9.2 — macOS DNS residual allowlist（leader 裁定）

`feat.rs:392` 的 `CoreManager::global().change_default_network_dns(...)` 是**第二个** core residual（原计划 §5 的"仅剩 Updater residual"断言因此是错的）。**Leader 裁定：不在 5a 迁移，allowlist 到 PR-5c / C3**，补标记：

```rust
// TODO(actor-migration): macOS DNS 仍走 legacy CoreManager。
// Reason: DNS 归位（MacosDnsGuard，与 start/stop 保序）是 PR-5c / C3 的范围；
//         在 5a 迁移它会把平台副作用编排提前带进本阶段。
// Remove when: PR-5c 把 MacosDnsGuard 移入 CoreActor。
```

于是 5a 结束后 `CoreManager::global()` 的**生产代码**残留恰好 **1 处**（`feat.rs:392`，且带 `#[cfg(target_os = "macos")]`），另有 1 处出现在 `process_core_bridge.rs:4` 的**文档注释**里（内容是"禁止调用"的告诫，不是调用）。S13 的 grep 判据按此写。

### S10 — 删除 legacy 生命周期

- 删除 `CoreManager::lifecycle_lock` 字段、`begin_lifecycle()`、`CoreLifecycleLease<'a>`（legacy 的那个）以及所有 `*_with_lease` 变体（`APP/core/clash/core.rs:386-422,428,444-449,486-651`）；
- **删除两条裸线程递归恢复路径**（design §5 禁止的第二层恢复，缺一不可）：
  1. `core.rs:577-582`——`recover_core()` 失败后 `sleep(5s)` + `std::thread::spawn` + 自调用；
  2. **`core.rs:228-238`**——`Instance` 事件循环里核心异常退出且 `tx.send` 失败时 `std::thread::spawn` → `CoreManager::global().recover_core()`（事实 A20f。原计划**遗漏**了这一条）。

  两条删完后 `recover_core()` 本身若失去调用者也一并删除；恢复完全交给 runtime 的 Supervisor（R13）。

- **保留**：`Instance`、`RunType`、`find_binary_path`、`change_default_network_dns`、`status()`（S9.2 的 DNS allowlist 仍需要）。

> 判定原则：只删"被 CoreActor 取代的排他与恢复机制"，不删"尚有 owner 的功能代码"。

### S11 — shutdown 接线（原计划的顺序描述有错）

事实 A18f：`cleanup_processes` 的**真实**顺序是 `save_window_state` → **`resolve_reset()`** → `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`。而 `resolve_reset()` 内部**已经停了一次核**（`resolve.rs:288`）。所以现状是**核被停两次，且第一次发生在 `client.shutdown()` 之前**——原计划"顺序保持 client.shutdown() → widget stop → 停核"的描述是错的。

**改法（reviewer 建议，已核实可行）：**

1. **从 `resolve_reset()` 里移除停核**，只留 `reset_sysproxy()`。可行性已验证：`resolve_reset` 全仓**只有一个调用者**（`help.rs:251`），移除后不影响任何其它路径；
2. `NyanpasuClient::shutdown()` 追加：rebuild worker 关闭**之后**调 `CoreClient::shutdown()`（`Shutdown` 消息 → 拒绝全部 waiters → 取消并 join 订阅任务 → 关 backend）；
3. 删除 `help.rs:268` 的 `CoreManager::global().stop_core()`——停核已由第 2 步完成；
4. 最终顺序：`save_window_state` → `resolve_reset()`（现在只重置系统代理）→ `client.shutdown()`（rebuild worker + CoreActor + backend）→ widget stop；
5. 更新 `shutdown()` 的契约 doc comment（`client/mod.rs:392-401` 现在明写"不停 CoreManager globals"，5a 后不再成立）。

> 顺序理由：系统代理必须在核心停止**之前**恢复，否则会留下一段"代理指向已死核心"的窗口——这正是现状把 `resolve_reset` 放在最前的原因，保留该位置。

### S12 — 测试

全部 TempDir + barrier/RPC 同步，**零 sleep 断言**（A10f）。

#### A2 gate 测试（对应 A-Exit 六项）

| ID      | 名称                                                 | 断言                                                                                                                                                              | A-Exit 项         |
| ------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| T-OP-01 | `waiters_are_granted_in_fifo_order`                  | 三个 waiter 依次入队；释放 active 后按入队顺序放行（用 oneshot 记录放行次序）                                                                                     | FIFO              |
| T-OP-02 | `dropping_a_waiting_guard_cancels_it`                | waiter guard drop → 从 waiters 移除；后续 release 不会把它误认为 active                                                                                           | 等待取消          |
| T-OP-03 | `guard_dropped_right_after_grant_releases_active`    | 用 barrier 让 release 与 grant 竞争：guard 在刚被提升为 active 后 drop → active 清空且下一个 waiter 被放行                                                        | 刚获批取消        |
| T-OP-04 | `stale_release_is_idempotent_noop`                   | 对已完成/未知 ID 发 `ReleaseOperation` → 不影响当前 active，不 panic                                                                                              | stale release     |
| T-OP-05 | `mutation_with_wrong_id_returns_stale`               | 持 A 的 id 时用 B 的 id 发 `Run` → `StaleOperation`，且 backend **零调用**（TestBackend 计数）                                                                    | wrong-id mutation |
| T-OP-06 | `shutdown_drains_all_waiters`                        | 一个 active + 两个 waiter 时 shutdown → 两个 waiter 都收到错误，backend 收到 stop                                                                                 | shutdown drain    |
| T-OP-07 | `acquire_times_out_and_releases_the_waiter`（RQ-04） | 用 `tokio::time::pause()` 推进到 `CORE_ACQUIRE_TIMEOUT` → `begin_operation` 返回 `AcquireTimeout`，且 waiter 已从队列移除（后续 release active 时直接放行第三个） | RQ-04             |

#### A1 backend parity 测试

| ID      | 断言                                                                                                                                                                                                                                                                                                            |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-BK-01 | **真实 parity**：`CoreBackend::{Local, Service}` 对 check / run / stop / recover 的成功路径产生一致的 `CoreStatusView` 转换。Local = TempDir + manager-compatible 探针核（见下 §S12.1）；Service = 真实 IPC roundtrip harness（见下 §S12.2）。**不得用 `TestBackend` 顶替**（A-Exit 明写 Local/Service parity） |
| T-BK-02 | `LocalBackend` 构造出的 `ManagerOptions.local_ipc_policy == LocalIpcPolicy::Disable`（显式化的回归钉）                                                                                                                                                                                                          |
| T-BK-03 | apply outcome 映射：7 个本地 `ApplyOutcome` 分支 → `CoreApplyData`，含 `DurabilityUncertain` 单层与**双层嵌套**（warning 以 `"; "` 拼接），`Noop` 不丢失                                                                                                                                                        |
| T-BK-04 | `local_error_kind` 对 12 个 `nyanpasu_ipc::api::error_kind` 常量的映射（断言用常量而非字面量）                                                                                                                                                                                                                  |
| T-BK-05 | `is_recovery_exhausted` 对上游前缀命中/不命中（纯函数层）                                                                                                                                                                                                                                                       |
| T-BK-06 | **`core_recovery_exhausted` 恰好发布一次**（D5 / design §5）：注入 `MockCoreDegradationSink`，重复投递同一个耗尽状态 → `publish` 调用次数 `== 1`                                                                                                                                                                |
| T-BK-07 | **latch 复位后可再次发布**：耗尽 → 发布 1 次 → 核心重新 `Running`（或 `SetBackend` 换槽）→ 再次耗尽 → 累计 `publish` 次数 `== 2`                                                                                                                                                                                |
| T-BK-08 | **换槽后目录锁可重新获取**（D2 协议回归钉）：构造 Local backend → `SetBackend` 换到另一个 Local（同一 `runtime_dir`）→ 新 manager 构造**成功**（不出现 `Error::RuntimeDirectoryOwned`）。这条专门防止"忘了 join 订阅任务 / 忘了 drop `Arc`"的回归                                                               |
| T-BK-09 | **无 backend 失败态**：让替换构造失败 → 槽位为 `None` → mutation 返回 `NoBackend`，`Status` 仍可读且 `state == Stopped`；随后一次成功的 `SetBackend` 能恢复                                                                                                                                                     |

#### RQ-02 revision 测试

| ID       | 断言                                                                                                                                                                                                                                                                                                                                         |
| -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-RV-01  | 三个刷新来源（status 查询 / 推送 / 操作返回）都能更新 `last_revision`；**同 generation 内**最后写入者赢                                                                                                                                                                                                                                      |
| T-RV-02  | Service 断线后 actor 自管的重连循环会重连；重连成功后收到的第一个 `CoreStatusChanged` 快照直接覆盖旧 revision（不发额外对账 RPC）                                                                                                                                                                                                            |
| T-RV-03a | `ConfigRevision → ConfigRevisionInfo`：`epoch` / `generation` / `source_hash` / `effective_hash` 保真，**`runtime_path` 被丢弃**                                                                                                                                                                                                             |
| T-RV-03b | `RevisionId → RevisionIdInfo`：三字段（`epoch` / `generation` / `effective_hash`）直拷。**这是两个不同的转换**——`RevisionId` 本来就没有 `runtime_path`，原 T-RV-03 把两者混为一谈，语句不成立                                                                                                                                                |
| T-RV-04  | **同连接内的乱序栅栏**（v2 的断言前提是错的——同一条连接内 generation **不会**变）：保持 `generation` 不变，先发起一次慢 `/status`（捕获 `version = N`）→ 期间接受一个更新的 ws 帧（`version` 变 N+1）→ 慢 `/status` 回来时因 `captured.version < current.version` 被**整包丢弃**，`last_revision` 不回退（barrier 控制到达顺序，不用 sleep） |
| T-RV-06  | **跨 generation 栅栏**：`SetBackend` 换槽后，旧 backend 的在途回包因 `captured.generation != current.generation` 被丢弃                                                                                                                                                                                                                      |
| T-RV-07  | **post-run 原子提交**：`run()` 返回的 `CoreStatusView` 在**回复调用方之前**已提交——`run` 的 RPC 一返回，`CoreClient::status()` 立即可见新 revision，无需等待任何推送                                                                                                                                                                         |
| T-RV-05  | **start 后同步回填**：Service `run()` 成功（`start_core` 返回 `()`，不带 revision）后 `last_revision` 已非空；Local `switch()` 同理                                                                                                                                                                                                          |

#### seam 回归

| ID      | 断言                                                                                                                                                                                         |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-SM-01 | 既有 `client/mod.rs` 与 `rebuild.rs` 的全部 lease 测试在 6 个替身更新后**继续通过**（A9f 列的替身逐个适配，不改测试语义）                                                                    |
| T-SM-02 | `rebuild.rs:930-1000` 的 `s04_concurrent_restart_waits_until_change_core_rollback_completes` 在三层嵌套（clash_patch_gate → rebuild_gate → OperationGate）下仍然通过——这是顺序不变式的回归钉 |

**Local backend 测试的 fake core 问题（事实 R17）。** 两侧的 fake core 都**不能**靠 `CARGO_BIN_EXE_*` 拿到：

- `nyanpasu-core-manager` 的 fake core 是**它自己 package 的 `[[bin]]`**，`CARGO_BIN_EXE_*` 只在同 package 内可见，app 侧取不到（R17）；
- 本仓的 `backend/fake-core` 对 `backend/tauri` 是 **dev-dependency**，而 dev-dependency **既不构建该 binary 也不设置 `CARGO_BIN_EXE_fake-core`**——这一点在 `backend/tauri/Cargo.toml:275-280` 有明确注释。既有做法是**预构建 + 运行时定位**：`cargo build -p fake-core`（或 `cargo test -p fake-core`），然后 `fake_core::require_bin_path()` 按 `current_exe` 的 profile/triple 查找，支持非空 `NYANPASU_FAKE_CORE` 覆盖，最后回退 target 目录（`backend/fake-core/src/lib.rs:399-418`）。现成消费者示例：`APP/client/process_core_bridge.rs:18-20`。

**因此：** 用真实 `LocalBackend` 驱动真实 `CoreManager` 时必须沿用同一套预构建 + `require_bin_path()` 流程，S13 的验证命令里要显式加上 `cargo build -p fake-core` 前置步骤。

#### S12.1 — Local parity 需要一个 manager-compatible 的探针核（**不允许降级为 TestBackend**）

事实 A19f 给出了具体缺口：manager 的默认 readiness 是 `ControllerVersionProbe`——"healthy iff `GET /version` succeeds"（`RT/.../health/probe.rs:109`）；而本仓 fake-core 的内置 HTTP **只**对精确 `PUT /configs` / `PATCH /configs` 作答，其余一律 404（`backend/fake-core/src/main.rs:22-23,305-344`），因此核心永远达不到 healthy，`start` 会卡在 `startup_timeout` 后失败。

**Leader 裁定：补齐这个缺口，不降级测试。** 两条可选路径，S12 开工时选一条并在 PR 描述里写明：

- **路径 A（推荐）：给 `backend/fake-core` 的内置 HTTP 加一个 `GET /version` 200 应答。** 改动面极小（`main.rs` 的请求分派加一条精确匹配分支，返回一个形如 `{"version":"fake"}` 的 JSON），且对既有 S09 进程矩阵测试是**纯 additive**——那些测试只断言 `/configs` 行为与退出码，不会因为多一个路由而改变。同时确认 `-t`（check）语义：manager 的 `check_config` 走 `-t` 干跑（R5/`kind.rs:88-95`），fake-core 已有 `CHECK_EXIT` / `CHECK_STDOUT` / `CHECK_STDERR` 行为键，应当已足够——**开工时实测确认**。
- **路径 B：新增一个最小的 manager-compatible 探针核**（只答 `GET /version` + 支持 `-t`），放在 `backend/fake-core` 内作为第二个 `[[bin]]`。仅在路径 A 被证明会污染既有测试时才选它。

**若两条路径都被证明不可行，停下来上报 leader**——不要静默把 T-BK-01 降级为 mock 对比，那会让 A-Exit 的 "Local/Service 基本生命周期 parity 测试" 变成空头支票。

#### S12.2 — Service parity 需要真实 IPC roundtrip harness

`ServiceBackend` 走的是命名管道 / Unix socket（R19：`\\.\pipe\{placeholder}` / `/var/run/{placeholder}.sock`）。parity 测试**不能**打真实 daemon（端点是全局固定路径，PR-5-pre 已确认这条约束），所以：

- 在测试里起一个**进程内的最小 IPC 服务端**，绑定到一个**测试专用 placeholder**（例如 `nyanpasu_ipc_test_{pid}_{n}`，避开生产的 `nyanpasu_ipc`），实现 parity 所需的四条端点：`/status`、`/core/start`、`/core/stop`、`/core/recover`，外加 `/ws/events`（供 T-RV-02 的重连断言）；
- `ServiceBackend` 用 `Client::new(<测试 placeholder>)` 构造——这正是 A1 卡"禁止 `service_default()`、必须实例化 client"的**可测性收益**，也是本条测试之所以可行的原因；
- Windows 命名管道与 Unix socket 的绑定方式不同，harness 要用 `#[cfg]` 分支；若某平台实现成本过高，**该平台上 `#[ignore]` 并在 PR 描述里写明**，而不是把整条 parity 降级。

### S13 — 门禁

```powershell
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
cargo build -p fake-core          # T-BK-01 的前置：dev-dependency 不会构建它（A15f）
pnpm test:backend
git diff --stat frontend/interface/src/ipc/bindings.ts   # 期望：空
pnpm lint:ts
pnpm architecture-ledger
pnpm lint:architecture-ledger
```

**bindings 预期：无变化。** 5a 不改任何 wire（`get_core_status` 保持元组形状，S9）。若 bindings 有 diff → 说明范围溢出，停下核查。

**ledger 基线（已于 2026-08-02 实测，`--mode=gate` 当前通过）：**

```
config_calls 116 · service_globals 74 · migration_markers 19 · legacy_dto_refs 300 · test_real_dirs 0
```

> 注意：这是 **PR-5-pre 落地后**的实测值，与本计划初稿引用的 120/80/18 不同（初稿误用了 5-pre 之前的快照）。以本节为准。

**ledger 预期变化（必须逐条核对后再 `--write-snapshot`）：**

| 指标                                       | 方向                                    | 原因                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------------------------------------ | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `service_globals["CoreManager::global()"]` | **显著下降**（基线 16 条计数）          | S9 迁移 + S9.1 Updater 迁移 + S10 删两条恢复路径 + S8 删 `LegacyCoreBridge`。**5a 后生产代码仅剩 `feat.rs:392` 一处**（macOS DNS，allowlist 到 5c），另有 `process_core_bridge.rs:4` 的文档注释文本命中                                                                                                                                                                                                                |
| `migration_markers`                        | 基线 19 → **预期 17**                   | **+2**：S4 的 R0 过渡标记、S9.2 的 macOS DNS allowlist 标记。**−4**：`core_bridge.rs` 的 `CoreManager::global()` 桥（随 `LegacyCoreBridge` 删）、`core/clash/core.rs` 的 legacy lifecycle 桥、`updater/instance.rs` 的 legacy core manager 桥（S9.1 迁移）、`ipc.rs` 的 core manager status 桥（S9 迁移）。**±0（相对迁移）**：`core_bridge.rs` 的 connection-interruption 标记随 `on_profile_change` 一起搬到新实现处 |
| `config_calls`                             | 不应上升（基线 116）                    | 新代码禁止调 `Config::*()`；依赖全部注入。S8 把 `on_ipc_state_changed` 的 `Config::verge()` 读取改为从 typed 快照传入时**可能下降**——下降是好事，照实记录                                                                                                                                                                                                                                                              |
| `test_real_dirs`                           | **必须仍为 0**                          | 新测试只用 TempDir + 注入的 `RuntimePaths`；S12.2 的 IPC harness 用测试专用 placeholder，不碰生产端点                                                                                                                                                                                                                                                                                                                  |
| `bridgeFiles`                              | 8 → 7（若 `core_bridge.rs` 整文件删除） | 该文件同时还装着 `RunningConfigPatchPort` / `LegacyRunningConfigPatchBridge`（B3 才删）与 `restore_product`，**大概率保留**。以实际删除结果为准，不要为了凑数字而搬移代码                                                                                                                                                                                                                                              |

> **两条硬规则**：(1) `config_calls` 或 `test_real_dirs` 变差 → **回头改代码，不要靠改 snapshot 掩盖**；(2) `migration_markers` 若不等于 17，**逐条比对上表列出的加减项**找出差异来源再决定，**不要盲目 `--write-snapshot`**。

---

## 5. Exit 判据映射

task.md A-Exit 三条：

| Exit                                                                                              | 交付步骤    | 验证                                                                                                                                                                               |
| ------------------------------------------------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| operation 测试：FIFO / 等待取消 / 刚获批取消 / stale release / wrong-id mutation / shutdown drain | S5、S6、S12 | T-OP-01…06 全绿                                                                                                                                                                    |
| Local/Service 基本生命周期 parity 测试                                                            | S3、S12     | T-BK-01…09 全绿，其中 **T-BK-01 必须是真实 Local/Service 双端**（S12.1 探针核 + S12.2 IPC harness），不接受 TestBackend 顶替                                                       |
| legacy core 生命周期不再被新调用点使用                                                            | S9、S10     | `rg 'begin_lifecycle\|lifecycle_lock\|_with_lease' backend/tauri/src` 为 0；`CoreManager::global()` 的**生产代码**命中仅剩 `feat.rs:392`（macOS DNS，带 allowlist 标记，owner=5c） |

roadmap §6.1 附加项：

| §6.1 判据                                                    | 对应                                                                   |
| ------------------------------------------------------------ | ---------------------------------------------------------------------- |
| 封闭 enum，不定义 `CoreEngine` trait/factory                 | S3；`rg 'CoreEngine\|EngineFactory' backend/tauri/src` 为 0            |
| `LocalIpcPolicy::Disable` 显式写出                           | S3 + T-BK-02                                                           |
| 禁用 `service_default()`                                     | S3；ServiceBackend 用 `Client::new`                                    |
| client 预分配 `OperationId` + pending guard                  | S6 + T-OP-03/07                                                        |
| 不实现 TTL / auto-steal / watchdog                           | S5；`rg 'ttl\|auto_steal\|watchdog' backend/tauri/src/core/actor` 为 0 |
| actor 无第二层恢复，只**发布一次** `core_recovery_exhausted` | S3(D5) + S10（删两条裸线程路径） + T-BK-05/06/07                       |
| A3 兼容 seam 保留，旧 trait 名不扩散                         | S7；新代码里 `CoreLifecycle*` 只出现在适配 impl 中                     |
| RQ-02 / RQ-04 已作答                                         | 本计划 §2                                                              |

---

## 6. 风险与回滚

| 风险                                                       | 概率 | 影响                                  | 缓解                                                                                                                                                         |
| ---------------------------------------------------------- | ---- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `CoreManager::new()` 的运行目录独占锁与 daemon 冲突        | 中   | Service 模式下建 Local backend 会失败 | D2=A 只建当前模式匹配的那个；且 app 与 daemon 的 runtime_dir 本就不同（daemon 用 `service_data_dir`）。**S3 开工时先写一个断言两路径不相等的测试**，不要假设 |
| lease 被 move 进 `async move` 闭包导致 guard 不满足 `Send` | 中   | 编译失败                              | A4f 已定位为编译期硬约束；`CoreOperationGuard` 的字段（`OperationId` + `CoreClient` + `bool`）全部 `Send`，`ActorRef` 亦然                                   |
| 三层嵌套锁引入死锁                                         | 低   | 挂起                                  | A3f 证明全局顺序一致（10/10 处 `rebuild_gate` 先于 `begin`）；T-SM-02 作回归钉                                                                               |
| 6 个 lease 测试替身适配引发大面积测试改动                  | 高   | diff 变大、review 困难                | 适配只改**构造**不改**语义**；`MockRunningCoreBridge` 的 4 方法 mockall 面保持不变，只在 lease 侧多包一层 guard                                              |
| 上游"重启预算耗尽"字符串前缀漂移                           | 中   | `recovery_exhausted` 静默失效         | D5 集中在一处 + T-BK-05 钉住；typed 变体作**独立上游小 PR**（leader 已裁定不并入已收口的 R0 分支）                                                           |
| 本地 clippy 假红（共享 target kache 污染）                 | 中   | 误判                                  | 已知问题：用独立 `--target-dir` 复验再下结论                                                                                                                 |
| `apply` 实现但不接线被审查判为投机代码                     | 中   | review 争议                           | D4=A 已裁定：实现并由 T-BK-03 钉住 outcome 映射，生产接线留给 B3                                                                                             |
| **换槽时目录锁未释放**（漏 join 订阅任务 / 漏 drop `Arc`） | 中   | `SetBackend` 后新 manager 构造失败    | R6b 明确锁只在最后一个 `Arc<Inner>` drop 时释放；D2 换槽协议逐步写死；**T-BK-08 是专门的回归钉**                                                             |
| Service `/ws/events` 无重连导致状态静默陈旧                | 中   | UI 状态与实际不符                     | RQ-02 要求 actor 自管可取消重连循环；T-RV-02 钉住                                                                                                            |
| 过期 `/status` 回包把 `last_revision` 回退                 | 中   | B1 的 CAS 会用错 token                | generation 栅栏（RQ-02）；T-RV-04 用 barrier 精确构造该竞态                                                                                                  |
| S12.1 探针核改造污染既有 S09 进程矩阵测试                  | 低   | 既有测试红                            | 路径 A 是纯 additive 的路由新增（既有测试只断言 `/configs` 与退出码）；开工先实测，若污染则改走路径 B                                                        |
| Updater 迁移触及下载/替换事务                              | 中   | 更新流程回归                          | S9.1 只替换 3 个生命周期调用点，下载/校验/复制逻辑**逐字不动**；guard 覆盖范围与原 `begin_lifecycle` 完全一致                                                |

**回滚：** 改动集中在新增目录 `APP/core/actor/`、新增 `APP/client/core.rs`、以及 `setup.rs` / `ipc.rs` / `feat.rs` / `utils/{help,resolve,init}.rs` / `core/clash/core.rs` / `core/service/{ipc,mod}.rs` / `core/updater/instance.rs` / `client/{mod,core_bridge}.rs` / `backend/fake-core/src/main.rs` 的定点修改。S1–S6 可独立成一个 commit（新增代码，零调用点改动，可编译可测），S7–S11 为第二个 commit（接线与删除）。第一个 commit 单独回滚不影响生产路径。

---

## 7. 提交切分建议

1. `test(fake-core): answer GET /version for manager readiness probes` —— S12.1 路径 A（若选路径 B 则是新增 bin）。**单独打头**：它是 T-BK-01 的前置，且对既有测试应为纯 additive，先独立验证不污染；
2. `feat(core): add CoreBackend enum and cancellation-safe OperationId protocol` —— S1–S6 + T-OP / T-BK / T-RV（纯新增，生产路径未变）；
3. `refactor(core): own the core lifecycle in CoreActor` —— S7–S11 + T-SM 回归 + S13。其中 S9.1（Updater）与 S10（删 legacy lease）**必须在同一个 commit 内**——拆开任一半都无法编译。

---

## 8. 明确 out-of-scope（登记去向）

| 项                                                              | 去向                                                                                                |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| typed `CoreErrorKind` 消费（替换 S4 的过渡映射）                | R0 合并 + submodule bump 之后的**独立一步**；bump 本身待用户授权                                    |
| `StopReason::RestartBudgetExhausted` typed 变体                 | **独立上游小 PR**，待用户授权推送（leader 已裁定不并入已收口的 R0 分支）                            |
| apply 管线统一到 `CoreBackend::apply`                           | **PR-5b / B3**（D3=A）                                                                              |
| Promoted / Applied 入 actor、删 `RuntimeLifecycleStore`         | **PR-5b / B1**                                                                                      |
| 删 `rebuild_gate` / `clash_patch_gate`                          | **PR-5b / B2**                                                                                      |
| `change_core` 简化为 commit-first                               | **PR-5b / B4**                                                                                      |
| post-commit 失败矩阵（RQ-01）、apply parity 含 `Noop`（RQ-03）  | **PR-5b 计划**                                                                                      |
| watch snapshot 投影、100 条 `LogFrame` ring、删 `Logger` global | **PR-5c / C1**                                                                                      |
| `set_mode` / `reconcile_mode`、删 5 s 轮询线程与 statics        | **PR-5c / C2**                                                                                      |
| macOS DNS 归入 actor（`MacosDnsGuard`）                         | **PR-5c / C3**                                                                                      |
| UpdaterActor 完整迁移、`UpdaterManager::global()`               | **PR-6d**。5a 只把 Updater 的 **core 依赖**显式注入（S9.1，编译必要条件），updater 自身的全局性不动 |
| `clash-api` workspace 条目                                      | 无 app 侧消费者，暂不加（D1=A）                                                                     |
