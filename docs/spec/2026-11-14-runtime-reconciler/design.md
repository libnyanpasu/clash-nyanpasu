# 运行时对账器：期望状态模型 设计

状态：待实施（2026-11-14）。基线 `main @ fdf5760fa`，runtime 子模块 `3854f2c`（v2.0.3-1）。

任务卡见同目录 `tasks.md`。

路径缩写（均在 `backend/` 下）：

| 缩写  | 路径                                             |
| ----- | ------------------------------------------------ |
| `C/`  | `nyanpasu-core/src/client/`                      |
| `AW/` | `nyanpasu-core/src/client/application_workflow/` |
| `CL/` | `nyanpasu-core/src/client/core_lifecycle/`       |
| `S/`  | `nyanpasu-core/src/state/`                       |
| `A2/` | `nyanpasu-core/src/actor_v2/`                    |
| `E/`  | `nyanpasu-state-engine/src/`                     |
| `V/`  | `nyanpasu-runtime-values/src/`                   |

## 0. 结论摘要

今天每一次会影响内核的配置修改，都是一笔“源配置提交”与“运行时应用”绑在一起的事务（选择性 TCC）：先在源事务的 prepare 阶段把候选配置应用到内核，再写文件，写失败则把内核恢复到基线。这个模型派生出参与者协议、Try／Confirm／Cancel、基线恢复、隔离模式、命令策略表、请求提示与延期预算。

本设计把模型换成期望状态：

1. 三个源 owner（Application、Clash、Profiles）持有的已提交配置就是期望状态。owner 只做校验、原子写入、发布快照，不等待应用。
2. 提交前只做一次校验：构建候选运行时配置，让内核以 `-t` 验证。校验不改变正在运行的任何东西，不通过就不提交。
3. 运行中的内核只由一个对账器 actor 改变。它被事件唤醒，读取三个域当前的已提交值和内核的当前状态，用纯函数决定下一步，再执行并记录。
4. “已提交但未生效”是一个带原因的状态，不是异常。没有撤销，没有隔离模式，没有重试预算。

用户裁定（2026-11-14）：采用本模型（A-1）；显式切换通过校验却应用失败时保持已提交并显示未生效（A-2）；应用结果未知后允许对账器自动继续（A-3）；校验始终走本地控制面（A-4）；不同域的配置必须能独立修改，单一 actor 不得阻塞其他域的保存。

## 1. 现状事实

以下事实来自对基线代码的四份盘点（变更路径、生命周期、控制面、测试夹具）与一次独立的计划审计，每条都有读到的代码位置。

### 1.1 变更路径

- 三个 owner 的提交序列相同（`S/application.rs:122-213`、`S/clash_config.rs:102-167`、`S/profiles/actor.rs:454-530` 与 `:1152-1405`）：校验，调用 `runtime_impact`，有影响时经 `replace_if_version_with_participant` 提交，否则 `replace_if_version`。
- 参与者的 `on_prepare` 在写文件之前运行完整的运行时 Try：构建、`-t` 校验、宿主移交（含安装或启动服务）、`reconcile`（`AW/tcc.rs:660-1048`）。owner 随后等待 `settlement`，直到 workflow 完成 Confirm 或 Cancel。
- 所有域共用 `ApplicationWorkflowActor` 一个邮箱。一个域的内核重启会阻塞其他域所有影响运行时的保存。
- Effects owner 在每次 `Replaced` 之后被通知，与运行时结果无关。
- “是否影响运行时”的字段清单手写了两份（`S/mutation/impact.rs:292-406` 的投影与 `:185-215` 的 patch 出现清单），另有五处重新枚举。
- Profiles 的文件内容不在快照里。构建时从磁盘读取（`C/runtime_build.rs:56`）；文件的替换发生在源事务的本地写里，先于 `profiles.yaml` 的写入（`S/profiles/actor.rs:1248`、`E/manager/persistent_state.rs:393`）。今天 workflow 从 Try 到源决定一直持有执行域，所以没有别的构建能读到“已替换但尚未提交”的文件。

### 1.2 把运行时带到已提交配置的路径

- 共九条路径：启动对账、显式启动、`Reconcile`、重建目标的重试、变更目标的重试、服务跟随、显式恢复、服务端点恢复后的还原、替换内核二进制后的重启。
- 它们走两套应用引擎。E1（`CL/workflow.rs:552-596`）不校验、不设端口过渡闸门，并且在内核接受之前就写出运行时产物文件；E2（`try_critical`）校验、设闸门、应用成功后才发布产物。
- `reestablish`（`AW/startup.rs:462-539`）已经是“观察、规划、执行”的形状：S1 取证，S2 用纯函数 `plan_owner` 决定所有者，S3 处理停止意图并停掉没有回执的实例，S4 从已停止的基线应用已提交配置。
- 内核状态变化今天不是触发源。workflow 只在命令、服务就绪变化和两个定时器到来时读取状态。
- 用户停止意图只存在于内存（`CoreIntent::Stopped`），且 `StopCore` 没有生产调用方。另一处把“观察到内核已停止”一律当作用户停止（`AW/tcc.rs:631-635`），因此对已停止内核的保存总是“保存但不启动”。
- 所有权证明只在内存，每次会话从 `Unproven` 开始。

