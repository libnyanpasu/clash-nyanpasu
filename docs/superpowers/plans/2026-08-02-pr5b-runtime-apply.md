# PR-5b 实施计划 — 单一 runtime apply 管线

**日期：** 2026-08-02
**版本：** v2.1（leader 审查 L1–L7 修订；**D1–D5 全裁 A**，rider R1/R2 已盖章；无待决项）
**分支基线：** `refactor/core-manager-actor` @ `9727ef1d4`（PR-5a 阶段门已关闭：`ae4e0f288` + `6a3878bba` + `5277482a5` + `9727ef1d4`）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/task.md` 卡 B1–B4；`design.md` §6–§8
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.2；必答项 §6.4 **RQ-01 / RQ-03**
**平台：** Windows 11 / PowerShell

> **本计划对着已实施的 5a 代码写，不是对着 5a 计划的纸面附录写。** 全部事实来自 `9727ef1d4` 的工作树。

**v2 修订索引（leader 审查）：**

| 项  | 内容                                                 | 落点                        |
| --- | ---------------------------------------------------- | --------------------------- |
| L1  | `CandidateFile` 一并迁入 `core/actor/runtime.rs`     | S1、A.1、A.2、§4 D1         |
| L2  | `RuntimeRevisionAllocator` **留在 client 侧**        | S1、S2、A.1、A.3、§4 D1 后  |
| L3  | `restart()` 残余审计 → T-B4-03 改钉真实语义、F8 重述 | F26/F27、S6、§4 D5、T-B4-03 |
| L4  | 后台 rebuild 的 degradation 去向（I-B 缺口）         | F28/F29、§2.4、S2、T-PC-09  |
| L5  | 删除 `LifecycleSnapshot` 消息                        | F30、A.2、S2                |
| L6  | S4 重试判据预先定死；specta 已启用（新事实 F25）     | F25、S4、§8 风险表          |
| L7  | T-PC-06 用 `TestBackend` 脚本化传输错误              | §6.2 T-PC-06                |

**v2.1 盖章（leader 裁定 D5=A + 两条 rider）：**

| 项  | 内容                                                                        | 落点                           |
| --- | --------------------------------------------------------------------------- | ------------------------------ |
| D5  | 停止态 `change_core` 走 `RunningIdentity` 分支 + 仓内 `RuntimeApplyOutcome` | §4 D5、S6、A.4                 |
| R1  | §3 表补 `Started` 行（Applied **推进**）；restart 失败走 post-commit 路径   | §3.1、S6 流程、T-B4-03/05、A.6 |
| R2  | 仓内枚举是**语义**选择而非编译必要；F25 保留原样                            | §4 D5 的 R2 注                 |

---

## 0. 本阶段的边界

**做（= task.md B1–B4）：**

1. **B1**：Promoted / Applied 所有权迁入 `CoreActor`；删除 client 侧 `RuntimeLifecycleStore` 与 `publish_promoted` / `publish_applied` / `restore_promoted`；lifecycle 经第二条 watch 暴露；
2. **B2**：`CoreOperationGuard` 取代 `rebuild_gate`；全部事务**先取 guard 再读 snapshot**；保留 rebuild worker 的 capacity-1 coalesce；
3. **B3**：删除 API-first patch 与补偿层；`apply_promoted` 改走 `CoreBackend::apply`，按 `CoreApplyData` 映射结果；
4. **B4**：`change_core` 降级为普通 commit-first mutation，返回通用 `MutationOutcome<RuntimeApplyReport>`。

**不做（越界即返工）：**

- 不动 `CoreBackend` 封闭 enum 的形状，不加 trait / factory（已裁定）；
- 不加 actor 层二次恢复（design §5）；
- 不做 watch snapshot 的**状态投影**扩展、log ring、`set_mode` / `reconcile_mode`、macOS DNS（全部 C1–C3）；
- 不删 5 s 健康轮询线程与 `IPC_STATE` static（C2）；
- 不迁移 `feat::patch_clash_with_rebuild` 里的 sysproxy / systray / locale 副作用编排（PR-6e）；
- 不动 `on_profile_change` 的连接中断服务（PR-6）；
- 不碰 Updater 的 `UpdaterManager::global()`（PR-6d）。

---

## 1. 已核验事实（2026-08-02，全部读自 `9727ef1d4` 工作树）

### 1.1 5a 交付的 actor 面（B1/B3 的落点）

| ID  | 事实                                                                                                                                                                                                                   | 锚点                                 |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| F1  | `CoreActorState` 已有 `observed: BackendObservation`、`running: Option<CoreRequest>`、`status_tx: watch::Sender<CoreStatusView>`、`backend: Option<BackendSlot>`；**没有** promoted/applied 字段                       | `core/actor/mod.rs:47-62`            |
| F2  | `commit()` 与 `commit_backend()` **分流**：前者只更新缓存 + 发布 watch + 跑 latch + 终止态清 `running`；后者额外把 `view.run_type` 归一化为 `state.mode`。**合成观察走 `commit()`，真实后端观察走 `commit_backend()`** | `mod.rs:168-180`（`9727ef1d4` 引入） |
| F3  | `CoreBackend::apply(&request, expected: Option<RevisionIdInfo>) -> Result<CoreApplyData, CoreBackendError>` **已实现**（D4=A：5a 实现但生产未接线）                                                                    | `core/actor/backend.rs:344-348`      |
| F4  | actor 消息集已含 `Check` / `Run` / `Stop` / `Recover` / `SetBackend` / `RefreshStatus` / `RefreshHint` / `RunningIdentity` / `Shutdown`；**没有** apply 类消息                                                         | `mod.rs:70-111`                      |
| F5  | `CoreActorError` 四变体：`StaleOperation` / `NoBackend{last_error}` / `Backend(Arc<..>)` / `ShuttingDown`                                                                                                              | `core/actor/types.rs:59-69`          |
| F6  | `CoreOperationGuard::acquire` 超时常量 `CORE_ACQUIRE_TIMEOUT = 120s`                                                                                                                                                   | `client/core.rs:30`、`:278-293`      |

### 1.2 lease seam 现状（B2/B3 的落点）

| ID  | 事实                                                                                                                                                                                                                               | 锚点                     |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| F7  | `CoreLeaseAdapter` 字段：`guard` / `core` / `application` / `requests` / `runtime_paths` / `target_core: Option<ClashCore>`                                                                                                        | `client/core.rs:331-338` |
| F8  | **`restart()` 用 `self.target_core.take()`**（一次性消费）——`9727ef1d4` 的语义之一：回滚深路径因此会回退到 typed 快照里**已提交的旧核**，这是有意的                                                                                | `client/core.rs:463-471` |
| F9  | 五个 lease 方法的当前实现：`check_and_promote` 经 actor `check` + 客户端文件工作；`apply_candidate` **混合**（actor `check` + 裸 `put_configs`）；`apply_promoted` **纯裸 HTTP**；`restart` 经 actor `run`；`stop` 经 actor `stop` | `client/core.rs:390-477` |
| F10 | **唯一残余的裸 HTTP apply 通道**是 `apply_config_from()`：5 次重试包 `crate::core::clash::api::put_configs`，每次间隔 250 ms                                                                                                       | `client/core.rs:479-489` |

### 1.3 client 侧待删除面（B1/B2/B3 的落点）

| ID  | 事实                                                                                                                                                                                                   | 锚点                                                                |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| F11 | `NyanpasuClientInner` 含 `clash_patch` / `clash_patch_gate` / `rebuild_gate` / `rebuild` / `runtime_revisions` / `runtime`                                                                             | `client/mod.rs:232-261`                                             |
| F12 | `rebuild_gate` **共 10 处获取**；其中 9 处是「gate → `core.begin()`」紧邻，**唯独 `promote_default_runtime_config` 在 gate（`:406`）之后、`begin()`（`:443`）之前先构建了 snapshot 与 candidate**      | `mod.rs:1391,1436,1512,1574,1590`；`rebuild.rs:232,257,268,282,406` |
| F13 | `clash_patch_gate` **只有 1 处获取**（`patch_running_config`）                                                                                                                                         | `mod.rs:1511`                                                       |
| F14 | `publish_applied` **8 个调用点**、`publish_promoted` 3 个、`restore_promoted` **仅 1 个**（change_core 深回滚）；另有一处**绕过 publisher 的裸写** `runtime.write().applied = ...`                     | `mod.rs:1343/1360/1380`；裸写 `mod.rs:1501`                         |
| F15 | `regenerate_runtime_inner` **先分配 revision、再读 typed snapshots**，doc 明写「必须在 `rebuild_gate` 下运行」                                                                                         | `mod.rs:1595-1611`                                                  |
| F16 | `regenerate_runtime_with` 是 typed 与 legacy 两条路径**共用的** candidate→check→promote 核心                                                                                                           | `mod.rs:1613-1687`                                                  |
| F17 | `patch_running_config` 的 API-first 顺序：捕获 lifecycle → `compensation_for` → **先打 `clash_patch.patch()` 到运行核** → 再 `patch_clash_with_rebuild` → 失败走 `restore_applied_after_patch_failure` | `mod.rs:1507-1571`                                                  |
| F18 | `restore_applied_after_patch_failure` 的 `Ok` 分支**不可达**——它总是以 `bail!` 收尾                                                                                                                    | `mod.rs:1445-1505`（`:1502-1504`）                                  |
| F19 | `change_core` 有**三条回滚分支**：A 构建失败（discard，产物未动）／B 回滚重建成功后重启旧核／C 回滚重建也失败 → `restore_product` + `restore_promoted` + 重启旧核                                      | `rebuild.rs:275-400`                                                |
| F20 | **`ControllerBinding` 与 `config_patch_from_mapping` 在代码库中不存在**——仅出现在 spec/roadmap 文本里。B3 卡上这两项对当前代码是 **no-op**                                                             | 全仓 grep 仅命中 docs                                               |
| F21 | **`RuntimeApplyReport` 与 `ChangeCoreReport` 都不存在**；`change_clash_core` 与 `patch_clash_config` 两个命令目前都返回 unit `Result`                                                                  | `ipc.rs:479-496`、`:435-458`                                        |
| F22 | `MutationOutcome::from_parts` 是 **`CommittedDegraded` 的唯一产出点**（degradations 为空即 `Applied`）；`DegradationPhase` 已含 `CoreRollback` 与 `RuntimeApply`                                       | `client/runtime.rs:395-404`、`:456-471`                             |
| F23 | `RebuildCoordinator` 的 capacity-1 coalesce：`mpsc::channel(1)` + `try_send` 丢弃 + 500 ms 接收端去抖 + `try_recv` 排空；worker 经 `Weak<NyanpasuClientInner>` 调 `rebuild_running_config()`           | `rebuild.rs:21,24-44,58-187`；`mod.rs:435-449`                      |
| F24 | service harness 测试须套 `transport_available()` 守卫（`9727ef1d4` 的语义之三）；Unix 下它探测 `/var/run` 可写性                                                                                       | `core/actor/backend.rs:864-880`                                     |

### 1.4 v2 新增事实（L3/L4/L5/L6 的依据）

| ID  | 事实                                                                                                                                                                                                                                                                                  | 锚点                                                                       |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| F25 | **workspace 已启用 `specta`**：`nyanpasu-utils` 与 `nyanpasu-ipc` 的 features 含 `"specta"`，`apply.rs` 的 `ApplyOutcomeKind` / `CoreApplyData` 都带 `#[cfg_attr(feature = "specta", derive(specta::Type))]`。**A.4 可直接内嵌 upstream 类型，无需镜像 DTO**                          | `backend/Cargo.toml:36-41`；`nyanpasu_ipc/src/api/core/apply.rs:34-38,66`  |
| F26 | `lease.restart()` 今日**生产**调用点共 4 类 6 处：`start_promoted_runtime`（启动）、`patch_running_config`（B3 删）、`regenerate_and_restart_for_legacy`（legacy replay）、`change_core` 三处（B4 删）。**B3+B4 之后仍剩两类**——启动路径与 legacy replay，二者都不在 5b 范围内        | `mod.rs:1441,1540`；`rebuild.rs:271,315,333,377`                           |
| F27 | **`CoreBackend::apply` 要求核已在运行**——upstream doc 原文「apply never starts one」。今日 `change_core` 用 `restart()`，**核处于停止态时会把新核启起来**；而 `rebuild_running_config` / `patch_running_config` 的 apply 走裸 HTTP，核停止时本来就失败。故只有 `change_core` 有语义差 | `apply.rs:10-18`；`rebuild.rs:315`                                         |
| F28 | 后台 rebuild worker 对 `rebuild_running_config()` 的 `Err` **只写 `tracing::warn!`**，无人接收；唯一会把它转成 degradation 的是**同步** caller `collect_post_commit_degradations`，经 `map_runtime_rebuild_degradation` 塌成单一 `runtime_rebuild_failed` / `RuntimeBuild`            | `rebuild.rs:172-174`；`mod.rs:1013-1016`、`:979-988`                       |
| F29 | `CoreDegradationSink` 已存在并已注入 actor（5a 的 D5 latch 用它发 `core_recovery_exhausted`）；生产实现 `TauriCoreDegradationSink` 落到 `UiEventSink::notice_message`。`ClientSetupArgs` 有 `degradation` 字段，但 **`NyanpasuClientInner` 没有保存它**                               | `backend.rs:581-583`；`mod.rs:36,193`；`event_sink.rs:85-105`；`mod.rs:86` |
| F30 | `commit()` 在**消息处理函数内**就 `status_tx.send_replace`，早于 reply 发出。因此「await 守卫消息的 reply」happens-before「读到新的 watch 值」——测试不需要额外的快照消息                                                                                                              | `mod.rs:168-175`                                                           |

