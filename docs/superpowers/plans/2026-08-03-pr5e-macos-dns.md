# PR-5e 实施计划 — macOS DNS 生命周期（C3）

**日期：** 2026-08-03
**版本：** v1（从 PR-5d v4 拆出；已吸收 v4 对抗审对 C3 的全部 BLOCKING）
**分支基线：** `refactor/core-manager-actor` @ **`6f1a6683d`**
**前置：** **PR-5d（C2）必须先落地**——见 §1.1
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/` 下 `task.md` 卡 C3 + **`design.md` §9 `:337`**（macOS DNS 段）
**平台：** Windows 11 / PowerShell（**macOS 路径无法本地验证**）

---

## 0. 本计划的第一原则

> **错误通道报告的是调用的结果，永远不是副作用的缺席。**

一次写调用返回 `Err`，只说明**这次调用报告了失败**，不说明外部状态没被改动——daemon 可能已改完 DNS 才丢了响应，本地命令可能改完才非零退出。

**这条原则在 v4 里被声明了，却只在「施加」路径上被实现；恢复路径、候选推进、以及三条测试都仍在把 `Err` 当作「没发生」。** 审查者逐一点出。因此本计划把它升格为**受管辖点清单**（§2），实施与审查都按清单逐项核对，而不是靠记忆。

---

## 1. 边界与前置

### 1.1 前置：PR-5d 已落地

C3 **依赖** C2 提供的结构，且依赖方向是单向的：

| C3 需要的                                                  | 由 PR-5d 提供 |
| ---------------------------------------------------------- | ------------- |
| 六个控制入口的统一序列（拆 DNS 插在控制动作之前）          | §4.2 统一形态 |
| `reconcile_with(&guard)` 单一收敛点（重施加挂在其后）      | §4.5          |
| `ControlAdmission`（关停静默期，DNS 恢复要挂进去）         | §4.6          |
| 六方法 `ServiceControlOps`（teardown 测试要能替换 runner） | §3.2          |

**PR-5d 已完成的解耦点**：`change_default_network_dns(run_type, enabled)` 已加参（PR-5d §3.1，值取自 `feat.rs:409` 已持有的 `core_status().2`）。本 PR 把整个函数迁走。

### 1.2 做

1. `MacosDnsPort` 双适配器（**读一律本地**、写按模式分叉）；
2. `SetTunDns` 守卫消息 + `DnsOverride` 状态机；
3. 恢复拆分（主路径 `await` / `Drop` 只记日志）；
4. 六个控制动作前拆 DNS；
5. 写回读回校验 + **四态读**；
6. **actor 内 DNS I/O 的有界性**（PR-5d §4.6 明确移交本 PR）。

### 1.3 不做

启动时检测并清理残留 DNS 覆写（PR-6）；扩 IPC 线（见 §3.1 路线②）。

---

## 2. 受管辖点清单 —— §0 原则治理的每一处

> **这张表存在的理由**：v4 声明了原则却漏实现三处。**清单让下一位审查者核对列表，而不是重新发现遗漏。**
>
> **穷尽方法**：对「写」与「读」两类外部调用各自枚举全部调用点，再对每个调用点问「这里有没有把 `Err`/`Ok` 当成副作用的证据」。

| #   | 受管辖点                                 | 正确处置                                                                     | 落点        |
| --- | ---------------------------------------- | ---------------------------------------------------------------------------- | ----------- |
| 1   | **施加写** `write(target, Some(tun_ip))` | 写**之前**记账；`Err` 不撤销记账                                             | §4.2        |
| 2   | **施加后回读**                           | 三分：`desired` / `previous` / 其它。**`Ok` 也可能没生效**                   | §4.2、§4.3  |
| 3   | **恢复写** `write(target, previous)`     | **与施加同构**：`Err` 不等于没恢复；每次尝试后**都要回读**                   | §4.4        |
| 4   | **候选推进**                             | **由「回读证实」推进/终止，绝不由写的返回值推进**                            | §4.4        |
| 5   | **Service 写前漂移预检**                 | 预检 `Ok` 不保证 daemon 看到同一默认设备（TOCTOU），仍需回读                 | §3.1、§4.4  |
| 6   | **`Drop`**                               | 只记日志，**不写**——因此不产生受管辖的写                                     | §4.6        |
| 7   | **测试断言**                             | 凡断言「恢复失败」的测试，**必须包含最终回读**，不得以「所有写都 `Err`」收尾 | §6 T-DNS-24 |

---

## 3. 端口设计

### 3.1 读一律本地，写按模式分叉

> **BLOCKING（v4 审查升级）：`nyanpasu_ipc` 里根本没有 DNS 读端点。** `api/network/` 只有 `mod.rs` 与 `set_dns.rs`，唯一 network 端点常量是 `NETWORK_SET_DNS_ENDPOINT`；`get_dns`/`read_dns` 在 `nyanpasu_ipc/src` 与 `crates/nyanpasu-service-runtime/src` **零命中**。

| 路线                                            | 评价                                                                           |
| ----------------------------------------------- | ------------------------------------------------------------------------------ |
| ①承认 Service 写不可验证                        | 诚实但**放弃**了 Service 模式的全部验证能力，而 Service 是用户实际部署的那条路 |
| ②上游加读端点                                   | **否**。R0 仍未合并，这会是叠在其上的**第二个上游 PR**                         |
| **③读一律走本地 `networksetup -getdnsservers`** | **采纳**                                                                       |

**为什么③在 Service 模式下成立——已从实现确立，不是从名字推断：**

`get_default_network_hardware_port()` 的脚本（`crates/nyanpasu-utils/src/network/scripts/find-macos-default-device-port.sh`）实际执行：

```bash
DEFAULT_NETWORK_INTERFACE=$(route get default | grep interface | awk '{print $2}')
networksetup -listallhardwareports | awk ... # 把 BSD 设备名映射为硬件端口名
```

**两个输入都是系统级的**：`route get default` 读**内核路由表**（每机一份，不随调用者身份或会话变化）；`-listallhardwareports` 读系统网络配置。**daemon 调的是同一个函数**（`crates/nyanpasu-service-runtime/src/server/routing/network.rs:26`）。

> **这正面回答了审查者「同一 resolver 只保证同一算法、不保证同一观察」的质疑**：观察结果派生自内核路由表，而内核路由表**不是**按会话或身份分区的。因此 app 与 daemon 在**同一时刻**必然观察到同一默认设备。
>
> **R1 因此只剩时间维度**（两次调用之间默认设备变了），**不含执行上下文维度**。

**附带收益：读不依赖 daemon**——daemon 挂了、被 stop 了、被 uninstall 了，读照样能用。这是 §4.4 死锁序列的出路。

```rust
// core/actor/dns.rs —— 整个文件 #[cfg(target_os = "macos")]

