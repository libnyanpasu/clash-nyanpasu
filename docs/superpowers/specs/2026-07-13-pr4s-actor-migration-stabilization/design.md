# PR-4S — PR-1～PR-4 Actor Migration 稳定化门（设计 spec）

**日期：** 2026-07-13
**状态：** **COMPLETE**（PR-4S 稳定化门已关闭；**PR-5a unlocked**）。不宣称 actor migration 全量完成或 residual 清零（PR-5/6/7 残差仍在）。

**状态权威：** 本文件只承载设计语义（协议、类型形状、不变式、锁顺序、failure matrix、决策与验收）。S01～S10 进度、工作区验证记录与“是否完成”宣告以 [`task.md`](./task.md) 为准；closeout 证据见 [`smoke-evidence.md`](./smoke-evidence.md)（含 cleanup-tip **Q-18…Q-20**）、[`review-disposition.md`](./review-disposition.md)、[`residual-ledger.md`](./residual-ledger.md)。手工 smoke 执行权威为 [`smoke-evidence.md`](./smoke-evidence.md)（E-01…E-11 maintainer-attested PASS @ 2026-07-18；raw fields 未保留）。本文件不重复逐步执行日记。

**范围基线：** `main @ 9886aacc750b691d6abc893808ddaaf9dfb6a538`（`fix(proxy): resolve provider-owned proxies (#4954)`；包含 PR-4 `#4932`）
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

相对 **范围基线**（PR-4S 稳定化开始前），当时存在四类系统性缺陷（下列为 baseline 问题陈述，不是“当前工作区仍全部成立”的实时状态宣称；关闭进度见 `task.md`）：

1. **生命周期锁域**：`change_core` 与 restart/start/stop 可交错；`CoreManager` 生命周期锁不覆盖全部路径；缺少 process-level failure matrix（check fail、immediate start fail、apply 500、port conflict/release、lease serialization、clean stop/reap、two-graph isolation、change_core new-start fail + old rollback）。
2. **状态语义残差**：runtime store 兼任 promoted/applied；rollback 未完整恢复 product/Promoted/Applied；D6 patch 不以 Applied 为基、缺少 revision fence 与 Remove 语义。
3. **跨资源提交不一致**：legacy mirror prepare/apply 边界不清；`IVerge` 三域 patch 缺 version-checked saga 与 structured `PartialCommit`；Profiles 状态/文件缺 materialization 事务、durable revision、cancellation-safe import 与 reconcile；公共 wire 缺统一 `MutationOutcome`。
4. **验收与测试隔离**：process-global rebuild dispatcher（`REGEN_BRIDGE`/OnceCell first-install-wins）阻碍独立测试图；缺 test-only fake-core 与可审计三平台 smoke/evidence。

**仍属当前、且刻意留给后续 PR 的 residual（非 PR-4S 四类 baseline 缺陷本身）：** legacy `Config` / `CoreManager` global 仍非 full graph desired-state isolation（PR-5/6 ownership migration 范围）。

PR-4S 不继续扩大 actor 数量。它先以本文件的协议与不变式修复 PR-1～PR-4 的 correctness boundary，使 PR-5 可以在可靠状态模型上接管核心生命周期。**S10 / PR-4S 稳定化门已 COMPLETE（2026-07-18）；PR-5a unlocked。** 关闭证据包：E-01…E-11 maintainer-attested PASS（权威 [`smoke-evidence.md`](./smoke-evidence.md)；raw fields 未保留）；review Path A；target-tip CI run 29635372676；cleanup-tip CI run 29638274786 **SUCCESS** @ `8909566c0bb759f562d420af4b9672469920fc21`（权威 [`smoke-evidence.md`](./smoke-evidence.md) **Q-18…Q-20**）；residual ledger 文档化；local QA。**不**宣称 actor migration 完成或 PR-5/6/7 residual 清零。

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
    product_bytes: Arc<[u8]>, // exact promoted/applied product bytes
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

