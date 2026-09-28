# TCC T10：启动、后台源、恢复与关闭生命周期设计

> **已被取代（2026-09-28）：** 下列内容已被 [Workflow 与生命周期精简计划](../../../plan/2026-09-28-workflow-lifecycle-simplification.md) 取代；正文作为历史保留，不再改写。
>
> - §0.1 第 8 条与 §5 的单飞有序关闭（截止时间、先发请求再等待确认、结构化报告、`begin_terminate`）：改为根 CancellationToken 加各 owner 自行收尾（精简计划 §3.4，L2-2 `be09913ff`）。
> - §1.11 中 tracked task 与 `catch_unwind` 分支交还两个槽、panic 后槽保持 panic 时刻的内容：Runtime 的 actor handler 直接 await 整条命令，panic 不再捕获（精简计划 U5，L3-2 `74b8621cc`）。
> - §1.11 中“决定等待超时（Unresolved）”保留 `live`，以及 `Aborted { NeedsRecovery }` 保留 `live`、不执行 Cancel：决定等待不设期限（L3-1 `de66af65d`），NeedsRecovery 同样执行 Cancel（精简计划 Q-D，L3-3 `1e4a48c3e`）。
> - §1.11 动作表中的“调用方预算耗尽”：同进程调用方不再设预算，只剩 `core/actor_v2` 与 IPC 之间的边界期限，由 PR-0 收口（精简计划 §10）。
> - §12 R42 所说的 100 ms 决定窗口：`decision_wait` 已删除（L3-1）。

日期：2026-09-25（修订 5：codex 评审 1–3、CCG 评审 1 的 L3a 与 L3b，见 §12）
提交路径：`docs/superpowers/specs/2026-09-25-tcc-t10-lifecycle/design.md`（由 leader 提交）
代码基线：`main @ 4f59ca781`（只读核对）；实施基线为 L2 head（`refactor/remove-legacy-config`）。文中行号均指 `4f59ca781`，Task 6a/6b/7 开工前在分支上复核（Ruling R5）。
依据：TCC v2 计划 §2、§8–§11、§12 T10/T11、§13 V31/V33/V35–V37；`2026-09-24-tcc-t{6,7,8,9}-implementation.md` 与 T6–T9 review 记录；`AGENTS.md` §5–§13；Task 5 brief。

Leader 裁定（均有约束力）：

- service 模式下 daemon 未就绪：不启动 core，报 WaitingDependency，自动重探，不静默回退到 Local。
- 恢复模型只保留一个 `LiveAttempt` 槽和一个预写的 `PendingAction` 槽，正常执行和恢复都推进这两个槽。
- 任务切分为 6a（§1 + §4）、6b（§2 + §3）、7（§5）。

T10 完成条件（计划 §12）：**在 Try、AwaitDecision、Cancel、外围 IO 各阶段触发 Close 都有确定结果；service 残留实例不导致重复启动；random-port startup 不二次 patch 源配置。** 另需“受控处理外部文件摄取与订阅完成回执”。

---

## 0. 摘要与前提

### 0.1 决策一览

1. **StartupReconcile**：workflow 执行域内的一次性命令 `Command::StartupReconcile`，实现在新文件 `client/application_workflow/startup.rs`，由 `resolve_setup` 通过 facade 方法 `startup_reconcile()` 调用。流程：取证 → 纯函数 `plan_owner` 决定宿主 → 先满足 stop 意图、再停掉本会话无回执的在跑实例 → 复用 `try_critical` 应用最新 desired → 最后**恰好一次** `publish_full`。
2. **统一恢复模型**（§1.11）：workflow 持有一个 `LiveAttempt` 槽，facade 持有一个 `PendingAction` 槽。
   - `PendingAction` 在**每一个**会改变外部状态的 await **之前**写入；拿到已受理的 ticket 后更新；只在正面观测到完成、或确定未提交时清除。
   - 正常执行和恢复推进同一对槽，所以无论 panic 还是回执丢失，槽里留下的都是最新的动作，不会是过期动作。
   - 隔离的定义：执行域空闲，且任一槽非空。
   - 恢复顺序：先解决 `PendingAction`；再按 `LiveAttempt` 的来源继续——参与源决定的 mutation 走 mutation 恢复；已提交目标走 T8 已提交目标路径或 reestablish；lifecycle 命令走 reestablish。
3. **ServiceActor 保留超时 helper 的句柄**，并提供 `CommandSettled` 查询。service 的 phase 稳定不算完成证据；helper 未结束时拒绝新的变更命令，隔离保持。
4. **所有权与 stop 意图分离**：`Ownership { Unproven, Established { host } }`。
   - 显式启动（`restart_sidecar`、托盘重启、`enhance_profiles`）遇到所有权缺失或不匹配时，运行建立所有权的 reestablish，而不是拒绝。
   - 内部或自动启动路径遇到同样情况时跳过或拒绝。
   - §1.7 列出每条提交 runtime 的路径及其授权条件。
5. **类型收口**：`TargetOrigin` 表达已提交目标的来源；`try_critical` 系列只接收 `&LiveAttempt`；不伪造 `DecisionHandle`，不改 `nyanpasu-core`。
6. **后台源门控**：`ProducerGate { Held, Running, Stopped }`；Stopped 拒绝一切新的刷新和导入；刷新完成消息必须匹配 `RefreshAttemptToken`；边界任务经注入的 `ProducerTasks` 跟踪。
7. **回执**：每个 profile 保留最近一次后台源结果 `SourceStatus`，投影到 `ConfigurationStatus.sources`。
8. **单飞有序关闭**：`NyanpasuClient::shutdown`，spawn 单飞；**先发出所有清理请求，再等待确认**；截止时间用饱和运算；子项结果逐个报告。widget 子进程在 spawn 后立即登记，取消不会让它失去所有者。删除阻塞的 `Drop`。

### 0.2 前提（L1/L2 之后、L4 之前）

- L2 已删除 legacy `Config` / `Draft` / `bridge/` 镜像，以及 `resolve.rs` 中的 session-port 回写。托盘从 `TrayState` 中的 typed 视图渲染；静默启动、窗口几何和 locale 读取 typed 快照。
- L4 将删除 `Handle::global()`、`WindowManager::global()` 和可变 static。本设计不新增对 `Handle` 的依赖，不新增 `::global()` / `OnceCell` / `OnceLock` / `Lazy` 服务状态。
- `ProfilesActor::pre_start` 中的 materialization journal 恢复保持不变。

### 0.3 术语

| 术语                 | 含义                                                                                                              |
| -------------------- | ----------------------------------------------------------------------------------------------------------------- |
| owner / desired 宿主 | router 当前驱动的宿主 / 由 `enable_service_mode` 推出的宿主                                                       |
| 残留实例             | 正在运行、但本会话没有可验证 `RuntimeApplyReceipt` 的 core                                                        |
| 已提交目标           | `DeferredTarget`：源配置已提交、runtime 尚未收敛的目标，来源为 mutation 或 Reestablish                            |
| `LiveAttempt`        | 当前的尝试，或最近一次未结清的尝试：来源、阶段、基线、裁定                                                        |
| `PendingAction`      | 本应用发出、但尚未被正面观测到完成的那一个外部动作                                                                |
| 隔离                 | 执行域空闲，且 `LiveAttempt` 或 `PendingAction` 非空；此时只放行显式 RetryRuntime                                 |
| 生产者               | 订阅调度（含 catch-up）、外部 watcher、materialization ticker、下载、hotkey pump、UI forwarders、效果自动重试计时 |

---

## 1. StartupReconcile（Task 6a）

### 1.1 现状（`4f59ca781`）

- `resolve_setup`（~151–244）依次执行 `probe_service` → `restore_execution_host` → `reconcile_core`（失败只记日志）→ legacy 启动效果管线；每条 lifecycle 命令结束时还各自调用 `notify_committed(true)`（`workflow.rs` ~72–84）。
- 启动失败时不留 deferred 目标：状态显示 Healthy，RetryNow 是空操作（`tcc.rs` ~307）。
- 以 service 模式重启时，残留实例占用 Fixed 端口，候选构建因此失败；之后的每笔修改都被 `NoRestorableBaseline` 拒绝（`tcc.rs` ~686–704）。
- desired=Local 时不检查 service 中是否有 core；desired=Service 但 daemon 未就绪时，静默回退到 Local。
- panic 只把 `lifecycle.uncertain` 置位（`mod.rs` ~457–467）。facade 的 `last_submission` 在操作成功后仍然保留（`facade.rs` ~523–539）；handoff 和 service 命令经另一条路径（`observe_mutation`，~176–185）标记不确定；service 命令超时后，其阻塞 helper 仍在运行（`service_actor.rs` ~209、`service/control.rs` ~54 的 `spawn_blocking`）。

### 1.2 位置与入口

| 组件           | 位置                                                                                                                                                                                                                             |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 命令 / 输出    | `Command::StartupReconcile`（`application_workflow/mod.rs`）/ `Output::Startup(Box<StartupReport>)`（`core_lifecycle/mod.rs`）                                                                                                   |
| 策略           | 新文件 `application_workflow/startup.rs`：`startup_reconcile`、`reestablish`，纯函数 `plan_owner`、`ServiceEvidence::from_probe`、`next_wait`                                                                                    |
| 恢复模型       | 新文件 `application_workflow/attempt.rs`：`LiveAttempt`、`AttemptOrigin`、`AttemptStage`、`TryVerdict`、`recover()`（§1.11、§4）                                                                                                 |
| 待决动作       | `core/actor_v2/facade.rs`：`PendingAction`、`pending_action()`、`action_evidence()`                                                                                                                                              |
| lifecycle 机制 | `core_lifecycle/workflow.rs`：`adopt_ready_service()`（从被删除的 `RestoreExecutionHost` 分支移入）、`retire_unreceipted()`（停止 core 并作废端口，**不**设 `CoreIntent::Stopped`）、`ownership: Ownership`、`start_permitted()` |
| 初始所有权     | `ApplicationWorkflowArgs.ownership`：生产环境由 `with_parts` 传入 `Unproven`；workflow 级测试图注入 `Established { host: Local }`                                                                                                |
| facade         | `client/app_lifecycle.rs`：`NyanpasuClient::startup_reconcile() -> StartupReport`；RPC 失败映射为 `StartupOutcome::Unsettled`                                                                                                    |
| 调用点         | `utils/resolve.rs::resolve_setup`，顺序见下方                                                                                                                                                                                    |

```text
init::init_resources()                       // 保持：geo 资源须在 core 启动前就位
tray::icon::resize_images(..)                // 上移
app.listen("update_systray", ..)             // 上移到 StartupReconcile 之前；删除其后的直接 emit
let report = block_on(client.startup_reconcile());   // 替换 probe/restore/reconcile 块
log(report);
client.start_background_sources();           // Task 6b
storage::setup / clash::setup / start_clash_streams / create_window / jobs / proxies  // 相对顺序不变
```

- **一次性**：`ApplicationWorkflow.startup: Option<StartupReport>` 缓存首次结果；再次收到该命令时直接返回缓存，不探测、不提交、不通知。
- **不是源事务**：启动加载不是 patch；workflow 不持有源配置的写 client（有 T1 静态测试）。
- **阻塞 setup**：阻塞的位置和 180 s 上限都与今天的 `reconcile_core` 相同。

### 1.3 取证与宿主决策（S1–S2）

**S1：取证**（在执行域内）

- **先确认 ServiceActor 没有未结束的工作**：调用 `ServiceClient::command_settled()`（§1.11）。只要返回 false（例如 `pre_start` 自动更新的 helper 仍在运行），本次尝试就以依赖类结果结束，报 WaitingDependency："a service command started earlier is still running"，并按 `next_wait` 重探。这一步之前，S1/S2 不授权任何 runtime 动作（adopt、handoff、stop、提交）。
- `probe_service()` 的结果经 `ServiceEvidence::from_probe` 归为四类：`Absent`（NotInstalled / DaemonStopped）、`CoreStopped { ready }`、`CoreRunning { ready, instance, revision }`、`Unreadable { phase }`（probe 失败、phase 为 Unknown，或状态体不可信）。
- `refresh_status()` 给出宿主、router generation、状态、revision、source_hash、applied_kind。
- 以上汇总为 `StartupObservation`，覆盖 V37 要求的“宿主、实例 generation、运行配置身份”。

**S2：宿主决策**

纯函数 `plan_owner(desired, owner, &ServiceEvidence) -> OwnerPlan`，逐格单测：

