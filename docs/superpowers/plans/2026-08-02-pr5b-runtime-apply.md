# PR-5b 实施计划 — 单一 runtime apply 管线

**日期：** 2026-08-02
**版本：** v1
**分支基线：** `refactor/core-manager-actor` @ `9727ef1d4`（PR-5a 阶段门已关闭：`ae4e0f288` + `6a3878bba` + `5277482a5` + `9727ef1d4`）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/task.md` 卡 B1–B4；`design.md` §6–§8
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.2；必答项 §6.4 **RQ-01 / RQ-03**
**平台：** Windows 11 / PowerShell

> **本计划对着已实施的 5a 代码写，不是对着 5a 计划的纸面附录写。** 全部事实来自 `9727ef1d4` 的工作树。

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

---

## 2. RQ-01 — post-commit 失败矩阵（必答）

### 2.1 分界线的定义

**分界线是「typed desired state 是否已经提交」**，不是「操作是否已经开始」。

- `ApplicationClient::patch` / `ClashConfigClient::patch` 返回 `Ok` 的那一刻起，用户意图已经持久化 → 此后任何失败都**不得**表现为普通 `Err`（否则 UI 会显示「失败」而磁盘上已经变了）；
- 在此之前的任何失败，desired 未动，返回 `Err` 是诚实的。

因此 5b 的三类入口有不同的分界位置：

| 入口                                                                                                                         | 有无 desired commit     | 分界                                                     |
| ---------------------------------------------------------------------------------------------------------------------------- | ----------------------- | -------------------------------------------------------- |
| `patch_running_config`（B3）、`change_core`（B4）                                                                            | **有**，且在最前        | commit 之后全部 degraded                                 |
| `rebuild_running_config`（后台脏重建）                                                                                       | 无（响应更早的 commit） | **全部 degraded**——commit 早已发生，此处只是迟到的副作用 |
| `promote_existing_runtime_product` / `start_promoted_runtime` / `promote_default_runtime_config`（启动路径）、`restart_core` | 无                      | 全部 `Err`（没有已提交的用户意图需要保护）               |

### 2.2 七项逐条作答

`P` 列 = 该失败发生在分界线之前（`pre`）还是之后（`post`）。`post` 一律映射为 `Degradation { phase, code, retryable }` 并经 `MutationOutcome::from_parts` 变成 `CommittedDegraded`。

| #   | 失败                       | 触发点                                                             | P       | commit-first 入口的结果                                                                                                                                                                 | 无-commit 入口的结果 |
| --- | -------------------------- | ------------------------------------------------------------------ | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 1   | **operation acquire 超时** | `begin_operation()` 等待 `CORE_ACQUIRE_TIMEOUT`（F6）              | **pre** | `Err(OperationError::AcquireTimeout)`——**guard 在 desired commit 之前取得**（见 S3 的顺序约束），因此永远是 pre                                                                         | `Err`                |
| 2   | **build 失败**             | `regenerate_runtime_with` 的 `spawn_blocking` 构建段               | post    | `phase = RuntimeBuild`、`code = "runtime_build_failed"`、`retryable = true`                                                                                                             | `Err`                |
| 3   | **check 失败**             | `CoreBackend::check`（dry-run）                                    | post    | `phase = RuntimeCheck`、`code = "runtime_check_failed"`、`retryable = true`                                                                                                             | `Err`                |
| 4   | **promote 失败**           | candidate 哈希不符 / `restore_product` 写失败 / promote 后校验不符 | post    | `phase = RuntimePromote`、`code = "runtime_promote_failed"`、`retryable = true`；**Promoted 不推进，产物保持旧值**                                                                      | `Err`                |
| 5   | **revision 冲突**          | `CoreBackend::apply` 的 CAS（`Error::RevisionConflict`）           | post    | `phase = RuntimeApply`、`code = "revision_conflict"`、`retryable = true`；**Applied 不变**，下一次 rebuild 会带新 revision 重试                                                         | `Err`                |
| 6   | **IPC 连接丢失**           | Service backend 的传输错误（`ClientError`）                        | post    | `phase = RuntimeApply`、`code = "core_transport_lost"`、`retryable = true`；**Applied 不变**                                                                                            | `Err`                |
| 7   | **apply error**            | `CoreApplyData.outcome == RolledBack`，或 backend 返回 `Err`       | post    | `RolledBack` → `phase = CoreRollback`、`code = "core_rollback"`、`retryable = true`；其它 apply 错误 → `phase = RuntimeApply`、`code = "runtime_apply_failed"`。两者 **Applied 均不变** | `Err`                |

### 2.3 三条不变量

- **I-A（不撒谎）**：desired 已提交时**绝不**返回 `Err`——只返回 `CommittedDegraded`；
- **I-B（不静默）**：任何 `post` 失败**必须**产出至少一条 `Degradation`，不允许只写日志；
- **I-C（状态单调）**：`post` 失败不得回退已经推进的 Promoted；Applied 只在 backend 确认采纳新 revision 时才推进（§3）。

> 与 5a 的 `CoreActorError` 的关系：`StaleOperation` / `ShuttingDown` 属于**内部不变量破坏**，不映射 degradation——它们只可能出现在实现有 bug 或进程正在关停时，按 `Err` 上抛并记日志。`NoBackend` 在 commit-first 入口按第 6 行处理（`code = "core_backend_unavailable"`）。

---

## 3. RQ-03 — apply parity 矩阵（必答）

### 3.1 六个 outcome + 一个正交标志

`CoreApplyData { outcome: ApplyOutcomeKind, revision: ConfigRevisionInfo, warning: Option<String>, failed_apply: Option<String> }`。

**`Warning` 不是第七个分支**——它是与 outcome 正交的标志位，可以与**任何**一个 outcome 组合出现（来源是 runtime 的 `ApplyOutcome::DurabilityUncertain` 包装，可嵌套两层并以 `"; "` 拼接）。

| outcome      | Applied 是否推进 | 返回                                        | 说明                                                                                 |
| ------------ | ---------------- | ------------------------------------------- | ------------------------------------------------------------------------------------ |
| `Noop`       | **推进**         | `Applied`                                   | 配置已在生效——运行的就是该 revision，Applied 必须等于 Promoted，否则读模型会永远滞后 |
| `Patched`    | **推进**         | `Applied`                                   | 就地 `PATCH /configs`                                                                |
| `Reloaded`   | **推进**         | `Applied`                                   | 就地 `PUT /configs`                                                                  |
| `Restarted`  | **推进**         | `Applied`                                   | 同 epoch 内换进程                                                                    |
| `Switched`   | **推进**         | `Applied`                                   | 换核（B4 的正常成功路径）                                                            |
| `RolledBack` | **不推进**       | `CommittedDegraded { phase: CoreRollback }` | **旧配置在跑**；desired 与 Promoted 保留新值，Applied 保留旧值                       |

**Warning 的处理（与上表正交）：**

- `warning.is_some()` 时**追加**一条 `Degradation { phase: RuntimeApply, code: "core_apply_durability_uncertain", retryable: false }`；
- **不改变**上表的 Applied 推进决策；
- 因此 `Applied + warning` 会变成 `CommittedDegraded`（`from_parts` 见 F22），而 `RolledBack + warning` 会有**两条** degradation。

### 3.2 parity 测试要求

矩阵是 **6 × 2 = 12 个组合**（六个 outcome × warning 有/无），每个组合都要断言三件事：Applied 是否推进、返回的 `MutationOutcome` 变体、degradation 列表内容。测试编号 T-AP-01…12（§6）。

**双后端 parity**：Local 与 Service 对同一 outcome 必须产出同一映射结果。Local 侧由 `TestBackend` 脚本化 `CoreApplyData`；Service 侧沿用 5a 的 IPC harness，**并套 `transport_available()` 守卫**（F24）。

---

## 4. 需 leader 裁定的决策点

### D1 — `RuntimeSnapshot` 等类型放哪

B1 要把 Promoted/Applied 搬进 actor，但 `RuntimeSnapshot` / `RuntimeRevision` / `RuntimeLifecycleState` 现在住在 `client/runtime.rs`（F11 邻域）。

- **推荐 A**：把 `RuntimeRevision` / `RuntimeSnapshot` / `RuntimeLifecycleState` / `RuntimeRevisionAllocator` 移到 **`core/actor/runtime.rs`**（新文件），`client/runtime.rs` 只留构建侧的 `CandidateFile` / `RuntimeTransactionSnapshot` / degradation wire 类型。理由：所有权跟着数据走；actor 反向依赖 `client::` 是层次倒置。
- **选项 B**：类型不动，actor 直接 `use crate::client::runtime::*`。改动小，但让 `core::actor` 依赖 `client`，与 5a 建立的方向相反。

### D2 — lifecycle 用第二条 watch 还是塞进 `CoreStatusView`

B1 要求「CoreClient 通过 watch 暴露 lifecycle」。

- **推荐 A：第二条 watch 通道** `watch::Sender<RuntimeLifecycleState>`，与 `status_tx` 并列。理由：`CoreStatusView` 是 UI 投影（5 个小字段，每次 `commit()` 都克隆），而 `RuntimeSnapshot` 持有产物字节与整个 config Mapping——塞进同一条通道会让每次状态变化都克隆一份重量级快照。
- **选项 B**：扩 `CoreStatusView`。省一条通道，但把重负载塞进高频路径，且会改 5a 已稳定的 `commit()` 语义（F2）。

### D3 — `apply_candidate` 的去留

`apply_candidate`（F9）今天只被 `restore_applied_after_patch_failure` 使用，而后者随 B3 删除。

- **推荐 A：一并删除** `apply_candidate`，`CoreLifecycleLease` 收敛到 4 个方法。理由：删掉唯一调用者后它就是死代码。
- **选项 B**：保留备用。**不推荐**——违反「不留无调用者的抽象」。

### D4 — `change_core` 的 wire 变化幅度

B4 让 `change_clash_core` 返回 `MutationOutcome<RuntimeApplyReport>`（F21：今天返回 unit）。

- **推荐 A**：新增 `RuntimeApplyReport { outcome: ApplyOutcomeKind, desired_revision: u64, applied_revision: Option<u64> }`（design §8 的形状），命令返回 `MutationOutcome<RuntimeApplyReport>`。bindings 因此新增 `RuntimeApplyReport` 与 `ApplyOutcomeKind` 两个 TS 类型 + 命令返回类型变化——**这是本阶段唯一的 wire 变化**，S13 按「恰好这些」核对。
- **选项 B**：仍返回 unit，degraded 只进日志。**不推荐**——B4 卡明写「前端复用通用 `RuntimeApplyReport`/MutationOutcome 展示 degraded」。

---

## 5. 实施步骤

> 每步给出编辑内容 → 验证 → 通过判据。**不要**跑 `cargo clippy -- -D warnings`（仓库本就红）。已知坑：共享 target 的 kache 污染会造成本地 clippy 假红，用独立 `--target-dir` 复验再判定。

### S1 — 迁移 runtime lifecycle 类型（按 D1=A）

新建 `backend/tauri/src/core/actor/runtime.rs`，把 `RuntimeRevision` / `RuntimeRevisionAllocator` / `RuntimeSnapshot` / `RuntimeSnapshotData` / `RuntimeLifecycleState` 从 `client/runtime.rs:26-105` 整体搬入（**逻辑一字不改**，只改可见性为 `pub(crate)` 与 `use` 路径）。`client/runtime.rs` 保留 `CandidateFile`、`RuntimeTransactionSnapshot`、`MutationOutcome` / `Degradation` / `DegradationPhase`、`compensation_*`（后者在 S5 删）。

**验证：** `cargo check`；`rg 'client::runtime::RuntimeSnapshot' backend/tauri/src` 为 0。

### S2 — B1：Promoted / Applied 入 actor

**附录 A.1 声明全部新增字段与消息，此处只述行为。**

- `CoreActorState` 增 `lifecycle: RuntimeLifecycleState`、`lifecycle_tx: watch::Sender<RuntimeLifecycleState>`、`revisions: RuntimeRevisionAllocator`；
- 新增消息 `CheckAndPromote` / `ApplyPromoted` / `PublishApplied`（见 A.2），全部是**守卫消息**（校验 active `OperationId`，与 5a 的 mutation 同等准入）；
- promote 成功推进 `lifecycle.promoted` 并发布 `lifecycle_tx`；apply 成功按 §3 决定是否推进 `lifecycle.applied`；
- **`publish_promoted` 的「拒绝非递增 revision」与 `publish_applied` 的「必须存在 Promoted 且 `identity_eq`」两条校验原样迁入 actor**（F14 的语义不能丢）；
- `CoreClient` 增 `lifecycle()`（同步 watch 克隆）与 `subscribe_lifecycle()`（`cfg(test)`）。

client 侧删除：`runtime` 字段、`publish_promoted` / `publish_applied` / `restore_promoted` / `promoted_runtime` / `runtime_lifecycle_state` 及 `runtime_revisions` 字段（revision 分配随 lifecycle 一起进 actor）。

> **`restore_promoted` 直接删除、不迁入**：它唯一的调用者是 `change_core` 的深回滚（F14），而 B4 删掉整条深回滚路径。

**四条 runtime 读 IPC**（`ipc.rs:346/362/377/390`）改读 `client.promoted_runtime()` 的新实现——facade 方法保留同名同签名，内部改为 `core_client.lifecycle().promoted`，**wire 不变**。

**验证：** `rg 'RuntimeLifecycleStore|publish_promoted|publish_applied|restore_promoted' backend/tauri/src` 为 0；四条读 IPC 的 bindings 不变。

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
      → 统一 rebuild（新核）
      → apply
      → 按 §3 映射：Switched/其它成功 → Applied 推进；RolledBack → Applied 保持旧值 + CommittedDegraded(CoreRollback)
```

