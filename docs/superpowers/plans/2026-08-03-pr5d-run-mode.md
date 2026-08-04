# PR-5d 实施计划 — 运行模式探针（C2）

**日期：** 2026-08-03
**版本：** v8（v7 判 57/100 后修订。核心是**把三处「未建立就使用的事实」变成建立好的事实**：退出路径上的 `Drop` 到底跑不跑（§4.6.3 九条事实链）、被取消的 leader 到底留下什么状态（§4.6.4 两维分离）、`ACTOR_STOP_BUDGET` 到底要覆盖多少 I/O（§4.7 公式）。另新增第四个接缝 S4，并把散落的 `rg` 判据收进 §8 统一编号）
**分支基线：** `refactor/core-manager-actor` @ **`049bd30dc`**

> **基线为什么从 `6f1a6683d` 前移**：v6 头部仍写着 `6f1a6683d`，而被审查的 HEAD 是 `049bd30dc`。已核实 `git diff --stat 6f1a6683d..049bd30dc` **只动 `docs/superpowers/plans/` 三个文件**，因此 §2.1 的全部源码锚点不受影响——但**陈旧标记正是 5c 收尾那两个提交（`a062f1019`/`59a38dfb0`）要清掉的东西**，不能自己留一条。

**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/` 下**两个文件都算数**——`task.md` 卡 C2 + `design.md` §9（`:333` Service control 段直接管着本阶段）。**只读 `task.md` 会漏掉约束**，§2.4 记了一次实际漏检
**姊妹计划：** `docs/superpowers/plans/2026-08-03-pr5e-macos-dns.md`（C3）。§4.7 是两份文档之间的**契约**，出现分歧以本节与 5e 对应节的**显式互指**为准
**平台：** Windows 11 / PowerShell

---

## 0.0 v8 修订记录（逐条对应 v7 的判定）

| v7 的问题                                                                                                   | v8 的处置                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| §4.6.3 把「退出时 `CoreClientInner::drop` 跑不跑」这个**未建立的事实**同时用在两个相反方向                  | **已建立**：读 `tauri`/`tao` 源码得九条事实链，结论是**两条退出路径上都不触发**。安全断言成立且更强；那句宽慰是空话，已删（§4.6.3）                                   |
| R-C2-4 的前提写成「重启 actor 而不退进程才失效」                                                            | **加宽**：`restart_application`（`help.rs:279-296`）是**已发布路径**——先关停、再 spawn 后继、再 `process::exit`，超时时旧核可能与新实例并存                           |
| `ShutdownCompletion::drop` 在取消路径上也置 `Closed` ⇒ **拆除被永久跳过且无上报**；T-SD-08 还把它断言成正确 | **两维分离**（`entrants_closed` / `teardown`）：取消 → 退回 `NotStarted` + 发 `shutdown_abandoned` + 由 follower 接替。T-SD-08 定向推翻旧断言，新增 T-SD-10（§4.6.4） |
| 预算义务写成三个常量之和                                                                                    | **改写成对 5e §4.4 算法的公式**：`4×DNS_READ + DNS_WRITE + DNS_IPC + …`，并注明 5e 全文零命中 `ACTOR_STOP_BUDGET`，本文档单方面写不出约束力（§4.7、§2.5 D5）          |
| R-C2-7 交给 5e，但其处置**落在三个槽位之外** ⇒「5e 只是填槽」为假                                           | **声明第四个槽位 S4**（`apply_mode` 失败尾，S2 的补集），claim 收敛为「改动落在 S1–S4 四个槽位内」（§4.7）                                                            |
| 锁序不变式经 §5→§7→「门禁」三重转引，而 §8 里**没有这条门禁**                                               | 新增 **`G-LOCK-01`** 并给出化归论证；**§8 升为门禁唯一权威清单**，散在 §7/§9 的判据全部收编并编号                                                                     |
| G-SEAM 只证明词法位置，标记可藏在不可达代码里                                                               | 四个槽位各加 **`#[cfg(test)]` 哨兵**，让「空槽被走到」也可验（§4.7 末、`G-SEAM-SENTINEL`）                                                                            |
| T-SD-04/08 缺 barrier，删掉 `notify_waiters()` 也可能照绿                                                   | 两条都要求**显式 barrier**：先确认 follower 已注册再放行 leader（§6）                                                                                                 |
| T-PROBE-06 不覆盖 `.kill_on_drop(true)`                                                                     | **如实登记为未覆盖**（§7 单列一行 + §6 末给出不补测的理由 + §9 一行），不再默认它被验证                                                                               |
| 步骤 ⑤ 措辞仍不准确（「它们已经进去了」）                                                                   | 改为「通过了第一次检查、正排队；**真正拒绝它们的是第二次检查**」（§4.6.2）                                                                                            |
| 未记 S11 契约收窄                                                                                           | §1 第 9 条、§9 两行：`client/mod.rs:454` 注释须改；`:457`/`:461` 不受影响；收窄写进 PR 描述                                                                           |

> **codex 关于「PR-5e §4.8 与 S2 矛盾」的那条不予采纳**：它读的是已提交的 **5e v1**，而 v2 正按 v7 的位置重写。**这是审查时序造成的，不是本文档的缺陷**（leader 已裁定并统一协调）。

---

## 0.1 v7 修订记录（逐条对应 v6 的判定）

| v6 的问题                                                  | v7 的处置                                                                                                                                 |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| §4.7 的 S2/S3 槽位与 PR-5e 冲突                            | **PR-5e 的位置胜出**：S2 移进收敛实现的成功尾部（§4.5 `apply_mode`）、S3 移进 actor 的 `Shutdown` 臂。S1 保持原位，但改为**一处共享实现** |
| §4.7 声称「§4.4 每一行都到达 S2」                          | **假命题，已删**。第 2/4/6 行是收敛失败行，**到不了**。§4.7 逐行写明，并记 **R-C2-7**                                                     |
| `shutdown()` ② `rebuild.shutdown()` 无界，在新超时**之前** | 加 `REBUILD_DRAIN_BUDGET`；**复合上界写成具名预算之和**；T-SD-06 用**在飞**的 rebuild 验                                                  |
| ⑤ 的升级分支恒不触发（ractor 超时是 `Ok(Timeout)`）        | 改为按 `CallResult` 分支；**并撤回 `stop(None)` 本身**——理由见 §4.6.3                                                                     |
| 单次飞行状态机没有完成转移                                 | 补 leader/follower 选举 API + **RAII 完成守卫**（§4.6.4）；准入**在选举那一刻就关**，v6 的第 ③ 步删除                                     |
| 锁序无记录；§1 称九处统一持准入                            | §1 改表逐处写明；锁序不变式写进 §4.6.1                                                                                                    |
| 步骤 ⑥「持 permit 到 actor 停止之后」措辞强于机制          | §4.6.2 如实改写：信号量公平，**它只推迟错误，不阻止任何执行**                                                                             |
| T-SEAM-01 是源码门禁冒充行为测试                           | 拆成 **T-SEAM-01（行为，六处遍历同一实现）** + **§8 的 G-SEAM-01/02/03（标记门禁）**                                                      |
| §5 只有一行聚合的接缝断言                                  | 拆成四行（S1 定序与六处遍历、S2 仅成功路径、S3 位置、槽位无需自取守卫）                                                                   |
| `ACTOR_STOP_BUDGET` 不在常量表                             | §4.3 补两行（含 `REBUILD_DRAIN_BUDGET`）并加 **owner 列**                                                                                 |
| 残留清单三处互不一致                                       | 统一为 R-C2-1…7，§9 只留一行引用全集                                                                                                      |
| §9「C3 未被触碰」与 §1/§3.1 矛盾                           | 改写为真实范围：签名 + 函数体一行 + 调用点两行                                                                                            |
| §3.2 用 T-CTL 论证「六个方法全都必要」                     | 论证收窄给 T-MODE-02；结论不变                                                                                                            |

---

## 0. 为什么收窄而不是又一次修补

v2→v4 三轮对抗审的分数是 43 → 57 → 60，**但七条 BLOCKING 的分布比分数更能说明问题**：

| 归属                                                                | v4 的 BLOCKING 条数 |
| ------------------------------------------------------------------- | ------------------- |
| **纯 C3**（DNS 恢复、候选集、漂移搁浅、R6、write-Ok/read-previous） | **5**               |
| **C2∩C3 接缝**（控制失败漏重施加、关停竞态、drain 与 actor 挂死）   | **3**               |
| **纯 C2**                                                           | **0**               |

C2 侧两条（探针有界性、警告归属）本轮均判 **RESOLVED**。**每一条存活缺陷要么在 C3 内部，要么在两者的接缝上。**

三轮审查反复出现的**同一个形状**是：「计划声明了一条原则，但具体机制没有实现它」。这个形状正是**一份文档同时持有两套独立并发协议**时的产物——写 C2 顺序时想不到它要承载一个 C3 步骤，写 C3 恢复时又忘了前文立的原则。

**耦合是单向的**：C3 依赖 C2（拆除要在控制动作前、重施加要在 `Run` 后、适配器选择读 `state.mode`）；**C2 不依赖 C3**。

**唯一看似双向的点已解决**：D2 要删 `RunType::default()`，而 `core/clash/core.rs:78`（legacy `change_default_network_dns` 内）调它。v4 的处置是「随 C3 迁走」。收窄后改为：**把 mode 作为参数传进去**——调用点 `feat.rs:409` **已经**通过 `client.core_status()` 拿到了 `run_type`（`client/mod.rs:475-489` 第三个返回值），今天被 `let (state, _, _)` 丢掉。**用调用方手上已有的值，不需要 C3。**

**收窄不引入回归**：今天 DNS 与 start/stop 本就毫无保序，C2-only 让 legacy DNS 路径**原样不动**，`feat.rs:416-418` 的迁移标记改指 PR-5e。

---

## 1. 边界

**做（C2 + D2）：**

1. 服务状态探针（一次性、经兼容门控、**自身有界**、可注入）；
2. **九处调用点统一定序**——但**准入不是九处都有**（**v6 §1 写「九处统一为准入→守卫→…」是假的**，据此纠正）：

   | 调用点                          | `ControlAdmission`       | `CoreOperationGuard`     | 外部控制命令 |
   | ------------------------------- | ------------------------ | ------------------------ | ------------ |
   | #1 bootstrap                    | **无**（actor 尚不存在） | **无**                   | 无           |
   | #2–#7 六个控制入口              | **有**                   | 有                       | **有**       |
   | #8 `enable_service_mode` 变更后 | **无**                   | 有（`reconcile()` 自取） | 无           |
   | #9 boot 的 `init_service`       | **无**                   | 有（`reconcile()` 自取） | 无           |

   **因此 §5 的「关停开始后不再有新控制序列进入」只覆盖 #2–#7**，不覆盖 #8/#9。#8/#9 不发外部命令，关停期间最坏情形是一次与拆除并发的模式收敛；它的 actor 调用在 `Shutdown` 被处理之后一律返回 `ShuttingDown`。**如实记为 R-C2-5**，不靠措辞盖过去。锁序不变式见 §4.6.1。

3. 修 Service→Normal 缺口；
4. **控制动作失败的处置表**（保留基线「无论成败都 reconcile」语义）；
5. **关停静默期协议**（`ControlAdmission`）；
6. **先建后删**地移除 5 s 轮询与三个 statics；
7. **D2**：`CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`（删 statics 的前置阻塞）；
8. **为 PR-5e 声明三个接缝**（§4.7）——**槽位与前置条件在本 PR 冻结**，槽内内容由 5e 填；
9. **收窄 PR-5a 的 S11 契约（本 PR 的可见外溢，必须记账）**：`REBUILD_DRAIN_BUDGET`（§4.6.2 ②）使 `client/mod.rs:454` 的「**awaiting both exits**」不再成立，该行注释须随实施修正。**`:457`「rebuild 先于 core actor」与 `:461`「在飞 rebuild 允许跑完」都不受影响**——本 PR 放弃的是**等待**，不是**取消**（`client/rebuild.rs:215` 明说中途取消不安全，我们没有这么做）。**收窄本身写进 PR 描述**（§9）。
10. **两处一行改动，随本 PR 落地：**

- **`feat.rs:416-418` 的迁移标记改指 PR-5e。** 该标记现在写着「Remove when: **PR-5d** moves MacosDnsGuard into CoreActor」——拆分之后这句**已经不可能成真**。5c 刚刚才修正过一轮陈旧标记（`a062f1019`/`59a38dfb0`），**留一条新的陈旧标记正是那轮修正要防的事**。`ipc.rs:126` 指向 PR-5d 的那条**仍然正确，不动**。
- **`change_default_network_dns(run_type, enabled)` 加参。**

> **关于第 10 条第二项，须防被误读为「C3 泄漏进 C2」。真实范围是三处、共四行**（§9 的对应判据按这个写，不写「仅签名一处」）：
>
> | 处                             | 改动                                                    |
> | ------------------------------ | ------------------------------------------------------- |
> | `core/clash/core.rs` 签名      | 加 `run_type: RunType` 形参                             |
> | `core/clash/core.rs:78` 函数体 | `let run_type = RunType::default();` → 使用入参         |
> | `feat.rs:409`                  | `let (state, _, _)` → `let (state, _, run_type)`（F64） |
> | `feat.rs:420`                  | 传入 `run_type`——**已核实这是该函数唯一的调用点**       |
>
> DNS 逻辑（设备解析、快照、Service/Local 分叉、写入）**一行不动**，整体迁移属 PR-5e。改它的唯一理由是 D2 要删 `RunType::default()`，而它是四个调用点之一。

**不做：** C3 macOS DNS 全部（→ PR-5e）；`UpdaterManager::global()`（PR-6d）；五个 owner-PR globals；`feat::patch_verge` 的 sysproxy/systray/locale 编排（PR-6e）。

**5c 携带的 `KILL_FLAG` weak-CAS 缺陷**（`control.rs:274`）随轮询线程删除而消失，不单独修。

---

## 2. 锚点复核

### 2.1 已核实的锚点（基线 `6f1a6683d`）

`core/actor/gate.rs:20-30`、`:32-45`、`:55-60`、`:73-83`；`core/actor/request.rs:70-75`、`:78-92`（`:82-85` 提前返回、`:87` 取守卫、`:88` `set_backend`）；`core/actor/types.rs:44-50`、`:68-79`；`core/actor/mod.rs:37-50`、`:52-69`、`:185-190`、`:367-371`、`:436`、`:603-615`；`core/service/ipc.rs:28-30`（三 statics）、`:85-101`（`:97` 是 5 s）、`:103-124`（`:108` 警告条件）、`:131-138`；`core/service/mod.rs:18-30`（忙等在 `:26-28`）；`core/service/compat.rs:15-27`、`:29-52`、`:55-57`；`core/service/control.rs:58,99-102,106,149,188,234,283`、`:350-376`；`client/mod.rs:85`、`:244-264`、`:303-306`、`:455-465`、`:467-473`、`:475-489`、`:504-539`、`:544`、`:2767`；`client/core.rs:39-43`、`:105-119`、`:111`、`:277-283`、`:1207-1214`；`client/rebuild.rs:150-167`；`feat.rs:383`、`:401`、`:409`；`utils/init/mod.rs:251`；`utils/help.rs:263`；`.github/workflows/ci.yml:201-215,303-304,306-308`；`package.json:40`→`:42`。

### 2.2 关键事实

