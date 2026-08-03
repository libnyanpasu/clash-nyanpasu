# PR-5d 实施计划 — 运行模式探针（C2）

**日期：** 2026-08-03
**版本：** v5（**范围收窄**：C3 macOS DNS 已拆出为独立 PR，见 `2026-08-03-pr5e-macos-dns.md`）
**分支基线：** `refactor/core-manager-actor` @ **`6f1a6683d`**
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/` 下**两个文件都算数**——`task.md` 卡 C2 + `design.md` §9（`:333` Service control 段直接管着本阶段）。**只读 `task.md` 会漏掉约束**，§2.4 记了一次实际漏检
**平台：** Windows 11 / PowerShell

---

## 0. 为什么 v5 是收窄而不是又一次修补

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
2. **九处调用点**统一为 `准入 → 守卫 → 控制 → [update: 有界等待] → probe → reconcile`；
3. 修 Service→Normal 缺口；
4. **控制动作失败的处置表**（保留基线「无论成败都 reconcile」语义）；
5. **关停静默期协议**（`ControlAdmission`）；
6. **先建后删**地移除 5 s 轮询与三个 statics；
7. **D2**：`CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`（删 statics 的前置阻塞）。

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
> | 测试         | 为什么必须能替换 runner                          | 缺哪个方法就写不出来  |
> | ------------ | ------------------------------------------------ | --------------------- |
> | T-MODE-02    | 六个控制动作**各自独立断言** probe+reconcile     | `update`、`uninstall` |
> | T-MODE-04/05 | 有界等待三路，需在无真实 daemon 下让 update 返回 | `update`              |
> | T-CTL-01…04  | 控制失败四种处置，需让控制动作**按脚本失败**     | 六个全部              |
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
       └─ NyanpasuClientInner { .., probe }                     ← 新字段，紧挨 service_control（:257）
            └─ core_mode_reconciler()（:467-473）加 probe: self.inner.probe.clone()
                 └─ CoreModeReconciler { core, application, requests, probe }（request.rs:70-75）
```

`CoreModeReconciler` 是 `#[derive(Clone)]`，加 `Arc<dyn ServiceProbe>` 不破坏 Clone。测试侧沿用 `test_service_control()`（`client/mod.rs:2767`）的模式加 `test_service_probe()`。

### 4.2 九处调用点

**统一形态：**

```text
permit = admission.enter().await?              ← §4.6；已关停则 Err(ShuttingDown)
guard  = core.begin_operation().await?
  ├─ admission.check_open()?                   ← 紧贴外部命令之前再查一次（§4.6）
  ├─ result = service_control.<action>().await ← **不早退**，见 §4.4
  ├─ [仅 update 且 result.is_ok()] await_service_ready（§4.3）
  └─ reconciler.reconcile_with(&guard).await   ← **无论 result 成败都跑**（F61）
```

| #   | 位置                                                  | 今天                                        | 改为                                                                    |
| --- | ----------------------------------------------------- | ------------------------------------------- | ----------------------------------------------------------------------- |
| 1   | **bootstrap**（`client/mod.rs:303`）                  | `get_ipc_state()`（恒 `Disconnected`，F35） | `probe()` 一次——**顺带修掉 F35**。**唯一不在守卫内的探针**，理由见 §4.5 |
| 2   | install（facade `:504-510`）                          | 不 reconcile（F16）                         | 统一形态                                                                |
| 3   | start（`:512-521`）                                   | 轮询 + `reconcile(get_ipc_state())`         | 统一形态                                                                |
| 4   | restart（`:530-539`）                                 | 同上                                        | 统一形态                                                                |
| 5   | stop（`:523-528`）                                    | 同上                                        | 统一形态                                                                |
| 6   | uninstall（今在 `ipc.rs:936-937`）                    | 无                                          | **迁到 facade** + 统一形态                                              |
| 7   | update（今在 `utils/init/mod.rs:251`）                | 轮询                                        | **迁到 facade** + 统一形态 + 有界等待——**直接关系 smoke 2**             |
| 8   | `enable_service_mode` 变更后                          | 轮询 + reconcile（有 §4.7 的洞）            | `reconcile()`（自取守卫版）                                             |
| 9   | boot 的 `init_service`（`core/service/mod.rs:18-30`） | 起轮询线程 + 忙等 100 ms                    | `reconcile()`，**删忙等与整个函数**                                     |