**删除**：legacy `Config::verge().draft()/apply()/discard()`、回滚重建、`restore_product` 调用、第二次 old-core restart、`RuntimeTransactionSnapshot` 的 change_core 用法。

**`RolledBack` 时的终态**（B4 卡与 Exit 判据）：desired = 新核，Promoted = 新配置，Applied = 旧值。**不做**第二套应用层回滚。

> **F8 的语义必须继承**：`restart()` 的 `target_core.take()` 是一次性消费。B4 删掉回滚路径后，一次事务内只有一次 `check_and_promote` + 一次 apply，`take()` 语义自然满足；**但若实现里出现「同一 lease 内第二次 restart」，必须显式重设 `target_core`**，否则会回落到 typed 快照。S6 结束时用 `rg 'restart()' ` 复核 lease 生命周期内只有一次调用。

命令 `change_clash_core` 返回 `MutationOutcome<RuntimeApplyReport>`（D4=A）；外层 `run_legacy_verge_mutation` 的 typed 重播**保留**（PR-7a 才删）。

**验证：** `rg 'restore_promoted|RuntimeTransactionSnapshot' backend/tauri/src/client/rebuild.rs` 为 0。

### S7 — degradation 映射与 `RuntimeApplyReport`

新增 `RuntimeApplyReport`（D4=A 的三字段，`serde` + `specta`），放在 `client/runtime.rs` 与 `MutationOutcome` 同层。

