# PR-5d 实施计划 — 运行模式探针与 macOS DNS 生命周期

**日期：** 2026-08-03
**版本：** v1（首版；设计直接写在正文，**不设计放附录**——那是 5c 第 #1 条 BLOCKING 的成因）
**分支基线：** `refactor/core-manager-actor`（PR-5c 实施完成后的树；本计划的事实锚点取自 `899b069f5`，**实施前须复核**，见 §0）
**权威 spec：** `task.md` 卡 C2、C3
**上游材料：** PR-5c v4 终态 `git show 5a02a1727:docs/superpowers/plans/2026-08-02-pr5c-residual-cleanup.md`
**平台：** Windows 11 / PowerShell（**macOS 路径无法本地验证**，见 §10）

> **本阶段是并发设计，不是清理。** 5c 拆分的理由：**C2 要用一个可线性化的探针替换掉一个活的状态生产者，C3 要给一个从未有序过的东西建立顺序**——两件都与 5b 同量级。5c 的删除面靠「没有活调用者」即可证明，本阶段**每一条都要证明「每条路径都有人接、且接得住并发」**。

---

## 0. 边界与前置

**做：**

1. **C2**：服务状态探针（一次性、经兼容门控、可注入）、**九处调用点**、Service→Normal 缺口、**先建后删**地移除 5 s 轮询与三个 statics；
2. **C3**：`MacosDnsPort` 双适配器、`SetTunDns` 守卫消息、恢复拆分（主路径 await / `Drop` 只记日志）、控制动作前拆 DNS、写回读回校验；
3. **D2**：`CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`（它是删 statics 的前置阻塞）；
4. 5c 携带的 `KILL_FLAG` weak-CAS 缺陷——**随轮询线程删除而消失，不单独修**（5c §10.1 已记账）。

**不做：** `UpdaterManager::global()`（PR-6d）、五个 owner-PR globals、`feat::patch_verge` 的 sysproxy/systray/locale 编排（PR-6e）、启动时检测并清理残留 DNS 覆写（PR-6，见 §10）。

**前置（实施前必做）：** 本计划的行号锚点全部取自 `899b069f5`，而 **5c 已经删掉 `Instance`、`Logger`、`core/manager.rs`、`core/state.rs` 并改动 `core/clash/core.rs`**。开工第一步是**对着 5c 落地后的树复核全部锚点**，特别是 `core/clash/core.rs` 内的行号（该文件被删掉约 75%）。**锚点漂移不是小事**——5b 的教训是引用失准会让整条论证失效。

---

## 1. 已核验事实（承自 5c，编号保持原号）

### 1.1 C2 —— 运行模式

| ID  | 事实                                                                                                                                   | 锚点                                                                |
| --- | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| F9  | `pending_run_type` 在 Rust 源码中**不存在**（仅设计文档命中）→ 卡面该项是 **no-op**                                                    | 全仓 grep                                                           |
| F10 | 「reconcile 走 `CoreOperationGuard`」**已满足**                                                                                        | `core/actor/request.rs:87`                                          |
| F11 | 5 s 轮询与三个 statics 全在一个文件；`spawn_health_check` **4 处 spawn**（boot + install/start/restart）                               | `core/service/ipc.rs:28-30,85-101`（`:97` 是 5 s）                  |
| F12 | `get_ipc_state()` **5 处生产读**                                                                                                       | `feat.rs:383,401`；`client/mod.rs:305,544`；`core/clash/core.rs:70` |
| F13 | `RunType::default()` 读两个 legacy global，且被 `CoreStatusView::initial()` 调用——**删 statics 的主阻塞点**                            | `core/clash/core.rs:61-78`；`core/actor/types.rs:48`                |
| F14 | `set_backend` **生产调用点恰好一个**；**不存在 `set_mode`**（卡面的 `set_mode` 对应现有 `set_backend`）                                | `core/actor/request.rs:88`                                          |
| F15 | `ServiceControlOps` 只有 install/start/stop/restart；**update / uninstall 不在 trait 上**，是自由函数                                  | `core/actor/backend.rs:618-624`                                     |
| F16 | `uninstall_service` **绕过 facade**；`install_service` **不 reconcile**——两处不对称，**性质不同**（见 §2.5）                           | `ipc.rs:933-935`；`client/mod.rs:504-510`                           |
| F35 | **`IPC_STATE` 初值 `Disconnected`**，bootstrap 在任何 health check 之前读它 → **今天 bootstrap 恒判 `Normal`**，靠首次轮询**异步纠正** | `service/ipc.rs:28`；`client/mod.rs:303-306`                        |
| F36 | **探针两半已存在**：`control::status()`（子进程）+ **纯函数** `target_ipc_state()`；`health_check` = 两半 + 循环                       | `control.rs:351-376`；`ipc.rs:131-138`、`:103-124`                  |

