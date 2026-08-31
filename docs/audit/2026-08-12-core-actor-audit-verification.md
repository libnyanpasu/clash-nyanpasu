# CoreActor 迁移方向审计复核报告

- **日期**：2026-08-12
- **复核对象**：外部审计结论（ChatGPT 会话 `chatgpt.com/share/6a7c67e6-4cb4-83ea-8a2e-f675949c4487`，审计基线 `refactor/core-manager-actor` @ `2a247cca`）
- **复核基线**：同一提交 `2a247cca248cf0d4ec9d3e3b46bf7ead9118c200`；submodule `nyanpasu-runtime` @ v2.0.0-rc.1
- **关联输入**：
  - `docs/design/2026-08-08-core-manager-control-plane-runtime-backend-design.md`（控制面目标架构，用户指定的迭代方向）
  - `docs/superpowers/plans/2026-08-04-pr5d-v8-pr5e-v2-review-findings.md`（PR-5d v8 / PR-5e v2 对抗审结果）
- **用户裁定（2026-08-12）**：认同审计对当前设计的驳斥；BC 不是约束；CoreManager 须处理一切边界情况；每个操作事务化；check→change 改为直接 change，check 由 CoreManager 内部触发，失败经 `error_kind` 返回。
- **结论**：**审计的全部关键事实断言经逐条源码核实成立**；三个 P0 结构性指控成立；另有两处审计未写到的加重情节。方向按审计 + 2026-08-08 设计文档执行，app 侧集成设计见 `docs/design/2026-08-12-core-actor-v2-app-integration.md`。

---

## 1. 事实复核（逐条对源码，全部在 `2a247cca`）

| #    | 审计断言                                                                                          | 结果               | 证据（file:line）                                                                                                                                                                                                   |
| ---- | ------------------------------------------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0-1 | `replace_backend` 先发布 synthetic `Stopped`、后停旧核，且无论旧核停没停成都安装新 backend        | **属实**           | `backend/tauri/src/core/actor/mod.rs:266-306`：`take()` → `running = None` → `commit(synthetic_stopped)` → 旧 `shutdown().err()` 仅存入变量 → `CoreBackend::new` 安装新 backend → **装完之后**才返回 shutdown_error |
| P0-2 | `Shutdown` 发布假 `Stopped`、吞掉停失败、向调用方回 `Ok(())`                                      | **属实**           | `actor/mod.rs:603-614`：`commit(synthetic_stopped)` 在 `backend.shutdown().await` **之前**；失败仅 `tracing::warn!`；`client/core.rs:277-283` 的 `let _ =` 再吞一层                                                 |
| P0-3 | 运行模式切换是 `set_backend` + `run` 两条消息，非原子事务                                         | **属实**           | `core/actor/request.rs:88` 与 `:90` 是两次独立的守卫校验 RPC；`OperationGuard` 只防插队，不提供跨消息原子性                                                                                                         |
| 2.1  | 三重串行化（gate → mailbox → manager mutex）                                                      | **属实**           | `core/actor/gate.rs`（`OperationGate`）→ ractor mailbox → submodule `nyanpasu-core-manager/src/manager/mod.rs:107`（`ctrl: tokio::sync::Mutex<Ctrl>`）                                                              |
| 2.2  | `OperationId` 被实现为跨调用租约（Acquire/Release/validate）                                      | **属实**           | 5a 设计即如此（`AcquireOperation`/`ReleaseOperation`/`validate_operation`，`actor/mod.rs:185-190`）                                                                                                                 |
| 2.3  | Service 被建模为 `RuntimeBackend` 变体；GUI 侧 `ServiceBackend::run` 自行编排 status→(stop)→start | **属实**           | `core/actor/backend.rs:70-74`（`CoreBackend::{Local, Service, Test}`）；`:277-289`（status → 条件 stop → `start_core`）                                                                                             |
| 2.4  | GUI 维护第二份 lifecycle truth；`(None, Running)` 可构造                                          | **属实**           | `pre_start` 将 `observe_status()`（可为 Running）写入 `observed`，但 state 以 `running: None` 构造（`actor/mod.rs:411`）——`running` 是"GUI 发过什么命令"的影子，不是事实                                            |
| 2.5  | `RefreshHint`/`hint_pending` 是状态所有权错位的补丁                                               | **属实**           | `client/core.rs`（`hint_refresh` + `hint_pending: AtomicBool`）                                                                                                                                                     |
| 2.6  | GUI transport retry：5 次 × 250ms 盲重试 apply                                                    | **属实，数字精确** | `client/core.rs:549-565`：`for attempt in 0..5` + `attempt < 4` + `sleep(250ms)`；transport error 无法证明服务端未执行 → 重发业务操作可产生 duplicate/revision conflict                                             |
| 3    | legacy DNS 以内存回滚假设"写 Err ⇒ 外部无变化"                                                    | **属实，精确**     | `core/clash/core.rs:61`（`previous_dns: Mutex<Option<Vec<IpAddr>>>`）；`:121-122`（写失败即 `*previous_dns = previous_dns_clone; return Err`）——直接违反 PR-5e §0 第一原则                                          |