新增纯函数 `map_apply_outcome(data: &CoreApplyData, promoted_revision: u64) -> (RuntimeApplyReport, Vec<Degradation>)`，**§3 的 12 格矩阵全部由它决定**——它是唯一决策点，测试直接打它。

**验证：** T-AP-01…12 全绿。

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

**bindings 预期差异（恰好这些）**：新增 `RuntimeApplyReport` 与 `ApplyOutcomeKind` 两个 TS 类型；`changeClashCore` 与 `patchClashConfig` 的返回类型由 `null` 变为 `MutationOutcome<RuntimeApplyReport>`。**其余零变化**——四条 runtime 读 IPC、`getCoreStatus` 均不得变。

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

| ID      | 失败注入                     | 入口                             | 断言                                                                    |
| ------- | ---------------------------- | -------------------------------- | ----------------------------------------------------------------------- |
| T-PC-01 | acquire 超时                 | `change_core`                    | `Err`；**desired 未提交**（typed 快照未变）                             |
| T-PC-02 | build 失败                   | `change_core`                    | `CommittedDegraded`；`RuntimeBuild`；desired = 新核                     |
| T-PC-03 | check 失败                   | `change_core`                    | `CommittedDegraded`；`RuntimeCheck`；Promoted 未推进                    |
| T-PC-04 | promote 失败                 | `patch_running_config`           | `CommittedDegraded`；`RuntimePromote`；产物保持旧值                     |
| T-PC-05 | revision 冲突                | `patch_running_config`           | `CommittedDegraded`；`RuntimeApply` / `revision_conflict`；Applied 不变 |
| T-PC-06 | IPC 传输丢失                 | `change_core`（Service）         | `CommittedDegraded`；`core_transport_lost`；Applied 不变                |
| T-PC-07 | apply error（非 RolledBack） | `patch_running_config`           | `CommittedDegraded`；`runtime_apply_failed`；Applied 不变               |
| T-PC-08 | 启动路径同类失败             | `promote_default_runtime_config` | **`Err`**（无 desired commit）——证明分界线按入口区分                    |