### 1.2 C3 —— macOS DNS

| ID  | 事实                                                                                                                                                                                                    | 锚点                                                                  |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| F18 | **`MacosDnsGuard` 不存在**（仅两条「等 PR-5c 建它」的注释）                                                                                                                                             | `feat.rs:417-418`                                                     |
| F19 | 真正的覆写代码是 `CoreManager::change_default_network_dns` + `previous_dns` 状态                                                                                                                        | `core/clash/core.rs:404-457`、`:373-383`                              |
| F20 | 它读两个 global，且 **Service / Local 双路径在此分叉**                                                                                                                                                  | `core/clash/core.rs:409,415-420,440-450`                              |
| F21 | **IPC `set_dns` 已上线**（端点 + wire golden 均在）                                                                                                                                                     | `nyanpasu_ipc/src/client/shortcuts.rs:91-96`                          |
| F22 | **DNS 与 start/stop 今天毫无保序**；**走 restart 的路径根本不碰 DNS**；失败被 `let _ =` 吞掉                                                                                                            | `feat.rs:409-426`                                                     |
| F23 | **退出不恢复 DNS**——覆写跨崩溃/退出泄漏（**5c 之前就存在的缺陷**）                                                                                                                                      | `utils/resolve.rs:290`；`client/core.rs:277-283`                      |
| F24 | `SystemDnsCache` 只管 flush，**与 TUN 的 DNS 覆写生命周期无关**，勿混淆                                                                                                                                 | `client/system_dns.rs:4-7`                                            |
| F40 | **Local 写路径不提权**：`osascript` 调用**不带** `with administrator privileges`，脚本本体只有 `networksetup -setdnsservers`。所以它**不弹窗**，但**若需管理员权限就直接失败**，而失败被 `let _ =` 吞掉 | `nyanpasu-utils/src/network/mod.rs:47-54`；`scripts/set-macos-dns.sh` |
| F41 | **读路径同样不检查退出码**，且空/不可解析 stdout → `Ok(None)`。因此**当原始 DNS 本就是 `None` 时，一次失败的读会与期望值「相等」**——回读校验在该情形下会把失败误报成成功                                | `nyanpasu-utils/src/network/mod.rs`（`get_dns`）                      |

### 1.3 smoke / CI

| ID  | 事实                                                                                                                                                    | 锚点                                       |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| F33 | CI **有** macOS runner 且在 PR 上跑 `cargo test --all-features` → **cfg 门控单测真实运行**                                                              | `.github/workflows/ci.yml:201-215,303-304` |
| F34 | **但没有任何作业能跑 smoke 3**——无作业启动应用；TUN 需签名扩展 + root，**是能力边界非配置缺失**（加 job / 加 runner 都无效，需自托管 mac 且预批准扩展） | `ci.yml`（全仓仅 `:304` 一处测试调用）     |

---

## 2. 已裁定事项（**承自 5c，不重开**）

### 2.1 D2 = A —— `CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`

`RunType::default()` 读 `Config::verge()` + `get_ipc_state()` 却被 `CoreStatusView::initial()` 调用，是典型隐藏依赖（F13）。**依赖显式传入而不是从全局捞**——这是整个迁移的方向。

**`RunType::default()` 的五处调用点**（两次不完整筛查合并才得到完整清单，**方法本身值得复用：删符号时按生产 / 测试 / 注释三类各筛一遍**）：