### 1.3 控制面

- 本地控制面在服务托管内核期间仍然存活：移交用 `Stop` 而不是 `Shutdown`，`CoreClient` 整个会话都持有本地端点（`A2/mod.rs:1278,1484-1486`）。它的 `check` 不取 manager 锁、不进执行队列、在临时目录里验证，并发上限为 2，超出者等待；单次 `-t` 由进程适配器限时 30 秒。
- 今天的校验走“当前拥有内核的宿主”，在 workflow 的邮箱里执行，外面还包了一层 45 秒的等待上限（`AW/adapters.rs:112`）。
- core-manager 的 `reconcile` 在事务内对最终文档再跑一次 `-t`，在任何提交点之前。运行中的内核应用失败时它自己回到旧修订并报告 `RolledBack`，修订标识与尝试之前相同；从停止状态启动失败则是普通错误，状态成为带原因的 `Stopped`。
- 状态读取不能直接回答“内核在运行哪份文档”：`source_hash` 的算法是 core-manager 私有的，应用侧只能从一次 `reconcile` 的结果里得到它；本地 IPC 设置不被任何宿主发布。所以“观察到的等于期望的”只能靠对账器自己的应用回执与观察结果核对。
- `CoreActor` 每 2 秒读取一次端点状态并发布到 `watch<CoreStatusProjection>`，可以用作触发源。
- 脚本扩展的 HTTP 模块缓存由两个 shell 各自实现（`tauri/src/adapters/script_extensions/http.rs:106,144`、`nyanpasu-server/src/script_extensions.rs:91,107`），写入不是原子的。并发构建时读者可能读到写了一半的缓存而构建失败。

### 1.4 两处既有缺口（本设计不修）

- 运行中的内核收到 `expected_applied: None` 的 `reconcile` 时不做比较（`CM/manager/apply.rs:39-51`），与注释相反。
- `CoreFacade` 的待决动作在操作记录被淘汰或 daemon 重启后会一直被判为“未完成”，不回退到状态读取（`A2/facade.rs:251-283`）。今天只有应用重启能清除它；本设计之后仍然如此。

## 2. 目标与非目标

目标：

- G1 一次修改的提交不等待任何内核操作。一个域的保存不被另一个域引起的内核应用阻塞。
- G2 改变运行中内核的代码只有一个入口，一套应用顺序。
- G3 删除参与者协议、Try／Confirm／Cancel、基线恢复、隔离模式、命令策略表、请求提示、延期预算与收敛定时器。
- G4 线缆上的 `MutationOutcome` 与 `ConfigurationStatus` 保持形状，直到专门的线缆卡。

非目标：

- 不改 `A2/`（`CoreFacade`、`CoreClient`、`ServiceClient`）的分层与待决动作机制；不修 1.4 的两处缺口。
- 不改服务端点恢复：连接断开后的重连、daemon 被确认停止后的重新启动，以及它自己的次数上限（`CL/workflow.rs:111-167,430-456`）。它属于 IPC 连接一侧的既有机制。本设计只把“outage 之后内核是否该运行”交给对账器。
- 不改 Effects owner 与通知树。
- 不持久化用户的启停意图。
- 不合并三个源 owner。

## 3. 目标结构

```text
RPC / facade
    │ patch
    ▼
源 owner（各域一个，互不等待）
    │ ① check(候选)        ──▶  候选校验服务（并发，不经过对账器）
    │ ② 原子写入并发布快照               │ 本地控制面 -t
    │ ③ effects.<域>_committed
    │ ④ runtime.committed(域, 版本)
    ▼
facade 在状态 watch 上等待“已考虑到该版本”，组装 MutationOutcome

对账器（单一 actor，唯一改变内核的入口）
    触发：源已提交 · 启动 · 服务就绪变化 · 内核状态变化 · 显式启动/重试 · 生命周期命令 · 重同步节拍
    一轮：结清待决动作 → 观察 → 期望 → plan()（纯函数）→ 执行 → 记录并发布状态
```

### 3.1 四条规则

1. 源配置是唯一的期望状态。
2. 提交前的校验不提交源配置、不改变运行中的内核、不确认端口绑定、不发布产物，并且不经过对账器的邮箱。
3. 运行中的内核只由对账器改变。
4. 不一致是状态。对账器每轮都从当前事实出发，不依赖上一轮的中间结果。

### 3.2 候选校验服务

位置 `AW/check.rs`（R5c 改名后为 `C/reconciler/check.rs`）。它不是 actor：没有可变状态，没有生命周期。它持有三个域的只读快照句柄、构建端口、端口解析器、共享的修订分配器与本地端点。