| ID  | 决策                                        | 结论                                                                                                                                                                                                                                                      |
| --- | ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | 是否直接开始 PR-5                           | 否；先合并 PR-4S 稳定化门                                                                                                                                                                                                                                 |
| D2  | 普通 config apply 失败是否 rollback desired | 否；保留 desired，返回 committed-degraded，并维持 applied 旧 revision                                                                                                                                                                                     |
| D3  | runtime store 语义                          | 拆为 promoted/applied；不再用一个状态兼任两者                                                                                                                                                                                                             |
| D4  | 核心并发控制                                | 所有产品检查、apply、restart、change-core 使用统一 `CoreLifecycleLease`；固定锁顺序 `rebuild/patch gate → lifecycle lease`                                                                                                                                |
| D5  | rollback 材料                               | 捕获 `RuntimeTransactionSnapshot { product, lifecycle }`（旧产品字节 + 旧 lifecycle state）；旧 core selection 不进 snapshot，由 `Config::verge().discard()` 在 rollback rebuild 前恢复                                                                   |
| D6  | D6 patch compensation                       | instance-owned patch gate 串行；基于 Applied snapshot 的 transport-independent Set/Remove（禁止 JSON `null` 删除）；expected Applied revision fence 防止覆盖后续更新；私有 candidate 直接 apply，不 promote product                                       |
| D7  | actor mirror                                | `prepare(next)` 可失败且在 persist 前；`PreparedLegacyMirror::apply()` 不可失败、仅更新内存 projection、且在 persist 后；prepare failure 零提交                                                                                                           |
| D8  | legacy 三域 patch                           | manager-level expected-version CAS（actor 消息 `ReplacePreparedIfVersion`）；Application→Session→Clash saga；reverse compensation；structured `PartialCommit` 与 finalizer uncertainty                                                                    |
| D9  | profile 文件事务                            | durable server-owned `Profiles.revision`（≠ manager MVCC）；state-first / file-first / cleanup / reconcile；import fetch-before-commit（取消安全）；启动+周期 recovery；crate-internal `ProfileDegradation`；公共 `MutationOutcome` 映射                  |
| D10 | runtime 路径                                | `RuntimePaths` 由 composition root 注入；测试禁止全局 dirs                                                                                                                                                                                                |
| D11 | candidate 安全                              | 私有目录 + tempfile/random + create_new + owner-only 权限 + cleanup guard + stale cleanup                                                                                                                                                                 |
| D12 | rebuild dispatcher                          | 删除 `REGEN_BRIDGE`/OnceCell；instance-owned capacity-1 coalescing `RebuildCoordinator`；Weak worker；direct typed requests；explicit `shutdown` + 生产 `cleanup_processes` exit 集成；two-client/clone isolation；不允许 first-install-wins 静态 handler |
| D13 | outcome                                     | `MutationOutcome<T>` + phase/code 为唯一公共 wire；旧 `RebuildOutcome` 已删除，无 `_v1` alias                                                                                                                                                             |
| D14 | PR-3/4 回归                                 | 固化为 contract fixtures，不能仅依赖已关闭 issue                                                                                                                                                                                                          |
| D15 | 手工 smoke                                  | 必须生成可追溯记录；未记录视为未执行；执行权威 [`smoke-evidence.md`](./smoke-evidence.md)（E-01…E-11 maintainer-attested PASS @ 2026-07-18；raw fields 未保留）；与 cleanup-tip **Q-18…Q-20** 等证据共同关闭 S10 / PR-4S 门                               |

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
8. 应用启动时删除超过寿命的 stale candidate。

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
}
```

仅 all-or-nothing operation 使用。`change_core` 在任何修改前捕获 snapshot（product + lifecycle）。旧 core selection 不在 snapshot 内：rollback 时先通过 `Config::verge().discard()` 恢复旧 selected-core desired value，再做 rollback rebuild。rollback 顺序：

1. 恢复旧 selected core desired value（`Config::verge().discard()`，非 snapshot 字段）；
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

`change_core` 全程持有上述锁域；rollback 路径（旧核 rebuild/restart、product restore、Promoted/Applied 恢复）不释放 lease。其他 `run_core/restart_sidecar/recover` 必须等待 lease。测试使用 barrier：

- 新核 restart 返回失败后暂停；
- 并发触发 restart；
- 断言 restart 未进入；
- rollback 完成后并发 restart 才能继续。

并发测试使用 barrier/oneshot，禁止 sleep。

### 6.7 Applied-based patch compensation

facade 的 instance-owned `clash_patch_gate` 覆盖：

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

- previous value 来自 `lifecycle.applied`；`RuntimeSnapshot` 保存 hash 对应的 exact product bytes，避免从 YAML mapping 重序列化恢复；
- previous 中不存在的 key 生成 `Remove`；`Set` / `Remove` 是 transport-independent plan，删除绝不编码为 JSON `null`；
- compensation 带 expected applied revision；如果运行核已前进到其他 revision，拒绝旧补偿并返回 conflict degradation；
- patch gate 内再取得 rebuild gate 和 lifecycle lease；补偿因此受 rebuild/lifecycle exclusion 保护；
- compensation 为 Applied bytes 创建私有 candidate，并经 lease direct apply；不会 check/promote 或改写 product；
- compensation 成功不改变 desired；Promoted 保持最后一次成功 promote，Applied 回到旧 snapshot。例如最终矩阵为 `Promoted = P3`、`Applied = P1`；
- IPC 只解析 mapping 并调用 facade；API-first patch、desired persist、rebuild 和补偿不在 IPC 重复编排；
- set/remove、unknown Applied、revision conflict、exclusion、exact-bytes 与 P3/P1 须有 contract tests；PR-5b 将此 gate 和 applied owner 迁入 CoreActor mailbox。

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
  → bridge.prepare(next)                         // 可失败；失败则零提交
  → manager.upsert / replace_if_version(next)    // typed commit；CAS 路径校验 expected version
  → prepared.apply()                             // 不可失败
  → reply committed snapshot
```