| 位置                                | 处置                                                                                                                                                                                   |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core/actor/types.rs:48`            | D2 主目标，改为参数                                                                                                                                                                    |
| `core/clash/core.rs:409`            | macOS DNS 路径分叉 → 随 C3 迁走                                                                                                                                                        |
| `core/clash/core.rs:399`            | **5c 已删**（`CoreManager::status` 内）——实施前复核                                                                                                                                    |
| `client/core.rs:1211`               | **测试**，必然改动：断言注入的 mode，**并连名字一起改**为 `initial_watch_snapshot_reflects_the_injected_mode`（旧名里的「legacy empty status」在 D2 之后不再是参照物，**命名即契约**） |
| `client/process_core_bridge.rs:251` | 注释里的警告，删后**悬空**，顺手清理或改写                                                                                                                                             |

### 2.2 D3 = A（含修正形态）—— DNS guard 挂 actor state，但 `Drop` 不恢复

**主路径**（`Stop` / `Shutdown` / `SetBackend`）：actor 处理器内**显式 `await` 恢复**，**在**后端动作与 reply **之前**完成。

**`Drop`**：**只记 `tracing::error!`，措辞按不变量破坏写**（`"reached Drop with DNS override still active — main-path restoration was missed"`），**不尝试任何恢复**。

> **为什么不做「尽力而为的同步 Drop」**：Service 侧同步做不到、Local 侧能做——**那半个兜底恰好在开发者最常用的模式下生效**。开发日常跑 Local，兜底在 Local 上有效 → **主路径漏了恢复也不会被发现**；等到 Service 模式（用户实际部署、开发者最少跑的那条）才暴露。**一个「在你测得到的地方生效、在你测不到的地方失效」的兜底，会系统性地把主路径 bug 藏到生产环境**——它不是「一半生效」，是**反向选择**的一半。

**恢复失败去向**：degradation sink（`phase = CoreLifecycle`、`code = "macos_dns_restore_failed"`）。

**`Drop` 不覆盖强杀**（SIGKILL / 任务管理器）——**如实写明**，兜底（启动时检测并清理残留覆写）属 **PR-6**，不在本阶段。

### 2.3 D4 —— smoke 3 记为「未在本地验证**且不可由 CI 覆盖**」

用户裁定路径乙。**不是「CI 暂未配置」，是托管 runner 的能力边界**（F34）：加 job、加 runner 都解决不了，需**自托管 mac 且预先批准网络扩展**。**这个区别必须写明**，否则下一个人会以为加个 macOS job 就能补上。

**CI 覆盖的**：cfg 门控单测（顺序、降级等**逻辑**契约，F33）。
**未验证的（逐条点名）**：①真实 TUN 开关是否触发覆写；②真实 `networksetup` / IPC `set_dns` 是否成功改写系统 DNS；③关 TUN 与正常退出后 DNS **是否真的恢复**；④Service 与 Local 两条路径在真机上是否一致。

**结论必须显式出现在 PR 描述与发布说明里，不允许沉默跳过。**

### 2.4 `SetTunDns` 在 `Shutdown` 之后一律拒绝

与 `CoreActorError::ShuttingDown` 同形，**返回 `Err` 而非静默丢弃**——静默丢弃会让调用方以为设置成功。**须覆盖三种情形**：①`Shutdown` 已开始后到达的 `SetTunDns`；②`SetTunDns` 与 `Stop` 的相对顺序；③**`SetTunDns` 与 `Stop` 并发**——`Stop` 处理器内本来就有一次恢复（2.2 主路径），若 `SetTunDns` 在其后到达，会**重新打开一个已被恢复过的覆写，而此时核已经停了**。

### 2.5 §7 两处不对称（承自 5c，**性质不同**）

| 项                                  | 裁定                                                                                                                                                                                                    |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `install_service` **不** reconcile  | **有意，加一行注释说明、不改行为**——装服务不等于起服务，运行中的后端没变化，**没有可 reconcile 的对象**；模式在服务真正启动时被拾起                                                                     |
| `uninstall_service` **绕过 facade** | **缺陷，改走 facade 并 reconcile**——①违反「Tauri 命令是薄适配器」；②**实质风险**：核在 Service 模式运行时卸载服务会让当前后端失效。**不违反 C2 卡**：C2 禁的是「迁入 CoreActor」，**facade 不是 actor** |

---

## 3. C2 设计 —— 服务状态探针与调用点

### 3.1 探针（一次性、经兼容门控、可注入）

```rust
// core/service/probe.rs（新）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait ServiceProbe: Send + Sync + 'static {
    /// 一次性查询。失败按 fail-closed 处理为 Disconnected（与今天 health_check
    /// 的 Err 分支同语义）。ServiceCompat 一并返回——**警告职责的接手方靠它**（§5 #3）。
    async fn probe(&self) -> (IpcState, ServiceCompat, Option<anyhow::Error>);
}