| ID      | 事实                                                                                                                                                           | 锚点                                                                                  |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| F9      | `pending_run_type` 在 Rust 源码中**不存在** → 卡面该项是 **no-op**                                                                                             | 全仓 `rg`（命中仅 `docs/`）                                                           |
| F11     | 5 s 轮询与三个 statics 在一个文件；`spawn_health_check` **定义**在 `ipc.rs`，**四处调用点在别处**                                                              | `ipc.rs:28-30,85-101`；**调用点 `control.rs:101,229,324` + `core/service/mod.rs:25`** |
| F12     | `get_ipc_state()` **5 处生产读**                                                                                                                               | `feat.rs:383,401`；`client/mod.rs:305,544`；`core/clash/core.rs:48`                   |
| F13     | `RunType::default()` 读两个 legacy global，被 `CoreStatusView::initial()` 调用                                                                                 | `core/clash/core.rs:39-56`；`core/actor/types.rs:44-50`                               |
| F14     | `set_backend` 生产调用点恰好一个；**不存在 `set_mode`**                                                                                                        | `core/actor/request.rs:88`                                                            |
| F16     | `uninstall_service` **绕过 facade**；`install_service` 在 facade 上**不 reconcile**                                                                            | `ipc.rs:936-937`；`client/mod.rs:504-510`                                             |
| F35     | **`IPC_STATE` 初值 `Disconnected`**，bootstrap 在任何 health check 之前读它 → **今天 bootstrap 恒判 `Normal`**                                                 | `ipc.rs:28`；`client/mod.rs:303-306`                                                  |
| F36     | 探针两半已存在：`control::status()` + 纯函数 `target_ipc_state()`                                                                                              | `control.rs:350-376`；`ipc.rs:131-138`、`:103-124`                                    |
| F42     | **`CoreStatusView::initial()` 有两个调用点**；`core/actor/mod.rs:371` 随后把 `run_type` 覆盖掉——actor 侧早已注入，那次 `RunType::default()` 是**白读**         | `client/core.rs:111`；`core/actor/mod.rs:367-371`                                     |
| F45     | 六个控制入口签名不齐；`update`/`uninstall` **不在 trait 上**                                                                                                   | `control.rs:58,106,149,188,234,283`；`backend.rs:619-624`                             |
| F49     | **install 之后服务会自己起来**，且紧接着拉起 health checker                                                                                                    | `control.rs:99-102`                                                                   |
| F58     | **facade 已有 `NyanpasuClient::shutdown()`**，已是有序两步，带 PR-5a S11 契约注释；生产入口 `utils/help.rs:263`                                                | `client/mod.rs:455-465`                                                               |
| F59     | **`ServiceCompat::Unknown` 有两个来源**：`status != Running` 与 `status == Running` 但 `server` 为 `None`                                                      | `core/service/compat.rs:29-52`                                                        |
| F60     | **`RebuildCoordinator::shutdown` 就是「关准入 + 等在飞」的现成范式**                                                                                           | `client/rebuild.rs:150-167`                                                           |
| F61     | **今天三个 facade 控制方法「无论控制成败都 reconcile」**，控制错误**之后**才返回                                                                               | `client/mod.rs:512-538`                                                               |
| F62     | **今天的警告条件是合取式** `status == Running && !allows_service_backend()`，覆盖 Running 下三种 compat                                                        | `ipc.rs:108`                                                                          |
| **F64** | **`feat.rs:409` 的调用方已经持有 `run_type`**：`client.core_status()` 返回三元组第三项即 `RunType`，当前被 `let (state, _, _)` 丢弃。**这是 C2/C3 解耦的支点** | `feat.rs:409`；`client/mod.rs:475-489`                                                |
| F33     | CI 有 macOS runner 且在 PR 上跑 `cargo test --all-features`                                                                                                    | `ci.yml:201-215`、`:303-304`；`package.json:40`→`:42`                                 |

### 2.3 5c 收尾五提交

`0e20f35ba`/`b3fe68035`（ledger 扫描器，**加固**本计划 §7 依赖的两条门禁）、`a86478a7f`（roadmap §6.3 移交）、`a062f1019`/`59a38dfb0`（迁移标记改指 PR-5d）。**均无 C2 锚点影响。**

### 2.4 一次实际漏检（保留记录）

v3 曾据 `rg` 零命中断言「不引入完整 `ServiceControlPort`」已被删除。**那次 `rg` 只扫了 roadmap 与 `task.md`**，正本在 **`design.md:333`**，从未被碰过。

**方法论**：「在 X 与 Y 上零命中」与「该约束不存在」是两个命题。**下断言前先把量词范围定死，再让检索覆盖整个范围。** 计划头部原先只写「权威 spec：`task.md`」，是漏检的结构性成因，已改正。

---

### 2.5 受管辖点清单 —— **本计划声明的每条原则，逐一列出它治理的位置**

> **这张表存在的理由**：三轮审查反复抓到的是「声明了原则，但某处机制没实现它」。**清单让审查者核对列表，而不是重新发现遗漏。**
>
> **枚举方法（可复核，不是断言）**：对每条原则，先写下它的**量词范围**（「所有 X」里的 X 是什么），再用一次可重跑的检索把该范围内的全部实例列出来，最后逐个填处置。检索式写在表里，审查者可以重跑。

**原则 A：错误通道报告的是调用的结果，永远不是副作用的缺席。**
范围 = 本计划中**所有会引发外部副作用的调用**。检索：`rg -n 'service_control\.|\.probe|force_local_with|actor_ref\.call|rebuild\.shutdown|inflight\.acquire'`。

| #      | 位置                                                                                                  | 若把 `Err` / 超时当成「没发生」会怎样                                                                                              | 本计划的处置                                                                                                           |
| ------ | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| A1     | `service_control.<action>()` 返回 `Err`                                                               | runner 可能**已经部分**启动/停止/替换了 daemon 才非零退出。早退 = 把陈旧的后端判断留到某个无关操作才纠正                           | **仍然 reconcile**（§4.4 定则 3，与基线 F61 同形）                                                                     |
| A2     | `force_local_with` 返回 `Err`                                                                         | 模式**可能已经**被改。当成没改会让状态与现实分叉                                                                                   | §4.4 第 4 行：**返回 `Err` 并同时发两条降级**，不假装模式还是原样                                                      |
| A3     | 探针返回 `error`                                                                                      | daemon 状态**未知**，不等于「没在跑」                                                                                              | **fail-closed 判 `Disconnected`**（与今天 `health_check` 的 `Err` 分支同语义），并**发降级**（§4.3），不静默           |
| A4     | `actor_ref.call(Shutdown, Some(ACTOR_STOP_BUDGET))` 返回 `Ok(Timeout)` / `Ok(SenderError)` / `Err(_)` | 超时既不代表 actor 没在拆除，也不代表它拆完了；**尤其不代表 `backend.shutdown()`（`mod.rs:609`，全仓唯一调用点）执行过或没执行过** | §4.6.3：**报告为降级关停**（`shutdown_actor_stop_timeout`），具名残留 **R-C2-4**；**不发 `stop(None)`**——理由见 §4.6.3 |
| **A5** | `timeout(REBUILD_DRAIN_BUDGET, rebuild.shutdown())` 超时                                              | 在飞的 rebuild **仍在跑**，可能已把运行时配置写了一半。当成「rebuild 结束了」会让后续步骤按错误前提推进                            | §4.6.2：降级 `shutdown_rebuild_drain_timeout` + **R-C2-6**；后续步骤**按「rebuild 可能仍在飞」写**，不按「已结束」写   |
| **A6** | `timeout(QUIESCE_BUDGET, inflight.acquire())` 超时                                                    | 被放弃的控制序列**可能仍在执行外部 OS 命令**                                                                                       | §4.6.2：降级 `shutdown_quiesce_timeout` + 既有 **R-C2-1**，措辞为「等待有界」而非「已静默」                            |

**原则 B：签名只能保证「值到得了这里」与「值到不了这里」。**
范围 = 本计划所有靠签名承担的契约。检索：`rg -n 'fn reconcile|fn reconcile_with|fn force_local_with|fn apply_mode|fn run_control_sequence'`。

| #      | 契约                                | 由签名承担的部分                                                         | **不由签名承担、另行落点的部分**                                                                |
| ------ | ----------------------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| B1     | 陈旧探针结果喂不进 reconcile        | `reconcile`/`reconcile_with` **没有 `IpcState` 参数** ⇒ 那个值到不了这里 | 「探针不得在守卫外开始」——**签名管不了**，落 `rg` 门禁（§7）                                    |
| B2     | `force_local_with` 只在超时分支使用 | **无**——签名不表达调用位置                                               | 全部落 `rg` 门禁：恰好一处调用点                                                                |
| **B3** | 每一次成功收敛都经过 S2 槽位        | `apply_mode` 是**私有**的 ⇒ 模块外无法绕过三个公开入口自行收敛           | 「模块**内**不另开第二条收敛路径」「三个入口都调它」——签名管不了，落 `rg` 门禁（§7）+ T-SEAM-02 |
| **B4** | 六个 facade 控制方法走同一条序列    | `run_control_sequence` 是**私有**的，六个公开方法是它的薄包装            | 「六个方法都调它、且不自己重写序列」——落 T-SEAM-01（行为）+ `rg` 门禁                           |

**原则 C：凡「不会去做某事」型契约，一律靠测试 / 门禁 / `rg`，且必须说得出怎么验。**
范围 = §7 契约表中「由谁保证」非「签名 / cfg」的每一行——**该表即本原则的完整实例清单**，不另列。

**原则 D（v7 新增）：跨文档契约的每一处，两份文档必须能互相指到对方；单方面冻结的槽位不算契约。**
范围 = §4.7 的三个接缝。**这条原则是 v6 那次失败的直接产物**——v6 单方面冻结了 S2/S3，而 PR-5e 的 §4.6/§4.8 早已把 owner 与位置定在别处，**两份文档各自内部自洽、合起来无法实施**。检索：`rg -n 'PR-5e|pr5e|SEAM-5E' docs/superpowers/plans/2026-08-03-pr5d-run-mode.md` 与 `rg -n 'PR-5d|pr5d' docs/superpowers/plans/2026-08-03-pr5e-macos-dns.md`，**逐条配对**。

| #   | 接缝                         | 本文档的落点                       | **PR-5e 侧的对应落点**                                                                                              | 一致性                                                                        |
| --- | ---------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| D1  | S1 拆除                      | §4.7 表；`run_control_sequence` 内 | 5e §4.7「六个入口，在调用外部控制动作之前，都先在同一守卫内 `await` 拆除」                                          | **一致**（v6 亦一致）                                                         |
| D2  | S2 重施加                    | §4.7 表；`apply_mode` 成功尾部     | 5e §4.8「owner 唯一 = `CoreModeReconciler`……`reconcile_with` 的末尾（仍持守卫）」                                   | **v7 改判后一致**；v6 冻在 facade 调用点，与 5e 冲突                          |
| D3  | S3 关停恢复                  | §4.7 表；actor `Shutdown` 臂内     | 5e §4.6「主路径（Stop / `Shutdown` / SetBackend）：处理器内**显式 `await` 恢复**，在后端动作与 reply **之前**完成」 | **v7 改判后一致**；v6 冻在 facade，与 5e 冲突且**结构上做不到**（§4.7 S3 段） |
| D4  | S4 收敛失败尾                | §4.7「这就要求第四个槽位」段       | **5e 侧尚无对应节**——R-C2-7 的处置在 5e v1 里不存在                                                                 | **待 5e v2 建立**；本 PR 已冻结槽位与前置条件                                 |
| D5  | `ACTOR_STOP_BUDGET` 预算义务 | §4.3 owner 列 + §4.7 公式          | **5e 全文零命中 `ACTOR_STOP_BUDGET`**                                                                               | **不一致，且本文档单方面写不出约束力**——需 5e 侧反向条目（leader 已转交）     |

> **D4/D5 两行现在都是「不一致」，这是有意留在表里的。** 本原则的用处正是让**尚未闭合的跨文档契约可见**，而不是等它们在实施期暴露。**一份只列已一致项的对照表没有价值。**

---

## 3. 已裁定事项

### 3.1 D2 = A —— `CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`

| `RunType::default()` 调用点                 | 处置                                                                                                                                                                                                            |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core/actor/types.rs:48`                    | D2 主目标，改为参数                                                                                                                                                                                             |
| **`core/clash/core.rs:78`**                 | **改为参数**：`change_default_network_dns(run_type, enabled)`，值由 `feat.rs:409` 已持有的 `core_status().2` 传入（F64）。**这是 C2 与 C3 解耦的那一处**——legacy DNS 函数本体其余部分一行不动，整体迁移属 PR-5e |
| `client/core.rs:1211`（测试）               | 断言注入 mode，**并改名**为 `initial_watch_snapshot_reflects_the_injected_mode`——旧名的「legacy empty status」在 D2 之后不再是参照物，**命名即契约**                                                            |
| `client/process_core_bridge.rs:251`（注释） | 删后悬空，顺手清理                                                                                                                                                                                              |

| `CoreStatusView::initial()` 调用点（F42） | 处置                                                  |
| ----------------------------------------- | ----------------------------------------------------- |
| `client/core.rs:111`                      | 改 `initial(args.mode)`                               |
| `core/actor/mod.rs:367`                   | 改 `initial(args.mode)`，**并删掉 `:371` 的覆盖赋值** |

### 3.2 六个控制入口统一（两处不对称都是缺陷）

> **更正**：v2 曾称「`install_service` 不 reconcile 是有意的」。**F49 证明这在基线上就是假的**（`control.rs:99-102` 明写多数平台自动启动并拉起 health checker）。carve-out 整条删除。

| 入口                              | 今天                                                           | 到齐的动作                                             |
| --------------------------------- | -------------------------------------------------------------- | ------------------------------------------------------ |
| `install_service` `control.rs:58` | 收 reconciler（只为在 `:100-102` 起轮询）；facade 不 reconcile | **删参**；facade 统一序列                              |
| `start_service` `:188`            | 收 reconciler                                                  | **删参**；facade 序列                                  |
| `restart_service` `:283`          | 收 reconciler                                                  | **删参**；facade 序列                                  |
| `stop_service` `:234`             | 不收                                                           | 上 trait；facade 序列                                  |
| `update_service` `:106`           | 不收；调用点在 `utils/init/mod.rs:251`                         | 上 trait；**改由 facade 调用**；facade 序列 + 有界等待 |
| `uninstall_service` `:149`        | 不收；被 `ipc.rs:936-937` 直调                                 | 上 trait；**改由 facade 调用**；facade 序列            |

**结果：六个签名一致（`async fn(&self) -> anyhow::Result<()>`），六个都在 `ServiceControlOps` 上。**

> **与 `design.md:333`「不引入完整 `ServiceControlPort`，除非测试确实需要替换 OS command runner」不冲突**，两条独立论证：
>
> **论证一（判例，主）**：`plans/2026-08-02-pr5a-core-actor.md:1037` 已确立读法——该约束针对**迁进 CoreActor**，而非「存在可测边界」；5a 据此建了今天的四方法 trait，**经十二轮审查无异议**。PR-5d 补两个方法，**六个具体函数一行不搬**，仍在 `core::service::control`。**与任何文档增删无关。**
>
> **论证二（例外条款，条件已证成）**：
>
> | 测试         | 为什么必须能替换 runner                          | 缺哪个方法就写不出来                                |
> | ------------ | ------------------------------------------------ | --------------------------------------------------- |
> | T-MODE-02    | 六个控制动作**各自独立断言** probe+reconcile     | **`update`、`uninstall`——例外条款由这一行独力承担** |
> | T-SEAM-01    | 六个 facade 方法**各自**遍历同一条控制序列       | 同上                                                |
> | T-MODE-04/05 | 有界等待三路，需在无真实 daemon 下让 update 返回 | `update`                                            |
> | T-CTL-01…04  | 控制失败四种处置，需让控制动作**按脚本失败**     | **任一可脚本化的动作即可**——见下方更正              |
>
> **更正（v6 论证过强）**：v6 在 T-CTL 那行写「六个全部」。**不成立**——T-CTL-01…04 检验的是**错误处置策略**（控制错误优先、失败后仍 reconcile、失败后跳过就绪等待），策略不随动作而异，用**代表性动作**（如 `start` + `update`）即可覆盖。**结论不变**（仍须扩到六方法），但承担例外条款的是 **T-MODE-02 与 T-SEAM-01 的「逐个入口独立断言」**，不是 T-CTL。**论证过强本身就是本轮反复出现的那个形状**，故显式改正而不是悄悄改字。
>
> **真实 runner 不行**：`update_service`（`control.rs:106-147`）与 `uninstall_service`（`:149-186`）经 `runas`/`sudo` 提权调真实服务二进制；CI 三平台上要么二进制不存在、要么触发提权交互。
>
> **不需要扩就够用的**（如实划清）：T-MODE-03。
>
> **仍须写进 PR 描述**：四方法扩到六方法是**对既有边界的可见扩大**。

---

## 4. 设计

### 4.1 探针（一次性、经兼容门控、**自身有界**）

```rust
// core/service/probe.rs（新）
pub(crate) struct ProbeOutcome {
    pub state: IpcState,
    pub compat: ServiceCompat,
    /// `None` = 探针自身失败（子进程错误 / 超时）。
    /// 保留它才能复现基线的合取式警告条件（F62），因为 `ServiceCompat::Unknown`
    /// 同时来自「没在跑」与「在跑但没上报 server」两种情形（F59）。
    pub daemon_status: Option<ServiceStatus>,
    pub error: Option<anyhow::Error>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait ServiceProbe: Send + Sync + 'static {
    async fn probe_within(&self, budget: Duration) -> ProbeOutcome;
    async fn probe(&self) -> ProbeOutcome { self.probe_within(DEFAULT_PROBE_BUDGET).await }
}

/// **可替换的低层接缝**——存在的唯一理由是让「超时真的生效」可被测试（§6 注）。
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait ServiceStatusRunner: Send + Sync + 'static {
    async fn status(&self) -> anyhow::Result<StatusInfo<'static>>;
}

