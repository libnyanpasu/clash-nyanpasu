# PR-4S — PR-1～PR-4 Actor Migration 稳定化门（设计 spec）

**日期：** 2026-07-13  
**状态：** Proposed / 待逐节批准  
**范围基线：** `main @ 429045202`（包含 `#4932`）  
**上游依据：** `docs/design/actor-migration-roadmap.md` v3 §5  
**建议分支：** `fix/pr4s-actor-migration-stabilization`  
**原子性：** 本 spec 的 S01～S10 为一个稳定化 PR；可以多 commit，但不得只合并部分语义

---

## 1. 背景与问题定义

PR-1～PR-4 已完成以下主要方向：

- `NyanpasuClient` facade；
- Application / SessionState / ClashConfig typed actors；
- ProfilesActor、profile file service、scheduler/watcher；
- pure runtime executor + RuntimeBuilder；
- writable runtime global 删除；
- candidate → core check → promote → publish 管线；
- profile mutation 的初版 `RebuildOutcome`。

但当前实现仍有四类系统性缺陷：

1. **生命周期锁域不完整**：facade 的 `rebuild_gate` 与 `CoreManager::run_lock` 不是一个完整事务，`change_core` rollback 窗口可被其他 restart 穿透；
2. **状态语义不足**：现有 runtime store 表达 Promoted，却被 compensation 当作 Applied；深层 rollback 只恢复产品文件，不恢复 store；
3. **跨资源提交不一致**：typed state、legacy mirror、profile 文件、runtime 文件、核心进程之间缺乏明确 prepare/commit/compensate；
4. **验收与测试隔离不足**：单测会写真实用户配置目录，PR-3/4 已发生回归，PR-4 手工 smoke 无可审计闭环。

PR-4S 不继续扩大 actor 数量。它先修复 PR-1～PR-4 的 correctness boundary，使 PR-5 可以在可靠状态模型上接管核心生命周期。

---

## 2. 目标

1. 统一所有 runtime/core 变更的锁顺序和生命周期 lease，消除 `change_core` 与 restart/start/stop 交错。
2. 将 runtime store 改为显式的 Promoted / Applied 双状态，并附 revision、target core、product hash。
3. 任意 rollback 同时恢复产品字节、Promoted、Applied 和 core selection。
4. D6 patch 使用 Applied snapshot，串行化 patch，并支持移除新键。
5. 将 runtime product/candidate 路径全部注入；测试只使用 TempDir。
6. candidate 使用私有目录、随机名称、独占创建、限制权限、RAII cleanup 和启动残留清理。
7. typed actor 的 legacy mirror 改为 prepare-before-persist、infallible apply-after-persist。
8. legacy `IVerge` 跨三域 patch 使用 version-checked saga 和 compensation。
9. ProfilesActor 的状态/文件操作采用明确的 prepare/finalize/compensate；warning 进入结构化 outcome。
10. 去除 `REGEN_BRIDGE` 的 first-install-wins 和无界积压；测试服务图可独立构造和销毁。
11. 增加 PR-3/4 回归 contract suite 与 fake-core failure injection。
12. 形成可审计 Windows/macOS/Linux smoke 记录，并更新 roadmap 生成账本。

---

## 3. 非目标

- 不在本 PR 完成 CoreActor；CoreActor 属 PR-5a。
- 不删除所有 `CoreManager::global()`；但所有涉及 product/apply/restart/change-core 的调用必须进入统一 lifecycle port。
- 不完成 SystemProxyActor、HotkeyActor、ProxiesActor、UpdaterActor。
- 不解散全部 `feat.rs`；只迁移阻塞稳定化正确性的 runtime/core 调用链。
- 不实现通用分布式事务、event sourcing 或跨进程数据库。
- 不引入 snapshot graph UI、incremental runtime rebuild。
- 不对普通 desired config 实施通用自动 rollback；副作用失败采用 committed-degraded + reconcile。

---

## 4. 术语与不变式