### 6.3 B1/B2/B4 结构测试

| ID      | 断言                                                                                                     |
| ------- | -------------------------------------------------------------------------------------------------------- |
| T-LC-01 | Promoted 推进拒绝非递增 revision（迁自 F14 的 `publish_promoted` 校验）                                  |
| T-LC-02 | Applied 推进要求存在 Promoted 且 `identity_eq`（迁自 `publish_applied` 校验）                            |
| T-LC-03 | `lifecycle()` 是同步 watch 克隆，慢 `Run` 阻塞期间**立即返回**（沿用 5a 的活性性质）                     |
| T-B2-01 | **两个并发 rebuild 不重叠**，后一个在 `OperationGate` FIFO 之后**读取最新 snapshot**（Exit 判据原文）    |
| T-B2-02 | `promote_default_runtime_config` 在 guard 内构建 snapshot 与 candidate（F12 例外已修）                   |
| T-B4-01 | change-core `RolledBack`：desired = **新核**、Promoted = **新配置**、Applied = **旧值**（Exit 判据原文） |
| T-B4-02 | change-core 成功：三者一致推进；返回 `Applied(RuntimeApplyReport)`                                       |
| T-B4-03 | 一次 change_core 事务内 `restart()` 只调用一次（F8 的 `take()` 语义不被破坏）                            |

