# Workflow 与生命周期精简实施计划

> **状态**：实施中（2026-09-28）。§12 各项按默认执行；PR-0 不在本计划内执行。
>
> **基线**：`refactor/tcc-t11-cleanup@fb7de1b06`（PR #5389；栈 #5385–#5389 均未合并），`backend/nyanpasu-runtime@f5b581fad`。锁定版本：ractor 0.16.5、tokio-util 0.7.19、tauri 2.11.5、tauri-runtime-wry 2.11.4、tauri-plugin-global-shortcut 2.3.2、atomicwrites 0.4.4。
>
> **与评审稿的关系**：本文取代 `docs/reviews/2026-09-27-workflow-lifecycle-simplification-plan.md`。评审稿的方向保留，任务顺序、PR 切分和若干裁定以本文为准。
>
> **行号约定**：文中行号均为基线行号，路径未加前缀时相对 `backend/tauri/src/`。实施时按符号重新定位，不机械套用行号。
>
> **事实来源**：五份只读盘点（L1a state、L1b owners、L2 lifecycle、L3 workflow、L4 cleanup），每条结论都有代码位置。只从阅读推断、没有复现的内容标注“疑似”。

---

## 1. 目标与约束

### 1.1 目标

- **G1 缩窄 ApplicationWorkflow。** 它只承担运行态（内核、host、service）相关的关键操作。
  - 纯保存与普通副作用不再经过它。
  - 它不再编排应用关停。
  - journal 不再充当调用结果的通道。
- **G2 删除 `ApplicationWorkflowActor` 内重复实现的第二层串行调度。** 需要删除的包括：
  - `pending` / `ActiveOperation` / `Completed` / `drive()`；
  - `MutationContext` 表、准入过期、自动任务与用户命令之间的优先级。

  删除后，ractor mailbox 是唯一的串行化机制，handler 直接 await 一条完整命令。

### 1.2 用户裁定（约束，不再讨论）

| #   | 裁定                                                                                                                                                                                                        |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U1  | 实现尽可能简单：要么用锁，要么直接 actor。不在 ractor 之上自造调度层，不自造 timeout 机制。不用 op id / snapshot id 标记顺序来实现调度、优先级或回退。不用 Java 式 `of()` 构造。                            |
| U2  | 除网络 IO 外不设 timeout。同进程调用用 `None`，等待真实结果。                                                                                                                                               |
| U3  | IPC 客户端缺少请求超时，属于下层（runtime 子模块 `nyanpasu_ipc`），另开 PR 修复。本计划只写 roadmap、代码 TODO 和 PR 备注，不在上层兜底。                                                                   |
| U4  | 改造顺序自底向上：先底层模块，再通用架构（组合根、`NyanpasuClient` 生命周期），最后编排层（application workflow）。                                                                                         |
| U5  | panic 表示到达了不应到达的状态，必须中断执行。生产代码中的 `catch_unwind` 全部删除。确实要运行不可信的外部库时，应放到独立 std 线程并配 exception handler（本次不做）。                                     |
| U6  | 磁盘写入是 write + rename，本身是原子的。写失败不做磁盘回滚：返回错误，候选不提交。                                                                                                                         |
| U7  | 源持久化失败，而 Runtime 的 Try 已经把候选应用到内核时：Runtime 回滚到旧配置，并返回包含具体错误的错误。                                                                                                    |
| U8  | 主线程执行通过注入的、类型擦除的 `MainThreadExecutor` 完成：运行期用 Tauri 适配器，测试用 fake。client 不需要关心主线程。                                                                                   |
| U9  | 资源竞争只用三种方式解决：锁、消息管道，或架构拆分。<br>• 服务间通知是树形依赖，不构成图。<br>• mutation 由用户触发（将来可能加 watcher），触发面也是树形的。<br>• 同一资源上的并发操作退化为类似锁的串行。 |
| U10 | 声称存在竞争或缺陷，必须给出代码位置和复现办法，经用户同意后才写测试复现。                                                                                                                                  |
| U11 | F2（启动）本次只在 `resolve_setup` 处加 TODO，并把调整建议写进 docs 和 PR（§9）。`resolve_setup` 另行重构。                                                                                                 |
| U12 | Layer 2 与 Layer 3 共用一个 PR。                                                                                                                                                                            |
| U13 | 实施模式由用户在开工前指定。                                                                                                                                                                                |

### 1.3 对 2026-09-27 Fable 审计的处理

该审计把现有机制当作必须保留的保障，这违背 U1–U5，所以**它“保留 timeout / panic 隔离 / 优先级 / 版本号，推迟删除”的建议全部作废**。审计中的事实部分仍然采用：

| 审计项                                                                                                                                        | 在本计划中的处理                                                              |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| F1：`nyanpasu_ipc` 的 reqwest 客户端没有请求超时（`backend/nyanpasu-runtime/nyanpasu_ipc/src/client/mod.rs:116-126`），`wait_ms` 只约束服务端 | 属于下层问题，按 U3 另开 PR-0，本计划只登记                                   |
| F2：`resolve.rs:173` 在 setup 里 `block_on(startup_reconcile())`                                                                              | U11：加 TODO，并写入 §9                                                       |
| F3 / F7：退出在主线程 `block_on`；热键注销要在主线程往返一次                                                                                  | 作为实施顺序约束：非阻塞退出（L2-1）必须早于删除 `HOTKEY_RPC_TIMEOUT`（L2-2） |
| F10：`queued` 字段需要连同前端一起删                                                                                                          | 纳入 L3-2                                                                     |
| F12：被拒绝的 `ReplaceCoreBinary` 必须对 `progress.finished` 恰好调用一次                                                                     | 列为 V27                                                                      |
| F14：§2.2 表中的事实订正                                                                                                                      | 纳入 §2                                                                       |
| F15：`SkippedShutdown`                                                                                                                        | 纳入 P1-4                                                                     |
| F17：`with_pending_state_timeout` 无消费者                                                                                                    | 纳入 P1-5                                                                     |

- **F4 是事实错误。** 生产 panic hook（`lib.rs:62-117`）遇到任何 panic 都会弹框并 `exit(1)`，所以 `catch_unwind` 隔离在生产中本来就不生效。
- **F5 撤回。** 今天 effects `committed()` 的唯一生产发送方是 `client/application_workflow/workflow.rs:89`，没有可复现的对象。它转为 L3-4 的树形通知设计。

---

## 2. 事实基线

下表是决定设计的已核实事实。

| 事实                                                                                                                                                                                                                                                                                                                                                               | 证据                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **生产中没有任何 coordinator 注册常驻 subscriber。** 唯一的生产 `StateAckSubscriber` 是 `ApplicationMutationParticipant`，它只覆盖 `name` / `ack_options` / `on_prepare` 三个方法。                                                                                                                                                                                | `client/application_workflow/participant.rs:110-146`；`add_subscriber` / `with_subscriber` 只在测试中出现（`client/mod.rs:2111`，`tests/closing.rs:203`，`tests/mutations.rs:1064`） |
| 生产中唯一起作用的 ACK 期限，是 participant 的 90 s `MUTATION_ACK_TIMEOUT`。默认 30 s，以及 committed / rolled-back 两个期限，只包着 no-op 回调。                                                                                                                                                                                                                  | `participant.rs:52,115-117`；`backend/nyanpasu-core/src/state/ack.rs:53-57`                                                                                                          |
| 生产入口：`replace_if_version_with_participant` 共 4 处，`upsert` 1 处。没有生产代码使用 `replace_if_version`、`with_pending_state_timeout`、Simple / Weak / Builder 这几个 manager。                                                                                                                                                                              | `state/application.rs:119`，`state/clash_config.rs:106`，`state/profiles/actor.rs:468,1136`；`state/session_state.rs:60`                                                             |
| 所有 store 写入都在同一个 writer permit 之下进行，**生产中不可能出现 CAS 失配**。                                                                                                                                                                                                                                                                                  | `nyanpasu-core/src/state/transaction.rs:532,552`；`snapshot.rs:6-19`                                                                                                                 |
| 配置写入是 `AtomicFile`：先写同目录临时文件，`sync_all`，再 rename。写失败时旧文件保持不变，恢复性回写因 `config_written = false` 被跳过。                                                                                                                                                                                                                         | atomicwrites-0.4.4 `lib.rs:118-121,134-164,179-202`；`nyanpasu-core/.../persistent_state.rs:346-350,456-460`                                                                         |
| **一旦 `on_prepare` 被调用，每一条出口都会发布决定。** 各出口的发布点：<br>• `RollbackGuard` 在 prepare 之前已经上膛，drop 时发布 Aborted；<br>• 版本冲突在调用 participant 之前就 abort；<br>• 调用方取消也会 abort。<br>所以删除 `decision_wait` 不会因为写入方被丢弃而挂死。                                                                                    | `transaction.rs:355-360,182-195`；`coordinator.rs:414-421`；`persistent_state.rs:473-477`；`decision.rs:44-54`                                                                       |
| **Cancel 今天已经会把旧运行配置恢复回内核。** 恢复方式是用 baseline receipt 的 `config_text` 重新 reconcile，然后用一次新的观察去核验。                                                                                                                                                                                                                            | `client/application_workflow/tcc.rs:1393-1412` → `restore` `:1460-1606`                                                                                                              |
| 缺的是调用方这一侧：源持久化失败时，`?` 直接返回，跳过了 `finish`。Cancel 与这次回复并发进行，它的结果只写进 journal。                                                                                                                                                                                                                                             | `state/application.rs:117-127`、`clash_config.rs:104-114`、`profiles/actor.rs:466-479,1134-1225`；测试 `tests/mod.rs:1262-1304` 只能等 journal                                       |
| 今天**每一次保存**都会安装 participant。不影响运行态的保存会拿到一个伪 baseline，并且在 Runtime 隔离期间被拒绝。                                                                                                                                                                                                                                                   | `state/application.rs:116`、`clash_config.rs:103`、`profiles/actor.rs:462,1125`；`tcc.rs:168-179`；`mod.rs:596-597`                                                                  |
| **Runtime handler 等待决定时，不存在等待环。**<br>• Runtime 只通过 `StateSnapshot` 读取三个域；<br>• 候选随请求一起传入；<br>• profile 内容直接从磁盘读；<br>• 发出的通知都是 cast。                                                                                                                                                                               | `application_workflow/preparation.rs:21-31`；`mutation.rs:42-62`；`adapters.rs:17-36`；`client/effects/actor.rs:560-588`                                                             |
| ractor 0.16.5 的相关行为：<br>• handler 只会被 Kill 中断，本应用不发送 Kill；<br>• `drain()` 把标记排在已入队消息之后，此后的发送（包括 actor 给自己的 cast）一律被拒；<br>• `drain_and_wait(None)` 可用；<br>• handler panic 时 actor 停止，不执行 `post_stop`；<br>• `send_interval` 在 actor 忙碌时仍持续入队。                                                 | `ractor-0.16.5/src/actor.rs:905-912,945-1030`；`actor/actor_properties.rs:270-282`；`actor/actor_cell.rs:532-556`；`time.rs:110-132`                                                 |
| tokio-util 0.7.19 的 `TaskTracker::close()` 并不阻止新任务加入。                                                                                                                                                                                                                                                                                                   | `task/task_tracker.rs:329-339`                                                                                                                                                       |
| Tauri 的相关行为：<br>• `ExitRequested` 回调在主线程上同步执行，回调返回后立即读取 prevent；<br>• 对 `RESTART_EXIT_CODE`，`prevent_exit` 不生效；<br>• 在主线程上调用 `run_on_main_thread` 会直接内联执行。<br>global-shortcut 插件的 `run_main_thread!` 是 `run_on_main_thread` 加一次阻塞的 `recv()`。                                                           | tauri-runtime-wry-2.11.4 `lib.rs:4306-4366,235-249`；tauri-2.11.5 `app.rs:77,86-95`；global-shortcut-2.3.2 `lib.rs:75-86`                                                            |
| **今天的退出会阻塞主线程。**<br>• `lib.rs:327-329` → `help.rs:210` 在主线程上 `block_on`（第二个 runtime）；<br>• 重启走 `help.rs:221-239`：同样 `block_on`，之后 `std::process::exit(0)`；<br>• IPC `cleanup_processes` 是同步命令（`ipc.rs:1012-1017`）；<br>• 前端 `nyanpasu-version.tsx:168` 先 await `cleanupProcesses()`，然后才 `install()`、`relaunch()`。 | 见左                                                                                                                                                                                 |
| 今天唯一挡住“热键注销需要主线程往返”这一步的，是 `HOTKEY_RPC_TIMEOUT = 5 s`。                                                                                                                                                                                                                                                                                      | `client/hotkey/mod.rs:45`；`hotkey/actor.rs:331-341`                                                                                                                                 |
| 生产 panic hook 遇到任何 panic 都会退出。唯一例外是 tauri#10546 的 PostMessage payload。                                                                                                                                                                                                                                                                           | `lib.rs:62-117`（例外在 `:92-97`）                                                                                                                                                   |
| 所有 `Actor::spawn` 的 JoinHandle 都被丢弃。唯一现成的“令牌 + 跟踪器”是 `ProducerTasks`，只覆盖 4 个边界 producer。                                                                                                                                                                                                                                                | `client/app_lifecycle.rs:19-55`；L2 盘点 §4                                                                                                                                          |
| 部件（widget）的 D1 依赖：今天只有 effects 在关停时 abort group 2，才能释放 widget 的两把锁；而 widget 的握手没有期限。                                                                                                                                                                                                                                            | `client/ui_effects/adapters.rs:124,143,152`；`widget.rs:163,186-194`                                                                                                                 |
| effects 的输入**已经按域切好**：`ApplicationEffectInputs { app, clash, ports }`。源 actor 在 `EffectsClient` 之前 spawn。                                                                                                                                                                                                                                          | `client/effects/plan.rs:21-27`；`client/mod.rs:223` 对比 `:309`                                                                                                                      |
| 架构 ledger 的所有指标都是 0。`TODO(actor-migration)` 会被计为 `migration_markers`，**新写的 TODO 不能用这个标签**。                                                                                                                                                                                                                                               | `scripts/architecture-ledger.ts:45`                                                                                                                                                  |
| 启动时的状态：<br>• `startup_reconcile` 只受 `CALL_WAIT` 180 s 约束；<br>• 前端没有监听 `CoreStatusChangedEvent`；<br>• `CoreStatusBadge` 把 Starting 显示为“已停止”。                                                                                                                                                                                             | `resolve.rs:173`；`client/application_workflow/mod.rs:51`；`widget-shortcut.tsx:147-187`                                                                                             |