### 4.1 RuntimeRevision

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeRevision(u64);
```

- facade 在每次新 runtime build 开始时分配单调递增 revision；
- revision 与 candidate、product hash、Promoted、Applied 绑定；
- 失败 attempt 的 revision 不得复用。

### 4.2 RuntimeSnapshot

```rust
pub struct RuntimeSnapshot {
    pub revision: RuntimeRevision,
    pub target_core: ClashCore,
    pub product_sha256: [u8; 32],
    pub config: serde_yaml::Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}
```

### 4.3 RuntimeLifecycleState

```rust
pub struct RuntimeLifecycleState {
    /// 最新通过 check 并晋升到产品文件的快照。
    pub promoted: Option<Arc<RuntimeSnapshot>>,
    /// 运行核最后一次确认成功 apply/start 的快照。
    pub applied: Option<Arc<RuntimeSnapshot>>,
}
```

不变式：

- `applied.revision <= promoted.revision`，或 applied 为不同 core 的旧 revision；
- `promoted.product_sha256` 必须等于产品文件实际字节 hash；
- 四条 runtime 读 IPC 读取 `promoted`；
- compensation、运行态状态和 effect health 读取 `applied`；
- apply 失败只更新 promoted，不更新 applied；
- rollback 恢复产品时必须恢复 promoted；旧核成功恢复后恢复 applied。

### 4.4 Desired / Promoted / Applied

- Desired 由 Application/Clash/Profiles actor 持有；
- Promoted 由 facade runtime lifecycle store 持有；
- Applied 在 PR-4S 暂由 facade 在成功 apply/restart 后更新；PR-5b 迁入 CoreActor。

---

## 5. 决策记录

| ID  | 决策                                        | 结论                                                                                                                       |
| --- | ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| D1  | 是否直接开始 PR-5                           | 否；先合并 PR-4S 稳定化门                                                                                                  |
| D2  | 普通 config apply 失败是否 rollback desired | 否；保留 desired，返回 committed-degraded，并维持 applied 旧 revision                                                      |
| D3  | runtime store 语义                          | 拆为 promoted/applied；不再用一个状态兼任两者                                                                              |
| D4  | 核心并发控制                                | 所有产品检查、apply、restart、change-core 使用统一 `CoreLifecycleLease`；固定锁顺序 `rebuild/patch gate → lifecycle lease` |
| D5  | rollback 材料                               | 捕获 `RuntimeTransactionSnapshot`：旧产品字节、旧 lifecycle state、旧 core selection                                       |
| D6  | D6 patch compensation                       | patch gate 串行；基于 Applied snapshot；补偿支持 set 与 remove；使用 expected applied revision 防止覆盖后续更新            |
| D7  | actor mirror                                | `prepare(next)` 可失败且在 persist 前；`PreparedMirror::apply()` 不可失败且在 persist 后                                   |
| D8  | legacy 三域 patch                           | version-checked saga；任一步失败补偿已提交域；补偿失败返回 `PartialCommit`                                                 |
| D9  | profile 文件事务                            | 为 Add/Replace/Refresh 建 prepare/finalize/compensate；Delete cleanup 可 committed-degraded + persistent cleanup queue     |
| D10 | runtime 路径                                | `RuntimePaths` 由 composition root 注入；测试禁止全局 dirs                                                                 |
| D11 | candidate 安全                              | 私有目录 + tempfile/random + create_new + owner-only 权限 + cleanup guard + stale cleanup                                  |
| D12 | rebuild dispatcher                          | bounded/coalescing、可关闭、可重建；不允许 first-install-wins 静态 handler                                                 |
| D13 | outcome                                     | 采用 `MutationOutcome<T>` + phase/code；旧 `RebuildOutcome` 作为 wire compatibility alias 仅在同 PR 前端切换期间存在       |
| D14 | PR-3/4 回归                                 | 固化为 contract fixtures，不能仅依赖已关闭 issue                                                                           |
| D15 | 手工 smoke                                  | 必须生成可追溯记录；未记录视为未执行                                                                                       |

---

## 6. 架构设计

### 6.1 RuntimePaths

新增 Tauri-free value object：

```rust
#[derive(Clone)]
pub struct RuntimePaths {
    product: Utf8PathBuf,
    candidate_dir: Utf8PathBuf,
}
```

构造规则：

- composition root 从单一 `PathResolver` 派生；
- product = `<app-config>/runtime/clash-config.yaml`；
- candidate_dir = `<app-config>/runtime/.candidates/`；
- `NyanpasuClient`, runtime publisher 和 core adapter 共用同一实例；
- 测试传入 TempDir；
- `client/runtime.rs` 不得调用 `utils::dirs::*`。

### 6.2 CandidateFile guard

```rust
pub struct CandidateFile {
    path: Utf8PathBuf,
    bytes_sha256: [u8; 32],
}
```

创建流程：

1. `create_dir_all(candidate_dir)`；
2. 校验 candidate_dir 不是 symlink/reparse point；
3. 使用 `tempfile::Builder` 或等价随机名在私有目录独占创建；
4. Unix 显式 `0o600`；Windows 使用 owner-only 默认 ACL 并拒绝意外 reparse point；
5. 写入、`flush`，需要时 `sync_all`；
6. 计算 hash；
7. Drop best-effort 删除；显式 `cleanup()` 返回错误供日志；
8. 应用启动时删除超过阈值的 stale candidate。

产品晋升必须使用 captured bytes 或已打开 candidate 的不可变副本，并在 promote 后验证产品 hash。

### 6.3 CoreLifecyclePort 与 lease

application 层依赖：

```rust
#[async_trait]
pub trait CoreLifecyclePort: Send + Sync + 'static {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
}