### 6.4 回归（期望零改动通过）

- `rebuild.rs` coordinator 五连：`:469 / 505 / 537 / 573 / 623`（B2 的 coalesce 不变式）；
- `client/core.rs` 的 16 个 5a 测试（`:671`…`:1182`）——**若被迫修改，说明范围溢出**；
- `s04_concurrent_restart_waits_until_change_core_rollback_completes`（`rebuild.rs:931`）：B4 删掉回滚分支后该测试**必须重写**为「并发 restart 在 change_core 事务后串行执行」，语义（互斥）保持。

---

## 7. Exit 判据映射

| task.md B-Exit                                                                                     | 交付步骤 | 验证                              |
| -------------------------------------------------------------------------------------------------- | -------- | --------------------------------- |
| `rg 'rebuild_gate\|clash_patch_gate\|RunningConfigPatchPort\|LegacyRunningConfigPatchBridge'` 为 0 | S3、S5   | 该 `rg` 命令输出为空              |
| apply parity：Noop/Patched/Reloaded/Restarted/Switched/RolledBack；Warning 正交                    | S4、S7   | T-AP-01…13 全绿（12 格 + 双后端） |
| change-core rollback 断言 desired=new、Promoted=new、Applied=old                                   | S6       | T-B4-01                           |
| 两个并发 rebuild 不重叠，后一个读最新 snapshot                                                     | S3       | T-B2-01                           |
| RQ-01 已作答                                                                                       | §2       | T-PC-01…08                        |
| RQ-03 已作答                                                                                       | §3       | T-AP-01…13                        |

---

## 8. 风险与回滚

| 风险                                                      | 概率 | 影响                    | 缓解                                                                                                                                                    |
| --------------------------------------------------------- | ---- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Promoted/Applied 迁入 actor 后四条读 IPC 行为漂移         | 中   | 前端读到空/陈旧 runtime | facade 方法保持同名同签名，内部改实现；四条 IPC 的 bindings 必须零变化（S9 判据）                                                                       |
| 删 `rebuild_gate` 后 coalesce 语义被牵连                  | 中   | 重复/丢失 rebuild       | `RebuildCoordinator` 一行不改；五个 coordinator 测试零改动通过（T-6.4）                                                                                 |
| `apply_promoted` 改道后重试语义丢失                       | 中   | 瞬时失败变成硬失败      | 旧路径是 5 次 250 ms 重试（F10）；runtime 的 `apply_config` 自带 reconcile 超时——**S4 实施时须实测对比**，若确实变脆则在 backend 层补一层有界重试并记录 |
| `RolledBack` 被误判为成功                                 | 中   | Applied 错误推进        | §3 的映射集中在 `map_apply_outcome` 一个纯函数里；T-AP-11/12 直接打它                                                                                   |
| `target_core.take()` 在多次 restart 下失效                | 低   | 换核事务重启错核        | B4 后一次事务只有一次 restart；T-B4-03 钉住                                                                                                             |
| B3 删除面过大牵连 profile mutation 的 `CommittedDegraded` | 中   | 既有 degraded 路径回归  | `MutationOutcome::from_parts` 是唯一产出点（F22），不动它；profile 侧测试零改动                                                                         |
| Service parity 测试在 CI 某平台不可用                     | 中   | parity 静默消失         | 沿用 5a 规则：`transport_available()` 守卫 + 支持平台上必须常规运行（F24）                                                                              |