/// 覆写目标：**本地解析到的硬件端口名**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DnsTarget(pub String);

#[derive(Debug, thiserror::Error)]
pub(crate) enum DnsPortError {
    /// Service 写之前发现默认设备已不是记录的那个。**拒绝写，不猜。**
    #[error("default device drifted: recorded {recorded}, observed {observed}")]
    TargetDrifted { recorded: String, observed: String },
    /// 该适配器在结构上无法定向此目标（Service 无法写非默认设备）。
    #[error("this backend cannot address {0:?}")]
    NotAddressable(DnsTarget),
    #[error(transparent)]
    Io(#[from] anyhow::Error),
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait MacosDnsPort: Send + Sync + 'static {
    /// 解析当前默认硬件端口并读其 DNS。**总是本地执行、总是有界**（§3.2）。
    async fn read_default(&self) -> Result<(DnsTarget, Option<Vec<IpAddr>>), DnsPortError>;
    /// 读指定目标。**总是本地执行**，daemon 不在也能用。
    async fn read(&self, target: &DnsTarget) -> Result<Option<Vec<IpAddr>>, DnsPortError>;
    /// 写指定目标。
    async fn write(&self, target: &DnsTarget, dns: Option<Vec<IpAddr>>) -> Result<(), DnsPortError>;
    /// **本适配器此刻能否定向该目标**——候选集排序用（§4.4）。
    /// Local：恒 true。Service：仅当 target == 当前默认设备。
    async fn can_address(&self, target: &DnsTarget) -> bool;
}
```

|               | `LocalMacosDns`                                               | `ServiceMacosDns`                                                   |
| ------------- | ------------------------------------------------------------- | ------------------------------------------------------------------- |
| 读            | 本地 `networksetup`                                           | **同左**（共用 `LocalDnsReader`）                                   |
| 写            | `networksetup -setdnsservers <target> ..`，**可定向任意设备** | 先本地解析默认设备；`≠ target` → `Err(TargetDrifted)`；相等才发 IPC |
| `can_address` | `true`                                                        | `target == 当前默认设备`                                            |
| 权限          | 需 admin 组（F53）；UI 进程通常**没有**                       | daemon 持有                                                         |

### 3.2 actor 内 DNS I/O 必须有界（PR-5d 移交）

> v4 的 BLOCKING：`QUIESCE_BUDGET` 只界住 facade 许可；**真正会挂死的是 actor 持有的 DNS await**。ractor 逐条串行处理消息，一个卡在 DNS 命令里的处理器会让排队的 `Shutdown` 永远排不上。`kill_on_drop` 没有 timeout 等于没有——**没有东西去 drop 那个 future**。

**规则：actor 内每一次外部 I/O 都有显式有限预算。**

| 调用                                   | 预算                 | 超时后                                                    |
| -------------------------------------- | -------------------- | --------------------------------------------------------- |
| `read_default` / `read`（子进程）      | `DNS_READ_BUDGET`    | `Err(Io)` → 按四态①处置                                   |
| Local `write`（子进程）                | `DNS_WRITE_BUDGET`   | `Err(Io)` → **受 §0 原则管辖**：记账保留、标 `unverified` |
| Service `write`（IPC）                 | `DNS_IPC_BUDGET`     | 同上                                                      |
| 默认设备解析（脚本，**当前是同步的**） | 同 `DNS_READ_BUDGET` | 须先改为 `tokio::process` 异步调用，否则 timeout 包不住   |

**三个子进程调用一律 `.kill_on_drop(true)`。** 预算全部**实测**取上界，依据写进实施报告。

---

## 4. 状态机与生命周期

### 4.1 状态与消息

```rust
// CoreActorState 新增
#[cfg(target_os = "macos")] pub(crate) dns: Option<DnsOverride>,
#[cfg(target_os = "macos")] pub(crate) dns_ports: DnsPorts,

pub(crate) struct DnsPorts { local: Arc<dyn MacosDnsPort>, service: Arc<dyn MacosDnsPort> }

pub(crate) struct DnsOverride {
    target: DnsTarget,
    /// 覆写**之前**的原始 DNS。`None` 是合法值（原本就没配）。
    previous: Option<Vec<IpAddr>>,
    /// 建立覆写时的后端身份。**仅用于诊断**，不参与候选选择（§4.4）。
    origin: RunType,
    /// 尚未被回读证实。守卫**保持 active**。
    unverified: bool,
}

SetTunDns {
    operation: OperationId,
    /// Some(ip) = 开 TUN；None = 关 TUN 恢复原值。
    /// TUN 设备 IP 由 client 侧从 clash config 算好传入——**actor 不读配置全局**。
    desired: Option<IpAddr>,
    reply: RpcReplyPort<Result<DnsOutcome, CoreActorError>>,
}

pub(crate) enum DnsOutcome {
    Applied, AppliedUnverified,
    /// **写返回 Ok，但回读证明 desired 没生效**（v4 缺这一态，审查者点名）。
    AppliedNotObserved,
    NoChange, Restored, RestoredUnverified,
}

// CoreActorError 新增
CoreNotRunning, DnsRestoreFailed,
```

**注入路径**：`ClientSetupArgs`（`#[cfg(target_os="macos")] dns_ports`）→ `CoreClientArgs` → `CoreClient::spawn` → `CoreActorArgs` → `CoreActorState`。**与 `requests`/`degradation` 同一条既有路径。**

**不扩 `CoreRequest`**：它是 run/check/apply 共用的**全平台**进程描述。

### 4.2 施加：先记账，再写

```text
① read_default() → (target, previous)     ← 拿不到就直接 Err，什么都没做，安全
② state.dns = Some(DnsOverride{ target, previous, origin: state.mode, unverified: true })
                                           ← **在写之前记账**
③ write(&target, Some(tun_ip))             ← 成败都不改变②记下的恢复意图
④ read(&target) 三分消歧（§4.3）
```

### 4.3 施加后的回读消歧 —— **四种结果，v4 只定义了三种**

| 回读结果          | ③ 的返回 | 处置                                     | 返回值                                                                                            |
| ----------------- | -------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `== desired`      | `Ok`     | `unverified = false`                     | `Applied`                                                                                         |
| `== desired`      | `Err`    | `unverified = false`（**写其实生效了**） | `AppliedUnverified` + 降级 `macos_dns_write_reported_failure`                                     |
| `== previous`     | `Err`    | **移除守卫**（确无可恢复）               | ③ 的 `Err`                                                                                        |
| **`== previous`** | **`Ok`** | **移除守卫**                             | **`AppliedNotObserved`**（合成错误：写声称成功但未被观察到）+ 降级 `macos_dns_write_not_observed` |
| 其它 / 读失败     | 任意     | **保留守卫**、`unverified` 维持          | `AppliedUnverified` + 降级 `macos_dns_readback_failed`                                            |

> **第四行是 v4 的空洞**：`write` 返回 `Ok` 而回读证明 desired 没生效时，**没有 `Err` 可返回**。定义合成结果 `AppliedNotObserved`，并由 T-DNS-30 单独钉住。

### 4.4 恢复：候选集完整、由回读推进

> **v4 的两个缺陷**：候选集是固定二元组 `[当前 mode 适配器, Local]`，**当 `state.mode == Local` 时把仍然活着的 Service 排除在外**；且推进由「写返回 `Ok`」驱动——**违反 §0 原则**。
>
> **另需更正一处事实错误**：v4 称 Stop 序列在恢复前 `state.mode` 已收敛为 Local。**不对**——`SetBackend` 的恢复发生在 `replace_backend` **之前**，而 `replace_backend` 才改 `self.mode`（`core/actor/mod.rs:282`）。**该序列从 Service 开始，Local 是回退项。**

**算法及其后置条件：**

```text
restore(target, previous) -> Result<(), DnsRestoreFailed>

  候选集 = 所有适配器 { local, service }，按**定向安全性**排序：
    1. can_address(target) == true 者优先
    2. 同级时，能精确定向者优先（Local 可定向任意设备；Service 只能写"当前默认"）
    ⇒ 确定性顺序，去重

  for cand in 候选集:
      let _ = cand.write(target, previous).await;   // **返回值只用于诊断，不用于推进**
      match cand.read(target).await {               // ← 推进的唯一依据
          Ok(v) if v ≅ previous => return Ok(()),   // 集合相等即证实
          _                      => continue,
      }
  return Err(DnsRestoreFailed)                      // 候选穷尽且**没有任何一次回读证实**

后置条件（实施与审查都按这句核对）：
  **候选推进与终止只由「回读证实」决定，绝不由写的返回值决定。**
  因此：写 Err 也要回读；写 Ok 也要回读；穷尽定义为「所有候选都试过且都未被证实」。
```

**守卫只在证实后才清**：`state.dns = None` 只出现在 `return Ok(())` 那一条路径上。

### 4.5 漂移 + Local 不可用 = 具名残留，不塞进 R1

> **v4 把它归进 R1，错了**（审查者点名）：R1 是「预检到 daemon 解析之间的 TOCTOU」；这里**漂移已被成功检测**，问题是**没有任何适配器还能定向设备 A**。

**序列**：Service 在设备 A 建立覆写 → 默认设备变为 B → Stop 触发恢复 → Service `TargetDrifted`（A 已非默认）→ Local 因 UI 进程无 admin 权限失败 → §4.7 裁定 Stop 继续 → **设备 A 仍指向 TUN 地址，背后没有核**。A 若再次成为默认，解析即坏。重启服务也没用——Service 仍无法定向非默认的 A。

**裁定：不改 Stop 策略**（为一次无法恢复的 DNS 就让用户停不掉服务，代价不成比例，且会把人锁死），**改为具名残留 + 可执行的用户指引**。

|                                        |                                                                                                                                |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| **残留编号**                           | **R-C3-1**                                                                                                                     |
| **触发条件**                           | Service 模式建立覆写后默认设备变更，且 Local 写不可用（非 admin 账户）                                                         |
| **后果**                               | 设备 A 的 DNS 永久指向 TUN 地址；A 再次成为默认时解析失败                                                                      |
| **检测**                               | 恢复穷尽候选后返回 `DnsRestoreFailed`，**降级消息必须带上设备名 A 与原值**                                                     |
| **用户可见指引**（措辞是功能的一部分） | 「设备 **A** 的 DNS 仍指向 <tun_ip>。请在「系统设置 → 网络 → A → DNS」中手动改回 <previous 或「自动」>，或以管理员身份重试。」 |
| **owner / 移除条件**                   | 本表。移除条件 = ①`NetworkSetDnsReq` 支持定向设备（上游），**或** ②UI 进程获得 DNS 写权限                                      |

### 4.6 恢复的三个触发点与 `Drop`

**主路径**（`Stop` / `Shutdown` / `SetBackend`）：处理器内**显式 `await` 恢复**，在后端动作与 reply **之前**完成。

**`Drop`：只记 `tracing::error!`，措辞按不变量破坏写，不尝试任何恢复。**

> **为什么不做「尽力而为的同步 Drop」**：Service 侧同步做不到、Local 侧能做——**那半个兜底恰好在开发者最常用的模式下生效**，会系统性地把主路径 bug 藏到用户实际部署的模式才暴露。**「在你测得到的地方生效、在你测不到的地方失效」的兜底是反向选择。**

**不覆盖强杀**（SIGKILL / 任务管理器）——如实写明，兜底属 PR-6（R-C3-2）。

### 4.7 六个控制入口：控制动作前先拆 DNS

**规则：六个入口，在调用外部控制动作之前，都先在同一守卫内 `await` 拆除。**

**为什么是六个而不是四个**：要把 install 排除在外，就得证明 `nyanpasu-service install` 在已有 daemon 在跑时不会把它换掉/重启——**核不了，且是「不会去做某事」型断言**。铺到六个的代价是无活跃覆写时一次 `state.dns.is_none()` 的 no-op。**用一次 no-op 换掉一条无法验证的前提。因此没有需要点名的例外。**

| 场景          | 处置                        | 理由                                                                                                  |
| ------------- | --------------------------- | ----------------------------------------------------------------------------------------------------- |
| **uninstall** | **中止卸载** + 用户可见错误 | 卸载**不可逆**；拆 DNS 失败说明我们**连自己的写都验证不了**，此时执行不可逆操作是把已知的不确定性固化 |
| **其余五个**  | **继续，产出 degradation**  | 服务可再启动、通道会回来，泄漏**通常**可恢复（**例外见 R-C3-1**）                                     |

**中止 uninstall 的错误必须说清三件事**：做了什么（没有卸载）、为什么、怎么办（重试；或先手动关 TUN 再卸载）。

### 4.8 重施加：单一 owner，按「结果状态」而非「前序命令结果」

> **v4 两个缺陷**：①§3.2 与 §4.5 指定了**不同的 owner**；②重施加被挂在「控制动作成功」分支上——但**控制失败后 reconcile 仍可能成功起一个 Local 核**，而 TUN 仍然开着，DNS 却在控制动作前被拆掉了 → **TUN 在跑但没有 DNS 覆写**。

**裁定：owner 唯一 = `CoreModeReconciler`**（它持守卫、知道收敛后的 mode、能读 clash config）。facade 不做重施加。

**触发判据 = 收敛后的事实，不是前序命令的返回值：**

```text
reconcile_with 的末尾（仍持守卫）：
  if state.running.is_some() && desired_tun_enabled {
      SetTunDns(Some(tun_ip))          ← 无论前面的控制动作是 Ok 还是 Err
  }
```

**因此 PR-5d §4.4 处置表的每一行都要补一列「DNS 重施加」**，且该列的取值只取决于上式，不取决于该行的控制结果。失败以 `macos_dns_reapply_failed` 降级呈现，**不改变该行的返回值**。

`desired`（TUN 开关、TUN 设备 IP）由 `CoreModeReconciler` 新增的 `clash_config: ClashConfigClient` 字段读取（`NyanpasuClientInner.clash_config`，`client/mod.rs:247`）。**actor 侧一行配置全局都不读。**

### 4.9 `SetTunDns` 准入

**携带 `OperationId`，由 `validate_operation`（`core/actor/mod.rs:185-190`）校验。**

**为什么不是「自己取门」**：`OperationGate::acquire` 在门被占时塞进 `waiters`（`gate.rs:25-28`），**只有另一条 `ReleaseOperation` 消息被处理时才发放**。ractor 逐条串行处理，在 `handle()` 里 await 发放**永远等不到**——**构造性死锁**。

| #   | 场景                                                     | 规则                      | 构造                                                                                           |
| --- | -------------------------------------------------------- | ------------------------- | ---------------------------------------------------------------------------------------------- |
| A   | `Shutdown` 后到达                                        | `Err(ShuttingDown)`       | `operation.shutdown()` 清 `active` → 恒 `StaleOperation`；`backend.take()` → `ShuttingDown`    |
| B1  | `SetTunDns` 先取得许可                                   | `Stop` 的恢复晚于设置     | `OperationGate` FIFO                                                                           |
| B2  | `Stop` 先取得许可，晚到的 `SetTunDns(Some)` 持**新守卫** | **`Err(CoreNotRunning)`** | 准入检查 `state.running.is_some()`，**仅对 `Some(..)` 生效**——`None`（拆除）在核已停时仍须允许 |

> **注意**：`state.running` 有**多个**清除点（`Stop` `:532`、`replace_backend` `:268`、`Shutdown` `:605`，以及 `commit()` 观察到 `Stopped` 时 `:224-227`）。这**加强** B2，但使「删掉某一行就会红」的测试判据失效——见 §6。

---

## 5. 四态读与 R6

### 5.1 四态

| #   | 条件                                                     | 结果                   |
| --- | -------------------------------------------------------- | ---------------------- |
| 1   | 退出码非零                                               | **`Err`**              |
| 2   | 输出匹配「无 DNS 服务器」那句                            | `Ok(None)`             |
| 3   | 输出**全部**解析为 IP                                    | `Ok(Some(..))`         |
| 4   | **以上都不是**（含**混合输出**：部分 IP + 无法识别的行） | **`Err`**，不是 `None` |

> **第 3 条必须是「全部」**：审查者点名——用 `filter_map` 做部分解析会把「一个 IP + 一行诊断文字」当成成功。**混合输出属四态④。**

**Local 写实现**（`networksetup -setdnsservers`）直接 argv、设 `LC_ALL=C`、`kill_on_drop(true)`、**检查 `output.status`**。不复用上游 `nyanpasu_utils::network::macos::{set_dns, get_dns}`：设备名被文本拼进 bash 且 `$1` 未加引号（含空格的硬件端口名今天就是坏的，兼具注入面），读路径不看退出码。`$2` 是**本该**分词的，所以「两边加引号」是错的修法，**直接 argv 才对**。

### 5.2 R6：这是**功能缺口**，不是验证缺口

> **v4 严重低估了它**（审查者点名，leader 确认）：没有分支②，`read_default()` 在「无显式 DNS」的机器上返回 `Err` → **`previous` 从未被捕获 → 根本不会建立覆写 → TUN DNS 完全无法启用**。
>
> **而这很可能是多数情形**：DHCP 配置的 Mac 通常**没有**显式设置的 DNS 服务器，正是 `-getdnsservers` 打印那句提示的场景。

**三条路径的现状：**

| 路径                                                          | 状态                                                  |
| ------------------------------------------------------------- | ----------------------------------------------------- |
| ①真机捕获 fixture，得到确切文案                               | **首选，且被裁定为硬前置**（见下）                    |
| ②臆造一个字面串                                               | **禁止**。生产与测试共用臆造事实 = 自证；失败模式静默 |
| ③改用「退出码 0 + 无可解析 IP ⇒ 无服务器」判别（leader 提出） | **本轮未能采用——依据未确立**（见下）                  |

**关于路径③**：它确实不是原 bug 的翻版（原 bug 完全无视退出码，把**所有**情形包括失败都塌成 `None`；③保留失败在①）。但它成立的前提是 **`networksetup` 不会「退出码 0 且打印错误」**。**我未能从任何可引用来源确立这一点**——两轮定向检索（man page 转录、Stack Exchange、GitHub 脚本）**均无正面陈述**。按本项目纪律，**不确立即不采用**。

> 若将来有人确立了该行为（例如在真机上对无效设备名实测 `$?`），路径③可以取代①，**并且届时不需要 fixture**。本节保留该出口。

**裁定（Exit 判据据此决定，不递延）：**

> **真机捕获 fixture 是 PR-5e 的硬前置。** 拿不到 fixture，则 C3 的「启用」路径在**多数用户机器上不可用**——那不是一条可以记账带过的残留，是功能不成立。**因此：无 fixture 则 PR-5e 不进入实施。**

fixture 要求：用**新调用形态**（直调 `networksetup`、`LC_ALL=C`）捕获，**不能用上游脚本**（其 `echo $RES` 会把换行压成空格）；记录 provenance（macOS 版本、locale、完整命令行）；**生产匹配器独立于 fixture 文件**（不 `include_str!` 同一份），T-DNS-09 读 fixture。

---

## 6. 定序保证表

> **表与散文双向对齐**：正文每一条「X 之后 Y 一定已发生 / X 不会发生」都在此有行；每行回指正文小节。**措辞不得强于机制。**

| 断言                                                          | **构造**                                                          | 正文出处 | 测试                    |
| ------------------------------------------------------------- | ----------------------------------------------------------------- | -------- | ----------------------- |
| 恢复发生在后端动作与 reply 之前                               | 处理器内 `await` 点的源码顺序                                     | §4.6     | T-DNS-02/03             |
| 拆 DNS 发生在**六个**控制动作之前                             | 同一守卫内的调用点顺序                                            | §4.7     | T-DNS-05/06/17/18/26/27 |
| **写之前先记账**                                              | `state.dns = Some(..)` **早于** `write()` 的源码顺序              | §4.2     | T-DNS-19                |
| **回读三分消歧覆盖全部四种组合**                              | §4.3 表的四个分支                                                 | §4.3     | T-DNS-19/28/**30**      |
| **候选推进只由回读证实驱动**                                  | §4.4 循环的后置条件：`write` 返回值只入日志                       | §4.4     | **T-DNS-31**            |
| **候选集含全部适配器**                                        | §4.4 候选集构造遍历 `{local, service}`                            | §4.4     | **T-DNS-16**            |
| 守卫只在证实后才清                                            | `state.dns = None` 只在 `return Ok(())` 路径                      | §4.4     | T-DNS-20                |
| **Service 写前校验默认设备**                                  | `ServiceMacosDns::write` 的比对分支                               | §3.1     | T-DNS-14                |
| **actor 内每次外部 I/O 有限**                                 | 四处显式 `timeout` + `kill_on_drop`                               | §3.2     | **T-DNS-32**            |
| 设备变更时先恢复旧设备再取新快照                              | `SetTunDns(Some)` 开头 `read_default()` 与 `override.target` 比对 | §4.1     | T-DNS-23                |
| **重施加由「收敛后 running + TUN 期望」驱动，非前序命令结果** | `reconcile_with` 末尾的条件式                                     | §4.8     | **T-DNS-33**            |
| `Shutdown` 后不再有 `SetTunDns` 生效                          | `operation.shutdown()` 清 `active`；`backend.take()`              | §4.9     | T-DNS-07                |
| `Stop` 先取得许可时晚到的 `SetTunDns(Some)` 失败              | 准入检查 `state.running.is_some()`                                | §4.9     | T-DNS-13                |

---

## 7. 测试矩阵（**第三列必须真能红**）

| ID                      | 断言                                                                                                                           | **删掉哪行会让它红**                                                                                                                              |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-DNS-01                | `SetTunDns{Some}` → `write` 被调，guard 记为 active                                                                            | 处理器里 `port.write()`                                                                                                                           |
| T-DNS-02                | `Stop` 时恢复在 `backend.stop()` **之前**                                                                                      | `Stop` 臂 `restore().await`（对调即红）                                                                                                           |
| T-DNS-03                | `Shutdown` 时恢复在后端动作与 **reply** 之前                                                                                   | `Shutdown` 臂的 `restore().await`                                                                                                                 |
| T-DNS-04                | 回读比对真的会发现不一致                                                                                                       | 适配器里集合比较那行                                                                                                                              |
| T-DNS-05/06/17/18/26/27 | 六个入口各自：拆 DNS 在控制动作之前（**六条独立，不合并**）                                                                    | 各自 facade 方法的拆 DNS 行                                                                                                                       |
| T-DNS-07                | `Shutdown` 把等待中的取门请求以 `ShuttingDown` 排空——**直接持 `OperationGate`** 观察 waiter reply                              | `gate.rs:57-59` 的 `waiters.drain(..)`。**不能走 actor 集成测试**（state 析构会 drop reply port，删掉 drain 也不红）                              |
| T-DNS-13                | `Stop` 先取得许可 → 晚到的持新守卫的 `SetTunDns(Some)` → `Err(CoreNotRunning)`                                                 | **`SetTunDns` 臂的 `state.running.is_some()` 准入检查**（不能点 `Stop` 里的赋值——多清除点）                                                       |
| T-DNS-14                | `ServiceMacosDns::write` 漂移时 → `Err(TargetDrifted)` **且未发 IPC**                                                          | 写前比对分支                                                                                                                                      |
| **T-DNS-16**            | **候选集完整**：`state.mode == Local`、Local 写失败、**Service daemon 仍活着且 target 就是当前默认** → 经 **Service** 成功恢复 | 候选集构造里遍历 `{local, service}` 的那行（退回固定二元组即红）。**v4 版本空转**——那时 mode=Local 已使 Local 成为首选，删掉 Local 回退不影响结果 |
| T-DNS-19                | 写 `Err` 但回读显示 desired 生效 → 守卫**保留**、`unverified`、降级                                                            | `state.dns = Some(..)` 早于 `write()` 的顺序                                                                                                      |
| T-DNS-28                | 写 `Err` 且回读显示仍是 previous → **移除**守卫并返回 `Err`                                                                    | 回读消歧的 `== previous` 分支                                                                                                                     |
| **T-DNS-30**            | **写 `Ok` 但回读显示仍是 previous** → `AppliedNotObserved` + 降级（**v4 无此结果**）                                           | §4.3 第四行的合成结果分支                                                                                                                         |
| T-DNS-20                | 恢复回读校验失败 → 守卫**不清**                                                                                                | `state.dns = None` 前的校验条件                                                                                                                   |
| T-DNS-21                | 重复 `SetTunDns(Some(a))`→`(Some(b))` → 恢复得到**最初**原值                                                                   | 「已有覆写则不重新快照」那行                                                                                                                      |
| T-DNS-22                | 原值 `None` 的活跃覆写与「无覆写」可区分                                                                                       | 两层 `Option` 的外层判断                                                                                                                          |
| T-DNS-23                | 设备变更：先 `write(a, previous)` 再对 b 取快照                                                                                | target 比较那行                                                                                                                                   |
| T-DNS-24                | **所有候选写都 `Err` 但某次回读证实了 previous** → **返回 `Ok`**（不是 `DnsRestoreFailed`）                                    | §4.4 循环里「读证实即 `return Ok`」那行。**v4 的 T-DNS-24 把「全 `Err` ⇒ 失败」固化成测试，正是要推翻的行为**                                     |
| **T-DNS-31**            | **写 `Ok` 但回读未证实 → 继续下一候选**（不提前成功）                                                                          | 循环里「以回读结果决定 continue」那行                                                                                                             |
| T-DNS-25                | `Drop` 时守卫仍 active → 记 `error!` 且**不发起任何恢复**（断言适配器零调用）                                                  | `Drop` 里的 `tracing::error!` + 反向断言                                                                                                          |
| **T-DNS-32**            | **挂死的 DNS 子进程**：actor 处理器在 `DNS_READ_BUDGET` 内返回，随后排队的 `Shutdown` 能被处理                                 | 读实现里的 `timeout(..)`（去掉即 actor 永久卡住，`Shutdown` 超时）                                                                                |
| **T-DNS-33**            | **控制动作 `Err` + reconcile 成功起了 Local 核 + TUN 仍开** → **仍然重施加 DNS**                                               | `reconcile_with` 末尾的 `state.running.is_some() && desired_tun_enabled` 条件（挂回「控制成功」分支即红）                                         |
| T-DNS-08                | 四态①：非零退出 → `Err`。fixture = **非零退出 + 可解析 IP 输出**                                                               | `if !output.status.success()`                                                                                                                     |
| T-DNS-09                | 四态②：「无 DNS 服务器」→ `Ok(None)`——**读真机 fixture**（§5.2）                                                               | 匹配该文案那行                                                                                                                                    |
| T-DNS-10                | 四态③：全部可解析 → `Ok(Some(..))`                                                                                             | 解析 IP 列表那行                                                                                                                                  |
| T-DNS-11                | 四态④：不认识的输出 → `Err`                                                                                                    | 兜底分支（改成 `None` 即红）                                                                                                                      |
| **T-DNS-34**            | 四态④之**混合输出**：一个 IP + 一行诊断文字 → `Err`                                                                            | 「**全部**元素都须解析成功」那个判断（改成 `filter_map` 即红）                                                                                    |

---

## 8. 契约归属

| 契约                                | 由谁保证                | 为什么可验证                                                 |
| ----------------------------------- | ----------------------- | ------------------------------------------------------------ |
| 非 macOS 不存在 DNS 抽象            | **cfg / 类型**          | 非 macOS 上引用它编译不过                                    |
| **恢复推进不看写的返回值**          | **算法后置条件 + 测试** | T-DNS-24/31；后置条件写在 §4.4 算法旁，本地可核              |
| Service 写不会打到漂移后的设备      | **写前比对 + 返回值**   | `Err(TargetDrifted)` 可观测，T-DNS-14。**残余 TOCTOU = R1**  |
| **actor 不会被 DNS I/O 无限期卡住** | **四处显式 timeout**    | T-DNS-32                                                     |
| 「核已停时不建立覆写」              | **运行时准入 + 测试**   | T-DNS-13                                                     |
| DNS 路径不回头读全局                | **ledger 门禁**         | `core/actor/dns.rs` 的 `Config::*()` / `::global()` 计数恒 0 |
| 顺序类契约                          | **测试**                | 控制流性质                                                   |

---

## 9. 风险与残留

| #          | 残留                                                                                                                                      | 性质                                                                         | owner / 移除条件                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| **R1**     | Service 模式下默认设备在**我们校验与 daemon 解析之间**变化 → 写落到新接口。**仅时间维度**——执行上下文维度已排除（§3.1：内核路由表系统级） | **既有**（今天同样）。5d/5e 不修，但从「不可检测」改善为「可检测、窗口极窄」 | 移除条件 = `NetworkSetDnsReq` 支持设备字段（上游 PR）                          |
| **R1b**    | **Local 施加**也有「读默认 → 写」的窗口：若默认设备在快照后变了，Local 会**照旧写记录的设备**（审查者点名 v4 只写了 Service 侧）          | 同 R1                                                                        | 同 R1；或在 Local 写前也加一次默认设备比对（可在本 PR 做，**成本低，建议做**） |
| **R-C3-1** | 漂移已检测 + Local 不可用 → 设备 A 永久残留（§4.5）                                                                                       | **具名残留**，不属 R1                                                        | §4.5 表；含用户可见指引                                                        |
| **R-C3-2** | 强杀后 DNS 覆写残留                                                                                                                       | **既有**                                                                     | PR-6：启动时检测并清理                                                         |
| **R6**     | 无真机 fixture 则四态②不可实现 → **启用路径在多数机器上不可用**                                                                           | **功能缺口**，非验证缺口                                                     | **§5.2 裁定：无 fixture 则本 PR 不进入实施**                                   |

> ### 关于「非管理员账户可能一直静默失效」
>
> `networksetup -setdnsservers` 至少需要 admin 组身份（man page + SO 双源，**不是从代码形状推断**）→ 但代码**不提权**（`osascript` 不带 `administrator privileges`，全 crate 零命中）→ 失败被 `let _ =` 吞掉 → **没有任何观测点**。所以「这个功能在非管理员账户上可能从来就没工作过」**不是推测，而是当前代码结构下必然无法被发现的一类失效**。
>
> **加上退出码检查与回读校验之后它会第一次变得可见。这不是我们引入的回归，但我们会是发现它的人。**
>
> **判别方法**：在本 PR 之前的版本上用同一账户手动跑一次 `networksetup -setdnsservers`。

---

## 10. Exit 判据

| 要求                                                                                         | 验证                                                                                                                                 |
| -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| **真机 fixture 已取得**（§5.2 硬前置）                                                       | fixture 文件 + provenance 存在；**否则本 PR 不进入实施**                                                                             |
| 四态读全覆盖（含混合输出）                                                                   | T-DNS-08/09/10/11/34                                                                                                                 |
| **恢复由回读证实推进**                                                                       | T-DNS-24/31                                                                                                                          |
| **候选集完整**                                                                               | T-DNS-16                                                                                                                             |
| **回读四种组合齐备**                                                                         | T-DNS-19/28/30                                                                                                                       |
| **actor 内 DNS I/O 有界**                                                                    | T-DNS-32                                                                                                                             |
| **重施加按结果状态触发**                                                                     | T-DNS-33                                                                                                                             |
| 六个入口都在控制动作前拆 DNS                                                                 | T-DNS-05/06/17/18/26/27（六条独立）                                                                                                  |
| `MacosDnsGuard` 与 start/stop/backend-switch 保序                                            | T-DNS-02/03/13/14/23                                                                                                                 |
| 非 macOS 不加空抽象                                                                          | cfg 门控                                                                                                                             |
| bindings diff 为空                                                                           | `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`                                                                     |
| **R1/R1b/R-C3-1/R-C3-2 四条残留**逐条出现在 PR 描述里，**R-C3-1 含用户指引原文**             | 文本核对                                                                                                                             |
| **对 `design.md:337` 的有意偏离**（DNS 兄弟端口而非 `CoreBackend::Service`）出现在 PR 描述里 | 文本核对；`design.md` **本身不得修改**                                                                                               |
| **smoke 3**（macOS TUN/DNS）                                                                 | **未在本地验证且不可由 CI 覆盖**——托管 runner 的能力边界（TUN 需签名扩展 + root），加 job 加 runner 都无效；结论进 PR 描述与发布说明 |

> ### 与 `design.md:337` 的偏离
>
> spec 写「Service 模式需要提权时由 `CoreBackend::Service` 调 IPC set_dns」，本设计用**独立兄弟端口**。
>
> **机制理由**：恢复所需的适配器**不总是当前后端那个**——§4.4 的候选集会在 Service 不可定向时回退到 Local，而 `Shutdown` 之后 `state.backend` 已是 `None`（`core/actor/mod.rs:606`，此后 `backend()` 返回 `ShuttingDown`）。**方法长在 `CoreBackend` 上则「用 Local 恢复一个 Service 时期的覆写」在类型上无从表达。**
>
> **洁净性理由**：`CoreBackend` 是全平台核进程生命周期枚举，挂 macOS-only 的 DNS 方法与拒绝往 `CoreRequest` 塞 TUN 字段同理。
>
> 两条**分开标注**：前者机制、后者洁净性。（v3 曾用 `replace_backend` 的 `take()` 论证，**已撤回**——恢复排在 `replace_backend` 之前，那时后端还活着。）