```rust
/// `S/mutation/ports.rs`，由源 owner 消费。
#[async_trait::async_trait]
pub(crate) trait RuntimeGate: Send + Sync + 'static {
    /// 构建候选并让内核验证。失败即拒绝提交。
    async fn check(&self, candidate: RuntimeCandidate) -> Result<(), Arc<RuntimeError>>;
    /// 提交后的通知。尽力而为，不返回结果。
    fn committed(&self, domain: ConfigDomain, version: Version);
}

pub(crate) struct RuntimeCandidate {
    pub change: DomainChange,
    /// 尚未落盘的 profile 文件内容，按受管路径覆盖捕获结果。
    pub staged: BTreeMap<String, String>,
}
```

`check` 的步骤：

1. 候选域取候选值，其余两个域取当时的已提交快照；捕获 profile 内容并用 `staged` 覆盖。
2. 严格模式构建（今天的 `prepare_candidate_inputs`）。失败返回 `BuildRuntime`。
3. 解析内核二进制。失败返回 `ResolveCoreBinary`。
4. 在本地端点上 `check_config`，内联传入文档，等待它的真实结果。应用层不再套等待上限；进程适配器自己的 30 秒限时保留，它超时的结果仍归为“无法得到结论”。内核拒绝返回 `CoreRejectedConfig`；无法得到结论返回 `CheckUnavailable`。
5. 通过后把 `(目标身份, 已构建候选)` 投递给对账器（`Message::Checked`），供应用时复用同一份字节。这只是缓存：候选只有在对账器自己算出的目标身份与它相同时才会被使用。

校验必须给出结论才能提交。这保证“每个已提交的、影响运行时的状态在提交那一刻通过过校验”。校验不可用时保存被拒绝，错误可重试，没有任何东西被写入。

并发：

- 两个域同时校验时，各自以当时已提交的另一域为准。组合才无效的情况不会被校验发现，它表现为应用失败后的“未生效”。
- 候选的端口解析只做临时的绑定探测，不确认任何绑定。校验通过不保证稍后绑定成功。
- 用户脚本可以有自己的副作用（Lua 的标准库、HTTP 模块）。这与今天每次 Try 都运行脚本相同。
- 并发构建共享脚本模块缓存。R1c 把两个 shell 的缓存写入改为原子替换，读者只会看到旧值或新值。

### 3.3 源 owner 的提交序列

```rust
state.mutations.ensure_ready()?;
validate(&next)?;
let reaches = impact::reaches_runtime(&current, &next, &touched);   // 纯函数
if reaches {
    gate.check(RuntimeCandidate { change, staged }).await.map_err(refused)?;
}
let result = manager.replace_if_version[_with_local_write](expected_version, next, ..).await;
// Replaced:
let committed_version = manager.snapshot_handle().load().version;
effects.<domain>_committed(..);
if reaches { gate.committed(domain, committed_version) }
Ok(Snapshot { version: committed_version, reaches_runtime: reaches, .. })
```

- `expected_version` 是 CAS 的输入；通知与返回用的是写入之后读到的 `committed_version`。两者都是状态引擎的版本，三个域一致；Profiles 自己的持久修订号是另一个计数器，不用于等待。owner 是该域唯一的写入者，所以写入之后读到的就是这次提交的版本。
- 引擎在 `replace_if_version*` 返回之前已经发布了快照（`E/coordinator.rs:410`），所以 `committed` 发出时对账器一定能读到这个版本。
- 写失败时内核没有被碰过，直接返回写入错误。
- `reaches_runtime` 只看前后值与被修改的文件：三个域的投影不同，或当前闭包内的文件被写入。它不看请求里出现了哪些字段，所以同值重提不再触发运行时工作。被写入的闭包文件即使内容相同也算（与今天一致）；对账器随后按内容算出相同的目标身份，不会重复应用。
- 是否属于“显式切换”不再影响提交路径。

#### Profiles 域

带文件资源的提交（保存文件、刷新、导入、镜像）顺序为：取得候选字节 → 校验 → 准备资源与清理 → 本地写里替换文件并写入 `profiles.yaml` → 发布 → 完成与延后清理。校验在资源准备之前，所以校验被拒绝时没有暂存物需要清理。提交之后的物化降级照旧报告。

文件替换先于文档写入，这段时间里磁盘上是“未提交的新字节”。为了不让别的校验或对账轮次读到它们，引入一把只包住文件读写的锁（`ProfileContentLock`，`tokio::sync::RwLock<()>`，由组合根注入）：

- Profiles owner 在“本地写 + 文档写与快照发布 + 失败时的补偿”期间持有写锁。任何替换或补偿受管文件的路径都在写锁内，包括物化恢复。
- 构建输入的捕获在一次读锁内完成两件事：读取已提交的 Profiles 快照（文档与版本），读取它引用的文件。这样拿到的文档与字节属于同一次提交。

锁只跨文件 IO 与文档写入，不跨校验、不跨应用。owner 自己的校验发生在取锁之前，所以没有环。

这把锁只保证构建输入。面向用户的文件读取（`C/mod.rs` 的读文件接口）不经过它，与今天一样。

外部符号链接的文件不受提交约束：文件在 owner 处理变化之前就已经变了，拒绝这次变化也恢复不了它，之后的每次构建都会读到它。这与今天相同，不在本设计内解决。

