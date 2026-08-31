# PR-5 — CoreActor 迁移简化设计

**日期：** 2026-08-01  
**目标：** 迁移核心生命周期所有权，但不在 `clash-nyanpasu` 中重做一遍 `nyanpasu-runtime` 已经具备的 manager、状态机、恢复、配置应用与回滚能力。

## 1. 核心结论

PR-5 只新增一层 **应用事务协调**：

1. `CoreActor` 负责 GUI 进程内的所有权、运行后端选择、Promoted / Applied 状态和跨步骤事务排他；
2. `nyanpasu-core-manager` / `nyanpasu-service` 继续负责单次核心操作的内部串行、进程监督、健康检查、apply 分类与回滚；
3. 使用一个取消安全的 `OperationId` 协议，替代 GUI 侧的 `rebuild_gate`、`clash_patch_gate` 和传统生命周期 mutex；
4. 不新增 `CoreEngine` trait、`CoreEngineFactory`、`EngineStatus`、`EngineRevision`、`ApplyReport`、完整 `EngineError` 镜像；
5. 不在 actor 上增加第二层自动恢复；
6. 所有运行配置变更统一调用 runtime 的 full-config `apply_config`，不再在 GUI 侧先直接 PATCH Clash API；
7. 换核作为普通 desired-state mutation：desired 提交成功后，若 runtime 回滚，则保留 desired 新核并返回 `CommittedDegraded`，不执行第二套应用层回滚事务。

## 2. 两层串行模型

只保留两层互斥：

| 层             | 保护对象                                                                             | 实现                                                                                |
| -------------- | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| 应用事务层     | snapshot → build → check → promote → apply/start，以及换核、运行模式切换等跨组件操作 | `CoreActor` 的 `OperationId`                                                        |
| runtime 内部层 | epoch、active instance、runtime copy、quarantine、进程切换等 manager 内部不变式      | `nyanpasu-core-manager` 自身的 `ctrl` mutex；service 保留自身 closing/control latch |

`OperationId` **不替代** runtime 内部 mutex，也不替代 runtime directory ownership lock。它只替代 `clash-nyanpasu` 为跨多个 actor/RPC/文件步骤而设置的应用层锁。

迁移完成后删除：

- `NyanpasuClientInner::rebuild_gate`；
- `NyanpasuClientInner::clash_patch_gate`；
- legacy `CoreManager::lifecycle_lock`；
- GUI 侧 API-first patch 的补偿 mutex/fence 实现。

配置 actor 的普通 commit 不受 operation gate 阻塞。每次 rebuild 在读取 typed snapshots **之前**取得 `CoreOperationGuard`；构建期间发生的新 commit 由现有 dirty/coalesce 机制触发下一次 rebuild。

## 3. 取消安全的 OperationId

命名约定：

- `OperationId`：actor 用于校验和 fencing 的一次性操作身份；
- `CoreOperationGuard`：调用侧持有的 RAII guard；
- `OperationGate`：actor 内部的 active + FIFO waiters 状态；
- `OperationId` 不表示核心进程实例，也不表示登录、网络或端口 session；它没有 TTL、续租和 heartbeat 语义。
- 迁移期间仅为兼容既有 seam，`CoreOperationGuard` 可以暂时实现旧名 `CoreLifecycleLease`；新类型与新消息不得继续使用 Lease 命名。

### 3.1 为什么 OperationId 由 client 预分配

actor 在 `AcquireOperation` handler 中分配 ID、再通过 RPC 返回给调用方，会有取消窗口：actor 已经登记 active operation，但调用 future 在收到 ID 前被取消，此时调用方没有 guard 可执行 `Drop`。

因此 ID 必须由共享的 `CoreClient` 在发送 `AcquireOperation` 前预分配，并先构造 pending guard。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(NonZeroU64);

#[derive(Clone)]
pub struct CoreClient {
    actor: ActorRef<CoreActorMessage>,
    next_operation: Arc<AtomicU64>,
    snapshot: watch::Receiver<CoreSnapshot>,
}

pub struct CoreOperationGuard {
    id: OperationId,
    client: CoreClient,
    acquired: bool,
}
```

`CoreClient` 的 clones 共享同一个 `AtomicU64`。`0` 保留为无效值；溢出视为进程生命周期内不可恢复的内部错误，不做持久化或跨进程协议。

### 3.2 actor 状态

```rust
struct OperationGate {
    active: Option<ActiveOperation>,
    waiters: VecDeque<OperationWaiter>,
}

struct ActiveOperation {
    id: OperationId,
    acquired_at: Instant,
}

struct OperationWaiter {
    id: OperationId,
    reply: RpcReplyPort<Result<(), OperationError>>,
}
```

只需要两个控制消息：

```rust
enum CoreActorMessage {
    AcquireOperation {
        id: OperationId,
        reply: RpcReplyPort<Result<(), OperationError>>,
    },
    ReleaseOperation {
        id: OperationId,
    },