prepared object 捕获转换后的 legacy projection 和具体 legacy store handle。apply 不返回 Result；若实现中仍存在可失败操作，说明 prepare 边界设计错误。`PreparedTypedReplace<T>` 将 next typed state 与 prepared mirror 绑定；manager-level CAS conflict 或 persistence failure 均发生在 `apply()` 前，state/version 与 legacy projection 不前进。

**生命周期边界：** `PreparedLegacyMirror`、`PreparedTypedReplace<T>` 和 actor/client 的 `PrepareReplace` 只为 PR-7 前仍存在的 runtime legacy projection 服务，不是目标架构中的长期领域抽象。PR-7a 删除 `state/mirror.rs` 与最后的 legacy bridge 后，普通 typed mutation 直接执行 validate → manager persist → reply，不再生成 prepared mirror。

### 6.9 Legacy 三域 saga

为 Application/Session/Clash actor 增加：

```rust
ReplacePreparedIfVersion {
    expected_version: u64,
    prepared: PreparedTypedReplace<T>,
}
```

Saga：

1. 读取三域 `{state, version}`；
2. 纯函数生成三个 next state；
3. 对全部 forward 与 rollback state 完成 mirror prepare；任一 prepare 失败则零提交；
4. 固定顺序 Application → Session → Clash 通过 `ReplacePreparedIfVersion` 提交；expected-version CAS 由 manager/coordinator 在 persistence effect 前执行；
5. 任一步 CAS/commit 失败，按已提交域的逆序用 committed version 与 rollback prepared state 补偿；第三域失败时为 Session → Application；
6. typed 补偿全成功且 legacy state 确定：返回原始错误，最终三域保持旧值；
7. compensation conflict/error 或 legacy finalizer/state uncertainty：返回 `PartialCommit { primary_error, committed_domains, compensated_domains, failed_compensations }`，发布高优先级 degradation 并触发 reconciliation；
8. `CompensationFailure` 区分 `Conflict { domain, expected_version, actual_version }`、`Error { domain, message }` 与 `LegacyStateUncertain { message }`；补偿继续收集全部失败，不在首个失败处停止。

typed 直接 mutation 可以并发，但 manager-level version check 防止 saga 或 compensation 覆盖更晚的 typed commit。legacy finalizer 在全部 typed commit 后执行；若 finalizer 失败，即使 reverse compensation 全部成功，legacy persistence 仍不确定，必须保留 structured `PartialCommit`，不得返回普通 `Err`。

**PR-7a 清算契约：** `ReplacePreparedIfVersion` 的 prepared payload 同时承载 typed state 和 legacy mirror，因此不能在 mirror 删除后原样保留。legacy `IVerge` 三域入口及其 saga/finalizer 全部迁出后，PR-7a 同步删除三个 actor/client 的 `PrepareReplace`、`ReplacePreparedIfVersion`、`PreparedTypedReplace<T>` 以及仅被 saga 使用的 forward/rollback bookkeeping。manager/coordinator 的原子 `replace_if_version` 是独立并发控制 primitive：若届时仍有非 legacy production caller，则 actor 协议收敛为 `ReplaceIfVersion { expected_version, state }`；若无 caller，则删除 actor/client conditional protocol。不得保留一个仅把 legacy 名称隐藏起来的兼容 wrapper。

### 6.10 Profile materialization transaction

#### Durable revision（≠ manager MVCC）

- `Profiles.revision` 是 server-owned、可持久化的 durable state generation，专供 materialization journal 与 recovery 使用。
- 它故意不进入公共 Specta 文档；与 `PersistentStateManager` 的 process-local MVCC `version` 严格分离，不得从后者派生或混用。
- 每次 forward 或 compensating state commit 前，actor 在 prepare candidate 时 `bump_revision()`；prepare/journal 写入 expected `Profiles` 快照上的 durable revision、operation id、managed path 与 content hash。

#### 协议

```text
state-first: prepare(next) → state CAS → promote → complete/compensate
file-first:  prepare(next) → promote → state CAS → complete/compensate
cleanup:     prepare(next) → state CAS → activate → retry
reconcile:   reconcile(loaded profiles) before watchers or mutations
```

- state-first / file-first journal 相位位于应用私有目录（prepared / promoting / promoted / compensating）；cleanup 有 pending / ready 相位与 tombstone。
- prepare 失败零提交；promote/compensate 失败返回 compound materialization error；`complete` 失败可 crate-internal deferred degradation，由 reconcile 收尾。
- cleanup 成功可 `Removed` / `AlreadyAbsent`；active path 与 hash reuse 触发 fence，不盲删。