没有调用方的提交（计划内刷新、外部文件变化）同样由 owner 自己发出 `committed`。

### 3.4 facade 的结果

```rust
let committed = owner.patch(patch).await?;
let runtime = if committed.reaches_runtime {
    self.inner.reconciler.settled(domain, committed.version).await
} else {
    Settled::Unchanged
};
Ok(outcome(committed, runtime))
```

`settled` 在状态 watch 上等待一个电平：对账器最近一个结论所依据的该域版本不小于刚提交的版本，并且对账器空闲。实现是一个“读当前值、不满足则等 `changed()`”的循环，没有等待者队列，也没有超时。它不是逐请求的通道；调用方离开不影响任何事。

等待总会结束：

- 一轮的结论发布时带着这一轮开始时读取到的三域版本，不是结束时重新读的。
- 因待决动作未结清或正在关闭而提前结束的轮次，同样发布当时读取到的版本和相应的结论。
- watch 关闭（对账器已停止）时等待以“对账器已关闭”结束。

等待只发生在响应路径上。提交本身在 `owner.patch` 返回时已经完成，其他域的提交不受影响。

| 对账器在该版本上的结论     | `CommitReceipt.runtime` | 降级项                                                     |
| -------------------------- | ----------------------- | ---------------------------------------------------------- |
| 已生效                     | `Applied`               | 应用附带的降级（断连失败、产物发布失败、旧 daemon 未释放） |
| 用户已停止内核             | `SavedInactive`         | 无                                                         |
| 未生效，等待依赖或已被拒绝 | `Deferred`              | `RuntimeDeferred { cause }`                                |
| 结果未知                   | `RecoveryRequired`      | `RuntimeRecoveryRequired { cause }`                        |
| 对账器已关闭               | `RecoveryRequired`      | 同上，原因是关闭                                           |

最后两行在线缆卡（R5a）里并入 `Deferred`。

`CommitReceipt.operation_id` 是 owner 为这次源提交生成的标识，只在提交影响运行时时存在（与今天相同）。对账轮次有自己的标识；`ConfigurationStatus.recent_operations` 改为列出最近的轮次（触发、结论、原因），不再列出逐次变更。

### 3.5 对账器

对账器由 `ApplicationWorkflowActor` 原位演进而来。消息：

- 带回复的命令：`Startup`、`Start`（今天的 `Reconcile`）、`Retry`、`StopCore`、`ReplaceCoreBinary`、五个服务操作、`PrepareService`、`InterruptConnections`。
- 触发（cast）：`SourceCommitted { domain, version }`、`Checked { target, prepared }`、`ServiceReadiness`、`CoreChanged`、`Tick`。

每条消息的处理结束前都运行一轮 `reconcile(trigger)`，然后发布状态。邮箱是唯一的队列。

#### 记录

```rust
struct Record {
    /// 上次确认生效的应用：今天的 `RuntimeSnapshotStore::confirmed`（回执带目标身份）。
    applied: Option<ConfirmedRuntime>,
    /// 最近一次提交给内核的尝试。
    attempt: Option<Attempt>,
    /// 校验时构建好的候选，目标身份相同时复用。
    checked: Option<(TargetKey, PreparedRuntime)>,
    /// 启停意图。初值 Running；显式停止设为 Stopped；显式启动与欠下重启的二进制替换设为 Running。
    run: RunIntent,
    ownership: Ownership,
    service_failed: Option<u64>,
    /// 上次生效的 mode 与 `Profiles.current`，用于决定是否断连。
    applied_selection: Option<Selection>,
    /// 上一轮看到的服务模式偏好，用于发现“由开转关”。
    service_preferred: bool,
    release_owed: bool,
    pending_product: Option<(Arc<RuntimeSnapshot>, String)>,
    pending_release: Option<String>,
}

struct Attempt { target: TargetKey, host: ExecutionHost, outcome: AttemptOutcome }
enum AttemptOutcome { Applied, Refused(Arc<RuntimeError>), Unknown(Arc<RuntimeError>) }
```

#### 一轮

```text
1. 读取三域快照及其引擎版本（这一轮的结论以它们为准）。
2. 已关闭：发布结论“已关闭”，返回。
3. 有待决动作：consume_settled_action()；仍未完成则发布结论 Unknown，返回。
4. 观察：
   a. 所有权已建立在期望宿主上、且端点连通时走快路径：只刷新内核状态，服务用最近发布的相位。
   b. 否则：服务命令是否已结清；探测服务；刷新内核状态（今天的 S1）。
5. 期望：run、host = effective_host(偏好, 服务可用性, 当前宿主)、三域快照与捕获内容 → 目标身份。
6. step = plan(facts)。执行 step；step 改变了事实（移交、停止、服务回落）就回到 4，否则结束。
7. 维护：产物补发、daemon 释放（见“维护项”）。
8. 发布状态：第 1 步的版本、结论、降级项。
```

#### 回执核对