| desired | owner   | service 证据                            | 计划                     | 动作                                                                         |
| ------- | ------- | --------------------------------------- | ------------------------ | ---------------------------------------------------------------------------- |
| Local   | Local   | Absent / CoreStopped                    | `Owned`                  | —                                                                            |
| Local   | Local   | CoreRunning{ready}                      | `RetireServiceThenLocal` | adopt → `move_execution_host(Local)`（handoff 过程证明 service core 已停止） |
| Local   | Local   | CoreRunning{!ready} / Unreadable        | `Unproven{phase}`        | 不启动任何东西                                                               |
| Local   | Service | 任意                                    | `HandBackToLocal`        | `move_execution_host(Local)`                                                 |
| Service | Service | 任意                                    | `Owned`                  | —                                                                            |
| Service | Local   | CoreStopped{ready} / CoreRunning{ready} | `AdoptService`           | 只 adopt，不 converge                                                        |
| Service | Local   | Absent / CoreStopped{!ready}            | `WaitForService{phase}`  | 不回退到 Local，不启动 core                                                  |
| Service | Local   | CoreRunning{!ready} / Unreadable        | `Unproven{phase}`        | 不启动任何东西                                                               |

adopt 和 handoff 都先预写 `PendingAction`（§1.11）。router 给出确定回复时清除该动作：回复 not ready → `WaitForService`；回复 `StopUnconfirmed` 或预检失败 → `Unproven`。回复丢失时保留该动作，本次尝试进入隔离。

**决定**：从不调用 `ensure_ready` / install / start_daemon；desired=Service 时从不回退到 Local；只要无法证明不存在第二实例，就不启动 core。

### 1.4 stop 意图与残留实例退役（S3）

在 S2 确定的 owner 上按以下顺序判断：

| 顺序 | 条件                                                      | 动作                                                                                                                    |
| ---- | --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| 1    | 有 stop 意图（对所有原因一律适用），owner `Stopped`       | stop 意图已满足，不做应用                                                                                               |
| 2    | 有 stop 意图（对所有原因一律适用），owner `Running`       | 受控停止，停止后必须观测到 `Stopped`；否则把失败暴露出来（报 Blocked；若有 `PendingAction` 未决则进入隔离），不接受回执 |
| 3    | owner `Running`，回执经 `verify_recovery_target` 验证通过 | 接受，不重新提交（只会出现在恢复路径中）                                                                                |
| 4    | owner `Running`，没有回执或回执未验证                     | `retire_unreceipted()`，之后必须观测到 `Stopped`                                                                        |
| 5    | owner `Stopped`                                           | 以 `KnownRuntimeState::Stopped` 作为基线                                                                                |
| 6    | owner 处于过渡态，或读不到状态                            | 属于证据缺口，按依赖等待处理                                                                                            |

- 计划为 `Owned` / `AdoptService` / `HandBackToLocal` / `RetireServiceThenLocal`，且 S3 停在第 1、3 或 5 步时，写入 `ownership = Established { host }`；其余情况写入 `Unproven`。
- **S3 不为任何原因跳过 stop 检查。** `ExplicitStart` 只作废它**之前**记录的 stop 意图：显式启动命令被受理时，把 `CoreIntent::Stopped` 置回 `Idle`，`Restore` 义务不受影响。之后再有 StopCore，会重新记录 stop 意图，被保留的目标照样要遵守（§1.7）。因此 §1.4 与 §1.5 不再冲突：S4 的前提“没有未解除的 stop 意图”对所有原因都成立。

**决定**：本会话没有可验证回执的在跑实例，先经已证明的 owner 停掉，再从 Stopped 开始应用。原因：local-IPC 设置无法观测，只有被确认的提交才会产生回执；另外，候选端口会与待替换的实例冲突。代价是 GUI 崩溃重启后，service 中的实例会重启一次，但始终只有一个实例。

### 1.5 应用最新 desired（S4）

- 前提：S3 停在第 5 步，且没有未解除的 stop 意图。
- 输入：`capture_inputs` 读取三个准入后已提交的快照；`identity = target_key()`。
- 基线：直接构造——settled、Stopped、run_intent 为 Running、host 为 owner、expected 为最新 status；不经过 `observe_baseline()`。
- 执行：策略固定为 `AllowDeferredWhenSafe`，调用 `try_critical(&live, ..)`。S2 已保证 owner 与 desired 一致，因此 S4 不移动宿主；若发现不一致，防御性地按 WaitForService 处理。
- Applied 时：记录回执、确认端口，然后执行 `publish_committed_product` 和 `accept_transition`。

### 1.6 结果与失败表

| 阶段 | 观测 / 结果                                                     | `StartupOutcome`                                                   | 运行态 / 所有权            | 状态面板 runtime 行                                                     | 后续                                                 |
| ---- | --------------------------------------------------------------- | ------------------------------------------------------------------ | -------------------------- | ----------------------------------------------------------------------- | ---------------------------------------------------- |
| S1   | `command_settled()` 为 false（ServiceActor 的 helper 仍在运行） | ReadyDegraded                                                      | 不启动 core；`Unproven`    | WaitingDependency："a service command started earlier is still running" | 按 `next_wait` 自动重探；helper 结束后继续 S1        |
| S2   | `WaitForService`                                                | ReadyDegraded                                                      | 不启动 core；`Unproven`    | WaitingDependency                                                       | 按 `next_wait` 自动重探；RetryNow；用户可启动 daemon |
| S2   | `Unproven`                                                      | ReadyDegraded（目标 health = RecoveryRequired，不进入隔离）        | 不启动任何东西；`Unproven` | RecoveryRequired："service 可能持有 core"                               | 自动重探（只查询）；service 命令仍可用               |
| S2   | handoff 回复丢失                                                | RecoveryRequired（隔离：`LiveAttempt` + `PendingAction::Handoff`） | 未知                       | RecoveryRequired                                                        | 见 §4                                                |
| S3   | 受控停止确定失败                                                | ReadyDegraded（Blocked："stop 意图未满足"）                        | 旧实例仍在运行             | Blocked                                                                 | RetryNow                                             |
| S3   | 退役结果未知                                                    | RecoveryRequired（隔离：`PendingAction::Submission`）              | 未知                       | RecoveryRequired                                                        | 见 §4                                                |
| S3   | 过渡态 / 无状态                                                 | ReadyDegraded                                                      | 未提交                     | WaitingDependency                                                       | 自动重探                                             |
| S4   | Applied                                                         | Ready                                                              | 有回执；Established        | Healthy                                                                 | —                                                    |
| S4   | Deterministic / Permanent                                       | ReadyDegraded                                                      | Stopped；Established       | Blocked                                                                 | identity 改变的保存会重新排期；RetryNow              |
| S4   | Transient                                                       | ReadyDegraded                                                      | Stopped；Established       | RetryScheduled                                                          | 3 次重试（1/5/30 s）                                 |
| S4   | check `Unserviceable`                                           | ReadyDegraded                                                      | Stopped；Established       | WaitingDependency                                                       | 按 `next_wait`                                       |
| S4   | 提交结果未知                                                    | RecoveryRequired（隔离：`PendingAction::Submission`）              | 未知；端口作废             | RecoveryRequired                                                        | 见 §4                                                |
| 结尾 | 任意结果                                                        | —                                                                  | —                          | —                                                                       | **恰好一次** `publish_full`                          |

`StartupReport { operation_id, observation, outcome }`，其中 `StartupOutcome` 为 `Ready | ReadyDegraded { health, reason } | RecoveryRequired { reason } | Unsettled { reason }`。

### 1.7 提交 runtime 的全部路径及授权条件

不存在“单一漏斗”。下表列出所有会向 core 提交配置、或改变 runtime 的路径：

| #   | 路径                                                                                                           | 位置                                           | 授权条件                                                                                                                                                                                                                           |
| --- | -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | mutation Try                                                                                                   | `tcc.rs::try_critical → submit_runtime`        | 基线已稳定。若基线为 Running，必须有已验证的回执（R10）。换宿主只能经 `move_execution_host`；handoff 完成本身证明 source 已停止，并写入 `Established { target }`。基线为 Stopped 时走 SavedInactive，不提交                        |
| 2   | 已提交目标的自动重试（来源为 mutation）                                                                        | `retry_runtime → try_critical`                 | 同 #1，并以 identity 作为围栏。**自动重试从不移动宿主**：目标宿主与 owner 不一致时报 WaitingDependency（D11）                                                                                                                      |
| 3   | Reestablish S4（启动、重试、恢复、显式启动）                                                                   | `startup.rs → try_critical`                    | 同一次尝试中 S2/S3 已为 desired 宿主建立所有权                                                                                                                                                                                     |
| 4   | 恢复性回滚（Cancel、compensate_handoff、recover）                                                              | `tcc.rs::restore`                              | 只把 `LiveAttempt` 自己记录的基线回执恢复到它记录的宿主上；动作一律预写                                                                                                                                                            |
| 5   | 显式启动：`Command::Reconcile`（IPC `restart_sidecar`、`enhance_profiles`、托盘及 `feat::restart_clash_core`） | `ApplicationWorkflow::execute`                 | 当 `ownership == Established { host }` 且 host 与 owner、desired 都一致时，调用 `lifecycle.reconcile()`；否则运行 `reestablish(ExplicitStart)`，成功返回 `Output::Reconcile(report)`，否则返回描述降级原因的错误并留下可重试的目标 |
| 6   | service 端点恢复中重启被中断的 core                                                                            | `core_lifecycle::recovery_attempt → reconcile` | `start_permitted()`，即 `Established { host: Service }` 且 desired 为 Service；否则跳过                                                                                                                                            |
| 7   | 替换二进制后重启                                                                                               | `replace_binary → reconcile`                   | `start_permitted()`；否则照常安装、跳过重启，并把 Reestablish 目标排到立即执行                                                                                                                                                     |
| 8   | RuntimeDirty / ApplyControlChannel / SetExecutionHost                                                          | 死路由（留给 T11）                             | `start_permitted()`，否则拒绝                                                                                                                                                                                                      |

- **`Ownership` 的写入者只有三个**：reestablish 的 S2/S3；`move_execution_host` 完成（写 `Established { target }`）；构造参数给出的初始值。**StopCore 从不写它。**
- **StopCore 不清除 Reestablish 目标**：stop 意图只让后续尝试跳过 S4。某次尝试证明了所有权并确认 Stopped 之后，目标才清除。
- **较晚的 StopCore 取代保留下来的显式启动授权**：StopCore 被受理时，若保留的目标是 `Reestablish(ExplicitStart)`，就把它改写为 `Reestablish(Recovery)`。这样只保留所有权检查义务，不再保留启动授权。新记录的 stop 意图由 S3 第 1、2 步统一执行，所以依赖就绪后的尝试只做取证、确认 Stopped，不会提交任何东西。
- **停止状态下保存配置**：`observe_baseline()` 推断为 `StoppedByUser`（推断规则不变），走 SavedInactive，stop 意图保持，不启动 core。`confirm()` 的 SavedInactive 分支按目标 identity 处理：
  - 候选 `target_key()` 与 `target.identity` 不同：视为新目标，重置预算，`waits` 归零，立即排期。
  - identity 相同且仍有预算：立即排期，不补充预算。
  - identity 相同且预算已耗尽：不做改变。
  - 没有目标：`deferred = None`（原行为）。
- **N5 场景**：Local 所有权已建立 → StopCore → 保存 `enable_service_mode = true`（SavedInactive，不启动）→ 显式重启被受理，此前记录的 stop 意图随之作废 → 发现所有权与 desired 不一致 → `reestablish(ExplicitStart)`。daemon 就绪则 adopt 并应用；未就绪则留下 WaitingDependency 目标，RetryNow 可用。
- **启动前到达的修改**：此时 UI 尚未创建、生产者处于 Held、热键尚未注册，正常情况下没有写入者。即使有，FIFO 加 SavedInactive 也会让结果收敛。不另设启动闸门。

### 1.8 重试与等待节奏

**应用类结果**：S4 的 Applied / Transient / Deterministic / Permanent，即确实做过构建、检查或提交的尝试。

- 自动尝试**在预写 Submission 时**扣减 `attempts_remaining`，因此 panic 或回执丢失都不会漏扣。
- `attempts += 1`；重试延迟用 `RETRY_DELAYS`。
- 显式 RetryNow 只执行一次，不补充预算。

**依赖类结果**：ServiceActor 仍有未结束的 helper（`command_settled()` 为 false）、WaitForService、Unproven、证据缺口、check `Unserviceable`，以及有 stop 意图时所有权尚未证明。

- 不扣预算，不计入 `attempts`。
- 独立计数器 `DeferredTarget.waits` 饱和加一，上限为 `len-1`；延迟取 `REESTABLISH_WAIT_DELAYS[waits]`，其中 `REESTABLISH_WAIT_DELAYS = [5s, 5s, 10s, 30s, 60s]`。
- 预算已耗尽（`attempts_remaining == 0`）时，依赖类结果立即记为 `Blocked`，不安排任何自动尝试：`reestablish` 入口的闸门会在取证之前拒绝那次自动尝试，安排了也只是先承诺等待、再在没有任何新观测的情况下改判 Blocked。只有显式重试或新的 identity 才会再试（CCG 评审 1）。此时显式调用方（显式启动）得到可重试的 `BackendUnavailable`，因为原因是依赖而不是 runtime 本身；只有 runtime 自己拒绝的 Blocked 才报不可重试的 `ApplyFailed`（R44）。