#### 操作映射

| 操作                        | 协议                                                    | 权威 / 失败语义                                                                                                          |
| --------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Add                         | state-first（附 resource）                              | state 提交后 promote；promote 失败 version-checked state rollback + compensate；不得留下指向缺失文件的成功 profile       |
| ReplaceDefinition           | state-first；slot 变化附 resource；旧 path 可附 cleanup | 新资源 prepare → state CAS → promote；prepare-cleanup 失败补偿新资源并保留旧文件；promote 失败 cancel cleanup + rollback |
| Remote **import**           | actor-owned **fetch-before-commit** → 一次 state-first  | 校验/下载成功且调用方仍在等待后，才用真实 bytes 做一次 state-first commit；取消/失败零 state/file；不走占位回删          |
| Remote refresh（手动/定时） | file-first                                              | 既有 remote 的 download/validate → promote 新 bytes → CAS 元数据；CAS 失败 restore backup；stale fence 继续保留          |
| External Mirror 同步        | file-first                                              | 与 refresh 同协议，变更 fence 后写入                                                                                     |
| Delete                      | cleanup（state commit 为权威）                          | prepare-cleanup 失败则不删 state；state 删除成功后 activate/retry 失败 → `CleanupDeferred`，不回滚 state                 |
| ReconcileMaterializations   | recovery                                                | 启动与 actor-owned 周期任务串行恢复 journal / cleanup                                                                    |

#### Remote import 终态协议（cancellation-safe；取代占位回删）

**禁止**旧式 `add empty placeholder → refresh → delete compensation` 编排。终态协议固定为 actor 拥有的 fetch-before-commit：

```text
validate request
  → in-memory PendingImport (never durable; no Profiles item, no file, no journal)
  → fetch + validate content
  → CommitImported:
       reply closed / token missing  → discard; zero state / zero file
       fetch failed                  → ImportFailed; zero state / zero file
       success                       → ONE state-first commit with real bytes
                                       (existing materialization journal / recovery only)
  → facade post-commit auto-activation (degradation only; never erases ProfileId)
```

硬约束：

- **任何 durable placeholder 之前**必须完成 request 校验与 fetch/content validation；`PendingImport` / `ImportOperationToken` 仅内存，不进 schema、不进 `Profiles.revision`、不写新 import journal 字段。
- 取消发生在 durable commit 开始之前：丢弃下载结果，**零 state / 零 file**；不需要 delete compensation。
- actor restart / late `CommitImported` 找不到 token：无操作；因从未 durable 写入。
- 成功路径：一次 `commit_state_first` 写入完整 remote profile + 真实 bytes；物化恢复**只复用**既有 state-first journal / reconcile（prepare → state CAS → promote → complete/compensate），**不**新增 import-specific journal/schema/heuristics。
- `create_profile` 继续拒绝 remote 源（须走 `import_profile`）。直接 `Add` 的 remote 路径仍可 stage 空文件，但 public import **永不**经该路径。
- **Manual/scheduled remote refresh 保持 file-first 不变**；import 与 refresh 协议分离。
- auto-activation 仍是 facade **post-commit** 降级：`set_current_if_none`；`Ok(None)` 保持 Applied；hard failure → `SystemEffect` / `profile_auto_activation_failed`，**保留**已提交 `ProfileId`。

#### Superseded state-first journal 规则（已修正）

当 `profiles.revision > journal.revision` 且 managed path 仍 active：

- target 已匹配 journal hash：
  - `StatePromoting` → `complete`；
  - `StatePrepared` → `discard`（不 promote）；
- target 仍处于 pre-promote（备份/旧字节）：**必须 `compensate`，绝不可 promote** 将 stale staged content 应用到更新的 committed revision；
- target 已偏离 journal 且非 pre-promote：bail 为 recovery degradation，保留可审计现场。

此规则取代“revision 前进即自动 promote 未完成 state-first journal”的错误假设。file-first 在 revision 已前进且 target 匹配时可完成；不匹配则 compensate/discard。

#### 启动与周期 recovery

- client/actor 启动返回前执行一次 `reconcile(loaded profiles)`；
- `ProfilesActor` 持有 periodic reconcile task，仅 cast `ReconcileMaterializations`；actor mailbox 串行处理，不与 mutation 交错；
- reconcile 负责：完成已提交但未 complete 的 journal、补偿未提交/被 supersede 的 journal、重试 cleanup queue、隔离 malformed journal、sweep 无引用 artifact/tombstone。

#### Crate-internal degradation（非 S08 wire）

