# Nyanpasu Workflow、事务等待与生命周期精简实施计划

> **已被取代**：本文是评审稿，实施以 [`docs/plan/2026-09-28-workflow-lifecycle-simplification.md`](../plan/2026-09-28-workflow-lifecycle-simplification.md) 为准。

## 1. 目的与原则

### 1.1 本次改造的目的

将实现收缩到应用实际需要的复杂度：配置事务由 state 负责，关键内核操作由一个串行 owner 负责，普通副作用由各自资源 owner 负责，退出由 NyanpasuClient 广播停止请求。

本次不是重写一个更通用的工作流引擎，也不是通过更多状态和补偿覆盖任意中断。目标是消除不必要的本地中断、重复调度和重复结果路由，使大量异常中间状态不再由正常控制流程制造。

期望得到的结果：

- 本地配置 prepare、持久化和提交决定等待，不因业务无关的秒数到期而失败。
- 已进入关键执行段的操作不依附于 GUI、RPC 等待者或某个窗口的生命周期。
- 一个执行域只有一种串行化机制；保留 actor 就使用它的 mailbox 和 handler。
- shutdown 没有应用级阶段协议；独立 owner 并发收尾，确实存在的依赖由所属 owner 内部处理。
- 本地操作结果直接交还本次等待者，不通过 OperationId 在诊断历史中查找。
- 真正的 IPC 不确定性、持久化失败、旧下载返回等风险仍然得到正确处理。

### 1.2 强制设计原则

**P1 — 按真实边界设计。** 本地 actor 消息不是进程 IPC；多线程协作不按分布式事务处理。外部内核、服务 IPC、统计浮窗子进程，以及真实网络请求属于外部边界，不能混入本地计算和配置写入的统一超时策略。

**P2 — 本地事务默认执行到真实终态。** 对本地 Required prepare、配置写入、回滚及等待源事务决定，不施加通用 timeout。耗时提示不能改变事务结果，不能因耗时将操作升级为 RecoveryRequired。

**P3 — 等待者不拥有操作。** 窗口关闭、请求 future 被丢弃或调用者不再等待，不等于已经开始的业务操作被取消。操作由源配置 actor、runtime actor 或现有的持久化 owner 持有到结算完成。

**P4 — 一种 owner，一种串行化机制。** 本次保留既有 ractor，不同时引入 Arc<Mutex<Workflow>>，不在 actor 内再建 pending/active/Completed 调度器。无消息需求的独立对象才使用 Mutex/RwLock；已发布状态使用快照或 watch 读取。

**P5 — shutdown 是停止请求，不是强制终止。** NyanpasuClient 持有根 CancellationToken。各 owner 收到请求后停止新准入，完成已经进入的关键工作，再清理自己拥有的资源。不用根 CT 的 select!/run_until_cancelled 包住完整配置事务。

**P6 — 取消全局总序，保留局部资源依赖。** 不建立 ShutdownWorkflow、通用服务依赖图或退出阶段协议。Runtime owner 必须在当前事务完成后再关闭其 Core/Service 控制依赖；这不要求其他独立 owner 等待它。

**P7 — state 是源配置决定的唯一权威。** 保留 prepare/commit/rollback、原子持久化、实际失败后的资源恢复。Runtime participant 读取现有 DecisionHandle，不另外记录一份权威决定。

**P8 — Required 不可被当作“退出时可丢弃的通知”。** Required participant 不可用时，拒绝尚未开始的关键事务；已经参与的事务必须结算。不能把 is_shutdown() 映射为 Required ACK 成功。

**P9 — 框架原语优先。** 直接使用 ractor 的 call/cast/stop/drain/wait，Tokio 的 oneshot/watch/任务完成句柄，以及 CancellationToken。typed client 可以做薄业务适配，但不得重写队列、超时、共享 Future 和退出回执协议。

**P10 — 只删除失去用途的身份与状态。** 配置版本、内核实例/操作身份、异步下载代次、实际运行态 receipt，不能仅因去掉 timeout 而删除。本地结算关联 ID 可以退化为诊断信息；控制流不得反查诊断历史。

**P11 — 不承诺任意 panic 原地续跑。** 预期失败通过 Result 处理；编程错误导致的 panic 交给既有错误边界/监督策略。未知外部状态不得伪装成拒绝或成功，也不得直接启动替代执行者与旧操作并发。

**P12 — 先删无意义抽象，再调整 Rust 风格。** 自然、不可失败的同义转换使用 From；可能失败的转换使用 TryFrom；投影、筛选、策略决策使用明确方法名。不为了统一 of() 命名新增错误类型或 tuple 构造器。

## 2. 基线、范围与非目标

### 2.1 基线

- 仓库：libnyanpasu/clash-nyanpasu。
- 审计基线：PR #5389 栈顶 `fb7de1b06f0c50be13463a87f0139142bd62d7bb`。
- 涉及原 stack：#5385–#5389。
- 本文是待实施计划，不代表代码已修改、测试已通过。
- 实施前记录真实基线和 nyanpasu-runtime 子模块 SHA。分支已有变化时按符号和调用链重新定位，不机械应用旧行号。[S1]

### 2.2 现状依据

| 已核实事实                                                                                                       | 对实施的影响                                             |
| ---------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Application/ClashConfig/Profiles 的主要写 RPC 已传 None；主要 PersistentStateManager 写路径未设置 effect timeout | 保留写入保证，不把此次改造描述成“去掉原有 write timeout” |
| state 通知层统一对 prepare/committed/rolled_back 使用 AckOptions.timeout                                         | 精简必须先修改 state 的通知契约，不能只改 workflow 常量  |
| runtime participant 等待 Try 有 90 秒 ACK 上限；workflow 有 10 秒 admission、120 秒 decision_wait                | 删除人为制造的排队过期和本地决定未知分支                 |
| finish() 通过 OperationId 等待 journal 内出现最终 receipt，另有 180 秒上限                                       | 用本次请求的直接完成通道替换                             |
| 关停具有统一阶段报告、分段 deadline、超时后直接调用 core 的分支                                                  | 必须与新的执行到终态契约一起修订，不能遗留旁路           |
| WidgetManager 确实持有子进程与 IPC handshake                                                                     | 浮窗是明确的外部边界例外，不能全删其进程控制期限         |
| AGENTS/CLAUDE 中仍有“跨 actor RPC 优先有限 timeout”的规则                                                        | 同步修改规则，避免后续代码重新长出同类机制               |