pub(crate) struct OsServiceStatusRunner;                              // 子进程 + kill_on_drop(true)
pub(crate) struct OsServiceProbe { runner: Arc<dyn ServiceStatusRunner> }
```

**有界性放在 `OsServiceProbe::probe_within` 内部：**

```rust
match tokio::time::timeout(budget, self.runner.status()).await {
    Ok(Ok(info))  => { let (state, compat) = target_ipc_state(&info);
                       ProbeOutcome { state, compat, daemon_status: Some(info.status), error: None } }
    Ok(Err(e))    => ProbeOutcome { state: Disconnected, compat: Unknown, daemon_status: None, error: Some(e) }
    Err(_elapsed) => ProbeOutcome { state: Disconnected, compat: Unknown, daemon_status: None,
                                    error: Some(anyhow!("probe timed out after {budget:?}")) }
}
```

> **为什么必须在源头**：「每个调用方都要包一层 timeout」是**「不会忘记」型契约**，无法强制。四个调用点里三个**持着操作许可或阻塞着启动**，一次挂死的 `status --json` 会把核心门**无限期占住**——新的取门请求会超时，**活跃那个永不释放**。
>
> **为什么还要 `ServiceStatusRunner` 这层**（审查者点名的可测性问题）：一个永不返回的 `MockServiceProbe` **绕过了** `OsServiceProbe` 里的 timeout，删掉那行 timeout 测试照绿=空转。要让「超时生效」可测，**挂死必须发生在 timeout 之内**，即 runner 层。

**`OsServiceStatusRunner` 须设 `.kill_on_drop(true)`**（`control.rs:352-356` 今天没设）：`timeout` 丢弃 future 只取消等待，**`tokio::process::Command` 默认不杀子进程**。

**`target_ipc_state` 与 `ServiceCompat` 一行不改**——PR-5-pre 已审的 fail-closed 门。

**注入路径：**

```text
client/mod.rs::try_new_with_args
  └─ ClientSetupArgs { .., probe: Arc<dyn ServiceProbe>, .. }   ← 新字段，紧挨 service_control（:85）
       ├─ bootstrap 自用：:303-306 的 get_ipc_state() 换成 probe.probe().await
       ├─ NyanpasuClientInner { .., probe, admission }          ← 两个新字段（admission 见 §4.6）
       └─ core_mode_reconciler()（:467-473）加两个字段：
            └─ CoreModeReconciler { core, application, requests, probe, degradation }
                                                                （request.rs:70-75）
```

`CoreModeReconciler` 是 `#[derive(Clone)]`，加两个 `Arc<dyn _>` 不破坏 Clone。测试侧沿用 `test_service_control()`（`client/mod.rs:2767`）的模式加 `test_service_probe()`。

> **`degradation` 是 v6 漏掉的一个字段，不是新需求。** §4.3 的 `report_probe_diagnostics` 要 `publish` 降级，而 reconciler 侧的探针诊断与 `await_service_ready` 都要用它；v6 的注入图只加了 `probe`，那样写不出来。沿用既有的 `ClientSetupArgs.degradation`（`client/mod.rs:84`，`Arc<dyn CoreDegradationSink>`）与 bootstrap 已构造的 `client_degradation`（`:280`），**不新增 sink 类型**。诊断函数本身是自由函数，理由见 §4.3。

### 4.2 九处调用点

**统一形态 = 一处共享实现，不是六份复制。** 六个 facade 控制方法（#2–#7）全部收敛到 `NyanpasuClient::run_control_sequence` 这一个私有方法上；六个公开方法只剩「构造 `ServiceControlAction` 并转发」一行。

```rust
// client/mod.rs（私有）
enum ServiceControlAction { Install, Start, Stop, Restart, Update, Uninstall }

async fn run_control_sequence(&self, action: ServiceControlAction) -> anyhow::Result<()> {
    let _permit = self.inner.admission.enter().await?;   // §4.6；已关停则 Err(ShuttingDown)
    let guard = self.inner.core_client.begin_operation().await?;

    // ── SEAM-5E-S1（PR-5e 填；本 PR 为空）──────────────────────────
    // 拆除 DNS 覆写。`action` 在此作用域内可见，5e 按入口分岔
    // （uninstall 中止 / 其余降级）无需把本函数拆成六份。
    // ───────────────────────────────────────────────────────────────

    self.inner.admission.check_open()?;                  // 紧贴外部命令之前再查一次
    let result = self.inner.service_control.dispatch(action).await;   // **不早退**，见 §4.4
    // [仅 Update 且 result.is_ok()] await_service_ready（§4.3）
    // reconcile_with(&guard) —— **无论 result 成败都跑**（F61）；处置见 §4.4 六行表
}
```

`dispatch` 是 `ServiceControlOps` 上的 `match action { Install => self.install().await, .. }`——**六个方法签名已在 §3.2 统一为 `async fn(&self) -> anyhow::Result<()>`，所以 match 六臂各一行。**

> **为什么必须是一处而不是六处**：S1 是 PR-5e 的插入点，而 5e 需要**六个入口各自**在控制动作前拆除（其 §4.7 与 T-DNS-05/06/17/18/26/27）。六份复制意味着六个可以各自漂移的插入点，**「都插对了」变成一条无法用签名或类型表达的「不会忘记」型契约**。一处共享实现把它降级成「六个方法都调它」，那是可以用行为测试钉死的（T-SEAM-01）。5e 的六条独立入口测试仍然照写、照过——它们断言的是每个入口的可观察行为，一处实现同样交付。

| #   | 位置                                                  | 今天                                        | 改为                                                                          | 持准入？         |
| --- | ----------------------------------------------------- | ------------------------------------------- | ----------------------------------------------------------------------------- | ---------------- |
| 1   | **bootstrap**（`client/mod.rs:303`）                  | `get_ipc_state()`（恒 `Disconnected`，F35） | `probe()` 一次——**顺带修掉 F35**。**唯一不在守卫内的探针**，理由见 §4.5       | 否               |
| 2   | install（facade `:504-510`）                          | 不 reconcile（F16）                         | `run_control_sequence(Install)`                                               | **是**           |
| 3   | start（`:512-521`）                                   | 轮询 + `reconcile(get_ipc_state())`         | `run_control_sequence(Start)`                                                 | **是**           |
| 4   | restart（`:530-539`）                                 | 同上                                        | `run_control_sequence(Restart)`                                               | **是**           |
| 5   | stop（`:523-528`）                                    | 同上                                        | `run_control_sequence(Stop)`                                                  | **是**           |
| 6   | uninstall（今在 `ipc.rs:936-937`）                    | 无                                          | **迁到 facade** → `run_control_sequence(Uninstall)`                           | **是**           |
| 7   | update（今在 `utils/init/mod.rs:251`）                | 轮询                                        | **迁到 facade** → `run_control_sequence(Update)` + 有界等待——**关系 smoke 2** | **是**           |
| 8   | `enable_service_mode` 变更后                          | 轮询 + reconcile（有 §4.8 的洞）            | `reconcile()`（自取守卫版）                                                   | **否**（R-C2-5） |
| 9   | boot 的 `init_service`（`core/service/mod.rs:18-30`） | 起轮询线程 + 忙等 100 ms                    | `reconcile()`，**删忙等与整个函数**                                           | **否**（R-C2-5） |

### 4.3 诊断归属与有界等待就绪

**诊断有三个来源，但只有一个实现**（审查者点名 bootstrap 无归属）：

| 场景                                                                                                                                                                            | 接手方                                            | 动作                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `daemon_status == Some(Running) && !compat.allows_service_backend()`——**逐字复现 `ipc.rs:108` 的合取式**，覆盖 Running 下 `Unknown`/`Incompatible`/`Unparsable` 三种（F59/F62） | `report_probe_diagnostics`，**单一实现**          | `tracing::warn!(?compat, ..)`（smoke 2 要的就是这一条）                                            |
| `error.is_some()`                                                                                                                                                               | 同上                                              | `warn!` + degradation `service_probe_failed`、`retryable = true`                                   |
| **bootstrap 的那次探针**                                                                                                                                                        | **bootstrap 调同一个 `report_probe_diagnostics`** | 同上二者。degradation sink 在 bootstrap 处已构造（`client/mod.rs:280` `client_degradation`），可用 |

**「单一实现」要落到形状上**：`report_probe_diagnostics` 是 `core/service/probe.rs` 里的**自由函数**

```rust
pub(crate) fn report_probe_diagnostics(outcome: &ProbeOutcome, degradation: &dyn CoreDegradationSink);
```

**不能**做成 `CoreModeReconciler` 的方法——bootstrap 的那次探针发生在 `client/mod.rs:303`，而 reconciler 要到 client 构造完成之后才存在，届时**根本没有对象可调**。reconciler 侧写一个转发它的私有方法（下面代码里的 `self.report_probe_diagnostics`），bootstrap 直接传 `client_degradation`（`client/mod.rs:280`）。**两个调用点，一个实现。**

**`await_service_ready` 的每一次非就绪结果也走同一个接手方**（v4 用 `_` 通配丢掉了）。

```rust
// CoreModeReconciler::await_service_ready(&self, guard: &CoreOperationGuard) -> ReadyOutcome
let deadline = Instant::now() + READY_BUDGET;
let mut backoff = INITIAL_BACKOFF;
loop {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { return ReadyOutcome::TimedOut; }
    // **per-probe 预算与剩余时间取 min**——否则总耗时可超出 READY_BUDGET 近一个
    // PER_PROBE_BUDGET（审查者点名）。
    let outcome = self.probe.probe_within(remaining.min(PER_PROBE_BUDGET)).await;
    self.report_probe_diagnostics(&outcome);
    if outcome.state == IpcState::Connected && outcome.compat.allows_service_backend() {
        return ReadyOutcome::Ready;
    }
    tokio::time::sleep(backoff.min(deadline.saturating_duration_since(Instant::now()))).await;
    backoff = (backoff * 2).min(MAX_BACKOFF);
}
```

**超时去向裁定为 degraded，不是 `Err`**：更新进程本身成功退出了；返回 `Err` 等于告诉用户「更新失败了」，而那是假的。Local 是**合法运行状态**。与 5b 的 I-A 同源：**已经成功的事不许报成失败；没做成的后置副作用报降级。**

**常量来源分开记：**

| 常量                                   | 实测？ | 依据                                                                                                                       | **owner（谁有权改 / 谁必须复核）**                                                                                                                                                                                  |
| -------------------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `READY_BUDGET`                         | **是** | 实测 daemon 从 `update_service()` 返回到 `status()` 报兼容的耗时上界                                                       | PR-5d                                                                                                                                                                                                               |
| `PER_PROBE_BUDGET`                     | **是** | 实测一次正常 `control::status()` 子进程往返上界                                                                            | PR-5d                                                                                                                                                                                                               |
| `QUIESCE_BUDGET`（§4.6.2 ③）           | **是** | 实测最慢控制动作（`update`）的正常耗时上界                                                                                 | PR-5d                                                                                                                                                                                                               |
| **`REBUILD_DRAIN_BUDGET`**（§4.6.2 ②） | **是** | 实测一次在飞 rebuild 从进入 `rebuild()` 到返回的耗时上界（`client/rebuild.rs:216`）                                        | PR-5d                                                                                                                                                                                                               |
| **`ACTOR_STOP_BUDGET`**（§4.6.3）      | **是** | 实测 `Shutdown` 臂正常路径耗时上界——即 `backend.shutdown().await`（`core/actor/mod.rs:609`，**全仓唯一调用点**）的正常上界 | PR-5d **提出**；**PR-5e 必须复核**——5e 把 S3 的 DNS I/O 放进同一个 `Shutdown` 臂（§4.7），该预算必须覆盖 5e 的 `DNS_READ_BUDGET`/`DNS_WRITE_BUDGET`/`DNS_IPC_BUDGET` 之和，否则关停会**稳定**超时并**稳定**跳过清理 |
| `INITIAL_BACKOFF` / `MAX_BACKOFF`      | 否     | 不是正确性边界，**如实标注为选定值**                                                                                       | PR-5d                                                                                                                                                                                                               |

> **这张表是为了防「常量凭空出现」而建的**，v6 引入 `ACTOR_STOP_BUDGET` 却没在这里落行——**表本身没被使用，就等于没有**。v7 补两行，并加 owner 列，因为 `ACTOR_STOP_BUDGET` 是**唯一一个跨 PR 的预算**。

### 4.4 控制动作失败的处置

**基线行为必须保留**（F61）：runner 可能**部分**启动/停止/替换了 daemon 后才非零退出——立即返回会把陈旧的后端判断留到某个无关的后续操作才纠正。

| #   | 控制动作  | 就绪等待（仅 update） | reconcile                    | **返回**                     | degradation                                          |
| --- | --------- | --------------------- | ---------------------------- | ---------------------------- | ---------------------------------------------------- |
| 1   | `Ok`      | Ready / 不适用        | `Ok`                         | `Ok`                         | —                                                    |
| 2   | `Ok`      | Ready / 不适用        | `Err`                        | **reconcile 的 `Err`**       | —                                                    |
| 3   | `Ok`      | **TimedOut**          | `force_local_with` `Ok`      | **`Ok`**                     | `service_update_not_ready`                           |
| 4   | `Ok`      | TimedOut              | `force_local_with` **`Err`** | **`Err`**                    | `service_update_not_ready` + `mode_reconcile_failed` |
| 5   | **`Err`** | **跳过**              | **照跑**，`Ok`               | **控制动作的 `Err`**         | —                                                    |
| 6   | **`Err`** | **跳过**              | **照跑**，`Err`              | **控制动作的 `Err`**（优先） | `mode_reconcile_failed`                              |

**三条定则：**

1. **控制错误优先级最高**——与基线 `control?` 在 reconcile 之后的写法一致。用户问的是「我这次操作成没成」。
2. **控制失败后跳过就绪等待**：`update_service` 都失败了，等一个新 daemon 就绪没有意义。
3. **控制失败后仍然 reconcile**：这是**唯一**能把「部分生效」的现实同步回来的机会。

> **本表与 S2 槽位的关系**：**第 1/3/5 行到达 S2，第 2/4/6 行到不了**（收敛失败会在 `apply_mode` 的 `?` 处提前返回）。**v6 在 §4.7 写「每一行都经过这里」是假的。** 逐行核对表与后果见 §4.7 的「S2 的前置条件逐行核对」，后果记为 R-C2-7。

### 4.5 reconcile 的三个入口 + **一个共享收敛实现**

```rust
impl CoreModeReconciler {
    /// 自取守卫 → 转发。唯一的无守卫入口（#8、#9）。
    pub(crate) async fn reconcile(&self) -> anyhow::Result<()> {
        let guard = self.core.begin_operation().await?;
        self.reconcile_with(&guard).await
    }

    /// 已持守卫：**在守卫内探针** → 收敛。控制动作（#2..#7）用这个。**没有 IpcState 参数。**
    pub(crate) async fn reconcile_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()> {
        let app = self.application.get().await?.state;
        let outcome = self.probe.probe_within(PER_PROBE_BUDGET).await;
        self.report_probe_diagnostics(&outcome);
        let mode = crate::core::RunType::classify(app.enable_service_mode, outcome.state); // §4.8
        self.apply_mode(guard, mode, &app).await
    }

    /// 已持守卫、**不探针**、直接落 Local。**仅供 §4.3 超时分支**。
    pub(crate) async fn force_local_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()> {
        let app = self.application.get().await?.state;
        self.apply_mode(guard, crate::core::RunType::Local, &app).await
    }

    /// **三个入口唯一的收敛实现。私有。**
    async fn apply_mode(
        &self, guard: &CoreOperationGuard, mode: RunType, app: &ApplicationState,
    ) -> anyhow::Result<()> {
        self.core.set_backend(guard, mode).await?;
        let request = self.requests.for_product(app.core)?;
        self.core.run(guard, &request).await?;

        // ── SEAM-5E-S2（PR-5e 填；本 PR 为空）──────────────────────
        // **收敛成功尾部**：mode 已落、Run 已完成、守卫仍在手。
        // 只有走到这里才代表「收敛后的事实」已确立——这正是 5e §4.8
        // 判定重施加的依据。失败路径**到不了这里**，见 §4.7。
        // ───────────────────────────────────────────────────────────
        Ok(())
    }
}
```

**强制构造**：`reconcile`/`reconcile_with` **没有 `IpcState` 参数**——调用方在类型上就无法喂进陈旧探针结果。这是签名能给的那一类保证。

**`apply_mode` 私有且唯一**：模块外无法绕开三个公开入口自行收敛（`pub(crate)` 与 `private` 的差别在这里是实质的）。「模块**内**不另开第二条收敛路径」不由签名承担，落 §7 的 `rg` 门禁与 T-SEAM-02。