审计自述无法读取 findings 文档（属实：该文档只在工作区、未提交），其"存活 BLOCKING 集中在 C3/接缝"的推断与文档相符（5d 五条 BLOCKING 中四条 C3 耦合）。一处未独立核验：`cleanup_processes()` 无条件继续退出——不影响任何结论。

### 1.1 复核补充的两处加重情节（审计未写到）

1. **`replace_backend` 在新 backend `observe_status()` 失败分支（`actor/mod.rs:302`）把 shutdown_error 整个丢弃**——旧核没停成这个事实连返回值都进不去。
2. **跨 owner 双核场景没有任何隔离语义**：submodule 的 quarantine（`manager/mod.rs:321` `reject_quarantine`）只护住单个 manager 内部。Local→Service 切换时旧 Local 核停不掉 + Service 起新核 = **双核并存，且没有任何状态能表达它**。这是"Service 是另一个控制器、不是 backend 变体"的最硬证据。

---

## 2. 设计结论评估

审计的目标架构与 2026-08-08 设计文档同构（`CoreControl` / `ControlExecutor` / `CoreOrchestrator` / `RuntimeBackend`；`OperationId` = 幂等身份而非租约；D6 executor 拥有事务）。审计**超出**文档的三项新增全部成立且必要：

1. **`CoreEndpointRouter` + `ControllerGeneration` handoff**——文档只覆盖单 host 内部；Local↔Service 是跨控制器所有权转移（§1.1 第 2 条正是它要解的问题）；
2. **`MacosDnsController` 挂进 orchestrator 的固定阶段**——DNS 与生命周期进入同一事务，PR-5d/5e 的 S1–S4 接缝无物可依；
3. **结构化 `ShutdownReport`**——直接治 P0-2 的吞错。

两处与文档冲突，按"BC 不是问题"裁定修订（已写入设计文档修订记录）：

- 文档 G7 / §19.2 / §27 的 v1 wire 兼容与 legacy adapter 保真 → **只保留协议版本 fail-closed 门**（PR-5-pre 已建），不做 v1 行为仿真；
- 文档 §27.1 的 `CoreManager` facade 兼容包装 → 删除，直接切 `CoreControlHandle`。

**DNS 机制收口**：审计的 `DnsOverrideRecord` 仍是值比较归属，但把所有权/寿命修对了（持久化记录 + owner generation + host 启动 orphan reconcile）。机制层是否换 scutil `State:` 键（结构性归属，restore = 删自己的键）是 `MacosDnsController` 内部实现选择，接口不变，留作该组件的 Phase-0 spike，不阻塞架构。

---

## 3. 用户指令的规范化：直接 change、check 内部化、error_kind 返回