依据见源文件清单 [S2]–[S11]。这些是已核实路径，不是对全 workspace 的所有 API 都作出了无调用者保证。

### 2.3 本次范围

`backend/nyanpasu-core/src/state/`；`backend/tauri/src/state/` 的参与者接线；`client/application_workflow/`；`client/app_lifecycle.rs`；普通效果 owner 的本地等待与生命周期；Tauri 退出边界；相关状态 DTO、测试和指导文档。

### 2.4 明确不做

不替换 ractor，不建立新的 actor framework，不合并所有配置域，不新增通用事务引擎或 shutdown engine，不增加跨进程持久化工作流日志，不重写内核管理 IPC 协议，不做全仓库命名改造，不修改无关功能和配置文件格式。

不在本任务中直接编辑 `backend/nyanpasu-runtime` 子模块内部实现。确需修改其外部操作终态保证时，单独列出上游契约变更；在契约改善前保留现有核验，不能删掉保障后标 TODO。

此前 tray 首次重建失败、deep-link 交付等独立问题不混入此次架构精简，除非本次实际修改了相应调用路径。

## 3. 目标架构与关键契约

### 3.1 固定选择：保留 actor，移除双重调度

```text
Tauri / Tray / Hotkey / Profile sources
                 │
           NyanpasuClient
                 │
     Application / Clash / Profiles actors
                 │
       state candidate + validation
                 │
       ┌─────────┴────────────┐
       │                      │
纯本地保存/普通副作用       内核运行态相关修改
       │                      │
state 持久化、提交         Runtime actor
       │              单个 handler 执行完整操作
       │                      │
       │              Core/Service 外部边界
       └─────────┬────────────┘
                 │
          提交后的普通通知
```

初期可以沿用 `ApplicationWorkflowActor` 和现有目录名。先减少行为与状态，不在关键重构里同时做机械目录搬迁。概念上的职责收缩为 runtime owner，不额外创建 RuntimeCoordinator 与旧 workflow 并存。

### 3.2 三条现成通道，三种不同职责

| 通道                          | 发布者 → 接收者                     | 承诺                                                     |
| ----------------------------- | ----------------------------------- | -------------------------------------------------------- |
| Try ACK                       | Runtime participant → state prepare | 候选是否允许进入源持久化；成功不等于源配置已提交         |
| DecisionHandle                | state → Runtime participant         | 源配置已提交，或回滚及本地资源恢复结果；保留现有权威实现 |
| 本次请求的 settlement oneshot | Runtime owner → 源配置调用路径      | Runtime 的 Confirm/Cancel 已结束，或明确失败             |

这些不是三个新的通用协议类型。保留既有 Try 通道与 DecisionHandle，只给当前调用直接返回最终结果；禁止再建 receipt registry、按 ID 唤醒 waiter 的表，或从 journal 查询本次完成。

### 3.3 一次关键修改的执行顺序

```text
源 actor 检查准入、校验并构建候选
    → state 开始事务，调用 runtime participant
    → Runtime handler 检查准入
    → 构建、校验、尝试应用候选
    → 发送 Try ACK
    → state 持久化并发布最终 DecisionHandle
    → Runtime handler 直接等待到这个决定
    → Confirm 或 Cancel，完成必要的真实恢复
    → 发送本次 settlement
    → handler 返回，才允许下一项 runtime 命令执行
```

必须满足：state 发布决定，不能依赖 Runtime 先发送 settlement；Runtime 等决定时，不能 RPC 回正在等待自己的源 actor。其他域的输入来自已提交快照。

### 3.4 准入、取消和结果语义

- 已经发进 mailbox，不等于已开始关键执行。
- owner 在开始处理命令时检查停止请求；尚未开始的工作可以明确拒绝。
- 某个源事务在退出前已进入，但尚未向 Runtime 提交 Try，而 Runtime 已关闭：返回明确的“未提交/退出中”，让 state 清理候选；不伪装成成功，也不声称外部操作结果未知。
- Runtime 已进入 Try 后，不再因根 CT、调用者消失或本地 elapsed time 放弃本次结算。
- 请求发送失败与“发送后 owner 失败、没有回执”必须区分。oneshot 关闭不是成功，也不能无条件解释为“什么都没做”。
- 源持久化失败时，已经开始的 Runtime Try 也必须完成并得到 Cancel/恢复结论；不能在 source error 后直接跳过有义务等待的 settlement。
- 早期校验失败、版本冲突或 participant 从未被调用时，不得等待一个根本未建立的 Runtime 完成过程。这个事实只在本次调用/participant 内表达，不建全局 context 表。

### 3.5 必须保留的业务行为

显式内核/host/profile 切换仍遵守现有关键应用承诺；确定失败仍回滚；只有旧运行态安全且错误明确可重试时才允许 deferred；用户停止内核后的普通保存不得自动启动它；实际 IPC 未知结果仍核验；随机端口属于运行态 binding，不反写源配置。

纯界面设置不受 runtime 隔离阻塞。请求字段与最终 diff 均参与分类，不能将“重选同一个内核/配置”误判成纯 no-op。混合 patch 必须作为一个源事务提交或拒绝。

## 4. 任务总览与依赖