> **为什么 S2 必须落在这里，而不是 v6 写的调用点**（**leader 已裁定采用 PR-5e §4.8 的位置**）：
>
> 1. **调用点槽位覆盖不到 #8/#9。** 那两处走 `reconcile()`，根本不经过 §4.2 的 facade 序列——v6 把 S2 冻在 `reconcile_with(&guard).await` **返回之后的调用点上**，#8/#9 的每一次收敛都会绕过它。
> 2. **`force_local_with` 也是一次收敛。** §4.4 第 3 行（就绪超时 → 强制 Local → 成功）之后核在跑、模式已定，与第 1 行在「收敛后的事实」上没有区别。挂在调用点则要在两个地方各写一次。
> 3. **owner 归属**：PR-5e §4.8 明写「owner 唯一 = `CoreModeReconciler`，facade 不做重施加」。槽位放在 facade 调用点与该裁定直接冲突。

**「任何探针都不许在守卫外开始」是「不会去做某事」型契约**，落到 §7 门禁：`rg -n '\.probe(_within)?\('` 恰好三处（`reconcile_with` 内、`await_service_ready` 内、bootstrap）。

**bootstrap 是唯一守卫外探针，理由是真排除**：它在 `client/mod.rs:303`，而 `CoreClient::new` 在 `:312`——**actor 那时还不存在**，没有任何别的操作能在飞，也没有守卫可取。两行同在一个 `async move` 块，源码顺序即执行顺序。

`force_local_with` 同样上 `rg` 门禁：**恰好一处调用点**。

### 4.6 关停静默期协议

> **不把 `Shutdown` 变成守卫消息**：关停必须能在一个操作卡住时仍然生效，否则一次挂死的控制操作会让 app 关不掉。

**问题形状**：facade 控制序列在**外部**持有许可，而 `CoreClient::shutdown()` 发的是**无守卫**的 `Shutdown`（`client/core.rs:277-283`），actor 立即 `state.operation.shutdown()` 清掉活跃操作（`mod.rs:604`）。于是控制序列可能在「已经 await 过的关停」之后仍去执行外部 `start`/`install`/`restart`/`update`，而没有 actor 还能收敛它。

#### 4.6.1 `ControlAdmission` 与锁序不变式

```rust
enum AdmissionState {
    Open,
    /// 拆除已当选并在进行中。**准入在进入该态的那一刻就已关闭。**
    Closing(Arc<tokio::sync::Notify>),
    /// 拆除已终止（完成**或**被放弃）。不会再有第二次执行。
    Closed,
}

pub(crate) struct ControlAdmission {
    /// 用 std::sync::Mutex，**从不跨 await 持有**。
    state: std::sync::Mutex<AdmissionState>,
    /// 1 个许可，被整条控制序列持有（含外部命令）。
    inflight: Arc<tokio::sync::Semaphore>,
    /// §4.6.4 的「拆除被放弃」要在 **`Drop` 里**上报，而 `Drop` 是同步的。
    /// `CoreDegradationSink::publish` 恰好是同步方法（`core/actor/backend.rs:614-616`），
    /// 因此可以直接持有它，不需要另造一条异步上报通道。
    degradation: Arc<dyn CoreDegradationSink>,
}
```

**`AdmissionState` 的两个字段见 §4.6.4**（`entrants_closed` 与 `teardown` 是正交的两维，v7 把它们塞进一个三态枚举正是那条缺陷的根因）。`ControlAdmission` 在合成根 `try_new_with_args` 构造，注入 §4.1 已有的 `degradation`——**不新增依赖，也不是全局**。

**为什么需要与 `OperationGate` 并存的第二个机制**：`OperationGate` 只约束 **actor 消息**；`Shutdown` 本身是无守卫消息且会**主动清空**活跃许可，所以 gate 在关停面前不提供任何排他性。`ControlAdmission` 约束的是 **facade future 的存续**，包括那条 actor 完全看不见的外部 OS 命令。**作用域不同，不是冗余。**

**锁序不变式（本 PR 新增记录，不是新增约束）：**

> **任何同时取用两者的路径，必须先取 `ControlAdmission`、再取 `OperationGate`，释放顺序相反。**

**当下不存在反序**，已逐路径核实：#2–#7 六条控制路径先 `admission.enter()` 后 `begin_operation()`（§4.2 共享实现，一处）；#1 bootstrap 两者都不取；#8/#9 只取门、不取准入；`shutdown()` 只取准入、不取门。**记录它的理由不是修复现状，是防止将来出现「门 → 准入」的第三种路径**——那正是死锁的构造。落 §7 门禁。

**#8/#9 不受准入约束的后果**（§1 已列，此处给结论）：关停开始后，一次 `reconcile()` 仍可能取得守卫并收敛。它**不发外部 OS 命令**，因此 §4.6 要防的那个危害（外部命令活过 actor）不成立；其 actor 调用在 `Shutdown` 被处理后一律 `ShuttingDown`。**记为 R-C2-5，不用措辞盖过去。**

#### 4.6.2 关停序列（**全序列有界**）

```text
enter()：
 ① 查状态 → 非 Open 则 Err(ShuttingDown)
 ② acquire 许可（可能等待）
 ③ **再查一次** → 非 Open 则释放许可并 Err(ShuttingDown)
    ← ③是必需的：①与②之间可能发生关停

check_open()：查状态 → 非 Open 则 Err(ShuttingDown)

NyanpasuClient::shutdown()：
 ① role = admission.begin_shutdown()             ← 选举，见 §4.6.4
      Follower(n) → 等 n 后返回；Done → 立即返回
      Leader(completion) → 继续；**该次转移已经把准入关掉了**
 ② timeout(REBUILD_DRAIN_BUDGET, rebuild.shutdown())
      超时 → warn + degradation `shutdown_rebuild_drain_timeout`（R-C2-6）
 ③ permit = timeout(QUIESCE_BUDGET, inflight.acquire_owned())
      超时 → warn + degradation `shutdown_quiesce_timeout`（R-C2-1）
 ④ core_client.shutdown().await                  ← 见 §4.6.3，自身有界
 ⑤ drop(permit)                                  ← 见下方「⑤ 到底买到了什么」
 ⑥ completion.finish()                           ← **显式**：拆除确实跑完了 → Teardown::Done
      ← 若 ②③④ 中途 panic 或整个 future 被取消而到不了这里，
        `ShutdownCompletion::drop` 会把 teardown 退回 NotStarted、发
        `shutdown_abandoned` 降级并唤醒 follower，由下一个调用者接替（§4.6.4、T-SD-08）
```

**复合上界 = `REBUILD_DRAIN_BUDGET` + `QUIESCE_BUDGET` + `ACTOR_STOP_BUDGET`。** follower 的上界相同（它不可能早于 leader 开始）。

> **诚实限定**：这是**被 await 的时间**之和，不是墙钟上界；运行时被饿死或 executor 停摆不在其内。三个预算都在 §4.3 常量表有行、有依据、有 owner。

##### v6 的 ② 是无界的 —— 这条把 v6 的头号修复整个抵消掉了

v6 的序列第 ② 步写的是**裸** `rebuild.shutdown().await`。已核实 `client/rebuild.rs:155-166`，该方法结尾是

```rust
let _ = control.done_rx.await;   // 无超时
```

而 worker 侧的注释（`client/rebuild.rs:215`）写明：**「Once rebuild starts it intentionally runs to completion even if shutdown races in — cancellation mid-apply is not demonstrably safe.」** 一次在飞的 rebuild 可能正卡在 §4.6.3 所说的那些后端 await 上。**于是 shutdown 根本走不到 v6 序列的 ③（关准入）、④（`QUIESCE_BUDGET`）、⑤（`ACTOR_STOP_BUDGET`）——v6 给最后一步加的那个超时永远不会被执行到。** v6 的 T-SD-05 用的是空闲 rebuild 协调器，因此测不出这一点。

> **这就是「一份计划为修两件事而写，两件都没修完」的第二处**：v6 的头号修复（给 `Shutdown` RPC 加超时）本身是对的，但它装在一条**前驱无界**的序列上，等于没装。**上界必须整条序列成立，不是某一步成立。**

**改法**：② 包 `timeout(REBUILD_DRAIN_BUDGET, ..)`。超时后**不取消 rebuild**（worker 的注释说得对，中途取消不安全），只是不再等它；按 §2.5 A5 处置——**后续步骤按「rebuild 可能仍在飞」写**：它随后的 actor 调用会在 ④ 之后一律 `ShuttingDown`，因此可能留下一份只应用了一半的运行时配置。**记为 R-C2-6。**

**顺带修好的一件事**：v6 在 ③ 才关准入，也就是**整个无界的 rebuild 等待期间准入都还开着**。v7 在 ① 关（§4.6.4），关停一开始就没有新控制序列能进来。

##### ⑤ 到底买到了什么（v6 编号 ⑥；**它的措辞强于机制**）

v6 说「否则排在 close 之前入队的等待者会在 actor 拆除期间被唤醒」。**这条站不住**：

- `tokio::sync::Semaphore` 是**公平**的。排在 shutdown 的 `acquire` **之前**入队的等待者会先于 shutdown 拿到许可。**但注意措辞**：它们并非「已经进去了」——它们是**通过了 `enter()` 的第一次检查、正排在队列里**；拿到许可之后仍要过 `enter()` 的**第二次**状态检查，而此时准入已在 ① 关闭，所以**它们同样被拒**。**真正拒绝它们的是第二次检查，不是「先前已进入」。**（v7 这句写成「它们已经进去了」，仍然不准确——正是本节要修的那类毛病。）
- 排在 shutdown 的 `acquire` **之后**入队的等待者，同样在第二次检查上失败并立即释放。

**所以 ⑤ 的真实作用是：把那些注定失败的等待者的 `Err(ShuttingDown)` 推迟到拆除之后再返回。它不阻止任何执行，也不构成安全性质。** 保留它只因为代价为零、且让「同一时刻至多一条控制序列」这句话在拆除期间也逐字成立。§5 对应行按此改写，不再声称它阻止了什么。

#### 4.6.3 `core_client.shutdown()` 必须有界，但**不能**升级为 `stop(None)`

> **v5 曾把这条判给 PR-5e，错了**（leader 指出）：**后端操作一样会挂**，DNS 只是挂死点之一。**v6 加了超时，但超时分支从不触发，而一旦修好又会引入更糟的东西。**

**基线形状**：`CoreClient::shutdown()`（`client/core.rs:277-283`）是 `call(CoreActorMessage::Shutdown, None)`——无超时。ractor 逐条串行处理消息：只要某个处理器还卡在 `backend.run()` / `stop()` / `check()` / `recover()` / `observe_status()` / `replace_backend` 的 await 里，排队的 `Shutdown` 就永远轮不到。

**v6 的写法恒不触发。** v6 写的是：

```rust
match actor_ref.call(CoreActorMessage::Shutdown, Some(ACTOR_STOP_BUDGET)).await {
    Ok(_) => {}
    Err(_) => actor_ref.stop(None),
}
```

**ractor 把超时报成 `Ok(CallResult::Timeout)`，不是 `Err`。** 本仓自己的辅助函数就是这么写的（`client/core.rs:314-319`，`Ok(CallResult::SenderError | CallResult::Timeout) | Err(_)` 一起归为 `ShuttingDown`）。所以 `Ok(_) => {}` 把超时吞了，`stop(None)` 是死代码。

**而把它「修好」会引入一条 v6 没有记录的危害。** 已核实 `ractor-0.16.2/src/actor/actor_cell.rs:~150` 的 `listen_in_priority` 顺序：**1. signal → 2. stop → 3. supervision → 4. 普通消息**。于是：

- 处理器**真的卡死**时，`stop(None)` 同样排队，**不生效**——它对自己被引入的那个场景毫无作用；
- 处理器只是**慢**、随后返回时，`stop` 因为在更高优先级的端口上，**会抢在已排队的 `Shutdown` 之前终止 actor**。而 `Shutdown` 臂（`core/actor/mod.rs:603-616`）是 **`backend.shutdown().await` 的全仓唯一调用点**，且该 actor **没有 `post_stop`**（impl 只定义了 `handle`）。**清理因此被整个跳过。** Service 模式下 daemon 是独立进程，**进程退出不会带走它管的核**。
- PR-5e 把 S3 的 DNS 恢复放进同一个 `Shutdown` 臂（§4.7）之后，被跳过的还包括 **DNS 恢复**。

**裁定：保留超时，撤回升级。**

```rust
// CoreClient::shutdown() -> ShutdownDisposition
match self.inner.actor_ref
        .call(CoreActorMessage::Shutdown, Some(ACTOR_STOP_BUDGET)).await {
    Ok(CallResult::Success(())) => ShutdownDisposition::Completed,
    // 超时 / 发送失败 / 通道错误：**不发 stop(None)**，理由见下。
    Ok(CallResult::Timeout | CallResult::SenderError) | Err(_) => {
        ShutdownDisposition::AbandonedUnverified
    }
}
```

facade 收到 `AbandonedUnverified` 时按 §2.5 原则 A 处置：**报告降级关停，不报告干净关停**——`degradation.publish(shutdown_actor_stop_timeout)`，消息须写明**后端清理是否执行未知**。不改 `NyanpasuClient::shutdown()` 的签名（返回 `()`），因为唯一生产调用点 `utils/help.rs:263` 在退出路径上不消费返回值；**降级通道就是本计划一贯的报告机制**（§4.3、§4.4 同形），测试也从它观察。

**为什么撤回优于修好（逐案穷举，不是偏好）：**

| 处理器实际状态            | 发 `stop(None)`                         | 不发                                                      |
| ------------------------- | --------------------------------------- | --------------------------------------------------------- |
| 永久卡死                  | 排队，不生效。清理无论如何跑不了        | 同左                                                      |
| 慢，随后返回              | **抢在 `Shutdown` 前终止 → 清理被跳过** | `Shutdown` 照常执行 → **后端清理与 5e 的 DNS 恢复都跑到** |
| 已经死了（`SenderError`） | no-op                                   | no-op                                                     |

**没有任何一格是 `stop(None)` 更好的。**

##### 上表第二行依赖一个前提，v7 没有查证它 —— **v8 已经查证，逐级引用如下**

> **v7 的缺陷不是结论错，是论证里有一处未建立的事实被同时用在两个相反方向上。** v7 一边用 `CoreClientInner::drop`（`client/core.rs:377-380`）当作「不升级也不会漏掉 actor 终止」的宽慰，一边断言「不升级则排队的 `Shutdown` 一定还有机会执行」。**这两句依赖同一个事实：退出时那个 `Drop` 到底跑不跑。** 跑，则宽慰成立而安全断言破（`Drop` 里的 `stop(None)` 会抢在排队的 `Shutdown` 前面）；不跑，则安全断言成立而宽慰是空话。**v7 两句都写了，却没有定过是哪一种。**

**已建立的事实链（全部读源码，非从命名推断）：**

| #   | 事实                                                                                                                                           | 出处                                                                                                           |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| 1   | `CoreClient` 是 `#[derive(Clone)]` 且只含 `inner: Arc<CoreClientInner>`，故 `CoreClientInner::drop` 只在**最后一个** clone 消失时触发          | `client/core.rs:53-55`、`:377-380`                                                                             |
| 2   | `NyanpasuClient` 由 Tauri **managed state** 持有；`cleanup_processes` 取的是一个 **clone**（`state.inner().clone()`），丢掉它不是最后一次 drop | `utils/help.rs:258-260`                                                                                        |
| 3   | 应用主循环是 `app.run(...)`；`RunEvent::ExitRequested{ code: Some(_) }` 分支调用 `cleanup_processes` → `client.shutdown().await`               | `lib.rs:346`、`:350-352`；`help.rs:261-264`                                                                    |
| 4   | `App::run` **消费 `self`**（`App` 持有 `manager: Arc<AppManager>`，managed state 在其中），并把控制权交给 `runtime.run(..)`                    | `tauri-2.11.5/src/app.rs:1366-1374`                                                                            |
| 5   | wry 的 `Runtime::run` 直接调 `self.event_loop.run(event_handler)`                                                                              | `tauri-runtime-wry-2.11.4/src/lib.rs:3238-3242`                                                                |
| 6   | tao 的 `EventLoop::run` 返回类型是 **`!`**（永不返回）                                                                                         | `tao-0.35.3/src/event_loop.rs:220`                                                                             |
| 7   | 三个桌面平台的实现都以 `process::exit(exit_code)` 收场                                                                                         | `tao-0.35.3/src/platform_impl/windows/event_loop.rs:230`、`macos/event_loop.rs:202`、`linux/event_loop.rs:998` |
| 8   | Tauri 退出前只调 `cleanup_before_exit`，它清的是 tray icon / resources table / window，**完全不碰 managed state**                              | `tauri-2.11.5/src/app.rs:1108-1120`、`:1430-1437`                                                              |
| 9   | `restart_application` 走的是另一条路：显式 `std::process::exit(0)`，**`std::process::exit` 不运行任何析构函数**                                | `utils/help.rs:295`                                                                                            |