---

## 3. 目标架构

### 3.1 依赖方向（树形，只向下游通知）

```text
Tauri 边界（commands / tray / hotkey pump / ExitBoundary）
                 │
          NyanpasuClient（门面；持有根 CancellationToken + TaskTracker）
                 │
   ┌─────────────┼───────────────┬──────────────┐
Application   ClashConfig      Profiles      SessionState        ← 源 owner：actor，各自串行
   │  │          │  │            │  │
   │  └── Required participant（仅运行态相关修改）──► Runtime owner（ApplicationWorkflowActor）──► Core / Service
   │             │               │                         │
   └── app 切片 ─┴─ clash 切片 ──┴─ 托盘刷新 ─────────────┴── ports 切片 ──► Effects owner
                                                                              ├─► SystemProxy / Hotkey / Widget
                                                                              └─► Tray / Locale / Logger
```

- 每条通知边都只指向下游。任何 owner 都不读取、不转发兄弟域的快照（U9）。
- 同一个域的切片只有一个发送方，也就是该域的串行 owner。ractor mailbox 保证同一发送方的消息按顺序到达，所以不需要版本号。

### 3.2 Owner、串行化与退出职责

| Owner                                 | 形态               | 串行化                                                       | 收到根令牌后的职责                                                                                    |
| ------------------------------------- | ------------------ | ------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| Application / ClashConfig             | actor              | mailbox；事务在 handler 内执行，`&mut self` 即锁（P1-7）     | 入口拒绝新写入；排队的请求按拒绝处理；当前事务结算完成后结束                                          |
| Profiles                              | actor              | 同上                                                         | 入口拒绝新写入和新 producer；post_stop 中止下载、停止 watcher 和调度，并 await 这些任务（不设上限）   |
| SessionState                          | actor              | mailbox                                                      | **不做准入检查**；drain 时处理已经入队的最终几何保存，写完再结束                                      |
| Runtime（`ApplicationWorkflowActor`） | actor              | 只用 mailbox；handler await 整条命令                         | 入口拒绝新命令（用带外令牌判断）；当前命令结算后，在 post_stop 中关停 Core / Service                  |
| Effects                               | actor              | mailbox，外加 3 个组、每组最多一个在途任务（真实的独立并发） | 入口不再接收 Publish / Tick；post_stop await 在途的组任务，不 abort                                   |
| SystemProxy                           | actor              | mailbox                                                      | post_stop 停止 guard，然后执行已有的 `restore()`（等待自己的 OS 写入）                                |
| Hotkey                                | actor              | mailbox                                                      | post_stop 经 `MainThreadExecutor` 执行 `unregister_all`                                               |
| Widget                                | 普通结构体 + Mutex | 锁                                                           | 握手 `select!` 在令牌取消时结束；被跟踪的任务调用 `stop(now + WIDGET_STOP_BOUND)`（进程边界期限保留） |
| Updater                               | actor              | mailbox                                                      | 下载阶段可以取消；已开始的安装要等它完成                                                              |
| 边界 producer（4 个）                 | 被跟踪的任务       | 无                                                           | 令牌取消即结束（沿用 `ProducerTasks::track` 的写法）                                                  |
| Streams / Proxies / Logs / server     | —                  | —                                                            | 没有持久副作用，不跟踪，交给进程退出（Q-G）                                                           |

### 3.3 一次运行态相关修改的执行链（目标）

```text
源 actor handler
  impact = runtime_impact(previous, candidate, hints, class)        ← 唯一的纯分类函数（L3-6）
  ├─ None（纯保存 / 普通副作用）
  │     replace（不带 participant）→ 提交 → 发送本域切片给 effects → Ok(receipt: Unchanged)
  └─ Some(impact)
        (participant, settlement_rx) = coordinator.participant(op, hints, class, impact)
        result = manager.replace_if_version_with_participant(..).await
        settlement = settlement_rx.await       ← 总是等待；participant 从未被调用时立即返回 Err
        发送本域切片给 effects
        按 (result, settlement) 映射出 Ok(receipt, 降级) 或 Err(具体原因 + 回滚结果)

participant.on_prepare(change)
  cast Mutation(MutationRequest{op, change, hints, class, impact, decision, ack, settle})
  await ack（不设期限）

Runtime handler(Mutation)
  入口：令牌已取消 / 已隔离 / 决定已落定 → ack Rejected，丢弃 settle
  Try（构建 / 校验 / 提交到内核）→ ack
  decision.wait().await                 （不设期限）
  Committed → Confirm → 发送 ports 切片
  Aborted   → Cancel（按 baseline 恢复）→ 发送 ports 切片
  settle.send(receipt)；在 journal 中记一行（仅展示）
```

| 路径                                            | 调用方得到的结果                                                                                                                  |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 不影响运行态                                    | `Ok`，runtime 为 `Unchanged`；不与 Runtime 发生任何接触，不分配 OperationId                                                       |
| 组合尚未就绪                                    | `Err("application workflow is not ready")`，所有写入都走同一道现有的 `ensure_ready` 门                                            |
| participant 从未被调用（版本冲突）              | `Err(VersionConflict)`；修正 app / clash 忽略 `Conflict` 的问题（`state/application.rs:117-127`）                                 |
| 请求没有送达（cast 失败）                       | `Err`：“Runtime owner 不可用，未提交任何修改”（`Ack::Rejected`，见 L4-1）                                                         |
| 入口被拒（退出中 / 已隔离 / 已决定）            | `Err`，错误文本里带上拒绝原因                                                                                                     |
| Try 被拒                                        | `Err`，带拒绝信息；Runtime 结论为 Withdrawn                                                                                       |
| Try 结果未知                                    | `Err`：“结果未知，需要恢复”；Runtime 隔离                                                                                         |
| 提交成功                                        | `Ok(receipt, 降级)`；沿用 `state/mutation.rs:68-118` 的映射，去掉 `Pending` / `operation_pending`                                 |
| **持久化失败（U7）**                            | `Err` = 持久化的完整原因链，再加上以下之一：<br>• “运行态已回滚到上一份配置”；<br>• “运行态回滚失败：{restore error}；需要恢复”。 |
| Runtime owner 在 Try Ok 之后消失（panic = bug） | 源已经提交，返回 `Ok`，附 `runtime_recovery_required` 降级（复用现有 code，不新增类型）                                           |

### 3.4 关停与退出

- **`NyanpasuClient::request_shutdown(&self)`**：取消根令牌，然后 `tasks.close()`。可以幂等地重复调用。
- **`NyanpasuClient::wait_shutdown(&self)`**：`tasks.wait().await`。
- **每个被跟踪的 actor owner** 只需一段薄接线：`tracker.spawn(async move { token.cancelled().await; cell.drain_and_wait(None).await })`。
  - 准入由 handler 入口检查令牌完成。清理放在 `post_stop`。
  - drain 会拒绝 actor 给自己的 cast，所以通过自发 cast 回报结果的 owner（effects 的 `Completed`、updater 的 `Fetched` / `Finished`），在 post_stop 里直接 await 自己的任务句柄。
- **Runtime 在 L2 阶段仍带旧调度器。** 旧调度器依赖自发的 `Completed`，与 drain 不兼容，因此 L2-2 暂用现有的 `Close` 路径：`token.cancelled() → cast(Close) → cell.wait(None)`。L3-2 删除旧调度后，Runtime 改为统一的 drain 加 post_stop。这是同一个 PR 内的相邻提交，不留兼容层。
- **退出边界**（`utils/exit.rs`，由 Tauri 管理状态，**不新增 static**）用纯状态机 `ExitGate` 决定每次 `ExitRequested` 的去向：

| 状态     | `ExitRequested{None}` | `ExitRequested{Some(RESTART)}`             | `ExitRequested{Some(c)}`                                                                                      |
| -------- | --------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| Running  | prevent（留在托盘）   | 无法阻止：记 warn “未完成清理即重启”，放行 | prevent，同步捕获几何，cast 最终保存，`request_shutdown`，记下意图 `Exit(c)`，spawn 唯一的等待任务 → Stopping |
| Stopping | prevent               | 无法阻止：warn，放行                       | prevent（不重复启动）                                                                                         |
| Finished | prevent               | 放行                                       | 放行                                                                                                          |

- **等待任务**依次执行：
  1. `wait_shutdown().await`；
  2. 置为 Finished；
  3. 调用 `shutdown_hook::set_ready_for_shutdown()`（Windows）；
  4. 若意图是 Restart，此时才 spawn `launch` 重启器；
  5. `app_handle.exit(code)`，这次会被放行。
- **托盘 / IPC 的重启**只设意图 `Restart`，然后 `exit(0)`，不再调用 `std::process::exit`（Q-F）。
- **IPC `cleanup_processes`** 改为 async 命令：`request_shutdown()` 后 await `wait_shutdown()`，不退出应用，结束后置为 Finished。之后前端的 `relaunch()`（RESTART）或 Windows 安装器的 `process::exit` 都发生在清理完成之后，行为与今天一致。
- **不设任何超时**（U2）。已知限制见 §13。

### 3.5 主线程执行（U8）

```rust
/// Runs work on the UI thread. An implementation may run `task` before
/// returning when it is already on that thread (Tauri does).
pub trait MainThreadExecutor: Send + Sync + 'static {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> anyhow::Result<()>;
}

impl dyn MainThreadExecutor {
    /// Runs `task` on the main thread and returns its result. Errors only when
    /// the event loop refuses or drops the task.
    pub async fn run<T: Send + 'static>(
        &self,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> anyhow::Result<T> { /* tokio oneshot，不设期限 */ }
}
```

- **Tauri 适配器 `TauriMainThread(AppHandle)`** 放在 `client/event_sink.rs`，与 `TauriUiEventSink` 相邻。
- **测试 fake `InlineMainThread`** 直接执行任务，放在 `#[cfg(test)]` 下。
- **在组合根构造**：`setup.rs:45` 之后。
- **本次只注入两处**：`TauriShortcutRegistrar`（`setup.rs:217`）与 `TauriWindowControl`（`setup.rs:122`）。
  - `NyanpasuClient`、hotkey actor 和 effects 都看不到它。
  - 其余零散的 `run_on_main_thread` 调用（`core/tray/mod.rs:221`、`ipc.rs:1309/1324/1359`、`resolve.rs:160/240`）列入 §11，本次不动。

---

## 4. 分层、PR 与分支

| 层  | 内容                                                               | PR                                             | 分支（基于）                                                       |
| --- | ------------------------------------------------------------------ | ---------------------------------------------- | ------------------------------------------------------------------ |
| L0  | IPC 客户端请求超时（`nyanpasu_ipc`，runtime 子模块）               | PR-0，**本计划不执行**，只登记 TODO 与 roadmap | 上游 nyanpasu-runtime                                              |
| L1  | 底层模块：nyanpasu-core state（L1a）；资源 owner 的内部实现（L1b） | PR-1                                           | `refactor/lifecycle-lower-modules`（← `refactor/tcc-t11-cleanup`） |
| L2  | 通用架构：非阻塞退出边界、根令牌加 owner 自行收尾                  | PR-2                                           | `refactor/owner-shutdown-and-workflow`（← PR-1）                   |
| L3  | 编排：workflow 直接执行、结果直返、源侧分类与旁路、树形通知        | PR-2（U12）                                    | 同上                                                               |
| L4  | 收口：`TryAck`、恢复状态、`of()`、文档                             | PR-3                                           | `refactor/workflow-residue-cleanup`（← PR-2）                      |