核对通过时，对账器把这次应用重新记为已确认并关闭端口过渡（今天 S3 的 `Accepted` 分支），端口可用性因此发生变化时 `notify_bound`。结果未知之后旧实例其实没有变的情况由它恢复。

内核在运行时，“它运行的是不是上次确认生效的那次应用”由回执与观察核对：宿主相同，观察到的内核修订（epoch、generation、effective_hash）与 `source_hash` 等于回执里的修订，且观察到的运行 epoch 等于回执的 epoch。路由代数不参与比较：连接断开后重新接上同一个 daemon 会推进路由代数，而内核进程没有变；core-manager 回滚后的修订标识也与尝试之前相同。

#### 规划

`plan` 是纯函数。输入是这一轮的事实，输出是下一步。

```rust
struct Facts {
    trigger: Trigger,
    run: RunIntent,
    desired_host: ExecutionHost,
    owner: ExecutionHost,
    ownership: Ownership,
    service: Option<ServiceEvidence>,      // 快路径为 None
    core: CoreFact,                        // Stopped { failed } | Running { verified } | Transitional | Unreadable
    target: TargetKey,
    applied: Option<TargetKey>,            // 仅当回执核对通过
    attempt: Option<(TargetKey, ExecutionHost, AttemptKind)>,
}

enum Step {
    Idle(Conclusion),                      // Converged | Inactive | NotInEffect(cause) | Waiting(cause)
    Owner(OwnerPlan),                      // 今天的 AdoptService / HandBackToLocal / RetireServiceThenLocal
    FallBackToLocal,                       // 把这一代就绪的服务记为失败，期望宿主改为本地
    Stop { intent: bool },                 // 满足停止意图，或停掉没有回执的实例
    Apply,
}
```

`forced` 指触发是显式启动、显式重试，或替换二进制后欠下的重启。判定顺序：

| #   | 条件                                                                                       | 下一步                                                                                            |
| --- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| 1   | 所有权未建立在期望宿主上                                                                   | 按 `plan_owner`：`Owned` 继续；`WaitForService`、`Unproven` → `Idle(Waiting)`；其余 → `Owner(..)` |
| 2   | 内核状态处于过渡态或读不到                                                                 | `Idle(Waiting)`                                                                                   |
| 3   | `run == Stopped`，内核在运行                                                               | `Stop { intent: true }`                                                                           |
| 4   | `run == Stopped`，内核已停止                                                               | `Idle(Inactive)`                                                                                  |
| 5   | 内核在运行，回执核对不通过                                                                 | `Stop { intent: false }`                                                                          |
| 6   | 内核在运行，`applied == target`，非 `forced`                                               | `Idle(Converged)`                                                                                 |
| 7   | 所有者是服务宿主，内核已停止，上次尝试是本目标在服务宿主上，并且它被拒绝或内核带着失败原因 | `FallBackToLocal`                                                                                 |
| 8   | 上次尝试是本目标本宿主，并且（它被拒绝，或内核带着失败原因停止），并且非 `forced`          | `Idle(NotInEffect)`                                                                               |
| 9   | 其余                                                                                       | `Apply`                                                                                           |

- 第 1 行在第 2 行之前：所有权的移交只需要它自己的证据。服务端点已断开、而系统确认 daemon 已停止时，内核状态读不到，但把所有权交还本地是安全的；交还之后重新观察本地。读不到状态只禁止启动与应用。
- 第 5 行沿用今天 S3 的做法：对没有回执的运行实例先停止再从停止状态应用。
- 第 7 行是今天的服务回落（#5443）：服务宿主没能把内核跑起来时，这一代就绪的服务不再使用，内核改在本地运行；daemon 再次就绪是新的一代。应用被拒绝但旧配置仍在服务宿主上运行时不回落，落到第 8 行。今天的规则更宽：启动之后只要观察到服务宿主上的内核停止就回落；这里收窄为“同一目标在服务宿主上已经失败过”，差别列在第 4 节。
- 第 8 行让失败的目标停下来，也防止内核反复崩溃时对账器反复启动它。
- 对账轮次从不安装或启动 daemon：`Owner(..)` 的三种动作都只接管已经就绪的服务（今天的规则不变）。

#### 应用与发布

只有一套顺序：

```text
取候选：record.checked 的目标身份等于期望目标则复用，否则以非严格模式构建
需要断连时，在提交前取得旧实例的 API 句柄
runtime.begin_transition()
core.reconcile(intent, spec, 刚观察到的状态)
```

应用本身不再做独立的 `-t` 校验：校验是提交的闸门，`reconcile` 在事务内自己验证。