```rust
// crate-internal only; public MutationOutcome is exclusively S08
ProfileDegradation {
    phase: Cleanup | Reconcile,
    code: JournalInvalid | MaterializationDeferred | CleanupDeferred,
    message,
}
```

- 存储字段仅为 `phase` / `code` / `message`；**不**存储 `retryable`。
- 可重试性由 `ProfileDegradationCode::retryable()` 穷尽派生：`JournalInvalid → false`，`MaterializationDeferred | CleanupDeferred → true`。
- `CommitReport.degradations` 与 `MaterializationReconcileReport.degradations` 仍仅 actor/facade 内部；facade 映射到公共 `MutationOutcome`，actor 存储形状不变。

#### 验证要求

- deterministic failure injection：prepare 零提交、promote/compensate compound error、complete deferred、delete cleanup deferred；
- crash/journal fixture：state-promoting / file-promoted 中断后 complete、uncommitted file-prepared compensate、compensating phase 收尾；
- superseded state-first：`revision > journal` + active pre-promote → compensate never promote；
- cleanup fence：active path、hash reuse、already-absent、symlink no-follow；
- startup reconcile 先于 client ready；remote stale fence 保持；
- **import cancellation / restart / materialization（deterministic；barrier/oneshot，无 sleep）：**
  - happy path 一次 state-first 提交完整 remote profile + 真实 bytes；
  - fetch 失败 → 零 item / 零 file，且**无** delete compensation / 无 journal 触碰；
  - caller abort（成功 fetch 途中 / 失败 fetch 途中）→ 零 durable item；后续 import 仍可进行；
  - client drop / actor stop 期间 fetch 阻塞 → restart 后 items 仍空；
  - promote 失败 → 不留下已提交 import；
  - 显式 interval / pinned name 权威性；suggested interval 仅非显式时生效；
  - 零 interval 在 fetch 前 validation 拒绝。

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

落地要点：

- 终态 wire 仅 `applied` / `committed_degraded`；`RebuildOutcome` 已删除，无 `_v1` alias。
- create/import → `MutationOutcome<ProfileId>`；其余 profile mutation → `MutationOutcome<()>`。对于 `T = ()`，specta 仍生成稳定命名类型。
- facade `collect_post_commit_degradations` 合并：crate-internal profile degradations（内部 Cleanup/Reconcile 相位折叠为 public `ProfileMaterialization`；code 为 `journal_invalid` / `materialization_deferred` / `cleanup_deferred`）+ 影响 current 时的 post-commit rebuild degradation。
- **Runtime phase fidelity disposition：** 当前 rebuild 错误面是不透明 `Result`，统一映射粗粒度真实的 `RuntimeBuild` / `runtime_rebuild_failed`。**明确延期** RuntimeCheck/Promote/Publish/Apply 分相位精度，禁止伪精度；不属于 dispatcher/fake-core lane。
- **H1 retained-forward：** promote 后 compensating state CAS 失败 → `Ok(CommittedDegraded)` + 可恢复 forward head / real uid。
- **H2 auto-activation（create/import 共用 post-commit 协议）：** facade `try_auto_activate_if_none` / `set_current_if_none`；`Ok(None)` → Applied；hard error → `SystemEffect` / `profile_auto_activation_failed`，**保留**已提交 `ProfileId`。激活失败不得把成功 import/create 抹成 hard `Err`。
- **Import wire 终态：** `import_profile` 走 actor-owned fetch-before-commit（§6.10）；仅在一次 durable state-first 成功后才返回 `MutationOutcome<ProfileId>`。fetch/validation/cancel 失败 → 普通 `Err`（零 state/file），**不是** `CommittedDegraded`，也**不**依赖 placeholder delete。成功后的 materialization / rebuild / auto-activation 降级才进入 `committed_degraded`。
- 前端：`unwrapResult` 穷尽返回 `T`；`MutationCache` 在 success 路径识别 `committed_degraded` 并仍 invalidate；en/zh-cn/zh-tw/ru/ko 本地化 phase/code。
- Specta freeze + bindings freshness contract 拒绝 legacy status tag。
- **验证真相：** focused contract tests / build / clippy 通过；import cancellation + facade H1/H2 路径须 green。full workspace / multi-OS CI green 以 S10 证据包为准（local Q + tip CI + cleanup-tip **Q-18…Q-20**；权威 [`smoke-evidence.md`](./smoke-evidence.md)）。历史 `REGEN_BRIDGE` two-client isolation red contract 已关闭，不得再当作稳定 red；network 类 flaky 与 PR-5/6 residual global-state rebuild 失败须如实记录，不得伪装为未关闭的稳定化 blocker。

### 6.12 Rebuild dispatcher

最终落地：