此指令修掉了旧协议一个未被点破的 TOCTOU：caller 侧 check→change 两步之间世界可变，check 结论到 change 执行时已过期。check 收进事务内后，校验的就是**本次事务将要提交的那份 bytes/artifact**。

### 3.1 事务信封（单一 mutating 命令）

```text
Reconcile{ operation_id, intent_revision, artifact, config_bytes+digest,
           expected_applied, effects, force_restart }

① admission        closing? → ShuttingDown        队列满 → QueueFull
② idempotency      同 id+digest → 返回原 operation   同 id 异 payload → OperationConflict
③ CAS              expected_applied ≠ current → RevisionConflict
④ 内部 check       解析失败 → InvalidConfig        语义/dry-run 失败 → ConfigCheckFailed
   ―― 以上零副作用，Err 即干净中止 ――
⑤ stage artifact   提交 runtime 产物（可回滚点）
⑥ classify         Noop / Patch / Reload / Restart / Switch（内部决策，不再是调用方协议）
⑦ execute          停旧必须 StopProof；拿不到 → StopUnconfirmed + SafetyState::Quarantined
⑧ verify           read-back 观测态
⑨ fallback/rollback 失败降级重启；回滚失败 → RollbackFailed（标安全影响）
⑩ publish          原子发布 revision + status → ApplyOutcome（RolledBack 是成功完成的事务）
```

- 独立 `Check` 命令保留但**降格为咨询**：只读、semaphore 限并发、不进 mutating 队列，永远不是 change 的前置门。
- 调用方对 `ConfigCheckFailed` / `InvalidConfig` / `RevisionConflict` 按 kind 分支——落在 R0（nyanpasu-runtime PR #390）已铺好的 typed `CoreErrorKind` wire 上。**#390 合并 + submodule pin 移动由此成为本方向的硬前置。**

### 3.2 边界情况清单（CoreManager 必须处理，逐条固化为 contract test）

| 边界                    | 事务内处置                                                      | 治的旧病                    |
| ----------------------- | --------------------------------------------------------------- | --------------------------- |
| caller 取消/断线 mid-op | executor 拥有 task，跑到终态或补偿完                            | `OperationGuard` 租约全家   |
| 响应丢失但已执行        | 同 id 重查 registry，不重执行                                   | 5×250ms 盲重试              |
| 停旧不可确认            | `Quarantined`，禁起冲突实例，绝不假 `Stopped`                   | P0-1 / P0-2                 |
| 跨 host 切换中途死亡    | handoff 协议 + generation fencing，唯一 owner 或显式 quarantine | 双核并存                    |
| check 失败              | ④ 干净中止，`error_kind` 返回                                   | check/change TOCTOU         |
| 并发修改                | ③ CAS → `RevisionConflict`                                      | revision 冲突静默覆盖       |
| shutdown 与 apply 并发  | closing latch 拒新 + 在飞事务到安全点                           | `ControlAdmission`/选举全家 |
| daemon 重启丢 registry  | status 为事实源，client 重读（第一阶段接受）                    | —                           |
| stale runtime 事件      | epoch/generation 过滤，不污染新 owner                           | shadow state                |

---

## 4. 资产处置

审计 §八删除表与 §九保留表全部认可，无修订。要点：

- **删除**（app 侧）：`OperationGate` / `AcquireOperation`-`ReleaseOperation` / `CoreOperationGuard` / `CoreLifecyclePort`-`Lease` / GUI `CoreActor` 生命周期所有权 / `CoreBackend::{Local,Service}` / `BackendSlot`-`replace_backend`-`SetBackend` / `running` shadow / `CoreStatusView`+`FaithfulLifecycle` 双模型 / `RefreshStatus`-`RefreshHint`-`hint_pending` / `PublishPromoted`-`PublishApplied` / transport retry / legacy DNS singleton / `RunType::default()` / S1–S4 接缝 / 旧 wire 兼容桥。
- **保留并下沉**：epoch-revision-apply 分类-rollback-quarantine（→ `CoreOrchestrator`）、`stop_and_confirm_dead`-PID reaper-runtime store（→ `ProcessRuntimeBackend`）、`CoreErrorKind`（→ portable model）、协议 fail-closed 门（→ RPC adapter）、fake-core/barrier/failure-injection（→ backend contract tests）、Desired/Applied 分离与 `CommittedDegraded`（→ canonical status）、PR-5d/5e failure matrix（→ 验收测试）。
- 5c 中与 actor 形态无关的部分（ledger 扫描器三态化、`Logger` global 删除）可独立摘取。