**回滚：** 改动集中在 `core/actor/{mod,backend,runtime,types}.rs`、`client/{mod,core,rebuild,runtime}.rs`、`ipc.rs`、`setup.rs`、`bridge/verge.rs`。S1–S2 可独立成一个 commit（类型迁移 + actor 增字段，生产路径未变），单独回滚不影响行为。

---

## 9. 提交切分建议

1. `refactor(core): move runtime lifecycle types into the actor module` —— S1（纯搬迁，零行为）；
2. `feat(core): own promoted and applied state in CoreActor` —— S2 + T-LC；
3. `refactor(client): replace rebuild_gate with the core operation guard` —— S3 + T-B2；
4. `feat(core): route promoted apply through the core backend` —— S4 + S7 + T-AP；
5. `refactor(client): delete the api-first patch and compensation layer` —— S5 + T-PC；
6. `refactor(client): make change_core a commit-first mutation` —— S6 + T-B4 + S8 + S9。

第 2 步与第 3 步**必须分开**：前者改所有权，后者改并发原语；混在一起的 diff 无法判断回归来自哪一侧。

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
pub(crate) struct RuntimeRevisionAllocator(AtomicU64);
pub(crate) struct RuntimeSnapshot { /* 字段与迁移前逐字相同 */ }
pub(crate) struct RuntimeLifecycleState {
    pub(crate) promoted: Option<Arc<RuntimeSnapshot>>,
    pub(crate) applied: Option<Arc<RuntimeSnapshot>>,
}

// core/actor/mod.rs —— CoreActorState 新增字段（其余字段见 5a 现状）
pub(crate) lifecycle: RuntimeLifecycleState,
pub(crate) lifecycle_tx: watch::Sender<RuntimeLifecycleState>,
pub(crate) revisions: RuntimeRevisionAllocator,

// CoreActorArgs 新增
pub(crate) lifecycle_tx: watch::Sender<RuntimeLifecycleState>,
```

### A.2 新增消息（全部守卫消息）

```rust
CheckAndPromote {
    operation: OperationId,
    request: CoreRequest,
    candidate: CandidateFile,
    reply: RpcReplyPort<Result<Arc<RuntimeSnapshot>, CoreActorError>>,
},
ApplyPromoted {
    operation: OperationId,
    request: CoreRequest,
    expected: Option<RevisionIdInfo>,
    reply: RpcReplyPort<Result<CoreApplyData, CoreActorError>>,
},
LifecycleSnapshot {
    reply: RpcReplyPort<RuntimeLifecycleState>,   // 仅测试/诊断；生产读走 watch
},
```

### A.3 client 侧新增与删除

```rust
// CoreClient 新增
pub(crate) fn lifecycle(&self) -> RuntimeLifecycleState;          // 同步 watch 克隆
#[cfg(test)] pub(crate) fn subscribe_lifecycle(&self) -> watch::Receiver<RuntimeLifecycleState>;
pub(crate) async fn check_and_promote(&self, op: &CoreOperationGuard, req: &CoreRequest, cand: &CandidateFile)
    -> Result<Arc<RuntimeSnapshot>, CoreActorError>;
pub(crate) async fn apply_promoted(&self, op: &CoreOperationGuard, req: &CoreRequest, expected: Option<RevisionIdInfo>)
    -> Result<CoreApplyData, CoreActorError>;

// NyanpasuClientInner 删除字段
- clash_patch, clash_patch_gate, rebuild_gate, runtime, runtime_revisions

// ClientSetupArgs 删除字段
- clash_patch
```

### A.4 wire 类型（D4=A）

```rust
// client/runtime.rs，与 MutationOutcome 同层
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeApplyReport {
    pub outcome: nyanpasu_ipc::api::core::apply::ApplyOutcomeKind,
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
| `core_rollback`                   | `CoreRollback`   | true      |
| `core_apply_durability_uncertain` | `RuntimeApply`   | false     |