**结论（两条退出路径都已定案，不是「取决于 Tauri，无法静态确定」）：**

> **`CoreClientInner::drop` 在两条已发布的退出路径上都不会触发。**
> `quit_application`（`help.rs:274-276`）经 4→7：`App` 永不被 drop，managed state 永不被 drop。
> `restart_application`（`help.rs:279-296`）经 9：`process::exit` 直接跳过析构。

因此：

- **安全断言成立，而且比 v7 声称的更强**——退出路径上**根本没有任何东西**发送 `stop(None)`，所以排队的 `Shutdown` 不可能被优先级停止端口抢占。上表第二行「不发」列如实。
- **v7 那句宽慰是空话，已删。** actor 在退出时的终止来自**进程销毁**，不来自 `Drop`，也不来自任何升级；`Drop` 里那行 `stop(None)` 只在**非退出**场景（client 被真正丢弃）才有意义。**升级在已发布的退出路径上一分钱都买不到**——这反而是撤回它的第三条独立理由。

**诚实边界（这条 v6/v7 写对了，保留并收紧）：**

> ractor 无法抢占一个正在执行的 `handle()`。因此上面**只保证 facade 的等待有界，不保证 actor 终止、更不保证清理发生**。关停场景下可接受**仅因为进程随后就销毁**（事实 6/7/9，不再是假设）；**但「进程销毁」不等于「清理已发生」**——见 R-C2-4 与下面 `restart_application` 那一段。

##### `restart_application` 把 R-C2-4 从假设变成一条**已发布代码路径**上的实际后果

`utils/help.rs:279-296` 的顺序是：

```text
:280  cleanup_processes(app_handle)      → client.shutdown().await（可能在复合上界处超时返回）
:290  std::process::Command::new(path).spawn()   ← **后继进程在此刻已经起来了**
:294  app_handle.exit(0)
:295  std::process::exit(0)              ← 不运行析构
```

**若 `:280` 的关停在 `ACTOR_STOP_BUDGET` 处超时，`backend.shutdown()` 可能没跑，而 `:290` 立刻拉起一个新实例。** Service 模式下 daemon 是独立进程，**不随本进程销毁**，于是旧核可能与新实例并存。这不是「重启 actor 而不退进程」那种假想用法——**进程确实退出了，只是后继紧接着启动**。R-C2-4 的措辞据此加宽。

**一个方向相反的事实也要记**：v7 之前这条路径上的 `client.shutdown()` 是**无界**的，一次挂死的处理器会让 `cleanup_processes` 永远不返回，`:290` 永远到不了——**重启功能整个卡死**。复合上界把「重启卡死」换成了「重启可能与旧核并存」。**这是一次有意的取舍，不是净损失，两边都写出来。**

**因此 §5 那一行的措辞是**「关停**的等待**有界」，**不是**「关停一定完成」，**更不是**「清理一定发生」。

> **顺带一条对用户可见的收益**：`cleanup_processes` 在 `RunEvent::ExitRequested` 分支里被调用（`lib.rs:350-352`），也就是**在事件循环线程上** `block_on`（`help.rs:261`）。复合上界因此同时是「退出时 UI 最多卡多久」的上界。

#### 4.6.4 单次飞行：leader / follower 选举 + RAII 完成守卫

> v6 声明了 `Open / Closing(Arc<Notify>) / Closed` 三态，**但 ①–⑥ 里没有任何一步把 leader 转成 `Closed`、也没有任何一步唤醒 follower**。T-SD-04 描述了想要的行为，计划却没有提供实现它的机制。v6 的序列还有一处自相矛盾：若 ① 就转 `Closing` 且 `enter()` 拒绝 `Closing`，准入其实在 ① 关，不在它写的 ③。

**v7 的裁定保留：选举与关准入是同一次转移，都发生在 ①。** v6 的独立 `admission.close()` 步骤删除。

##### v7 的三态把两件事塞进了一个 `Closed`，**结果是取消 = 永久跳过拆除**

> v7 的 `ShutdownCompletion::drop` 在**每一条**退出路径上转 `Closing → Closed`——这正是它被设计出来的目的，但也意味着：leader 当选 → 外层 timeout / 任务 abort / panic 在拆除完成**之前**取消了它 → `Drop` 置 `Closed` → follower 醒来并认定「拆除已完成」→ 此后每一次 `shutdown()` 都得到 `Done` → **rebuild 与 actor 拆除永远不会发生，而且什么都不上报**。
>
> **v7 的 T-SD-08 把这个行为断言成了正确行为**，于是缺陷被测试固化。它还与 §4.6.4 自己那句「清理是否发生由降级通道报告」直接冲突——被取消的那一次**一条降级也没发**。

**根因**：`Closed` 同时承担了两个互不等价的意思——**「拆除跑完了」**与**「leader 不在了」**。v8 把它们拆开，并把「准入是否关闭」从「拆除进行到哪一步」里分离出来（两者本来就正交）：

```rust
struct AdmissionState {
    /// **一经置位永不清除**：关停一旦开始，准入不再重开。
    entrants_closed: bool,
    teardown: Teardown,
}

enum Teardown {
    NotStarted,
    Running(Arc<Notify>),
    Done,
}
```

| `entrants_closed` | `teardown`       | `enter()` | `begin_shutdown()`          | 含义                                           |
| ----------------- | ---------------- | --------- | --------------------------- | ---------------------------------------------- |
| `false`           | `NotStarted`     | 放行      | 当选 → `Leader`             | 正常                                           |
| `true`            | `Running(n)`     | **拒绝**  | `Follower(n)`               | 准入已关，拆除进行中                           |
| `true`            | **`NotStarted`** | **拒绝**  | **当选 → `Leader`（重试）** | 准入已关，**上一任 leader 被取消，拆除未完成** |
| `true`            | `Done`           | **拒绝**  | `Done`                      | 准入已关，拆除**确实跑完了**                   |

**第三行是新增的那个态**，它就是「leader 不在了但活没干完」。注意它**不需要第四个枚举值**——`entrants_closed = true` 与 `teardown = NotStarted` 的组合本身就表达了它，这也是把两个维度拆开的收益。

```rust
impl ControlAdmission {
    pub(crate) fn begin_shutdown(self: &Arc<Self>) -> ShutdownRole {
        let mut state = self.state.lock().expect("control admission");
        state.entrants_closed = true;                    // ← 准入在此刻关闭，且永不重开
        match &state.teardown {
            Teardown::NotStarted => {                    // 首次当选，或**接替被取消的 leader**
                let notify = Arc::new(Notify::new());
                state.teardown = Teardown::Running(notify);
                ShutdownRole::Leader(ShutdownCompletion { admission: self.clone(), finished: false })
            }
            Teardown::Running(notify) => ShutdownRole::Follower(notify.clone()),
            Teardown::Done => ShutdownRole::Done,
        }
    }
}

impl ShutdownCompletion {
    /// 正常路径**显式**调用（消费 self）：拆除确实跑完了。
    pub(crate) fn finish(mut self) {
        self.finished = true;                            // Drop 随即按「已完成」处理
    }
}

impl Drop for ShutdownCompletion {
    fn drop(&mut self) {
        let (notify, abandoned) = {
            let mut state = self.admission.state.lock().expect("control admission");
            match std::mem::replace(&mut state.teardown, Teardown::NotStarted) {
                Teardown::Running(n) if self.finished => {
                    state.teardown = Teardown::Done;     // 完成
                    (Some(n), false)
                }
                // **被取消 / panic**：退回 NotStarted，让下一个调用者接替。
                // entrants_closed 保持 true——准入不因放弃而重开。
                Teardown::Running(n) => (Some(n), true),
                other => { state.teardown = other; (None, false) }
            }
        };
        if abandoned {
            // publish 是同步的（backend.rs:614-616），Drop 里可以调。
            self.admission.degradation.publish(Degradation::shutdown_abandoned());
        }
        if let Some(n) = notify { n.notify_waiters(); }
    }
}
```

**follower 醒来后必须重新选举，而不是直接返回**——因为它可能是被「放弃」唤醒的：

```rust
loop {
    match self.inner.admission.begin_shutdown() {
        ShutdownRole::Done => break,                   // 拆除确实跑完了
        ShutdownRole::Leader(completion) => {          // 接替被取消的 leader
            self.run_teardown(completion).await;       // 内部结束时 completion.finish()
            break;
        }
        ShutdownRole::Follower(notify) => {
            let waiting = notify.notified();           // **先注册**
            if self.inner.admission.is_teardown_done() { break; }   // **后复查**
            waiting.await;
        }
    }
}
```

> **「先注册、后复查」这一段 codex 复核为本来就安全**：`Notified` 在**创建时**就记下 generation，因此 `notify_waiters()` 与 `.await` 之间没有丢唤醒窗口。**保留它不是因为有窗口，而是因为唤醒之后必须重新判断醒来的原因**（完成？还是放弃？）——上面的 `loop` 才是它现在存在的理由。

**语义收紧（v7 那句话本身也是错的）：**

- `Teardown::Done` 现在**只**表示「拆除跑完了」。follower 收到 `Done` 才可以认为没有活儿剩下。
- **「跑完了」仍然不等于「清理成功」**——`ACTOR_STOP_BUDGET` 超时属于「跑完了但清理未知」，由 `shutdown_actor_stop_timeout` 降级报告（§4.6.3）。
- **被放弃**由 `shutdown_abandoned` 降级报告，并且**允许一次受控重试**：准入始终关闭，所以重试期间不会有新控制序列混进来。

**这三条现在各有各的可观测信号**，不再挤在一个状态里。

**残留（如实记账，§8）**：R-C2-1、R-C2-2、**R-C2-4**、**R-C2-5**、**R-C2-6**。

### 4.7 为 PR-5e 声明的四个接缝（**空槽，但槽位与前置条件在本 PR 定死**）

> **拆分只有在这一节存在时才真正省事。** v4 的三条接缝 BLOCKING（控制失败漏重施加、关停竞态、drain 与 actor 挂死）之所以能降级为「在这里加一步」，前提是**本 PR 交付的序列已经写明这一步该插在哪、插入时周围保证什么**。若 5d 交付一条没有声明接缝的序列，5e 仍然是一次重新设计，拆分白拆。
>
> **本节是契约，不是备注**：槽位的**位置**与**前置条件**由本 PR 冻结；**槽内内容**由 PR-5e 填。
>
> **v7 的改判**：v6 的 S2 与 S3 冻在了 **PR-5e 用不了的位置**——所以它们不是「5e 不得移动」，而是**本节必须改到 5e 的位置去**。leader 已裁定：**S2/S3 采 PR-5e 的位置，S1 保持 v6 的位置**（leader 曾考虑把 S1 移到 `check_open()` 之后并**已撤回**：现在这个顺序才让 `check_open()` 能抓到「拆除期间才开始的关停」并压掉那条控制命令）。此后若再要移动，仍须回过头修订本节并说明理由。

| 槽位            | **精确位置**                                                                                                                           | **进入该槽时，本 PR 保证成立的前置条件**                                                                                                                                                                                                                                                  | 5e 将填入                                                                                     |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| **S1 拆除**     | `NyanpasuClient::run_control_sequence`（§4.2，**唯一实现**）：`begin_operation()` 之后、`admission.check_open()?` **之前**             | ①已持 `CoreOperationGuard`（FIFO 序已确定）；②已通过准入；③**外部控制命令尚未发出**；④**`action: ServiceControlAction` 在作用域内**——5e 按入口分岔无需拆成六份；⑤**六个 facade 方法全部经由此处**（T-SEAM-01 钉住）                                                                       | 拆 DNS 覆写并 `await`；失败按入口分岔（uninstall 中止／其余降级）                             |
| **S2 重施加**   | `CoreModeReconciler::apply_mode`（§4.5，**三个入口唯一的收敛实现**）：`self.core.run(..)` 返回 `Ok` 之后、`Ok(())` 之前                | ①仍持同一守卫；②`set_backend` 与 `run` **均已成功**，因此 `state.running` 反映的是**收敛后**的事实；③**#2–#9 的每一次成功收敛都经过此处**（含 #8/#9 的 `reconcile()` 与 §4.4 第 3 行的 `force_local_with`；#1 bootstrap 只探针、不收敛，故不在范围内）；④**失败收敛到不了此处**——逐行见下 | 按「`running.is_some()` **且** TUN 期望开启」决定是否重施加；失败以降级呈现，**不改变返回值** |
| **S3 关停恢复** | `CoreActor` 的 `Shutdown` 臂（`core/actor/mod.rs:603-615`）：`backend.shutdown().await`（`:609`）与 `reply.send(())`（`:613`）**之前** | ①actor 正在处理 `Shutdown`，**无其他处理器并发**；②facade **不会**在该处理器返回前强制终止 actor（§4.6.3 已撤回 `stop(None)`）；③`state.backend` 尚未被消费；④reply 尚未发出（T-SD-07 钉住「reply 在后端动作之后」）；⑤**facade 侧不留 S3 槽位**                                          | 在处理器内 `await` 恢复；失败进降级                                                           |

> **第四个槽位 S4（收敛失败尾）在下文「这就要求第四个槽位」一段声明**，它是 S2 的补集，两者合起来覆盖 §4.4 的全部六行。上表三行 + S4 = **四个**冻结槽位。

**四条槽位的共同前置**：S1/S2/S4 位于**同一个 `CoreOperationGuard` 之内**，S3 位于 **actor 处理器之内**，因此 5e 填入的步骤**一律不需要自取守卫**——actor 内自取守卫正是 v2 那条构造性死锁（PR-5e §4.9 已完整论证）。

#### S2 的前置条件逐行核对 —— **v6 在这里立了一条假的全称命题**

v6 写「§4.4 六行处置表的每一行都经过这里」。**第 2/4/6 行恰恰是收敛失败的那三行**，`apply_mode` 会在 `?` 处提前返回，**到不了尾部**。

| §4.4 行 | 收敛调用           | 结果  | 到达 S2？                                        |
| ------- | ------------------ | ----- | ------------------------------------------------ |
| 1       | `reconcile_with`   | `Ok`  | **是**                                           |
| 2       | `reconcile_with`   | `Err` | **否**——`set_backend`/`run`/`for_product` 已失败 |
| 3       | `force_local_with` | `Ok`  | **是**（与第 1 行同为一次成功收敛）              |
| 4       | `force_local_with` | `Err` | **否**                                           |
| 5       | `reconcile_with`   | `Ok`  | **是**（控制动作 `Err` 不影响收敛是否成功）      |
| 6       | `reconcile_with`   | `Err` | **否**                                           |

**第 2/4/6 行会发生什么，必须写出来而不是留一条全称句：** S1 已经把 DNS 覆写拆掉了，而收敛失败意味着**我们并不知道核处于什么状态**，因此**不重施加**是正确的处置——按「收敛后的事实」触发的机制，在没有确立的事实时就不该触发。代价是：若此时 TUN 仍被期望开启且核实际上在跑，DNS 覆写就缺席了。**记为 R-C2-7。**

##### 这就要求第四个槽位 —— **否则「5e 只是填槽」这句话在 R-C2-7 上是假的**

> v7 把 R-C2-7 的 owner 记成 PR-5e 就算交代完了。**不成立**：要让第 2/4/6 行**可见地降级**，5e 必须改动**失败路径**上的代码，而失败路径**不在 S1/S2/S3 任何一个槽位里**。于是 5e 在这一项上不得不动槽位之外的代码——**而「5e 只是填槽」正是拆分本身的全部理由**。这不是措辞问题。

**裁定：声明第四个槽位 S4，而不是弱化那句话。** 代价是把 `apply_mode` 拆成「一个内层 + 一个成功尾 + 一个失败尾」的对称形状——**本来就该对称**，v7 只写了成功尾是不完整的：

```rust
async fn apply_mode(&self, guard: &CoreOperationGuard, mode: RunType, app: &ApplicationState)
    -> anyhow::Result<()>
{
    match self.apply_mode_inner(guard, mode, app).await {
        Ok(()) => {
            // ── SEAM-5E-S2（成功尾）── 见上文
            Ok(())
        }
        Err(error) => {
            // ── SEAM-5E-S4（失败尾；PR-5e 填；本 PR 为空）────────────
            // 收敛失败：不重施加（无「收敛后的事实」可依据），但**必须可见**。
            // 5e 在此发 macos_dns_reapply_skipped 之类的降级。
            // **不改变返回值**——错误优先级仍按 §4.4 三条定则。
            // ───────────────────────────────────────────────────────────
            Err(error)
        }
    }
}

/// `set_backend` → `for_product` → `run`，三个 `?` 都落在这里。
async fn apply_mode_inner(&self, ..) -> anyhow::Result<()> { .. }
```