| 任务 | 目的                                                         | 前置                    |
| ---- | ------------------------------------------------------------ | ----------------------- |
| T00  | 固化契约、修改指导规则、建立调用清单与回归基线               | 无                      |
| T01  | 取消 state 本地 ACK 计时策略，修复 Required shutdown 语义    | T00                     |
| T02  | 移除本地 admission/decision/finish 超时，直接返回 settlement | T01                     |
| T03  | Runtime actor 直接串行执行完整操作，删除双重调度             | T02；与 T05 联调        |
| T04  | 纯保存/普通副作用彻底绕过 Runtime participant                | T03                     |
| T05  | NyanpasuClient 广播 CT，各 owner 收尾，删除统一关停阶段      | T02；与 T03 原子集成    |
| T06  | 审计 owner 内部本地 timeout、effects 调度及无主任务          | T05                     |
| T07  | 根据剩余真实消费者缩减恢复状态、身份与中间类型               | T03–T06                 |
| T08  | 清理转换与无用 wrapper，同步 DTO、文档与架构门禁             | T07                     |
| T09  | 执行行为矩阵、全量验证与平台 smoke                           | 贯穿；最终验收在 T08 后 |

不得先删保护性状态，再保留制造这些状态的 timeout/取消路径。不得将 T03 的 handler 模型改完，却继续使用旧关停“超时后直停 core”旁路。

## 5. 详细实施任务

### T00 — 固化契约和调用清单

**文件：** `AGENTS.md`、`CLAUDE.md`、`docs/design/actor-migration-roadmap.md`、现有 selective-TCC 计划，以及新增的本次实施文档。

**实施：**

1. 同步替换“跨 actor RPC 优先有限 timeout”：同进程普通调用默认 `None`；外部资源 adapter 才定义有业务意义的期限。
2. 增加 P2–P11 对 owner、取消、完成关系的规则。特别注明普通 after-commit 通知与关键 Runtime participant 不同，不能被“先提交再副作用”的泛化说明混淆。
3. 将旧文档中的 admission/ACK/decision 时间预算、应用级七阶段 shutdown、诊断历史参与结算等标记为被本计划替代；保留历史，不静默改写过去的验收记录。
4. 搜索生产与测试中的 `timeout`、`timeout_at`、ractor `Some(Duration)`、`abort/kill`、`select!`、`run_until_cancelled`、`Drop`、`poll!`。每个生产命中记录：谁拥有操作、超时丢弃谁、是否会留下外部/本地写入、是否有实际业务期限。
5. 查清 state `with_pending_state_timeout`、AckOptions、StateAckSubscriber 和各 manager 的全部 workspace 消费者。重点覆盖 Simple/Weak/Persistent 路径，不能只改主要三个配置域。
6. 建立 owner 清单：资源、后台任务、构造/销毁位置、接收 CT 后的行为、必须继续存活的依赖。

**交付：** 一份嵌入本计划的超时/owner 清单，不新建运行时 registry 或扫描框架。记录真实测试基线与环境限制。

**验收：** 指导文件无相互矛盾的 timeout 要求；每个保留期限都能指出具体资源边界；每个后台任务都有真实 owner。

### T01 — 移除 state 通知层的通用 timeout

**文件：** `backend/nyanpasu-core/src/state/{ack.rs,coordinator.rs,transaction.rs,transaction/notify.rs}`、`manager/` 和相关测试/订阅者实现。

**实施：**

1. 目标是 state 不再负责给本地 callback 计时。`AckOptions` 只保留参与策略；同步修改 `required/advisory` 构造与调用者。不要新增 TimeoutPolicy、无限 Duration 或另一层计时 wrapper。
2. `on_prepare`、必要 rollback callback 和 post-commit notification 不再被 state 的通用 `timeout` 丢弃。普通通知应快速投递给资源 owner；不得把耗时 OS 工作塞回 state 的提交路径。
3. Required 已关闭应返回明确失败，而不是 `SkippedShutdown` 成功。Advisory 是否跳过保持通知语义。已进入事务不能用全局退出信号取消其关键参与者。
4. 从本地计时中移除 `AckStatus::TimedOut`、`SubscriberAck.timeout` 等失去意义的字段与分支；保留 elapsed 诊断。外部 adapter 的实际 Timeout 仍能作为源错误返回，不需要 state 再计时。
5. `with_pending_state_timeout()` 若无生产消费者则删除；若有消费者，先把真实 deadline 放回对应外部 adapter 再删除本地通用入口。无法确认消费者语义的 API 不先破坏，必须作为明确残留列出。
6. 保持主要配置 write 不设超时，保持原子写、恢复、writer permit 和 DecisionHandle 权威。不要为本次精简重写已经有效的持久化 owner。
7. 检查 prepare 的 JoinSet/任务异常处理。取消计时不意味着忽略 participant panic；真实任务失败必须结束源事务，不能遗失完成信号。

**验收：** 本地 prepare 超过旧 30/90 秒仍正常等待；Required shutdown 不允许候选绕过；写入失败时仍恢复；源 owner 非正常结束后，已开始事务必须有可信的终态/资源未知状态，不能永远遗留 Undecided。

### T02 — 取消人为本地等待预算，settlement 直返

**文件：** `client/application_workflow/{participant.rs,mutation.rs,tcc.rs,mod.rs}`、`state/mutation.rs`、三个源配置 actor、相关状态 DTO 和测试。

**实施：**