### 4.3 诊断归属与有界等待就绪

**诊断有三个来源，但只有一个实现**（审查者点名 bootstrap 无归属）：

| 场景                                                                                                                                                                            | 接手方                                            | 动作                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `daemon_status == Some(Running) && !compat.allows_service_backend()`——**逐字复现 `ipc.rs:108` 的合取式**，覆盖 Running 下 `Unknown`/`Incompatible`/`Unparsable` 三种（F59/F62） | `report_probe_diagnostics`，**单一实现**          | `tracing::warn!(?compat, ..)`（smoke 2 要的就是这一条）                                            |
| `error.is_some()`                                                                                                                                                               | 同上                                              | `warn!` + degradation `service_probe_failed`、`retryable = true`                                   |
| **bootstrap 的那次探针**                                                                                                                                                        | **bootstrap 调同一个 `report_probe_diagnostics`** | 同上二者。degradation sink 在 bootstrap 处已构造（`client/mod.rs:280` `client_degradation`），可用 |

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

| 常量                              | 实测？ | 依据                                                                 |
| --------------------------------- | ------ | -------------------------------------------------------------------- |
| `READY_BUDGET`                    | **是** | 实测 daemon 从 `update_service()` 返回到 `status()` 报兼容的耗时上界 |
| `PER_PROBE_BUDGET`                | **是** | 实测一次正常 `control::status()` 子进程往返上界                      |
| `QUIESCE_BUDGET`（§4.6）          | **是** | 实测最慢控制动作（`update`）的正常耗时上界                           |
| `INITIAL_BACKOFF` / `MAX_BACKOFF` | 否     | 不是正确性边界，**如实标注为选定值**                                 |

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

### 4.5 reconcile 的三个签名

```rust
impl CoreModeReconciler {
    /// 自取守卫 → **在守卫内探针** → 应用。唯一的无守卫入口（#8、#9）。
    pub(crate) async fn reconcile(&self) -> anyhow::Result<()>;
    /// 已持守卫：**在守卫内探针** → 应用。控制动作（#2..#7）用这个。**没有 IpcState 参数。**
    pub(crate) async fn reconcile_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()>;
    /// 已持守卫、**不探针**、直接落 Local。**仅供 §4.3 超时分支**。
    pub(crate) async fn force_local_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()>;
}
```

**强制构造**：`reconcile`/`reconcile_with` **没有 `IpcState` 参数**——调用方在类型上就无法喂进陈旧探针结果。这是签名能给的那一类保证。

**「任何探针都不许在守卫外开始」是「不会去做某事」型契约**，落到 §7 门禁：`rg -n '\.probe(_within)?\('` 恰好三处（`reconcile_with` 内、`await_service_ready` 内、bootstrap）。

**bootstrap 是唯一守卫外探针，理由是真排除**：它在 `client/mod.rs:303`，而 `CoreClient::new` 在 `:312`——**actor 那时还不存在**，没有任何别的操作能在飞，也没有守卫可取。两行同在一个 `async move` 块，源码顺序即执行顺序。

`force_local_with` 同样上 `rg` 门禁：**恰好一处调用点**。

### 4.6 关停静默期协议

> **不把 `Shutdown` 变成守卫消息**：关停必须能在一个操作卡住时仍然生效，否则一次挂死的控制操作会让 app 关不掉。

**问题形状**：facade 控制序列在**外部**持有许可，而 `CoreClient::shutdown()` 发的是**无守卫**的 `Shutdown`（`client/core.rs:277-283`），actor 立即 `state.operation.shutdown()` 清掉活跃操作（`mod.rs:604`）。于是控制序列可能在「已经 await 过的关停」之后仍去执行外部 `start`/`install`/`restart`/`update`，而没有 actor 还能收敛它。

```rust
struct ControlAdmission {
    /// 单次飞行状态：Open / Closing(Arc<Notify>) / Closed。
    /// **不能用 AtomicBool**——它分不出「关停进行中」与「关停已完成」，
    /// 于是第二个 shutdown() 会在拆除尚未结束时就返回（审查者点名）。
    state: Mutex<AdmissionState>,
    /// 1 个许可，被整条控制序列持有（含外部命令）。
    inflight: Arc<tokio::sync::Semaphore>,
}
```

