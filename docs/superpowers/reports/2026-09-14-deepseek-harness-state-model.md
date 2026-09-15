# DeepSeek Harness 状态管理模型分析与对 Nyanpasu 事务化的启示

日期：2026-09-14
分析对象：`deepseek-ai/deepseek-harness` master（2026-09-14 浅克隆；引用格式 `包路径:行号`）
关联计划：`docs/superpowers/plans/2026-09-14-application-workflow-tcc.md`（下称「计划 v1」）
参考对话：ChatGPT「解释状态事务实现」两轮（结论已逐条与源码核对；核对差异见 §1.7）

## 0. 结论

Harness 没有「横跨状态、外部进程、UI 的大事务」。它把问题切成四层，各层用不同的一致性手段：

| 层                          | 机制                                                                                     | 一致性保证                                                 |
| --------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| 权威状态                    | 每会话一个 append-only 事件日志，`Session.append` 是唯一提交点                           | 先校验、再入日志、再通知；入日志即已提交，观察者失败不回滚 |
| 应用语义事务（Agent 创建）  | 私有未发布世界 → `setup()` → 同步 `commit()` 复验 → 落盘 → 同步 `publish()`              | 发布前观察者看不到半成品；失败用记忆化的逆序 disposer 撤销 |
| 外部副作用（工具执行）      | 先记录意图 `tool/call`，执行，再记录结果 `tool/result`；未知结果有命名状态并可确定性修复 | 不回退外部世界，把「不确定」写进日志                       |
| 派生视图（UI / projection） | 纯同步 fold + `asOfSeq` 水位；客户端 higher-seq-wins                                     | 迟到帧永远赢不了较新值                                     |

对 Nyanpasu 最有分量的三条启示：

1. **Prepare 要重，Commit 要轻。** 所有不触碰外部世界就能失败的事（构建、序列化、校验、核心 dry-run）放在提交前；提交点本身接近不可失败。
2. **外部世界不是事务参与者，而是「意图先写、结果后写」的 reconcile 对象。** 期望值与已应用值是两条记录；差距靠 reconcile 收敛；不确定结果要命名并有确定性修复路径。
3. **提交与通知的同步边界只在单线程事件循环里成立。** 进程重载、OS 代理、快捷键注册天然异步，必须在提交之后 spawn，不能在提交点内 await。

这三条与计划 v1「在 `on_prepare` 内部 await runtime reload 与 OS 效果、失败后恢复 checkpoint」的设计直接冲突。§3 给出修订建议（方案 C），§4 给出计划的具体改动清单。

## 1. Harness 的状态模型（源码核对）

### 1.1 权威状态：一个校验过的同步提交点

- `packages/core/session/src/index.ts:710-761` `append()`：深拷贝并拒绝不可 JSON 化的数据（`:720`）→ 重入守卫 `if (entry?.appending) throw`（`:728-731`）→ 构造并 `deepFreeze` 事件（`:732-738`）→ `validateSessionEventData` + `surfaceManager.validateNext`（`:739-740`）→ `log.push`（`:749`）→ 通知观察者（`:752`）。
- 文档注释（`:676-679`）：「Once the event enters the log, the append is committed: observer failures are logged and contained per listener」。
- `seq = log.length`（`:668-671`），位置即序号；热路径不做 IO，「persistence plugins buffer asynchronously」（`:675-676`）。
- 「Model-visible means logged」不是口号：`packages/core/agent-loop/src/invariant.ts:22-56` 在每次模型请求前把 `messages` 与 `session.deriveMessages()` 重新推导比对，不一致即失败。

### 1.2 应用语义事务：未发布世界 → 同步提交 → 发布

`packages/core/agent-loop/src/index.ts:805-836` `setupAndPublish`：

```text
prepare()                       // 私有 agentCtx / Session / Agent，任何注册表都还没有它
await raceAbort(setup(...))     // 异步组装；调用者取消 / owner 卸载 / 工厂关闭三路 abort 融合成一个信号（:558-567）
setupCommit?.commit()           // 同步，最后一次可以拒绝（packages/core/agent/src/index.ts:36-42）
await appendUnstoredSuffix()    // 落盘在发布之前（:749-757）
prepared.publish(source)        // 同步，无 await：enter 注册表 → announce → session-start（:662-677）
catch → prepared.dispose()      // 记忆化、逆序、失败聚合成 AggregateError（:576-616）
```