| 结果                                                                                                       | 记录                                                                            | 端口与产物                                                                                                    | 结论          |
| ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------- |
| `Reconciled`                                                                                               | `applied`、`attempt = Applied`、`applied_selection`                             | 记录确认的应用；发布产物（失败记为 `pending_product`）；`accept_transition`；按策略断连；`notify_bound(true)` | `Converged`   |
| `RolledBack`                                                                                               | `attempt = Refused`                                                             | `accept_transition`；旧回执仍然有效                                                                           | `NotInEffect` |
| `Unchanged`，或提交之前的构建、二进制错误                                                                  | `attempt = Refused`                                                             | `accept_transition`                                                                                           | `NotInEffect` |
| `NotSubmitted`，原因是事实在观察与提交之间变了或依赖暂不可用（修订冲突、所有者变了、正在移交、端点不可用） | 不记录                                                                          | `accept_transition`                                                                                           | `Waiting`     |
| `NotSubmitted`，其余原因                                                                                   | `attempt = Refused`                                                             | `accept_transition`                                                                                           | `NotInEffect` |
| `Unknown`                                                                                                  | `attempt = Unknown`；若上一次尝试是同一目标同一宿主的 `Unknown`，记为 `Refused` | `ports.invalidate()`；过渡保持打开                                                                            | `Unknown`     |
| 停止成功                                                                                                   | 不变                                                                            | `ports.invalidate()`                                                                                          | 回到观察      |
| 宿主移交完成                                                                                               | `ownership`                                                                     | `ports.invalidate()`（今天的 `follow_host`）                                                                  | 回到观察      |

`NotSubmitted` 归为 `Waiting` 的错误种类只有四个：`RevisionConflict`、`OperationConflict`、`QueueFull`、`BackendUnavailable`。其余种类（包括 `Internal` 与没有种类的错误）都记为 `Refused`。现有的 `not_submitted_cause` 把无法归类的错误当作可重试，不沿用。

“连续两次未知”数的是两次真实的提交：第一次的待决动作结清之后，对账器又提交了一次，结果仍然未知。结清动作本身不产生应用结果。

#### 维护项

每轮结束前检查，与这一轮有没有应用无关：

- 产物补发：`pending_product` 仍对应当前确认的应用时重发，否则丢弃。只在显式重试的轮次里做。
- daemon 释放：对账器看到服务模式偏好由开转关时记下 `release_owed`。偏好仍为关、所有权已证明在本地、daemon 在运行并且服务于本实例时，停止它；失败记为 `pending_release`，显式重试时再做。偏好又被打开时清除两者。

#### 触发

| 触发               | 来源                                                                                 | 说明                                                                                                                                   |
| ------------------ | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `Startup`          | 命令                                                                                 | 首轮；无论结论如何，向每个 owner 发布一次完整期望值（今天的 `publish_full`）；返回 `StartupReport`。再次调用返回第一次的报告，不再运行 |
| `SourceCommitted`  | owner 的 `committed`                                                                 |                                                                                                                                        |
| `ServiceReadiness` | `ServiceClient` 的 watch                                                             | 今天已有                                                                                                                               |
| `CoreChanged`      | `CoreObserver` 的 watch，按 `(host, generation, connectivity, state, revision)` 去重 | 新增                                                                                                                                   |
| `Start`            | 命令                                                                                 | 把 `run` 设为 Running；`forced`                                                                                                        |
| `Retry`            | 命令                                                                                 | `forced`；重试产物发布与 daemon 释放                                                                                                   |
| `BinaryReplaced`   | `ReplaceCoreBinary` 命令在安装完成后                                                 | 欠下重启时为 `forced`                                                                                                                  |
| `Tick`             | 现有的 5 秒节拍                                                                      | 服务端点恢复（不改）；上一轮结论是 `Waiting` 或 `Unknown` 时再跑一轮                                                                   |

`Tick` 做两件互不相干的事：

- 服务端点恢复：照旧独立判断是否到期（端点处于 `Degraded { desired: Service }`、未被抑制、次数未用完、间隔已过）。它的计数与暂停只覆盖重连这一段。重连成功后无论上一轮结论是什么都跑一轮。
- 重同步：上一轮结论是 `Waiting` 或 `Unknown` 时再跑一轮。它不携带状态，不退避，也不计数。结论是 `NotInEffect` 时不会因为节拍而重新应用配置。

`ReplaceCoreBinary` 的停止、隔离恢复、安装三步不变（`CL/workflow.rs:741-804`）。重启是否欠下的判断也不变：期望内核就是被替换的那个，或者安装前不得不停掉它。欠下重启时把 `run` 设为 Running（今天替换选中的内核同样会启动它），通知进度“正在重启”，再跑一轮。

#### 状态

```rust
pub struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<OperationId>,
    pub seen: EngineVersions,              // 最近一个结论所依据的三域引擎版本
    pub conclusion: Conclusion,
    pub degradations: Vec<Degradation>,    // 最近一轮应用附带的降级
    pub maintenance: Option<String>,
    pub recent: VecDeque<RoundRecord>,     // 调试页用，长度 32
}
```

`ConfigurationStatus.runtime` 由它投影：`Converged`、`Inactive` → `Healthy`；轮次进行中 → `Pending`；`Waiting` → `WaitingDependency`；`NotInEffect` → `Blocked`；`Unknown` → `RecoveryRequired`（R5a 之后为 `WaitingDependency`）。`attempts` 与 `automatic_remaining` 恒为 0，R5a 删除。

### 3.6 服务模式开关

今天打开服务模式时，daemon 的安装与启动发生在 Try 内部，排在构建与校验之后；提权被拒绝则整个请求被拒绝。