---

## 2. RQ-01 — post-commit 失败矩阵（必答）

### 2.1 分界线的定义

**分界线是「typed desired state 是否已经提交」**，不是「操作是否已经开始」。

- `ApplicationClient::patch` / `ClashConfigClient::patch` 返回 `Ok` 的那一刻起，用户意图已经持久化 → 此后任何失败都**不得**表现为普通 `Err`（否则 UI 会显示「失败」而磁盘上已经变了）；
- 在此之前的任何失败，desired 未动，返回 `Err` 是诚实的。

因此 5b 的三类入口有不同的分界位置：

| 入口                                                                                                                         | 有无 desired commit     | 分界                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------- | ----------------------- | ------------------------------------------------------------------------- |
| `patch_running_config`（B3）、`change_core`（B4）                                                                            | **有**，且在最前        | commit 之后全部 degraded                                                  |
| `rebuild_running_config`（后台脏重建）                                                                                       | 无（响应更早的 commit） | **全部 degraded**——commit 早已发生，此处只是迟到的副作用；投递路径见 §2.4 |
| `promote_existing_runtime_product` / `start_promoted_runtime` / `promote_default_runtime_config`（启动路径）、`restart_core` | 无                      | 全部 `Err`（没有已提交的用户意图需要保护）                                |

### 2.2 七项逐条作答

`P` 列 = 该失败发生在分界线之前（`pre`）还是之后（`post`）。`post` 一律映射为 `Degradation { phase, code, retryable }` 并经 `MutationOutcome::from_parts` 变成 `CommittedDegraded`。

| #   | 失败                       | 触发点                                                             | P       | commit-first 入口的结果                                                                                                                                                                                                               | 无-commit 入口的结果 |
| --- | -------------------------- | ------------------------------------------------------------------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 1   | **operation acquire 超时** | `begin_operation()` 等待 `CORE_ACQUIRE_TIMEOUT`（F6）              | **pre** | `Err(OperationError::AcquireTimeout)`——**guard 在 desired commit 之前取得**（见 S3 的顺序约束），因此永远是 pre                                                                                                                       | `Err`                |
| 2   | **build 失败**             | `regenerate_runtime_with` 的 `spawn_blocking` 构建段               | post    | `phase = RuntimeBuild`、`code = "runtime_build_failed"`、`retryable = true`                                                                                                                                                           | `Err`                |
| 3   | **check 失败**             | `CoreBackend::check`（dry-run）                                    | post    | `phase = RuntimeCheck`、`code = "runtime_check_failed"`、`retryable = true`                                                                                                                                                           | `Err`                |
| 4   | **promote 失败**           | candidate 哈希不符 / `restore_product` 写失败 / promote 后校验不符 | post    | `phase = RuntimePromote`、`code = "runtime_promote_failed"`、`retryable = true`；**Promoted 不推进，产物保持旧值**                                                                                                                    | `Err`                |
| 5   | **revision 冲突**          | `CoreBackend::apply` 的 CAS（`Error::RevisionConflict`）           | post    | `phase = RuntimeApply`、`code = "revision_conflict"`、`retryable = true`；**Applied 不变**，下一次 rebuild 会带新 revision 重试                                                                                                       | `Err`                |
| 6   | **IPC 连接丢失**           | Service backend 的传输错误（`ClientError`）                        | post    | `phase = RuntimeApply`、`code = "core_transport_lost"`、`retryable = true`；**Applied 不变**                                                                                                                                          | `Err`                |
| 7   | **apply error**            | `CoreApplyData.outcome == RolledBack`，或 backend 返回 `Err`       | post    | `RolledBack` → `phase = CoreRollback`、`code = "core_rollback"`、`retryable = true`；核未运行（F27）→ `code = "core_not_running"`；其它 apply 错误 → `phase = RuntimeApply`、`code = "runtime_apply_failed"`。三者 **Applied 均不变** | `Err`                |