提交规则按 AGENTS §18：

- 每个提交都可以构建；后端 wire 改动与前端使用方放在同一个提交；bindings 只用导出测试生成。
- 修复折叠进它所属的提交，不做 fix-up 提交。
- 栈底 #5385–#5389 若被上游 squash 合并，用 `git rebase --onto` 去掉重复提交（先例：记忆 pr5-v2-review）。

---

## 5. PR-1：底层模块（L1）

### P1-1 `docs(plan): plan the workflow and lifecycle simplification`

- 新增本文件。
- 评审稿 `docs/reviews/2026-09-27-workflow-lifecycle-simplification-plan.md` 目前未入库，作为输入一并入库（Q-I），并在顶部加一行，指向本文。

### P1-2 `docs(agents): scope deadlines to network IO and state the ownership rules`

- **`AGENTS.md:135` 与 `:238`**：把 “Prefer finite timeouts for cross-actor request/reply calls …” 替换为：
  - 同进程调用用 `None`，等待真实结果；
  - 只有网络 IO 的 adapter 定义期限；
  - IPC 的期限属于 IPC 层。
- **在 §6 / §8 中增加以下规则**（中文或英文与原文保持一致）：
  - panic = bug，生产代码中不写 `catch_unwind`；
  - 一个 owner 只用一种串行化机制，不在 actor 内再建队列或调度；
  - 等待者不拥有操作：调用方被丢弃不会取消已开始的工作；
  - 关停 = 根 CancellationToken 加 owner 各自收尾，没有全局阶段或预算；
  - 主线程工作经注入的 `MainThreadExecutor` 执行；
  - 通知只向下游，按域发送。
- **§10 第 4 条补充说明**：Runtime participant 是提交**前**的 Required 投票；“先提交再副作用”只适用于普通副作用。
- `CLAUDE.md` 只有一行 `@AGENTS.md`，不需要编辑。

### P1-3 `refactor(state): stop timing subscriber acknowledgements`

- **nyanpasu-core `state/ack.rs`：**
  - 删除 `AckOptions`（`:31-58`）。trait 方法改为 `fn policy(&self) -> AckPolicy { AckPolicy::Required }`，替换 `ack_options`（`:175-178`）及 `Arc` 的委托（`:209-211`）。
  - 删除 `SubscriberAck.timeout`（`:262-269`）、`AckStatus::TimedOut`（`:232-247`）、`SubscriberFailureKind::TimedOut`（`:108`）、`RollbackReason::Timeout`（`:119-121`）。
  - `is_required_failure` 与 `has_advisory_failures`（`:271-279`、`:297-305`）不再匹配 `TimedOut`。
  - 重写 trait 文档（`:142-164`）：不再有本地期限；`on_prepare` 不得 RPC 回源 actor，违反即永久挂起。
- **`state/transaction/notify.rs`**：
  - 删除 `tokio::time::timeout` 包装（`:47`、`:100`、`:146-159`）、`Err(_)` 分支（`:71-78`、`:122-128`）和 `timeout` 字段（`:39`、`:83`、`:189`）。
  - 删除 `Duration` 的 import。
- **`state/transaction.rs:415-422`**：删掉 `TimedOut` 分支；更新文档（`:93-96`）。
- **`persistent_state.rs:384-390`**：更新文档。
- **backend/tauri：**
  - `participant.rs`：删除 `MUTATION_ACK_TIMEOUT`（`:52`）、`ack_timeout`（`:60`）、`with_ack_timeout`（`:76-106`）、`ack_options` 覆盖（`:115-117`），以及 `AckOptions` / `Duration` 的 import；更新文档（`:31-51`）。
  - `state/mutation.rs:169-171`：删除 `IsolatedParticipant` 的覆盖。
- **登记 IPC 期限（U3）：**
  - 在 `setup.rs:74-75` 加 `// TODO(ipc-timeout): nyanpasu_ipc::Client 未设请求超时；上游设置后删除 core/actor_v2 的外层包装。` **不得使用 `TODO(actor-migration)`**，否则 ledger 会计数。
  - 在 `docs/design/actor-migration-roadmap.md` §12.2 增加一行：Owner = 上游 nyanpasu-runtime；移除条件 = `Client::new` 设置请求超时，本仓库 bump 子模块。
- **测试：**

| 测试                                                                                                               | 处理                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| core `ack.rs:318-334` `advisory_rejected_counts_as_failure`                                                        | 删除 `timeout` 字段                                                                                                                                                                                            |
| `coordinator.rs:1026-1056` `test_advisory_ack_failure_is_ok`                                                       | 改用 `policy()`                                                                                                                                                                                                |
| `coordinator.rs:1865-1896` `test_advisory_failure_report_helper`                                                   | 改用 `policy()`                                                                                                                                                                                                |
| `coordinator.rs:1229-1269` `test_timeout_subscriber`                                                               | **重写为 V01**：`start_paused`，Required subscriber 停在一个 `Notify` 上，`advance(>90 s)` 后 upsert 仍 pending；释放后返回 `Ok`，并已提交                                                                     |
| `coordinator.rs:1778-1836` `test_build_initialized_prepare_timeout_returns_init_ack_error`                         | 删除（init 否决已由 4 个 `*_ack_failure_returns_recoverable_manager` 覆盖）                                                                                                                                    |
| `single_shot_participant.rs:117-216`                                                                               | 改用 `RollbackReason::CoordinatorError(..)`                                                                                                                                                                    |
| `single_shot_participant.rs:220-284` `a_committed_decision_survives_a_lost_commit_notification`                    | **重写**：把 `on_committed` 停在 `Notify` 上，断言此时 `decision() == Committed{1}`；释放后返回 `Replaced`（否则会真实 sleep 3600 s）                                                                          |
| tauri `tests/mutations.rs` 的辅助函数 `mutate` / `mutate_with_hints` / `simple_mutate`（`:459-563`）及其 15 处调用 | 去掉 `ack_timeout` 参数                                                                                                                                                                                        |
| `DropSignal::ack_options`（`:715-717`）                                                                            | 改为 `policy()` 委托                                                                                                                                                                                           |
| `a_host_switch_moves_the_runtime_inside_the_try_and_back_on_cancel`（`:2181-2188`）                                | 改用 `new`                                                                                                                                                                                                     |
| **`an_abandoned_prepare_waits_for_the_try_before_restoring_and_releasing`**（`:1106-1212`）                        | 删除（前提是 50 ms 的 ACK 超时）。它守护的不变量是：Cancel 的恢复完成之前，域一直被占用，下一项不开始。这个不变量移到否决路径上：扩展 `:1042`（Try 已应用后被否决 → Cancel），断言下一项修改在恢复完成后才开始 |
| **`an_expired_ack_keeps_the_domain_until_the_handoff_is_compensated`**（`:2404-…`）                                | 删除（它也是 T10 记录中已知会挂住的测试）。“handoff 补偿完成才放行下一项”由 `:2133` 覆盖，并按否决路径补一条断言                                                                                               |

- **验收**：
  - V01；
  - 在 `nyanpasu-core/src/state` 与 `backend/tauri/src` 中执行 `rg 'TimedOut|ack_timeout|MUTATION_ACK_TIMEOUT'`，生产代码命中 0 处。

### P1-4 `refactor(state): drop the shutdown-skip hook`

- 删除 `is_shutdown`（`ack.rs:170-173`、`:205-207`）与 `AckStatus::SkippedShutdown`（`:245-246`）。
- 删除 `notify.rs:35-43`、`:95-97`、`:142-144`，以及剪枝循环 `transaction.rs:379-385`。Sequential 路径（`:399-406`）自己的 `update_subscribers` 保留。
- 契约写入 trait 文档：无法服务的 subscriber 在 `on_prepare` 中返回 `Failed` 或 `Rejected`。唯一的生产 participant 今天已经是这样做的（`participant.rs:124-144`）。
- 测试：删除 `coordinator.rs:1271-1303`、`:1305-1339`。Required 否决已由 `test_required_ack_failure_prevents_commit`（`:889-914`）覆盖。

### P1-5 `refactor(state): remove the unused effect timeout`

- **`coordinator.rs`：**
  - 删除 `with_pending_state_timeout`（`:271-301`）；
  - 删除 `run_pending_state` 的 `effect_timeout` 参数（`:376-384`），以及 4 个调用点对应的实参（`:266`、`:291-298`、`:318-325`、`:346-353`）；
  - 删除超时匹配（`:455-461`）、`EffectTimedOut` 分支（`:465-472`）和 `unreachable!`（`:483-485`）；
  - `Duration` 的 import 移进 test 模块。
- `error.rs:175-176`（连同 `:2` 的 import）、`persistent_state.rs:264-266`、`:515-519`、`persistent_builder.rs:239-241`：删除对应的变体和分支。
- 测试：删除 `coordinator.rs:1523-1561`。“effect 失败时不消耗 change id”已由 `:1489-1521` 覆盖。

### P1-6 `refactor(state): let notification and write panics propagate`（U5）

- `notify.rs:171-201`：删除合成的 `<notify task join failure>` Required 失败（`:182-197`），改为 `res.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))`。`:231-242`、`:266-278` 同样处理。
- `spawn_blocking(..).await?`：panic 改为继续传播，其余错误照旧。涉及 `persistent_state.rs:309-314`、`persistent_builder.rs:209-210`、`weak_persistent_state.rs:189-192`。
- `persistent_state.rs:491-495`：如果 P1-7 被否决，改为 `.expect("source persistence owner panicked")`；如果 P1-7 执行，这段代码随之消失。

### P1-7 `refactor(state): run the participant transaction on the caller`（确认项 Q-A）

- **目标**：事务直接在源 actor 的 handler 内执行。“锁或 actor”：`&mut self` 本身就是锁，actor mailbox 已经串行，不再需要分离的任务和第二把锁。
- **删除**（`persistent_state.rs:395-496`）：
  - `persistence_owner()`（`coordinator.rs:74-84`）；
  - watch 对（`:421-422`）、`started`（`:423-424`、`:439`）；
  - 两次 `has_changed` 检查（`:434-438`、`:444-448`）；
  - `select!`（`:469-483`）；
  - oneshot 与 spawn（`:420`、`:428-495`）；
  - 多余的 `'static` bound（`:404-409`）。
- **`RollbackGuard`：**
  - **保留 drop 时发布决定。** panic 展开时，这是 Runtime 不会永久停在 `Undecided` 的保证。
  - 删除分离任务里重跑回滚通知、并接力 permit 的逻辑（`transaction.rs:214-228`）。生产中没有 subscriber 实现 `on_rolled_back`。
- **semaphore**：实施时核对 coordinator 的入口签名。
  - 如果都经由持有 `&mut self` 的 manager 进入，删除 semaphore；
  - 否则保留它作为唯一的锁。

  二选一，不能同时保留两套。

- **测试：**
  - 改写或删除以下 caller-drop 测试：`single_shot_participant.rs:427,482,549,721`，tauri `tests/mutations.rs:1474`、`tests/recovery.rs:1151`。新语义是 owner 执行到终态（V03）。
  - `transaction.rs:742,802,881,950,998` 按分离重跑被删除的情况调整。
- **风险**：`upsert` 仍使用 `spawn_blocking`。调用方被丢弃时，孤立写入可能与后续写入者竞争（L1a S1，疑似）。在新模型下，调用方只会在 panic 或 runtime 拆除时被丢弃，不做额外处理。

### P1-8 `refactor(state): drop the config write-back after a store race`（确认项 Q-B）

- 删除以下代码。它们只在 CAS 失配时回写磁盘，而 CAS 失配在生产中不可达，且这属于 U6 排除的磁盘回滚：
  - `config_write_steps` 的恢复半段（`persistent_state.rs:333`、`:345-351`）；
  - `config_written` 分支（`:416-417`、`:450`、`:456-460`）；
  - builder 恢复（`persistent_builder.rs:198-200`、`:215-227`）；
  - CAS 恢复路径（`coordinator.rs:507-532`，改为直接返回 `StateCasMismatch`）；
  - `WithEffectError::Recovery`（`error.rs:183-192`）、`UpsertError::Recovery`（`:150-158`）、`ReplaceIfVersionError::Recovery`（`persistent_state.rs:164-175`）、`InconsistentPersistence`（`error.rs:111-137`）。