#[async_trait]
pub trait CoreLifecycleLease: Send {
    async fn check_and_promote(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
        product: &Utf8Path,
    ) -> anyhow::Result<[u8; 32]>;

    async fn apply_promoted(&mut self, product: &Utf8Path) -> anyhow::Result<()>;
    async fn restart(&mut self) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<()>;
}
```

legacy adapter 在 `begin()` 时持有 `CoreManager` 的生命周期互斥 guard。`CoreManager::run_core()`、recover 和现存直接生命周期入口必须获取同一把锁；lease 内部使用不重复加锁的 inner 方法。

固定锁顺序：

```text
patch_gate（如有）
  → rebuild_gate
    → CoreLifecycleLease
      → runtime store write（短持有）
```

禁止反向获取。状态查询不得持有 rebuild gate。

### 6.4 Runtime 发布流程

```text
allocate revision
  → snapshot desired inputs
  → build RuntimeSnapshot + yaml bytes
  → create private CandidateFile
  → acquire CoreLifecycleLease
  → check exact candidate bytes against target core
  → atomic promote to product
  → verify product hash
  → publish lifecycle.promoted = snapshot
  → optional apply/restart
       success: lifecycle.applied = snapshot
       failure: applied 保旧，返回 CommittedDegraded(RuntimeApply)
```

`check_and_promote` 失败：产品、promoted、applied 均保持旧值。

`promote` 成功但 store publish 失败：产品是权威；返回 degraded，并立即排队 store reconciliation。此理论路径必须有故障注入测试，不能只靠注释。

### 6.5 RuntimeTransactionSnapshot

```rust
pub struct RuntimeTransactionSnapshot {
    pub product: Option<Vec<u8>>,
    pub lifecycle: RuntimeLifecycleState,
    pub selected_core: ClashCore,
}
```

仅 all-or-nothing operation 使用。`change_core` 在任何修改前捕获。rollback 顺序：

1. 恢复旧 selected core desired value；
2. 尝试从 committed desired state 重建旧核 runtime；
3. 若重建失败，原子恢复旧 product bytes；
4. 恢复 lifecycle.promoted；
5. 启动旧核；
6. 成功则恢复 lifecycle.applied；失败则 applied 保留旧 snapshot 但 health 标记 stopped/degraded；
7. 返回结构化 `ChangeCoreOutcome`，错误链不得吞掉 rollback failure。

### 6.6 Change-core 并发模型

整个事务持有：

```text
rebuild_gate + CoreLifecycleLease
```

其他 `run_core/restart_sidecar/recover` 必须等待 lease。测试使用 barrier：

- 新核 restart 返回失败后暂停；
- 并发触发 restart；
- 断言 restart 未进入；
- rollback 完成后并发 restart 才能继续。

### 6.7 Applied-based patch compensation

新增 facade `clash_patch_gate`，覆盖：

```text
read applied snapshot
  → API-first patch
  → persist desired clash state
  → rebuild/check/promote
  → apply/restart