> **第八项（D5=A 引入，不在 RQ-01 原列表内）**：停止态 `change_core` 的 `restart()` 失败。desired 早已提交，故同样是 `post`——`phase = CoreLifecycle`、`code = "core_start_failed"`、`retryable = true`，Applied 不推进（§3.1 的 `Started` 行、T-B4-05）。用 `CoreLifecycle` 而非 `RuntimeApply`：失败发生在核**生命周期**上，不是在配置应用上，与 5a 的 `core_recovery_exhausted` 同相。

### 2.3 三条不变量

- **I-A（不撒谎）**：desired 已提交时**绝不**返回 `Err`——只返回 `CommittedDegraded`；
- **I-B（不静默）**：任何 `post` 失败**必须**产出至少一条 `Degradation`，不允许只写日志；
- **I-C（状态单调）**：`post` 失败不得回退已经推进的 Promoted；Applied 只在 backend 确认采纳新 revision 时才推进（§3）。

> 与 5a 的 `CoreActorError` 的关系：`StaleOperation` / `ShuttingDown` 属于**内部不变量破坏**，不映射 degradation——它们只可能出现在实现有 bug 或进程正在关停时，按 `Err` 上抛并记日志。`NoBackend` 在 commit-first 入口按第 6 行处理（`code = "core_backend_unavailable"`）。

### 2.4 degradation 投递到哪里（I-B 对**后台入口**如何满足）

I-B 说「不允许只写日志」，但 `rebuild_running_config` 有两类 caller，其中一类根本没有 `MutationOutcome` 的接收方（F28）。三条投递路径必须分清：

| 调用方                                                                                                 | 投递路径                                                                                             | 现状                                                      |
| ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| **同步 post-commit**：`collect_post_commit_degradations`（profile mutation 的 `after_commit` 等 5 处） | 返回值——追加进调用方的 `MutationOutcome<..>` degradations                                            | 已存在，但塌成单一 `runtime_rebuild_failed`（F28）        |
| **后台 worker**：`RebuildCoordinator` 的 dirty 重建                                                    | **`CoreDegradationSink::publish`**（F29 已有的注入面，与 5a 的 `core_recovery_exhausted` 同一 sink） | **今天只有 `tracing::warn!`——这是 I-B 的缺口，5b 必须补** |
| **命令入口**：`patch_running_config` / `change_core`                                                   | 返回值——`MutationOutcome<RuntimeApplyReport>`                                                        | S5 / S6 新建                                              |

**5b 的两项对应动作：**

1. **同步路径精度提升**：`map_runtime_rebuild_degradation` 的 doc 现在写着「不要臆造 `RuntimeCheck` / `Promote` / `Apply` 精度，错误面撑不住」（`mod.rs:979-980`）——5b 恰恰把错误面做出来了。把它改为直接透传新管线产出的 `Vec<Degradation>`（§2.2 的 phase/code 分级），删掉那条 doc 与单一 `runtime_rebuild_failed` 常量。
2. **后台路径补 sink**：`NyanpasuClientInner` 增 `degradation: Arc<dyn CoreDegradationSink>` 字段（`ClientSetupArgs` 早有此值，F29，只是没留存），`rebuild_running_config` 的**内部**改为返回 `(Result<()>, Vec<Degradation>)` 形状的等价物；worker 闭包把 degradations 逐条 `publish`，同步 caller 则把它们并入自己的 `MutationOutcome`。`tracing::warn!` 保留（可观测性），但**不再是唯一出口**。

> 为什么用 `CoreDegradationSink` 而不是 lifecycle watch 携带 `last_error`：watch 是**状态**投影（当前值、可被下一次覆盖），degradation 是**事件**（每条都要送达一次）。用状态通道送事件会在两次重建之间静默丢失中间那条。5a 已经为「事件」选定了 sink，5b 沿用，不新造第二套。

---

## 3. RQ-03 — apply parity 矩阵（必答）

### 3.1 六个 apply outcome + 一个非 apply 终态 + 一个正交标志

`CoreApplyData { outcome: ApplyOutcomeKind, revision: ConfigRevisionInfo, warning: Option<String>, failed_apply: Option<String> }`。

**`Warning` 不是第七个分支**——它是与 outcome 正交的标志位，可以与**任何**一个 outcome 组合出现（来源是 runtime 的 `ApplyOutcome::DurabilityUncertain` 包装，可嵌套两层并以 `"; "` 拼接）。