```text
enter()：
 ① 查 closed → 已关则 Err(ShuttingDown)
 ② acquire 许可（可能等待）
 ③ **再查一次 closed** → 已关则释放许可并 Err(ShuttingDown)
    ← ③是必需的：①与②之间可能发生 close（审查者点名）

NyanpasuClient::shutdown()：
 ① 单次飞行：已有关停在进行 → 等它完成后返回（不重复执行拆除）
 ② rebuild.shutdown().await                     ← 既有第一步
 ③ admission.close()                            ← 关准入
 ④ permit = timeout(QUIESCE_BUDGET, inflight.acquire())
      超时 → warn + degradation `shutdown_quiesce_timeout`
 ⑤ core_client.shutdown().await
 ⑥ **④拿到的 permit 在 ⑤ 之后才释放**
    ← 否则排在 close 之前入队的等待者会在 actor 拆除期间被唤醒（审查者点名）
```

**为什么需要与 `OperationGate` 并存的第二个机制**：`OperationGate` 只约束 **actor 消息**；`Shutdown` 本身是无守卫消息且会**主动清空**活跃许可，所以 gate 在关停面前不提供任何排他性。`ControlAdmission` 约束的是 **facade future 的存续**，包括那条 actor 完全看不见的外部 OS 命令。**作用域不同，不是冗余。**

> **本 PR 范围内不存在「actor 内部挂死」的对应风险。** v4 那条 BLOCKING（drain 超时而 actor 仍挂死）的挂死点是 **DNS 读写**，随 C3 一并移出。C2 留在 actor 内的 await 只有既有的 backend 操作，其边界属 PR-5a 既有设计，本 PR 不改。**PR-5e 必须重新处理这一条**——已写进该计划的前置。

**残留（如实记账，§8）**：R-C2-1、R-C2-2。

### 4.7 修 Service→Normal 缺口

今天 `request.rs:82-85` 提前返回导致 `classify(true, ..)` 硬编码，**用户关闭服务模式后 reconcile 什么都不做**。改法：删掉提前返回，把真值送进 `classify`。`classify` 本身**不改**（`core/clash/core.rs:30-36` 已正确）。

### 4.8 步骤顺序：先建后删，**不是双轨并行**

> **两个生产者同时写同一状态而无定序，比一个更糟。** 单生产者的错误是确定性的；双生产者的错误是竞态的。

- **S-a**：建探针 + 修 4.7 缺口 + 接上九处调用点，**同一步停掉轮询的 reconcile 派发**；
- **S-b**：删轮询线程与三个 statics、`RunType::default()`（D2）、`core/service/mod.rs::init_service`。

---

## 5. 定序保证表

> **表与散文双向对齐。措辞不得强于其机制。**
>
> **穷尽方法（可复核）**：对 §4 逐节抽取所有「X 之后 Y 一定已发生 / X 不会发生」句式，每条生成一行；再对本表逐行回指正文小节号。两遍都做完才算齐。下表右侧的「正文出处」列即第二遍的产物。

| 断言                                                                                          | **靠什么构造保证**                                                     | 正文出处 | 测试            |
| --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | -------- | --------------- |
| 两次控制动作的模式结论不交错                                                                  | `OperationGate` FIFO + 三步同守卫                                      | §4.2     | T-MODE-03       |
| 控制动作 → probe → reconcile 三步不可拆                                                       | `reconcile_with(&guard)` **无 `IpcState` 参数**；`rg` 门禁钉死探针三处 | §4.5     | T-MODE-03       |
| bootstrap 的守卫外探针安全                                                                    | 执行于 `CoreClient::new` **之前**，actor 尚不存在                      | §4.5     | T-PROBE-02      |
| **探针必然在有限时间内返回**                                                                  | `OsServiceProbe` 内部 `timeout` + runner 层 `kill_on_drop`             | §4.1     | T-PROBE-06      |
| **就绪等待总耗时不超过 `READY_BUDGET`**                                                       | per-probe 预算取 `remaining.min(PER_PROBE_BUDGET)`                     | §4.3     | T-MODE-05       |
| **警告覆盖面不小于基线**                                                                      | `daemon_status` 保留 `ServiceStatus`，条件逐字复现合取式               | §4.3     | T-PROBE-03/05   |
| **bootstrap 的探针诊断有归属**                                                                | bootstrap 调用同一个 `report_probe_diagnostics`                        | §4.3     | T-PROBE-07      |
| **控制失败时仍然 reconcile，且控制错误优先返回**                                              | §4.4 表的源码顺序（reconcile 在前、`control?` 在后），与基线 F61 同形  | §4.4     | T-CTL-01…04     |
| **控制失败时不进入就绪等待**                                                                  | 就绪等待外层的 `result.is_ok()` 条件                                   | §4.4     | T-CTL-04        |
| **关停开始后不再有新控制序列进入**                                                            | `ControlAdmission::enter` 的**双重** closed 检查                       | §4.6     | T-SD-03         |
| **drain 在预算内完成时，关停不会越过在飞控制序列**（**条件式——预算耗尽则不保证，见 R-C2-1**） | 关准入 → 有界 drain → **持 permit 到 actor 停止之后**                  | §4.6     | T-SD-01/02      |
| **重复 `shutdown()` 不重复执行拆除**                                                          | 单次飞行状态机，**不是 `AtomicBool`**                                  | §4.6     | T-SD-04         |
| 任何时刻只有一个模式生产者                                                                    | S-a 同步停掉轮询派发                                                   | §4.8     | `rg` 判据（§9） |