    CheckAndPromote { operation: OperationId, request: PromoteRequest, reply: ... },
    ApplyPromoted   { operation: OperationId, request: ApplyRequest, reply: ... },
    StartPromoted   { operation: OperationId, reply: ... },
    Restart         { operation: OperationId, reply: ... },
    Stop            { operation: OperationId, reply: ... },
    SetBackend      { operation: OperationId, mode: RunMode, reply: ... },
    Recover         { operation: OperationId, reply: ... },
    Shutdown        { reply: RpcReplyPort<()> },

    BackendStatus(CoreStatusView),
    BackendLog(Arc<LogFrame>),
}
```

### 3.3 AcquireOperation / ReleaseOperation 规则

- 无 active operation：立即登记为 active，并回复成功；
- 已有 active operation：将 waiter 放入 FIFO；handler 立即返回，绝不等待释放；
- `ReleaseOperation(active_id)`：清除 active，并从 FIFO 中寻找下一个仍有接收方的 waiter；
- `ReleaseOperation(waiting_id)`：从 waiters 删除，等价于取消等待；
- 过期或未知 ID：幂等 no-op，并写 debug 日志；
- mutation 消息的 ID 与 active 不一致：返回 `StaleOperation`；
- shutdown：拒绝全部 waiters，清空 active，再关闭 backend。

`CoreOperationGuard` 在调用 `AcquireOperation` 前已经存在：

```rust
async fn begin_operation(&self) -> Result<CoreOperationGuard> {
    let mut operation =
        CoreOperationGuard::pending(self.clone(), self.allocate_operation_id()?);
    operation.acquire().await?;
    Ok(operation)
}
```

若 future 在等待或刚获批时被取消，pending guard 的 `Drop` 都会发送 `ReleaseOperation { id }`。正常路径允许显式 `release().await`；`Drop` 只作为取消/early-return 兜底。发送失败仅意味着 actor 已终止，不再存在需要让给后续操作的 active operation。

不实现：

- TTL；
- auto-steal；
- watchdog self-message；
- UI degradation 定时器；
- operation 续期或心跳。

可以在 `ReleaseOperation` 时对持有时长超过阈值写 warning，但不影响所有权。

### 3.4 Operation 范围

必须持有 `CoreOperationGuard`：

- runtime build/check/promote/apply；
- start/restart/stop；
- change core；
- Local ↔ Service backend 切换；
- 与 start/stop 强关联的 macOS DNS 修改。

不需要 operation guard：

- 读取 status；
- 读取 Promoted / Applied；
- 读取日志；
- 订阅状态或日志；
- 普通 typed config commit。

## 4. CoreBackend：封闭 enum，不使用 CoreEngine trait

本项目只有两个生产后端，且两者由同一组织维护，使用封闭 enum 比 trait + factory + error/status 镜像更直接：

```rust
enum CoreBackend {
    Local(LocalBackend),
    Service(ServiceBackend),
    #[cfg(test)]
    Test(TestBackend),
}
```

```rust
impl CoreBackend {
    async fn check(&self, request: &CoreRequest) -> Result<()>;
    async fn start(&self, request: &CoreRequest) -> Result<()>;
    async fn apply(&self, request: &CoreRequest) -> Result<CoreApplyData>;
    async fn stop(&self) -> Result<()>;
    async fn restart(&self) -> Result<()>;
    async fn recover(&self) -> Result<()>;
}
```

- Local：直接调用 `nyanpasu_core_manager::CoreManager`；
- Service：直接调用实例化的 `nyanpasu_ipc::client::Client`；禁止 `service_default()`；
- 测试：使用 `Test` variant，而不是为两个固定实现引入完整动态 trait 层；
- mode 切换通过 `SetBackend` 在 operation guard 下执行，不需要 `CoreEngineFactory` 或 `pending_run_type`。调用方需要切换时先正常排队取得 `CoreOperationGuard`。

### 4.1 复用 runtime 类型

不在 app 侧复制以下类型：

- `EngineStatus`：使用 manager status / IPC `CoreInfos` 的最小 UI 投影；
- `ApplyReport`：统一使用 IPC 已定义的 `CoreApplyData` / `ApplyOutcomeKind`；Local 只需一个转换函数；
- `EngineRevision`：Local 使用 `RevisionId`，Service 使用 `RevisionIdInfo`，在 backend 内转换；
- 完整 `EngineError`：只保留机器可读 kind + 原始 message/source。

建议在 `nyanpasu-runtime` 做一个小型协同修改，而不是在 app 复制字符串表：

```rust
// nyanpasu-core-metadata
#[serde(rename_all = "snake_case")]
pub enum CoreErrorKind { ... }