pub(crate) struct OsServiceProbe;   // 调 control::status() + target_ipc_state()
```

**为什么是 trait 而非自由函数**：①**bootstrap 需要它，而那时 `CoreClient` 还不存在**（F35）；②测试要脚本化「daemon 在跑但不兼容」这类分支，而真实实现要起子进程；③它与 `ServiceControlOps` 同层同形。

**`target_ipc_state` 与 `ServiceCompat` 一行不改**——PR-5-pre 已审的 fail-closed 门，探针只是宿主。

### 3.2 九处调用点（逐一点名，无遗漏）

| #   | 位置                                                 | 今天怎么拿模式                                  | 改为                                          |
| --- | ---------------------------------------------------- | ----------------------------------------------- | --------------------------------------------- |
| 1   | **bootstrap**（`client/mod.rs:303`）                 | `get_ipc_state()`（**恒 `Disconnected`**，F35） | `probe()` 一次——**顺带修掉 F35 这个既有缺陷** |
| 2   | **install 之后**                                     | 轮询异步发现                                    | `probe()` + reconcile                         |
| 3   | **start 之后**                                       | 轮询                                            | 同上                                          |
| 4   | **restart 之后**                                     | 轮询                                            | 同上                                          |
| 5   | **stop 之后**                                        | 轮询                                            | 同上                                          |
| 6   | **uninstall 之后**                                   | 轮询                                            | 同上——**今天缺**（2.5 已裁定走 facade）       |
| 7   | **update 之后**（`init/mod.rs:251`）                 | 轮询（v1→v2 升级后靠它发现 v2）                 | `probe()` + reconcile——**直接关系 smoke 2**   |
| 8   | **`enable_service_mode` 配置变更后**                 | 轮询 + reconcile（但见 3.3 的洞）               | `probe()` + reconcile                         |
| 9   | **boot 的 `init_service`**（`service/mod.rs:18-29`） | 起轮询线程 + 忙等 100 ms                        | 直接 `probe()` + reconcile，**删忙等**        |

### 3.3 修 Service→Normal 缺口

今天（`request.rs:82-85`）提前返回导致 `classify(true, ..)` 硬编码，**用户关闭服务模式后 reconcile 什么都不做、后端停留在 Service**。

**改法**：删掉提前返回，把真值送进 `classify`。`classify` 本身**不改**——它已经正确，缺的只是有人把 `false` 喂给它。

### 3.4 步骤顺序：**先建后删**，但**不是「双轨并行」**

> **5c v4 曾把这一步写成「轮询仍在跑、双轨等价、任一步可独立回滚」——那是错的**，第二轮对抗审推翻了它：**两个生产者同时写同一状态而无定序，比一个更糟**。单生产者的错误是确定性的、可复现的；双生产者的错误是竞态的、随时序变化。**「保留旧机制」看似保守，实际引入了一个新的失效模式。**

**正确的顺序**（§5 #2 给出定序机制后才能定稿）：

- **S-a**：建探针 + 修 3.3 的缺口 + 接上九处调用点，**同时在同一步停掉轮询的 reconcile 派发**（保留线程但让它不再写模式，或直接连线程一起删）——**关键是任何时刻只有一个模式生产者**；
- **S-b**：删轮询线程与三个 statics、`RunType::default()`（D2）。

---

## 4. C3 设计 —— DNS 适配器与恢复拆分

### 4.1 适配器（macOS-only，注入式）

```rust
// core/actor/dns.rs —— 整个文件 #[cfg(target_os = "macos")]
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait MacosDnsPort: Send + Sync + 'static {
    async fn read_current(&self, device: &str) -> anyhow::Result<Option<Vec<IpAddr>>>;
    async fn set(&self, device: &str, dns: Option<Vec<IpAddr>>) -> anyhow::Result<()>;
}