- **删除** process-global `REGEN_BRIDGE` / `OnceCell` first-install-wins 与无界 channel；历史 “`REGEN_BRIDGE` red contract” 陈述作废。
- facade 内 **instance-owned** capacity-1 coalescing `RebuildCoordinator`（`mpsc` 容量 1 + receiver-side coalesce window；`try_send` 满则合并）。
- request/reply regeneration **直接**调用 typed facade 方法，不走进程静态 dispatcher。
- background worker 仅捕获 **`Weak`** client graph，upgrade 后调用 `rebuild_running_config()`；无 strong Arc cycle。
- **`shutdown().await`** 关闭 dirty path、等待 worker 退出；in-flight rebuild 允许完成；coalesce wait 可中断；post-shutdown dirty 为空操作。`Drop` 仅 best-effort。
- **生产 exit 集成**：`cleanup_processes` 从 Tauri managed state 取 `NyanpasuClient` 并在 core/widget teardown 前 `client.shutdown().await`。
- **隔离测试**：two-client graphs independent；clones share one coordinator；legacy call sites use supplied client；paused-time coalesce/shutdown tests（`tokio::time::pause/advance`）。
- **PR-5/6 residual**：shutdown **不**拆除 desired-state actors / `CoreManager::global()` / system proxy / OS resources；legacy `Config`/`CoreManager` 全局态仍非 full graph desired-state isolation。

### 6.13 S09 fake-core process matrix

- **test-only package** `backend/fake-core`（workspace member；tauri **dev-dependency**；`publish = false`；**never packaged** as production sidecar/resource）。
- **协议权威：** argv shapes、`FAKE_CORE_*` env、TCP READY/RELEASE barrier、HTTP apply 注入、binary discovery/prebuild 与 `ScopedChild` RAII 细节以 [`backend/fake-core/README.md`](../../../../backend/fake-core/README.md) 为准。设计层 disposition：
  - 使用 **real core argv**（check / mihomo / clash-rs / premium），**不是**早期 illustrative CLI flags（`--check-ok/--start-fail/--apply-fail` 等）；
  - 行为仅由 env 注入；parent/child 以 TCP READY/RELEASE 定序（禁止 sleep 作为主协议）；无 barrier 且无 immediate start-exit → fail-fast；
  - exact `PUT /configs` 与 `PATCH /configs` 状态注入；严格 env 解析；RAII child reap。
- **`cfg(test)` `ProcessCoreLifecycleAdapter`**：TempDir `RuntimePaths`；禁止 `CoreManager::global()` / real sidecar / 真实用户目录。
- **process matrix 要求**（focused 路径；**非** full workspace green 宣称）：
  - check fail → product/Promoted/Applied 不变；
  - immediate start fail → 无 child leak；
  - apply 500 after promote → Promoted 新、Applied 旧/None；
  - port hold conflict + stop 后释放；
  - lease serialization（barrier/oneshot，无 sleep）；
  - clean stop / ScopedChild reap；
  - two graph PID/port/path isolation；
  - process-level `change_core` new-start failure + old-core rollback success。
- **prebuild：** 跨 crate 须预构建 `fake-core`；discovery 与错误文案约定见 fake-core README（`NYANPASU_FAKE_CORE` → profile sibling → target path；`PREBUILD_COMMAND`）。
- **平台限制：** std-only 跨平台协议；Windows service-mode / TUN 权限路径属 S10 手工 smoke（E-09…E-11；执行权威 [`smoke-evidence.md`](./smoke-evidence.md)）。

---

## 7. Failure Matrix