**S4 的前置条件（与其余三个同格式冻结）：** ①仍持同一守卫；②收敛**已失败**，`error` 在作用域内；③**§4.4 第 2/4/6 行恰好都到达此处**，且**只有**这三行到达——这正是 S2 的补集，两者合起来覆盖六行；④**不得改变返回值**，只能新增可观测信号。

**因此「5e 只是填槽」这句话的准确形式是：5e 的改动落在 S1/S2/S3/S4 四个已冻结槽位内。** 四个之外若还需要动别的代码，**回来修订本节**——这条规矩本身不变。

#### S3 的位置为什么必须在 actor 内 —— **v6 放在 facade 是做不到的**

v6 把 S3 冻在 facade 的 drain 之后、`core_client.shutdown()` 之前。两条**已核实**的理由否掉它：

1. **facade 那一点上没有 `CoreOperationGuard`。** 关停序列全程不取守卫（§4.6），而 PR-5e 的 `SetTunDns` 携带 `OperationId` 并由 `validate_operation` 校验（其 §4.9）。facade 无从构造一个合法的 `OperationId`；临时取一个守卫又会与正在拆除的门相冲。
2. **一条 facade 级的恢复 RPC 会排在有界的 `Shutdown` RPC 之前，然后卡在同一个挂死的处理器后面**——它会把 §4.6 刚建立起来的有界性整个毁掉。

PR-5e §4.6 本来就把主路径（`Stop` / `Shutdown` / `SetBackend`）的恢复放在**处理器内、后端动作与 reply 之前**，且不经 `SetTunDns` 消息，因此不需要 `OperationId`。**采 5e 的位置。**

**本 PR 因此欠 5e 一条预算义务。v7 把它写成「三个常量之和」，那是低估了**——5e §4.4 的恢复是一个**遍历全部候选适配器**的循环，每个候选都要「写 + 强制回读」，而候选排序与 Service 写前漂移预检各自还要一次默认设备读。按 5e §4.4/§3.1 的算法逐步计数：

| 步骤                                                            | 出处（PR-5e） | 代价                   |
| --------------------------------------------------------------- | ------------- | ---------------------- |
| 候选排序：`can_address(target)`（Service 侧要解析当前默认设备） | §3.1 表、§4.4 | 1 × `DNS_READ_BUDGET`  |
| 候选 A（Service）写：写前漂移预检                               | §3.1 表       | 1 × `DNS_READ_BUDGET`  |
| 候选 A 写本身（IPC）                                            | §3.2          | 1 × `DNS_IPC_BUDGET`   |
| 候选 A **强制回读**（推进的唯一依据）                           | §4.4 算法     | 1 × `DNS_READ_BUDGET`  |
| 候选 B（Local）写                                               | §3.2          | 1 × `DNS_WRITE_BUDGET` |
| 候选 B **强制回读**                                             | §4.4 算法     | 1 × `DNS_READ_BUDGET`  |

**因此义务的正确形式是一条公式，不是一个和：**

```text
ACTOR_STOP_BUDGET  ≥  4 × DNS_READ_BUDGET
                    +  1 × DNS_WRITE_BUDGET
                    +  1 × DNS_IPC_BUDGET
                    +  backend.shutdown() 的正常上界（§4.3 本行原有依据）
                    +  调度余量
```

**这是下界，不是定值**：5e §9 的 R1b 建议「Local 写前也加一次默认设备比对」，若采纳则读次数变 5；候选集或循环结构一旦改变，公式必须**重新推导**，不是把常数改大。**写成公式的全部意义就在这里**——一个和会让人以为改 5e 的算法不影响这条约束。

若该式不成立，关停会**稳定**超时、**稳定**走进 §4.6.3 的 `AbandonedUnverified`，从而**稳定**跳过清理与 DNS 恢复。

> **「5e 必须复核」这句话写在本文档里不产生任何约束力**——PR-5e 全文没有一处 `ACTOR_STOP_BUDGET`。**本条义务必须在 5e 侧有一行对应的反向条目才算落地**，否则它只是本文档的一厢情愿。leader 已将该要求单独发给 5e 的规划者；本节保留公式，供两侧互相核对（§2.5 原则 D 的 D4）。

#### 本 PR 对槽位的验证义务（v6 把两件不同的事混成一条测试）

> v6 的 T-SEAM-01 声称「断言三个槽位存在且为空，且顺序正确」。**注释对普通 Rust 测试不可见**，而**词法顺序也不证明运行时调用顺序**——尤其跨六个分处不同文件的 facade 方法。那是一条源码门禁冒充行为测试。

拆成两件：

| 性质         | 落点                                           | 断言                                                                                                                           |
| ------------ | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| **行为**     | §6 T-SEAM-01 / T-SEAM-02 / T-SEAM-03 / T-SD-07 | 六个 facade 方法**各自**遍历 S1；三个 reconcile 入口**各自**在成功时遍历 S2、失败时遍历 S4；`Shutdown` 的 reply 在后端动作之后 |
| **源码位置** | §8 G-SEAM-01…04（`rg` 门禁）                   | 四个 `SEAM-5E-S1/S2/S3/S4` 标记各恰好一处，且在**约定的函数内**、与相邻锚点行号序正确                                          |

##### v7 的门禁只能证明**词法位置**，不能证明标记在被走到的路径上

> codex 点名：一个嵌在条件分支里、或落在两个锚点之间某段**不可达代码**里的标记，**照样通过 `rg` 行号序门禁**，而它根本不在被遍历的路径上。这是「门禁看起来覆盖了、其实没有」的经典形状。

**采纳，代价很小：四个槽位各放一个 `#[cfg(test)]` 哨兵调用**，与标记注释同处一行区块：

```rust
// ── SEAM-5E-S1（PR-5e 填；本 PR 为空）──
#[cfg(test)]
crate::client::test_support::seam_visited(Seam::S1);
// ───────────────────────────────────────
```

- `seam_visited` 是 `#[cfg(test)]` 的、往一个 thread-local / 测试注册表里记一笔的空操作，**release 构建里不存在**（符合 CLAUDE.md §13「mock-only API 放 `#[cfg(test)]`」）。
- 行为测试因此**真的能观察到「这一次执行走过了 S1」**，而不是只能观察到它周围两行的相对顺序。**把标记挪进死代码，测试立刻红。**
- 这同时补上了 v7 的一个空白：本 PR 槽位为空，v7 没有任何办法验证「空槽被走到」。哨兵让空槽也可验。

**门禁证明位置，哨兵 + 测试证明遍历，`rg` 计数证明唯一性；三者各管一段，都不能替代另一段。** 这样划分之后，「PR-5e 是填槽而不是重新设计」这句话的凭据是**三者的合取**，不是一条名不副实的测试。

### 4.8 修 Service→Normal 缺口

今天 `request.rs:82-85` 提前返回导致 `classify(true, ..)` 硬编码，**用户关闭服务模式后 reconcile 什么都不做**。改法：删掉提前返回，把真值送进 `classify`。`classify` 本身**不改**（`core/clash/core.rs:30-36` 已正确）。

### 4.9 步骤顺序：先建后删，**不是双轨并行**

> **两个生产者同时写同一状态而无定序，比一个更糟。** 单生产者的错误是确定性的；双生产者的错误是竞态的。

- **S-a**：建探针 + 修 4.8 缺口 + 接上九处调用点，**同一步停掉轮询的 reconcile 派发**；
- **S-b**：删轮询线程与三个 statics、`RunType::default()`（D2）、`core/service/mod.rs::init_service`。

---

## 5. 定序保证表

> **表与散文双向对齐。措辞不得强于其机制。**
>
> ### 穷尽方法 —— **写成可重跑的步骤，而不是一句「已穷尽」**
>
> 「本表已完整」这个断言**连续三轮被证伪**（每轮都有散文保证缺行）。所以这里写的是**产生它的程序**，审查者可以照着重跑并与结果比对：
>
> **第一遍（散文 → 表，抓漏行）**：在 §4 全文检索保证性句式的标记词——`rg -n '之前|之后|一定|必然|不会|不再|恒|只在|唯一' docs/superpowers/plans/2026-08-03-pr5d-run-mode.md`——对每一处命中判断它是否构成「X 之后 Y 一定已发生」或「X 不会发生」；是则必须在本表有对应行。
>
> **第二遍（表 → 散文，抓过强措辞）**：本表每行的**「正文出处」列**必须指到 §4 的具体小节；打开该节，逐字比对**表里的措辞是否强于该节给出的机制**。本轮这一遍抓到一条：§4.6 的关停保证在 drain 超时后**并不成立**，因此该行措辞已改为条件式（并非「关停不会越过」，而是「关停**的等待**有界，且 drain 在预算内完成时不会越过」）。
>
> **第三遍**：对每一行问「**这条保证依赖的前提，是否有另一条保证在维护它**」。§4.6.3（v6 序列里编号 ⑤ 的那一步）就是这一遍抓出来的——「drain 有界」依赖「后续的 `core_client.shutdown()` 也有界」，而 v5 把后者判给了别的 PR，前者随之落空。
>
> > **第三遍在 v6 被跑过，而且漏了。** 它只审了**正在修的那一步**（⑤）的前提，没有回头审它的**前驱**（②的 `rebuild.shutdown()`），于是 v6 给序列末尾装了一个**永远执行不到**的超时。**这不是执行不认真，是程序本身的范围定错了。**
>
> **第四遍（v7 新增，针对上面这次漏检）**：凡是**多步序列**上的有界性/定序保证，**逐步遍历整条序列**——对第 1…N 步逐一问「这一步自身有界吗？它把控制权交给谁？」——而不是只查被修的那一步。检索：`rg -n '①|②|③|④|⑤|⑥' docs/superpowers/plans/2026-08-03-pr5d-run-mode.md`，对每个序列块整块过一遍。v7 用它抓到了 ②。
>
> 四遍都跑完才算齐；**跑过的检索式留在上面，供复核**。

| 断言                                                                                                   | **靠什么构造保证**                                                                                                      | 正文出处      | 测试                                                   |
| ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- | ------------- | ------------------------------------------------------ |
| 两次控制动作的模式结论不交错                                                                           | `OperationGate` FIFO + 三步同守卫                                                                                       | §4.2          | T-MODE-03                                              |
| 控制动作 → probe → reconcile 三步不可拆                                                                | `reconcile_with(&guard)` **无 `IpcState` 参数**；`rg` 门禁钉死探针三处                                                  | §4.5          | T-MODE-03                                              |
| bootstrap 的守卫外探针安全                                                                             | 执行于 `CoreClient::new` **之前**，actor 尚不存在                                                                       | §4.5          | T-PROBE-02                                             |
| **探针必然在有限时间内返回**                                                                           | `OsServiceProbe` 内部 `timeout` + runner 层 `kill_on_drop`                                                              | §4.1          | T-PROBE-06                                             |
| **就绪等待总耗时不超过 `READY_BUDGET`**                                                                | per-probe 预算取 `remaining.min(PER_PROBE_BUDGET)`                                                                      | §4.3          | T-MODE-05                                              |
| **警告覆盖面不小于基线**                                                                               | `daemon_status` 保留 `ServiceStatus`，条件逐字复现合取式                                                                | §4.3          | T-PROBE-03/05                                          |
| **bootstrap 的探针诊断有归属**                                                                         | bootstrap 调用同一个 `report_probe_diagnostics`                                                                         | §4.3          | T-PROBE-07                                             |
| **控制失败时仍然 reconcile，且控制错误优先返回**                                                       | §4.4 表的源码顺序（reconcile 在前、`control?` 在后），与基线 F61 同形                                                   | §4.4          | T-CTL-01…04                                            |
| **控制失败时不进入就绪等待**                                                                           | 就绪等待外层的 `result.is_ok()` 条件                                                                                    | §4.4          | T-CTL-04                                               |
| **关停开始后不再有新的\*\*控制序列\*\*（#2–#7）进入**（**#8/#9 不受准入约束，见 R-C2-5**）             | `begin_shutdown()` 置 `entrants_closed`（**一经置位永不清除**）+ `enter()` 的**双重**状态检查                           | §4.6.2/§4.6.4 | T-SD-03、**T-SD-10**                                   |
| **drain 在预算内完成时，关停不会越过在飞控制序列**（**条件式——预算耗尽则不保证，见 R-C2-1**）          | ①关准入 → ③有界 drain。**⑤ 的持 permit 不参与本保证**——见 §4.6.2「⑤ 到底买到了什么」                                    | §4.6.2        | T-SD-01/02                                             |
| **`shutdown()` 整条序列的等待有界**（不只是最后一步）                                                  | ②③④ 三步各自有预算，复合上界 = `REBUILD_DRAIN_BUDGET + QUIESCE_BUDGET + ACTOR_STOP_BUDGET`                              | §4.6.2        | **T-SD-06**（在飞 rebuild）+ **T-SD-05**（挂死处理器） |
| **重复 `shutdown()` 不重复执行拆除，且 follower 一定被唤醒**                                           | leader/follower 选举 + `finish()` / `Drop` 双路完成信号，**不是 `AtomicBool`**                                          | §4.6.4        | T-SD-04、**T-SD-08**                                   |
| **leader 被取消不会让拆除被永久跳过**（退回 `NotStarted` + 发 `shutdown_abandoned` + 受控重试）        | `entrants_closed` 与 `teardown` **两维分离**；`Drop` 未经 `finish` 即判「放弃」                                         | §4.6.4        | **T-SD-08**、**T-SD-10**                               |
| **`Teardown::Done` 仅表示「拆除跑完了」**，不表示清理成功                                              | 三种结局各有独立可观测信号：`Done` / `shutdown_actor_stop_timeout` / `shutdown_abandoned`                               | §4.6.3/§4.6.4 | T-SD-05、T-SD-08                                       |
| **`core_client.shutdown()` 的等待有界**（**不是「关停一定完成」，更不是「清理一定发生」**，见 R-C2-4） | `call(Shutdown, Some(ACTOR_STOP_BUDGET))` 并按 `CallResult` 分支；**不发 `stop(None)`**                                 | §4.6.3        | **T-SD-05**、**T-SD-09**                               |
| **退出路径上没有任何东西发送 `stop(None)`**，故排队的 `Shutdown` 不被优先级停止端口抢占                | **已建立的九条事实链**：`App::run` 永不返回 + `process::exit` 跳过析构 ⇒ `CoreClientInner::drop` 两条退出路径上都不触发 | §4.6.3        | **T-SD-09** + `G-NO-FORCED-STOP`                       |
| **S1 在守卫之后、`check_open()` 之前，且六个 facade 方法全部遍历它**                                   | `run_control_sequence` 唯一实现 + 标记位置 + **`#[cfg(test)]` 哨兵**                                                    | §4.7          | **T-SEAM-01**、G-SEAM-01                               |
| **S2 只在收敛成功尾部，且三个 reconcile 入口全部遍历它**（**§4.4 第 2/4/6 行到不了**，见 R-C2-7）      | `apply_mode` 私有且唯一 + 标记位于 `run()` 成功之后 + 哨兵                                                              | §4.7          | **T-SEAM-02**、G-SEAM-02                               |
| **S3 在 actor 的 `Shutdown` 臂内、后端动作与 reply 之前**                                              | `Shutdown` 臂中 reply 位于 `backend.shutdown().await` 之后 + 标记位置 + 哨兵                                            | §4.7          | **T-SD-07**、G-SEAM-03                                 |
| **S4 恰好覆盖 §4.4 第 2/4/6 行**（S2 的补集，两者合起来覆盖六行）                                      | `apply_mode` 的单一 `match`：成功尾 = S2，失败尾 = S4 + 哨兵                                                            | §4.7          | **T-SEAM-03**、G-SEAM-04                               |
| **四个槽位都不需要自取守卫**                                                                           | S1/S2/S4 在守卫内，S3 在处理器内（actor 内自取守卫是构造性死锁）                                                        | §4.7          | 结构核对（§9）                                         |
| **同时取准入与门的路径一律「先准入、后门」**                                                           | `admission.enter(` **恒一处**（在 `run_control_sequence` 内）⇒ 第二个同时取两者的函数不可构造                           | §4.6.1        | **G-LOCK-01**（§8）                                    |
| 任何时刻只有一个模式生产者                                                                             | S-a 同步停掉轮询派发                                                                                                    | §4.9          | `rg` 判据（§9）                                        |

---

## 6. 测试矩阵