| outcome                              | Applied 是否推进 | 返回                                        | 说明                                                                                                                                                       |
| ------------------------------------ | ---------------- | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Noop`                               | **推进**         | `Applied`                                   | 配置已在生效——运行的就是该 revision，Applied 必须等于 Promoted，否则读模型会永远滞后                                                                       |
| `Patched`                            | **推进**         | `Applied`                                   | 就地 `PATCH /configs`                                                                                                                                      |
| `Reloaded`                           | **推进**         | `Applied`                                   | 就地 `PUT /configs`                                                                                                                                        |
| `Restarted`                          | **推进**         | `Applied`                                   | 同 epoch 内换进程                                                                                                                                          |
| `Switched`                           | **推进**         | `Applied`                                   | 换核（B4 的正常成功路径）                                                                                                                                  |
| `RolledBack`                         | **不推进**       | `CommittedDegraded { phase: CoreRollback }` | **旧配置在跑**；desired 与 Promoted 保留新值，Applied 保留旧值                                                                                             |
| **`Started`**（非 apply 产出，D5=A） | **推进**         | `Applied`                                   | 核原本停止，本路径把它启起来了：restart 成功后**运行的就是 promoted revision**，因此与 `Noop` 同理必须推进，否则读模型永远滞后。`report.outcome = Started` |

前六格逐一映射到 `RuntimeApplyOutcome` 的同名变体（A.4）。第七行 `Started` **不由 `apply` 产出**——它是 D5=A 的停止态启核路径直接构造的，**不进 `map_apply_outcome`**，因此不进 §3.2 的 12 格 parity 矩阵；但它是一个真实终态，Applied 推进规则必须在这张表上说清。

**`Started` 路径的失败侧（R1）**：停止态分支的 `restart()` 失败时，desired 早已提交（§2.1），因此**走 §2.2 的 post-commit 路径**——返回 `CommittedDegraded`，`phase = CoreLifecycle`、`code = "core_start_failed"`、`retryable = true`，Applied **不推进**。成功侧由 **T-B4-03** 钉住，失败侧由 **T-B4-05** 钉住（§6.3）。

**Warning 的处理（与上表正交）：**

- `warning.is_some()` 时**追加**一条 `Degradation { phase: RuntimeApply, code: "core_apply_durability_uncertain", retryable: false }`；
- **不改变**上表的 Applied 推进决策；
- 因此 `Applied + warning` 会变成 `CommittedDegraded`（`from_parts` 见 F22），而 `RolledBack + warning` 会有**两条** degradation。

### 3.2 parity 测试要求

矩阵是 **6 × 2 = 12 个组合**（六个 outcome × warning 有/无），每个组合都要断言三件事：Applied 是否推进、返回的 `MutationOutcome` 变体、degradation 列表内容。测试编号 T-AP-01…12（§6）。

**双后端 parity**：Local 与 Service 对同一 outcome 必须产出同一映射结果。Local 侧由 `TestBackend` 脚本化 `CoreApplyData`；Service 侧沿用 5a 的 IPC harness，**并套 `transport_available()` 守卫**（F24）。

---

## 4. 决策点（D1–D5 leader 已全部裁定，2026-08-02；**无待决项**）

### D1 — `RuntimeSnapshot` 等类型放哪 —— **裁定 A**

B1 要把 Promoted/Applied 搬进 actor，但 `RuntimeSnapshot` / `RuntimeRevision` / `RuntimeLifecycleState` 现在住在 `client/runtime.rs`（F11 邻域）。

- **裁定 A**：把 `RuntimeRevision` / `RuntimeSnapshot` / `RuntimeSnapshotData` / `RuntimeLifecycleState` **以及 `CandidateFile`** 移到 **`core/actor/runtime.rs`**（新文件）。理由：所有权跟着数据走；actor 反向依赖 `client::` 是层次倒置。
- **选项 B（未采纳）**：类型不动，actor 直接 `use crate::client::runtime::*`。改动小，但让 `core::actor` 依赖 `client`，与 5a 建立的方向相反。

> **L1 修正**：v1 曾把 `CandidateFile` 留在 `client/runtime.rs`，同时让 A.2 的 `CheckAndPromote` 消息携带它——那正是 D1=A 要消灭的反向依赖。`CandidateFile` 必须同迁。迁后方向是 **client 构造 → actor 消费**，即 `client → core`，正确。
>
> **L2 修正**：`RuntimeRevisionAllocator` **不迁**，留在 `client/runtime.rs`。它是 **ID 源泉**（一个 `AtomicU64`），不是 lifecycle 状态；迁入 actor 会逼出一条 `AllocateRevision` 守卫消息和一次多余的 round-trip，而单调性本来就由 actor 在 promote 时校验（T-LC-01 已钉）。它 `use` 迁走的 `RuntimeRevision`，方向仍是 client → core。**A.1 / A.2 / A.3 / S1 / S2 / S3 五处已按此对齐。**

### D2 — lifecycle 用第二条 watch 还是塞进 `CoreStatusView` —— **裁定 A**

B1 要求「CoreClient 通过 watch 暴露 lifecycle」。

- **裁定 A：第二条 watch 通道** `watch::Sender<RuntimeLifecycleState>`，与 `status_tx` 并列。理由：`CoreStatusView` 是 UI 投影（5 个小字段，每次 `commit()` 都克隆），而 `RuntimeSnapshot` 持有产物字节与整个 config Mapping——塞进同一条通道会让每次状态变化都克隆一份重量级快照。
- **选项 B（未采纳）**：扩 `CoreStatusView`。省一条通道，但把重负载塞进高频路径，且会改 5a 已稳定的 `commit()` 语义（F2）。

### D3 — `apply_candidate` 的去留 —— **裁定 A**

`apply_candidate`（F9）今天只被 `restore_applied_after_patch_failure` 使用，而后者随 B3 删除。

- **裁定 A：一并删除** `apply_candidate`，`CoreLifecycleLease` 收敛到 4 个方法（`check_and_promote` / `apply_promoted` / `restart` / `stop`——`restart` 因 F26 的两类残余调用者而**保留**，见 D5）。理由：删掉唯一调用者后它就是死代码。
- **选项 B（未采纳）**：保留备用。违反「不留无调用者的抽象」。

### D4 — `change_core` 的 wire 变化幅度 —— **裁定 A**

B4 让 `change_clash_core` 返回 `MutationOutcome<RuntimeApplyReport>`（F21：今天返回 unit）。

- **裁定 A**：新增 `RuntimeApplyReport { outcome, desired_revision: u64, applied_revision: Option<u64> }`（design §8 的形状），命令返回 `MutationOutcome<RuntimeApplyReport>`。bindings 因此新增两个 TS 类型 + 命令返回类型变化——**这是本阶段唯一的 wire 变化**，S9 按「恰好这些」核对。`specta` 已在 workspace 启用（F25），upstream 类型可直接内嵌，**不做镜像 DTO**。
- **选项 B（未采纳）**：仍返回 unit，degraded 只进日志。B4 卡明写「前端复用通用 `RuntimeApplyReport`/MutationOutcome 展示 degraded」。

### D5 — 核处于停止态时 `change_core` 怎么办（L3 审计的产物）—— **裁定 A**

L3 让我审计 `restart()` 残余，审出一条 v1 漏掉的语义差（F26/F27）：

- `CoreBackend::apply` **不启核**（upstream doc「apply never starts one」）；
- 今天的 `change_core` 用 `lease.restart()`，因此**核停止时切核会把新核启起来**；
- 另两条 apply 入口（`rebuild_running_config` / `patch_running_config`）今天走裸 HTTP，核停止时本来就失败——所以**只有 `change_core` 有行为差**。

若 B4 无条件改走 `ApplyPromoted`，「停止态下切核」会从「切完并启动」变成「切完但失败」，这是 B4 卡没有授权的行为回归。

- **裁定 A：按 5a 的 `RunningIdentity` 分支**。`Ok(Some(_))`（在跑）→ `ApplyPromoted` 承载切换（`Switched`）；`Ok(None)`（已停）→ 走 `restart()` 启新核，**与今天逐字同行为**；`Err(NoBackend)` → 按 §2.2 第 6 行 degraded。`RuntimeApplyReport.outcome` 用**本仓自有**枚举 `RuntimeApplyOutcome`（镜像 upstream 六个变体 + `Started`）。
- **选项 B（未采纳）**：无条件 apply，停止态切核返回 degraded。更简单，但改用户可见行为，且 `Started` 这个真实状态在 wire 上无处表达。

**leader 裁定 A（2026-08-02），四条理由：** ①前提经独立核实——`apply.rs` doc 原文「The core must already be running: apply never starts one」；②选项 B 未经 B4 卡授权就改用户可见行为，违反迁移政策；③`Ok(None)` → `restart()` 与今天逐字同行为，且与 5a updater 的 `Ok(None)` 先例不冲突——两者各自保留各自的 legacy 行为（updater 停止态换二进制**不**启动，change_core 停止态切核**启动**）；④`Started` 第七变体是诚实建模，把 `Ok(None)` 分支映射成 `Switched` 是撒谎（apply 根本没承载它）。

> **R2 —— 为什么用仓内枚举而不是直接复用 `ApplyOutcomeKind`：这是语义选择，不是编译必要。** F25（specta 已在 workspace 启用）说明的是「直接内嵌 upstream 类型**能编译**、无需镜像 DTO」；本条决定的是「**不该**直接内嵌」，理由有两条且都与编译无关：`Started` 在 upstream 枚举里没有对应变体（承载不了 D5=A 的真实终态），以及把我们的 TS wire 与 submodule 的 API 版本解耦。**两条事实并立不矛盾**——F25 保留原样不动。

---

## 5. 实施步骤

> 每步给出编辑内容 → 验证 → 通过判据。**不要**跑 `cargo clippy -- -D warnings`（仓库本就红）。已知坑：共享 target 的 kache 污染会造成本地 clippy 假红，用独立 `--target-dir` 复验再判定。

### S1 — 迁移 runtime lifecycle 类型（按 D1=A）

新建 `backend/tauri/src/core/actor/runtime.rs`，从 `client/runtime.rs` 整体搬入（**逻辑一字不改**，只改可见性与 `use` 路径）：

| 搬入 `core/actor/runtime.rs`                                                                                                                                                        | 留在 `client/runtime.rs`                                                                                                                                                                                                      |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RuntimeRevision`（`:27-33`）、`RuntimeSnapshotData`（`:52-57`）、`RuntimeSnapshot`（`:59-96`）、`RuntimeLifecycleState`（`:98-102`）、**`CandidateFile`（`:297-334`，含 `Drop`）** | **`RuntimeRevisionAllocator`（`:35-50`）**、`MutationOutcome` / `Degradation` / `DegradationPhase`、candidate 的构建函数与 `prepare_private_dir` 等文件工作、`compensation_*`（S5 删）、`RuntimeTransactionSnapshot`（S6 删） |

- **`CandidateFile` 同迁（L1）**：`CheckAndPromote` 消息按值携带它（A.2），类型必须在 `core::actor` 一侧可命名，否则 actor 反向依赖 `client::`。它只依赖 `camino` + `tokio::fs`，不拖任何 client 专有依赖。
- **`RuntimeRevisionAllocator` 不迁（L2）**：留在 client 侧，`use crate::core::actor::runtime::RuntimeRevision`。F15 的「先分配 revision、再读 typed snapshot」顺序因此**原样保留**，也不需要新增 `AllocateRevision` 消息。
- `RuntimeLifecycleStore` / `new_runtime_lifecycle_store`（`:117-121`）**不迁、直接删**（S2 由 actor 状态取代）；今天 6 处测试构造点（`mod.rs:2256/2327/2612/2928/3983` 等）随之调整。