---

## 6. 测试矩阵

> **第三列**：删掉那行生产代码，这条测试真的会红吗？**填不出第三列的测试不进矩阵。**
>
> **两类空转陷阱**（前几轮各踩一次）：①状态有**多个**写入点，删其一另一仍生效；②**mock 打在被测机制之上**，删掉机制对 mock 无影响——**接缝必须低于被测机制**。

| ID             | 断言                                                                                                                   | **删掉哪行会让它红**                                                                                                                                                  |
| -------------- | ---------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-PROBE-01     | 兼容门 fail-closed：daemon 在跑但不放行 → 探针 `Disconnected`                                                          | 探针里调 `target_ipc_state()` 那行                                                                                                                                    |
| T-PROBE-02     | bootstrap 用探针真值而非 `Disconnected` 默认（修 F35）                                                                 | `client/mod.rs:303` 的 `probe().await`                                                                                                                                |
| T-PROBE-03     | **Running + `Unparsable` 也告警**（不只 `Incompatible`）                                                               | 警告条件里的 `!compat.allows_service_backend()`（改成 `matches!(Incompatible)` 即红）                                                                                 |
| T-PROBE-04     | 探针失败发出 `service_probe_failed` 降级                                                                               | 处理 `error` 的 `degradation.publish(..)` 行                                                                                                                          |
| T-PROBE-05     | **Running + `Unknown` 告警；Stopped + `Unknown` 不告警**                                                               | `ProbeOutcome.daemon_status` 字段本身（去掉它两种 `Unknown` 分不开，必红）                                                                                            |
| **T-PROBE-06** | **注入永不返回的 `MockServiceStatusRunner`**，`OsServiceProbe::probe_within` 仍在预算内返回                            | `OsServiceProbe` 里的 `tokio::time::timeout(..)`。**接缝在 runner 层**——用 `MockServiceProbe` 会绕过被测 timeout，那是空转                                            |
| **T-PROBE-07** | bootstrap 探针失败/不兼容时**也**产出诊断                                                                              | bootstrap 处的 `report_probe_diagnostics(..)` 调用行                                                                                                                  |
| T-MODE-01      | 关闭 `enable_service_mode` → 得 `Normal` 并 `set_backend`                                                              | `request.rs:82-85` 删提前返回后送真值那行                                                                                                                             |
| T-MODE-02      | 六个控制动作后**各自**触发 probe+reconcile——逐条独立断言，**断言「至少一次」**                                         | 各自 facade 方法里的 `reconcile_with(&guard)`                                                                                                                         |
| T-MODE-03      | start→stop 序列下终态 `Normal`，晚到 probe 不翻转                                                                      | `reconcile_with` 的 `guard` 参数                                                                                                                                      |
| T-MODE-04      | 有界等待成功路径：脚本 runner 第 N 次兼容 → `Service`，无降级                                                          | `await_service_ready` 循环体                                                                                                                                          |
| **T-MODE-05**  | **挂死 runner**：`await_service_ready` 在 `READY_BUDGET` 内返回 TimedOut（**不是** `READY_BUDGET + PER_PROBE_BUDGET`） | `remaining.min(PER_PROBE_BUDGET)` 里的 `remaining.min(..)`（去掉即超预算，断言时限即红）                                                                              |
| T-CTL-01       | 控制 `Err` + reconcile `Ok` → 返回**控制的** `Err`，且 reconcile **确实跑过**                                          | `reconcile_with` 调用位于 `control?` **之前**（改成早退即红）                                                                                                         |
| T-CTL-02       | 控制 `Err` + reconcile `Err` → 返回**控制的** `Err`，reconcile 失败进降级                                              | 错误优先级那行                                                                                                                                                        |
| T-CTL-03       | 控制 `Ok` + 就绪超时 + `force_local_with` **失败** → 返回 `Err` + 两条降级                                             | §4.4 第 4 行的失败分支                                                                                                                                                |
| T-CTL-04       | 控制 `Err`（update）→ **跳过**就绪等待（断言 `await_service_ready` 零调用）                                            | 就绪等待外层的 `result.is_ok()` 条件                                                                                                                                  |
| **T-GATE-01**  | **`Shutdown` 把等待中的取门请求全部以 `ShuttingDown` 排空**——测试**直接持有 `OperationGate`**、观察 waiter 的 reply    | **`gate.rs:57-59` 的 `waiters.drain(..)` 循环**。**不能走 actor 集成测试**：actor state 析构会 drop reply port，等待者照样收到错误，删掉 drain 也不会红（审查者点名） |
| **T-SD-01**    | `Shutdown` 落在守卫取得之后、外部命令之前 → **外部命令不被调用**                                                       | 外部命令前的 `admission.check_open()?`                                                                                                                                |
| **T-SD-02**    | 控制序列卡在外部命令 → `shutdown` 在 `QUIESCE_BUDGET` 内返回 + `shutdown_quiesce_timeout` 降级                         | `timeout(QUIESCE_BUDGET, ..)`（改成裸 await 即挂死）                                                                                                                  |
| **T-SD-03**    | 在 `enter()` 的 acquire **之后**、返回之前发生 `close()` → 该次 `enter()` 返回 `Err(ShuttingDown)`                     | `enter()` 的**第二次** closed 检查（删掉即放行，必红）                                                                                                                |
| **T-SD-04**    | 并发两次 `shutdown()` → 拆除**恰好执行一次**，两个调用都在其完成后返回                                                 | 单次飞行状态机；**改成 `AtomicBool` 即红**（第二个调用会在拆除完成前提前返回）                                                                                        |