pub(crate) struct LocalMacosDns;                    // nyanpasu_utils::network::macos
pub(crate) struct ServiceMacosDns { client: .. };   // IPC set_dns（F21）
```

**不违反 D3 的「非 macOS 不加空抽象」**：整个文件在 `#[cfg(target_os = "macos")]` 下，**非 macOS 平台上这个 trait 根本不存在**——D3 禁的是「为了对称而在所有平台造一个空 port」。

fake 必须**按序记录** enable / restore / 与后端动作的相对次序，供测试断言**顺序**而非终态。

### 4.2 数据从哪来：**不扩 `CoreRequest`**

`CoreRequest` 是 run/check/apply 三条路共用的**全平台**进程描述；塞 macOS-only 的 TUN 字段会污染两条无关路径。改走独立守卫消息，desired 由 client 侧从 clash config 算好传入——**actor 不读任何配置全局**（D2/D3 接缝声明的落地形式）。

**消息形状待定于 §5 #6**（device 从哪来、`None` 原值如何表达 active）。

### 4.3 恢复拆两类

见 §2.2（已裁定）。**T-DNS 的顺序断言是唯一能证明主路径有效的东西**——`Drop` 不再恢复了，若只断言终态，判据会退回空转。

### 4.4 顺序：控制动作前先拆 DNS

```text
拆 DNS（await，IPC 尚在）  →  service_control.stop() / uninstall()  →  probe + reconcile
```

**stop 与 uninstall 两条都要测顺序**——它们是两个独立调用点，合并测则删掉一个另一个仍绿。

**拆除失败时的分岔（承自 5c 讨论，已裁定）：**

| 场景          | 处置                                        | 理由                                                                                                                                      |
| ------------- | ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **uninstall** | **中止卸载**，返回 `Err` + **用户可见错误** | 卸载**不可逆**，而拆 DNS 失败说明我们**当下正处在一个连自己的写都验证不了的状态**——在这种状态下执行不可逆操作，是把已知的不确定性固化下来 |
| **stop**      | **继续，产出 degradation**                  | 服务可再启动、通道会回来，泄漏可恢复；为拆 DNS 失败就让用户停不掉服务代价不成比例，还可能把人锁死                                         |

**判别原则**：**失败会让泄漏变成永久的 → 中止；泄漏仍可恢复 → 继续并降级。**

**中止 uninstall 的用户可见错误必须说清三件事**：**做了什么**（没有卸载）、**为什么**（DNS 覆写未能拆除，继续卸载会永久残留）、**怎么办**（重试；或先手动关闭 TUN 再卸载）。**只返回 `Err` 不够**——用户可见的失败，**措辞本身就是功能的一部分**。

### 4.5 写回读回校验（**前提已被 F41 削弱，见 §5 #5**）

裁定是「写回后读回比对」，**两条实施约束**：

**(a) 必须是语义比较**：解析成 `IpAddr` 后比较**集合**（忽略顺序与重复），解析失败即视为不一致。**做不到语义比较就不要做这个校验**——文本比较会产生**假失败**，那比不校验更糟：会把成功的操作报成失败，然后有人为了让它绿而删掉校验。

**(b) 失败进 degradation，不静默**；且测试**必须走真实适配器的校验路径**——测的是「**回读比对真的会发现不一致**」，不是「适配器会传播 `Err`」。

**TOCTOU 不在范围**：本校验的语义是「**我们的写有没有生效**」，并发的外部变更不属于它要回答的问题。

---

## 5. 五条已知待解问题（**审查者送的设计输入，不等第一轮审查再提**）

### #2 C2 不可线性化（**最重**）

**竞态**：start 完成后发起延迟 probe → stop 完成并 reconcile 成 `Normal` → 旧的 start-probe 返回 `Connected` 后到，reconcile 成 `Service`。**终态与操作顺序不符。**

**根因**：`控制动作 → probe → reconcile` 三步**不是原子的**，且多个 probe 之间**无定序**。