1. 删除 `MUTATION_ACK_TIMEOUT`、`with_ack_timeout`、本地 admission expiry 和 `decision_wait`。等待 `DecisionHandle::wait()` 的真实结果。
2. 删除只服务于排队计时的 `AdmissionExpired` 和计时任务。保留容量限制时，用入口 `try_acquire_owned` 等现成容量控制，permit 随请求存活；不复制 pending 队列，不等待若干秒再过期。
3. 本地 workflow typed client 普通 call 使用 `None`；保留错误类型适配。实际进程 IPC 的 deadline 留在下层，不在整个业务操作上重复套一层。
4. 为已经提交的本次 Runtime 请求携带最终 settlement sender；源路径直接等待对应 receiver。Try ACK 不提前代表全部完成。
5. 删除 `wait_mutation(operation_id)` 对 journal 的控制性查询，以及 `MutationCoordinator::finish` 的 180 秒等待和由该计时产生的 `operation_pending` 降级。
6. 本次请求未调用 participant、未送达、被准入拒绝、Try 拒绝、源提交成功、源持久化失败、Runtime owner 失败都要有明确路径。早期失败不等待不存在的执行，Runtime 已参与后的源失败不跳过 Cancel 结算。
7. journal 继续用于展示近期结果；允许丢历史，不允许历史容量、更新顺序影响本次调用返回。
8. 同时修订旧关停中的本地 settlement 截止时间和“超时后绕过 workflow 直接停 core”路径。新本地事务不设超时后，旧退出路径不能成为新的中断源。

**验收：** 慢 write 跨过旧 120 秒界限不进入 RecoveryRequired；本次完成与 journal 是否订阅/是否保留记录无关；请求 future 被丢弃不撤销已经进入的修改；实际 IPC 故障仍按真实结果处理。

### T03 — Runtime actor 直接执行，删除双重调度

**文件：** `client/application_workflow/{mod.rs,workflow.rs,tcc.rs,startup.rs,attempt.rs}` 和其测试。

**实施：**

1. 将 `workflow: Option<Box<ApplicationWorkflow>>` 改为始终由 actor 持有的普通业务状态。
2. handler 直接 `await` 一次完整命令；不将整个 workflow take 出、spawn、再由 Completed 消息送回。
3. 删除仅用于第二层调度的 `ActiveOperation`、`pending: VecDeque<Request>`、`Completed { workflow, ... }`、`drive()`、`shutdown_waiters`、`MutationContext` 注册/退休，以及仅服务这些结构的 wake 消息。
4. 执行阶段通过 watch/已发布快照展示。状态读取不在 mailbox 中等待正在执行的操作，也不反向参与准入判断。
5. state 的 Try 等待和 DecisionHandle 发布不经同一个 Runtime mailbox 回送；禁止同步调用源 actor。直接 settlement 必须发生在必要 Confirm/Cancel 完成之后。
6. legitimate deferred retry 继续使用 actor 自带定时消息。最多保留实际需要的定时目标；旧 timer 消息到来时检查当前目标，不重新建立优先级调度器。
7. 框架级 stop/drain 使用固定版本 API；测试证明当前 handler 能结算、队列命令会被明确拒绝或丢弃并正确关闭回复。普通退出不用 kill/abort 当前 handler。
8. 不自动重启一个出错的 Runtime owner 来继续写内核；真实未知操作先核验。初期可保留原有 recovery 结构，等 T07 再按消费者删除。

**验收：** 两个关键命令严格按前一项完整结算后再执行；长 Try 不阻塞只读状态；消息生命周期不需要额外 active task；取消 source RPC 不取消 handler；关闭与队列交错无等待环。

### T04 — 非 Runtime 保存从入口分流

**文件：** `state/{application.rs,clash_config.rs,profiles/actor.rs,mutation.rs}`、`client/application_workflow/impact.rs`、effects 通知及 UI 状态接线。

**实施：**

1. 复用一个纯分类实现，避免 state 与 workflow 各写一份“哪些字段影响内核”。分类读取候选、旧值和请求意图，不由 GUI 指定豁免策略。
2. 纯保存只走本地 state 校验/持久化/提交；普通副作用保存提交后通知 owner；只有关键内核修改安装 Required Runtime participant。
3. 纯保存不构造假 baseline、TryVerdict 或 runtime recovery context，不为等待结果生成 runtime OperationId。
4. 保持显式 core/host/profile 激活即使 diff 为空也需要相应确认。保留停止内核后的 SavedInactive 行为；混合 patch 原子性不变。
5. Profiles 中不影响当前闭包的内容/元数据更新不进 Runtime，但必要的本地文件事务和清理仍归 Profiles/state。
6. Runtime actor 故障或隔离时，纯 UI 设置仍可保存；实际 runtime 相关变更继续被正确拒绝。
7. 普通效果在退出时关闭可以忽略迟到通知，不能让已经提交的源配置被误报为回滚。非退出情况下的投递失败仍按现有降级/日志语义报告。

**验收：** 用一个会在被调用时立即报错的 Runtime fake，证明纯 UI patch 从未触达它；相关/非相关/混合/同值重选/停止状态的分类矩阵通过。

### T05 — NyanpasuClient CT 广播与局部收尾

**文件：** `client/{mod.rs,app_lifecycle.rs}`、`setup.rs`、`lib.rs`、`utils/{help.rs,resolve.rs}`、各 typed client/owner、`server/mod.rs`、窗口退出与重启入口。

**目标接口：**

```rust
// 接口草图；字段名称应复用现有结构，不单独新增生命周期框架。
impl NyanpasuClient {
    pub fn request_shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tasks.close();
    }

    pub async fn wait_shutdown(&self) {
        self.inner.tasks.wait().await;
    }
}
```

`tasks` 是框架 TaskTracker 或既有等价完成集合，不是新的 ServiceRegistry。退出完成等待必须覆盖真实 owner 生命周期，不能只跟踪一个已经把工作发出去的 wrapper。TaskTracker 的 close 不阻止新增任务，因此准入在 owner/消息入口负责；启动后登记的根生命周期任务必须在 client 对外可用前建立。[S14]

**实施：**