**回归契约**：区分**存活测试被迫修改**（不允许，停下核查）与**被删模块自带单测随属主消失**（预期）。

**已知必改**：`client/core.rs:1207-1214` → 断言注入 mode **并改名**；`core/service/ipc.rs:140-187` 的两条 `target_ipc_state` 单测随文件重整迁到 `core/service/probe.rs`，断言不变。

---

## 7. 契约归属

> **口诀**：签名只能保证**「值到得了这里」**（及其对偶「到不了这里」）与**「类型在此平台不存在」**；凡「**不会去做某事**」一律靠测试 / 门禁 / `rg`。
>
> **同族第二条**（承自 v4，C3 那边是主战场，这里仍适用）：**返回值的错误通道只报告调用的结果，不报告副作用的缺席。**

| 契约                                   | 由谁保证                                    | 为什么可验证                                                               |
| -------------------------------------- | ------------------------------------------- | -------------------------------------------------------------------------- |
| 调用方无法把陈旧探针结果喂给 reconcile | **签名**                                    | `reconcile`/`reconcile_with` **没有 `IpcState` 参数**                      |
| **探针必然有限时间返回**               | **实现内部的 `timeout` + 低层 runner 接缝** | 单点可验（T-PROBE-06）；**不是**「每个调用方记得包一层」那种不可强制的契约 |
| 任何探针都不在守卫外开始               | **ledger / `rg` 门禁**                      | `rg -n '\.probe(_within)?\('` 恒三处且位置固定                             |
| `force_local_with` 只在超时分支用      | **`rg` 门禁**                               | 恰好一处调用点                                                             |
| 顺序类契约                             | **测试**                                    | 控制流性质，类型系统表达不了                                               |
| **关停后不再有新控制序列进入**         | **`ControlAdmission` 双重检查 + 测试**      | T-SD-03；**已进入的序列见 R-C2-1，不谎称关闭**                             |
| `get_ipc_state` / statics 归零         | **`rg` 判据**                               | 删除类不变量                                                               |