- 契约（`packages/core/agent/src/index.ts:101-117`）：setup 在「inserting or announcing either the session or agent」之前运行，「so observers can never see a partially configured world」；失败「rolls the scope back without publishing either id」。
- `publish()` 同步且每步之间 `assertLive()` 复查（`:662-677`）；观察者一旦被通知就不可撤回，之后的生命周期用正向事件 `agent/disposed` / `session/disposed` 关闭，不是撤回。
- `createStoredSession` 只取写所有权不写内容（`:721-725`）：失败的 setup「leaves no stored residue — the same id can be created again」。

### 1.3 资源事务：disposer 而不是快照恢复

- `vendor/cordis/src/fiber.ts:402-442` `effect()`：disposer 在产生时登记，逆序执行，幂等；fiber 卸载或显式 dispose「whichever comes first」。
- 生成器 effect（`:76-93`, `:356-399`）逐个 `yield` disposer；异步生成器每轮检查 epoch（`:390`），重载会放弃半途的 effect。
- `AgentLoop.prepare` 正是这样用的（`packages/core/agent-loop/src/index.ts:623-631`）：`yield machine.scope.rawDispose; yield () => dispose(true)`。
- 卸载时 disposer 抛错被记录并包含，不打断其余清理（`fiber.ts:683-685`）。

### 1.4 外部副作用：意图先写、结果后写、未知结果有名字

- `packages/core/agent-loop/src/tool-calls.ts:165-197` `startCall`：`:168` 先 `appendToolCall`（写意图）→ `:170` 调度器 `prepare` → `:174` `dispatch`（执行）；`commitReady()`（`:147-161`）之后按模型顺序 `appendToolResult`（写结果）。并发执行，串行提交。
- 未知结果被命名：`packages/core/session/src/repair.ts:15` `TOOL_NOT_STARTED`、`:18` `TOOL_OUTCOME_UNKNOWN`；`interruptedTurnClosers`（`:29`）合成错误结果、`step/end`、interrupted `turn/end`，「sequences continue the log」——修复是追加，不是改写。
- 派发前取消也要落日志：`appendSkippedToolCall`（`:250-260`）同时写 `tool/call` 与 `isError` 的 `tool/result`，保证转录无缝。
- 只有一处是「提交后才执行」：模型请求。`docs/architecture.md:109`：「cancellation during either async phase commits neither」。

### 1.5 派生视图：水位与 higher-seq-wins

- `packages/session/session-projection/src/index.ts:22-67`：projection 单元是纯同步 fold，`apply(state, event)`；不关心的事件必须返回同一引用（`:63-66`），`Object.is` 相同即零下游工作（`:646`, `:682-691`）。
- `ProjectionSnapshot { asOfSeq, values }`（`:108-115`）：一次快照里所有值对应同一日志位置；`snapshot()` 必须同步，否则「would tear the carriers' consistency cut」（`:16-20`）。
- 跨 seq 缺口是致命错误（`:643-645`），不静默跳过。
- 客户端 `packages/api/session-controller/src/client/sessions/projection-store.ts:134-139`：`if (seq <= row.seq) return  // higher seq wins; replays and stale frames drop`；`truncate`（`:169-175`）是「宿主丢失未持久化状态」的显式栅栏。
- `packages/client/ui-settings/src/client/settings-scope.ts:146`：「superseded failures leave recovery to it」——只有最新一次写入拥有恢复权。

### 1.6 并发、取消、不确定

- 单写者三层：`append` 拒绝重入（`session/src/index.ts:729-731`）；进程内写所有权 = handle（`agent-loop/src/index.ts:877-879`）；跨进程内核写租约（`session-persistence-jsonl/src/storage.ts:352-354`）。持久化操作串行在一条不会被毒化的 promise 链上（`:357-370`）。
- 取消 ≠ 完成：`raceAbort`（`:150-163`）只让调用者停等；`raceAbortCall(..., releaseAbandoned)`（`:166-187`）在调用者放弃后仍释放迟到的资源（`:776-783` 关闭迟到创建的 handle）。
- 工厂 `dispose()` 等待所有活动 agent 与在途创建（`:138-146`）；每个 agent 的销毁是「disposed-cause cancel followed by quiescence」（`:592-594`）。
- 持久层：`flush` 是唯一的耐久屏障（`session-persistence/src/handle.ts:99-109`）；游标只在 `persistBatch` 成功后推进（`storage.ts:338-342`）；撕裂尾修复的每一步只在自己落盘后清状态，失败可重试（`:324-337`）；读到日志缩短是硬错误（`:171-175`）。