1. 根 CT 由 NyanpasuClient 持有；各 owner 通过启动参数注入。独立 owner 使用适当的 child token，内部私有依赖不能因同一个根 token 抢先自毁。
2. 不新增 ShutdownActor、AppLifetimeActor、通用 Shutdown trait 注册器或可配置阶段表。长生命周期任务不要捕获整个 NyanpasuClient 导致无意义的 Arc 循环。
3. 独立 owner 并发收尾。Runtime owner 在当前操作结束后才停止 Core/Service 控制依赖；Core/Service 不作为与 Runtime 平级、独立响应根 CT 立即销毁的对象。
4. ractor 服务的 CT 监听只是框架 stop/drain/wait 的薄接线。建议 drain + handler 准入检查，令排队请求在退出时获得明确拒绝；私有依赖在 owner 收尾中结束。不要复刻 Terminating/Requested。
5. 状态 actor 继续运行当前 handler 到 state 与 Runtime settlement 完成；源事务尚未提交 Try 时遇到 Runtime 已关，安全拒绝并清理，不无限等待。
6. 删除统一七阶段 run_shutdown、ShutdownBudgets/Deadlines、Reply/Issued/Terminating/Requested、回复余量和超时后 fallback；只保留有实际使用者的最终失败信息，不建新的嵌套报告状态机。
7. 所有 GUI/托盘/IPC 退出入口只请求关闭。Tauri 边界在首次退出时 prevent_exit，发信号并立即返回；唯一完成等待者结束后设置“允许最终退出”并发出退出/重启。这个事件循环标记不进入业务协议。
8. 最终窗口几何信息在边界同步捕获，然后保证 SessionState 的最后保存请求在其 drain 之前已提交。普通会话保存不得取消；不能先结束 SessionState 再尝试保存，也不需要等待内核停止后才保存。
9. 退出、重启、操作系统关机等原有入口统一接线；错误码和退出/重启意图由边界保存，不能在每次请求里重新执行 shutdown。
10. 普通清理失败记录真实原因。等待所有任务结束不等于所有清理成功；严重失败使用已有错误呈现方式，不谎报 Done。系统强制结束与 graceful shutdown 分开描述。
11. 不在任意 NyanpasuClient clone 的 Drop 中取消根 CT。审查最后一个 typed client 的 Drop/stop 与显式 drain 是否冲突，普通句柄释放不能抢先越过必要的保存或清理。
12. 区分日志查询 actor 与实际 tracing writer；真实 writer 的 flush/guard 释放在应用边界保障，不靠“日志索引最后停”替代。构造中途失败时清理已建立资源，不能在 setup 失败后留下未归属的长期任务。

**owner 收尾表：**

| owner                            | 根停止请求后的职责                                                              | 不应建立的全局依赖                       |
| -------------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------- |
| Application/Clash/Profiles state | 拒绝新业务；当前事务结算；本地资源收尾                                          | 不等待 GUI 或日志查询 actor              |
| Runtime owner                    | 当前命令结算；随后停止自己使用的 Core/Service 控制资源                          | 不要求 Hotkey/Tray 先结束                |
| SystemProxy                      | 停止 guard/新目标；等待自己的 OS 写；恢复原始设置                               | 不等待其他普通效果                       |
| Hotkey                           | 不再接收新绑定；完成本地注册；注销自己持有的快捷键                              | 不依赖 core 停止                         |
| Tray/UI 通知                     | 停止新提交，清理边界资源，忽略退出后的普通刷新                                  | 不参与源事务决定                         |
| Widget                           | 结束事件接收；完成自己的进程/handshake/IPC 清理                                 | 不依赖 streams 自然关闭才能结束 listener |
| Profiles sources/Updater         | 停止计时器、watcher、可取消下载；不启动新的安装/提交；已进入的安装/提交执行到底 | 不把下载取消策略传递给 state write       |
| Streams/Proxies                  | 结束自己持有的连接/监听/任务；迟到通知不再次开启                                | 不和 Runtime 相互等待                    |
| SessionState                     | 接受已提交的最终几何保存，写完后结束                                            | 不依赖内核是否已停止                     |
| 日志查询/索引 actor              | 结束自己持有的查询任务                                                          | 不冒充 tracing writer 的 flush owner     |
| 内部 HTTP server                 | 停止接收请求；现有请求依业务终态结束；线程/任务被实际等待                       | 不靠整个进程退出才结束                   |

**验收：** 重复 request 幂等；各独立清理并发；在途 Try/Confirm/Cancel 不被打断；最终几何保存不被越过；全部真实 owner 结束才返回 wait；主线程持续处理注销/窗口任务；退出后不再产生源写或重新安装代理。

### T06 — 精简普通 owner 的内部等待

**文件：** `client/{effects,system_proxy,hotkey,ui_effects}/`、`widget.rs`、Profiles sources/scheduler、Updater、Streams、日志与 server 接线。

**实施：**

1. 移除仅因同进程 RPC 慢而返回 timeout/degraded 的普通调用上限；等待 owner 的真实 Result。
2. OS 写入可能耗时但不能靠丢弃等待者中断。开始后由 owner 等待其 blocking task 返回；返回后再清理。不要声称所有 syscall 都必然快速完成。
3. SystemProxy 的 restore 归回该 owner 的退出路径。现有 post_stop 只停 guard，不能仅接 CT 然后停止 actor 而遗漏恢复。[S8]
4. Hotkey 清理必须由仍在运行的 GUI 事件循环执行；不在 Tauri 主线程同步 block_on 等待。
5. Effects 的 independent groups/coalescing 不因“也有 Completed”就全删：它可能负责真实独立并发和新目标合并。只删除内部人工 timeout、无业务价值的等待者取消和统一 cleanup 编排；保留被测试证明需要的分组及旧结果屏蔽。
6. 取消 GROUP_REAP_BOUND 等本地 wrapper 计时后，不再用 group.abort 代替资源清理。可丢弃的纯通知与不可丢弃的实际操作分开。
7. 检查 Widget 的阻塞 send/handshake worker：保留其真实 IPC/子进程期限，但期限到达不是 worker 已结束的证据；不得释放 owner 后让 worker 仍在使用资源。
8. 网络下载、脚本运行等真实有风险的边界各自保留原有合理控制。JS/Lua 等用户脚本的执行限制如已存在，保留在解释器/隔离边界；不要用取消整个配置 prepare 来替代它。本次不另造脚本沙箱。
9. 自动更新安装、订阅文件落盘等可取消/不可取消切换点明确：下载阶段可取消；提交/安装已经开始后等待结算。