**我的提案（待审）**：**三步同处一个 `CoreOperationGuard` 之内**，定序机制**就是 `OperationGate` 的 FIFO**——它是 5a 已实现且已测的现成机制，不需要新造。这样两次控制动作天然串行，晚到的 probe 不可能跨越守卫边界落在别人中间。

**但它直接撞上 #4 的死锁**（`reconcile()` 自己还要取一次守卫），所以两条必须一起解。

**另一半**：`update_service()` **只等更新进程退出、不等 daemon 就绪**，所以 S-b 之后一次立即 probe 可能把模式**永久判成 `Normal`**（没有轮询再来纠正了）。**提案**：update 后改为**有界等待就绪**（轮询 `status()` 直到兼容或超时），而非单次 probe。**超时后的去向要定**（degraded 还是 Err）。

### #3 `health_check` 的警告职责没人接手

它不只是 status + 分类 + 循环，还**发不兼容 daemon 的警告并派发转换**——而 **smoke 2 明确要求那条警告**。

**提案**：探针返回值已含 `ServiceCompat`（§3.1），**由调用方在 reconcile 路径上发警告**。**须点名是哪个调用点发**，否则九处各发一次会刷屏。

### #4 拆 DNS 的守卫跨度与 `reconcile()` 死锁

**两个子问题**：①**一个 `CoreOperationGuard` 是否横跨拆除与外部控制动作**——不横跨的话，中间会有另一条 `SetTunDns` 把 DNS 重新打开；②**横跨到 post-control `reconcile()` 会死锁**，因为 `reconcile()` 自己要取一次守卫（`request.rs:87`）。

**提案**：引入**「已持守卫」的 reconcile 变体**（`reconcile_with(&guard, ..)`），原 `reconcile()` 保留为「自取守卫」的薄包装。这同时解决 #2 的原子性需求——**两条是同一个机制的两面**。

### #5 读路径同样不检查退出码（**这条削弱了 4.5 的前提**）

F41：读路径不检查 `output.status`，空/不可解析 stdout → `Ok(None)`。**于是当原始 DNS 本就是 `None` 时，一次失败的读会与期望值「相等」，恢复被误报成功。**

**这意味着 4.5 的「读回校验」在最需要它的情形下失效**——恢复到 `None` 正是关闭 TUN 的主路径。

**候选**（**须裁定**）：**(a)** 我们的适配器**自己实现读**（自起子进程 + 检查 `ExitStatus`），不用上游 `get_dns`；**(b)** 只在 `expected != None` 时做校验，并**显式写明 `None` 情形不可验证**；**(c)** 用哨兵区分「没有配置 DNS」与「读失败」。

**我倾向 (a)**：它让校验在**全部**情形下有意义，代价是我们仓内多一处子进程调用逻辑；(b) 诚实但恰好在主路径上留洞。

### #6 适配器接线不全

- guard 只有 `previous` / `device`，但 port 要 device 参数、而 `SetTunDns` **不带 device**；
- **没有字段注入 Local / Service 两个适配器**；
- **没有规则钉住 `SetBackend` 改 `state.mode` 之前用哪个适配器**；
- **原始值为 `None` 时需要单独的 active 标记**（`Option<MacosDnsGuard>` 或 `Option<Option<Vec<IpAddr>>>`），否则「记录 guard 为 active」实现不出来。

**待设计**：消息与 state 的完整形状，**在附录之外、正文声明一次**。

---

## 6. 定序保证表（**每条「X 之后 Y 一定已发生」都要说出机制**）

> 本阶段是并发设计，**「判据可验证」升级为「定序必须有机制」**：不能靠时序巧合。#2 那条不可线性化正是这么被抓出来的。

| 断言                                 | 靠什么机制保证                         | 状态                            |
| ------------------------------------ | -------------------------------------- | ------------------------------- |
| 两次控制动作的模式结论不交错         | `OperationGate` FIFO（5a 已有）        | **提案中**（#2/#4 一起定稿）    |
| 恢复发生在后端动作与 reply 之前      | actor 处理器内的 `await` 点            | 已裁定（2.2）                   |
| 拆 DNS 发生在 stop / uninstall 之前  | 调用点顺序 + 同一守卫                  | **待 #4 定稿**                  |
| `Shutdown` 后不再有 `SetTunDns` 生效 | 守卫准入检查（与 `ShuttingDown` 同形） | 已裁定（2.4）                   |
| update 之后模式反映的是 v2 daemon    | **有界等待就绪**，非单次 probe         | **提案中**（#2 后半）           |
| 任何时刻只有一个模式生产者           | S-a 同步停掉轮询派发                   | 已定（3.4，**不再用「双轨」**） |