- **不动** `local_recovery`。profiles 的 compensate 撤销的是一次成功的 promote，不是失败写入的回滚。
- 测试：`persistence_settlement.rs:17,69`、`single_shot_participant.rs:622,799,895`、`coordinator.rs:1564,1617,1665` 改为断言“CAS 失配 → 返回错误，store 不变”，或删除。

### P1-9 `feat(client): run hotkey registration on an injected main-thread executor`（U8）

- 新增 `client/main_thread.rs`：trait、`impl dyn MainThreadExecutor { async fn run }`，以及 `#[cfg(test)] InlineMainThread`。
- `client/event_sink.rs`：新增 `TauriMainThread`。
- **`client/hotkey/ports.rs:272`**：`ShortcutRegistrar` 改为 async（`validate` 仍为同步）。**`#[cfg_attr(test, mockall::automock)]` 必须写在 `#[async_trait]` 之上**（参照 `WindowControl` 在 `ports.rs:295-297` 的写法），这样 `ui_effects/tests.rs:127` 不用改。
- **`hotkey/adapters.rs`：**
  - `TauriShortcutRegistrar` 持有 `Arc<dyn MainThreadExecutor>`，把每个方法的完整插件调用序列放进同一个主线程任务。例如 register = `is_registered` → `unregister` → `on_shortcut`（`:68-86`）。这样插件内部的 `run_main_thread!` 会内联执行，插件的 map 锁也在主线程上获取。这顺带消除了 L1b 疑似的跨线程锁等待（插件 `lib.rs:221-225`，未复现）。
  - `TauriWindowControl`（`:141-156`）改用 `run`。
- `hotkey/actor.rs`：删除 `blocking()`（`:331-341`）；`:251`、`:268`、`:274` 直接 await 注册器。
- `setup.rs`：在 `:45` 之后构造执行器，经 `build_application_effects`（`:181-186`）传给 `:122` 与 `:217`。
- **本提交保留 `HOTKEY_RPC_TIMEOUT`。** 退出仍在主线程 `block_on`，删掉它会使退出永久挂住。它在 L2-2 删除，那时 L2-1 已经让主线程不再阻塞。
- 测试：
  - `hotkey/tests.rs:183-288` 的 `RecordingRegistrar` 改为 async 实现；
  - 新增 `run` 的单测：用 inline fake 返回结果；任务被丢弃时返回错误（V28）。

### P1-10 `refactor: remove production catch_unwind from the resource owners`（U5）

| 位置                                      | 删除内容                                                                                           | 随之删除的测试与夹具                                                                                                            |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `client/effects/actor.rs:276-279`         | `catch_unwind`、`:4` 的 import，以及只有空 vec 才能到达的 `effect_owner_silent` 分支（`:364-370`） | 无                                                                                                                              |
| `core/tray/executor.rs:163-174`           | `Unwind` drop guard                                                                                | `a_panicking_step_leaves_the_executor_schedulable`（`:441-467`）、`FakeTray.panics_on`                                          |
| `core/updater/mod.rs:232-238`、`:296-307` | 两处包装，以及 `:2`、`:10` 的 import                                                               | `panicking_fetch_and_prepare_release_admission_for_retry`（`tests.rs:370-394`）、`ReadyBackend` 的 panic 计数与 `consume_panic` |
| `state/profiles/actor.rs:609-621`         | 包装、`:27` 的 import、`:590-591` 的文档句                                                         | `client/profiles.rs:3106-3176`                                                                                                  |

workflow 中的 5 处（`application_workflow/mod.rs:340,409,512-537,541`）依附于被跟踪的任务结构，随 L3-2 一起删除。`app_lifecycle.rs:369-377` 随 L2-2 删除。

### P1-11 `refactor: stop turning worker panics into errors`（U5）

- 对 `JoinError` / oneshot 发送端被丢弃的情况：如果只可能来自 panic，就改为继续传播 panic；真正的取消照旧当作取消处理。实施时逐个确认。
- 涉及位置：
  - `client/system_proxy/actor.rs:844-853`、`system_proxy/adapters.rs:263-272`；
  - `core/updater/instance.rs:164-171`；
  - `core/clash/ws.rs:459-468`；
  - `state/profiles/actor.rs:655-657,854-856,897-900,1075,1141,1150,1292,1470`；
  - `widget.rs:145-157,186-189,252-257,285-290`。
- 例外：`nyanpasu-logging`（子模块）的同类转换登记为上游事项，不在这里改。

### P1-12 `refactor(client): await in-process calls outside the shutdown path`（U2）

- 以下 RPC 超时改为 `None`，并删除它们的 `Timeout` 分支和 `timed_out` / `timeout_health` 辅助函数：
  - updater 的 120 s（`core/updater/mod.rs:428-444`，注释 `:339`）；
  - streams 的 10 s（`core/clash/ws.rs:418-428`、`:704-719`）；
  - proxies 的 120 s（`core/proxies.rs:232-235`、`:336-352`）；
  - session 保存的 10 s（`client/session_state.rs:78-87`，`:116`）；
  - system proxy 的 `SYSTEM_PROXY_RPC_TIMEOUT`（`system_proxy/mod.rs:29-30`，`status()` 只在测试中使用）。
- **关停路径上的期限留到 L2-2 统一删除**：`HOTKEY_RPC_TIMEOUT`、`SYSTEM_PROXY_RESTORE_TIMEOUT`、profiles 的 `stop_producers(timeout)`、effects 的 deadline 参数。这些调用在关停时仍然被旧编排的外层 `Issued::by` 包着，所以本提交不会让退出挂住。
- **不动**：`core/actor_v2` 中 CoreClient 的调用期限（`mod.rs:476,522,545,608,706,730,791,857,1044`）、`facade.rs` 的 `OPERATION_WAIT` 60 s、`CHECK_BUDGET` 45 s、ServiceClient 的命令期限。它们是内核管理层与 IPC 之间的边界期限，由 PR-0 统一处理（§10）。

### PR-1 完成后的状态

- 一次卡住的 Try 会让源永久等待：90 s 的 ACK 上限已经删除（U2、U3）。
- 退出和启动仍受旧的关停预算与 `CALL_WAIT` 约束。
- 已知且可接受：这些约束会在 PR-2 中被一起替换。

---

## 6. PR-2：通用架构（L2）与编排（L3）

### L2-1 `refactor(lifecycle): request the exit without blocking the main thread`

- **新增 `utils/exit.rs`：**
  - `ExitBoundary` 放进 Tauri 管理状态（不是 static），内含纯状态机 `ExitGate`（§3.4 表），`ExitIntent` 取 `Exit(code)` 或 `Restart`。
  - `ExitGate::on_exit_requested(code) -> ExitDecision` 是纯函数，带单测（V30）。
- **`lib.rs:323-329`**：`ExitRequested{Some}` 交给边界处理。
- **L2-1 阶段，等待任务里仍调用旧的 `client.shutdown(request)`**（改为在 tauri async runtime 上 await，不再 `block_on`）。L2-2 把它换成 `request_shutdown` / `wait_shutdown`。
- **`utils/help.rs`：**
  - `cleanup_processes` 不再 `block_on`；
  - `restart_application`（`:221-239`）只设 `Restart` 意图并 `exit(0)`，删除 `std::process::exit(0)`；
  - `quit_application` 不变。
- **`ipc.rs`：**
  - `cleanup_processes`（`:1012-1017`）改为 async：await 关停结束，不退出应用，然后置为 Finished；
  - `restart_application`（`:861-866`）与 `quit_application`（`:1341-1345`）走同一个边界。
- **`WindowEvent::CloseRequested` 的保存**（`lib.rs:334-339` → `resolve.rs:413-415`）：改用新增的 `SessionStateClient::queue_main_window_save(geometry)`（cast）。主线程上不再 `block_on`（Q9 默认）。
- **Windows hook**：`set_ready_for_shutdown()` 改由等待任务在最终退出前调用。`ShutdownState::CleaningUp` 是死分支（`shutdown_hook.rs:94-104`），只在 §11 中提及，不删。
- **重启器**（`cmds/mod.rs:71-89`）：保留 `launch`。它会在单例锁上重试约 4 s，因此必须在所有 owner 结束之后、最终 exit 之前才 spawn。
- 测试：
  - `ExitGate` 状态表单测（V30）；
  - 主线程不阻塞（V22）只能靠 smoke 验证。

### L2-2 `refactor(lifecycle): let each owner tear itself down on the shutdown token`

这一提交是一次不可再分的切换：关停所有权从编排者转交给各个 owner。

- **根令牌与跟踪器：**
  - `ProducerTasks`（`app_lifecycle.rs:19-55`）改为 `NyanpasuClient` 的 `shutdown: CancellationToken` 加 `tasks: TaskTracker`；
  - 因为 SystemProxy、Hotkey 和 executor 都在 client 之前 spawn（`setup.rs:101-102`），它们由组合根 `setup.rs` 创建，再经 `ClientSetupArgs` 传给 client；
  - 新增 `request_shutdown()` / `wait_shutdown()`；
  - 新增一个薄辅助函数 `drain_on_shutdown(tracker, token, cell)`（§3.4）。
- **边界切换**：L2-1 的等待任务改为按顺序执行：
  1. 用 cast 提交最终几何保存（在取消令牌之前入队）；
  2. `request_shutdown()`；
  3. `wait_shutdown()`。
- **各 owner 的接线**：child token 经各自的启动参数注入，不使用全局变量。

| Owner                     | 改动                                                                                                                                                                                                              | 删除                                                                                                                                                                                                                                                                                                    |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SystemProxy               | `SystemProxyArgs.shutdown` 取代自建令牌（`mod.rs:62`）；现有的令牌拒绝检查（`actor.rs:216-223` 等）改读新令牌；post_stop = `stop_guard()` 后接 `restore()`（`:706-757`）                                          | `signal_shutdown`、`restore` 客户端方法、`Message::Restore`、`SYSTEM_PROXY_RESTORE_TIMEOUT`、`timed_out` / `timeout_health`（`mod.rs:151-217`）                                                                                                                                                         |
| Hotkey                    | `Args` 加入令牌；reconcile 在入口检查令牌（`actor.rs:121-129`）；post_stop 的 `unregister_all`（`:103-112`）改为经执行器在主线程完成（此时主线程不再阻塞）                                                        | `HOTKEY_RPC_TIMEOUT`、`timed_out` / `timeout_health`（`mod.rs:42-45,146-161`）、`Message::UnregisterAll` 及其客户端方法                                                                                                                                                                                 |
| Widget（D1）              | `WidgetManager` 构造时接收令牌；握手的 `select!`（`widget.rs:186-194`）在令牌取消时转入 `stop_owned`；监听循环（`:108-125`）同样 `select!` 令牌；一个被跟踪的任务在令牌取消时调用 `stop(now + WIDGET_STOP_BOUND)` | 无；进程边界的期限全部保留                                                                                                                                                                                                                                                                              |
| Effects                   | 令牌取消后，`Publish` / `Tick` / `RetryNow` 在入口直接忽略（取代 `closed` 与 `retries_held`）；post_stop await 在途的组任务，不 abort                                                                             | `Message::Shutdown{deadline}` / `HoldRetries`、`GROUP_REAP_BOUND` 与 abort + 回收逻辑（`actor.rs:21-23,436-481,536-542`）、`ApplicationEffectExecutor::{begin_shutdown, shutdown}`（`executor.rs:234-291`）、`EffectsShutdown`（`ports.rs:15-59`）、`StepOutcome` 在 effects 中的使用、`deadline_after` |
| Profiles                  | 令牌取消后，入口拒绝新 producer 与新写入；post_stop 中止下载，并直接 await 这些任务                                                                                                                               | `StopProducers`、`stop_producers(timeout)`、`DOWNLOAD_STOP_BOUND`、`ProducersStopped.unfinished`（`actor.rs:46-47,218-225,540,573-579,2268-2271`；`client/profiles.rs:112-132`）                                                                                                                        |
| Application / ClashConfig | 令牌取消后，入口拒绝写入                                                                                                                                                                                          | 各自的 `begin_terminate`                                                                                                                                                                                                                                                                                |
| SessionState              | 不做准入检查，drain 时处理已入队的最终保存                                                                                                                                                                        | `begin_terminate`                                                                                                                                                                                                                                                                                       |
| Updater                   | 令牌只中止抓取和下载阶段；已开始的安装要等它完成；post_stop await 工作任务                                                                                                                                        | `Message::Shutdown` 的 abort-all 行为（`mod.rs:380-401`）、`begin_terminate`                                                                                                                                                                                                                            |
| Runtime（旧调度仍在）     | 令牌 = `lifecycle.closing` 的 child；`Request` 与 `BeginMutation` 的入口改读 `closing_token.is_cancelled()`；被跟踪的任务执行 `token.cancelled()`、`cast(Close)`、`cell.wait(None)`                               | `BeginClosing`、`ClosingAck`、`wait_settled`、`Settlement`、`begin_closing`、`begin_terminate`；`stage` watch 若已无读者也删除                                                                                                                                                                          |
| 边界 producer             | 改为用根令牌跟踪                                                                                                                                                                                                  | `ProducerTasks::stop(budget)`                                                                                                                                                                                                                                                                           |