**验收：** 延迟 fake OS 写不会产生人工失败；清理不与同 owner 的在途写并发；外部 IPC 超时仍可诊断；没有“外层完成但底层写仍无人持有”的任务。

### T07 — 缩减恢复状态与身份用途

**文件：** `client/application_workflow/{attempt.rs,mutation.rs,tcc.rs,startup.rs,preparation.rs}`、`runtime.rs`、`runtime_recovery.rs`、`state/mutation.rs` 及必要 source-state 接线。

**实施：**

1. 为每个长期字段标出生产写者、生产读者、控制含义。测试独占字段不得被误认成生产需求。
2. 将正常路径的 candidate、baseline、verdict、准备好的产物尽量放回一次 handler 的局部变量。只有下一次命令仍需要的数据进入长期状态。
3. 删除仅用于恢复“本地 decision budget 到期”的状态和入口。不要强迫所有错误进入 LiveAttempt。
4. 保留真实 confirmed receipt、已提交但尚未应用的目标、尚未核验的外部操作、用户 stop intent，以及实际本地资源恢复失败证据。
5. 不新增泛化的 panic continuation。生产 bug 触发隔离/既有退出策略；不得将未知结果当作 transient retry。
6. source OperationId 失去结算用途后，可只保留为日志/用户查询关联；不要因为已有类型就仍让每个纯 UI 保存生成一套 runtime ID。
7. state Version/CAS、本地资源 journal、ProfileId、RefreshAttemptToken、内核实例 generation、payload digest 按实际用途保留。结构上已经串行并不足以证明全部代次都可删除。
8. 不顺手删除 PersistentStateManager 的 persistence_owner/RollbackGuard。先证明公开调用路径不会因 caller drop/panic 被丢弃；只改善应用调用 owner，不降低可复用 state API 的取消安全保证。
9. Deferred retry 继续遵守当前 desired 目标，不重新应用旧目标；恢复与普通用户显式 retry 不必维护重复的源决定。

**验收：** 所有被删 recovery 分支都能关联到已删除的中断源；实际 IPC reply lost、源文件写入失败、资源恢复失败、旧下载完成的测试仍通过。

### T08 — Rust 接口、状态与文档清理

**文件：** 本次触及的 typed client、转换函数、状态 DTO、`specta_export.rs`、前端 bindings/configuration-status、架构 ledger 和指导文档。

**实施：**

1. 移除空转 wrapper，不为了统一风格再创建 GeneralReply/CompletionHandle/OperationRegistry。
2. 值到值的自然转换改用 From，可能失败的解析改用 TryFrom。`ActionView::of` 这类展示投影改为 view；可选提取改为 verdict/recovery_kind，避免把正常 None 包成错误。
3. `MutationDomain::domain_change` 若仍只有类型转换职责，使用 `From<StateChange<ConcreteConfig>> for DomainChange` 和相应泛型约束，删除额外转换 trait；有其他实际职责才保留。
4. 检查 TryAck 是否只是重复 Ack。可直接返回 Ack 的本地通道不保留四变体的镜像，但保留与 ACK 不同的业务结论。
5. 不把唯一一次 lossy transport 分类改成全局 From。必要位置直接 match ractor 的发送失败/回复丢失/成功。
6. 删除仅因本地计时而展示的 operation_pending/Unsettled 原因；保留真实执行中的 Pending 与真实外部 RecoveryRequired。用户可见状态不可凭时间“猜结束”。
7. 后端 wire 变化与前端消费者在同一提交更新，bindings 用生成命令，不手改；纯内部重构不强迫修改公共 DTO。
8. ledger 记录机制删除和实际剩余例外。不要做按关键词禁止全仓库 timeout/OperationId 的门禁，不要求测试数量只增不减，不用新增框架测试来补删掉的旧机制测试数量。
9. AGENTS/CLAUDE 同步；旧 roadmap、TCC 记录注明哪些语义仍有效、哪些机制已经被替代。

**验收：** 不再保留无消费者的抽象；From/TryFrom 使用符合语义；没有手写 bindings；公共行为及错误诚实性保持。

### T09 — 回归、平台验证与交付记录

每个实现任务同时修改自己的测试。T09 不是把所有测试推迟到最后，而是最后汇总跨模块结果。

采用 fake ports、TempDir、oneshot/Notify 测试屏障与 Tokio 虚拟时间。测试自己的看门狗或 CI timeout 仅用于判定测试失败，不能被移入生产逻辑充当事务 deadline。

必须区分源码判断、fake 测试、真实平台 smoke；没有运行的检查标记未执行，不沿用上一 stack 的测试数字。

## 6. 关键行为测试矩阵