### 1.7 与 ChatGPT 结论的核对差异

| ChatGPT 说法                                                                                | 源码                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `session-persistence-sqlite` 用 `BEGIN/INSERT/COMMIT` 批量落盘                              | **不存在**。会话存储是 `session-persistence-jsonl`；sqlite 只用于 `storage-sqlite`（KV，projection 缓存）与 `session-query-sqlite`（查询索引）。耐久性靠 `flush` 屏障，不是 SQL 事务 |
| 「外部副作用只在提交后执行」                                                                | 更准确的是「意图先提交，执行，结果后提交」（§1.4）；只有模型请求是严格提交后执行                                                                                                     |
| Harness 对 Nyanpasu 的建议：Workflow 是 coordinator 而非 participant，GUI/OS 全部 reconcile | 方向与源码一致；但 Harness 自身对「不可确认」的处理（命名 + 确定性修复）在 ChatGPT 回答里没有展开，这恰好是 Nyanpasu 最缺的一环                                                      |

## 2. 哪些可迁移、哪些不可迁移

### 2.1 可迁移（13 条，按对我们的价值排序）

| #   | Harness 机制                                                                  | 映射到 Nyanpasu                                                                                                                                                   |
| --- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Prepare 重、publish 轻（`setupAndPublish`）                                   | 构建 runtime 快照、序列化、校验、`/core/check` dry-run、快捷键/端口/PAC URL 预检全部在提交前；提交 = state CAS + 文件原子替换                                     |
| 2   | 提交点内不 await 外部世界（`publish()` 无 await；projection 必须同步）        | 配置 actor 的 `handle` 内只做校验与提交；runtime/OS 效果在提交后由 workflow 的 tracked task 执行                                                                  |
| 3   | 意图先写、结果后写（`tool/call` → `tool/result`）                             | `desired_revision`（提交即写）与 `applied_revision`（应用成功才写）是两条记录；`RuntimeLifecycleState.applied` 与 `EffectStatus.applied_revision` 已是这个形状    |
| 4   | 未知结果命名 + 确定性修复（`TOOL_OUTCOME_UNKNOWN`, `interruptedTurnClosers`） | `RecoveryRequired` 是一等状态：启动时「上次退出的 OS 代理状态未知」→ 显式 full reconcile；runtime `outcome_uncertain` → 命名状态 + 显式 Recover，而不是靠重启清零 |
| 5   | higher-seq-wins / superseded / 只有最新写入拥有恢复权                         | owner 侧 `Superseded`（PR-6 已有）；workflow 的 reconcile 只追最新 `desired_revision`；旧失败不重放                                                               |
| 6   | 一个异步操作 = 一个生命周期控制器（`packages/AGENTS.md:9`）                   | #5250 的 tracked task + `OperationId` + `uncertain` 闩，原样保留                                                                                                  |
| 7   | 取消 ≠ 完成；释放迟到结果（`raceAbortCall`）                                  | `CALL_WAIT` 超时不放行准入（已有）；迟到出现的核心进程仍要被 quarantine/recover（runtime 已有）                                                                   |
| 8   | 整值不发增量（projection doc `:70`）                                          | `ApplicationEffect` 携带完整 desired（PR-6 已有）；扩展到 runtime apply 状态事件                                                                                  |
| 9   | 引用相等的变更门（fold 返回同一引用即零工作）                                 | `struct-patch` diff 为空 → 不触 runtime、不触 OS；但 `applied < desired` 时必须无视 diff 重新 apply（修 retry 空洞）                                              |
| 10  | 观察者失败被包含，不回滚提交（`append` `:676-679`）                           | 提交后 UI/tray/logger 失败 = `CommittedDegraded`，永不回滚配置                                                                                                    |
| 11  | disposer 在做事的地方登记（`ctx.effect`）                                     | 需要撤销的只剩「提交前的临时资源」：暂存的 profile 内容、临时 check 文件、候选端口探测                                                                            |
| 12  | 单写者 + 不可重入 + 不会被毒化的串行链                                        | 配置 actor 单写者（已有）；workflow 执行域串行（已有）；两者之间不允许同步环（计划 v1 §2.1 规则保留）                                                             |
| 13  | 落盘游标只在成功后推进；修复可重试                                            | `PersistentStateManager` 的 CAS 已是这个形状；profile journal 的 compensate/reconcile 已是这个形状                                                                |