- **从 `app_lifecycle.rs` 删除**（保留 `startup_reconcile` 与 `start_background_sources`）：
  - `ShutdownRequest`、`MainWindowGeometry`、`ShutdownBudgets`；
  - `ShutdownReport`、`ShutdownStepReport`、`ShutdownStep`、`StepOutcome`；
  - `Reply`（连同 `Reply::of`）、`UNACKNOWLEDGED`、`REPLY_MARGIN`、`ShutdownRun`；
  - 测试探针、`joined`、`Deadlines`、`deadline_after`、`Issued`；
  - `Terminating` / `Requested`；
  - `run_shutdown` 与 7 个步骤函数，包括直停 core 的旁路（`:696-703`）、`stop_logs`、`log_report`。
- **随之删除的外部代码：**
  - `NyanpasuClientInner` 的 `shutdown`、`shutdown_budgets`、`shutdown_probe` 字段（`client/mod.rs:186-190,381-384`）；
  - `client/mod.rs:57-60` 的 re-export；
  - 9 个 `begin_terminate`。
- **typed client 的 Drop 保持不变。** 生产中 `NyanpasuClient` 常驻 managed state，不会走到 Drop。
- **测试：**

| 测试                                                                                         | 处理                                                                                                                                                                                                                                                                                                                                                                                    |
| -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app_lifecycle.rs` 测试模块（`:811-1865`）                                                   | 按 L2 盘点 §8.1 逐条处理：删除预算、报告和 Reply 相关的测试；以下改写为 V14、V16、V18、V21：<br>• X1 → 结束后每个 owner 都只收尾一次；<br>• X2 与 X12 → 在途命令先结算，再停 core，没有直停；<br>• X7 → `request_shutdown` 幂等；<br>• X11 → 最终几何已持久化；<br>• `a_stuck_actor…` → 其余 owner 照常结束，`wait_shutdown` 继续等待。<br>X6（语言 patch）随 L3-6 的旁路改写，这里先删 |
| effects `shutdown_signals_before_it_reaps_and_cleans_up_once`（`effects/tests.rs:696-726`）  | 改写：令牌取消后在途的组被 await，不开新组；删除缓存结果那一半                                                                                                                                                                                                                                                                                                                          |
| effects `held_retries_never_run_but_later_commits_still_apply`（`:728-759`）                 | 删除（令牌取消后一律关闭，迟到的提交通知被忽略，评审稿 T04.7）                                                                                                                                                                                                                                                                                                                          |
| effects 用 `client.shutdown(..)` 收尾的测试（`:283-617` 共 5 条）                            | 改为 `request_shutdown` 加 `wait_shutdown`                                                                                                                                                                                                                                                                                                                                              |
| ui_effects `the_shutdown_signal_ends_a_pac_download_before_the_restore_runs`（`:831-927`）   | 改写为 SystemProxy 自身退出路径上的相同日志顺序                                                                                                                                                                                                                                                                                                                                         |
| ui_effects `a_restore_stuck_behind_an_os_call_is_reported_unconfirmed`（`:932-998`）         | 改写：restore 等到卡住的 OS 读取完成后才执行                                                                                                                                                                                                                                                                                                                                            |
| ui_effects `a_hanging_widget_stop_holds_back_no_other_cleanup`（`:1002-1028`）               | 改写为 V18：各 owner 相互独立                                                                                                                                                                                                                                                                                                                                                           |
| ui_effects `a_widget_aborted_mid_handshake_is_reaped_by_the_shutdown`（`:1078-1096`）        | 改写为 V25：由令牌结束握手                                                                                                                                                                                                                                                                                                                                                              |
| ui_effects `a_widget_whose_handshake_cannot_be_released_is_reported_blocked`（`:1100-1120`） | 已由 `widget.rs:696-724` 覆盖，删除                                                                                                                                                                                                                                                                                                                                                     |
| hotkey `exit_unregisters_all`（`:494-511`）                                                  | 改写：取消令牌，drain 后 wait，断言注册器收到了 `unregister_all`                                                                                                                                                                                                                                                                                                                        |
| hotkey `a_release_the_gone_actor_never_saw_is_reported_unreachable`（`:513-536`）            | 删除                                                                                                                                                                                                                                                                                                                                                                                    |
| hotkey `reconcile_after_unregister_all_is_rejected`（`:538-574`）                            | 改写为令牌取消之后拒绝                                                                                                                                                                                                                                                                                                                                                                  |
| system_proxy 中与 restore / cancel 相关的测试（`:476-527,674-707,837-1236`）                 | 触发方式改为退出路径；不变量不变                                                                                                                                                                                                                                                                                                                                                        |
| system_proxy `a_restore_the_gone_actor_never_saw_is_reported_unreachable`（`:913-934`）      | 删除                                                                                                                                                                                                                                                                                                                                                                                    |
| updater `shutdown_*`（`tests.rs:176-200`）                                                   | 改写：令牌取消后下载中止；另加“安装进行中则等待其完成”                                                                                                                                                                                                                                                                                                                                  |
| profiles `stop_aborts_pending_downloads_and_drops_their_late_completions`（`:3765-3839`）    | 签名调整                                                                                                                                                                                                                                                                                                                                                                                |
| profiles `stopped_producers_refuse_new_work_for_good`（`:3843-…`）                           | 签名调整                                                                                                                                                                                                                                                                                                                                                                                |
| workflow `tests/closing.rs`                                                                  | 改为令牌加 `Close` 的语义（在 L3-2 定稿）                                                                                                                                                                                                                                                                                                                                               |
| `client/mod.rs` 构造器中的 `ProducerTasks::default()` / `ShutdownBudgets::default()`         | 改为传入根令牌和跟踪器                                                                                                                                                                                                                                                                                                                                                                  |
| `client/mod.rs:1922`、`:3902` 等收尾处                                                       | 改为 `request_shutdown` 加 `wait_shutdown`                                                                                                                                                                                                                                                                                                                                              |

- **验收**：
  - V14、V16、V18–V21、V25、V26；
  - `rg 'ShutdownBudgets|REPLY_MARGIN|enum Issued|struct Terminating|begin_terminate|HOTKEY_RPC_TIMEOUT|SYSTEM_PROXY_RESTORE_TIMEOUT|GROUP_REAP_BOUND|DOWNLOAD_STOP_BOUND' backend/tauri/src` 命中 0 处。

### L3-1 `refactor(workflow): wait for the decision and the caller without local budgets`

- `tcc.rs:1220-1237`：`await_decision` 直接写成 `request.decision.wait().await`，映射为 Committed / Aborted{Restored} / Aborted{NeedsRecovery}；删除 `Unresolved("reached no decision within the decision budget")`。
- 删除 `MutationBudgets`（`mutation.rs:448-470`），以及它在 `ApplicationWorkflow.budgets`（`workflow.rs:31-34`）、args（`mod.rs:274-275,285`）、组合根（`client/mod.rs:338`）和测试 `tests/mod.rs:329` 中的副本。10 s 的准入预算在 L3-2 随 `pending` 一起删除；本提交先把它固定为该处的私有常量。
- `CALL_WAIT` 与 `call_with_timeout`（`mod.rs:51,1026-1042`）改为 `call(.., None)`。只有发送失败或 `SenderError` 映射为 “workflow unavailable”。
- `startup_reconcile` 返回 `Err` 时，`Unsettled` 只表示 actor 不可用（`mod.rs:1146-1158`）。
- **F2 TODO（U11）**：在 `utils/resolve.rs:171-173` 加 `// TODO(startup): resolve_setup 需要重构，startup_reconcile 不应阻塞 setup；见 docs/plan/2026-09-28-workflow-lifecycle-simplification.md §9。` 同时在 roadmap §12.2 加一行：Owner = 后续 setup PR；移除条件 = `startup_reconcile` 不在 setup 线程上执行，窗口与后台源按它的完成排序。
- 测试：删除 `an_elapsed_decision_wait_keeps_the_attempt_until_the_decision_is_read`（`recovery.rs:914`）；为 V02 增加 `start_paused` 用例：Try 成功后源写入被阻塞超过旧的 120 s，Runtime 仍保持等待，决定到达后正确 Confirm 或 Cancel。

### L3-2 `refactor(workflow): run each command in the actor handler`（G2）

- **状态**：`workflow: Option<Box<ApplicationWorkflow>>` 改为普通字段 `workflow: ApplicationWorkflow`。
- **删除第二层调度：**
  - `active` / `ActiveOperation`、`pending`；
  - `MAX_PENDING` 作为准入的用途（保留为展示历史的上限，改名为 `HISTORY_LEN`）；
  - `recovery_due`、`shutdown_waiters`、`abandoned`；
  - `MutationContext` 及 `context_mut` / `wake_mutation` / `withdraw_mutation` / `retire_mutation`（`mod.rs:212-217,233,564-678`）；
  - `drive()`（`:379-562`）；
  - `Message::{Completed, WakeMutation, AdmissionExpired, LiveMutationContexts, PanicAtConfirm}`；
  - 准入预算与 `send_after` 准入计时器（`:600,630-639,837-847`）；
  - `ApplicationWorkflowClient::wake_mutation`（test）。
- **handler 形态**：入口先检查，再执行命令，最后收尾。
  - 入口检查：`closing_token.is_cancelled()`、`workflow.isolated()`、`req.decision` 是否已落定。
  - 执行：`state.workflow.execute(..).await` 或 `run_mutation(..).await`。
  - 收尾：`publish()`，其中 `status.active` 在开始和结束时各写一次，**先 publish 再回复**；`publish_journal`；回复。
  - 隔离时：只放行显式 `RetryRuntime`；`StartupReconcile` 走 `startup_unsettled` 并返回报告；`Mutation` 回 Rejected；`ReplaceCoreBinary` 调用 `progress.finished(Some(reason))`；tick 跳过。
- **panic（U5）**：删除 5 处 `catch_unwind`（`mod.rs:340-346,409-415,512-537,541-547`）及 import（`:20,22`）；删除 `panic_at_confirm`（`workflow.rs:51-53`、`tcc.rs:1248-1252`、`mod.rs:990-991`）；删除承诺在 panic 后继续存活的文档（`attempt.rs:4-9,50-52`、`workflow.rs:41-43`、`tcc.rs:369-371,942-943`、`startup.rs:264-268`）。
- **计时器**：
  - `RecoveryTick`（5 s）在 handler 内直接执行 `RecoverServiceEndpoint`，前提是未退出、未隔离，且 `lifecycle.recovery_due()`。
  - 250 ms 的 `ConvergenceTick` 轮询改为单个 `send_after(next_attempt)`，在每处写入 `next_attempt` 的位置重新设定（`tcc.rs:292,424,493-500,510,1447-1450`、`workflow.rs:162`、`startup.rs:870,891,934,936`），消息到达时再核对当前目标。
  - 不做优先级：mailbox 的 FIFO 是可接受的行为。
- **关停**：Runtime 改用统一的 `drain_on_shutdown`；`post_stop` 执行核心关停（原 `Close` / `Shutdown` 的主体，`CL/workflow.rs:222`），关停报告缓存在状态里。删除 `Message::Close`，`ClientInner` 的 Drop（`mod.rs:934-938`）改为 `drain()`。显式的 `Shutdown` 命令按 FIFO 执行，结果被缓存。
- **状态 DTO（后端 wire 与前端同一个提交）：**
  - 删除 `CoreLifecycleStatus.queued`、`PublishedView.queued`、`ConfigurationStatus.queued`（`configuration_status.rs:19,110`）；
  - `configuration-status.tsx:209-213` 删除对应渲染，`frontend/interface/tests/configuration-status.test.ts:31` 删除 fixture 字段；
  - 用 `export_typescript_bindings` 重新生成 `bindings.ts:844`。
- **测试**（L3 盘点 §7.2）：