| 编号 | 场景                                                     | 期望                                                           |
| ---- | -------------------------------------------------------- | -------------------------------------------------------------- |
| V01  | 本地 prepare 被屏障阻塞，虚拟时间超过旧 ACK 期限         | 无人工 rollback；释放屏障后正常继续                            |
| V02  | Try 已成功，源 write 跨过旧 120 秒界限                   | Runtime 保持等待；源决定到达后正确 Confirm/Cancel              |
| V03  | 本地 patch 调用者 future 被丢弃                          | 已进入操作由 owner 完成；不因 reply 无人接收回滚               |
| V04  | 同时有两个不同源域的关键变更                             | 第二项 Runtime Try 不越过第一项完整结算；读取正确已提交快照    |
| V05  | 源校验失败/版本冲突，participant 未执行                  | 立即返回真实错误；不等待不存在的 settlement                    |
| V06  | 请求未送达/Runtime 在准入前退出                          | 明确拒绝，无内核修改；state 清理临时候选                       |
| V07  | Try 拒绝且 source abort                                  | 原源状态/原运行态保留；完成通知正常结束                        |
| V08  | Try 成功但源持久化失败                                   | 完成本地恢复及 Runtime Cancel；不提前接受下一项关键操作        |
| V09  | 源持久化或资源恢复真正失败                               | 诚实报告错误/资源未知，不伪装为安全恢复                        |
| V10  | journal 未订阅、历史淘汰或 UI 丢事件                     | 本次 settlement 仍正常返回                                     |
| V11  | source owner/participant 异常结束                        | 不永久 Undecided；不把 channel closed 当成功；外部未知保持隔离 |
| V12  | Runtime 隔离期间保存纯 UI 配置                           | 保存成功，Runtime fake 未被调用                                |
| V13  | 同值重选内核/host/profile；混合 patch；StoppedByUser     | 关键承诺、原子性和不擅自启动保持                               |
| V14  | 重复、多入口同时发 shutdown                              | 同一停止信号，各资源只收尾一次；只有一个边界完成退出/重启      |
| V15  | shutdown 在 source 开始后、Runtime Try 投递前发生        | 安全拒绝或按已接受契约完成，不死锁                             |
| V16  | shutdown 在 Try/AwaitDecision/Confirm/Cancel 中发生      | 当前 handler 执行到底；Core/Service 不提前销毁                 |
| V17  | 根 CT 已取消时遇到 Required participant                  | 不被 SkippedShutdown 当作成功放行                              |
| V18  | 三个独立 owner 各自阻塞清理                              | 三者并发开始；Client 等待全部真实结束                          |
| V19  | OS 写入慢于旧 hotkey/system-proxy RPC 上限               | 无人工失败；写结束后再本 owner 清理                            |
| V20  | 关闭后迟到 effect/timer/download 完成                    | 不重新注册快捷键、安装代理或开始新持久化                       |
| V21  | 窗口几何保存与退出并发                                   | 最终请求在 SessionState drain 前提交，保存完成后才结束         |
| V22  | Tauri 实际主线程调度快捷键注销                           | event loop 持续运行，不发生 block_on 自锁                      |
| V23  | IPC 执行成功但回复丢失、daemon 重连/重启                 | 保留实际操作核验，不盲重试或启动第二个内核                     |
| V24  | 老下载迟到、profile 被编辑/删除或新请求替代              | fencing 保留，旧结果不覆盖新状态                               |
| V25  | Widget handshake/阻塞发送、server 活跃请求、日志索引任务 | 各自真实资源被 owner 收尾；TaskTracker 不提前报告空            |
| V26  | owner 有动态子任务、根 tracker close 与子任务登记交错    | 父 owner 在子任务结束前持续被跟踪，不出现“已完成后又开始写”    |

如果某测试只是在验证已删除的任意本地 timeout 语义，应改为执行到底的测试；实际 I/O 失败、取消安全与 IPC 不确定性测试不可删除。

## 7. PR / 提交组织

建议三个可独立构建与验证的 PR，而不是按文件拆成很多相互不安全的层。

### PR-A：本地事务契约与直接完成结果

覆盖 T00–T02。一起完成 state ACK 政策、Required shutdown 规则、decision/admission/finish timeout 清理、直接 settlement 和相关测试。

同时去掉旧 shutdown 本地 settlement 超时后直停 core 的旁路；该 PR 暂时保留的旧关停入口必须等待真实在途结算，不能抵消新的事务保证。它只是迁移中的旧实现，不建立并行新实现或运行时切换开关。

### PR-B：单执行者与 owner 生命周期

覆盖 T03–T06。actor 完整 handler、普通保存分流、NyanpasuClient 根 CT、owner 局部收尾、GUI 退出接线与旧关停框架删除，在同一集成 PR 验证。

这些任务可以按源代码责任分工，但不能分别发布互不兼容的阶段。关键执行路径与其关闭方式一起落地。

### PR-C：剩余恢复模型与接口收口

覆盖 T07–T09。仅在新行为矩阵通过后删除失去消费者的 recovery/identity 状态，完成 From/TryFrom、DTO、文档和架构门禁。

不要求固定删除行数或达到某个类型数量。验收对象是重复机制消失、业务不变量保持，而不是 diff 好看。

**提交要求：** 每个提交保持构建；后端 wire 与前端使用者一起修改；不保留大面积 dead_code allow 隐藏残留；没有必要的兼容层不新增；修复与其所属行为折叠，不将未完备中间状态作为已交付。

## 8. 删除与保留清单

### 8.1 目标删除

- 应用层 `Issued`、`Reply`、`Terminating/Requested` 与 shared shutdown future/waiter 协议。
- 应用级关停阶段表、分段预算、REPLY_MARGIN、统一阶段报告状态机、超时后直停 core 的旁路。
- Runtime actor 上第二层 `pending/active/Completed/drive` 调度。
- 本地 `MUTATION_ACK_TIMEOUT`、admission expiry、decision_wait、wait_mutation 的历史回查与计时。
- 仅依赖这些人工中断的恢复上下文、重复状态和测试。
- 本地 ACK 中没有独立语义的镜像转换类型和无用 timeout wrapper。

### 8.2 必须保留，除非有单独的消费者证明

- 原子持久化、writer permit、DecisionHandle、真实资源恢复及可复用 state API 的取消保护。
- 明确的关键命令策略、停止意图、deferred 目标及合理重试。
- 实际内核 receipt、实例/操作身份、内容标识与未解决外部操作核验。
- 配置版本、公共条件替换、ProfileId、下载代次和实际并发结果屏蔽。
- 真实进程 IPC/网络/脚本边界限制，以及系统强制退出约束。
- 有消费者的只读进度和诊断日志；TaskTracker 等直接框架完成原语。