**`waits` 只在离开依赖等待时归零**，即某次尝试得到应用类结果，或 identity 改变。仅进入 S4 不归零，因为 check `Unserviceable` 就发生在 S4 内部。纯函数签名为 `next_wait(waits, outcome_class) -> (Duration, u8)`。

来源为 mutation 的目标保持 T8 的固定 5 s 等待。

### 1.9 通知

- 新增 `CommitNotifications::publish_full`。EffectsClient 的实现为 `Publish { full: true, refresh: true }`，它取代并删除 `EffectsClient::reconcile`。
- 无论结果如何，StartupReconcile 恰好调用一次 `publish_full`。内部的 adopt / handoff / stop 直接调用 `self.lifecycle.*`，不经过 `Command::Core`，所以不会触发逐命令通知。
- 托盘首次构建由 `Tray(Full)` 经提前安装的监听器完成。
- 重试 Applied 后发 diff 通知（`notify_committed(true)`）。

### 1.10 Random port

绑定的唯一写入者是回执；StartupReconcile 路径上没有任何源配置写入口。**决定**：由测试 S4 固定这一性质。

### 1.11 统一恢复模型：`LiveAttempt` + `PendingAction`

**两个槽**：都是 workflow box 内的普通字段。tracked task 运行时持有它们，并在 `Completed` 中连同 workflow 一起交还 actor（catch_unwind 分支同样如此）。因此 panic 之后，槽里的内容就是 panic 那一刻的内容，**不需要在 panic 时另行复制**。实现约束：两个槽在所有 await 期间都必须留在 workflow box 内，不得临时 `take()` 到可能随 unwind 丢失的局部变量中。把目标从 `self.deferred` 移入 `live` 属于槽之间的移动，两端都在 box 内，满足这一约束。

```rust
// application_workflow/attempt.rs
pub(super) struct LiveAttempt {
    pub operation_id: OperationId,
    pub origin: AttemptOrigin,
    pub stage: AttemptStage,          // Preparing | TryingCritical | AwaitDecision | Confirming | Cancelling | Recovering
    pub baseline: Option<Baseline>,   // KnownRuntimeState + host + binding + run intent；None = 从未观测/触碰 runtime
    pub verdict: Option<TryVerdict>,  // ACK 发出前（源决定）或目标更新前（已提交目标）写入
    pub unresolved: Option<String>,   // 尝试没能结清的原因；正常运行时为 None
}
pub(super) enum AttemptOrigin {
    /// 参与一个未决源事务（新 mutation）；恢复时读 DecisionHandle。
    SourceDecision { domain: ConfigDomain, decision: DecisionHandle },
    /// 调和一个已提交目标（mutation 来源的重试、StartupReconcile、Reestablish）。
    /// 携带目标自身的 identity / 预算 / waits；恢复时继续它，不推断任何源 ACK。
    CommittedTarget(Box<DeferredTarget>),
    /// 一条 lifecycle 命令。
    Lifecycle { command: LifecycleCommand },
}
pub(super) enum TryVerdict { Applied(AppliedVerdict), Deferred { identity: String }, SavedInactive, Saved, Rejected }
/// Applied 的 Try 连同其提交之后仍欠下的义务，随 verdict 一起在 ACK 发出前预写（CCG R36）。
pub(super) struct AppliedVerdict { pub receipt: Arc<RuntimeApplyReceipt>, pub product: Arc<RuntimeSnapshot>, pub releases_service: bool }

// mutation.rs
pub(crate) struct DeferredTarget { operation_id, origin: TargetOrigin, identity: String, baseline, cause,
                                   attempts_remaining, attempts, waits: u8, health, next_attempt }
pub(crate) enum TargetOrigin { Mutation { domain: ConfigDomain }, Reestablish(ReestablishCause) }
pub(crate) enum ReestablishCause { Startup, Recovery, ExplicitStart }

// core/actor_v2/facade.rs：取代 outcome_uncertain + last_submission
pub(crate) enum PendingAction {
    Submission { operation: OperationId, endpoint: EndpointHandle, host: ExecutionHost,
                 generation: ControllerGeneration, accepted: bool, observed_terminal: bool },
    Handoff { target: ExecutionHost, generation_before: ControllerGeneration },
    ServiceCommand { command: ServiceCommandKind },
}
```

**预写规则**（在 facade 内实现，正常执行与恢复中的所有调用都遵守）：

| await                                                                                                       | 调用前写入                                                                        | 中途更新                                                                                                                                                                          | 何时清除                                                                                               | 何时保留                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `submit_and_wait`（reconcile、stop、recover；恢复性回滚也经此路径）                                         | `Submission { operation: 新 id, endpoint: 当前连接的 endpoint, accepted: false }` | 拿到 ticket 后设 `accepted = true`，endpoint 改为 ticket 中的值                                                                                                                   | 观测到终态且分类确定（Reconciled / RolledBack / Unchanged / Stopped …），或 submit 返回 `NotSubmitted` | submit 返回 Unknown；`wait_operation` 返回 None 或非终态；已到终态但 runtime 意外变化或 output 意外（此时 `observed_terminal = true`） |
| `core.change_host`（change_execution_host、adopt_service_host、recover_service_endpoint 的第二段）          | `Handoff { target, generation_before }`                                           | —                                                                                                                                                                                 | router 给出任一确定回复（Ok、预检失败、StopUnconfirmed、ShuttingDown、OperationConflict）              | 调用方预算耗尽，或 actor 不可用（Internal）                                                                                            |
| ServiceActor 的变更命令（ensure_ready、install、start、stop、uninstall、update、recover_endpoint 的第一段） | `ServiceCommand { command }`                                                      | ensure_ready / recover_endpoint 成功时，先消费已完成的 `ServiceCommand`（清空槽），再写入 `Handoff`，然后调用 change_host。这是“先消费、后写入”，不违反“槽非空时拒绝写入”的不变式 | actor 回复 Ok                                                                                          | 回复丢失，或回复 `BackendUnavailable` / `Internal`（与今天 `observe_mutation` 的判据一致）                                             |

- **槽非空时拒绝写入**：返回 `OperationConflict("an earlier action is unresolved")`，绝不覆盖旧动作。恢复总是先解决旧动作、再产生新动作，所以槽里始终是最新的动作。revision 1 的“首条优先”规则因此删除。
- `retire_unreceipted`、S3 的受控停止，以及 restore 中的 handoff 与回滚提交，都同样先预写。

**各类动作的完成证据**（`action_evidence(&PendingAction) -> Settled | Pending(reason)`）：

| 动作             | Settled 的条件                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Submission`     | 只认该操作本身的完成证据：`observed_terminal` 为真，或在**记录下的** endpoint 上 `wait_operation(operation)` 返回 Succeeded / Failed。不再以 router generation 变化作为捷径：同宿主的 degraded 重新 adopt 会推进 generation，却不会停止原实例（`actor_v2/mod.rs` ~1047）。槽非空时不会发生新的 handoff，所以记录下的 endpoint 不会被本应用替换；若它暂时不可达，证据保持 Pending，隔离保持                                                                                                                                                                                                                       |
| `Handoff`        | router 的 mailbox 有序查询 `CoreClient::handoff_settled(generation_before)`（`CoreActorMessage::HandoffSettled`）回答 true。它排在这次 handoff 的 `ChangeHost` 之后处理，所以只剩 stop 腿可能还在跑：只要 router 不再处于从 `generation_before` 开始的 `HandingOff`，这次 handoff 就处理完了。完成并服务、stop 腿失败且源已断开而落入 `Degraded`、被拒绝、被关闭（`ShutDown`）都算 Settled；仍在 `HandingOff` 为 Pending；actor 不可用（查询出错）也是 Pending，不算完成。这是**完成证据，不是健康证据**：`refresh_status()` 对 degraded router 一律报错，健康读既不能证明、也不能否定 handoff 已完成（CCG R35） |
| `ServiceCommand` | `ServiceClient::command_settled()` 为真：actor 已处理完该命令（FIFO 保证它在该命令之后才回答），**并且**没有仍在运行的 helper（见下）。phase 稳定只算状态证据；`Exhausted` 闩锁也不算完成证据                                                                                                                                                                                                                                                                                                                                                                                                                    |

**ServiceActor 改动**（`core/actor_v2/service_actor.rs`；§10 相应收窄非目标）：

- 变更类 adapter 调用改为 `tokio::spawn` 一个持有 `Arc<dyn ServiceHostAdapter>` 的任务，再以 `timeout(command_timeout, &mut handle)` 等待。**`pre_start` 中的启动自动更新（`service_actor.rs` ~381–391）使用同一个包装**；它发生在 facade 存在之前，所以不进入 facade 的 `PendingAction` 槽，而是由 `outstanding` 跟踪，并由 S1 的 `command_settled()` 把关（§1.3）。
- 超时后，句柄保存在 `ServiceActorState.outstanding: Option<(ServiceCommandKind, JoinHandle<Result<(), String>>)>`，并回复 `BackendUnavailable("… still running")`。
- `outstanding` 非空期间，新的变更命令，以及一切会交出 endpoint、从而导向 runtime 所有权的消息（`EnsureReady`、`AdoptIfReady`、`RecoverEndpoint`），都一律返回 `OperationConflict`。因此即使 probe 短暂报告 Ready，也无法绕过仍在运行的 helper 去 adopt。
- 新消息 `CommandSettled(RpcReplyPort<bool>)`：`outstanding` 为空，或其句柄已结束时回复 true；句柄已结束时同时回收句柄、重新 probe 并 publish。
- probe 是只读操作，不纳入跟踪。

**隔离判据**：执行域空闲，且 `live.is_some() || core.pending_action().is_some()`。

- 它取代 `lifecycle.uncertain` 和 revision 1 的 `Isolation` 枚举。
- `CoreLifecycleStatus.uncertain` 与 `configuration_status` 都读这一判据。
- `MutationJournal.recovery` 改为从两个槽投影出的只读视图（operation id、来源种类、阶段、动作种类、`unresolved`）；wire 字段不变。

**各执行路径如何推进两个槽**：

**清除 `live` 的正面规则**：只有同时满足以下条件，才清除 `live`；任一条件不满足，就保留 `live` 并写入 `unresolved`，保持隔离，**即使 `PendingAction` 为空**：

1. 该来源所需的权威结论已经得出；
2. 必要的资源结算已完成；
3. 必要的 runtime 验证已完成；
4. `PendingAction` 为空。

- **mutation**：`run_mutation` 开始时写入 `LiveAttempt { origin: SourceDecision, stage: Preparing, baseline: None }`，之后依次推进：观测基线 → `TryingCritical` → ACK 发出前写入 `verdict` → `AwaitDecision` → `Confirming` / `Cancelling`。只有以下三种情况满足清除规则：`Committed` 且 Confirm 完成（Applied 的收尾是与恢复共用的幂等例程 `finish_applied`，见 §4.2）；`Aborted { Restored }` 且 Cancel 的恢复已验证（或没有需要恢复的 runtime）；Try 被拒且没有触碰 runtime。**决定等待超时（`await_decision` 的 Unresolved，`tcc.rs` ~1155）、`Aborted { NeedsRecovery }`、基线恢复未通过验证**这三种正常返回的情况，都保留 `live`（`unresolved` 写明原因）。
- **已提交目标的重试 / StartupReconcile**：从 `self.deferred` 取出目标（或新建 Reestablish 目标），放入 `LiveAttempt { origin: CommittedTarget(target) }`。尝试以确定的结果结束时（Applied，或已分类的失败 / 等待），把更新后的目标放回 `self.deferred`（Applied 时为 None），然后按清除规则清除 `live`。
- **lifecycle 命令**：写入 `LiveAttempt { origin: Lifecycle { command } }`。命令确定返回、且 `PendingAction` 为空时清除。
- **panic**：`mod.rs` 的 catch_unwind 分支不再设置任何标志，两个槽保持 panic 时刻的内容。mutation 的 ack oneshot 随 `MutationRequest` 一起被丢弃，participant 因此得到 `Ack::Failed`，源事务中止（现有行为）。
- **恢复**：在同一个 `live`（stage 改为 `Recovering`）和同一个 `PendingAction` 槽上推进，见 §4。

`try_critical` / `dispose` / `compensate_handoff` 改为接收 `&LiveAttempt`，从中读取 operation id 和来源；`retry_runtime` 不再合成 `MutationRequest`。不在 nyanpasu-core 中暴露预置的 `DecisionHandle`。

### 1.12 本节删除（Task 6a）

- `resolve_setup` 中的 probe/restore/reconcile 块、直接 emit、`reconcile_application_effects` 块及其降级循环、`Handle::update_systray_part()`。
- `client/mod.rs::{probe_service, restore_execution_host}`、`ApplicationWorkflowClient::{probe_service, restore_host}`、`Command::{ProbeService, RestoreExecutionHost}`。
- `client/effects/mod.rs::{reconcile_application_effects, effect_inputs}`、`EffectsClient::reconcile`。
- `facade.rs::{outcome_uncertain, last_submission, original_operation_terminal, accept_verified_recovery}`，由 `PendingAction` 取代。
- `CoreLifecycleWorkflow.uncertain`、`ApplicationWorkflow.recovery`、`RecoveryContext` 类型（由 `LiveAttempt` 取代）、`enter_recovery`；`DeferredTarget.{domain, decision, digest}` 改为 `origin` / `identity`。
- `tcc.rs`：`retry_runtime` 中合成的请求、“没有恢复上下文”的 Err 分支（~288–294）；`inspect_mutation_recovery` 并入 `attempt.rs::recover()`。
- 测试：`boot_restore_*` 改写为 S5 / S8；`TestControlEndpoint::prime` 改为调用 `startup_reconcile()`；直接调用 `reconcile_core()` 的测试改为先 prime。

**决定（§1）**：StartupReconcile 依次执行 S1–S4，最后恰好一次 `publish_full`。所有权与 stop 意图分离；显式启动时补建所有权；每条提交路径的授权条件逐条列明。恢复只依赖 `LiveAttempt` + `PendingAction` 两个槽和预写规则，正常执行与恢复共用这两个槽。

---

## 2. 后台生产者门控（Task 6b）

### 2.1 现状

- `post_start`（~1122–1146）在构造时就以 catch_up 方式布置调度、watcher 和 ticker；`reconcile_committed`（~428–436）每次提交后重新布置。
- 下载任务未被跟踪（~1497、~1705）。刷新的 pending 表只按 UID 建键（~1495），缺条目时仍会提交（~1542–1547）。
- `setup.rs` 里的 pump（~115）和 forwarders（~204–241）是裸 spawn。

### 2.2 ProfilesActor 门控

```rust
enum ProducerGate { Held, Running, Stopped }                 // pre_start 初始为 Held
ProfilesActorMessage::StartProducers
ProfilesActorMessage::StopProducers { reply: RpcReplyPort<ProducersStopped> }
#[derive(Clone, Copy, PartialEq, Eq)] pub(crate) struct RefreshAttemptToken(u64);
```

| 输入                                                | Held                                                                               | Running                                                                                                                                                                     | Stopped      |
| --------------------------------------------------- | ---------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| `StartProducers`                                    | 进入 Running：`scheduler.reconcile(.., catch_up = true)`，并启动 watcher 和 ticker | no-op                                                                                                                                                                       | no-op        |
| `StopProducers`                                     | 进入 Stopped                                                                       | 进入 Stopped：停止调度、watcher 和 ticker；对每个 pending 的刷新和导入先 `abort()`，再在 1 s 内等待其结束；手动发起的调用方收到“application is shutting down”；删除这些条目 | 幂等         |
| `RefreshRemote { Manual }`                          | 处理                                                                               | 处理                                                                                                                                                                        | **拒绝**     |
| `RefreshRemote { Scheduled }`                       | 丢弃                                                                               | 处理                                                                                                                                                                        | 丢弃         |
| `ImportRemote`                                      | 处理                                                                               | 处理                                                                                                                                                                        | **拒绝**     |
| `ExternalFileChanged` / `ReconcileMaterializations` | 丢弃                                                                               | 处理                                                                                                                                                                        | 丢弃         |
| `reconcile_committed`                               | 只重建 index                                                                       | 同时 reconcile 调度和 watcher                                                                                                                                               | 只重建 index |
| `CommitRefreshed { uid, token, .. }`                | 仅当 `pending_refresh[uid].token == token` 时处理；否则丢弃，不删条目，也不写回执  | 同左                                                                                                                                                                        | 同左         |

- `PendingRefresh { token, origin, reply, task }`、`PendingImport { .., task }`；`CommitImported` 本来就按 token 匹配。
- `post_start` 不再布置任何生产者。
- facade 的 `start_background_sources()` 由 `resolve_setup` 在 `startup_reconcile()` 返回后调用；即使返回 Unsettled 也照常调用，因为生产者触发的修改在 FIFO 中排在启动命令之后。

### 2.3 composition root 的边界生产者

- `ProducerTasks { stop: CancellationToken, tasks: TaskTracker }` 位于 `client/app_lifecycle.rs`，不依赖 Tauri。
  - `track(fut)`：把任务包装成可取消、且被跟踪的 future。
  - `stop(budget)`：cancel → close → 有界等待。
- `ClientSetupArgs.producers` 由 composition root 创建并注入。`setup.rs` 用 `tauri::async_runtime::spawn(producers.track(..))` 启动 hotkey pump 和三个 forwarder。
- 托盘 proxies 接收任务不在此列，归 L4 处理。

**决定（§2）**：生产者只在 `StartProducers` 之后、`StopProducers` 之前运行；Stopped 拒绝一切新工作；刷新完成消息按 attempt token 匹配；边界任务经注入的 `ProducerTasks` 跟踪。

---

## 3. 订阅完成回执与外部摄取（Task 6b）

| 要求                               | 决定                                                                                                                                  |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| T10 订阅完成回执                   | 每个结束的后台源都写一条 `SourceStatus`                                                                                               |
| 尊重 `RefreshOrigin`               | 按 §2.2 表处理。同一 UID 已有刷新在途时，定时刷新合并为 `Superseded`，手动刷新维持现有错误。无论哪种来源，结束时都更新 `SourceStatus` |
| V31 过期结果只丢弃自身             | 指纹不符时记为 `Superseded`；token 不匹配的完成消息直接丢弃                                                                           |
| V33 外部内容不可接受时如实反映状态 | 记为 `Rejected { code: "external_source_rejected" }`；保留 last-good runtime；不改回外部文件                                          |

```rust
// state/profiles/sources.rs（specta 导出）
pub enum SourceOrigin { ScheduledRefresh, ManualRefresh, ExternalFile }
pub enum SourceOutcome { Committed { operation_id: Option<String> }, Superseded { reason: String },
                         Failed { message: String }, Rejected { code: String, message: String } }