**验证：** `cargo check`；`rg 'client::runtime::(RuntimeSnapshot|RuntimeLifecycleState|CandidateFile)' backend/tauri/src` 为 0；`rg 'RuntimeLifecycleStore' backend/tauri/src` 为 0。

### S2 — B1：Promoted / Applied 入 actor

**附录 A.1 声明全部新增字段与消息，此处只述行为。**

- `CoreActorState` 增 `lifecycle: RuntimeLifecycleState`、`lifecycle_tx: watch::Sender<RuntimeLifecycleState>`。**不增 `revisions`**（L2：allocator 留 client 侧）；
- 新增消息 `CheckAndPromote` / `ApplyPromoted`（见 A.2），两条都是**守卫消息**（校验 active `OperationId`，与 5a 的 mutation 同等准入）；
- promote 成功推进 `lifecycle.promoted` 并发布 `lifecycle_tx`；apply 成功按 §3 决定是否推进 `lifecycle.applied`；
- **`publish_promoted` 的「拒绝非递增 revision」与 `publish_applied` 的「必须存在 Promoted 且 `identity_eq`」两条校验原样迁入 actor**（F14 的语义不能丢）。**单调性由 actor 兜底**，因此 allocator 留在 client 侧不削弱任何不变量（T-LC-01 直接打 actor）；
- `CoreClient` 增 `lifecycle()`（同步 watch 克隆）。**不加 `LifecycleSnapshot` 消息、也不加 `subscribe_lifecycle()`**（L5）：`lifecycle_tx` 的发布发生在消息处理函数内、早于 reply（F30 已在 `commit()` 上验证同一时序），所以「await 守卫调用的 reply → 读 `lifecycle()`」已经是确定性的读后写；再加一条诊断消息只会多一个需要 `cfg(test)` 门控的面。

client 侧删除：`runtime` 字段、`publish_promoted` / `publish_applied` / `restore_promoted` / `runtime_lifecycle_state`。**`runtime_revisions` 字段保留**（L2）。`promoted_runtime()` **保留同名同签名**，内部改读 `core_client.lifecycle().promoted`。

**同时接上 §2.4 的 sink（L4）**：`NyanpasuClientInner` 增 `degradation: Arc<dyn CoreDegradationSink>`（值早已在 `ClientSetupArgs` 里，F29），供后台 rebuild worker 投递 degradation 使用。

> **`restore_promoted` 直接删除、不迁入**：它唯一的调用者是 `change_core` 的深回滚（F14），而 B4 删掉整条深回滚路径。

**四条 runtime 读 IPC**（`ipc.rs:346/362/377/390`）改读 `client.promoted_runtime()` 的新实现——facade 方法保留同名同签名，内部改为 `core_client.lifecycle().promoted`，**wire 不变**。

**验证：** `rg 'RuntimeLifecycleStore|publish_promoted|publish_applied|restore_promoted|LifecycleSnapshot' backend/tauri/src` 为 0；四条读 IPC 的 bindings 不变。

### S3 — B2：`CoreOperationGuard` 取代 `rebuild_gate`

删除 `rebuild_gate` 字段与全部 10 处获取（F12）。**顺序约束（关键）**：

```text
begin_operation()  →  读 typed snapshots  →  分配 revision  →  build → check → promote → apply
```

- 9 处「gate → begin」紧邻的站点：删掉 gate 那一行即可，`begin()` 已经在最前；
- **`promote_default_runtime_config` 必须调整**（F12 的例外）：把 `rebuild.rs:443` 的 `begin()` 提到 `:406` 原 gate 的位置，使 snapshot 与 candidate 的构建都在 guard 内；
- `regenerate_runtime_inner` 的 doc（`mod.rs:1595`「必须在 `rebuild_gate` 下运行」）改为「必须在 `CoreOperationGuard` 下运行」，**revision 先分配再读 snapshot 的顺序保持不变**（F15）；
- `restart_core` facade（`mod.rs:491-493`）本就自取 guard，删 gate 后自然一致。

**保留 coalesce（F23）**：`RebuildCoordinator` 一行不改。它的串行化来自 worker 单线程 + capacity-1 通道，与 gate 无关。「构建期间到达的新 commit 触发下一次 rebuild」由 `try_send` + `try_recv` 排空保证。

**验证：** `rg 'rebuild_gate' backend/tauri/src` 为 0；`rebuild.rs` 的 5 个 coordinator 测试（`:469/505/537/573/623`）零改动通过。

### S4 — 统一 apply：`apply_promoted` 改走 backend

新增 actor 消息 `ApplyPromoted { operation, request, expected, reply -> Result<CoreApplyData, CoreActorError> }`，内部调 `CoreBackend::apply`（F3），成功后按 §3 提交并发布两条 watch。

`CoreLeaseAdapter::apply_promoted` 改为调 `core.apply_promoted(&guard, &request, expected)`；**删除 `apply_config_from`**（F10 的裸 HTTP 通道）。`expected` 取 actor 已观察到的 `last_revision`（RQ-02 在 5a 的答案：apply 一律传 `Some(..)`；为 `None` 时视为不变量破坏，返回 `Err`）。

按 D3=A **删除 `apply_candidate`**。

#### S4 的重试判据（L6：预先定死，不留「实测再说」）

旧路径 `apply_config_from` 是 5 次 × 250 ms 重试（F10）；新路径由 runtime 的 `apply_config` 承担 reconcile。**决策规则如下，实施时按规则走，不再开放讨论：**

| 新路径把该类失败暴露为                                             | 动作                                                                               |
| ------------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| **传输类**硬失败（连接被拒 / 连接重置 / socket 未就绪 / 读写超时） | 在 **`CoreLeaseAdapter` 层**对**仅传输类错误**补有界重试，沿用 **5 × 250 ms** 形状 |
| `check` 失败、语义失败（revision 冲突、配置被拒）、`RolledBack`    | **一律不重试**——重试对它们没有意义，只会把一次失败放大成 5 次副作用                |
| runtime 内部已自带重试 / reconcile 且传输类失败不外泄              | **不补**，保持单次调用                                                             |

判定方法：实施时读 `CoreBackendError` 的传输类变体（Service 侧 `ClientError`、Local 侧 manager 的 IO 错误）在新路径下是否会原样冒出到 `apply_promoted` 的返回值。**实测数据（哪类错误、是否外泄、重试是否生效）写进实施报告**，但**加不加重试由上表决定，不由实测口味决定**。

**验证：** `rg 'put_configs' backend/tauri/src` 只剩 `feat.rs:79`（`change_clash_mode`，PR-6 范围）；`rg 'apply_config_from|apply_candidate' backend/tauri/src` 为 0。

### S5 — B3：删除 API-first patch 与补偿层

`patch_running_config`（F17）重写为：

```text
guard → typed desired commit（ClashConfigClient::patch）→ 统一 rebuild → apply → 按 §3 映射
```

删除：`clash_patch` 字段与 `ClientSetupArgs.clash_patch`、`RunningConfigPatchPort`、`LegacyRunningConfigPatchBridge`、`clash_patch_gate`、`restore_applied_after_patch_failure`（F18）、`client/runtime.rs:123-190` 的 `PatchCompensationPlan` / `PatchCompensationOp` / `compensation_for`、以及 `mod.rs:1501` 的裸写。

**保留** `feat::patch_clash_with_rebuild`（`feat.rs:254-336`）：它的 mixed-port / external-controller 检查与 sysproxy / systray 后效属于 PR-6e，本阶段只把它的 rebuild 闭包接到新管线上。

> **B3 卡的两项对当前代码是 no-op**：`ControllerBinding` 与 `ConfigPatch mapper`（`config_patch_from_mapping`）**在代码库中不存在**（F20）。计划照实记录，**不要**为了「删掉它们」而先造出来。

`patch_clash_config` 命令返回类型改为 `MutationOutcome<RuntimeApplyReport>`（与 B4 统一）。

**验证：** `rg 'RunningConfigPatchPort|LegacyRunningConfigPatchBridge|clash_patch_gate|compensation_for|PatchCompensationPlan' backend/tauri/src` 为 0。

### S6 — B4：`change_core` 降级为普通 commit-first mutation