> **第三列**：删掉那行生产代码，这条测试真的会红吗？**填不出第三列的测试不进矩阵。**
>
> **两类空转陷阱**（前几轮各踩一次）：①状态有**多个**写入点，删其一另一仍生效；②**mock 打在被测机制之上**，删掉机制对 mock 无影响——**接缝必须低于被测机制**。

| ID             | 断言                                                                                                                                                                                                                                                                                  | **删掉哪行会让它红**                                                                                                                                                                                                                                                                                                                           |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-PROBE-01     | 兼容门 fail-closed：daemon 在跑但不放行 → 探针 `Disconnected`                                                                                                                                                                                                                         | 探针里调 `target_ipc_state()` 那行                                                                                                                                                                                                                                                                                                             |
| T-PROBE-02     | bootstrap 用探针真值而非 `Disconnected` 默认（修 F35）                                                                                                                                                                                                                                | `client/mod.rs:303` 的 `probe().await`                                                                                                                                                                                                                                                                                                         |
| T-PROBE-03     | **Running + `Unparsable` 也告警**（不只 `Incompatible`）                                                                                                                                                                                                                              | 警告条件里的 `!compat.allows_service_backend()`（改成 `matches!(Incompatible)` 即红）                                                                                                                                                                                                                                                          |
| T-PROBE-04     | 探针失败发出 `service_probe_failed` 降级                                                                                                                                                                                                                                              | 处理 `error` 的 `degradation.publish(..)` 行                                                                                                                                                                                                                                                                                                   |
| T-PROBE-05     | **Running + `Unknown` 告警；Stopped + `Unknown` 不告警**                                                                                                                                                                                                                              | `ProbeOutcome.daemon_status` 字段本身（去掉它两种 `Unknown` 分不开，必红）                                                                                                                                                                                                                                                                     |
| **T-PROBE-06** | **注入永不返回的 `MockServiceStatusRunner`**，`OsServiceProbe::probe_within` 仍在预算内返回                                                                                                                                                                                           | `OsServiceProbe` 里的 `tokio::time::timeout(..)`。**接缝在 runner 层**——用 `MockServiceProbe` 会绕过被测 timeout，那是空转                                                                                                                                                                                                                     |
| **T-PROBE-07** | bootstrap 探针失败/不兼容时**也**产出诊断                                                                                                                                                                                                                                             | bootstrap 处的 `report_probe_diagnostics(..)` 调用行                                                                                                                                                                                                                                                                                           |
| T-MODE-01      | 关闭 `enable_service_mode` → 得 `Normal` 并 `set_backend`                                                                                                                                                                                                                             | `request.rs:82-85` 删提前返回后送真值那行                                                                                                                                                                                                                                                                                                      |
| T-MODE-02      | 六个控制动作后**各自**触发 probe+reconcile——逐条独立断言，**断言「至少一次」**                                                                                                                                                                                                        | `run_control_sequence` 里的 `reconcile_with(&guard)`（六条同时红——这是**一处实现**的必然结果，不掩盖）                                                                                                                                                                                                                                         |
| T-MODE-03      | start→stop 序列下终态 `Normal`，晚到 probe 不翻转                                                                                                                                                                                                                                     | `reconcile_with` 的 `guard` 参数                                                                                                                                                                                                                                                                                                               |
| T-MODE-04      | 有界等待成功路径：脚本 runner 第 N 次兼容 → `Service`，无降级                                                                                                                                                                                                                         | `await_service_ready` 循环体                                                                                                                                                                                                                                                                                                                   |
| **T-MODE-05**  | **挂死 runner**：`await_service_ready` 在 `READY_BUDGET` 内返回 TimedOut（**不是** `READY_BUDGET + PER_PROBE_BUDGET`）                                                                                                                                                                | `remaining.min(PER_PROBE_BUDGET)` 里的 `remaining.min(..)`（去掉即超预算，断言时限即红）                                                                                                                                                                                                                                                       |
| T-CTL-01       | 控制 `Err` + reconcile `Ok` → 返回**控制的** `Err`，且 reconcile **确实跑过**                                                                                                                                                                                                         | `reconcile_with` 调用位于 `control?` **之前**（改成早退即红）                                                                                                                                                                                                                                                                                  |
| T-CTL-02       | 控制 `Err` + reconcile `Err` → 返回**控制的** `Err`，reconcile 失败进降级                                                                                                                                                                                                             | 错误优先级那行                                                                                                                                                                                                                                                                                                                                 |
| T-CTL-03       | 控制 `Ok` + 就绪超时 + `force_local_with` **失败** → 返回 `Err` + 两条降级                                                                                                                                                                                                            | §4.4 第 4 行的失败分支                                                                                                                                                                                                                                                                                                                         |
| T-CTL-04       | 控制 `Err`（update）→ **跳过**就绪等待（断言 `await_service_ready` 零调用）                                                                                                                                                                                                           | 就绪等待外层的 `result.is_ok()` 条件                                                                                                                                                                                                                                                                                                           |
| **T-GATE-01**  | **`Shutdown` 把等待中的取门请求全部以 `ShuttingDown` 排空**——测试**直接持有 `OperationGate`**、观察 waiter 的 reply                                                                                                                                                                   | **`gate.rs:57-59` 的 `waiters.drain(..)` 循环**。**不能走 actor 集成测试**：actor state 析构会 drop reply port，等待者照样收到错误，删掉 drain 也不会红（审查者点名）                                                                                                                                                                          |
| **T-SD-01**    | `Shutdown` 落在守卫取得之后、外部命令之前 → **外部命令不被调用**                                                                                                                                                                                                                      | 外部命令前的 `admission.check_open()?`                                                                                                                                                                                                                                                                                                         |
| **T-SD-02**    | 控制序列卡在外部命令 → `shutdown` 在 `QUIESCE_BUDGET` 内返回 + `shutdown_quiesce_timeout` 降级                                                                                                                                                                                        | `timeout(QUIESCE_BUDGET, ..)`（改成裸 await 即挂死）                                                                                                                                                                                                                                                                                           |
| **T-SD-03**    | 在 `enter()` 的 acquire **之后**、返回之前发生关停 → 该次 `enter()` 返回 `Err(ShuttingDown)`                                                                                                                                                                                          | `enter()` 的**第二次**状态检查（删掉即放行，必红）                                                                                                                                                                                                                                                                                             |
| **T-SD-04**    | 并发两次 `shutdown()` → 拆除**恰好执行一次**，两个调用都在其**完成之后**才返回。**必须有显式 barrier**：先确认 follower 已取得 `Follower(n)` 且已注册 `notified()`，再放行 leader 完成                                                                                                | 选举 + 完成时的 `notify_waiters()`；**改成 `AtomicBool` 即红**。**没有 barrier 就是空转（陷阱①）**：follower 若首次轮询已在 `Done` 之后，删掉 `notify_waiters()` 测试照绿——它走的是「后复查」那条腿，与被测的唤醒无关                                                                                                                          |
| **T-SD-05**    | **actor 处理器卡在一次后端 await 里** → `shutdown()` 仍在 `ACTOR_STOP_BUDGET` 内返回，发出 `shutdown_actor_stop_timeout` 降级，且**断言此刻后端 `shutdown()` 尚未被调用**（清理未发生，如实观察）                                                                                     | `call(..)` 的 `Some(ACTOR_STOP_BUDGET)` **以及** `Ok(CallResult::Timeout)` 那条匹配臂（**任一都会红**：前者改回 `None` 即永久挂起；后者退回 v6 的 `Ok(_) => {}` 则超时被吞、降级不发）。**接缝在后端层**——mock 掉 `CoreClient::shutdown` 会绕过被测超时，那是空转                                                                              |
| **T-SD-06**    | **在飞的 rebuild 阻塞在 `RebuildCoordinator::shutdown` 之下** → `shutdown()` 仍在复合上界内返回 + `shutdown_rebuild_drain_timeout` 降级                                                                                                                                               | ② 的 `timeout(REBUILD_DRAIN_BUDGET, ..)`（改成裸 await 即永久挂起）。**接缝在 worker 闭包**（`start_worker` 的 `rebuild` 参数），**严格低于**被测的 `RebuildCoordinator::shutdown`——mock 掉后者是空转。**必须先用显式 ack 等到闭包已进入**，否则 worker 会在 `COALESCE_WINDOW` 里就被 shutdown 信号打断，rebuild 根本不在飞，测试空转（陷阱①） |
| **T-SD-07**    | `Shutdown` 的 reply 在 `backend.shutdown()` 被观察到**之后**才发出（**S3 的前置条件**）                                                                                                                                                                                               | `reply.send(())`（`core/actor/mod.rs:613`）的位置——挪到 `backend.shutdown().await` 之前即红                                                                                                                                                                                                                                                    |
| **T-SD-08**    | **leader 的 shutdown future 在拆除中途被丢弃**（外层 `timeout` 取消它）→ ①`teardown` 退回 `NotStarted` 而**不是** `Done`；②发出 `shutdown_abandoned` 降级；③follower 被唤醒后**接替成为新 leader 并真的执行了拆除**；④`entrants_closed` 全程为 `true`。**同 T-SD-04 的 barrier 要求** | `ShutdownCompletion::drop` 里区分 `finished` 的那条分支。**v7 的 T-SD-08 断言的是「状态到达 `Closed`」——那正是缺陷本身，本条是对它的定向推翻**：若退回 v7 的无条件 `Closed`，③ 会红（拆除被永久跳过）、② 也会红（一条降级都不发）                                                                                                              |
| **T-SD-09**    | **处理器先阻塞、`shutdown()` 超时返回、随后解除阻塞** → 排队的 `Shutdown` 仍被处理，后端 `shutdown()` **最终被观察到**                                                                                                                                                                | **反向契约**（「不做某事」型，见 §7）：加回 `actor_ref.stop(None)` 即红——停止端口优先级高于普通消息端口，actor 会在 `Shutdown` 之前终止。**配套 `rg` 判据见 §8 `G-NO-FORCED-STOP`**                                                                                                                                                            |
| **T-SD-10**    | **拆除被放弃期间准入仍然关闭**：leader 取消后、接替者开始前，一次 `enter()` 仍返回 `Err(ShuttingDown)`                                                                                                                                                                                | `begin_shutdown()` 里 `state.entrants_closed = true` 那行**位于 match 之外**（挪进 `NotStarted` 分支后，接替路径会把它重置，测试即红）。**这条钉的是「两维分离」的下半截**——上半截由 T-SD-08 钉                                                                                                                                                |
| **T-SEAM-03**  | **§4.4 第 2/4/6 行（收敛失败）遍历 S4 而非 S2；第 1/3/5 行相反**——六行逐条独立断言，用 `#[cfg(test)]` 哨兵观察                                                                                                                                                                        | `apply_mode` 的 `match` 里 S4 所在的 `Err` 臂哨兵（删掉则失败三行观察不到任何槽位，必红）。**这条测试就是「S4 恰为 S2 补集」那句话的机制**                                                                                                                                                                                                     |
| **T-SEAM-01**  | **六个** facade 控制方法**各自**遍历同一条 `run_control_sequence`，且观察序为 `enter → begin_operation → check_open → 控制动作 → reconcile_with`（**六条独立断言**）                                                                                                                  | `run_control_sequence` 里的 `admission.check_open()?`（删掉则「控制动作前不再查准入」这条序被破坏）。**六个方法各写一条**——这是 §3.2 例外条款的承担者之一                                                                                                                                                                                      |
| **T-SEAM-02**  | **三个** reconcile 入口（`reconcile` / `reconcile_with` / `force_local_with`）**各自**在成功时都走到同一个 `apply_mode` 尾部；失败时**都不到达**                                                                                                                                      | `apply_mode` 里 `self.core.run(..)` 的 `?`（去掉 `?` 改为忽略错误，则失败路径也会到达尾部，失败侧断言即红）                                                                                                                                                                                                                                    |

**回归契约**：区分**存活测试被迫修改**（不允许，停下核查）与**被删模块自带单测随属主消失**（预期）。

**已知必改**：`client/core.rs:1207-1214` → 断言注入 mode **并改名**；`core/service/ipc.rs:140-187` 的两条 `target_ipc_state` 单测随文件重整迁到 `core/service/probe.rs`，断言不变。

> **关于 T-SD-09 的第三列是「加回一行会红」而不是「删掉一行会红」**：它验的是一条**否定契约**（§7 口诀里那一类），本来就没有对应的正向生产代码行。矩阵的规矩因此在这一条上**如实破例**，并用 §8 的 `rg` 判据补上「那行确实不存在」的一半。**破例写明，好过把它伪装成正向断言。**

> ### T-PROBE-06 **不覆盖** `.kill_on_drop(true)` —— 如实划清
>
> T-PROBE-06 注入的是 `MockServiceStatusRunner`，它**根本不 spawn 子进程**，所以「`timeout` 丢弃 future 之后子进程是否被杀」这件事**它一个字都没验**。把 `OsServiceStatusRunner` 里的 `.kill_on_drop(true)` 删掉，T-PROBE-06 **照绿**，而挂死的 `status --json` 子进程会一直泄漏。
>
> **不为它补测试**，理由说得出：要真验它得 spawn 一个真实的长命子进程、拿到 pid、在 timeout 之后跨平台断言该 pid 已消失——三平台的进程回收语义不同（Windows 无 `SIGCHLD`、macOS/Linux 需处理僵尸），成本与收益不成比例。
>
> **所以它在 §7 里被登记为「由代码审查保证、未被测试覆盖」**，并在 §9 出现一行。**「这条没测」必须是被记录的决定，不是矩阵里的一个空白。**

---

## 7. 契约归属

> **口诀**：签名只能保证**「值到得了这里」**（及其对偶「到不了这里」）与**「类型在此平台不存在」**；凡「**不会去做某事**」一律靠测试 / 门禁 / `rg`。
>
> **同族第二条**（承自 v4，C3 那边是主战场，这里仍适用）：**返回值的错误通道只报告调用的结果，不报告副作用的缺席。**

> **第三条（v8 新增，来自 leader 的发现）：一条经三重交叉引用指向「某个门禁」的不变量，如果那个门禁在 §8 里不存在，它就等于不存在。** v7 的锁序不变式正是这样——§5 指向「§7 `rg` 判据」，§7 说「由 `rg` 门禁保证」，§8 的门禁清单里**没有这一条**。**因此本表「由谁保证」列写 `rg` 门禁的每一行，都必须在 §8 有一个 `G-` 编号**；下表第三列已逐行给出编号，没有编号的行不许写「门禁」。

| 契约                                            | 由谁保证                                    | 为什么可验证（**`rg` 类必须给出 §8 的 G- 编号**）                                                                    |
| ----------------------------------------------- | ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| 调用方无法把陈旧探针结果喂给 reconcile          | **签名**                                    | `reconcile`/`reconcile_with` **没有 `IpcState` 参数**                                                                |
| **探针必然有限时间返回**                        | **实现内部的 `timeout` + 低层 runner 接缝** | 单点可验（T-PROBE-06）；**不是**「每个调用方记得包一层」那种不可强制的契约                                           |
| **挂死子进程会被回收**（`.kill_on_drop(true)`） | **代码审查——\*\*未被任何测试覆盖\*\***      | **如实登记的空白**：T-PROBE-06 的 mock runner 不 spawn 子进程，删掉 `kill_on_drop` 它照绿。不补测试的理由见 §6 末    |
| 任何探针都不在守卫外开始                        | **`rg` 门禁 `G-PROBE-SITES`**               | `rg -n '\.probe(_within)?\('` 恒三处且位置固定                                                                       |
| `force_local_with` 只在超时分支用               | **`rg` 门禁 `G-FORCE-LOCAL`**               | 恰好一处调用点                                                                                                       |
| **模块内不另开第二条收敛路径**                  | **`rg` 门禁 `G-APPLY-MODE` + 测试**         | `apply_mode` 私有；`rg -n 'set_backend\('` 在 `request.rs` 内恒一处（T-SEAM-02/03 覆盖遍历）                         |
| **六个 facade 控制方法不各自重写序列**          | **`rg` 门禁 `G-LOCK-01` + 测试**            | `rg -n 'admission\.enter\('` 恒一处（在 `run_control_sequence` 内）；T-SEAM-01 覆盖遍历                              |
| 顺序类契约                                      | **测试**                                    | 控制流性质，类型系统表达不了                                                                                         |
| **关停后不再有新\*\*控制序列\*\*（#2–#7）进入** | **状态机转移 + 双重检查 + 测试**            | T-SD-03、T-SD-10；**已进入的序列见 R-C2-1、未受准入约束的 #8/#9 见 R-C2-5，两者都不谎称关闭**                        |
| **不发 `stop(None)` 抢占已排队的 `Shutdown`**   | **`rg` 门禁 `G-NO-FORCED-STOP` + 测试**     | `rg -n 'actor_ref\.stop\('` **恰好一处**，且在 `CoreClientInner::drop`（`client/core.rs:379`）内；T-SD-09 从行为侧验 |
| **同时取准入与门时先准入后门**                  | **`rg` 门禁 `G-LOCK-01`**                   | **同一条门禁两用**：`admission.enter(` 恒一处 ⇒ 第二个「同时取两者」的函数在结构上不可构造，反序无从产生             |
| `get_ipc_state` / statics 归零                  | **`rg` 门禁 `G-STATICS-ZERO`**              | 删除类不变量                                                                                                         |