改为 facade 的 `set_service_mode(enabled)`：

- 打开：先向对账器发 `PrepareService`（安装并启动 daemon；成功时清除 `service_failed`）。失败（包括用户拒绝提权）直接返回错误，不提交。成功后提交偏好。对账器在下一轮把内核移到服务宿主。
- 关闭：直接提交偏好。对账器把内核移回本地，然后释放 daemon。

`PrepareService` 成功之后，偏好的提交仍可能失败（校验被拒绝、写入失败）。这时 daemon 已安装并在运行，偏好仍为关闭，调用方得到的是提交的错误。不做补偿：已就绪的 daemon 不影响以本地方式运行的内核。

RPC `set_service_mode` 改为调用这个 facade 方法。

### 3.7 断连

对账器比较上次生效与本次生效的 mode 与 `Profiles.current`。mode 变了且 `on_mode_change`，或选择变了且 `on_profile_change`，就在应用成功且实例没有被替换时关闭旧实例的连接。应用重启后的第一次应用没有“上次”，不断连。关闭失败是降级项，不重放；`applied_selection` 的更新不取决于关闭是否成功。

重复设置当前 mode 时没有新值可应用，但今天会关闭连接。这个行为保留在共享的 facade 入口 `patch_runtime_overrides`（RPC 与热键都经过它）：patch 只包含 mode、且它等于已提交的 mode 时，不提交，向对账器发 `InterruptConnections`，按 `on_mode_change` 关闭一次，关闭失败作为降级项返回。

patch 同时带有未变的 mode 与其他变化时，它是一次普通的修改：mode 没有变，不断连。

## 4. 行为变化

| 场景                                                               | 今天                                         | 之后                                                                                 |
| ------------------------------------------------------------------ | -------------------------------------------- | ------------------------------------------------------------------------------------ |
| 候选构建失败，或被内核判为无效                                     | 拒绝，不提交                                 | 相同                                                                                 |
| 通过校验但真实应用失败的显式切换（切 profile、切内核、开服务模式） | 拒绝，不提交                                 | 已提交；内核留在旧配置；结果为 `Deferred` 并带原因；显式重试或改回                   |
| 写文件失败                                                         | 运行时已被改动，再恢复                       | 运行时未被触碰                                                                       |
| 应用结果未知                                                       | 隔离：拒绝一切影响运行时的变更，直到显式恢复 | 保存不受影响；对账器在下一个节拍结清动作并继续                                       |
| 内核在运行但没有本会话回执                                         | 拒绝保存（`NoRestorableBaseline`）           | 照常提交；对账器先停掉该实例再应用                                                   |
| 连接断开后重新接上、内核进程未变                                   | 之后影响运行时的保存被拒绝（同上一行）       | 回执仍然有效，不重启                                                                 |
| 内核处于过渡态时保存                                               | 拒绝保存（`CoreTransitioning`）              | 照常提交；状态稳定后应用                                                             |
| 内核意外停止后保存影响运行时的配置                                 | 保存但不启动                                 | 新目标会启动内核；同一目标不会被反复启动                                             |
| 显式启动失败之后                                                   | 之前的停止意图仍然有效                       | 意图已是“运行”；之后的新目标会启动内核                                               |
| 应用失败后的自动重试                                               | 1、5、30 秒三次                              | 不自动重试；显式重试、新目标或宿主变化才再次尝试                                     |
| 等待依赖                                                           | 5、5、10、30、60 秒退避                      | 依赖变化的事件立即触发；否则每 5 秒重看一次                                          |
| 连续多次保存                                                       | 每次完整的构建、校验、应用                   | 校验各做一次；应用合并                                                               |
| 两个域同时保存                                                     | 后者等前者的内核操作结束                     | 互不等待                                                                             |
| 校验在哪里执行                                                     | 拥有内核的宿主；服务托管时在 daemon 内       | 总在本地，使用应用解析到的内核二进制；应用层不再限时 45 秒                           |
| 把运行时带到已提交配置的路径（启动、重试、服务跟随）               | 应用前做一次独立校验                         | 不做独立校验；`reconcile` 事务内的校验仍在                                           |
| 同值重提                                                           | 触发一次完整应用                             | 不触发运行时工作                                                                     |
| 重复设置当前 mode                                                  | 应用一次（无变化）并关闭连接                 | 只关闭连接                                                                           |
| patch 同时带未变的 mode 与其他变化                                 | 应用后关闭连接                               | 应用，不关闭连接                                                                     |
| 服务宿主上的内核无故停止（没有失败原因），或停止后目标变了         | 回落到本地                                   | 先在服务宿主上启动一次；失败后才回落                                                 |
| 打开服务模式时拒绝提权                                             | 拒绝，不提交                                 | 相同                                                                                 |
| 打开服务模式，daemon 就绪后提交失败                                | 不会发生（校验先于安装）                     | daemon 留在已安装并运行的状态，偏好未提交，返回提交错误                              |
| `Reconcile` 命令在所有者已证明时                                   | 不校验，先写产物再提交给内核                 | 与其他路径同一套顺序                                                                 |
| 关闭时已提交、尚未应用的修改                                       | 不存在（关闭时拒绝整个请求）                 | 已提交；结果为 `RecoveryRequired`（R5a 后为 `Deferred`），原因是关闭；下次启动时应用 |
| 调试页的最近操作                                                   | 逐次变更的回执                               | 最近的对账轮次                                                                       |