新形态（对比 F19 的三分支回滚）：

```text
guard → ApplicationClient::patch(core = new)   ← desired 提交，此后一律 degraded
      → 统一 rebuild（新核）→ CheckAndPromote
      → RunningIdentity?                        ← D5=A 的分支
          Ok(Some(_))  → ApplyPromoted          ← apply 内部承载切核（Switched）
                         → map_apply_outcome()  ← §3 的 12 格，唯一决策点
          Ok(None)     → restart()              ← 核本来就停着：启新核，与今天同行为
                         Ok  → outcome=Started，Applied 推进        （不经 map_apply_outcome）
                         Err → CommittedDegraded(CoreLifecycle /
                                 core_start_failed)，Applied 不推进  （R1）
          Err(NoBackend) → §2.2 第 6 行 degraded
      → RolledBack → Applied 保持旧值 + CommittedDegraded(CoreRollback)
```

**删除**：legacy `Config::verge().draft()/apply()/discard()`、回滚重建、`restore_product` 调用、回滚分支里的两次 old-core restart、`RuntimeTransactionSnapshot`（连类型一起删——change_core 是它唯一的使用者）。

**`RolledBack` 时的终态**（B4 卡与 Exit 判据）：desired = 新核，Promoted = 新配置，Applied = 旧值。**不做**第二套应用层回滚。

#### `restart()` 残余审计（L3）与 F8 的重新表述

v1 说「事务内 `restart()` 恰好一次」，**那是错的**：新流程的切核由 `CoreBackend::apply` 内部承载（F27 的 upstream doc 明写「`core_type` 与运行核不同即为切换」，产出 `Switched`），`restart()` 在核**在跑**的那条主路径上根本不出现。审计结果（F26）：

| `restart()` 调用点                                          | B3/B4 之后         | 归属                                                 |
| ----------------------------------------------------------- | ------------------ | ---------------------------------------------------- |
| `change_core` 主路径 + 两处回滚（`rebuild.rs:315/333/377`） | **删除**           | B4                                                   |
| `patch_running_config` 的 restart 分支（`mod.rs:1540`）     | **删除**           | B3——restart-vs-apply 改由 `apply` 自己分类           |
| `start_promoted_runtime`（`mod.rs:1441`）                   | **保留**           | 启动路径；apply 不启核（F27）                        |
| `regenerate_and_restart_for_legacy`（`rebuild.rs:271`）     | **保留**           | legacy replay（`feat.rs:244/369/392`），PR-6/7a 范围 |
| `change_core` 的**停止态**分支（D5=A 新增）                 | **保留，条件触发** | B4                                                   |

**结论：`restart()` 归零不了**，方法与 `target_core` 机制都必须留着（D3 只删 `apply_candidate`）。

**F8 的重新表述**：`restart()` 的 `target_core.take()` 是一次性消费，其目的是「回滚深路径不得重启失败的新核」。B4 删掉整条深回滚路径后，**这个防护对象消失了**——`take()` 从「防回滚误重启」退化为「防同一 lease 内第二次 restart 误用陈旧目标」。因此 T-B4-03 改钉真实语义（见 §6.3）：**同一 lease 内 `check_and_promote` 设置的 `target_core` 只能被消费一次；若停止态分支触发，`restart()` 使用的正是本次 promote 的新核**。

**验证（一）：** `rg 'lease\.restart\(\)|\.restart\(\)\.await' backend/tauri/src/client/rebuild.rs` 只剩 `regenerate_and_restart_for_legacy` 与 change_core 的停止态分支两处。

命令 `change_clash_core` 返回 `MutationOutcome<RuntimeApplyReport>`（D4=A）；外层 `run_legacy_verge_mutation` 的 typed 重播**保留**（PR-7a 才删）。

**验证（二）：** `rg 'restore_promoted|RuntimeTransactionSnapshot' backend/tauri/src` 为 0。

### S7 — degradation 映射与 `RuntimeApplyReport`

新增 `RuntimeApplyReport`（D4=A 的三字段，`serde` + `specta`；F25 确认 workspace 已启用 specta，upstream 类型可直接内嵌）与 `RuntimeApplyOutcome`（D5=A），放在 `client/runtime.rs` 与 `MutationOutcome` 同层。

新增纯函数 `map_apply_outcome(data: &CoreApplyData, promoted_revision: u64) -> (RuntimeApplyReport, Vec<Degradation>)`，**§3 的 12 格矩阵全部由它决定**——它是唯一决策点，测试直接打它。**停止态启核（D5=A）不经过它**：那条路径没有 `CoreApplyData`，由 S6 直接构造——成功侧 `RuntimeApplyOutcome::Started` + `applied_revision = Some(promoted)`，失败侧 `core_start_failed` 降级且 `applied_revision` 保持旧值（R1）。这不是「第二个决策点」：它判的是「有没有 apply 结果」，不是「apply 结果是什么」。

**同时收敛 §2.4 的两条投递路径：**

- 删掉 `map_runtime_rebuild_degradation`（`mod.rs:981`）与它那条「错误面撑不住精度」的 doc——5b 之后 rebuild 失败带着 §2.2 的 phase/code 出来，`collect_post_commit_degradations` 直接并入即可；
- 后台 worker 闭包（`mod.rs:435-449` 处注册的那条）改为：拿到 degradations 后逐条 `inner.degradation.publish(..)`，`tracing::warn!` 保留。

**验证：** T-AP-01…12 全绿；T-PC-09 证明后台失败到达 sink。

### S8 — 测试适配

按 §6 的清单逐个改。原则：**只改构造与断言目标，不改测试语义**；coordinator 的 5 个测试与 5a 的 16 个 `client/core.rs` 测试**期望零改动**（若被迫改动，说明 B2/B3 溢出了范围，停下核查）。

### S9 — 门禁

```powershell
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
cargo build --manifest-path .\backend\Cargo.toml -p fake-core --bin manager-probe-core
pnpm test:backend
git diff frontend/interface/src/ipc/bindings.ts
pnpm lint:ts
pnpm architecture-ledger
pnpm lint:architecture-ledger
```

**bindings 预期差异（恰好这些）**：新增 `RuntimeApplyReport` 与 `RuntimeApplyOutcome` 两个 TS 类型（D5=A：第二个是**本仓自有**枚举，不是 upstream 的 `ApplyOutcomeKind`）；`changeClashCore` 与 `patchClashConfig` 的返回类型由 `null` 变为 `MutationOutcome<RuntimeApplyReport>`。**其余零变化**——四条 runtime 读 IPC、`getCoreStatus` 均不得变。

**ledger 预期**：`config_calls` 应**下降**（B4 删掉 `change_core` 的 legacy draft/apply/discard 三处 `Config::verge()`）；`migration_markers` 应下降（`rebuild.rs` 的 core-selection 与 log-sink 两条 TODO 随 B4 删除）；`test_real_dirs` **必须仍为 0**。逐项核对后再 `--write-snapshot`。

---

## 6. 测试矩阵

### 6.1 apply parity（RQ-03；§3 的 12 格）

| ID         | 组合                                        | 断言                                                                                 |
| ---------- | ------------------------------------------- | ------------------------------------------------------------------------------------ |
| T-AP-01    | `Noop` 无 warning                           | Applied **推进**至 Promoted；`Applied`；degradations 空                              |
| T-AP-02    | `Noop` + warning                            | Applied 推进；`CommittedDegraded`；恰 1 条 `RuntimeApply` durability                 |
| T-AP-03/04 | `Patched` 无/有 warning                     | 同上形态                                                                             |
| T-AP-05/06 | `Reloaded` 无/有 warning                    | 同上形态                                                                             |
| T-AP-07/08 | `Restarted` 无/有 warning                   | 同上形态                                                                             |
| T-AP-09/10 | `Switched` 无/有 warning                    | 同上形态                                                                             |
| T-AP-11    | `RolledBack` 无 warning                     | Applied **不推进**；`CommittedDegraded`；恰 1 条 `CoreRollback`                      |
| T-AP-12    | `RolledBack` + warning                      | Applied 不推进；**2 条** degradation（`CoreRollback` + durability）                  |
| T-AP-13    | Local / Service 双后端对同一 outcome 同映射 | 两侧 `map_apply_outcome` 结果逐字段相等（Service 侧套 `transport_available()`，F24） |

### 6.2 post-commit 失败矩阵（RQ-01；§2.2 的七项）