| 操作               | 失败点                                   | Desired                  | Product/Promoted           | Applied                      | 返回                                                                   |
| ------------------ | ---------------------------------------- | ------------------------ | -------------------------- | ---------------------------- | ---------------------------------------------------------------------- |
| typed config patch | mirror prepare                           | 旧                       | 不变                       | 不变                         | Err，零提交                                                            |
| typed config patch | persistence                              | 旧                       | 不变                       | 不变                         | Err，零提交                                                            |
| typed config patch | post-commit effect                       | 新                       | 按需                       | 旧或新                       | CommittedDegraded                                                      |
| runtime rebuild    | build/check                              | 新                       | 旧                         | 旧                           | CommittedDegraded / legacy Err                                         |
| runtime rebuild    | promote                                  | 新                       | 旧                         | 旧                           | Degraded                                                               |
| runtime rebuild    | publish store                            | 新                       | 新产品；store 待 reconcile | 旧                           | Degraded                                                               |
| runtime apply      | core reject                              | 新                       | 新                         | 旧                           | CommittedDegraded(RuntimeApply)                                        |
| change_core        | 新核 build/check                         | 旧 core intent           | 旧                         | 旧                           | Err                                                                    |
| change_core        | 新核 restart，rollback 成功              | 旧                       | 旧                         | 旧                           | Err + rollback_succeeded                                               |
| change_core        | rollback rebuild 失败、文件恢复成功      | 旧                       | 恢复旧                     | 旧核成功后恢复旧             | compound Err                                                           |
| change_core        | 旧核也启动失败                           | 旧                       | 旧                         | 旧 snapshot + health stopped | compound Err / stopped                                                 |
| legacy 三域 patch  | 第二域 persist                           | compensation 后旧        | 不变                       | 不变                         | Err                                                                    |
| legacy 三域 patch  | compensation 失败                        | 部分                     | 不变                       | 不变                         | PartialCommit + critical degradation                                   |
| profile add        | stage/write                              | 旧                       | —                          | —                            | Err                                                                    |
| profile add        | finalize rename                          | 回滚旧                   | —                          | —                            | Err                                                                    |
| remote **import**  | validate / fetch 失败                    | 旧（零新增）             | —                          | —                            | `ImportFailed` / validation Err；零 state/file；无 delete compensation |
| remote **import**  | caller cancel before durable commit      | 旧（零新增）             | —                          | —                            | 丢弃 fetch；零 state/file                                              |
| remote **import**  | state-first promote 失败且 rollback 成功 | 旧                       | —                          | —                            | Err；不留下完整/空壳 import                                            |
| remote **import**  | post-commit auto-activation 失败         | 新 profile 已提交        | —                          | —                            | `CommittedDegraded` + 保留 ProfileId                                   |
| remote refresh     | metadata persist                         | 旧 metadata，恢复旧 file | —                          | —                            | Err/compound（file-first；非 import）                                  |
| delete             | file cleanup                             | 已删除                   | —                          | —                            | CommittedDegraded + cleanup pending                                    |

---

## 8. 回归测试设计

### 8.1 PR-2 actor / bridge

- mirror prepare 转换失败：manager version/state 不变；
- persist 成功后 apply 不可能返回错误；
- three-domain saga 第二/第三域失败；
- typed concurrent mutation 导致 version conflict，saga 不覆盖新值；
- compensation 自身失败返回 PartialCommit。

验证覆盖：prepared-mirror zero-commit、manager CAS monotonic/conflict/persistence failure、第二/第三域失败与逆序补偿、concurrent typed update conflict、finalizer/legacy-state uncertainty 和 structured `PartialCommit` 字段均须有 deterministic tests。并发交错使用 oneshot/mpsc channel 与 release barrier，不使用 sleep。

### 8.2 PR-3 profiles

固化以下 regression fixtures：

- legacy migration 保留 IPv6（#4893）；
- local profile import wire（#4916）；
- remote source / options / subscription wire（#4917、#4920）；
- profile title/default interval 与显式 interval；
- add 初始文件失败不留下 profile；
- **remote import fetch-before-commit**（取代 `add empty placeholder → refresh → delete`）：fetch 失败 / cancel 零 state/file；成功一次 state-first + 真实 bytes；无 placeholder delete compensation；
- refresh metadata persist 失败恢复旧文件（**file-first；manual/scheduled only**）；
- delete cleanup 失败进入 cleanup queue；
- symlink/reparse point 拒绝写入。

materialization 验证要求（deterministic，无 sleep）：

- state-first / file-first prepare → promote → complete / compensate；
- crash journal recovery（state-promoting、file-promoted、uncommitted file-prepared、compensating phase）；
- superseded state-first：`revision > journal` + active pre-promote 只 compensate、不 promote；
- cleanup fence（active path、hash reuse、already-absent、symlink no-follow）；
- startup + periodic reconcile；malformed journal isolation；
- crate-internal `ProfileDegradation` 字段断言；公共 `MutationOutcome` 映射；
- import cancellation / restart / materialization contracts（见 §6.10 验证列表）。

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

早期 illustrative CLI flags（`--check-ok/--start-fail/--apply-fail` 等）**不是**最终协议。最终 disposition（test-only；never production packaged）：

- **real core argv shapes**（check / mihomo start / clash-rs start / premium start）；
- **`FAKE_CORE_*` env** 控制 check/start exit、stdout/stderr、hold/http ports、apply HTTP status/body、exit-after-release；
- **TCP READY/RELEASE** 同步（非 sleep、非文件 barrier 作为主协议）；
- 动态 hold/http 端口（含 ephemeral `0` + READY 回报）；
- exact `PUT /configs` / `PATCH /configs` 状态注入；
- `ScopedChild` + parent-owned `ReadyBarrier`；
- prebuild / cross-crate discovery：见 [`backend/fake-core/README.md`](../../../../backend/fake-core/README.md)。

用于验证真实进程边界与 lifecycle lease/path isolation，而不是只验证 mock 调用顺序。S10 三平台 smoke 仍独立于本 matrix。

---

## 9. 前端与 wire