---

## 8. 门禁与残留

**门禁：**

1. **「diff 应为空」形态的判据，只要跑在中间提交之后，必须与基线比**：`git diff --exit-code <base>..HEAD -- <path>`；
2. **ledger 三步顺序**：report 核对 → `--write-snapshot` → gate 比对；
3. **删模块要有「模块不存在」断言**（`core/service/mod.rs::init_service`、`ipc.rs` 的轮询部分）。

**bindings 预期**：`ServiceProbe` / `ProbeOutcome` / `ServiceStatusRunner` / `ControlAdmission` 全部 `pub(crate)`；`uninstall_service` 命令名与签名不变；不新增命令。**结论：diff 恰好为空**，判据 `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`（与 `ci.yml:306-308` 同形）。

**残留：**

| #          | 残留                                                        | 性质                                                                   | owner / 移除条件                                                                      |
| ---------- | ----------------------------------------------------------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| **R-C2-1** | drain 超时后，被放弃的控制序列仍可能完成其外部命令          | **本 PR 引入的有界窗口**；今天是**无界**同类问题且更糟（连准入都没有） | 移除条件 = 外部命令可取消（需 `runas`/`sudo` 侧支持，不在本仓）                       |
| **R-C2-2** | `check_open()` 与外部命令 spawn 之间的 TOCTOU               | 同上                                                                   | 与 R-C2-1 同条件                                                                      |
| **R-C2-3** | update 有界等待超时后 daemon 可能稍后才就绪，而已收敛 Local | **本 PR 引入的取舍**（今天靠 5 s 轮询最终纠正）                        | 下次任一服务控制动作会重新 probe 纠正；**不加后台重试**——那会把第二个模式生产者请回来 |

---

## 9. Exit 判据

| 要求                                                              | 验证                                                                                                                              |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 显式模式收敛全部走守卫                                            | T-MODE-01/02/03；`rg` 探针三处                                                                                                    |
| `reconcile` 家族无 `IpcState` 参数                                | 签名核对（编译期即拦）                                                                                                            |
| **探针自身有界且该性质可测**                                      | T-PROBE-06（接缝在 runner 层）                                                                                                    |
| **就绪等待不超预算**                                              | T-MODE-05                                                                                                                         |
| **警告覆盖面不小于基线**                                          | T-PROBE-03/05/07                                                                                                                  |
| 删 `pending_run_type` 设计                                        | **no-op**（F9）                                                                                                                   |
| 删轮询线程与 statics                                              | `rg 'IPC_STATE\|KILL_FLAG\|HEALTH_CHECK_RUNNING\|spawn_health_check\|get_ipc_state'` 为 0                                         |
| 删 `impl Default for RunType`                                     | `rg 'RunType::default'` 为 0；`initial` 两个调用点都传参，`mod.rs:371` 覆盖赋值已删                                               |
| 六个入口签名一致且全在 `ServiceControlOps` 上                     | 结构核对：六个具体函数**仍在 `core::service::control`**（满足 `design.md:333`，按 5a `:1037` 读法）；**扩到六方法须写进 PR 描述** |
| **控制失败六种处置**                                              | T-CTL-01…04                                                                                                                       |
| **关停静默期**                                                    | T-SD-01…04、T-GATE-01；**R-C2-1/2 必须出现在 PR 描述里**                                                                          |
| bindings diff 为空                                                | `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`                                                                  |
| **C3 未被本 PR 触碰**                                             | `core/clash/core.rs::change_default_network_dns` 仅有**签名加参**一处改动；`feat.rs:416-418` 迁移标记改指 **PR-5e**               |
| **smoke 2**（v1→v2 升级 + 拒绝升级 fail-closed Local）            | 本机可跑，**须真实服务环境**；**它是 C2 的真正验收点**——迁移不完整会正好打断它**而 `rg` 门禁全绿**                                |
| **R-C2-1/2/3 三条残留**逐条出现在 PR 描述里                       | 文本核对——**「不修」必须是被记录的决定，不是沉默**                                                                                |
| **对 `design.md` 的一处有意偏离**（六方法 trait）出现在 PR 描述里 | 文本核对；`design.md` **本身不得修改**                                                                                            |

> **smoke 3（macOS TUN/DNS）不在本 PR 范围** —— 随 C3 移交 PR-5e。