```

补偿计划：

```rust
pub enum PatchCompensationOp {
    Set { key: String, value: serde_yaml::Value },
    Remove { key: String },
}
```

规则：

- previous value 来自 `lifecycle.applied`；
- previous 中不存在的 key 生成 Remove；
- compensation 带 expected applied revision；如果运行核已前进到其他 revision，拒绝旧补偿并返回 conflict degradation；
- compensation 成功不改变 desired；Promoted 保持最后一次成功 promote；Applied 回到旧 snapshot；
- PR-5b 将此 gate 和 applied owner 迁入 CoreActor mailbox。

### 6.8 Prepared legacy mirror

Tauri-free trait：

```rust
pub trait PreparedLegacyMirror: Send {
    /// 只做不可失败的内存 apply；实现不得序列化、解析或访问磁盘。
    fn apply(self: Box<Self>);
}

pub trait VergeLegacyBridge: Send + Sync {
    fn prepare(
        &self,
        next: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;

    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig>;
}
```

Window/Clash 同构。

Actor commit：

```text
clone + patch + validate
  → bridge.prepare(next)          // 可失败；失败则零提交
  → manager.upsert(next)          // typed commit
  → prepared.apply()              // 不可失败
  → reply committed snapshot
```

prepared object 捕获转换后的 legacy projection 和具体 legacy store handle。apply 不返回 Result；若实现中仍存在可失败操作，说明 prepare 边界设计错误。

### 6.9 Legacy 三域 saga

为 Application/Session/Clash actor 增加：

```rust
ReplaceIfVersion {
    expected: u64,
    state: T,
}
```

Saga：

1. 读取三域 `{state, version}`；
2. 纯函数生成三个 next state；
3. 对三个 next state 做全部校验和 mirror prepare；
4. 固定顺序 Application → Session → Clash 进行 `ReplaceIfVersion`；
5. 任一步失败，按逆序用 `ReplaceIfVersion(new_version, old_state)` 补偿；
6. 补偿全成功：返回原始错误，最终三域保持旧值；
7. 补偿失败：返回 `PartialCommit { committed_domains, compensated_domains, failed_compensations }`，发布高优先级 degradation 并触发 reconciliation。

typed 直接 mutation 可以并发，但 version check 防止 saga 覆盖更晚的 typed commit。

### 6.10 Profile materialization transaction

#### Add managed/local profile

```text
prepare state + validate
  → stage initial file in profiles/.staging
  → persist profiles state
  → atomic rename stage → materialized path
  → finalize metadata if needed
```

rename 失败时 actor mailbox 尚未处理下一条 mutation：立即将 profiles state version-checked 回滚到 before，并删除 stage；返回 Err。不得留下指向缺失文件的成功 profile。

#### ReplaceDefinition

- 新 materialization/symlink 先 prepare；
- state commit 后 finalize；
- finalize 失败时 rollback state 和新资源；
- old file cleanup 在成功 finalize 后执行；cleanup 失败可 committed-degraded + cleanup queue。

#### Remote refresh

```text
download + validate
  → stage new bytes
  → capture old materialized bytes/path metadata
  → promote staged bytes
  → persist subscription/updated_at
```

metadata persist 失败则恢复旧 bytes；恢复失败返回 compound `ProfileMaterialization` degradation。URL/definition stale fence 继续保留。

#### Delete

state 删除是权威 commit。文件删除失败不回滚 profile state，写入持久 cleanup queue 并返回 committed-degraded。下次启动和定期 reconcile 重试。

### 6.11 MutationOutcome

```rust
#[derive(Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Applied { value: T },
    CommittedDegraded {
        value: T,
        degradations: Vec<Degradation>,
    },
}