| 组  | 测试                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 处理                                                                                                                                                                                                                                                    |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G1  | `queue_is_bounded…`（`tests/mod.rs:991`）、`an_occupied_domain_refuses…admission_budget`（`mutations.rs:783`）、`a_late_cancel_for_a_settled_attempt…`（`:928`）、`an_isolated_domain_retires_the_contexts…`（`:3390`）、`a_panicking_mutation_retires_its_context`（`:3487`）、`full_queue_rejects…`（`connection_policy.rs:762`）                                                                                                                                                                                                                             | 删除                                                                                                                                                                                                                                                    |
| G1  | `replacement_serializes_reconcile_and_retains_files_after_caller_cancellation`（`mod.rs:829`）、`uninstall_waits_for_the_complete_host_switch…`（`:597`）、`shutdown_rejects_pending_work_and_waits_for_the_active_installation`（`:965`）、`cancelled_profile_waiter_keeps_admission…`（`connection_policy.rs:665`）、`profile_interruption_serializes_mode_host_and_binary_operations`（`:826`）、`lifecycle_work_cannot_overtake_pending_interruption` / `profile_mutations_cannot_overtake…` / `shutdown_rejects_a_queued_profile_apply…`（`:324,543,720`） | **移植**：去掉 `queued` 断言，以及在 busy 期间调用 `barrier` 的写法（现在会阻塞）。改用事件或 `Notify` 同步点，保留原有的顺序断言                                                                                                                       |
| G1  | **`queued_installation_timeout_is_settled_when_shutdown_or_uncertainty_rejects_it`**（`mod.rs:1528-1576`）                                                                                                                                                                                                                                                                                                                                                                                                                                                      | **必须保留（V27）**：执行前被拒的 `ReplaceCoreBinary`，在“退出中”和“已隔离”两种情况下都恰好调用一次 `progress.finished`。把 `call_with_timeout` 换成 spawn 出来的 call，把其中的 panic 变体换成“丢失回执导致隔离”（`set_result_missing`）               |
| G1  | **`a_second_domain_is_admitted_only_after_the_first_commits_and_reads_it`**（`mutations.rs:850`）                                                                                                                                                                                                                                                                                                                                                                                                                                                               | **必须保留（V04）**：`wait_queued` 换成第二个 participant `on_prepare` 入口处的同步点                                                                                                                                                                   |
| G3  | L1（`recovery.rs:232`）、L4（`:276`）、L2（`:310`），`mod.rs:1046` 的 panic 那一半                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | 删除                                                                                                                                                                                                                                                    |
| G3  | L3（`:567`）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | 移植到 `WaitScript::Missing`                                                                                                                                                                                                                            |
| G3  | L5（`:636`）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | 移植到 `WaitScript::Running`                                                                                                                                                                                                                            |
| G3  | L7（`:740`）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | 移植到 `Missing`；若已被 L6 覆盖则删除                                                                                                                                                                                                                  |
| G3  | `startup.rs:1077`、`:1183`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | 改为用“丢失回执”造成中断，保留“只发布一次完整视图”的不变量                                                                                                                                                                                              |
| G3  | `recovery.rs:388,464,1018,1064`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 只借 `panic_at_confirm` 制造“已提交、Confirm 未完成”：没有其他生产路径的随之删除；`:1237` 移植到丢失 `stop_service` 应答的情况。删除 panic 夹具（`builder.panic`、`panic_capture`、`notifications.panic_next`、`WaitScript::Panic`、`Installer.panic`） |
| G6  | `service_recovery.rs`（17 条）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 语义不变；`RecoveryTick` cast 之后的 `barrier` 会等到内联恢复完成。`shutdown_during_restart_prevents_readoption_and_reconcile`（`:607`）依赖带外令牌，按令牌移植                                                                                        |
| G7  | `closing.rs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `closing_admission_refuses_new_work…`（`:38`）保留；X2–X4（`:65,117,188`）改为：令牌取消后，当前 Try / 等待决定 / Cancel 执行到底，下一条命令被拒；`an_abandoned_workflow_still_stops_the_core`（`:251`）改为 drop 触发 drain，由 post_stop 停 core     |

### L3-3 `refactor(workflow): hand the mutation settlement back to the source`（G1、U7）

- **结果通道**：`MutationRequest` 携带 `settle: oneshot::Sender<MutationReceipt>`。
  - participant 用一个只能取一次的单元（`Mutex<Option<Sender>>`）持有它，因为 `on_prepare(&self)` 拿不到所有权。
  - `MutationCoordinator::participant(..)` 返回 `(participant 工厂, settlement_rx)`。
  - `MutationCommand` 包装（`mutation.rs:144-147`）与 `Output::Settled`（`CL/mod.rs:51-54`）删除。
- **源 actor**（`state/application.rs:107-135`、`clash_config.rs:95-120`、`profiles/actor.rs:455-496,1120-1225`）：
  - `replace_..` 返回后，**总是**等待 `settlement_rx`，不再用 `?` 提前返回；
  - 显式映射 `Conflict`，修正 app / clash 忽略它的问题；
  - 组合出 §3.3 表中的错误（U7）。错误文本本身写明持久化原因和回滚结果，不只依赖 `{:#?}` 的 Debug 输出。
- **Try 结果未知时**（`tcc.rs:241`），handler 不再提前返回，而是先结算，发出 RecoveryRequired 回执，再保持隔离。
- **`Aborted{NeedsRecovery}`**（Q-D 默认）：与 Restored 一样执行 Cancel，把运行态回滚到 store 中已提交的版本。错误同时报告资源恢复失败的原因。取消“不 Cancel、永久隔离”这一特例（`attempt.rs:506-509`、`tests/mutations.rs:1466-1473`、`recovery.rs:1151`）。
- **删除：**
  - `wait_mutation`（`mod.rs:1120-1142`）；
  - `MutationCoordinator::finish` 中的 `Pending` / `operation_pending` 分支（`state/mutation.rs:44-119` 中的 `:81`、`:104-108`）；
  - 不可达的 `Rejected → RecoveryRequired`（`:77-79`）；
  - `RuntimeCommitStatus::Pending`（`client/runtime.rs:541`），并重新生成 `bindings.ts:2514`；前端没有读取它的代码；
  - `client/mod.rs:922-946` 里 `CoreUpdateInstaller` 专为 `CALL_WAIT` 超时准备的 `InstallPending` 映射。它被改成只剩“actor 不可用”这一种；L1b 疑似更新器任务可能永久 Pending，这一点在 §11 登记。
- **journal 只用于展示**：丢失、截断或没人订阅，都不影响调用方拿到结果（V10）。
- **测试：**
  - `config_persistence_failure_restores_runtime_and_keeps_source_unchanged`（`tests/mod.rs:1262`）与 `mutations.rs:1220`：从等待 journal 改为断言**返回的错误**里既有持久化原因，也有“已回滚”；新增恢复失败的变体（V08、V09）；
  - `settled()` 辅助函数改为读取调用返回的结果；
  - 新增 V05、V06、V11。

### L3-4 `refactor(effects): notify effects per domain from each owner`（U9）

- **`CommitNotifications`**（`effects/ports.rs:136-148`）改为按域的方法：
  - `application_committed(ApplicationEffectFields, requested: Vec<EffectKind>)`：Application actor 在提交后发送（运行态相关的修改则在结算后发送）；
  - `clash_committed(ClashEffectFields)`：ClashConfig actor 发送；
  - `runtime_bound(Option<ResolvedPortBindings>, refresh: bool)`：Runtime 在 Confirm / Cancel、lifecycle 命令和启动结束时发送；
  - `profiles_committed()`：Profiles actor 发送，只请求托盘的部分刷新；
  - `publish_full(Option<ResolvedPortBindings>)`：启动结束时由 Runtime 发送一次。effects 用自己已有的 app / clash 切片加上 ports 做完整下发。
- **effects actor**：`desired` 按域替换对应切片后再做 diff（`actor.rs:160-200`）。每个切片只有一个串行发送方，所以不需要版本号。
- **删除 workflow 中的跨域投影**：`notify_requested`、`notify_committed`、`publish_full` 里的 `ApplicationEffectInputs::project`（`workflow.rs:69-98`）；`project` 改为只在组合根计算初始值时使用。
- **接线**：源 actor 在 `EffectsClient` 之前 spawn，因此复用现有的后接点 `MutationCoordinator::connect`（`client/mod.rs:343-345`），把 effects sink 与 workflow 一起接入。不新增 registry。
- **已知的行为差异**：一次同时修改 core 和系统代理的 app patch，会先收到 ports，再收到 app 切片，可能各应用一次。effects 对同一 kind 只保留最新目标，而且每组只有一个在途任务，最终状态一致。
- **测试**：effects 测试改用按域 API；新增 V29（app 切片与 ports 切片由不同 owner 交错到达，最终 `desired` 同时是两者的最新值）。

### L3-5 `refactor(state): accept local write hooks without a participant`

nyanpasu-core 的最小改动：让 `replace_if_version_with_participant` 的 participant 可选，或新增一个只接收本地钩子的入口，两者选一。实施时选改动更小的做法。它的唯一消费者是 L3-6 中 profiles 的旁路保存，所以放在消费者之前的相邻提交，不提前放进 PR-1（不写投机代码）。

### L3-6 `refactor(workflow): classify at the source and keep plain saves off the runtime`（G1）

- **单一纯函数** `impact::runtime_impact(previous, candidate, hints, class) -> Option<RuntimeImpact>`，合并三处逻辑：`tcc.rs:144-152` 的 `needs_runtime`、`policy.rs:83` 的 `critical`（D1），以及 `tcc.rs:1682-1712`。
  - 请求意图与最终 diff 都参与判断：`ExplicitSwitch` 或“点名了运行态字段”即使 diff 为空也算相关。这保留 R15 与重复切换模式时的连接中断语义。
  - 混合 patch 作为一个事务，整体分类。
- **源 actor 分支**：先执行 `ensure_ready`（所有写入同一道门）。运行态相关时安装 participant；否则不安装 participant，也不分配 OperationId。profiles 使用 L3-5 的入口。
- **Runtime 从请求里读取 `impact`，不再重新分类。**
- **删除：**
  - 伪 baseline（`tcc.rs:168-179`）；
  - `needs_runtime` 分支（`:154-158,164-179`）；
  - `CommandPolicy::{SaveOnly, SaveThenNotify}`（`policy.rs:25-29,86-94`）；
  - `ChangedOwnerInputs` 在 `policy_for` 中的使用和 `changed_owner_inputs`（`tcc.rs:180,713-728`）；
  - `RuntimePrepareOutcome::Saved`、`TryVerdict::Saved`、`MutationOutcomeKind::Saved`；
  - `try_critical` 中的 Saved 分支（`:741-746`）；
  - 恢复分支 `(Some(TryVerdict::Saved), _)`（`attempt.rs:500`）。
- **重复逻辑：**
  - D3（`selection_changed`）与 D4（`leaves_service_mode`）改为调用 `impact`。
  - D2（profiles 的 `AffectsRule` / `evaluate_affects` 与 `classify_profiles`）：**先写对照测试，锁定两处当前的行为，再合并成同一个纯函数**。两处的 `Touched` 语义不同：actor 侧是“闭包变化或 uid 在闭包内”，impact 侧是“定义不同或 touched 文件在闭包的文件内”。如果不能等价合并，停下来报告，不自行裁剪行为。
- **测试：**
  - `a_gui_save_does_not_query_core_status`（`mutations.rs:3553`）改为 V12：一个被调用就立刻返回错误的 Runtime fake，纯 UI patch 从未触达它；
  - 新增分类矩阵 V13：相关 / 无关 / 混合 / 同值重选（core、host、profile）/ 内核已停止 / 内容在闭包内或闭包外；
  - 删除 `policy.rs:232,350,471`；
  - `domain_actor_refuses_writes_before_composition_is_ready`（`:3940`）保持不变。

### PR-2 验收

- V02–V30 中归属 PR-2 的各项全部通过（§8）。
- `rg 'MUTATION_ACK_TIMEOUT|AdmissionExpired|decision_wait|wait_mutation|CALL_WAIT|MutationContext|fn drive|shutdown_waiters|catch_unwind' backend/tauri/src backend/nyanpasu-core/src` 在生产代码中命中 0 处。
- bindings 零漂移。
- 前端 typecheck 通过。

---

## 7. PR-3：收口（L4）

### L4-1 `refactor(workflow): acknowledge with Ack instead of a mirror type`

- 删除 `TryAck`（`mutation.rs:149-171`）。
  - 通道改为 `oneshot::Sender<Ack>`；
  - `RuntimePrepareOutcome::ack()` 直接返回 `Ack`；
  - 测试（`tests/mutations.rs:2088-2122`）改用 `matches!`。
- `participant.rs:124-133`：cast 失败表示请求根本没有送到，改为 `Ack::Rejected("…未提交")`，不再是 `Failed`。依据是评审稿 §3.4：发送失败与回复丢失要区分。

### L4-2 `refactor(workflow): keep only recovery state with a real cause`

- **`LiveAttempt.verdict`** 缩为 `Option<AppliedVerdict>`，删除 `TryVerdict` 及 `TryVerdict::of`。
  - 仍然保留它，是因为 Confirm 可能留下一个挂起的 `release_service`，Applied 这一分支依然可达。
  - `recover_source_decision` 中 Committed×{Deferred, SavedInactive, Saved} 以及“disagree”这些分支（`attempt.rs:469-504`）：**先确认已没有生产代码能进入它们，再删除**（中等置信度）。
  - `tcc.rs:462` 与 `startup.rs:797` 两处只写不读，删除。
- **删除 `LifecycleCommand` 及其 `of`。** `AttemptOrigin::Lifecycle` 改为 unit variant，`workflow.rs:145` 改为 `!matches!(cmd, Shutdown)`。
- **删除 `ActionView` 及其 `of`。** `RecoveryView` 只保留 `{operation_id, reason}`：`configuration_status.rs:67-74` 只读这两个字段。
- **删除 `CoreLifecycleStatus.completed` 与 `CoreLifecycleOperationResult`。** `configuration_status.rs:75-76` 在 uncertain 时改用 recovery view 的 id。roadmap §12.2 中对应的 residual 行随之关闭。
- **删除只写不读的字段**：`DeferredTarget.baseline`、`Deferred.baseline`、`TargetOrigin::Mutation{domain}`；`MutationReceipt.{impact, policy, refusal}` 若没有生产读者也删除。
- **删除过期的 allow**：`#![allow(dead_code)]`（`mutation.rs:14-17`、`participant.rs:12-15`）与 `#[allow(dead_code)]`（`mod.rs:927,1112`）。

### L4-3 `refactor: replace of() constructors with From and named functions`（U1）

| 现有写法                                                                 | 目标                                                                                                           |
| ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `ApplicationRuntimeInputs::of`（`impact.rs:273`）                        | `application_runtime_inputs(app)`（有损投影，不用 `From`）                                                     |
| `ClashRuntimeInputs::of`（`impact.rs:327`）                              | `clash_runtime_inputs(clash)`                                                                                  |
| `PortsFingerprint::of`（`client/ports.rs:32`）                           | `ports_fingerprint(clash)`                                                                                     |
| `AppliedRevisions::of`（`system_proxy/actor.rs:112`）                    | `revision(kind)`（这是 getter，不是构造）                                                                      |
| `MutationDomain::domain_change`（`mutation.rs:74-114`）                  | 3 个 `impl From<StateChange<X>> for DomainChange`，删除该 trait；约束写为 `DomainChange: From<StateChange<T>>` |
| `impl From<&Reestablished> for StartupOutcome`（`startup.rs:213`，有损） | 具名方法 `Reestablished::outcome(&self)`                                                                       |

`Reply::of`、`TryVerdict::of`、`LifecycleCommand::of`、`ActionView::of` 已在前面的提交中随各自类型删除。

### L4-4 `docs: mark the superseded lifecycle and timeout statements`

- 按 §14 的清单加“已被取代”横幅。只标注，不改写历史记录。
- roadmap：
  - §12.1 中 T10 的证据行改指本计划；
  - §12.2 中 IPC 与 resolve_setup 两行已在 P1-3、L3-1 添加，这里核对；
  - 关闭 `CoreLifecycleOperationResult` 那一行。
- 更新本计划顶部的状态行。
- ledger：确认没有计数变化；不增加针对 timeout / OperationId 的关键词门禁。

---

## 8. 行为测试矩阵

“失效条件”一列指：删掉哪一行生产代码，这条测试就会变红。没有失效条件的测试可能只是在测桩，不计入。

| #   | 场景                                                     | 期望                                                        | 提交                          | 失效条件                                        |
| --- | -------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------- | ----------------------------------------------- |
| V01 | Required prepare 被阻塞，虚拟时间越过旧的 90 s           | 没有人工回滚；释放后正常提交                                | P1-3                          | 重新包上 `tokio::time::timeout`                 |
| V02 | Try 已成功，源写入越过旧的 120 s                         | Runtime 一直等待，决定到达后才 Confirm 或 Cancel            | L3-1                          | 恢复 `decision_wait`                            |
| V03 | 调用方的 future 被丢弃                                   | 已开始的操作执行到终态，不因回复无人接收而回滚              | P1-7、L3-2                    | 恢复 caller-cancel 的 `select!`                 |
| V04 | 两个不同源域同时提交关键修改                             | 第二项的 Try 不越过第一项的完整结算，读到第一项已提交的快照 | L3-2                          | 删除 handler 的 await，改为 spawn               |
| V05 | 校验失败或版本冲突，participant 未执行                   | 立即返回真实错误，不等待不存在的结算                        | L3-3                          | 源侧改为无条件等待 journal                      |
| V06 | 请求未送达，或 Runtime 在准入前已停止                    | 明确的 Rejected“未提交”；内核未改；候选已清理               | L3-3、L4-1                    | cast 失败映射回 `Failed`                        |
| V07 | Try 被拒，源 abort                                       | 源和运行态都保持原样；错误中带拒绝原因                      | L3-3                          | 错误文本丢失原因                                |
| V08 | Try 已应用，源持久化失败                                 | 执行 Cancel 恢复；错误 = 持久化原因 + “已回滚”              | L3-3                          | 源侧恢复 `?` 提前返回                           |
| V09 | 持久化失败，且恢复也失败                                 | 错误同时包含两个原因；Runtime 隔离                          | L3-3                          | 恢复失败被吞掉                                  |
| V10 | journal 没人订阅，或历史被截断                           | 本次结果照常返回                                            | L3-3                          | 结果改回经 journal 查找                         |
| V11 | Runtime 在 Try Ok 之后消失                               | 源已提交，附 `runtime_recovery_required` 降级               | L3-3                          | 把 `RecvError` 映射为 Ok 且不带降级             |
| V12 | Runtime 隔离期间保存纯 UI 设置                           | 保存成功，Runtime fake 从未被调用                           | L3-6                          | 源侧重新安装 participant                        |
| V13 | 同值重选内核 / host / profile；混合 patch；StoppedByUser | 关键承诺、原子性不变；不擅自启动内核                        | L3-6                          | `runtime_impact` 忽略请求意图                   |
| V14 | 多个入口重复或并发请求关停                               | 同一个信号，每个 owner 只收尾一次                           | L2-2                          | owner 收尾不幂等                                |
| V15 | 关停发生在源已开始、Try 尚未送达时                       | 被拒，不死锁                                                | L2-2、L3-2                    | 入口不检查令牌                                  |
| V16 | 关停发生在 Try / 等待决定 / Confirm / Cancel 之中        | handler 执行到底；core 在其后才停                           | L2-2、L3-2                    | post_stop 早于 handler 结束执行，或恢复直停旁路 |
| V17 | Required participant 无法服务                            | 返回 `Failed` 并否决提交（钩子已删）                        | P1-4                          | — （由现有否决测试覆盖）                        |
| V18 | 三个 owner 的清理都被阻塞                                | 三者并发开始；`wait_shutdown` 等全部真正结束                | L2-2                          | 在同一个任务里串行清理                          |
| V19 | 假的 OS 写入比旧的 5 s 上限更慢                          | 没有人工失败；写完后才由本 owner 清理                       | L2-2                          | 恢复 RPC 超时                                   |
| V20 | 关停后才到达的 effect / timer / 下载完成                 | 不重新注册热键、不重装代理、不开始新的持久化                | L2-2                          | 入口不检查令牌                                  |
| V21 | 保存窗口几何与退出并发                                   | 最终保存在 SessionState drain 之前入队并写完                | L2-1、L2-2                    | 在 cast 之前先取消令牌                          |
| V22 | Tauri 主线程在清理期间调度热键注销                       | 事件循环持续运行，不会 `block_on` 自锁                      | L2-1                          | **只能 smoke**                                  |
| V23 | IPC 执行成功但回复丢失，daemon 重连或重启                | 保留实际核验，不盲目重试，不启动第二个内核                  | 现有（移植后的 L3 / L5 / L7） | —                                               |
| V24 | 旧下载迟到；profile 已被编辑或删除                       | fencing 仍在，旧结果不覆盖新状态                            | 现有                          | —                                               |
| V25 | widget 握手期间取消令牌                                  | 握手结束，子进程被收尾；进程边界期限仍然保留                | L2-2                          | 握手的 `select!` 没有令牌分支                   |
| V26 | owner 的动态子任务与 tracker 的 close 交错               | 父 owner 在子任务结束前一直被跟踪                           | L2-2                          | 在 close 之后才 spawn 被跟踪的任务              |
| V27 | 执行前被拒的 `ReplaceCoreBinary`（退出中 / 已隔离）      | `progress.finished` 恰好调用一次                            | L3-2                          | 拒绝路径不调用 `finished`                       |
| V28 | `MainThreadExecutor::run`                                | 返回任务结果；任务被丢弃时返回错误                          | P1-9                          | —                                               |
| V29 | app 切片与 ports 切片由不同 owner 交错到达               | 最终 `desired` 同时是两个域的最新值                         | L3-4                          | 切片改为整体替换                                |
| V30 | `ExitGate` 状态表                                        | 按 §3.4 放行或阻止；RESTART 不可阻止                        | L2-1                          | —                                               |

- 已删除机制的测试，按表中处理，不以测试数量作为指标。
- 真实 IO 失败、取消安全、IPC 不确定性这几类测试不得删除。
- 测试自己的看门狗或 CI 超时只用来判定测试失败，不能挪进生产代码当作事务期限。

---

## 9. F2：启动调整建议（U11；写入 docs 与 PR-2 正文）

**现状（`utils/resolve.rs:150-233`）**：以下步骤依次阻塞，窗口要等内核启动和 service IPC 结束后才出现。

1. `block_on(startup_reconcile())`（`:173`）；
2. `start_background_sources`；
3. `block_on(start_clash_streams())`（`:205`）；
4. `create_window`（`:214`）。

组装阶段 `setup.rs` 中的 `block_on`：

| 位置                    | 内容                                                                                                     | 是否涉及外部 IO |
| ----------------------- | -------------------------------------------------------------------------------------------------------- | --------------- |
| `:79-97`                | core 与 service actor；ServiceActor 的 `pre_start` 会探测 service IPC，daemon 不兼容时还可能执行提权更新 | **是**          |
| `:209`、`:216`          | system proxy / hotkey 的 spawn                                                                           | 否              |
| `client/mod.rs:218,256` | 构造 client                                                                                              | 只有文件系统    |
| `:147`                  | widget                                                                                                   | 否              |

**建议**（未来目标：内核可以带 unhealthy 状态启动，GUI 尽快显示，setup 不受 `NyanpasuClient` 初始化影响）：

1. **窗口先行。** `create_window` 只依赖本地配置快照（`enable_silent_start`），应排在所有依赖内核的步骤之前。注意：webview 可能调用的管理状态必须在窗口创建之前 `manage`。今天 `PendingDeepLinks` 是在 `resolve_setup` 创建窗口之后才 manage（`lib.rs:286,291`）。
2. **`startup_reconcile` 只投递，不等待。** L3-2 之后，它就是 Runtime mailbox 的第一条消息，后续命令按 FIFO 天然排在它后面。`start_background_sources` 可以立即启动，`:193-194` 的注释已经说明它们的变更会排在启动命令之后。结果经现有的状态 watch 发布，完成时写日志。
3. **内核初始状态为 Starting / unhealthy。**
   - 前端今天没有监听 `CoreStatusChangedEvent` / `ServiceStatusChangedEvent`；
   - `use-core-status.ts` 只在挂载或聚焦时 refetch；
   - `CoreStatusBadge` 把 Starting、Restarting、Switching 都显示为“已停止”（`widget-shortcut.tsx:147-187`）。

   需要补上事件订阅（或失效刷新）和 Starting 分支。托盘、热键、系统代理今天要等 reconcile 结束后的 `publish_full` 才拿到完整值（`startup.rs:258-260`）；改为异步后，这些值会在 reconcile 完成时才出现。

4. **clash streams 同样异步启动。** 它依赖内核就绪，应随内核状态连接和重连。需要先核实 streams 自己是否已有重连逻辑（`core/clash/ws.rs:559-563` 有 1–30 s 的退避）。
5. **组装原则**：`spawn` / `pre_start` 只构建本地对象，不做外部 IO。ServiceActor 的探测与 daemon 更新（`setup.rs:79-97`）移到第一条消息或后台任务中执行。凡是可以异步等待的资源，都优先放到异步处理。

效果：`CALL_WAIT` 删除后（L3-1），启动过程中不再有会被外部边界卡死的等待，也不需要为启动保留任何 timeout。

---

## 10. 保留清单

| 保留项                                                                                                                                                                                                                                        | 理由                                                                                                                      |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `core/actor_v2` 中 CoreClient 的调用期限（`mod.rs:476,522,545,608,706,730,791,857,1044`）、`facade.rs` 的 `OPERATION_WAIT` 60 s 与 `:263-267` 的 5 s、`CHECK_BUDGET` 45 s（`application_workflow/adapters.rs:129`）、ServiceClient 的命令期限 | 属于内核管理层与 IPC 之间的边界。由 PR-0 统一收口：上游设置请求超时后，删除这些外层包装                                   |
| 网络：`PAC_DOWNLOAD_TIMEOUT`（`system_proxy/adapters.rs:160`）、updater 的 reqwest 120 s（`updater/instance.rs:66`）、websocket 重连退避                                                                                                      | 网络 IO（U2）                                                                                                             |
| widget 的全部进程边界期限（`WIDGET_STOP_BOUND`、`STOP_GRACE`、IPC 发送 / 释放 / kill）                                                                                                                                                        | 子进程与 IPC 边界；而且它们本身就是期限，不是兜底                                                                         |
| effects 的 3 个独立组、按 kind 合并、过期结果屏蔽、`RetryBudget` 重试调度                                                                                                                                                                     | 真实的独立并发，有测试证明（`effects/tests.rs:124-257`，`ui_effects/tests.rs:683-770`）。没有重复 mailbox 的串行化（Q-E） |
| 周期调度：`RECOVERY_INTERVAL` 5 s、`RETRY_DELAYS`、proxies 刷新、updater 清理、日志 lease                                                                                                                                                     | 调度而非期限                                                                                                              |
| `DecisionHandle`、writer permit（或 `&mut self`）、原子写、`local_recovery`（profiles 的 compensate）                                                                                                                                         | 事务的正确性                                                                                                              |
| 配置 `Version` / CAS、`ProfileId`、下载代次、内核实例 generation、payload digest、`RuntimeRevision`、confirmed receipt、停止意图、未解决的外部操作（`PendingAction`）                                                                         | 真实的身份与状态（评审稿 T07.7）                                                                                          |
| `RollbackGuard` 在 drop 时发布决定                                                                                                                                                                                                            | panic 展开时，Runtime 不会永久停在 `Undecided`                                                                            |
| `MutationCoordinator`（Pending → Ready 的后接点）                                                                                                                                                                                             | 启动接线，不是调度器                                                                                                      |
| panic hook 的 PostMessage 例外（`lib.rs:92-97`）                                                                                                                                                                                              | 上游 tauri#10546 的 workaround（Q-C）                                                                                     |

---

## 11. 残留与独立问题（不并入本计划）

| 项                                                                                                                       | 性质                                        | 建议去向                                                                             |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------- | ------------------------------------------------------------------------------------ |
| **PR-0**：`nyanpasu_ipc::Client` 没有请求超时（`client/mod.rs:116-131`，上游 origin/main 也一样）                        | 下层缺陷（U3）                              | 上游 nyanpasu-runtime PR；本仓库在 `setup.rs:74-75` 加 TODO，并在 roadmap §12.2 登记 |
| `resolve_setup` 重构（§9）                                                                                               | U11                                         | 后续 setup PR；`resolve.rs:171-173` 的 TODO 与 roadmap 行                            |
| 子模块 `nyanpasu-logging` 的 5 s call / stop 期限，以及 JoinError→Err 转换（`session.rs:44-63,92-98`）                   | 同进程期限，但在子模块里                    | 上游 PR                                                                              |
| 用户 JS / Lua 脚本没有执行上限（`application_workflow/adapters.rs:53-89`，`enhance/`）                                   | 早已存在；死循环会让该域保存永久挂起        | 独立 issue（解释器边界）                                                             |
| macOS 的 Quit / Cmd+Q / 注销会直接进入 `RunEvent::Exit`，没有 `ExitRequested`（`lib.rs:267-283,323-350`）                | 疑似今天就没有优雅关停，需要 macOS 复现     | 独立 issue                                                                           |
| 日志的 `WorkerGuard` 从未 drop / flush（`utils/init/logging.rs:93-97`）                                                  | 早已存在                                    | 独立 issue                                                                           |
| 内部 HTTP server 从未停止；选端口与绑定之间有竞争窗口（`lib.rs:308-316`，`setup.rs:153-155`）                            | 早已存在，疑似                              | 独立 issue                                                                           |
| 前端没有监听 core / service 状态事件，Starting 被显示为“已停止”                                                          | F2 的相关项                                 | 随 §9 的后续 PR                                                                      |
| `ShutdownState::CleaningUp` 死分支（`shutdown_hook.rs:32,94-104`）                                                       | 早已存在的死代码，按 AGENTS §3 只提及不删除 | 后续 Windows PR（roadmap 已有 hook static 一行）                                     |
| 零散的 `run_on_main_thread` 调用（`core/tray/mod.rs:221`、`ipc.rs:1305-1362`、`resolve.rs:160-162,237-254`）             | 可以改用 `MainThreadExecutor`               | 后续                                                                                 |
| `CoreUpdateInstaller` 只剩“actor 不可用”一种 pending 映射后，更新器任务可能永久 Pending（`core/updater/mod.rs:352-368`） | 疑似，需要复现（U10）                       | 实施 L3-3 时核对；若成立，报告后再定                                                 |
| 今天每次退出可能卡约 5 s 并报告热键 Incomplete                                                                           | 疑似；L2-1 / L2-2 之后应当消失              | smoke 确认                                                                           |
| Linux 的 SIGTERM 没有处理（`Cargo.toml` 中的 `ctrlc` 未使用）                                                            | 早已存在                                    | 独立 issue                                                                           |

---

## 12. 待用户确认

以下是默认值，确认后执行；如有不同意见，逐项回复即可。

| #   | 事项                                                                                                                                             | 默认                |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------- |
| Q-A | P1-7：事务在源 actor 的 handler 内执行，删除 `persistence_owner`、调用方取消分支和冗余的锁；保留 drop 时发布决定                                 | 执行                |
| Q-B | P1-8：删除 CAS 失配后的配置回写。生产不可达，而且属于磁盘回滚                                                                                    | 执行                |
| Q-C | panic hook 仍然 `exit(1)`，走优雅退出边界，以便系统代理能恢复；PostMessage 例外保留                                                              | 保持                |
| Q-D | `Aborted{NeedsRecovery}`（profiles 资源补偿失败）同样 Cancel 回滚，并报告两个错误；取消“永久隔离”特例                                            | 执行（统一适用 U7） |
| Q-E | effects 的分组、合并、重试调度保留，只删除超时、abort 和关停协议                                                                                 | 保留                |
| Q-F | 重启沿用 `launch` 重启器，但放到所有 owner 结束之后再 spawn；不换成 tauri 的 `restart()`（后者带 macOS Info.plist 查找，但单例锁上的行为未验证） | 沿用                |
| Q-G | 退出时只跟踪有真实清理工作的 owner（§3.2 表）；Streams / Proxies / Logs / server 交给进程退出                                                    | 执行                |
| Q-H | L4-3 的 `of()` 全部改掉，包括 workflow 之外的 `ports.rs` 与 `system_proxy`                                                                       | 执行                |
| Q-I | P1-1 把未入库的评审稿作为输入一并入库                                                                                                            | 入库                |

---

## 13. 已知限制（按 U2 / U3 接受）

- **PR-0 落地之前**，外部边界卡住（daemon 接受连接但不应答、提权提示无人响应）会造成：
  - 该域的保存永久挂起；
  - 启动永久挂起（在 §9 的重构之前，`resolve.rs:173` 仍会 `block_on`）；
  - 退出无法完成；Windows 注销会被一直挡住，直到操作系统强制结束。
- **Runtime handler 自身 panic 时**：actor 停止，不执行 `post_stop`，因此 core 不会由我们停止。panic hook 仍会请求退出。Runtime 被跟踪的任务会在 actor 停止后结束，所以 `wait_shutdown` 不会挂住。
- **系统强制结束**（kill、断电）与优雅关停是两回事，不在本计划的保证范围内。

---

## 14. 文档取代清单（L4-4）

只标注“已被取代”，不改写历史内容。

| 文档:行                                                                                                                     | 被取代的内容                                                                                                        | 取代它的内容                                                |
| --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `AGENTS.md:135,238`                                                                                                         | 跨 actor RPC 优先使用有限超时                                                                                       | P1-2 直接替换正文（这是现行规则，不是历史）                 |
| `AGENTS.md` §10 第 4–5 条                                                                                                   | 先提交，再触发副作用                                                                                                | 仍然有效；P1-2 补充说明 Required participant 是提交前的投票 |
| `docs/design/actor-migration-roadmap.md:125,351,542-543,564,566`                                                            | T10 的关停、RQ-04 的调用侧有限超时、`begin_terminate`、`CoreLifecycleOperationResult` residual、日志关停报告        | L2-2 / L3-1 / L4-2                                          |
| roadmap `:68`                                                                                                               | 事务占用保持到权威决定和必要结算完成                                                                                | 仍然有效，现在由单个 handler await 实现                     |
| `docs/plan/2026-09-14-application-workflow-selective-tcc-v2.md:3,337,342,368-372,374-398,789-824,832,938,970-975,1020,1025` | 按 OperationId 定位事务、有界准入、§5.5 预算、图 13 的调用方超时分支、§11.3 的有序退出、图 12 的“超时 / panic”、V24 | 本计划：V19 由单一 handler 从结构上满足；V24 作废           |
| `docs/plan/2026-09-25-tcc-t10-implementation.md:11,50-54,60,74,76-81,101`                                                   | 单飞有序关闭、R21 的 panic、lifecycle 命令 panic、关停行为差异、会挂住的测试                                        | L2-2、U5                                                    |
| `docs/superpowers/specs/2026-09-25-tcc-t10-lifecycle/design.md:36,226,271,308,311,460-569,795`                              | 有序关闭、`catch_unwind` 分支、调用方预算、决定等待超时、panic 标志、§5 期限、R42                                   | L2-2、L3-1、L3-2                                            |
| `docs/plan/2026-09-25-tcc-t11-implementation.md:49-50,64`                                                                   | 保留的 fencing：准入 FIFO、context、准入预算；等待超时的错误文字                                                    | L3-1 / L3-2（隔离门保留）                                   |
| `docs/superpowers/specs/2026-09-12-pr6-application-effects/design.md:599`                                                   | `SYSTEM_PROXY_RPC_TIMEOUT` / restore 5 s                                                                            | P1-12 / L2-2                                                |
| `docs/plan/2026-09-13-runtime-apply-options.md:65-216`                                                                      | 有界队列、tracked task、panic / uncertain 测试                                                                      | L3-2                                                        |
| `docs/roadmap/2026-09-09-tracked-config-and-snapshot-store.md:35,59,149,202`                                                | 沿用 CoreClient 的有限超时；消费者阻塞或 panic 不影响核心事务                                                       | 分别重新评估：IPC 期限仍然有效；panic 隔离已被 U5 取代      |

---

## 15. 验证命令与环境

从仓库根目录执行。每条命令只在该 PR 实际执行过时才记录结果；没有执行的明确标记为“未执行”。

```bash
# 新 worktree 的前置条件（AGENTS §17）
pnpm -F interface build
cargo build --manifest-path backend/Cargo.toml -p fake-core

cargo build --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib     # Windows 上代替 cargo check（boa_engine ICE）
cargo test  --manifest-path backend/Cargo.toml --all-features -p nyanpasu-core
cargo test  --manifest-path backend/Cargo.toml --all-features -p clash-nyanpasu --lib -- --test-threads=8
cargo test  --manifest-path backend/Cargo.toml --all-features -p clash-nyanpasu export_typescript_bindings
git diff --exit-code -- frontend/interface/src/ipc/bindings.ts
cargo test  --manifest-path backend/Cargo.toml --workspace --all-features -- --test-threads=8
pnpm lint:clippy && pnpm lint:rustfmt
pnpm typecheck && pnpm test:frontend
pnpm lint:architecture-ledger && pnpm test:architecture-ledger
cargo check --manifest-path backend/Cargo.toml --target x86_64-unknown-linux-gnu -p clash-nyanpasu   # 补 cfg(unix)
```

**环境注意：**

- **Windows 上的 bindings 导出**：导出测试需要 PATH 中有 `pnpm.cmd` shim；失败后先 `git restore` `bindings.ts`，再重跑（见记忆 windows-pnpm-cmd-shim）。
- **Windows 上的 clippy**：出现损坏时，用隔离的 `CARGO_TARGET_DIR` 加 `CARGO_INCREMENTAL=0`；在 PowerShell 中用 `--config build.rustc-wrapper=""`。不要 pin 工具链（见记忆 no-unilateral-toolchain-pin）。
- **macOS**：本地无法替代，以三平台 CI 为准。
- **Lint 范围**：`pnpm lint` 会被未跟踪文件拖累，按单项分别执行。
- **Smoke**（维护者，三平台）：
  - 退出、托盘退出、托盘重启、IPC 重启；
  - 更新安装流程（`cleanupProcesses` → install → relaunch）；
  - Windows 注销；
  - 热键在退出时注销；
  - 系统代理在退出时恢复；
  - widget 在退出时收尾；
  - 最终窗口几何的保存。

  没有执行的 smoke 如实标记为“未执行”。