## 9. 命令与验证记录

以下均是实施时应运行的命令，不是本文已运行的结果。从仓库根执行；采用仓库锁定的 toolchain、Node/pnpm 版本和子模块提交。[S12]

```bash
cargo check --manifest-path backend/Cargo.toml --all-targets --all-features

cargo test --manifest-path backend/Cargo.toml --all-features -p nyanpasu-core
cargo test --manifest-path backend/Cargo.toml --all-features \
  -p clash-nyanpasu --lib -- --test-threads=8

# 修改 IPC wire 后使用现有导出测试生成 bindings。
cargo test --manifest-path backend/Cargo.toml --all-features \
  -p clash-nyanpasu export_typescript_bindings

pnpm typecheck
pnpm test:frontend
pnpm lint:architecture-ledger
pnpm test:architecture-ledger

cargo test --manifest-path backend/Cargo.toml --all-features \
  --workspace -- --test-threads=8
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
```

按仓库既有 fake-core fixture 机制准备测试二进制。若修改了新旧前端状态处理，增加 `pnpm web:build`。Windows/macOS/Linux 各自执行与本平台有关的编译和 smoke；不得用一个平台的 fake 测试代替三平台系统副作用验证。

对受影响路径做定向检索，分别处理生产与测试代码：

```bash
rg -n 'MUTATION_ACK_TIMEOUT|AdmissionExpired|decision_wait|wait_mutation' \
  backend/tauri/src

rg -n 'enum Issued|struct Terminating|enum Requested|ShutdownBudgets|REPLY_MARGIN' \
  backend/tauri/src

rg -n 'timeout_at|timeout\(|\.abort\(|\.kill\(|run_until_cancelled|poll!' \
  backend/nyanpasu-core/src/state backend/tauri/src/client
```

最后一项不是要求零命中；它是检查剩余命中是否符合真实外部边界、测试保护和所有权契约。无法在本地执行的平台检查如实记录为未执行。

## 10. 最终验收标准

本计划只有在以下结果同时成立时才算完成：

1. 本地 Required prepare、配置写入、回滚和源决定等待，不再有通用业务 timeout。
2. Runtime actor 使用自身顺序 handler，不存在第二层 workflow 调度器。
3. 本次调用结果不依赖 OperationId 查 journal，诊断历史只用于观察。
4. 普通保存不经过 Runtime participant，真实关键修改仍满足既有事务承诺。
5. NyanpasuClient 发 CT 并等待 owner 完成；没有独立的全局 shutdown 工作流或阶段协议。
6. 独立 owner 并行收尾，真实 Core/Service 与在途事务依赖只在 Runtime owner 内部维护。
7. Required 关闭不被当作成功；调用者消失、退出和实际业务失败的语义不混淆。
8. 没有迟到写入、无主 blocking task 或等待图死锁；Tauri 主线程在清理期间保持运行。
9. 实际 IPC 未知结果、文件恢复失败、下载乱序、停止意图和版本一致性保障未被削弱。
10. 指导文档、测试、DTO、架构 ledger 与实现一致，验证结果逐项真实记录。

**最终目标：让本地工作执行到底，让外部不确定性留在外部边界，让资源 owner 自己收尾。不是写一个更好的通用 workflow，而是不再需要那套 workflow。**

## 11. 源码与框架依据

以下源码均固定到上述审计 SHA；源文件反映现状，前文任务反映目标，不将二者混写。

- [S1] [PR #5389 元信息与 stack](https://github.com/libnyanpasu/clash-nyanpasu/pull/5389)
- [S2] [AGENTS.md](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/AGENTS.md)：现有 finite timeout、actor、DI 与文档同步规则。
- [S3] [state/ack.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/nyanpasu-core/src/state/ack.rs) 与 [transaction/notify.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/nyanpasu-core/src/state/transaction/notify.rs)：ACK timeout 与 SkippedShutdown。
- [S4] [state/coordinator.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/nyanpasu-core/src/state/coordinator.rs) 与 [manager/persistent_state.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/nyanpasu-core/src/state/manager/persistent_state.rs)：主要持久化路径和事务 owner。
- [S5] [application_workflow/participant.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/application_workflow/participant.rs)、[mutation.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/application_workflow/mutation.rs)、[tcc.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/application_workflow/tcc.rs)：本地预算和关键结算。
- [S6] [application_workflow/mod.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/application_workflow/mod.rs) 与 [state/mutation.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/state/mutation.rs)：自建调度与按历史等待 receipt。
- [S7] [client/app_lifecycle.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/app_lifecycle.rs)：当前统一关停机制。
- [S8] [client/system_proxy/actor.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/system_proxy/actor.rs)：代理所有权、restore 和 post_stop。
- [S9] [client/effects/actor.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/client/effects/actor.rs)：分组并发、目标合并与收尾。
- [S10] [widget.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/widget.rs)：浮窗子进程与 IPC。
- [S11] [state/profiles/actor.rs](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/backend/tauri/src/state/profiles/actor.rs)：下载代次与资源提交。
- [S12] [package.json](https://github.com/libnyanpasu/clash-nyanpasu/blob/fb7de1b06f0c50be13463a87f0139142bd62d7bb/package.json)：现有检查命令。
- [S13] [ractor 文档：消息与停止语义](https://docs.rs/ractor/latest/ractor/)：stop 完成当前 handler，kill 会中断；实施按 Cargo.lock 的具体版本验证。
- [S14] [Tokio TaskTracker](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html) 与 [CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html)：通知停止、等待完成及 close 的限制。
- [S15] [Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)：计时结束后的 future 取消语义。
- [S16] [Rust From](https://doc.rust-lang.org/stable/std/convert/trait.From.html)：自然、不可失败、保持语义的转换惯例。