pub struct Degradation {
    pub phase: DegradationPhase,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
```

对于 `T = ()`，specta 仍生成稳定命名类型，避免前端特殊处理裸 enum 与 wrapper 两套形态。

旧 `RebuildOutcome` 在同 PR 中一次性迁移并删除；不保留 `_v1` IPC alias。

### 6.12 Rebuild dispatcher

目标：消除静态 first-install-wins 和无界消息积压。

优先方案：

- `RebuildCoordinator` 作为 facade 内对象；
- background notification 用容量 1 的 dirty/coalescing channel；
- request/reply operation 直接调用 typed facade method，不走进程静态 dispatcher；
- 剩余 legacy 调用链改为显式接收 `NyanpasuClient` / 窄 port；
- coordinator 有 `shutdown()`，client Drop/测试结束可关闭；
- 同一进程可构造多个完全独立 client graph。

若某一 legacy 静态入口确实不能在本 PR 注入，允许保留单一临时 adapter，但必须：

- 不使用 `OnceCell` first-install-wins；
- 测试可 reset；
- bounded；
- 在 task/roadmap 指明 PR-5/6 的唯一删除条件；
- 新代码禁用。

---

## 7. Failure Matrix

| 操作               | 失败点                              | Desired                  | Product/Promoted           | Applied                      | 返回                                 |
| ------------------ | ----------------------------------- | ------------------------ | -------------------------- | ---------------------------- | ------------------------------------ |
| typed config patch | mirror prepare                      | 旧                       | 不变                       | 不变                         | Err，零提交                          |
| typed config patch | persistence                         | 旧                       | 不变                       | 不变                         | Err，零提交                          |
| typed config patch | post-commit effect                  | 新                       | 按需                       | 旧或新                       | CommittedDegraded                    |
| runtime rebuild    | build/check                         | 新                       | 旧                         | 旧                           | CommittedDegraded / legacy Err       |
| runtime rebuild    | promote                             | 新                       | 旧                         | 旧                           | Degraded                             |
| runtime rebuild    | publish store                       | 新                       | 新产品；store 待 reconcile | 旧                           | Degraded                             |
| runtime apply      | core reject                         | 新                       | 新                         | 旧                           | CommittedDegraded(RuntimeApply)      |
| change_core        | 新核 build/check                    | 旧 core intent           | 旧                         | 旧                           | Err                                  |
| change_core        | 新核 restart，rollback 成功         | 旧                       | 旧                         | 旧                           | Err + rollback_succeeded             |
| change_core        | rollback rebuild 失败、文件恢复成功 | 旧                       | 恢复旧                     | 旧核成功后恢复旧             | compound Err                         |
| change_core        | 旧核也启动失败                      | 旧                       | 旧                         | 旧 snapshot + health stopped | compound Err / stopped               |
| legacy 三域 patch  | 第二域 persist                      | compensation 后旧        | 不变                       | 不变                         | Err                                  |
| legacy 三域 patch  | compensation 失败                   | 部分                     | 不变                       | 不变                         | PartialCommit + critical degradation |
| profile add        | stage/write                         | 旧                       | —                          | —                            | Err                                  |
| profile add        | finalize rename                     | 回滚旧                   | —                          | —                            | Err                                  |
| remote refresh     | metadata persist                    | 旧 metadata，恢复旧 file | —                          | —                            | Err/compound                         |
| delete             | file cleanup                        | 已删除                   | —                          | —                            | CommittedDegraded + cleanup pending  |

---

## 8. 回归测试设计

### 8.1 PR-2 actor / bridge

- mirror prepare 转换失败：manager version/state 不变；
- persist 成功后 apply 不可能返回错误；
- three-domain saga 第二/第三域失败；
- typed concurrent mutation 导致 version conflict，saga 不覆盖新值；
- compensation 自身失败返回 PartialCommit。

### 8.2 PR-3 profiles

固化以下 regression fixtures：

- legacy migration 保留 IPv6（#4893）；
- local profile import wire（#4916）；
- remote source / options / subscription wire（#4917、#4920）；
- profile title/default interval 与显式 interval；
- add 初始文件失败不留下 profile；
- refresh metadata persist 失败恢复旧文件；
- delete cleanup 失败进入 cleanup queue；
- symlink/reparse point 拒绝写入。

### 8.3 PR-4 runtime/core

- candidate check/promote/publish 各阶段故障注入；
- candidate Drop cleanup、stale cleanup、权限；
- product hash 与 store hash 一致；
- apply 失败：promoted 新、applied 旧；
- change_core 四级失败分支；
- concurrent restart barrier；
- patch compensation set/remove、revision conflict；
- mixed-port fixed/random、旧核占用（#4921）；
- runtime IPC 只读取 promoted。

### 8.4 Test path isolation

CI grep/deny：

```text
#[cfg(test)] ... dirs::app_config_dir()
#[test] ... runtime_config_path()  // 若其内部仍读全局 dirs
```

允许测试调用的 path API 必须由 test `RuntimePaths` 实例返回。

### 8.5 Fake-core

新增 test-only Rust binary，支持参数/环境变量：

- `--check-ok/--check-fail`；
- `--start-ok/--start-fail`；
- `--apply-ok/--apply-fail`；
- barrier 文件/端口；
- 占用 fixed port；
- 可控 stdout/stderr；
- 可控延迟和退出码。

用于验证真实进程边界，而不是只验证 mock 调用顺序。

---

## 9. 前端与 wire

- `MutationOutcome<T>` 由 specta 生成；
- `MutationCache` 识别 `committed_degraded`；
- toast 展示本地化 phase/code，详细 message 写日志/可展开；
- profile warnings 不再只记录 tracing；
- create/import/update/delete/reorder/activate/patch 均返回统一 wrapper；
- `unwrapResult` 改为穷尽式返回 `T`，不保留理论上的 `undefined`；
- TS binding shape 有逐字 contract assertion；
- 同 PR 更新 en/zh-cn/zh-tw/ru/ko。

---

## 10. 可观测性

结构化 tracing 字段：

```text
runtime.revision
runtime.target_core
runtime.product_sha256
runtime.promoted_revision
runtime.applied_revision
mutation.domain
mutation.outcome
mutation.degradation_phase
core.rollback_stage
profile.uid
profile.materialization_op
```

禁止记录 profile 文件内容、订阅凭据、secret、完整 runtime YAML。

新增诊断读取（内部或 debug command，非必需前端公开）：

```rust
RuntimeHealth {
    promoted_revision,
    applied_revision,
    target_core,
    running_core,
    degraded,
    last_error,
}
```

---

## 11. 安全与崩溃恢复

- candidate/staging/cleanup queue 目录均在应用私有路径；
- 所有 file promotion 使用 atomic rename；
- 不跟随意外 symlink/reparse point；
- startup reconcile：
  - 删除 stale runtime candidates；
  - 删除 profile staging leftovers；
  - 重试 delete cleanup queue；
  - 校验 product hash 与 promoted snapshot；若内存 store 初始为空，从可信产品重建只读 snapshot 或等待首次 rebuild，不读取 unchecked 文件；
- crash 发生在 state commit/file finalize 之间时，下一次启动依据 journal/queue 恢复。

journal 只保存路径、operation id、revision 和 hash，不保存敏感完整内容；旧字节备份使用 owner-only 临时文件并在完成后删除。

---

## 12. 实施边界与文件落点

预计修改：

- `backend/tauri/src/client/runtime.rs`
- `backend/tauri/src/client/rebuild.rs`
- `backend/tauri/src/client/core_bridge.rs`（trait 可能重命名为 `core_port.rs`）
- `backend/tauri/src/client/mod.rs`
- `backend/tauri/src/client/ports.rs`
- `backend/tauri/src/setup.rs`
- `backend/tauri/src/core/clash/core.rs`
- `backend/tauri/src/ipc.rs`
- `backend/tauri/src/feat.rs`（仅阻塞链）
- `backend/tauri/src/state/{application,session_state,clash_config}.rs`
- `backend/tauri/src/state/mirror.rs`
- `backend/tauri/src/bridge/{verge,window,clash}.rs`
- `backend/tauri/src/state/profiles/actor.rs`
- `backend/tauri/src/client/profiles.rs`
- `backend/tauri/src/service/profile_file.rs`
- frontend binding/provider/profile hooks
- test support fake-core、fixtures、architecture ledger script
- roadmap / task / PR-4 spec disposition

禁止：

- 新 `::global()`；
- 新 mutable static service；
- actor/client import Tauri；
- 用另一个 global wrapper 隐藏现有 global；
- 通过 sleep 解决并发测试；
- 测试读取真实用户配置路径。

---

## 13. 验收标准

### 13.1 自动化

- `cargo test --workspace --all-features`；
- `cargo clippy` / rustfmt；
- frontend typecheck/build；
- specta bindings freshness；
- fake-core integration suite；
- architecture ledger / denylist；
- 所有回归 fixture；
- Windows/macOS/Linux CI 均通过。

### 13.2 手工 smoke

必须覆盖并记录：

1. 首次启动 + 删除产品后的 fallback；
2. profile 切换 + mode/allow-lan/ipv6；
3. mixed-port fixed/random 与即时生效；
4. mihomo ↔ clash-rs 成功换核；
5. 新核二进制故障时硬回滚；
6. remote-dependent profile 断网后的 committed-degraded；
7. patch rebuild 失败后的 Applied-based compensation；
8. local/remote/composition profile 创建、导入、刷新和删除；
9. Windows service mode；
10. macOS/Linux TUN 权限路径。

### 13.3 文档

- v3 roadmap 状态更新为 PR-4S 已完成；
- PR-4 四个 review finding 逐条 disposition；
- PR-4 五项 smoke 证据链接；
- `TODO(actor-migration)` ledger 自动刷新；
- 所有延期项有负责 PR 和删除条件。

---

## 14. 风险

| 风险                                     | 缓解                                                                      |
| ---------------------------------------- | ------------------------------------------------------------------------- |
| 稳定化 PR 跨模块较多                     | 单一 spec、分 commit lane、failure matrix 驱动 review；不并入外围 actor   |
| lifecycle lease 与现有 run_lock 双锁死锁 | 固定锁顺序；lease 内只调 unlocked inner；loom/barrier 风格测试            |
| profile compensation 再失败              | version-checked rollback + compound outcome + startup journal reconcile   |
| wire 改动再次引入 Specta/Serde 漂移      | 实例化 union 逐字冻结 + frontend build                                    |
| Applied 初始未知                         | boot/start 成功路径显式发布；未知时 D6 不盲目补偿，返回可重试 degradation |
| private candidate 跨平台权限差异         | 平台 adapter + CI metadata assertions；无法保证的 OS 记录具体限制         |
| PR-5 与 PR-4S 重叠                       | PR-5 设计可并行，代码以 PR-4S merge commit 为新基线                       |

---

## 15. 成功定义

PR-4S 完成后：

- PR-1～PR-4 不再存在已确认的 correctness blocker；
- 产品文件、Promoted、Applied 和 selected core 在所有已建模失败分支保持一致或显式 degraded；
- typed state 与 legacy mirror 不会产生“已提交但普通 Err”；
- legacy 三域 patch 不会静默部分提交；
- profile 状态/文件有恢复协议；
- 测试图完全隔离；
- 已发生回归被 contract suite 固化；
- PR-5 可以只关注 ownership migration，而不同时修补 PR-4 的事务漏洞。