**实施顺序**：runtime 仓先行（PR-A 控制面内聚 → PR-B service 成为 CoreControl RPC host），app 仓 lockstep 一次性切（PR-C），随后 PR-D（handoff + DNS）、PR-E（清算）。不做双轨迁移——#5070 已 CONFLICTING、栈上 CI 从未跑过、BC 不设限，双轨的全部理由都不存在。

---

## 5. nyanpasu-service 修改盘点与 revert 评估

**改动全集**：对 service（`nyanpasu-runtime` 仓）的全部修改只有 **R0 一项** = PR [#390](https://github.com/libnyanpasu/nyanpasu-runtime/pull/390)（OPEN，未合并），5 提交、9 文件：

| 文件                                                              | 内容                                                         |
| ----------------------------------------------------------------- | ------------------------------------------------------------ |
| `nyanpasu-core-metadata/src/error_kind.rs` (+152)                 | `CoreErrorKind` 单一 wire 表                                 |
| `nyanpasu-core-manager/src/error.rs` (+142)                       | `Error::kind()` 类型化分类                                   |
| `nyanpasu-service-runtime/src/server/manager_bridge.rs` (+10/−86) | service 从 manager 读 kind，删除自维护字符串表               |
| `nyanpasu_ipc/src/api/mod.rs` (+19/−45)                           | 字符串常量 → typed enum（serde 值不变，v2 wire golden 全绿） |
| `nyanpasu_ipc/src/client/mod.rs` (+57/−2)                         | client error 暴露 typed kind                                 |
| 测试 ×2                                                           | roundtrip / wire golden                                      |

纯错误协议收敛，**零生命周期行为变化、零 wire 值变化**；未合并，对任何已发布产物零影响；父仓 submodule pin 从未被移动。

**授权轨迹**：R0 属于 2026-08-01 定稿、经用户批准的 PR-5 规格（roadmap §6.R0 且明写"未经用户显式授权不得 push"）；push + 开 PR 于 2026-08-02 经用户单独授权。若"上游仓库改动需先做内容级专项确认"应成为常驻规则，请明示，将记录为标准流程。

**Revert 评估：不值得，且方向相反。** R0 是 PR-5 全部产出中**唯一被新架构原样消费**的部分：审计 §九保留表将 `CoreErrorKind` 列为 portable control model 资产；2026-08-08 文档 §25 的错误模型直接扩展它；用户"check 出错经 error_kind 判断"的指令**依赖**它。revert 等于回到字符串嗅探，再在 PR-A 里重做一遍同样的事。正确处置是**合并 #390 并 bump pin**——它已从"暂缓"升格为新方向的硬前置。

---

## 6. 未决项（需用户裁定）

1. PR 栈 #5070–#5074 处置：按审计降级为历史证据关闭，或先摘取可独立成立部分；
2. **#390 合并 + submodule pin 移动授权**（硬前置）；
3. 新方向 spec 是否替代 5d/5e 计划的槽位地位（本报告与两份设计文档即为基础）；
4. Service 模式下 app 退出是否停核（现行为停核；新设计默认保留，见 app 集成设计 OQ-2）；
5. 5d/5e 计划与 findings 文档归档方式；roadmap 中 🟡 规划中章节已同步改判（见 `actor-migration-roadmap.md` §6.5）。