## 5. 迁移

原位演进，每个提交都可编译、测试通过。顺序与理由：

1. PR-1 前提：构建不再需要独占访问；校验改走本地控制面并去掉应用层的等待上限；脚本模块缓存原子写入。
2. PR-2 提交优先：先把“已提交目标的应用”与独立校验分开；然后逐域切换（Application、Clash、Profiles 各一个提交），切换期间 workflow 同时接受旧的 `BeginMutation` 与新的 `SourceCommitted`；全部切完后删除参与者路径和它的全部消费者，并同步规范文档。
3. PR-3 单一循环：先把两套应用引擎并成一套，并按 3.5 的矩阵发布；再用纯函数 `plan` 与轮次取代延期目标、预算、定时器、隔离与 `LiveAttempt`；最后加入内核状态触发。
4. PR-4 收缩状态引擎：删除没有消费者的参与者与决定机制。
5. PR-5 线缆、前端文案、剩余的规范文档与模块改名。

PR-2 的中间状态规则：

- 新旧两条路径应用成功时都更新 `applied_selection`。
- 一轮只发布它实际读取并使用的版本；旧路径的 `BeginMutation` 不更新 `seen`。
- `Checked` 只是缓存，不是提交的证明。
- Profiles 域切换之前，它的文件替换发生在自己的 `BeginMutation` 处理期间，那段时间 workflow 的邮箱被占着，对账轮次读不到未提交的字节。别的域的校验可能读到，结论因此可能偏差；这属于 3.2 已经接受的“以当时所见为准”。内容锁随 Profiles 域的切换（R2d）引入。物化恢复（`S/profiles/actor.rs:2552`）是例外：它今天就不在任何 Try 的保护之内，同样在 R2d 纳入写锁。
- 已切换的域在 workflow 被隔离时仍然可以提交；对账在恢复之后进行。这与最终行为一致，只是恢复仍需显式触发，直到 PR-3。

## 6. 测试

- 纯函数：`plan` 用表驱动测试覆盖判定表的每一行，并覆盖触发种类与上次尝试的组合。`reaches_runtime` 沿用 `impact.rs` 现有的表。
- owner：用 `RuntimeGate` 的替身断言顺序（校验、写入、通知）与三种失败（校验拒绝时文件未变、写失败时没有通知、不影响运行时的保存不校验）。
- 对账器：沿用现有夹具（真实的 `CoreClient` 与路由，`TestControlEndpoint` 作为端点，`FakeDaemon`）。启动、服务恢复与断连策略三组现有场景测试改为断言可观察结果（状态、端点收到的调用顺序），不再断言回执与延期目标。
- facade：第 4 节每一行至少一个测试。其中“一个域的应用被挂起时，另一个域的提交仍然完成”直接对应用户的约束。
- 确定性的并发测试：文件替换之后、文档写入失败之前发起的内容捕获，读到的是补偿之后的旧内容（用闸门挂住本地写，不用 sleep）。
- 替身的局限写在测试旁：`TestControlEndpoint` 只拒绝过期的 `expected_applied`，不验证文档，也不要求先校验再应用。

## 7. 风险与未验证的前提

- 本地校验与 daemon 内应用使用的内核二进制与数据目录是否总是相同，没有验证。两者不同时校验结论可能与应用结果不一致，表现为“未生效”。
- `-t` 对启用 TUN 的配置是否需要特权，没有验证。
- 校验验证的是源文档，`reconcile` 事务内验证的是改写了控制器与本地 IPC 设置之后的文档。两者可能不一致，同样表现为“未生效”。
- 从停止状态启动失败在 `CoreFacade` 里通常被归为 `Unknown`（推断，没有读到覆盖它的测试）。本设计下它在下一个节拍被结清，再由判定表第 7、8 行处理。
- 待决动作永远结不清时（1.4 的第二处缺口），对账器停在 `Unknown`，保存仍可提交但不会应用，直到应用重启。今天同样需要重启，区别是今天连保存也被拒绝。
- 服务模式的移交、daemon 崩溃恢复只有替身测试，没有真机验证。
- 通过校验却应用失败的实际频率没有数据。

## 8. 不在范围

- `A2/` 三层是否还能合并、待决动作在观察优先之后是否仍然必要：需要对照 core-manager 的操作存储单独调研。
- 服务端点恢复是否还需要自己的次数上限与自动启动 daemon：服务回落（#5443）之后，daemon 停止时内核已经会移到本地继续运行，这部分可能是多余的，需要单独评估。
- 持久化启停意图；给停止内核增加 UI 入口。
- 外部符号链接文件的内容不受提交约束。
- 修 1.4 的两处缺口。