| ID      | 失败注入                     | 入口                             | 断言                                                                                                                                                                                                                                                       |
| ------- | ---------------------------- | -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-PC-01 | acquire 超时                 | `change_core`                    | `Err`；**desired 未提交**（typed 快照未变）                                                                                                                                                                                                                |
| T-PC-02 | build 失败                   | `change_core`                    | `CommittedDegraded`；`RuntimeBuild`；desired = 新核                                                                                                                                                                                                        |
| T-PC-03 | check 失败                   | `change_core`                    | `CommittedDegraded`；`RuntimeCheck`；Promoted 未推进                                                                                                                                                                                                       |
| T-PC-04 | promote 失败                 | `patch_running_config`           | `CommittedDegraded`；`RuntimePromote`；产物保持旧值                                                                                                                                                                                                        |
| T-PC-05 | revision 冲突                | `patch_running_config`           | `CommittedDegraded`；`RuntimeApply` / `revision_conflict`；Applied 不变                                                                                                                                                                                    |
| T-PC-06 | IPC 传输丢失                 | `change_core`                    | `CommittedDegraded`；`core_transport_lost`；Applied 不变。**实现方式（L7）：优先用 `TestBackend` 脚本化传输错误**——不需要真 harness、也就不需要 `transport_available()` 守卫；只有在必须验证真实 `ClientError` 映射时才起真 IPC harness，那时套守卫（F24） |
| T-PC-07 | apply error（非 RolledBack） | `patch_running_config`           | `CommittedDegraded`；`runtime_apply_failed`；Applied 不变                                                                                                                                                                                                  |
| T-PC-08 | 启动路径同类失败             | `promote_default_runtime_config` | **`Err`**（无 desired commit）——证明分界线按入口区分                                                                                                                                                                                                       |
| T-PC-09 | 后台 rebuild 失败            | `RebuildCoordinator` worker      | **degradation 到达 `CoreDegradationSink`**（用 `RecordingSink`，`client/core.rs:529` 已有），而不是只进日志（§2.4 的 I-B 缺口）；同一失败经**同步** caller 时则出现在 `MutationOutcome` 里                                                                 |

### 6.3 B1/B2/B4 结构测试

| ID      | 断言                                                                                                                                                                                                             |
| ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-LC-01 | Promoted 推进拒绝非递增 revision（迁自 F14 的 `publish_promoted` 校验）                                                                                                                                          |
| T-LC-02 | Applied 推进要求存在 Promoted 且 `identity_eq`（迁自 `publish_applied` 校验）                                                                                                                                    |
| T-LC-03 | `lifecycle()` 是同步 watch 克隆，慢 `Run` 阻塞期间**立即返回**（沿用 5a 的活性性质）                                                                                                                             |
| T-B2-01 | **两个并发 rebuild 不重叠**，后一个在 `OperationGate` FIFO 之后**读取最新 snapshot**（Exit 判据原文）                                                                                                            |
| T-B2-02 | `promote_default_runtime_config` 在 guard 内构建 snapshot 与 candidate（F12 例外已修）                                                                                                                           |
| T-B4-01 | change-core `RolledBack`：desired = **新核**、Promoted = **新配置**、Applied = **旧值**（Exit 判据原文）                                                                                                         |
| T-B4-02 | change-core 成功（**核在跑**）：三者一致推进；`RuntimeApplyReport.outcome == Switched`；**全程零次 `restart()`**——切核由 `apply` 承载（L3 的真实语义）                                                           |
| T-B4-03 | change-core（**核已停**，D5=A 分支）：`RunningIdentity` 返回 `Ok(None)` → 恰好一次 `restart()`，且该次 Run 请求用的是**本次 promote 的新核**（`target_core.take()` 消费的正是它，F8 重述）；`outcome == Started` |
| T-B4-04 | `restart()` 的 `target_core` 一次性消费：同一 lease 内第二次 `restart()` 落回 typed 快照而非重用陈旧目标（把 F8 的机制本身钉住，与 change_core 流程解耦）                                                        |
| T-B4-05 | change-core 停止态分支的**失败侧**（R1）：`restart()` 失败 → `CommittedDegraded`，`phase = CoreLifecycle` / `code = "core_start_failed"`；desired = 新核（已提交）、Promoted = 新配置、**Applied 不推进**        |

### 6.4 回归（期望零改动通过）

- `rebuild.rs` coordinator 五连：`:469 / 505 / 537 / 573 / 623`（B2 的 coalesce 不变式）；
- `client/core.rs` 的 16 个 5a 测试（`:671`…`:1182`）——**若被迫修改，说明范围溢出**；唯一预期的例外见下一条；
- **`rollback_build_failure_restarts_the_committed_old_core`（`rebuild.rs:1276`，`9727ef1d4` 为 PR-5a Finding 1 新增）**：它脚本化的正是 change_core 的**回滚重建失败**分支，而 B4 把整条深回滚路径删掉。该测试**必须删除**，其保护的不变量由 **T-B4-04** 接手（直接钉 `target_core` 的一次性消费机制，不再依赖已消失的回滚流程）。实施时在 commit body 里写明这次接管，否则会被读成「删了个回归测试」；
- `change_core_rolls_back_via_second_regenerate_and_restart`（`rebuild.rs:710`）：同理**删除**——它断言的二次重建+重启在 B4 后不存在；
- `s04_concurrent_restart_waits_until_change_core_rollback_completes`（`rebuild.rs:931`）：B4 删掉回滚分支后该测试**必须重写**为「并发 restart 在 change_core 事务后串行执行」，语义（互斥）保持。

---

## 7. Exit 判据映射

| task.md B-Exit                                                                                     | 交付步骤 | 验证                              |
| -------------------------------------------------------------------------------------------------- | -------- | --------------------------------- |
| `rg 'rebuild_gate\|clash_patch_gate\|RunningConfigPatchPort\|LegacyRunningConfigPatchBridge'` 为 0 | S3、S5   | 该 `rg` 命令输出为空              |
| apply parity：Noop/Patched/Reloaded/Restarted/Switched/RolledBack；Warning 正交                    | S4、S7   | T-AP-01…13 全绿（12 格 + 双后端） |
| change-core rollback 断言 desired=new、Promoted=new、Applied=old                                   | S6       | T-B4-01                           |
| 两个并发 rebuild 不重叠，后一个读最新 snapshot                                                     | S3       | T-B2-01                           |
| RQ-01 已作答（含 §2.4 的 degradation 投递路径）                                                    | §2       | T-PC-01…09                        |
| RQ-03 已作答（含 R1 的 `Started` 终态）                                                            | §3       | T-AP-01…13 + T-B4-03 / T-B4-05    |

---

## 8. 风险与回滚

| 风险                                                      | 概率 | 影响                    | 缓解                                                                                                                                                                                          |
| --------------------------------------------------------- | ---- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Promoted/Applied 迁入 actor 后四条读 IPC 行为漂移         | 中   | 前端读到空/陈旧 runtime | facade 方法保持同名同签名，内部改实现；四条 IPC 的 bindings 必须零变化（S9 判据）                                                                                                             |
| 删 `rebuild_gate` 后 coalesce 语义被牵连                  | 中   | 重复/丢失 rebuild       | `RebuildCoordinator` 一行不改；五个 coordinator 测试零改动通过（T-6.4）                                                                                                                       |
| `apply_promoted` 改道后重试语义丢失                       | 中   | 瞬时失败变成硬失败      | 旧路径是 5 次 250 ms 重试（F10）。**判据已在 S4 预先定死**：仅传输类错误在 `CoreLeaseAdapter` 层补 5 × 250 ms，check / 语义失败 / `RolledBack` 一律不重试；实测数据入实施报告，但**不改判据** |
| `change_core` 在停止态下的行为回归                        | 中   | 切核后核不再自动启动    | D5=A 的 `RunningIdentity` 分支保留今天的启核行为；成功侧 T-B4-03、失败侧 T-B4-05 钉住                                                                                                         |
| `RolledBack` 被误判为成功                                 | 中   | Applied 错误推进        | §3 的映射集中在 `map_apply_outcome` 一个纯函数里；T-AP-11/12 直接打它                                                                                                                         |
| `target_core.take()` 在多次 restart 下失效                | 低   | 启核用错目标            | T-B4-04 直接钉机制本身（不再依赖 B4 已删除的回滚流程）；T-B4-03 钉停止态分支只消费一次                                                                                                        |
| B3 删除面过大牵连 profile mutation 的 `CommittedDegraded` | 中   | 既有 degraded 路径回归  | `MutationOutcome::from_parts` 是唯一产出点（F22），不动它；profile 侧测试零改动                                                                                                               |
| Service parity 测试在 CI 某平台不可用                     | 中   | parity 静默消失         | 沿用 5a 规则：`transport_available()` 守卫 + 支持平台上必须常规运行（F24）                                                                                                                    |