- `MutationOutcome<T>` 由 specta 生成并冻结；
- `MutationCache` 仅识别 `committed_degraded`，且仍走 mutation success / query invalidation；
- toast 展示本地化 phase/code，详细 message 写日志/可展开；
- profile warnings 不再只记录 tracing；
- create/import/update/delete/reorder/activate/patch 均返回统一 wrapper；
- `unwrapResult` 穷尽返回 `T`，wire drift 不坍缩为 `undefined`；
- TS binding shape 有逐字 contract assertion（含 `RebuildOutcome` 删除）；
- en/zh-cn/zh-tw/ru/ko 五语 phase/code 已落地。

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
  - 删除 profile staging leftovers / unreferenced materialization artifacts；
  - 按 durable `Profiles.revision` 恢复 materialization journal 与 delete cleanup queue；
  - 校验 product hash 与 promoted snapshot；若内存 store 初始为空，从可信产品重建只读 snapshot 或等待首次 rebuild，不读取 unchecked 文件；
- crash 发生在 state commit/file finalize 之间时，下一次启动与 periodic reconcile 依据 journal/queue 恢复。

profile journal 只保存路径、operation id、durable `Profiles.revision` 和 hash，不保存敏感完整内容；旧字节备份使用 owner-only 临时文件并在完成后删除。

**Superseded state-first 恢复（已修正）：** 若 committed `Profiles.revision > journal.revision` 且 active target 仍为 pre-promote 备份字节，reconcile 必须 compensate 并丢弃 staged forward content，**不得 promote** 到更新的 committed revision。

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

必须覆盖并记录（映射到 canonical `E-xxx`；执行权威 [`smoke-evidence.md`](./smoke-evidence.md)）：

1. 首次启动 + 删除产品后的 fallback（E-01）；
2. profile 切换 + mode/allow-lan/ipv6（E-02）；
3. mixed-port fixed/random 与即时生效（E-07）；
4. mihomo ↔ clash-rs 成功换核（E-03）；
5. 新核二进制故障时硬回滚（E-04）；
6. remote-dependent profile 断网后的 committed-degraded（E-05）；
7. patch rebuild 失败后的 Applied-based compensation（E-06）；
8. local/remote/composition profile 创建、导入、刷新和删除（E-08）；
9. Windows service mode（E-09）；
10. macOS/Linux TUN 权限路径（E-10 / E-11）。

**规则：** 未记录视为未执行。Maintainer attestation（GPG-signed 仓库记录 / 本仓库 durable `smoke-evidence.md` 条目）可作为执行证据；raw commit/build/os/app/core/log 未保留时必须显式 `not captured`，不得伪造 artifact。

**当前执行状态（非本文件 closeout 日记；权威 [`smoke-evidence.md`](./smoke-evidence.md)）：** maintainer `4o3F` 于 2026-07-18 证明 E-01…E-11 **全部 PASS**；raw fields **not captured**。连同 Path A、target-tip CI、cleanup-tip CI **Q-18…Q-20 SUCCESS** @ `8909566c…`、residual ledger 与 local QA，S10 / PR-4S 稳定化门 **COMPLETE**；**PR-5a unlocked**。

### 13.3 文档

- v3 roadmap / task 状态：PR-4S 稳定化门 **COMPLETE**；**PR-5a unlocked**（权威进度以 [`task.md`](./task.md) 为准；**不**宣称 actor migration 或 residual 清零）；
- PR-4 四个 review finding 逐条 disposition（[`review-disposition.md`](./review-disposition.md) Path A）；
- PR-4 五项 smoke + §13.2 证据链接（[`smoke-evidence.md`](./smoke-evidence.md)；E-01…E-11 PASS (maintainer-attested)；cleanup-tip **Q-18…Q-20**）；
- `TODO(actor-migration)` residual ledger 已文档化（[`residual-ledger.md`](./residual-ledger.md)；**残差仍属 PR-5/6/7**）；
- 所有延期项有负责 PR 和删除条件（见 residual ledger；**未**清零）。

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

PR-4S 完成后（**稳定化门已达成 COMPLETE**；下列为门的成功语义，**不含** actor migration 终态或 residual 清零）：

- PR-1～PR-4 不再存在已确认的 correctness blocker；
- 产品文件、Promoted、Applied 和 selected core 在所有已建模失败分支保持一致或显式 degraded；
- typed state 与 legacy mirror 不会产生“已提交但普通 Err”；
- legacy 三域 patch 不会静默部分提交；
- profile 状态/文件有恢复协议；
- 测试图完全隔离（process-global rebuild dispatcher 已移除）；
- 已发生回归被 contract suite 固化；
- **PR-5a unlocked** — PR-5 可以只关注 ownership migration，而不同时修补 PR-4 的事务漏洞；
- PR-5/6/7 residual（legacy `Config` / `CoreManager` globals 等）**仍然存在**，见 [`residual-ledger.md`](./residual-ledger.md)。