pub struct SourceStatus { pub profile: ProfileId, pub name: String, pub origin: SourceOrigin,
                          pub outcome: SourceOutcome, pub health: ConvergenceHealth, pub at: i64 }
pub struct SourcesSnapshot { pub event_seq: u64, pub entries: Vec<SourceStatus> }
```

- health 映射：Committed / Superseded → Healthy；Failed / Rejected → Blocked。
- ProfilesActor 独占这份状态：每个 profile 只保留最新一条，删除 profile 时一并修剪，通过注入的 `watch::Sender` 发布。
- `ConfigurationStatus.sources` 和 `event_seq` 合并这份快照；forwarder 多监听一路。
- 前端只渲染 health 不为 healthy 的行；新增 i18n 键 `configuration_sources`；bindings 重新生成。

**结果分类**（订阅刷新先查 profile 是否仍在、定义与 URL 是否未变，再看下载结果：围栏不通过时，下载失败同样记为 Superseded）：

| 来源        | 结果                                         | `SourceOutcome`                      |
| ----------- | -------------------------------------------- | ------------------------------------ |
| 订阅刷新    | 下载或校验失败                               | Failed                               |
| 订阅刷新    | 定义已被替换、profile 已删除，或旧下载已失效 | Superseded                           |
| 订阅刷新    | 提交成功                                     | Committed                            |
| 订阅刷新    | 源事务或 Try 拒绝                            | Rejected(`subscription_rejected`)    |
| 外部 Mirror | 读取失败                                     | Failed                               |
| 外部文件    | 内容校验失败，或提交 / Try 被拒              | Rejected(`external_source_rejected`) |
| 外部文件    | 提交成功                                     | Committed                            |

补记（2026-09-26，按 Task 6b 实现）：`commit_file_first` 的任何错误都归入 Rejected，订阅刷新为 `subscription_rejected`，外部 Mirror（含 Symlink 的状态写入）为 `external_source_rejected`。这包括 materialization prepare / promote 失败、participant 或 Try 拒绝、persist IO 错误和版本冲突，不只上表的“源事务或 Try 拒绝”。分类依据是失败的步骤，不解析错误文本。

**为什么不只记日志**：V33 要求状态如实，而 T9 的状态区是用户能看到的事实来源。

**决定（§3）**：ProfilesActor 独占的每个 profile 最新一条 `SourceStatus` 就是回执，经 `ConfigurationStatus.sources` 暴露。

---

## 4. 恢复（Task 6a）

### 4.1 入口

入口沿用 `retry_configuration_runtime` → `Command::RetryRuntime { explicit: true }`，不新增 IPC，bindings 不变。隔离期间 `drive()` 只放行显式重试；自动计时从不执行恢复。

### 4.2 `recover()`（`attempt.rs`）：先解决动作，再按来源继续

**第 1 步，解决 `PendingAction`**：

- `action_evidence` 为 `Pending`：返回 `OperationConflict(reason)`，保持隔离。
- 为 `Settled`：消费（清除）该槽，但保留 `live` 和隔离，并把 `live.stage` 设为 `Recovering`。

**第 2 步，按 `live.origin` 继续**：这一步中每个带副作用的动作都经过 facade，因此都会**预写一个新的** `PendingAction`。若回复丢失或 panic，槽里留下的是这个新动作，而不是已经解决的旧动作。

**第 3 步，结清**：`live = None`，然后 `publish` 并调用 `notify_committed(true)`。

**`SourceDecision`**（新 mutation；以 `DecisionHandle` 给出的决定为准）：

| 决定                                    | baseline | verdict                                        | 动作                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------- | -------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Undecided / `Aborted { NeedsRecovery }` | —        | —                                              | 保持隔离                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `Aborted { Restored }`                  | None     | —                                              | 从未触碰 runtime，直接结清                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `Aborted { Restored }`                  | Some     | —                                              | 恢复并验证基线（T8）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `Committed`                             | —        | `Applied(v)`                                   | 验证 `v.receipt`；不符则恢复到它（T8）。然后运行 Confirm 用的同一个幂等例程 `finish_applied(v)`：清除被取代的 deferred 目标，发布已提交的 runtime 产物 `v.product`，离开 service 模式时（`v.releases_service`）按状态检查释放旧 daemon。决定等待超时后源才提交时，也走这一行。发布失败与 Confirm 中一样记为可重试的 maintenance 降级（`pending_product` → `ConfigurationStatus.maintenance`），之后的显式重试会重新发布，绝不丢弃；释放 daemon 失败同样记为可重试的 maintenance（`pending_release`，`service_stop_failed`），之后的显式重试在仍为 Local 期望宿主时再次按状态检查释放（R43）。**中断连接不重放**：它只属于切换那一刻，事后再做会切断切换之后新建的连接，所以有时限、不可重试（CCG R36） |
| `Committed`                             | Some     | `Deferred { identity }`                        | 验证基线，必要时恢复；然后安装来源为 mutation 的目标。若 `self.deferred` 已有 identity 相同的目标，沿用它的 `attempts_remaining` / `attempts` / `waits`（与 T8 `confirm` 相同，`tcc.rs` ~1256）；只有真正的新目标才给满额预算。最后结清                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `Committed`                             | — / None | `Saved` / `SavedInactive`，或 baseline 为 None | 结清并通知                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `Committed`                             | Some     | None / `Rejected`                              | 矛盾（没有 Ok ACK 却已提交），保持隔离                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

**`CommittedTarget(target)`**（调和已提交目标：不存在新的源 ACK，也不做矛盾推断）：

- 若 `target.origin = Mutation`、baseline 为 Some，且观测到的 runtime 与基线一致（或基线为 Stopped、观测也为 Stopped）：把目标原样放回 `self.deferred`。预算已在预写 Submission 时扣过，这里不重复扣，也不补充。然后结清。
- 其他情况（与基线不一致，或目标来源为 Reestablish）：执行 `reestablish(ReestablishCause::Recovery)`，沿用目标携带的预算和 `waits`。

**`Lifecycle { command }`**：执行 `reestablish(Recovery)`，stop 意图优先（§1.4）。若 stop 意图未能满足，保持隔离，并设 `unresolved = "stop intent not satisfied"`。

预写规则保证：`PendingAction` 为空就意味着没有任何动作在途（每个变更 await 之前都已写入）。因此 lifecycle panic 不需要额外的“稳定证据”。

**决定（§4）**：恢复时，先按当前 `PendingAction` 自身的完成证据解决它，再依 `LiveAttempt` 的三种来源分别继续。恢复中的每个副作用都预写新的 `PendingAction`。已提交目标沿用 T8 路径或走 reestablish，不做源协议推断。

---

## 5. 单飞有序关闭（Task 7）

### 5.1 现状

- `utils/help.rs::cleanup_processes`（~243–274）不是单飞：它先保存 SessionState，在 Closing 之前就恢复代理，不停止 actor 或生产者，只有逐步的超时。
- executor 的 shutdown 是串行的，并且先停 widget（`executor.rs` ~210–229）。
- widget 的 `start()` 先 spawn 子进程，要等 IPC 握手完成后才把它登记进 `self.instance`（`widget.rs` ~109–138）；`stop()` 先 `take()` 再 await。`WidgetManager::drop` 会阻塞，而且每个 clone 被 drop 时都会触发。
- closing 期间，一旦 workflow 空闲就会自动停止 core（`mod.rs` ~362–376）。

### 5.2 位置与单飞

- 入口在 `client/app_lifecycle.rs`：`NyanpasuClient::shutdown(&self, ShutdownRequest) -> Arc<ShutdownReport>`。
- 首次调用时 `tokio::spawn` 编排任务，并把它的句柄以 `Shared` 形式存入 `NyanpasuClientInner.shutdown: tokio::sync::OnceCell<..>`。后续调用只 await 这个句柄；丢弃调用方不会取消编排；后到的请求被忽略。
- `ShutdownRequest { main_window: Option<WindowState> }` 由边界捕获；`ClientSetupArgs.shutdown_budgets: ShutdownBudgets` 在测试中可注入。
- 边界（`help.rs`）：`capture_main_window_geometry` → `block_on(client.shutdown(..))` → 设置 Windows 就绪标志。各退出调用点不变。

### 5.3 截止时间：先发出请求，再等待确认

```text
deadline         = started + budgets.overall                          // 生产环境 75 s
remaining(now)   = deadline.saturating_duration_since(now)
wait(step)       = min(step.cap, remaining(now))                      // StopCore 为 remaining.saturating_sub(session + actors)
```

每个步骤和子项都分两段执行：

1. **发出（issue）**：同步或非阻塞地发出全部请求——cast、`ActorRef::stop(None)`、取消令牌，或先把 RPC future 创建出来并 poll 一次，让请求入队（ractor 的 `call` 在首次 poll 时同步发送）。这样即使剩余预算为零，也保证每个子项都已发出。
2. **等待（await）**：对全部确认做 `join_all`，受 `wait(step)` 约束。

报告为每个子项给出三种结果之一：

- `Done`：收到正面确认。
- `Incomplete("issued, not acknowledged")`：已发出，但在预算内未确认。
- `NotAttempted(reason)`：只用于**已知**的前置条件失败，即有正面证据表明请求没有发出。例如发送时 actor 已经死亡、`cast` 或发送本身立即失败。仅仅没有收到回复，不足以证明“未尝试”：请求可能已经执行，甚至已经完成。

生产环境各步上限之和为 47 s，小于 75 s，StopCore 至少能拿到 28 s。owner 的执行上界不由 RPC 超时推导：RPC 超时只限制**等待**，请求本身仍留在 owner 的邮箱里。

### 5.4 步骤

| #   | 步骤                 | 发出                                                                                                                                                                                          | 等待                                                                                                                                | 上限 | 报告                                                                                                                                                                                                                                    |
| --- | -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `CloseAdmission`     | `begin_closing()` → `Message::BeginClosing`：设置 closing；取消 dirty / recovery / convergence 计时；拒绝排队中的请求（排队 mutation 收到 `TryAck::Rejected`）；不碰活动任务，也不发 Shutdown | `ClosingAck`                                                                                                                        | 2 s  | Done{rejected, active} / Incomplete                                                                                                                                                                                                     |
| 2   | `StopProducers`      | 同时发出：`stop_producers`、`ProducerTasks::stop`（先 cancel）、`hold_retries`、updater `shutdown`                                                                                            | 等待这四个确认                                                                                                                      | 5 s  | 四个子项                                                                                                                                                                                                                                |
| 3   | `SettleTransactions` | 无（只等待）                                                                                                                                                                                  | 在 status watch 上等待 `shutting_down && active.is_none()`                                                                          | 20 s | Done{settled, isolated} / Incomplete(op, stage)；若为 Incomplete，第 5 步改为直接停止 core                                                                                                                                              |
| 4   | `SealEffects`        | `EffectsClient::shutdown()`（§5.5）                                                                                                                                                           | EffectsActor 回复三个子项的状态                                                                                                     | 12 s | 子项 `SystemProxy` / `Hotkeys` / `Widget`；EffectsActor 未回复时，父步骤记为 `Incomplete("shutdown issued, completion unconfirmed")`，三个子项记为 `Incomplete("unconfirmed")`；只有发送 Shutdown 本身立即失败时，才记为 `NotAttempted` |
| 5   | `StopCore`           | 第 3 步为 Done 时，发 workflow `shutdown()`（`CoreCommand::Shutdown`）；否则直接发 `CoreClient::shutdown()`（CoreActor 的 ShutDown 槽保证幂等）                                               | 回复                                                                                                                                | 余量 | Done{proven stop / nothing running} / Incomplete                                                                                                                                                                                        |
| 6   | `SaveSession`        | `save_main_window(request.main_window)`                                                                                                                                                       | 回复                                                                                                                                | 3 s  | Done / Skipped / Incomplete                                                                                                                                                                                                             |
| 7   | `StopActors`         | 同时发出：`streams.stop()`，以及对 Streams、Proxies、Updater、Profiles、Effects、ApplicationWorkflow、Application、ClashConfig、SessionState 的 `ActorRef::stop(None)`                        | 对每个 `ActorCell` 的终止做 `join_all`；之后先用 tracing 输出前面各步的报告，再发出 `app_logs.shutdown()`，其确认只等到整体截止时间 | 5 s  | 逐个 actor 报告；日志停止的结果记入 `ShutdownReport.logs`                                                                                                                                                                               |

core 生命周期策略：停止 owner 上由本应用驱动的 core，daemon 继续运行。CoreActor、ServiceActor、SystemProxy 与 Hotkey 这几个 actor 留给进程退出时处理。

### 5.5 SealEffects

EffectsActor 的 `Message::Shutdown` 是幂等的：首次结果会被缓存，重复调用直接返回。处理顺序如下：

1. **封闭**：设 `closed = true`，中止计时器，清空 pending。
2. **发取消信号**：调用 `ApplicationEffectsPort::begin_shutdown()`（同步、非阻塞）。executor 在其中调用 `SystemProxyClient::signal_shutdown()`，只触发取消令牌。
3. **收回组任务**：对每个组包装任务执行 `abort()`，再在 1 s 内等待它结束。这样做安全的前提是：组 future 中所有持有资源的部分，都已在第一个 await 之前把资源登记给了 owner。SystemProxy 和 Hotkey 这类纯 RPC 等待方天然满足这一点；widget 在 §5.6 改造后也满足。因此 abort 丢掉的只是等待方，不会丢掉资源。
4. **独立清理**：调用 `port.shutdown(budget)`。executor 先**同时发出**三项清理，再用 `join!` 按各自上界等待：
   - `SystemProxy`：`restore()`。按 FIFO 排在在途写入之后，closed 状态下不会被重新安装；上界 5 s，超时报 “restore queued but not confirmed”。
   - `Hotkeys`：`unregister_all()`。
   - `Widget`：`stop()`，上界 `WIDGET_STOP_BOUND = 3 s`。

   任一项挂起都不会拖延另外两项。

### 5.6 widget 子进程所有权（`widget.rs`）

- **spawn 后立即登记**：`start()` 全程持有 `instance` 锁。`spawn()` 返回后、第一个 await 之前，就执行 `instance.replace(WidgetInstance::Starting { process })`；握手成功后再切换为 `Running { tx, process }`。这样即使 `start` 的 future 在握手中途被丢弃，子进程仍记录在 manager 的状态里。
- **`stop()` 不先 take**：持锁通过 `instance.as_mut()` 操作。`Starting` 状态直接 kill；`Running` 状态先发 Stop，宽限期过后再 kill。确认进程已退出后才清空槽位。若 `stop` future 因超时被丢弃，实例仍留在槽中，报告 `Incomplete("widget process still owned, exit not confirmed")`。
- **`kill_on_drop(true)`**：子进程以此选项启动，作为实例被 drop 时的最后兜底。
- **握手线程的解除与保留**：
  - 问题：握手在 `spawn_blocking` 中阻塞于 `IpcOneShotServer::accept()`（`nyanpasu-egui/src/ipc.rs` ~31），该调用没有超时。子进程若在连接前死掉，父进程的监听端并不会因此关闭（`widget.rs` ~121）。
  - 保留句柄：`Starting` 状态额外保存握手 worker 的 `JoinHandle` 和一次性 server 名 `server_name`。
  - 解除阻塞：在 `nyanpasu-egui/src/ipc.rs` 新增 `release_server(server_name)`。它以客户端身份连接自己的一次性 server，并发送一个一次性 sender，使 `accept()` 返回（backend/nyanpasu-egui 不属于 runtime 子模块，可以修改）。
  - 停止流程：`stop()` 处理 `Starting` 时依次执行：kill 子进程 → `release_server` → 在 `WIDGET_STOP_BOUND` 的剩余时间内等待 worker 句柄结束。
  - 超时时：若 worker 仍未结束，报告 `Incomplete("widget handshake worker still blocked")`，句柄继续保留在槽中，不宣称它已随子进程退出。
- 删除 `impl Drop for WidgetManager`。
- **controller 的禁用同样无条件 stop**：`apply(Disabled)` 与关闭路径的 `stop()` 一样调用有界的 `runtime.stop()`，不先问 `is_running()`。失败启动的清理未能确认时，实例仍为 `Starting`，它并不在运行；禁用必须重试这次清理，确认之前 widget 效果保持 degraded。槽为空时 stop 是空操作。

### 5.7 其他改动

- `drive()` 的 closing 分支只在有 shutdown 等待者、或处于 abandoned 状态时才发出 Shutdown。
- 新增 `ApplicationWorkflowClient::{begin_closing, wait_settled}` 和 `EffectsClient::hold_retries`。
- `ApplicationEffectsPort` 新增 `begin_shutdown`；`shutdown` 增加 `budget` 参数。

### 5.8 报告

```rust
pub struct ShutdownReport { pub steps: Vec<ShutdownStepReport>, pub logs: StepOutcome, pub elapsed: Duration }
pub struct ShutdownStepReport { pub step: ShutdownStep, pub outcome: StepOutcome, pub elapsed: Duration,
                                pub children: Vec<(&'static str, StepOutcome)> }
pub enum ShutdownStep { CloseAdmission, StopProducers, SettleTransactions, SealEffects, StopCore, SaveSession, StopActors }
pub enum StepOutcome { Done { detail: Option<String> }, Skipped { reason: String },
                       Incomplete { reason: String }, NotAttempted { reason: String } }
```

只有正面证据才报 `Done`。任一子项的结果不是 Done 或 Skipped，父步骤就不能报 Done。

`logs` 是应用日志停止的结果：它在各步报告写入日志之后才发出，所以不在 `steps` 里；超出整体截止时间仍未确认时为 `Incomplete`。`elapsed` 在它之后才定稿，包含这段等待。

### 5.9 本节删除（Task 7）

- `help.rs` 中的编排体。
- `shutdown_core`、`shutdown_application_effects`、`shutdown_logs`。
- widget 的 `Drop`，以及 `start()` 中延后登记的写法。
- `setup.rs` 中的裸 spawn。
- `drive()` 中无条件发出 Shutdown 的逻辑。
- executor 串行的 shutdown。

**决定（§5）**：单飞编排依次执行七步，每步都先发出、再等待；截止时间用饱和运算；子项逐个报告。SealEffects 依次执行：封闭 → 取消信号 → 收回等待方 → 并发、独立的清理。widget 子进程在 spawn 后立即登记所有权，任何取消都不会让它失去所有者。

---

## 6. 测试矩阵（fake port、TempDir、barrier、测试时钟）

复用现有夹具：`TestControlEndpoint`、`host_transition_client_seeded`、fake service adapter、`ParkedEndpoint`、`BlockingBuilder`、profiles mock。

新增夹具：

- 可录制的 `CommitNotifications`；
- 共享事件日志；
- 可在指定阶段 panic 的 build / validator fake；
- `wait_operation` 可配置为 panic、返回 None 或非终态的 endpoint fake；
- 变更调用停在 barrier 上的 service adapter fake；
- 握手可被阻塞的 widget runtime fake。

| ID              | 场景                                                                                                                                                   | 断言                                                                                                                                                                                                                                    | 位置                                                           | 覆盖                  |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | --------------------- |
| S1–S2           | `plan_owner` 全表；`ServiceEvidence` 归类                                                                                                              | 逐格                                                                                                                                                                                                                                    | `startup.rs` 单测                                              | V37                   |
| S3              | 干净的 Local 启动                                                                                                                                      | Ready；1 次提交；1 次 full；0 次 committed；Established                                                                                                                                                                                 | `tests/startup.rs`                                             | §1                    |
| S4              | Random mixed port                                                                                                                                      | 四个域的源版本与文件字节不变；绑定为实际选中的端口；1 次提交                                                                                                                                                                            | 同上                                                           | random-port           |
| S5 / S6         | 残留 service core，desired 分别为 Service / Local                                                                                                      | 分别为：先 stop 再 reconcile / 先 adopt 再 handoff 回 Local；始终只有一个实例                                                                                                                                                           | 同上                                                           | V37                   |
| S7              | service 不可读，desired=Local                                                                                                                          | 0 次提交；目标为 RecoveryRequired 且未隔离；`Unproven`；service 命令仍可用；service 变为 Stopped 后自动进入 Ready                                                                                                                       | 同上                                                           | V37                   |
| S8              | desired=Service，daemon 缺席                                                                                                                           | WaitingDependency；不执行 install / start；daemon Ready 后自动 adopt 并进入 Ready                                                                                                                                                       | 同上                                                           | leader 裁定           |
| S9 / S10 / S10b | 瞬时失败；确定性失败后修复；相同内容连续保存 5 次                                                                                                      | 符合 §1.7、§1.8 的预算规则                                                                                                                                                                                                              | 同上                                                           | 发现 7                |
| S11             | S4 提交结果未知                                                                                                                                        | 进入隔离；槽中为 `PendingAction::Submission{accepted}`；操作终结后才继续                                                                                                                                                                | 同上                                                           | V35                   |
| S12 / S13 / S14 | 再次调用 StartupReconcile；每种结果只有一次 full；启动前到达的 mutation                                                                                | 分别为：返回缓存 / 恰好一次 / 最终收敛                                                                                                                                                                                                  | 同上                                                           | —                     |
| S15a–c          | 不可读 service + StopCore；Service 不可用 + StopCore；所有权已建立 + StopCore                                                                          | 分别为：保留目标 / 不回退到 Local / 清除目标                                                                                                                                                                                            | 同上                                                           | 发现 2                |
| S16             | 所有权未建立时替换二进制                                                                                                                               | 完成安装，跳过重启，目标重新排期                                                                                                                                                                                                        | 同上                                                           | §1.7 #7               |
| S17             | 等待退避                                                                                                                                               | 依赖类结果的间隔依次为 5、5、10、30、60、60 s；**check `Unserviceable` 反复出现时同样逐级退避**；只有应用类结果才让 `waits` 归零；`attempts` 与预算不受影响                                                                             | `startup.rs` 单测 + `tests/startup.rs`                         | N7                    |
| S18             | 显式启动时所有权不匹配                                                                                                                                 | Local 已建立 → StopCore → 保存 `service_mode = true`（SavedInactive，不启动）→ `reconcile_core()` 运行 `reestablish(ExplicitStart)`：daemon Ready 时 adopt 并应用；未 Ready 时报 WaitingDependency 并留下可重试的目标（RetryNow 可用）  | 同上                                                           | N5                    |
| S19             | 停止状态下保存配置                                                                                                                                     | stop 意图保持；0 次提交                                                                                                                                                                                                                 | 同上                                                           | N5                    |
| S20             | ExplicitStart → WaitingDependency → StopCore → 依赖就绪                                                                                                | StopCore 把目标改写为 `Reestablish(Recovery)`；依赖就绪后的尝试只做取证并确认 Stopped，**0 次应用提交**；所有权证明完成后目标清除                                                                                                       | `tests/startup.rs`                                             | 评审 3 #2             |
| S21             | 启动自动更新的 helper 停在 barrier，且其 probe 报告 Ready                                                                                              | StartupReconcile 报 WaitingDependency；helper 结束前 0 次 runtime 提交、0 次 adopt（`AdoptIfReady` 返回 `OperationConflict`）；barrier 释放后下一次尝试进入 Ready                                                                       | `tests/startup.rs` + `service_actor` 测试                      | 评审 3 #3             |
| S22             | 预算耗尽后显式重试遇到依赖不可用；随后显式启动                                                                                                         | 目标立即为 Blocked，`next_attempt` 为 None；不扣预算，不计 `attempts`，0 次提交；显式启动得到可重试的 `BackendUnavailable`，目标仍为 Blocked                                                                                            | `tests/startup.rs`                                             | CCG 评审 1            |
| L1              | Try 期间 panic                                                                                                                                         | `live` 为 SourceDecision 且 baseline 非空；走 mutation 恢复：决定为 Aborted 时恢复基线                                                                                                                                                  | `tests/recovery.rs`                                            | 发现 1                |
| L2              | Confirm 期间 panic（verdict 分别为 Applied / Deferred / Saved）                                                                                        | 分别为：验证回执并运行 `finish_applied` / 安装目标 / 结清并通知。**Deferred 的补充情形**：`self.deferred` 已有 identity 相同且预算耗尽的目标时，恢复后沿用该目标的计数，预算仍为 0，不补满；identity 不同时才给满额预算                 | 同上                                                           | 发现 1、评审 3 #6     |
| L3 / L4         | Cancel 期间 panic；Preparing 期间 panic                                                                                                                | 分别为：恢复基线 / baseline 为 None，决定为 Aborted 后直接结清                                                                                                                                                                          | 同上                                                           | 发现 1                |
| L5              | 提交已被接受，waiter panic，而操作仍在运行                                                                                                             | `PendingAction::Submission{accepted: true}` 保留；操作终结前 Retry 保持隔离；终结后走 mutation 恢复                                                                                                                                     | 同上                                                           | **N1**                |
| L6              | 恢复中出现第二个动作：A 已终结，恢复性提交 B 丢失回执                                                                                                  | 槽中为 B，A 已被消费；再次 Retry 只检查 B，不会再发一次恢复                                                                                                                                                                             | 同上                                                           | **N2**                |
| L7              | 恢复中第二个动作期间 panic：B 在途时 panic                                                                                                             | 槽中为 B；stage 为 `Recovering`                                                                                                                                                                                                         | 同上                                                           | **N2**                |
| L8              | service install 的 helper 超出 bound（adapter 停在 barrier），probe 显示稳定的 Ready                                                                   | 保持隔离；新的变更类 service 命令返回 `OperationConflict`；barrier 释放后 `command_settled` 为 true，随后执行 reestablish                                                                                                               | `tests/recovery.rs` + `service_actor` 测试                     | **N3**                |
| L9              | 来源为 mutation 的自动重试丢失运行回执                                                                                                                 | `live` 为 CommittedTarget；操作终结后基线验证一致，目标原样放回，预算只扣一次；之后显式重试 Applied；**不**按矛盾处理                                                                                                                   | 同上                                                           | **N4**                |
| L10             | handoff 回复丢失，router 停在 HandingOff；之后 handoff 或完成，或在源断开后失败而使 router 落入 Degraded                                               | 仍在 HandingOff 时 Retry 保持 Pending 与隔离；handoff 结束后（包括 Degraded、此时一切状态读都被拒绝）恢复消费该动作并执行 reestablish                                                                                                   | 同上 + `actor_v2` 测试                                         | 发现 4、CCG R35       |
| L11             | StopCore 失败后的恢复                                                                                                                                  | 执行受控停止；确认 Stopped 之前不结清                                                                                                                                                                                                   | 同上                                                           | 发现 5                |
| L12             | 自动计时；T8 回归                                                                                                                                      | 自动计时从不执行恢复；既有测试通过                                                                                                                                                                                                      | 同上                                                           | —                     |
| L13             | 正常返回：Try 成功、`PendingAction` 为空，但决定等待超时（常规预算；被测 mutation 的写入停住后暂停时钟并推进过预算，其他 mutation 不依赖真实时间窗口） | `live` 保留，`unresolved` 写明原因；执行域隔离，新 mutation 被拒；决定稍后送达后，Retry 按 SourceDecision 表结清，并完成 Confirm 的义务：较早的 Blocked 目标被清除，产物已发布                                                          | `tests/recovery.rs`                                            | 评审 3 #1、CCG R36    |
| L14             | 正常返回：决定为 `Aborted { NeedsRecovery }`                                                                                                           | `live` 保留，Retry 保持隔离；`PendingAction` 为空不会让它结清                                                                                                                                                                           | 同上                                                           | 评审 3 #1             |
| L15             | Submission 未决期间，同宿主 degraded 重新 adopt，推进了 router generation                                                                              | 证据仍为 Pending，隔离保持；记录下的操作终结后才继续                                                                                                                                                                                    | 同上                                                           | 评审 3 #7             |
| L16             | 恢复已提交的 Applied 时产物发布失败                                                                                                                    | 恢复结清且解除隔离；maintenance 记下 `runtime_product_publish_failed`；之后的显式重试重新发布，不重新提交                                                                                                                               | 同上                                                           | CCG R36               |
| L17             | 恢复一个离开 service 模式的已提交 Applied 时，daemon 拒绝停止                                                                                          | 恢复结清且解除隔离；maintenance 记下 `service_stop_failed`；daemon 恢复可停后，之后的显式重试释放它                                                                                                                                     | 同上                                                           | CCG R43               |
| P1–P6           | 门控；Held 下可手动刷新；Stop 取消下载；Stopped 拒绝新工作；attempt token；`ProducerTasks::stop`                                                       | 符合 §2                                                                                                                                                                                                                                 | `client/profiles.rs`、`app_lifecycle.rs` 测试                  | 发现 6                |
| R1–R8           | 回执与状态投影；前端渲染                                                                                                                               | 符合 §3                                                                                                                                                                                                                                 | 同上 + `frontend/interface/tests/configuration-status.test.ts` | V31 / V33             |
| R9              | 刷新在途时定义被替换，随后旧下载失败                                                                                                                   | 记为 Superseded（healthy），调用方收到“changed”，与成功下载的过期结果相同                                                                                                                                                               | `client/profiles.rs` 测试                                      | CCG 评审 1（L3b）     |
| X1              | 关闭顺序                                                                                                                                               | 事件日志中各步的发出顺序                                                                                                                                                                                                                | `app_lifecycle.rs` 测试                                        | brief 5               |
| X2–X4           | 分别在 Try / AwaitDecision / Cancel 期间 Close                                                                                                         | 结果符合 §5 和本节所列                                                                                                                                                                                                                  | 同上                                                           | V36                   |
| X2b             | Try 只有在 effects 收到取消信号后才能结束；SettleTransactions 预算较小                                                                                 | SettleTransactions 为 `Incomplete`（操作仍在运行），即整个结算等待期间 effects 都未被封闭；之后 SealEffects 照常完成。把 SealEffects 移到 SettleTransactions 之前（即使报告顺序不变）时该测试失败                                       | `app_lifecycle.rs` 测试                                        | CCG 评审 1（L3b，M9） |
| X5              | 外围 IO 期间 Close                                                                                                                                     | PAC 只有在取消后才结束：封闭 → 取消信号 → PAC 退出 → 恢复，且不会重新安装；OS 写入超出上界时，SystemProxy 报 Incomplete                                                                                                                 | 同上 + `system_proxy/tests.rs`                                 | V36                   |
| X5b             | widget stop 挂起                                                                                                                                       | 代理恢复和热键注销照常完成；Widget 报 Incomplete                                                                                                                                                                                        | `effects/tests.rs`                                             | 发现 3                |
| X5c             | widget 已 spawn、握手被阻塞时关闭                                                                                                                      | 组被 abort 后实例仍为 `Starting`；`stop()` kill 并回收子进程（fake 记录到 kill），调用 `release_server`；**观测到握手 worker 结束**后，报告中 Widget 为 Done；fake 拒绝释放时，报 `Incomplete("widget handshake worker still blocked")` | `effects/tests.rs` + `widget` 测试                             | **N6**、评审 3 #4     |
| X5d             | widget `stop()` 被上界截断                                                                                                                             | 实例仍在槽中；报 `Incomplete("widget process still owned")`                                                                                                                                                                             | 同上                                                           | **N6**                |
| X5e             | 失败启动的清理未能释放握手，实例保留为 `Starting`，随后禁用 widget                                                                                     | 每次禁用都重试清理；确认之前 widget 效果为 degraded（`widget_apply_failed`），实例仍在槽中；worker 结束后的禁用为 Healthy 且槽为空；再次禁用不产生任何宿主操作                                                                          | `ui_effects/tests.rs`                                          | CCG 评审 1（L3b）     |
| X6–X8           | 排队中的 mutation；重复或并发调用 shutdown；调用方被丢弃                                                                                               | 分别为：Rejected / 返回同一个 `Arc`，每步只执行一次 / 照常完成                                                                                                                                                                          | `app_lifecycle.rs` 测试                                        | —                     |
| X9              | 预算小于尾部保留                                                                                                                                       | 每个 **owner / actor 子项**都在 fake 中记录到请求（`streams.stop`、各 actor 的 stop、三个 effect owner、core shutdown、session save）；报告中每个子项都是 Done / Incomplete / NotAttempted 之一，并附原因                               | 同上                                                           | **N8**                |
| X10–X12         | EffectsActor 幂等且顺序正确；SessionState 最后保存；StartupReconcile 期间 Close                                                                        | —                                                                                                                                                                                                                                       | —                                                              | —                     |
| X13             | EffectsActor 在预算内未回复 Shutdown                                                                                                                   | SealEffects 为 `Incomplete("shutdown issued, completion unconfirmed")`，三个子项为 `Incomplete("unconfirmed")`，都不记为 NotAttempted；actor 已死、发送立即失败时，才记为 NotAttempted                                                  | `app_lifecycle.rs` 测试                                        | 评审 3 #5             |
| X14             | 日志 actor 在剩余的整体预算内停不下来（fake 日志文件阻塞一个会话的刷新）                                                                               | `ShutdownReport.logs` 为 `Incomplete("issued, not acknowledged")`；`elapsed` 包含这段等待，不小于整体预算；返回早于日志自身的 5 s 等待                                                                                                  | `app_lifecycle.rs` 测试                                        | CCG 评审 1（L3b）     |

---

## 7. TCC 计划与现有代码的分歧及裁定

| #       | 计划                                                  | 代码                                                                                 | 裁定                                                                   |
| ------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| D1      | §11.1 最后才开放写入                                  | 协调器提前连接                                                                       | 维持现状；依靠 FIFO，且此时没有写入者；由 S14 验证                     |
| D2      | §11.2 无法证明归属时不启动第二个实例                  | 不检查残留实例                                                                       | §1.3–§1.4 + `Ownership`                                                |
| D3      | §3.3 / §4.2 写端只属于源事务                          | 目标必须带 handle                                                                    | 已提交目标不再带 handle（`TargetOrigin`）                              |
| D4      | §4.3 `RetryLatest` / `Recover` / `Close`              | 只有 `RetryRuntime` 和 `Close`                                                       | `RetryRuntime` 同时承担重试与恢复；新增 `BeginClosing`                 |
| D5 / D6 | §11.3 SessionState 最后保存，关闭顺序固定             | 先保存；closing 时自动停止 core                                                      | 按 §5.4                                                                |
| D7      | §8.2 关闭时清理 widget                                | Drop 阻塞；子进程延后登记                                                            | 按 §5.6                                                                |
| D8      | V11 显式 Stopped                                      | 推断为 `StoppedByUser`                                                               | 保留该推断；SavedInactive 按 identity 规则处理；显式启动时补建所有权   |
| D9      | §9.2 等待不耗预算                                     | T8 固定等 5 s                                                                        | Reestablish 用独立的 `waits` 计数器退避                                |
| D10     | §11.3 core 生命周期策略                               | 策略是隐式的                                                                         | 显式命名，行为不变                                                     |
| D11     | §2.3 不在后台弹 UAC                                   | Local 回退后 Try 可能调用 `ensure_ready`                                             | Reestablish 只 adopt；自动重试不移动宿主                               |
| D12     | §11.4 RecoveryRequired 必须带出处，只查询、不盲目重发 | panic 只置一个 bool；把 `last_submission` 当证据；service 命令超时后 helper 仍在运行 | `LiveAttempt` + 预写的 `PendingAction` + ServiceActor 保留 helper 句柄 |
| D13     | 图 14 中的“自动更新”                                  | —                                                                                    | 即 §2 / §3 的订阅调度                                                  |

---

## 8. 风险与结构性防护

| 风险                                                  | 防护                                                                                                |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| panic 或回执丢失后不知道动作的出处                    | 两个槽都在 workflow box 内，经 `Completed` 交还给 actor；预写规则保证“槽为空”当且仅当“没有动作在途” |
| 留下过期动作，或第二个动作丢失                        | 槽非空时拒绝写入；恢复先消费旧动作，再产生新动作                                                    |
| 提交已被接受但结果未观测                              | 在 await 之前写入 ticket（沿用基线 `facade.rs` ~523–539 的时机）                                    |
| service helper 超时后仍在运行                         | ServiceActor 保留句柄并拒绝新命令；只有 `command_settled` 能证明完成                                |
| 把已提交目标误判为源协议矛盾                          | 用 `AttemptOrigin::CommittedTarget` 区分；预算在预写 Submission 时扣除                              |
| stop 意图与所有权混淆，用户找不到出口                 | 两者是独立状态；显式启动时补建所有权；逐条列出每条提交路径的授权条件                                |
| 关闭时等待尚未取消的工作，或一个 owner 拖住其他 owner | 先封闭、发取消信号、收回等待方；各子项并发且各自有上界                                              |
| 资源随取消而丢失                                      | widget 在第一个 await 之前登记；`stop()` 确认退出前不 take；`kill_on_drop` 兜底                     |
| 预算为零时部分子项根本没发出                          | 先发出、再等待；发不出的子项显式标为 `NotAttempted`                                                 |
| 生产者竞态                                            | Held / Running / Stopped 三态表；attempt token                                                      |
| 预算被反复补充，或退避失效                            | 只有 identity 改变才补充预算；`waits` 只在离开依赖等待时归零                                        |
| 后台弹出 UAC                                          | Reestablish 只 adopt；自动重试不移动宿主                                                            |
| 测试改动面                                            | `prime()` 改为调用 `startup_reconcile()`；workflow 测试图注入 `Established`；新增参数都有 `Default` |

---

## 9. T10 删除与留给 T11 的内容

**T10 删除**：

- §1.12（Task 6a）和 §5.9（Task 7）所列内容；
- Task 6b：`post_start` 中布置生产者的代码、`unwrap_or(PendingRefresh { reply: None })`、`origin: _origin`、`ExternalFileChanged` 中只记 warn 的返回；
- revision 1 引入、本稿删除的设计机制（均未实现）：`Isolation` 枚举、`InFlight` 日志、panic 时复制上下文、“首条优先”、`RecoveryContext`、`recover_lifecycle` 中“无动作的 panic”所用的稳定证据规则、`UncertainAction` 只在 Unknown 时记录的语义。

**留给 T11**：

- dirty 接线；
- 死路由：`set_execution_host`、`ApplyControlChannel`、`regenerate_runtime`、`recover_core`、`change_execution_host`、`rebuild_running_config`、`promote_existing_runtime_product`、`start_promoted_runtime`、`stop_core`；
- `reconcile_core` 的去留（T10 只改变它的授权条件，见 §1.7 #5）；
- 逐命令通知；
- `effects/plan.rs` 中 `runtime_apply_kind` 一族；
- `update_systray` 监听器的重复（与 L4 一起处理）。

---

## 10. 非目标

- 不承诺跨崩溃 exactly-once，不新增持久事务日志。
- 不在不重启的前提下复用被 adopt 的实例。
- 启动和自动重试中不 install / start daemon，也不回退到 Local。
- 启动时不重新扫描外部源；不扫描进程（daemon 被杀死后遗留的 core 由该 daemon 的 manager 清扫）。
- 不新增 IPC；UI 只在现有面板中增加 source 行。
- 不改 `nyanpasu-core`、`nyanpasu-runtime`，不改 CoreActor 的语义，不改 daemon 的退出策略。
- **ServiceActor 只做一处收窄的语义改动**：变更类 adapter 调用（包括 `pre_start` 的启动自动更新）超出上界后，保留其句柄；句柄未结束期间，拒绝新的变更命令和交出 endpoint 的消息，并经 `CommandSettled` 暴露完成情况（§1.11）。除此之外不改。
- widget 握手线程的解除依赖在 `nyanpasu-egui` 中新增 `release_server`（§5.6）；若解除失败，报告为 Incomplete，而不是宣称已释放。
- 不处理托盘 proxies 接收任务、`Handle`、`WindowManager`、可变 static（归 L4）。
- 不引入 actor 监督，不引入通用工作流引擎。

---

## 11. 任务切分与验收

提交顺序为 6a → 6b → 7，每个提交都必须能构建。6a 与 6b 之间，ProfilesActor 暂时保留现有的 `post_start` 布置。

- **Task 6a**（§1 + §4；测试 S1–S21、L1–L15）
  - 新增 `startup.rs`、`attempt.rs`（`LiveAttempt` / `AttemptOrigin` / `recover`）。
  - facade 实现 `PendingAction` 预写和 `action_evidence`。
  - ServiceActor 保留 helper 句柄，并将其用于 `pre_start` 的自动更新；`outstanding` 期间拒绝交出 endpoint 的消息；提供 `CommandSettled`。S1 先检查 `command_settled()`。
  - 新增 `Ownership`、`start_permitted`，显式启动时补建所有权。
  - `DeferredTarget` 增加 `identity` / `waits`；新增 `TargetOrigin`、`next_wait`；SavedInactive 按 identity 规则处理。
  - 新增 `publish_full`；替换 `resolve_setup` 的启动块；完成 §1.12 的删除。
  - 验收：Global Constraint 4；`rg -n "reconcile_application_effects|restore_execution_host|probe_service\(|outcome_uncertain|last_submission|original_operation_terminal|RecoveryContext" backend/tauri/src` 无结果。
- **Task 6b**（§2 + §3；测试 P1–P6、R1–R8）
  - 生产者门控、attempt token、下载任务句柄。
  - `ProducerTasks`、`ClientSetupArgs.producers`，以及 `setup.rs` 中的包装。
  - `start_background_sources`。
  - `sources.rs`、`ConfigurationStatus.sources`、前端、i18n、bindings。
  - 验收：Global Constraint 4 加前端检查；`rg -n "origin: _origin|unwrap_or\(PendingRefresh" backend/tauri/src` 无结果。
- **Task 7**（§5；测试 X1–X13）
  - `app_lifecycle.rs` 编排：先发出后等待、截止时间、报告。
  - `BeginClosing` 和 `drive` 的改动。
  - EffectsActor 的 `hold_retries`、幂等 Shutdown、按 §5.5 的顺序执行。
  - `begin_shutdown`、`shutdown(budget)`、executor 并发子项、`signal_shutdown`。
  - widget：spawn 后立即登记；`stop()` 不先 take；`kill_on_drop`；保留握手 worker 句柄，并用 `nyanpasu-egui` 的 `release_server` 解除阻塞；删除 `Drop`。
  - `help.rs` 胶水代码；完成 §5.9 的删除。
  - 验收：Global Constraint 4；`rg -n "shutdown_core|shutdown_application_effects|impl Drop for WidgetManager" backend/tauri/src` 无结果。

---

## 12. 修订记录

### codex 评审 1

| #   | 发现                                     | 决定                                                                                                                  | 修改的章节          |
| --- | ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------- |
| 1   | mutation 的 panic 会绕过权威源决定       | 属实。revision 1 引入 `Isolation` + `InFlight`；**评审 2 后由 `LiveAttempt` + `PendingAction` 取代**                  | §1.11、§4、§6 L1–L4 |
| 2   | StopCore 移除了所有权屏障                | 属实。引入 `Ownership`；StopCore 不再清除目标                                                                         | §1.4、§1.7、§6 S15  |
| 3   | effects 关闭时在等待尚未取消的工作       | 属实（针对设计原文：现有代码会先 abort，但 executor 的 shutdown 是串行的）。改为封闭 → 发信号 → 收回等待方 → 独立清理 | §5.5、§6 X5         |
| 4   | `last_submission` 不是“未观测操作”的记录 | 属实。改为类型化的待决动作；**评审 2 再改为预写**                                                                     | §1.11、§4           |
| 5   | 恢复时 stop 意图被回执捷径绕过           | 属实。S3 先判断 stop 意图                                                                                             | §1.4、§6 L11        |
| 6   | Stopped 状态仍允许新下载                 | 属实。Stopped 一律拒绝新工作；引入 attempt token                                                                      | §2.2                |
| 7   | SavedInactive 会补充预算                 | 属实。只有 identity 改变才补充                                                                                        | §1.7、§6 S10b       |
| 8   | 依赖等待没有递增的计数器                 | 属实。新增 `waits`；**评审 2 修正了归零时机**                                                                         | §1.8                |
| 9   | 关闭的预算保证需要明确                   | 采纳。拆成子项，截止时间用饱和运算；**评审 2 改为先发出、再等待**                                                     | §5.3–§5.5           |

### codex 评审 2（leader 的结构性裁定：只保留一个 `LiveAttempt` 槽和一个预写的 `PendingAction` 槽）

| #                       | 发现                                                  | 核对（`4f59ca781`）                                                                                                                                                        | 决定                                                                                                                                                         | 修改的章节               |
| ----------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------ |
| R1-1 / R1-4（部分解决） | 动作记录存在 panic 窗口；已提交目标的恢复不完整       | 见 N1、N4                                                                                                                                                                  | 删除 `Isolation` / `InFlight` / `RecoveryContext` / 首条优先，改为双槽模型；隔离 = 执行域空闲且槽非空                                                        | §0.1、§1.11、§4、§9      |
| R1-8（部分解决）        | 一进入 S4 就把 `waits` 归零                           | 见 N7                                                                                                                                                                      | 见 N7                                                                                                                                                        | §1.8                     |
| R1-9（部分解决）        | 复合步骤 poll 一次并不等于所有请求都已发出            | 见 N8                                                                                                                                                                      | 见 N8                                                                                                                                                        | §5.3–§5.4                |
| N1                      | 只在结果 Unknown 时才记录，留下 panic 窗口            | 属实：基线代码在 submit 之前记录身份（`facade.rs` ~523），在等待之前记录 ticket（~539）                                                                                    | 预写：每个变更 await 之前写入；拿到 ticket 后更新；只在观测到完成或确定未提交时清除                                                                          | §1.11、§6 L5             |
| N2                      | 恢复中的第二个未知动作会丢失                          | 属实：T8 在 ~464 清除旧动作、在 ~502 写入新动作；revision 1 采用首条优先                                                                                                   | 槽非空时拒绝写入；恢复先消费已解决的动作，再为每个副作用预写新动作；panic 后槽中是最新动作                                                                   | §1.11、§4.2、§6 L6–L7    |
| N3                      | service phase 稳定不能证明命令已完成                  | 属实：`bounded` 超时（`service_actor.rs` ~209）后，`spawn_blocking` 启动的 helper 仍在运行（`control.rs` ~54）                                                             | ServiceActor 保留超时 helper 的句柄；句柄存在期间拒绝新的变更命令；完成证据改为 `CommandSettled`；phase 和 `Exhausted` 都不算完成证据；§10 相应收窄非目标    | §1.11、§10、§6 L8        |
| N4                      | 把已提交目标的重试误当成未确认的源事务                | 属实：`retry_runtime`（~281、~366）复用的是已经提交的决定                                                                                                                  | 新增 `AttemptOrigin::CommittedTarget(target)`，携带 identity、预算和 `waits`；恢复走 T8 已提交目标路径或 reestablish；预算在预写 Submission 时扣除           | §1.8、§1.11、§4.2、§6 L9 |
| N5                      | 所有权不匹配时没有可用的恢复入口                      | 属实：SavedInactive 会清除 deferred（~1285）；没有目标时 Retry 是空操作（~307）。另外 TCC 直接调用 `submit_runtime`（`core_lifecycle/workflow.rs` ~506），并不存在单一漏斗 | 显式启动遇到所有权缺失或不匹配时运行 `reestablish(ExplicitStart)`；停止状态下保存配置保持 stop 意图；§1.7 列出所有提交路径及其授权条件，不再宣称存在单一漏斗 | §1.4、§1.7、§6 S18–S19   |
| N6                      | widget 组被取消后可能留下孤儿进程                     | 属实：`widget.rs` 在 ~109 spawn，到 ~138 才登记；executor 直接 await 它（~112）                                                                                            | spawn 后、第一个 await 之前就登记为 `Starting`；`stop()` 确认退出前不 take；`kill_on_drop` 兜底；只 abort 那些已把资源交给 owner 的 future                   | §5.5–§5.6、§6 X5c–X5d    |
| N7                      | 一进入 S4 就归零，导致 check `Unserviceable` 无法退避 | 属实：validator 不可用是在 Try 内部发现的（~805），早于提交                                                                                                                | `waits` 只在离开依赖等待时（得到应用类结果，或 identity 改变）归零；`next_wait` 接收结果类别作为参数                                                         | §1.8、§6 S17             |
| N8                      | poll 一次不能保证所有清理请求都已发出                 | 属实：typed 调用可能卡在等待回复上（`effects/actor.rs` ~490）；StopActors 原本是依次 await                                                                                 | 每步分为“发出”和“等待”两段；StopActors 先对所有 actor 调用 `stop(None)`，再 `join_all`；发不出的子项记为 `NotAttempted`                                      | §5.3–§5.4、§5.8、§6 X9   |

### codex 评审 3（结论：OK with changes；双槽模型确认成立，不再重新设计）

| #   | 发现                                               | 核对（`4f59ca781`）                                                                                                          | 决定                                                                                                                                                          | 修改的章节                     |
| --- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| 1   | `PendingAction` 为空不代表源事务已结算             | 属实：`await_decision` 在超时和 `NeedsRecovery` 时返回 Unresolved（`tcc.rs` ~1155）；T8 在这两种情况下都保留恢复状态（~234） | 改为正面清除规则：权威结论、资源结算、runtime 验证都完成，且没有待决动作，才清除 `live`；决定超时、`NeedsRecovery`、基线验证失败时保留 `live`（`unresolved`） | §1.11、§6 L13–L14              |
| 2   | 保留下来的 ExplicitStart 目标会越过较晚的 StopCore | 属实：StopCore 先记录意图（`core_lifecycle/workflow.rs` ~191）；原 §1.4 允许 ExplicitStart 跳过 stop 检查，与 §1.5 冲突      | S3 对所有原因统一执行 stop 意图；ExplicitStart 只在受理时作废此前的 stop 意图；较晚的 StopCore 把保留目标改写为 `Reestablish(Recovery)`，只保留所有权检查义务 | §1.4、§1.7、§6 S20             |
| 3   | 启动可能绕过 ServiceActor 仍在运行的 helper        | 属实：`pre_start` 自动更新（`service_actor.rs` ~381）在 facade 之外；`AdoptIfReady` 仅凭 Ready probe 就放行（~406）          | helper 跟踪包装同样用于 `pre_start` 更新；`outstanding` 期间拒绝交出 endpoint 的消息；S1 先确认 `command_settled()`，否则按依赖等待                           | §1.3、§1.6、§1.11、§10、§6 S21 |
| 4   | 杀掉未连接的 widget 不会释放阻塞的 accept 线程     | 属实：`IpcOneShotServer::accept()` 没有超时（`nyanpasu-egui/src/ipc.rs` ~31）                                                | 保留握手 worker 句柄；以 `release_server` 自连接解除阻塞，并有界等待；解除失败时如实报告 Incomplete                                                           | §5.6、§10、§6 X5c              |
| 5   | 没有 EffectsActor 的回复，不能断定 NotAttempted    | 属实：RPC 超时无法说明 handler 是否已运行（`effects/actor.rs` ~490）                                                         | 未回复时父步骤为 `Incomplete("shutdown issued, completion unconfirmed")`，子项为 unconfirmed；NotAttempted 仅用于已知的前置条件失败                           | §5.3、§5.4、§6 X13             |
| 6   | 源决定恢复会给未变的目标补满预算                   | 属实：T8 对 identity 相同的目标保留剩余预算（`tcc.rs` ~1256）                                                                | 已有 identity 相同的目标时沿用其计数；只有新目标才给满额                                                                                                      | §4.2、§6 L2                    |
| 7   | 以 router generation 推断 Submission 完成并不可靠  | 属实：同宿主 degraded 重新 adopt 会推进 generation，却不停止原实例（`actor_v2/mod.rs` ~1047）                                | 删除该捷径，只认该操作本身的完成证据；另外明确 ServiceCommand→Handoff 是“先消费、后写入”                                                                      | §1.11、§6 L15                  |

三轮评审都没有驳回任何发现。codex 认可的部分全部保留：`TargetOrigin`、启动路径不写源配置、生产者状态由 actor 独占、spawn 单飞关闭、refresh token、语义目标身份、独立的等待计数器。

### CCG 评审 1（L3a：恢复模型与启动；leader 裁定 R35、R36、R42–R44）

| #   | 发现                                                               | 核对                                                                                                                                                                                                                                | 决定                                                                                                                                                                                                                                                                                                                            | 修改的章节               |
| --- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| R35 | 回复丢失的 handoff 若以 Degraded 结束，会让执行域永久隔离（Major） | 属实：`action_evidence(Handoff)` 只在 `refresh_status()` 成功时认 `Connected / Degraded`，而 `RefreshStatus` 对每个 degraded router 都返回 `BackendUnavailable`；源在 stop 腿期间断开、stop 又失败的 handoff 正是以 `Degraded` 结束 | 完成证据与健康证据分离：新增 mailbox 有序查询 `HandoffSettled` / `handoff_settled(generation_before)`，以 `PendingAction::Handoff` 记录的 generation 为键；Completed、失败后落入 Degraded、被拒绝、ShutDown 都算 Settled，仍在 HandingOff 不算，actor 不可用为错误（不算 Settled）；`action_evidence` 不再读 `refresh_status()` | §1.11、§6 L10            |
| R36 | 恢复已提交的 Applied mutation 时跳过 Confirm 其余的义务（Major）   | 属实：Committed + Applied 分支只验证/恢复回执，既不清除被取代的 deferred 目标，也不发布产物；决定等待超时、源随后才提交时无需 panic 即可触发                                                                                        | Confirm 对 Applied 的收尾抽成一个幂等例程 `finish_applied`，Confirm 与恢复共用；`TryVerdict::Applied` 在 ACK 前预写 `AppliedVerdict { receipt, product, releases_service }`；恢复在验证/恢复回执之后运行它；发布失败记为可重试的 maintenance，不丢弃；中断连接有时限、不可重试，不重放                                          | §1.11、§4.2、§6 L13、L16 |
| 3   | 预算耗尽时，依赖等待仍安排一次会被闸门拒绝的自动尝试（Minor）      | 属实：`wait_for_dependency()` 总是排期，`reestablish()` 随后在 `attempts_remaining == 0` 时拒绝它                                                                                                                                   | 耗尽预算的依赖类结果立即为 Blocked，不排期，与闸门一致                                                                                                                                                                                                                                                                          | §1.8、§6 S22             |
| R42 | 延迟决定测试依赖较早的 mutation 在真实的 100 ms 决定窗口内完成     | 属实：整个夹具共用一个 `decision_wait`，较早的 mutation 在负载下可能超时                                                                                                                                                            | 所有 mutation 使用常规预算；被测 mutation 的写入停住后暂停时钟并推进过预算                                                                                                                                                                                                                                                      | §6 L13                   |
| R43 | 恢复中释放 daemon 失败只写日志，与 Confirm 不对等                  | 属实：恢复没有 receipt 可承载 `service_stop_failed`                                                                                                                                                                                 | 释放失败与发布失败走同一通道：`pending_release` 记入 `ConfigurationStatus.maintenance`；`finish_applied` 在 Confirm 与恢复中都这样记录；之后的显式重试在期望宿主仍为 Local 时按状态检查再次释放                                                                                                                                 | §4.2、§6 L17             |
| R44 | 耗尽预算且依赖不可用时，显式启动报不可重试的 `ApplyFailed`         | 属实：显式启动按 health 映射错误，Blocked 一律为 `ApplyFailed`                                                                                                                                                                      | `Reestablished::Degraded` 携带 `dependency`；依赖原因即使 Blocked 也报可重试的 `BackendUnavailable`，目标仍为 Blocked 且不排期。`retryable` 不驱动任何自动重试（`recover_service_endpoint` 只读自己的尝试结果；IPC 错误以字符串传给前端）                                                                                       | §1.8、§6 S22             |

### CCG 评审 1（L3b：后台源回执、widget 所有权与有序关闭）

| #   | 发现                                                    | 核对                                                                                                                                             | 决定                                                                                                                                                                       | 修改的章节         |
| --- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| 1   | 禁用 widget 可能在保留未结束资源的情况下报成功（Major） | 属实：`apply(Disabled)` 只在 `is_running()` 为真时才 stop，而 `Starting` 返回 false；失败启动的清理未能确认退出或握手结束时，恰好保留 `Starting` | 禁用与关闭一样无条件调用有界的 `runtime.stop()`；确认之前保持 degraded，槽为空时 stop 是空操作                                                                             | §5.6、§6 X5e       |
| 2   | 失败的下载绕过过期定义围栏（Minor）                     | 属实：`RefreshOutcome::Failed` 在 profile 与定义身份检查之前就返回，定义被替换后旧下载的失败记为 Failed/Blocked                                  | 先查 profile 是否仍在、定义与 URL 是否未变，再对两种下载结果分类；围栏不通过时失败也记为 Superseded                                                                        | §3、§6 R9          |
| 3   | 日志停止逃出整体截止时间和报告（Minor）                 | 属实：报告与 `elapsed` 在 `app_logs.shutdown()` 之前就已定稿，而它在 runtime 子模块内另有 5 s 等待                                               | 仍先记录前面各步的报告，再发出日志停止；确认只等到剩余的整体截止时间，结果记入 `ShutdownReport.logs`，`elapsed` 在其后定稿；子模块不改                                     | §5.4、§5.8、§6 X14 |
| 4   | 编排顺序缺少确定性测试（X2 / M9，Minor）                | 属实：X2 观察到生产者取消后就释放事务，没有证明编排已进入结算等待                                                                                | 新增 X2b：Try 以 effects 的取消信号为唯一出口，结算等待耗尽预算时 Try 仍在运行，证明整个等待期间 effects 未被封闭；把 SealEffects 移到 SettleTransactions 之前时该测试失败 | §6 X2b             |