// nyanpasu-core-manager
impl Error {
    pub fn kind(&self) -> Option<CoreErrorKind>;
}
```

IPC 继续把它序列化为现有 `error_kind` 字符串，wire 不变；client 将字符串解析回 enum。不要新建 crate，也不要新建 transport-neutral `CoreEngine` 框架。

## 5. 不增加第二层自动恢复

Local manager 已经根据 `InstanceOptions` 执行有界 Supervisor 重启与指数退避；Service daemon 使用同一 manager。CoreActor 不再配置额外 `RecoverPolicy`，也不发送 delayed `Recover{attempt}`。

actor 只做：

- 观察 backend 最终状态；
- Supervisor/daemon 最终放弃后，发布一次 `core_recovery_exhausted` degradation；
- 用户显式重试时调用 `recover` 或 `restart`；
- shutdown 时关闭 backend。

这样不会产生“manager 重试 5 次 + actor 再重试 3 轮”的不可预测恢复链。

## 6. RuntimeLifecycleState 由 CoreActor 单独拥有

既然 check/promote/apply 都在同一个 actor operation 下完成，Promoted 与 Applied 应由同一所有者维护：

```rust
struct CoreActorState {
    backend: CoreBackend,
    mode: RunMode,
    operation: OperationGate,
    runtime: RuntimeLifecycleState, // promoted + applied
    status: CoreStatusView,
    logs: VecDeque<Arc<LogFrame>>,
}
```

- `CheckAndPromote` 成功后推进 Promoted；
- apply/start 成功且采用新 revision 后推进 Applied；
- `RolledBack` 保持 Applied 不变；
- apply error 保持 Applied 不变；
- durability warning 不阻止状态推进，但返回 degraded；
- CoreClient 通过 watch snapshot 读取状态，不为读取发送 mailbox RPC。

删除 `NyanpasuClient` 中独立的 `RuntimeLifecycleStore`、`publish_promoted`、`publish_applied` 和 `restore_promoted`。

## 7. 配置应用只走 full-config apply

新的统一路径：

1. typed desired config commit；
2. 取得 `CoreOperationGuard`；
3. 读取最新 typed snapshots；
4. build candidate；
5. backend dry-run check；
6. 原子 promote product，推进 Promoted；
7. `apply_config(product, expected_revision)`；
8. 根据 `CoreApplyData.outcome` 推进 Applied 或返回 degraded；
9. 释放 operation guard。

runtime 自己选择：

- `PATCH /configs`；
- `PUT /configs`；
- same-epoch restart；
- core switch；
- rollback。

因此删除：

- `RunningConfigPatchPort`；
- `LegacyRunningConfigPatchBridge`；
- `ControllerBinding` 与 actor 内 clash-api client cache；
- `config_patch_from_mapping`；
- `clash_patch_gate`；
- GUI 侧 patch compensation plan/fence；
- PR-5 中“clash-api 随核迁移”的范围。

其余 proxies/connections/ws/tray 等直接 clash-api 消费者继续留给 PR-6。

## 8. ChangeCore 采用普通 commit-first 语义

`change_core` 不再维护专用的五分支应用层回滚事务：

1. `ApplicationClient::patch(core = new_core)`；
2. 走统一 build/check/promote/apply；
3. runtime 若成功 switch：Applied 推进；
4. runtime 若返回 `RolledBack`：旧核/旧 revision 仍实际运行，desired 新核与 Promoted 新配置保留，返回 `CommittedDegraded { phase: CoreRollback }`；
5. 后续显式 retry 或下一次 rebuild 再尝试 desired 新核。

这与项目的 commit-first 规则一致，并让 Promoted / Applied 的分离真正表达 desired 与 effective 不一致。

删除：

- legacy verge draft/discard/apply；
- rollback rebuild；
- product bytes restore；
- 第二次 old-core restart；
- `ChangeCoreReport` 专用 wire；
- 仅为该 wire 增加的前端分支。

若前端需要立即展示结果，复用一个通用 `RuntimeApplyReport`，不要定义 change-core 专属结果：

```rust
pub struct RuntimeApplyReport {
    pub outcome: ApplyOutcomeKind,
    pub desired_revision: u64,
    pub applied_revision: Option<u64>,
}
```

## 9. 其他范围收敛

### 日志

CoreActor 订阅 upstream `LogFrame`，只保留 GUI 所需的 100 条内存 ring。直接复用 `nyanpasu-core-metadata::LogFrame`，不定义 `LogSink` trait。manager 的 JSONL sink 保持原样。

### Service control

install/update/uninstall/start/stop 是 service 管理，不属于 CoreActor。保留一个具体 `ServiceController`；操作完成后调用 `CoreClient::set_mode/reconcile_mode`。不引入完整 `ServiceControlPort`，除非测试确实需要替换 OS command runner。

### macOS DNS

作为 start/stop 的平台 side effect 放在 actor 内，用一个小型 `MacosDnsGuard`；非 macOS 不定义空 trait。Service 模式需要提权时由 `CoreBackend::Service` 调 IPC set_dns。

### Updater

不为了把 `CoreManager::global()` 指标强行归零而增加 `attach_core_port()` 半迁移桥。Updater 的完整注入仍由 PR-6d 完成；PR-5 允许保留一个有明确 owner/remove condition 的 residual。

## 10. 保留的安全门

- submodule/path dependency 与 sidecar release lockstep；
- `ServiceCompat` major 版本 fail-closed；
- `LocalIpcPolicy::Disable` 显式设置；
- stable release 前 bump 到正式 v2.0.0；
- TempDir 测试与 `test_real_dirs == 0`；
- Windows 旧 daemon 升级 smoke；
- macOS TUN/DNS smoke。