### 2.2 不可迁移

| Harness                            | 为什么不搬                                                                                                                       |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| 事件日志作为权威配置存储           | 配置域是长寿命 last-write-wins 文档；重放历史编辑没有收益且无界增长。我们已有版本化快照 + MVCC + `Epoch(NonZeroU64)`             |
| Cordis fiber / HMR / 生成器 effect | 没有运行时插件热插拔；留下的只是「在做事的地方登记撤销」这一条                                                                   |
| 同步的提交即通知边界               | 依赖单线程事件循环。ractor 给了串行化，但 OS 代理、进程 spawn、快捷键注册天然异步——这正是不能在 `on_prepare` 里 await 它们的原因 |

## 3. 对计划 v1 的修正：方案 C

### 3.1 三个方案

|                                                     | A：commit-first（两栈现状）       | B：apply-first 全回退（计划 v1）                                                  | C：prepare-heavy commit + 有栅栏的 reconcile（本报告推荐）                                          |
| --------------------------------------------------- | --------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| 提交前                                              | 参数校验                          | 构建 + 校验 + **真实应用 runtime/OS 效果**                                        | 构建 + 校验 + **核心 dry-run + 效果预检**（不触碰运行中的世界）                                     |
| 提交后                                              | runtime apply + effects，失败降级 | 发布、结算                                                                        | runtime apply + effects 作为 tracked op，失败降级 **且 `applied < desired` 持续 reconcile**         |
| 失败回退范围                                        | 无                                | 一切（含 OS 状态、进程）                                                          | 一切**可在提交前判定**的失败（坏 profile、脚本错误、YAML 非法、端口占用、非法快捷键、核心拒绝配置） |
| 世界失败（进程启动崩溃、OS API 失败、PAC 拉取失败） | 降级，可能永久失联（retry 空洞）  | 回退用户意图                                                                      | 降级 + 有界重试 + 命名的不确定态 + 「还原」= 正向 patch                                             |
| ACK 内阻塞                                          | 无 ACK                            | 最长到 runtime reload + PAC 完成                                                  | 构建 + `-t` 检查（典型 < 2s）                                                                       |
| 新增机制                                            | —                                 | OS 代理 / 快捷键 checkpoint 与 rollback、runtime 回滚到 checkpoint、Cancel 状态机 | `/core/check` 接线、效果预检、`ReconcileStatus{desired, applied, health}`、重试预算                 |
| 与 roadmap §1.3                                     | 一致                              | 反转                                                                              | 收紧（不反转）                                                                                      |

### 3.2 为什么 C 比 B 更符合用户最初的两条诉求

- 「CoreManager 等副作用服务不在事务模式中运行」：C 让它们通过**两个类型化接缝**参与——提交前 `RuntimeValidator`（构建 + `/core/check`，`nyanpasu-core-manager` 已提供 `CoreControl::check`，`control/mod.rs:552-579`，advisory、只读、不排队）与提交后统一的 `ReconcileStatus`。手写兜底（facade 的 gate、retry map、`after_commit` 重建）全部删除，由 workflow 的 reconcile 循环收敛。
- 「apply 出错优先降级而不是回退」：C 把绝大多数「apply 错误」前移为提交前拒绝（真正的回退：什么都没写）。剩下的世界失败，Harness 与 roadmap 的判断一致：回退用户意图是错的（用户明确要开系统代理，OS API 抖一下就把意图删掉是荒谬的），正确做法是保留意图 + 持续收敛 + 用户可见的差距。
- 计划 v1 §0.2 列出的 retry 空洞由「`applied_revision < desired_revision` 时无视 diff 重新 apply」结构性修复，不依赖回退。
- 计划 v1 §2.2 承认的最大代价（Try 期间配置 actor mailbox 被占）在 C 下缩小到构建 + 检查的时长；T1（读路径改 snapshot）仍保留。

### 3.3 C 下仍然保留回退的地方