**回滚：** 改动集中在 `core/actor/{mod,backend,runtime,types}.rs`、`client/{mod,core,rebuild,runtime}.rs`、`ipc.rs`、`setup.rs`、`bridge/verge.rs`。S1–S2 可独立成一个 commit（类型迁移 + actor 增字段，生产路径未变），单独回滚不影响行为。

---

## 9. 提交切分建议

1. `refactor(core): move runtime lifecycle types into the actor module` —— S1（纯搬迁，零行为）；
2. `feat(core): own promoted and applied state in CoreActor` —— S2 + T-LC；
3. `refactor(client): replace rebuild_gate with the core operation guard` —— S3 + T-B2；
4. `feat(core): route promoted apply through the core backend` —— S4 + S7 + T-AP；
5. `refactor(client): delete the api-first patch and compensation layer` —— S5 + T-PC-01…08；
6. `feat(client): publish background rebuild degradations to the core sink` —— §2.4 的两项动作 + T-PC-09；
7. `refactor(client): make change_core a commit-first mutation` —— S6 + T-B4 + S8 + S9。

第 2 步与第 3 步**必须分开**：前者改所有权，后者改并发原语；混在一起的 diff 无法判断回归来自哪一侧。第 6 步单独成 commit：它是 §2.4 认定的既有 I-B 缺口修复，与 B1–B4 的删除面无关，混进去会让「本来就漏、还是这次改漏」分不清。

---

## 10. 明确 out-of-scope（登记去向）

| 项                                                               | 去向                                                 |
| ---------------------------------------------------------------- | ---------------------------------------------------- |
| watch snapshot 的状态投影扩展、100 条 `LogFrame` ring            | **PR-5c / C1**                                       |
| `set_mode` / `reconcile_mode`、删 5 s 轮询与 statics             | **PR-5c / C2**                                       |
| macOS DNS 归入 actor                                             | **PR-5c / C3**                                       |
| `feat::patch_clash_with_rebuild` 的 sysproxy/systray/locale 后效 | **PR-6e**                                            |
| `on_profile_change` 的连接中断服务                               | **PR-6**                                             |
| `UpdaterManager::global()`                                       | **PR-6d**                                            |
| `run_legacy_verge_mutation` 的 typed 重播                        | **PR-7a**                                            |
| `feat.rs:79` 的 `change_clash_mode` 裸 `put_configs`             | **PR-6**（不在本阶段的 apply 管线内）                |
| `ControllerBinding` / `config_patch_from_mapping`                | **不存在**（F20）；B3 卡该两项记为 no-op，不新造再删 |

---

## 11. 附录 A — 接线单点声明（normative；其它小节只引用，不复述）

> 沿用 5a 的反漂移机制：本附录是 5b 全部新增/变更类型与消息的**唯一声明处**。

### A.1 actor 侧新增

```rust
// core/actor/runtime.rs（S1 从 client/runtime.rs 迁入，逻辑不改）
pub(crate) struct RuntimeRevision(u64);
pub(crate) struct RuntimeSnapshotData { /* 字段与迁移前逐字相同 */ }
pub(crate) struct RuntimeSnapshot { /* 字段与迁移前逐字相同 */ }
pub(crate) struct RuntimeLifecycleState {
    pub(crate) promoted: Option<Arc<RuntimeSnapshot>>,
    pub(crate) applied: Option<Arc<RuntimeSnapshot>>,
}
pub(crate) struct CandidateFile { /* 含 Drop；L1 同迁 */ }

// 注意（L2）：RuntimeRevisionAllocator **留在 client/runtime.rs**，
// 它 `use crate::core::actor::runtime::RuntimeRevision`。actor 不持有 allocator。

// core/actor/mod.rs —— CoreActorState 新增字段（其余字段见 5a 现状）
pub(crate) lifecycle: RuntimeLifecycleState,
pub(crate) lifecycle_tx: watch::Sender<RuntimeLifecycleState>,

// CoreActorArgs 新增
pub(crate) lifecycle_tx: watch::Sender<RuntimeLifecycleState>,
```

### A.2 新增消息（两条，全部守卫消息）

```rust
CheckAndPromote {
    operation: OperationId,
    request: CoreRequest,
    candidate: CandidateFile,          // core/actor/runtime.rs（L1）
    reply: RpcReplyPort<Result<Arc<RuntimeSnapshot>, CoreActorError>>,
},
ApplyPromoted {
    operation: OperationId,
    request: CoreRequest,
    expected: Option<RevisionIdInfo>,
    reply: RpcReplyPort<Result<CoreApplyData, CoreActorError>>,
},
// L5：不加 LifecycleSnapshot 诊断消息。lifecycle_tx 在处理函数内、reply 之前发布
// （F30 的时序），所以 await reply 后读 lifecycle() 已经确定性可见。
```

### A.3 client 侧新增与删除

```rust
// CoreClient 新增
pub(crate) fn lifecycle(&self) -> RuntimeLifecycleState;          // 同步 watch 克隆
pub(crate) async fn check_and_promote(&self, op: &CoreOperationGuard, req: &CoreRequest, cand: CandidateFile)
    -> Result<Arc<RuntimeSnapshot>, CoreActorError>;
pub(crate) async fn apply_promoted(&self, op: &CoreOperationGuard, req: &CoreRequest, expected: Option<RevisionIdInfo>)
    -> Result<CoreApplyData, CoreActorError>;

// NyanpasuClientInner 删除字段
- clash_patch, clash_patch_gate, rebuild_gate, runtime
// NyanpasuClientInner 保留字段（L2）
  runtime_revisions: runtime::RuntimeRevisionAllocator
// NyanpasuClientInner 新增字段（L4）
+ degradation: Arc<dyn crate::core::actor::backend::CoreDegradationSink>

// ClientSetupArgs 删除字段
- clash_patch
```

### A.4 wire 类型（D4=A + D5=A）

```rust
// client/runtime.rs，与 MutationOutcome 同层。
// F25：workspace 已启用 specta，upstream apply 类型自带 specta::Type，无需镜像 DTO。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeApplyOutcome {
    Noop,
    Patched,
    Reloaded,
    Restarted,
    Switched,
    RolledBack,
    /// 核原本停止，本次操作把它启起来了（D5=A）。upstream 的 apply 不启核，
    /// 因此这个状态在 `ApplyOutcomeKind` 里没有对应变体。
    Started,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeApplyReport {
    pub outcome: RuntimeApplyOutcome,
    pub desired_revision: u64,
    pub applied_revision: Option<u64>,
}
```

命令返回：`change_clash_core` 与 `patch_clash_config` 均改为 `Result<MutationOutcome<RuntimeApplyReport>>`。

### A.5 唯一的映射决策点

```rust
/// §3 的 12 格矩阵**全部**由它决定；outcome→Applied 推进、warning→追加 degradation
/// 都在这里，其它地方不得再判 outcome。
pub(crate) fn map_apply_outcome(
    data: &CoreApplyData,
    promoted_revision: u64,
) -> (RuntimeApplyReport, Vec<Degradation>);
```

### A.6 degradation code 常量表（§2.2 / §3 引用）

| code                              | phase            | retryable |
| --------------------------------- | ---------------- | --------- |
| `runtime_build_failed`            | `RuntimeBuild`   | true      |
| `runtime_check_failed`            | `RuntimeCheck`   | true      |
| `runtime_promote_failed`          | `RuntimePromote` | true      |
| `revision_conflict`               | `RuntimeApply`   | true      |
| `core_transport_lost`             | `RuntimeApply`   | true      |
| `core_backend_unavailable`        | `RuntimeApply`   | true      |
| `runtime_apply_failed`            | `RuntimeApply`   | true      |
| `core_not_running`                | `RuntimeApply`   | true      |
| `core_start_failed`               | `CoreLifecycle`  | true      |
| `core_rollback`                   | `CoreRollback`   | true      |
| `core_apply_durability_uncertain` | `RuntimeApply`   | false     |