---

## 7. 测试矩阵

> **第三列是断言**：删掉那行生产代码，这条测试真的会红吗？

| ID         | 断言                                                                 | **删掉哪行会让它红**                                     |
| ---------- | -------------------------------------------------------------------- | -------------------------------------------------------- |
| T-PROBE-01 | 兼容门 fail-closed：daemon 在跑但不放行 → 探针返回 `Disconnected`    | 探针里调 `target_ipc_state()` 的那行                     |
| T-PROBE-02 | **bootstrap 用探针真值而非 `Disconnected` 默认**（修 F35）           | bootstrap 处的 `probe()` 调用行                          |
| T-PROBE-03 | 不兼容时**发出警告**（smoke 2 要求）                                 | 警告发出点（#3 定稿后填）                                |
| T-MODE-01  | 关闭 `enable_service_mode` → 得 `Normal` 并 `set_backend`            | `request.rs` 送真值进 `classify` 那行                    |
| T-MODE-02  | 六个控制动作后各探测一次——**逐条独立断言，不合并**                   | 各自的 `probe()+reconcile` 行                            |
| T-MODE-03  | **#2 的竞态**：start→stop 序列下终态为 `Normal`，晚到的 probe 不翻转 | 守卫跨度那行（去掉守卫即红）                             |
| T-DNS-01   | `SetTunDns{Some}` → 适配器 `set` 被调，guard 记为 active             | 处理器里 `port.set()` 那行                               |
| T-DNS-02   | **顺序**：`Stop` 时恢复在 `backend.stop()` **之前**                  | `restore().await` 早于 `backend.stop()` 那行（对调即红） |
| T-DNS-03   | **顺序**：`Shutdown` 时恢复在后端动作与 **reply** 之前               | shutdown 处理器的 `restore().await` 行                   |
| T-DNS-04   | **回读比对真的会发现不一致**（走真实适配器）                         | 适配器里 `read_current()` 比对那行                       |
| T-DNS-05   | Service **stop**：拆 DNS 在 `stop()` 之前                            | `stop_service` 里的拆 DNS 行                             |
| T-DNS-06   | Service **uninstall**：同上顺序 + **失败时中止卸载**                 | `uninstall_service` 里的拆 DNS 行与中止分支              |
| T-DNS-07   | `Shutdown` 后到达的 `SetTunDns` → `Err` 而非静默丢弃                 | 准入检查那行                                             |

**回归契约**：区分**存活测试被迫修改**（不允许，停下核查）与**被删模块自带单测随属主消失**（预期）。**已知必改一条**：`initial_watch_snapshot_matches_legacy_empty_status` → 断言注入 mode **并改名**（2.1）。

---

## 8. 契约归属

> **判别口诀**：签名能保证的只有**「值到得了这里」**与**「类型在此平台不存在」**；凡「**不会去做某事**」一律靠测试 / 门禁 / `rg`，**且必须说得出怎么验**。

| 契约                           | 由谁保证        | 为什么可验证                                                                                                                   |
| ------------------------------ | --------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| 非 macOS 不存在 DNS 抽象       | **cfg / 类型**  | 非 macOS 上**引用它编译不过**——真正的类型级保证                                                                                |
| DNS 路径选择不回头读全局       | **ledger 门禁** | `core/actor/dns.rs` 的 `Config::*()` / `::global()` 计数恒为 0——**计数可数**；签名给了 `mode` 参数但**拦不住函数体里再读一次** |
| 顺序类契约                     | **测试**        | 控制流性质，类型系统表达不了 → T-DNS-02/03/05/06                                                                               |
| `get_ipc_state` / statics 归零 | **`rg` 判据**   | 删除类不变量                                                                                                                   |

---

## 9. 门禁（**沿用 5c 学到的三条**）