---

## 8. 门禁与残留

**门禁 —— 本节是门禁的\*\*唯一权威清单\*\*，§5/§7/§9 只许引用这里的 `G-` 编号。**

> **v7 在这里塌了一次**：锁序不变式被 §5→§7→「`rg` 门禁」三重转引，而本节的 1–5 条里**根本没有它**。**一条经三次交叉引用指向空处的不变量，正是本计划反复在抓的那个形状。** v8 因此把散在 §7/§9 正文里的 `rg` 判据全部收进本节并编号；**没有编号的判据不许在别处被称作「门禁」。**

| 编号                 | 判据                                                                                                                                                                                                                                        |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **G-DIFF-BASE**      | 「diff 应为空」形态的判据，只要跑在中间提交之后，必须与基线比：`git diff --exit-code <base>..HEAD -- <path>`                                                                                                                                |
| **G-LEDGER**         | ledger 三步顺序：report 核对 → `--write-snapshot` → gate 比对                                                                                                                                                                               |
| **G-MODULE-GONE**    | 删模块要有「模块不存在」断言（`core/service/mod.rs::init_service`、`ipc.rs` 的轮询部分）                                                                                                                                                    |
| **G-SEAM-01**        | `rg -n 'SEAM-5E-S1'` **恰好一处**，在 `client/mod.rs` 的 `run_control_sequence` 内，行号介于该函数的 `begin_operation(` 行与 `admission.check_open(` 行**之间**                                                                             |
| **G-SEAM-02**        | `rg -n 'SEAM-5E-S2'` **恰好一处**，在 `core/actor/request.rs` 的 `apply_mode` 内，位于 `match` 的 `Ok` 臂                                                                                                                                   |
| **G-SEAM-03**        | `rg -n 'SEAM-5E-S3'` **恰好一处**，在 `core/actor/mod.rs` 的 `Shutdown` 臂内，行号在 `backend.shutdown().await` 与 `reply.send(` 两行**之前**                                                                                               |
| **G-SEAM-04**        | `rg -n 'SEAM-5E-S4'` **恰好一处**，在 `core/actor/request.rs` 的 `apply_mode` 内，位于**同一个 `match` 的 `Err` 臂**                                                                                                                        |
| **G-SEAM-SENTINEL**  | `rg -n 'seam_visited\('` **恰好四处**，与四个 `SEAM-5E-S*` 标记一一相邻，且全部在 `#[cfg(test)]` 之下（§4.7 末）                                                                                                                            |
| **G-NO-FORCED-STOP** | `rg -n 'actor_ref\.stop\('` **恰好一处**，落在 `client/core.rs` 的 `impl Drop for CoreClientInner` 内（今天在 `:379`）。**不覆盖** actor 自身 `Shutdown` 臂末尾的 `myself.stop(None)`（`mod.rs:614`，接收者不同，正常终止路径，本 PR 不动） |
| **G-LOCK-01**        | `rg -n 'admission\.enter\('` **恰好一处**，在 `run_control_sequence` 内，且行号**早于**同函数的 `begin_operation(` 行 —— 论证见下                                                                                                           |
| **G-PROBE-SITES**    | `rg -n '\.probe(_within)?\('` **恰好三处**：`reconcile_with` 内、`await_service_ready` 内、bootstrap                                                                                                                                        |
| **G-FORCE-LOCAL**    | `rg -n 'force_local_with\('` 的调用点**恰好一处**（§4.3 就绪超时分支）                                                                                                                                                                      |
| **G-APPLY-MODE**     | `rg -n 'set_backend\('` 在 `core/actor/request.rs` 内**恰好一处**（即 `apply_mode_inner`），确保没有第二条收敛路径                                                                                                                          |
| **G-STATICS-ZERO**   | `rg 'IPC_STATE\|KILL_FLAG\|HEALTH_CHECK_RUNNING\|spawn_health_check\|get_ipc_state\|RunType::default'` **为 0**                                                                                                                             |
| **G-SHUTDOWN-BOUND** | `rg -n 'call\(CoreActorMessage::Shutdown, None\)'` 为 **0**；且 `rg -n 'rebuild\.shutdown\(\)'` 的调用点**不是裸 await**（必须被 `timeout(REBUILD_DRAIN_BUDGET, ..)` 包住）                                                                 |

**关于 `G-LOCK-01` —— 一条门禁同时承担两件事，论证要写出来：**

> 锁序不变式是「任何同时取 `ControlAdmission` 与 `OperationGate` 的路径，必须先准入后门」。**直接检查「所有同时取两者的函数里两行的顺序」需要跨函数分析，`rg` 做不到。** 但本 PR 的结构提供了一条更强也更容易验的替代命题：
>
> **`admission.enter(` 全仓恰好一处** ⇒ **不存在第二个能同时取到两者的函数** ⇒ **反序在结构上不可构造**。剩下唯一那处的顺序，由同一条门禁的行号比较直接钉死。
>
> **这不是弱化，是把全称命题化归成一个可被 `rg` 判定的单点命题。** 代价：若将来真需要第二个取准入的地方，本门禁会红——**那正是应该红的时候**，因为那一刻锁序才第一次成为真问题。

**bindings 预期**：`ServiceProbe` / `ProbeOutcome` / `ServiceStatusRunner` / `ControlAdmission` / `ShutdownRole` / `ShutdownDisposition` 全部 `pub(crate)`；`uninstall_service` 命令名与签名不变（已核实其 specta 导出在 `specta_export.rs:77`，facade 迁移不动签名）；不新增命令。**结论：diff 恰好为空**，判据 `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`（与 `ci.yml:306-308` 同形）。

**残留（本节是残留的唯一权威清单；§9 只引用它，不重列子集）：**

| #          | 残留                                                                                                                                                                                                                                                                                                                                                                                                                                                     | 性质                                                                                                                                                       | owner / 移除条件                                                                                                                                                                             |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **R-C2-1** | `QUIESCE_BUDGET` 超时后，被放弃的控制序列仍可能完成其外部命令                                                                                                                                                                                                                                                                                                                                                                                            | **本 PR 引入的有界窗口**；今天是**无界**同类问题且更糟（连准入都没有）                                                                                     | 移除条件 = 外部命令可取消（需 `runas`/`sudo` 侧支持，不在本仓）                                                                                                                              |
| **R-C2-2** | `check_open()` 与外部命令 spawn 之间的 TOCTOU                                                                                                                                                                                                                                                                                                                                                                                                            | 同上                                                                                                                                                       | 与 R-C2-1 同条件                                                                                                                                                                             |
| **R-C2-3** | update 有界等待超时后 daemon 可能稍后才就绪，而已收敛 Local                                                                                                                                                                                                                                                                                                                                                                                              | **本 PR 引入的取舍**（今天靠 5 s 轮询最终纠正）                                                                                                            | 下次任一服务控制动作会重新 probe 纠正；**不加后台重试**——那会把第二个模式生产者请回来                                                                                                        |
| **R-C2-4** | **`ACTOR_STOP_BUDGET` 超时后，后端清理是否执行\*\*未知\*\***：`backend.shutdown()`（`core/actor/mod.rs:609`，全仓唯一调用点）可能没跑；Service 模式下 daemon 是独立进程，**进程退出不会带走它管的核**。5e 落地后被跳过的还包括 DNS 恢复。**已在一条已发布代码路径上实际成立**：`restart_application`（`utils/help.rs:279-296`）先 `cleanup_processes`（`:280`）、再 `spawn` 后继进程（`:290`）、再 `process::exit`（`:295`）——超时时旧核可能与新实例并存 | **既有**（ractor 不抢占执行中的 `handle()`，本 PR 未引入也无法消除）。本 PR 把**等待**从无界改为有界，并把结果**如实报成降级关停**而非干净关停             | 「进程随后销毁」已由事实链 6/7/9 确证，但**销毁 ≠ 清理已发生**，且 `restart_application` 上还有后继进程紧接着启动。移除条件 = 后端 await 各自有界（属 PR-5a 设计面）或 ractor 提供处理器抢占 |
| **R-C2-8** | **leader 被取消时拆除被放弃**：发 `shutdown_abandoned` 降级，准入保持关闭，由下一个 `shutdown()` 调用者接替；**若此后再无人调用，拆除就不会发生**                                                                                                                                                                                                                                                                                                        | **本 PR 引入的显式取舍**（v7 是**静默**跳过且不可重试，更糟）。生产退出路径上 `cleanup_processes` 的 `block_on` 不会被取消，因此这是健壮性残留而非常规路径 | 移除条件 = 把拆除放进一个不随调用者取消而消失的 owned task。**本 PR 不做**：需要 `'static` 克隆与 detached 任务，而取消在生产路径上不发生（`help.rs:261`），不值这个复杂度                   |
| **R-C2-5** | **#8/#9（`reconcile()` 自取守卫）不受 `ControlAdmission` 约束**：关停开始后仍可能有一次模式收敛与拆除并发                                                                                                                                                                                                                                                                                                                                                | **既有**（今天连准入都不存在）。**不发外部 OS 命令**，因此不构成 §4.6 要防的那类危害                                                                       | 移除条件 = 把 #8/#9 也纳入准入（需给 `reconcile()` 一个持准入的变体）。**本 PR 不做**——收益仅为提前几毫秒返回 `ShuttingDown`，不值一次接口分裂                                               |
| **R-C2-6** | **`REBUILD_DRAIN_BUDGET` 超时后，在飞的 rebuild 被放弃等待（但\*\*不被取消\*\*）**：其后续 actor 调用会返回 `ShuttingDown`，可能留下只应用了一半的运行时配置                                                                                                                                                                                                                                                                                             | **本 PR 引入的有界窗口**；v6/今天是**无界等待**（不会半途而废，但会永久挂起）                                                                              | 移除条件 = rebuild 支持安全的中途取消（`client/rebuild.rs:215` 的注释明说今天不具备），或 apply 变为原子                                                                                     |
| **R-C2-7** | **§4.4 第 2/4/6 行（收敛失败）到不了 S2 槽位**：5e 填槽后，这三行不会重施加 DNS；若此时 TUN 仍被期望开启且核实际在跑，覆写就缺席                                                                                                                                                                                                                                                                                                                         | **本 PR 冻结的契约后果**；**槽位为空期间无实际影响**                                                                                                       | **owner = PR-5e**：它需要在这三行给出可见降级而不是静默。移除条件 = 5e 为失败收敛定义一条明确处置                                                                                            |

---

## 9. Exit 判据

| 要求                                                                           | 验证                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 显式模式收敛全部走守卫                                                         | T-MODE-01/02/03；`G-PROBE-SITES`                                                                                                                                                                                                                                      |
| `reconcile` 家族无 `IpcState` 参数                                             | 签名核对（编译期即拦）                                                                                                                                                                                                                                                |
| **探针自身有界且该性质可测**                                                   | T-PROBE-06（接缝在 runner 层）                                                                                                                                                                                                                                        |
| **`.kill_on_drop(true)` \*\*未被测试覆盖\*\*，是被记录的空白**                 | §7 该行 + §6 末的理由段；**不得在 PR 描述里被说成「已验证」**                                                                                                                                                                                                         |
| **就绪等待不超预算**                                                           | T-MODE-05                                                                                                                                                                                                                                                             |
| **警告覆盖面不小于基线**                                                       | T-PROBE-03/05/07                                                                                                                                                                                                                                                      |
| 删 `pending_run_type` 设计                                                     | **no-op**（F9）                                                                                                                                                                                                                                                       |
| 删轮询线程与 statics                                                           | `G-STATICS-ZERO`                                                                                                                                                                                                                                                      |
| 删 `impl Default for RunType`                                                  | `G-STATICS-ZERO`（含 `RunType::default`）；`initial` 两个调用点都传参，`mod.rs:371` 覆盖赋值已删                                                                                                                                                                      |
| 六个入口签名一致且全在 `ServiceControlOps` 上                                  | 结构核对：六个具体函数**仍在 `core::service::control`**（满足 `design.md:333`，按 5a `:1037` 读法）；**扩到六方法须写进 PR 描述**                                                                                                                                     |
| **控制失败六种处置**                                                           | T-CTL-01…04                                                                                                                                                                                                                                                           |
| **关停静默期**                                                                 | T-SD-01…**04**、T-SD-10、T-GATE-01                                                                                                                                                                                                                                    |
| **关停\*\*整条序列\*\*的等待有界**（不等于关停一定完成，更不等于清理一定发生） | T-SD-05/**06**；`G-SHUTDOWN-BOUND`；复合上界 = `REBUILD_DRAIN_BUDGET + QUIESCE_BUDGET + ACTOR_STOP_BUDGET`                                                                                                                                                            |
| **单次飞行的三种结局各有独立信号**                                             | T-SD-04（完成）+ **T-SD-08**（放弃 → 降级 + 接替）+ T-SD-05（完成但清理未知）。**三条都要**——v7 把「放弃」并进「完成」，测试反而固化了缺陷                                                                                                                            |
| **放弃期间准入不重开**                                                         | **T-SD-10**（`entrants_closed` 置位在 match 之外）                                                                                                                                                                                                                    |
| **超时不会跳过\*\*本可执行的\*\*清理**                                         | **T-SD-09** + `G-NO-FORCED-STOP`；**并以事实链确证退出路径上无 `stop(None)`**（§4.6.3 九条，含 `tao`/`tauri` 版本锚点）                                                                                                                                               |
| **四个 5e 接缝已声明且为空**                                                   | **位置**：G-SEAM-01…04；**遍历**：G-SEAM-SENTINEL + T-SEAM-01/02/03、T-SD-07。**两组缺一不可**——门禁不证明执行序，测试不证明标记位置；哨兵是让「空槽被走到」也可验的那一环                                                                                            |
| bindings diff 为空                                                             | `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`                                                                                                                                                                                                      |
| **C3 的改动范围恰为三处四行**（不是「仅签名一处」）                            | `core/clash/core.rs` 签名加参 + 函数体 `:78` 一行 + `feat.rs:409` 解构 + `feat.rs:420` 传参（**已核实是该函数唯一调用点**）；DNS 逻辑本体零改动；`feat.rs:416-418` 迁移标记改指 **PR-5e**                                                                             |
| **smoke 2**（v1→v2 升级 + 拒绝升级 fail-closed Local）                         | 本机可跑，**须真实服务环境**；**它是 C2 的真正验收点**——迁移不完整会正好打断它**而 `rg` 门禁全绿**                                                                                                                                                                    |
| **§8 残留表的\*\*全部\*\*条目（R-C2-1…8）**逐条出现在 PR 描述里                | 文本核对——**「不修」必须是被记录的决定，不是沉默**。**§8 是唯一权威清单**；v6 在此处列过两份互相矛盾的子集（一处 1/2/4、一处 1/2/3），v7 起不再重列子集                                                                                                               |
| **对 `design.md` 的一处有意偏离**（六方法 trait）出现在 PR 描述里              | 文本核对；`design.md` **本身不得修改**                                                                                                                                                                                                                                |
| **欠 PR-5e 的预算义务**已写进 PR 描述，**且以公式形式**                        | `ACTOR_STOP_BUDGET ≥ 4×DNS_READ + DNS_WRITE + DNS_IPC + backend.shutdown() 上界 + 余量`（§4.7 S3 段逐步推导）。**并注明 5e 侧尚无对应条目**（§2.5 D5）                                                                                                                |
| **`client/mod.rs:454` 的 S11 契约注释已修正**                                  | 该行今天写「Stop the instance-owned rebuild worker and core actor, **awaiting both exits**」——`REBUILD_DRAIN_BUDGET` 使之为假。**`:457`（rebuild 先于 actor）与 `:461`（在飞 rebuild 允许跑完）不受影响**，因为放弃等待不等于取消。改注释属实施动作，本计划只声明要求 |
| **「本 PR 收窄了 PR-5a 的 S11 契约」写进 PR 描述**                             | 文本核对——与六方法 trait 那条并列。**这是对一个已合并契约的可见收窄，必须显式记账**                                                                                                                                                                                   |

> **smoke 3（macOS TUN/DNS）不在本 PR 范围** —— 随 C3 移交 PR-5e。