- 提交前的一切（`Ack::Rejected` → `ClientError::ApplyRejected`）。
- runtime 自身的内部事务：`ReconcileOutcomeKind::RolledBack`（manager 已恢复旧修订）上报为 `CoreRollback` 降级，`applied` 停在旧修订，不再压成 `ApplyFailed`。
- 提交前的临时资源：暂存 profile 内容、check 临时文件、候选端口——用 disposer 风格在产生处登记清理。
- legacy 三域 saga 的 CAS 补偿，直到 PR-7a 删除。

## 4. 若采纳 C，计划 v1 的改动清单

| 计划 v1 位置               | 改动                                                                                                                                                                                           |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| §1.1 TCC 映射              | Try = 校验（构建候选 runtime、`/core/check`、效果预检、跨域引用检查）；Confirm = 入队 tracked apply 操作（不在 ACK 内 await）；Cancel = 丢弃候选与暂存                                         |
| §1.2 失败矩阵              | 「Required apply（Try）」一行删除；新增「Post-commit apply：`CommittedDegraded` + `ReconcileStatus` 差距 + 有界重试；uncertain → `RecoveryRequired`」                                          |
| §2.1 不变量                | 新增第 8 条：配置 actor `handle` 内的 ACK 只做校验，不 await 任何外部效果                                                                                                                      |
| §3.1 状态机                | `Applying` 改为 `Validating`（短）；`AwaitDecision` 保留；`Confirming` 改为 `Applying(op)`（tracked task，可被后续 patch 的 apply 排队取代：只追最新 desired）                                 |
| §4.x 三类流程图            | runtime 分支移到 COMMIT 之后；提交前增加 `/core/check` 节点                                                                                                                                    |
| §5.2 Try                   | 只保留第 1、3、5（build）步 + 新增 dry-run check + 效果预检；删除 checkpoint 捕获、reconcile、`try_apply`                                                                                      |
| §5.3 Confirm               | 变为「入队 apply 操作」；apply 操作内容 = 原 §5.2 第 5、6 步 + 原 §5.3 全部；失败 = 降级 + `applied` 不推进                                                                                    |
| §5.4 Cancel                | 只剩丢弃候选、清理暂存；`RecoveryRequired` 只由 apply 操作的 uncertain 产生                                                                                                                    |
| §5.5 owner checkpoint 契约 | **整节删除**；替换为「owner 统一 `ReconcileStatus{desired_revision, applied_revision, health}` + `Superseded`」（PR-6 已有 90%）                                                               |
| §6 T4                      | 删除；改为 T4'「`RuntimeValidator` 与 `EffectPreflight` 端口 + `/core/check` 接线（local host 直接 `CoreControl::check`，service host 走 v1 `/core/check`）」                                  |
| §6 T6                      | Try/Cancel 缩小；新增 T6'「reconcile 循环：`applied < desired` 无视 diff 重 apply；崩溃循环用 `RECOVERY_BUDGET` 式预算；`还原` = 正向 patch」                                                  |
| §6 T8                      | `ApplyRejected` 只表示提交前；`MutationOutcome` 增加 `revision`（可选）供前端丢弃迟到帧                                                                                                        |
| §7 决策                    | 新增 D-0（B vs C，推荐 C）；D-2/D-3（PAC、快捷键部分失败）在 C 下改为「预检拒绝 + 世界失败降级」；D-4 服务模式切换在 C 下 = 提交前校验（服务已安装、版本兼容）+ 提交后 handoff 作为 tracked op |
| §8 roadmap §1.3            | 不反转；改为「prepare-heavy commit-first」：prepare 必须含构建 + dry-run；提交后失败只能是世界失败；`desired/applied` 差距由 reconcile 收敛并对用户可见                                        |
| §9 验收矩阵                | 「core apply 成功，PAC 失败 → 全部恢复」改为「PAC URL 非法 → 提交前拒绝；PAC 拉取失败 → 降级 + 直连 fallback + 下次 reconcile 重试」；新增「同值重存且 `applied < desired` → 仍然 apply」      |

## 5. 未在 Harness 中找到、需要我们自己定的

- 世界失败的重试策略（预算、退避、何时停止）——Harness 的工具执行不重试，靠模型决定。
- 「还原到上次已应用」的用户动作——Harness 没有对应概念（对话不需要）。
- 跨三个配置域的复合修改（legacy saga）——Harness 是单日志，天然单域。