1. **「diff 应为空」形态的判据，只要跑在中间提交之后，必须与基线比**：`git diff --exit-code <base>..HEAD -- <path>`；
2. **ledger 三步顺序**：report 核对 → `--write-snapshot` → gate 比对。**顺序本身是判据的一部分**；
3. **删模块要有「模块不存在」断言**，不能只查调用点归零。

**bindings 预期**：待定——`set_mode` / `reconcile_mode` 若暴露到命令面会有 wire 变化，**须在设计定稿时明确并写成「恰好这些」**。

---

## 10. 风险

| 风险                                       | 概率 | 影响                       | 缓解                                                  |
| ------------------------------------------ | ---- | -------------------------- | ----------------------------------------------------- |
| **锚点漂移**（本计划锚点取自 5c 之前的树） | 高   | 引用失准、论证失效         | §0 前置：实施前对着 5c 落地后的树复核全部锚点         |
| #2 定序机制未定稿就开工                    | 中   | 竞态回归，且 `rg` 门禁全绿 | #2/#4 必须先定稿；T-MODE-03 钉住                      |
| **DNS 覆写在非管理员账户可能一直静默失效** | 中   | **误判为 5d 弄坏的**       | 见下（**必须预先写明**）                              |
| `Drop` 不覆盖强杀 → DNS 残留               | 中   | 退出后全机解析受影响       | 如实写明；兜底属 PR-6                                 |
| smoke 3 不可验证                           | 高   | Exit 判据不可满足          | D4 已裁：记为已知未验证风险，结论进 PR 描述与发布说明 |

> ### 关于「非管理员账户可能一直静默失效」——**必须预先写明**
>
> **推理链**：`networksetup -setdnsservers` 在 macOS 上通常需要管理员权限 → 但代码**不提权**（F40）→ 失败被 `let _ =` 吞掉 → **没有任何观测点**。所以「这个功能在非管理员账户上可能从来就没工作过」**不是推测，而是当前代码结构下必然无法被发现的一类失效**——不是「碰巧没人报」，是**报不出来**。
>
> **加上读回校验之后它会第一次变得可见。** 这**不是我们引入的回归，但我们会是发现它的人**。
>
> **判别方法**（供冒烟时立即区分）：**在 5d 之前的版本上用同一账户手动跑一次 `networksetup -setdnsservers`**，看是否需要授权——能立刻分辨「5d 弄坏的」还是「5d 让它第一次可见」。
>
> **这也是本阶段的一项真实收益**，值得记账：C3 的价值不只是保序，还包括**让一条此前不可观测的路径变得可观测**。
>
> 与 5b 那条纪律方向相反但同源：那次是**别把既有缺陷算成我们引入的**，这次是**别把即将暴露的既有缺陷当成我们弄坏的**。

---

## 11. Exit 判据

| 要求                                                       | 验证                                                                                                  |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| 显式 `set_mode` / `reconcile_mode` 走守卫                  | T-MODE-01/02/03；`rg` 判据                                                                            |
| 删 `pending_run_type` 设计                                 | **no-op**（F9：不存在）                                                                               |
| 删轮询线程与 statics                                       | `rg 'IPC_STATE\|KILL_FLAG\|HEALTH_CHECK_RUNNING\|spawn_health_check\|get_ipc_state'` 为 0             |
| install/update/uninstall 保持独立 controller，不迁入 actor | 结构核对；facade 调 controller **不违反**该约束                                                       |
| `MacosDnsGuard` 与 start/stop 保序                         | T-DNS-02/03/05/06                                                                                     |
| Service backend 用 IPC `set_dns`                           | T-DNS 双适配器 parity                                                                                 |
| 非 macOS 不加空抽象                                        | cfg 门控——非 macOS 上类型不存在                                                                       |
| **smoke 2**（v1→v2 升级 + 拒绝升级 fail-closed Local）     | 本机可跑，**须真实服务环境**；**它是 C2 的真正验收点**——C2 迁移不完整会正好打断它**而 `rg` 门禁全绿** |
| **smoke 3**（macOS TUN/DNS）                               | **未在本地验证且不可由 CI 覆盖**（D4）；结论进 PR 描述与发布说明                                      |
