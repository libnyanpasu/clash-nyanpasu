# PR-5e 实施计划 — macOS DNS 生命周期（C3）

**日期：** 2026-08-03
**版本：** v2（v1 判 37/100 后重建。七条 BLOCKING 全部改判：**接缝契约按 PR-5d v7 §4.7 落地**、**施加表改为两两不交并给 `NoChange` 落实产出分支**、**重复施加与设备变更写进算法**、**恢复加归属判定**、**异步解析器归本 crate**、**fixture 硬前置改为 Phase 0 CI 采集**、**提权事实更正并作出裁定**）
**分支基线：** `refactor/core-manager-actor` @ **`049bd30dc`**
**前置：** **PR-5d（C2）必须先落地**——见 §1.1
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/` 下 `task.md` 卡 C3 + **`design.md` §9 `:337`**（macOS DNS 段）
**姊妹计划：** `docs/superpowers/plans/2026-08-03-pr5d-run-mode.md`（C2，**已 v7**）。其 §4.7 是**已定契约**，本计划按它建，不再重议
**平台：** Windows 11 / PowerShell（**macOS 路径无法本地验证**——但这不等于无法经验取得，见 §5.2）

---

## 0. 本计划的第一原则

> **错误通道报告的是调用的结果，永远不是副作用的缺席。**

一次写调用返回 `Err`，只说明**这次调用报告了失败**，不说明外部状态没被改动——daemon 可能已改完 DNS 才丢了响应，本地命令可能改完才非零退出，超时更是连"调用是否返回过"都不知道。

**它的对偶同样重要，v1 没有写出来：**

> **能断言副作用缺席的构造只有一种——调用从未被发起。**

本计划有两处依赖这条对偶（§4.2 的 ①' 与 R1b 回退、§4.4 的归属判定），**两处都必须在写调用之前完成判断**；一旦写已发出，无论返回什么都不能再回到"什么都没发生"。

---

## 0.0 v2 修订记录（逐条对应 v1 的判定）

| v1 的问题                                                    | v2 的处置                                                                                                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **BLOCKING 1** 接缝三槽位与 5d 冲突（v1 写作时 5d 尚是 v6）  | 5d v7 已改判并采纳本计划的 S2/S3 位置。**§4.6/§4.7/§4.8 按 v7 §4.7 重写**，并写入 v7 移交的三项义务（预算之和、清理可被跳过、R-C2-7）      |
| **BLOCKING 2** 施加表 `== desired` 与 `== previous` 行可重叠 | 新增**不变式 I-DNS-1**（§4.2 ①'）：`previous ≅ desired` 时不建立覆写，返回 `NoChange`。五行表在该不变式下两两不交；**洗白路径记为 R-C3-3** |
| **BLOCKING 3** 重复施加 / 设备变更只在测试里，算法里没有     | §4.2 改写为按 `state.dns` 分三情形 (a)/(b)/(c)，规则进算法；§6 对应行改指 §4.2                                                             |
| **BLOCKING 4** 恢复无归属检查                                | §4.4 加**循环外一次归属判定**：`current ∉ {previous, applied}` → **零写**；`DnsOverride` 新增 `applied` 字段                               |
| **BLOCKING 5** 异步解析器无实现归属                          | §5.1 定义 `MacosCommandRunner` + `LocalDnsReader`，`resolve_default()` 落在**本 crate**，不动 submodule                                    |
| **BLOCKING 6** fixture 硬前置                                | §5.2 改为 **Phase 0 CI 采集**（`ci.yml` 的 macOS runner 或 `deps-build-macos.yaml` dispatch）；**臆造字面串的禁令保留**                    |
| **BLOCKING 7** §9 的"全 crate 零提权"是假的                  | §9.1 新增：逐条列出两处提权及其锚点，**裁定 DNS 写路径不提权**并给出三条理由；R-C3-1 移除条件②重写；新增 **R-C3-5**                        |
| QUALITY 1 §4.4 后置条件字面为假、`let _ =` 谎称"用于诊断"    | §4.4 后置条件改写为三句式；写的返回值**记日志**                                                                                            |
| QUALITY 2/3 T-DNS-32/33 空转                                 | 32 的接缝下沉到 `MacosCommandRunner` 并改为观察 actor 消息循环恢复；33 走真实 facade→reconciler→actor 链路                                 |
| QUALITY 4 T-DNS-19 只断言终态                                | 改为**写挂死至超时**场景，经后续一次恢复写观察记账（见 §7）                                                                                |
| QUALITY 5 T-DNS-07 与 5d T-GATE-01 完全重复                  | **删除**，§6 该行 owner 改标 PR-5d；T-DNS-32 加与 5d T-SD-05 的分界说明                                                                    |
| QUALITY 7 §6 表不全且有过强行                                | 重建为 28 行，补齐缺行；三处过强措辞逐条弱化                                                                                               |
| QUALITY 8 锚点/owner 错漏                                    | §2 指向改 §7；设备变更行改指 §4.2；T-DNS-04 移到端口之上；`RestoredUnverified` 得到产出分支；`DnsRestoreFailed` 改为携值结构变体           |
| QUALITY 9 候选排序其实是常量                                 | §4.4 改写为固定序 `[Local, Service]` 并写明后果；`can_address` **从 trait 删除**（唯一用途消失）                                           |
| R1b 悬而未决                                                 | **裁定：做**（§4.2 ③'）。它**不移除** R1b，只把"不可检测"改善为"可检测、窗口极窄"                                                          |

---

## 1. 边界与前置

### 1.1 前置：PR-5d v7 已落地

C3 **依赖** C2 提供的结构，且依赖方向是单向的。**下表逐行指向 5d v7 的节号**（v7 相对 v6 有移位，已逐条复核）：

| C3 需要的                                                | 由 PR-5d v7 提供                               |
| -------------------------------------------------------- | ---------------------------------------------- |
| 六个控制入口的**一处共享序列** + S1 槽位                 | §4.2（`run_control_sequence`）+ §4.7 S1 行     |
| **单一收敛实现** `apply_mode` + S2 槽位                  | §4.5 + §4.7 S2 行                              |
| `Shutdown` 臂的有界性 + S3 槽位（**facade 侧无槽位**）   | §4.6.3 + §4.7 S3 行及其"为什么必须在 actor 内" |
| `ControlAdmission`（关停静默期）                         | §4.6.1–§4.6.4                                  |
| 六方法 `ServiceControlOps`（teardown 测试要替换 runner） | §3.2                                           |
| `ACTOR_STOP_BUDGET` 常量与其 owner 列（本 PR 须复核）    | §4.3                                           |

**PR-5d 已完成的解耦点**：`change_default_network_dns(run_type, enabled)` 已加参（5d §3.1，值取自 `feat.rs:409` 已持有的 `core_status().2`）。本 PR 把整个函数迁走，`feat.rs:416-418` 的迁移标记（5d 已改指 PR-5e）随之清除。

### 1.2 本 PR 从 5d v7 继承的三项义务

5d v7 在改判接缝时把三件事明确移交本 PR。**它们不是背景，是必须在本文档里有落点的条款：**

| 义务                                                                                                                                                  | 5d 出处                   | 本文档落点                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- | -------------------------------------------------------- |
| **预算之和**：`ACTOR_STOP_BUDGET` 必须覆盖本 PR 放进 `Shutdown` 臂的全部 DNS I/O                                                                      | §4.3 owner 列、§4.7 S3 段 | **§3.3**（`DNS_RESTORE_BUDGET` 单一常量 + 外层 timeout） |
| **清理可被跳过**：`ACTOR_STOP_BUDGET` 超时 ⇒ 降级关停 ⇒ **S3 可能根本没跑**；5d 已撤回 `stop(None)`，且**不留 facade 侧 S3 槽位**，因此没有第二次机会 | §4.6.3、R-C2-4            | **R-C3-6**（§9.3）                                       |
| **R-C2-7**：5d §4.4 第 2/4/6 行（收敛失败）到不了 S2，本 PR 须让它**可见降级而非静默**                                                                | §4.7 S2 逐行核对、R-C2-7  | **§4.8.2** + T-DNS-37                                    |

### 1.3 做

1. `MacosDnsPort` 双适配器（**读一律本地**、写按模式分叉）+ **本 crate 内的异步命令 runner 与默认设备解析器**；
2. `SetTunDns` 守卫消息 + `DnsOverride` 状态机（含**不变式 I-DNS-1**）；
3. 恢复：**循环外归属判定 + 固定候选序 + 回读推进**；主路径 `await`、`Drop` 只记日志；
4. 填 5d 的三个接缝 S1 / S2 / S3；
5. 写回读校验 + **四态读**；
6. **actor 内 DNS I/O 的有界性**（逐次调用 + **每个处理器整体**两层）。

### 1.4 不做

启动时检测并清理残留 DNS 覆写（PR-6）；扩 IPC 线（见 §3.1 路线②）；**修 `utils::sudo::sudo` 的参数拼接**（它活在六个服务控制入口上，改它会改那六处的行为，属 CLAUDE.md §3 的"不要改邻近代码"——见 §9.1）。

---

## 2. 受管辖点清单 —— §0 原则治理的每一处

> **这张表存在的理由**：v1 声明了原则却在恢复路径漏了归属判定、在施加路径漏了不交性。**清单让下一位审查者核对列表，而不是重新发现遗漏。**
>
> **穷尽方法（可重跑）**：对「写」「读」「不执行调用即断言副作用缺席」三类各自枚举全部位置，再对每个位置问「这里有没有把 `Err`/`Ok`/超时当成副作用的证据」。v2 相对 v1 新增第 3、7、9、10 行。

| #      | 受管辖点                                             | 正确处置                                                                                                    | 落点        |
| ------ | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------- |
| 1      | **施加写** `write(target, Some(tun_ip))`             | 写**发起之前**记账；`Err`/超时都不撤销记账                                                                  | §4.2 ④      |
| 2      | **施加后回读**                                       | 五分消歧（§4.3）。**`Ok` 也可能没生效**；读失败保留守卫                                                     | §4.3        |
| **3**  | **恢复前的归属读**（v2 新增）                        | 在**任何写之前**读一次：`current ∉ {previous, applied}` ⇒ **零写**；读失败 ⇒ 按"多半仍是我们的"写并如实标记 | §4.4        |
| 4      | **恢复写** `write(target, previous)`                 | 与施加同构：`Err` 不等于没恢复；**返回值记日志，不推进**                                                    | §4.4        |
| 5      | **候选推进**                                         | **由「回读证实」推进/终止，绝不由写的返回值推进**                                                           | §4.4        |
| 6      | **Service 写前漂移预检**                             | 预检 `Ok` **只保证我们观察的那一刻**；daemon 自己解析的时刻不受保证 ⇒ R1                                    | §3.1、§8    |
| **7**  | **Local 施加写前的默认设备再解析**（R1b，v2 裁定做） | 不相等 ⇒ **不发起写**、**不留记账**（依据 §0 对偶）；窗口只被缩短，不被关闭 ⇒ R1b                           | §4.2 ③'     |
| 8      | **`Drop`**                                           | 只记日志，**不写**——因此不产生受管辖的写                                                                    | §4.6        |
| **9**  | **S3 整体可能不执行**（v2 新增）                     | 关停超时 ⇒ `Shutdown` 臂可能没被处理 ⇒ 恢复没跑。**如实记为 R-C3-6，不靠"应该会跑"措辞**                    | §3.3、§9.3  |
| **10** | **收敛失败 ⇒ S2 不触发**（v2 新增，5d R-C2-7）       | 不重施加是正确的（无既定事实可依），但**必须可见降级**                                                      | §4.8.2      |
| 11     | **测试断言**                                         | 凡断言「恢复失败」的测试，**必须包含最终回读**，不得以「所有写都 `Err`」收尾                                | §7 T-DNS-24 |

---

## 3. 端口设计

### 3.1 读一律本地，写按模式分叉

> **BLOCKING（v1 已确立，保留）：`nyanpasu_ipc` 里根本没有 DNS 读端点。** `api/network/` 只有 `mod.rs` 与 `set_dns.rs`，唯一 network 端点常量是 `NETWORK_SET_DNS_ENDPOINT`；`get_dns`/`read_dns` 在 `nyanpasu_ipc/src` 与 `crates/nyanpasu-service-runtime/src` **零命中**。

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

> 这正面回答了「同一 resolver 只保证同一算法、不保证同一观察」的质疑：观察结果派生自内核路由表，而内核路由表**不是**按会话或身份分区的。因此 app 与 daemon 在**同一时刻**必然观察到同一默认设备。
>
> **R1 因此只剩时间维度**，不含执行上下文维度。**但时间维度的后果比 v1 写的严重**——见 §8 与 R1 的重写。

**附带收益：读不依赖 daemon**——daemon 挂了、被 stop 了、被 uninstall 了，读照样能用。这是 §4.4 死锁序列的出路。

```rust
// core/actor/dns.rs —— 整个文件 #[cfg(target_os = "macos")]

/// 覆写目标：**本地解析到的硬件端口名**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DnsTarget(pub String);

#[derive(Debug, thiserror::Error)]
pub(crate) enum DnsPortError {
    /// 写之前发现默认设备已不是记录的那个。**拒绝写，不猜。**
    /// Service：daemon 只能写"当前默认"，写下去必然打错设备。
    /// Local（施加路径，§4.2 ③'）：写下去会把覆写落在一台已非默认的设备上。
    #[error("default device drifted: recorded {recorded}, observed {observed}")]
    TargetDrifted { recorded: String, observed: String },
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
    /// 只解析默认硬件端口，不读 DNS。§4.2 ③' 的 R1b 再解析用。
    async fn resolve_default(&self) -> Result<DnsTarget, DnsPortError>;
    /// 写指定目标。
    async fn write(&self, target: &DnsTarget, dns: Option<Vec<IpAddr>>) -> Result<(), DnsPortError>;
}
```

> **`can_address` 已从 trait 删除**（v1 有此方法）。v1 用它给候选集排序；§4.4 已确认那个排序恒等于常量 `[Local, Service]`，于是它在恢复路径上唯一的调用点消失。而"Service 能否定向该目标"这件事，`ServiceMacosDns::write` 的写前预检**本来就会判断并返回 `Err(TargetDrifted)` 且不发 IPC**——保留 `can_address` 等于把同一次解析做两遍（多一次 `DNS_READ_BUDGET`，直接抬高 §3.3 的和）。**删掉它是简化，不是能力损失。**

|      | `LocalMacosDns`                                                            | `ServiceMacosDns`                                                   |
| ---- | -------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| 读   | 本地 `networksetup`（共用 `LocalDnsReader`，§5.1）                         | **同左**（同一个 `LocalDnsReader` 实例）                            |
| 写   | `networksetup -setdnsservers <target> ..`，**可定向任意设备**              | 先本地解析默认设备；`≠ target` → `Err(TargetDrifted)`；相等才发 IPC |
| 权限 | 需 admin 组（F53）；UI 进程通常**没有**，且**本 PR 不提权**（裁定见 §9.1） | daemon 持有                                                         |

### 3.2 actor 内 DNS I/O 必须有界（**两层**）

> 5d 移交的 BLOCKING：`QUIESCE_BUDGET` 只界住 facade 许可；**真正会挂死的是 actor 持有的 DNS await**。ractor 逐条串行处理消息，一个卡在 DNS 命令里的处理器会让排队的 `Shutdown` 永远排不上。`kill_on_drop` 没有 timeout 等于没有——**没有东西去 drop 那个 future**。

**第一层：每一次外部 I/O 都有显式有限预算。**

| 调用                                                                   | 预算               | 含几次子进程                                            | 超时后                                                |
| ---------------------------------------------------------------------- | ------------------ | ------------------------------------------------------- | ----------------------------------------------------- |
| `read_default()`                                                       | `DNS_READ_BUDGET`  | 3（`route` + `listallhardwareports` + `getdnsservers`） | `Err(Io)` → 按四态①处置                               |
| `read(target)`                                                         | `DNS_READ_BUDGET`  | 1（`getdnsservers`）                                    | 同上                                                  |
| `resolve_default()`                                                    | `DNS_READ_BUDGET`  | 2（`route` + `listallhardwareports`）                   | 同上                                                  |
| Local `write`                                                          | `DNS_WRITE_BUDGET` | 1（`setdnsservers`）                                    | `Err(Io)` → **受 §0 管辖**：记账保留、标 `unverified` |
| Service `write`（含其内部一次 `resolve_default()` 预检，**预算另计**） | `DNS_IPC_BUDGET`   | 0（IPC）                                                | 同上                                                  |

**预算加在整个操作上，不是每次子进程上**——`read_default()` 的三次子进程共用一个 `DNS_READ_BUDGET`。**三个子进程调用一律 `.kill_on_drop(true)`。** 预算全部**实测**取上界，依据写进实施报告。

**第二层：每个含 DNS I/O 的处理器整体再包一层预算。** 第一层保证不了整体上界——一个处理器里有 4~5 次调用，其和才是它占住消息循环的时间；而"每处都记得包"是「不会忘记」型契约。因此：

| 处理器路径                                                     | 外层预算             | 定义                                                                      |
| -------------------------------------------------------------- | -------------------- | ------------------------------------------------------------------------- |
| 恢复（`SetTunDns(None)` / `Stop` / `SetBackend` / `Shutdown`） | `DNS_RESTORE_BUDGET` | §3.3                                                                      |
| 施加（`SetTunDns(Some)`）                                      | `DNS_APPLY_BUDGET`   | `4 × DNS_READ_BUDGET + DNS_WRITE_BUDGET + DNS_RESTORE_BUDGET`（推导见下） |

`DNS_APPLY_BUDGET` 的推导按 §4.2 的三情形取最坏值，**逐项可核**：

```text
情形 (a)：read_default ①            READ
          resolve_default ③'        READ
          write ④                        WRITE
          read ⑤                    READ                 = 3×READ + WRITE
情形 (b)：resolve_default（分派用）  READ
          write ④ + read ⑤          READ + WRITE         = 2×READ + WRITE
          （b1 分支改走恢复循环 ⇒ + DNS_RESTORE_BUDGET）
情形 (c)：resolve_default（分派用）  READ
          c1 恢复                          DNS_RESTORE_BUDGET
          c3 = 情形 (a)              3×READ + WRITE
                                    ⇒ 4×READ + WRITE + DNS_RESTORE_BUDGET   ← 最坏
```

> **这条推导在 v2 的初稿里算错过一次**（写成 `3×READ + WRITE`，漏了情形 (c) 的分派读与其内嵌的恢复）。**外层 `timeout` 的价值正在于此**：算错的是常量的取值，而不是上界是否存在——处理器占住消息循环的时间仍由那一个 `timeout` 封顶。

**外层超时的处置遵循 §0**：守卫**不清**、发降级、处理器返回，让后续步骤（如 `Shutdown` 的 `backend.shutdown()`）继续推进。**不重试。**

### 3.3 `DNS_RESTORE_BUDGET` —— 对 5d 预算义务的兑现

5d v7 §4.3 的 owner 列写着：`ACTOR_STOP_BUDGET` **必须覆盖 5e 放进 `Shutdown` 臂的全部 I/O 预算之和**，否则关停会稳定超时、稳定走 `AbandonedUnverified`、稳定跳过清理。

**兑现方式不是让 5d 去加一串常量，而是把和收敛成一个由本 PR 拥有的常量，并用一个真实的外层 `timeout` 强制它：**

```text
DNS_RESTORE_BUDGET  ≜  4 × DNS_READ_BUDGET + DNS_WRITE_BUDGET + DNS_IPC_BUDGET

推导（§4.4 恢复路径的最坏情形，逐项可核）：
  ① 归属读 read(target)                                        1 × READ
  ② 候选 1 = Local：write                                          WRITE
  ③ 回读 read(target)                                          1 × READ
  ④ 候选 2 = Service：write —— 其内部预检 resolve_default()     1 × READ
                          —— IPC 本身                                IPC
  ⑤ 回读 read(target)                                          1 × READ
（`can_address` 已删，故不再有第 5 次 READ；见 §3.1 注）

Shutdown 臂内的实际写法：
  timeout(DNS_RESTORE_BUDGET, state.restore_dns()).await
```

**因此 5d 只需核对一个不等式：**

```text
ACTOR_STOP_BUDGET  ≥  DNS_RESTORE_BUDGET + backend.shutdown() 的正常耗时上界 + 余量
```

**外层 `timeout` 是这条义务成立的构造，算术只是它的合理性检查。** 即使逐项预算实测偏低、或将来某条路径多了一次读，`Shutdown` 臂占住消息循环的时间仍以 `DNS_RESTORE_BUDGET` 为上界。**这正是「一步有界不等于整条序列有界」（5d §4.6.2）的同一课在本 PR 的应用。**

**这条义务的量词必须写死：它管的是 `Shutdown` 臂\*\*内部\*\*的时间，不是 facade 等待 `Shutdown` 的总时间。**

5d §4.3 把 `ACTOR_STOP_BUDGET` 的依据定为「实测 `Shutdown` 臂**正常路径**耗时上界」——**它不为排队时间预留任何余量**。而 `Shutdown` 必须排在**在飞的处理器**之后（ractor 逐条串行）。于是：

| 在飞的处理器           | 本 PR 之前             | 本 PR 之后                   | 后果                                               |
| ---------------------- | ---------------------- | ---------------------------- | -------------------------------------------------- |
| 一次 DNS 施加（挂死）  | **无界**（无 timeout） | ≤ `DNS_APPLY_BUDGET`         | 从"永久挂起"变成"有界地推迟" —— **改善，不是保证** |
| 一次后端 await（挂死） | 无界                   | 无界（不属本 PR，5a 设计面） | `ACTOR_STOP_BUDGET` 耗尽 ⇒ S3 不执行 ⇒ **R-C3-6**  |

**因此正确的说法是**：本 PR 让 DNS 路径不再是"actor 被无限期卡住"的成因之一；它**没有**、也无法让 `Shutdown` 一定被处理。**不把 `DNS_APPLY_BUDGET` 塞进上面那个不等式**，是因为那会把一条本就无法成立的保证（"排队时间也有上界"）伪装成算术问题——真正的后端 await 仍然无界。

**诚实限定**：以上都是**被 await 的时间**之和，不是墙钟上界；运行时被饿死或 executor 停摆不在其内。且**它保证不了 S3 一定被执行**——若 `Shutdown` 之前排着一个卡死的处理器，`Shutdown` 根本轮不到（R-C3-6）。

---

## 4. 状态机与生命周期

### 4.1 状态与消息（**只声明类型，规则在 §4.2–§4.4**）

```rust
// CoreActorState 新增
#[cfg(target_os = "macos")] pub(crate) dns: Option<DnsOverride>,
#[cfg(target_os = "macos")] pub(crate) dns_ports: DnsPorts,

pub(crate) struct DnsPorts { local: Arc<dyn MacosDnsPort>, service: Arc<dyn MacosDnsPort> }

pub(crate) struct DnsOverride {
    target: DnsTarget,
    /// 覆写**之前**的原始 DNS。`None` 是合法值（原本就没配）。
    previous: Option<Vec<IpAddr>>,
    /// **我们写下去的值**（v2 新增）。归属判定要用它区分
    /// 「设备仍是我们改的样子」与「第三方接管了」——见 §4.4。
    applied: Vec<IpAddr>,
    /// 建立覆写时的后端身份。**仅用于诊断**，不参与候选选择（§4.4 序是常量）。
    origin: RunType,
    /// 尚未被回读证实。守卫**保持 active**。
    unverified: bool,
}

SetTunDns {
    operation: OperationId,
    /// Some(ip) = 开 TUN；None = 关 TUN / 拆除。
    /// TUN 设备 IP 由 client 侧从 clash config 算好传入——**actor 不读配置全局**。
    desired: Option<IpAddr>,
    reply: RpcReplyPort<Result<DnsOutcome, CoreActorError>>,
}

pub(crate) enum DnsOutcome {
    Applied,
    /// 写报告失败（或超时）但回读证明 desired 生效；或回读本身不可得。
    AppliedUnverified,
    /// **写返回 Ok，但回读证明 desired 没生效。**
    AppliedNotObserved,
    /// **产出分支有三条，全部在 §4.2/§4.4 里**（v1 此变体无产出者）：
    /// ①施加时 `previous ≅ desired`（I-DNS-1）；②`SetTunDns(None)` 而无活跃覆写；
    /// ③施加时期望值恰好回落到 `previous`（§4.2 情形 (b) 的退化分支）。
    NoChange,
    Restored,
    /// **产出分支（v1 此变体无产出者）**：归属读失败 ⇒ 盲写 ⇒ 全程无回读证实。
    /// 语义 = **既不能断言已恢复，也不能断言未恢复**。
    RestoredUnverified,
    /// 归属判定发现设备当前值既不是 `previous` 也不是 `applied`：第三方已接管。
    /// **零写**，清守卫。
    RestoreSkippedNotOurs { observed: Option<Vec<IpAddr>> },
}

// CoreActorError 新增（均 #[cfg(target_os = "macos")]）
CoreNotRunning,
/// **必须是携值结构变体**（v1 是单元变体，装不下 R-C3-1 的用户消息要素）。
DnsRestoreFailed {
    target: String,
    previous: Option<Vec<IpAddr>>,
    applied: Vec<IpAddr>,
    observed: Option<Vec<IpAddr>>,
},
```

`CoreActorError` 现有变体用 `Arc<CoreBackendError>` 承载数据（`core/actor/types.rs:68-79`），**结构变体与其 derive 集相容**，不需要改 derive。

**注入路径**：`ClientSetupArgs`（`#[cfg(target_os="macos")] dns_ports`）→ `CoreClientArgs` → `CoreClient::spawn` → `CoreActorArgs` → `CoreActorState`。**与 `requests`/`degradation` 同一条既有路径。**

**不扩 `CoreRequest`**：它是 run/check/apply 共用的**全平台**进程描述。

### 4.2 施加：三情形分派 + 不变式 I-DNS-1

> **v1 的缺陷**：算法无条件取新快照并替换 `state.dns`，而 T-DNS-21（重复施加不得重新快照）与 T-DNS-23（设备变更先恢复旧设备）**假设了两条算法里根本没有的规则**。按 v1 写下的算法，`SetTunDns(Some(a))` → `SetTunDns(Some(b))` 会把**已被覆写的值**当作 `previous` 捕获，原值永久丢失。

**不变式 I-DNS-1（本节与 §4.3 的不交性都建立在它上面）：**

> **任何时刻，若 `state.dns == Some(o)`，则 `o.previous ≇ 当前期望的 desired`。**

`≅` = **集合相等**（顺序无关、去重后比较；`None` 与 `Some(vec![])` 视为同一值）。

**`SetTunDns(Some(tun_ip))` 的处理器，先按 `state.dns` 分三情形：**

```text
情形 (a) —— state.dns == None（首次施加）
  ① read_default() → (target, previous)          ← 拿不到就 Err，什么都没做，安全
  ①' if previous ≅ Some(vec![tun_ip]):
        **不建立覆写**，return NoChange           ← I-DNS-1 的建立点；也是 §0 对偶的用法：
                                                     写从未发起 ⇒ 可以断言什么都没变
  ②  state.dns = Some(DnsOverride{ target, previous,
                     applied: vec![tun_ip], origin: state.mode, unverified: true })
                                                  ← **在写发起之前记账**
  ③' resolve_default() 再解析一次；≠ target 则：
        撤销 ②的记账、return Err(TargetDrifted)   ← R1b。撤销是安全的，**因为写从未发起**
  ④  write(&target, Some(tun_ip))                 ← 成败都不改变②记下的恢复意图
  ⑤  read(&target) 五分消歧（§4.3）

情形 (b) —— state.dns == Some(o) 且 o.target == resolve_default()（同一设备，重复施加）
  **不重新快照。** previous 保持 o.previous 不变——这是 T-DNS-21 要的那条规则。
  b1. if o.previous ≅ Some(vec![tun_ip]):
        期望值回落到原值 ⇒ 「有覆写」与「无覆写」不可区分，且继续持有记账会在将来
        把 previous 当作"恢复"写回去——**那正是 BLOCKING 2 的洗白路径**。
        ⇒ 执行 §4.4 恢复循环；证实后 state.dns = None，return NoChange。
           （维持 I-DNS-1：不再有 Some(o)）
  b2. 否则：o.applied = vec![tun_ip]；o.unverified = true；跳到 ④（幂等重申写 + 回读）

情形 (c) —— state.dns == Some(o) 且 o.target ≠ resolve_default()（默认设备已变更）
  c1. **先对 o.target 执行 §4.4 的恢复循环**（把旧设备还原成 o.previous）
  c2. 无论恢复是否被证实，**都继续**——否则一次设备切换会永久禁用 TUN DNS。
      恢复未被证实时：产出降级 `macos_dns_device_change_restore_failed`，
      且旧设备的残留按 **R-C3-1** 报告（它不再被 state.dns 跟踪）
  c3. state.dns = None，然后按情形 (a) 对新设备走完整流程
```

**为什么 ③' 的撤销不违反 §0**：§0 禁止的是"由 `Err` 推断副作用缺席"。这里没有任何写调用被发起过——`write` 的调用点在 ④，而 ③' 在它之前返回。**能断言副作用缺席的唯一构造就是调用从未发生**（§0 对偶）。

**为什么 ③' 值得多一次 `resolve_default()`**（R1b 的裁定依据）：不做它，漂移时的行为是——覆写落在已非默认的设备 A 上，`state.dns` 记 A，TUN 实际使用的设备 B 没有覆写，**功能静默失效且留下一处残留**。做它，同样的漂移变成一次 `Err(TargetDrifted)` + 可见降级 + **零残留**。两者的功能结果相同（TUN 都没有 DNS 分流），差别全在**可见性与残留**——那正是本计划的价值序。**代价 = 一次有界子进程读。**

**R1b 不因此被移除**：③' 与 ④ 之间仍有窗口。**从"不可检测"改善为"可检测、窗口极窄"，与 R1 同形。**

### 4.3 施加后的回读消歧 —— 五行，**在 I-DNS-1 下两两不交**

> **v1 的 BLOCKING**：`== desired` 行与 `== previous` 行在 `previous == desired` 时同时匹配。若 desired 行胜出，后续一次关停会把 TUN 地址当作"原值"写回去；若 previous 行胜出，一次成功的 no-op 被报成失败。

**不交性的构造在 §4.2，不在本表**：I-DNS-1 保证进入 ④ 时 `previous ≇ desired`，因此 `≅ desired` 与 `≅ previous` 互斥。**本表不再自行处理重叠，它依赖那条不变式；审查者应核对 §4.2 的三条维持路径。**

| 回读结果            | ④ 的返回     | 处置                                     | 返回值                                                         |
| ------------------- | ------------ | ---------------------------------------- | -------------------------------------------------------------- |
| `≅ desired`         | `Ok`         | `unverified = false`                     | `Applied`                                                      |
| `≅ desired`         | `Err` / 超时 | `unverified = false`（**写其实生效了**） | `AppliedUnverified` + 降级 `macos_dns_write_reported_failure`  |
| `≅ previous`        | `Ok`         | **移除守卫**                             | **`AppliedNotObserved`** + 降级 `macos_dns_write_not_observed` |
| `≅ previous`        | `Err` / 超时 | **移除守卫**（确无可恢复）               | ④ 的 `Err`                                                     |
| 其它值 / **读失败** | 任意         | **保留守卫**、`unverified` 维持 `true`   | `AppliedUnverified` + 降级 `macos_dns_readback_failed`         |

> **第三行是 v1 的空洞**：`write` 返回 `Ok` 而回读证明 desired 没生效时，**没有 `Err` 可返回**。合成结果 `AppliedNotObserved` 由 T-DNS-30 单独钉住。
>
> **移除守卫的两行为什么安全**：设备当前值 `≅ previous`，也就是**没有需要恢复的东西**。这不是由写的返回值推出的，是由**回读**推出的——符合 §0。

### 4.4 恢复：归属判定在前、固定候选序、由回读推进

> **v1 的三个缺陷**：①候选集"按定向安全性排序"其实是常量，把常量伪装成算法；②**没有归属判定**——无条件写 `previous`，会覆盖用户/DHCP/VPN/另一进程在此期间做的改动；③后置条件的字面表述为假（读失败时并没有"回读证实"却仍在推进），且 `let _ = write(..)` **丢弃**了返回值，与"用于诊断"的说法不符。
>
> **另需保留 v1 的一处正确更正**：`SetBackend` 的恢复发生在 `replace_backend` **之前**，而 `replace_backend` 才改 `self.mode`（已核实源码顺序：`backend.take()`（`core/actor/mod.rs:267`）→ `running = None`（`:268`）→ 旧后端 `shutdown()`（`:273`）→ `self.mode = mode`（`:282`））。**该序列从旧 mode 开始。**

```text
restore(target, previous, applied) -> DnsOutcome

  ── 归属判定（**循环之外，恰好一次**）──────────────────────────────
  match read(target).await {                    // LocalDnsReader，两适配器共用同一实例
      Ok(c) if c ≅ previous => return Restored,             // 已是原值：**零写**
      Ok(c) if c ≅ applied  => { blind = false; }           // 仍是我们写下去的值 ⇒ 可以写
      Ok(other)             => { state.dns = None;
                                 return RestoreSkippedNotOurs{ observed: other } }  // **零写**
      Err(_)                => { blind = true; }            // 读不到 ⇒ 见下方裁定
  }

  ── 写 / 回读循环（**固定序**，见下）─────────────────────────────
  for cand in [local, service] {
      match cand.write(target, previous).await {            // 返回值**不推进**
          Ok(())  => tracing::debug!(?target, backend = ?cand, "dns restore write returned ok"),
          Err(e)  => tracing::warn!(?target, backend = ?cand, %e, "dns restore write reported failure"),
      }
      match read(target).await {                            // ← 推进的唯一依据
          Ok(c) if c ≅ previous => { state.dns = None; return Restored }
          _                     => continue,
      }
  }

  if blind { RestoredUnverified }                           // 守卫**保留**
  else     { Err(DnsRestoreFailed{ target, previous, applied, observed: 最后一次读的结果 }) }
                                                            // 守卫**保留**

后置条件（三句，实施与审查都逐句核对；**v1 的一句式字面为假**）：
  1. **成功终止**（`Restored`）**必须**由一次回读证实产生；
  2. **缺少证实只导致推进到下一候选**，不导致成功；
  3. **写的返回值既不推进也不终止**，它只进日志。
```

**为什么归属读失败时仍然写（`blind = true`）——逐案而非偏好：**

| 设备实际状态                  | 写 `previous`                     | 不写                                              |
| ----------------------------- | --------------------------------- | ------------------------------------------------- |
| 仍是我们的 TUN 地址（最可能） | **修好**：设备回到用户的原值      | **设备停在一个背后没有核的 TUN 地址上，解析全坏** |
| 第三方已改为 Q                | 把 Q 覆盖成 P（**有损，但可用**） | 保留 Q（正确）                                    |
| 已经是 P                      | no-op                             | no-op                                             |

**读不到时最该避免的是「设备指向死掉的 TUN 地址」**，那是唯一会让用户完全断网的格。因此写，并用 `RestoredUnverified` 如实标注"两个方向都断言不了"。

**候选序是常量，不是排序**（v1 把它写成两级排序）：`Local` 能定向任意设备、`Service` 只能写"当前默认"，因此在任何输入下 `Local` 都排第一。**写成常量 `[Local, Service]`，行为不变。**

**保留"每次恢复都先试 Local"的后果，如实写出**：非管理员账户上 Local 写**必然失败**（§9.1），所以每次恢复都有一次注定失败的 Local 写。**代价 = 一次有界子进程调用（`DNS_WRITE_BUDGET`），且本 PR 不提权 ⇒ 不会弹凭据对话框**（这是 §9.1 裁定的直接收益）。**收益 = T-DNS-16 那条路径**：`state.mode == Local` 且 Local 写失败时，Service 仍可能成功；反之当 target 已非默认设备时，只有 Local 能定向它。删掉常量序中的任一项都会丢一条真实路径。

**守卫的清除点恰好三处**（`state.dns = None`）：`Restored` 的两处 return、`RestoreSkippedNotOurs`。**其余路径一律保留守卫。**

### 4.5 漂移 + Local 不可用 = 具名残留，不塞进 R1

> **v1 把它归进 R1，错了**：R1 是「预检到 daemon 解析之间的 TOCTOU」；这里**漂移已被成功检测**，问题是**没有任何适配器还能定向设备 A**。

**序列**：Service 在设备 A 建立覆写 → 默认设备变为 B → 关停/停核触发恢复 → Service `TargetDrifted`（A 已非默认）→ Local 因 UI 进程无 admin 权限失败 → §4.7 裁定其余五个入口继续 → **设备 A 仍指向 TUN 地址，背后没有核**。A 若再次成为默认，解析即坏。重启服务也没用——Service 仍无法定向非默认的 A。

**裁定：不改控制策略**（为一次无法恢复的 DNS 就让用户停不掉服务，代价不成比例，且会把人锁死），**改为具名残留 + 可执行的用户指引**。

|                                        |                                                                                                                                                                                                                   |
| -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **残留编号**                           | **R-C3-1**                                                                                                                                                                                                        |
| **触发条件**                           | ①Service 模式建立覆写后默认设备变更，且 Local 写不可用（非 admin 账户）；**②§4.2 情形 (c) 中旧设备恢复未被证实**（v2 补入——那条路径同样会留下不再被跟踪的 A）                                                     |
| **后果**                               | 设备 A 的 DNS 永久指向 TUN 地址；A 再次成为默认时解析失败                                                                                                                                                         |
| **检测**                               | 恢复穷尽候选后返回 `DnsRestoreFailed{ target, previous, applied, observed }`，**四个值都进降级消息**——这正是 §4.1 把它改成携值结构变体的原因                                                                      |
| **用户可见指引**（措辞是功能的一部分） | 「设备 **A** 的 DNS 仍指向 `<applied>`。请在「系统设置 → 网络 → A → DNS」中手动改回 `<previous 或「自动」>`。」**不再写"或以管理员身份重试"**——本 PR 不提供任何应用内提权入口（§9.1），那句会承诺一个不存在的操作 |
| **owner / 移除条件**                   | 本表。移除条件 = ①`NetworkSetDnsReq` 支持定向设备（上游），**或** ②为 DNS 写路径引入提权——**该机制在仓内已存在但被本 PR 明确拒用**，其复用前提见 §9.1 的三条                                                      |

### 4.6 恢复的四个触发点、`Drop`，以及 S3 的无 `OperationId` 路径

**同一个实现，四个触发点。** 恢复逻辑是 `CoreActorState` 上的一个方法（`restore_dns(&mut self) -> DnsOutcome`），四处调用：

| 触发点                                     | 位置                                                                      | 是否携 `OperationId`                         |
| ------------------------------------------ | ------------------------------------------------------------------------- | -------------------------------------------- |
| **S1 拆除**：`SetTunDns { desired: None }` | facade `run_control_sequence` 内（5d §4.7 S1 槽位）发出的 RPC             | **是**——facade 此处持 `CoreOperationGuard`   |
| `CoreActorMessage::Stop` 臂                | `backend.stop()`（`core/actor/mod.rs:525`）**之前**                       | **是**——`Stop` 自带并已 `validate_operation` |
| `SetBackend` → `replace_backend`           | `replace_backend` 调用**之前**（`self.mode` 尚未改，见 §4.4 注）          | **是**——`SetBackend` 自带                    |
| **S3 关停**：`Shutdown` 臂                 | `backend.shutdown().await`（`:609`）与 `reply.send(())`（`:613`）**之前** | **否**——见下                                 |

**S3 为什么没有 `OperationId`，以及为什么这是对的（5d v7 §4.7 已采纳本立场）：**

- `Shutdown` 臂在 `:604` 就执行了 `state.operation.shutdown()`，**清空 active 并把全部 waiter 以 `ShuttingDown` 排空**（`gate.rs:55-60`）。此后任何 `OperationId` 都不可能通过 `validate_operation`（`mod.rs:185-190`）。
- facade 的关停序列**全程不取守卫**（5d §4.6），因此也无从构造一个合法的 `OperationId`。
- 一条 facade 级的恢复 RPC 会排在有界的 `Shutdown` RPC **之前**，然后卡在同一个挂死的处理器后面，**把 5d 刚建立的有界性整个毁掉**。

**因此 S3 直接在处理器内调用 `state.restore_dns()`，不发消息、不取门、不做 `validate_operation`。** 它合法的理由是构造性的：**它已经在处理器内部**，`Shutdown` 消息本身就是那次串行化。**facade 侧不留任何 S3 槽位**（5d §4.7 S3 行前置条件⑤）。

**S1 的 RPC 必须带有限超时**：`call(SetTunDns{..}, Some(DNS_RESTORE_BUDGET + 余量))`。理由与 5d §4.6.3 同：facade 的等待不能无界。超时按 §0 处置——**不假设拆除没发生**，走入口分岔（§4.7）。

**幂等性**：四个触发点可能重复触发（例如 S1 已拆除后 `Stop` 又到）。`restore_dns()` 在 `state.dns == None` 时**零 I/O 返回 `NoChange`**。

**`Drop`：只记 `tracing::error!`，措辞按不变量破坏写，不尝试任何恢复。**

> **为什么不做「尽力而为的同步 Drop」**：Service 侧同步做不到、Local 侧能做——**那半个兜底恰好在开发者最常用的模式下生效**，会系统性地把主路径 bug 藏到用户实际部署的模式才暴露。**「在你测得到的地方生效、在你测不到的地方失效」的兜底是反向选择。**

**不覆盖强杀**（SIGKILL / 任务管理器）——如实写明，兜底属 PR-6（R-C3-2、R-C3-3）。

### 4.7 S1：六个控制入口，**一处实现按 `action` 分岔**

5d v7 §4.2 已把六个 facade 控制方法收敛到**一个**私有方法 `NyanpasuClient::run_control_sequence`；S1 槽位在 `begin_operation()` 之后、`admission.check_open()?` **之前**，且 **`action: ServiceControlAction` 在作用域内**（5d §4.7 S1 行前置条件④）。

> **该顺序是 5d 刻意选定并复核过的**：S1 在 `check_open()` 之前，正是为了让 `check_open()` 能抓到**拆除期间才开始的关停**并压掉那条控制命令。**本 PR 不移动它。**

**因此本 PR 填的是一份代码、六种分岔，而不是六份复制：**

```rust
// SEAM-5E-S1（run_control_sequence 内）
#[cfg(target_os = "macos")]
if let Err(e) = self.tear_down_dns_override(&guard).await {     // SetTunDns{ desired: None }
    match action {
        ServiceControlAction::Uninstall => return Err(uninstall_aborted(e)),  // **外部命令不发出**
        _ => self.inner.degradation.publish(macos_dns_teardown_failed(&e)),   // 继续
    }
}
```

**为什么是六个而不是四个**：要把 install 排除在外，就得证明 `nyanpasu-service install` 在已有 daemon 在跑时不会把它换掉/重启——**核不了，且是「不会去做某事」型断言**。铺到六个的代价是无活跃覆写时一次 `state.dns.is_none()` 的 no-op（**零 I/O**，§4.6）。**用一次 no-op 换掉一条无法验证的前提。因此没有需要点名的例外。**

| 场景          | 处置                        | 理由                                                                                                  |
| ------------- | --------------------------- | ----------------------------------------------------------------------------------------------------- |
| **uninstall** | **中止卸载** + 用户可见错误 | 卸载**不可逆**；拆 DNS 失败说明我们**连自己的写都验证不了**，此时执行不可逆操作是把已知的不确定性固化 |
| **其余五个**  | **继续，产出 degradation**  | 服务可再启动、通道会回来，泄漏**通常**可恢复（**例外见 R-C3-1**）                                     |

**中止 uninstall 的错误必须说清三件事**：做了什么（**没有卸载**）、为什么、怎么办（重试；或先手动关 TUN 再卸载）。

**六条独立入口测试仍然照写**（T-DNS-05/06/17/18/26/27）：一处实现交付的是**每个入口的可观察行为**，六条测试断言的正是那个，不因共享实现而合并（5d §4.2 的同一论证）。

### 4.8 S2：重施加与收敛失败的可见降级

#### 4.8.1 重施加：单一 owner、按「收敛后的事实」触发

**owner 唯一 = `CoreModeReconciler`**（它持守卫、知道收敛后的 mode、能读 clash config）。**facade 不做重施加。** 5d v7 §4.7 已采纳该裁定，槽位落在 `apply_mode` 的成功尾部——`self.core.run(..)` 返回 `Ok` 之后、`Ok(())` 之前。

```text
apply_mode 的成功尾部（仍持守卫）：
  if state.running.is_some() && desired_tun_enabled {
      SetTunDns(Some(tun_ip))          ← 无论前面的控制动作是 Ok 还是 Err
  }
```

**触发判据是收敛后的事实，不是前序命令的返回值。** 5d §4.4 处置表的第 1/3/5 行都到达此处（含控制动作 `Err` 但收敛成功的第 5 行），这正是 v1 抓到的那个洞：控制失败后 reconcile 仍可能成功起一个 Local 核，而 TUN 仍然开着，DNS 却在 S1 被拆掉了。失败以 `macos_dns_reapply_failed` 降级呈现，**不改变 `apply_mode` 的返回值**。

`desired`（TUN 开关、TUN 设备 IP）由 `CoreModeReconciler` 新增的 `clash_config: ClashConfigClient` 字段读取（`NyanpasuClientInner.clash_config`，`client/mod.rs:247`）。**actor 侧一行配置全局都不读。**

#### 4.8.2 收敛失败（5d R-C2-7）：不重施加，但**必须可见**

5d §4.4 的第 2/4/6 行是收敛失败行，`apply_mode` 会在 `?` 处提前返回，**到不了 S2**。5d 已裁定「不重施加是正确的处置」——按"收敛后的事实"触发的机制，在没有确立的事实时就不该触发——并**把"让它可见"这件事指派给本 PR**。

**暴露面**：S1 已经把覆写拆掉了。若此时 TUN 仍被期望开启、而核实际上在跑，**DNS 覆写就缺席了**，且用户看不到任何提示。

**处置：在 `apply_mode` 内加一个统一的失败出口，发降级。**

```rust
async fn apply_mode(&self, guard: &CoreOperationGuard, mode: RunType, app: &ApplicationState)
    -> anyhow::Result<()>
{
    let converged: anyhow::Result<()> = async {
        self.core.set_backend(guard, mode).await?;
        let request = self.requests.for_product(app.core)?;
        self.core.run(guard, &request).await?;
        Ok(())
    }.await;

    if let Err(e) = &converged {
        // R-C2-7：收敛失败 ⇒ S2 不触发 ⇒ 若 TUN 仍被期望，覆写缺席。
        #[cfg(target_os = "macos")]
        if self.desired_tun_enabled().await.unwrap_or(false) {
            self.degradation.publish(macos_dns_reapply_skipped_unconverged(e));
        }
        return converged;
    }

    // ── SEAM-5E-S2（本 PR 填）───────────────────────────────
    // `self.core.run(..)` 已返回 Ok；仍持同一守卫。
    // ────────────────────────────────────────────────────────
    Ok(())
}
```

**降级消息必须说三件事**：①模式收敛失败；②DNS 覆写已在控制动作前被拆除且**本次不会**重施加；③**下一次成功的收敛会重施加**（这句是真的：任何走到 `apply_mode` 尾部的路径都会触发 S2）。**只有 `desired_tun_enabled` 为真时才发**——TUN 没开时"覆写缺席"对用户没有任何意义。

> **对 5d 门禁的影响，已逐条核对：** 上面把 5d 的 `apply_mode` 函数体包进一个内联 `async` 块，以取得**一个**失败出口（否则要在 `reconcile_with` 与 `force_local_with` 两处各写一遍，那又是一条「不会忘记」型契约）。5d §8 的 **G-SEAM-02** 判据是「`SEAM-5E-S2` 恰好一处，在 `core/actor/request.rs` 的 `apply_mode` 内，行号在该函数 `self.core.run(` 行之后」——改写后 `self.core.run(` 仍在 `apply_mode` 的词法体内、仍在标记之前，**两条判据都成立**。5d 的 **T-SEAM-02**（三个入口成功时都走到尾部、失败时都到不了）同样不受影响。**这条写在这里，是为了让 5d 的审查者不必自己发现。**

### 4.9 `SetTunDns` 准入

**携带 `OperationId`，由 `validate_operation`（`core/actor/mod.rs:185-190`）校验。**

**为什么不是「自己取门」**：`OperationGate::acquire` 在门被占时把请求塞进 `waiters`（`gate.rs:20-30`），**只有另一条 `ReleaseOperation` 消息被处理时才发放**（`release` → `grant_next`）。ractor 逐条串行处理，在 `handle()` 里 await 发放**永远等不到**——**构造性死锁**。（S3 不受此限，因为它根本不取门，见 §4.6。）

| #   | 场景                                                     | 规则                      | 构造                                                                                                                   |
| --- | -------------------------------------------------------- | ------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| A   | `Shutdown` 后到达                                        | `Err(ShuttingDown)`       | `operation.shutdown()`（`:604`）清 `active` → 恒 `StaleOperation`；`backend.take()`（`:606`） → `ShuttingDown`         |
| B1  | `SetTunDns` 先取得许可                                   | `Stop` 的恢复晚于设置     | `OperationGate` FIFO：`acquire` 在门被占时 `waiters.push_back`（`gate.rs:26`），`release` 后 `grant_next` 按入队序发放 |
| B2  | `Stop` 先取得许可，晚到的 `SetTunDns(Some)` 持**新守卫** | **`Err(CoreNotRunning)`** | 准入检查 `state.running.is_some()`，**仅对 `Some(..)` 生效**——`None`（拆除）在核已停时仍须允许                         |

> **注意**：`state.running` 有**多个**清除点（`Stop` `:532`、`replace_backend` `:268`、`Shutdown` `:605`，以及 `commit()` 观察到 `Stopped` 时 `:224-227`）。这**加强** B2，但使「删掉某一行就会红」的测试判据失效——T-DNS-13 的第三列因此指向 `SetTunDns` 臂里的准入检查本身，不指向任何一处赋值。

---

## 5. 本地命令层、四态读与 Phase 0

### 5.1 命令 runner、`LocalDnsReader`、异步默认设备解析

> **BLOCKING（v1）**：§3.2 说默认设备解析器"须先改为异步"，但**没给实现归属**。已核实：`get_default_network_hardware_port()` 在
> `backend/nyanpasu-runtime/crates/nyanpasu-utils/src/network/mod.rs:6`，返回 `std::io::Result<String>`（**同步**，`std::process::Command::output()`），而 `backend/nyanpasu-runtime` 是 **git submodule**（`.gitmodules`）。改它就是一个上游 PR——**正是 §3.1 用来否掉路线②的同一条理由**。v1 还提到 `LocalDnsReader` 却从未定义它。

**裁定：整条本地命令路径落在本 crate（`backend/tauri/src/core/actor/dns.rs`），不动 submodule。**

```rust
/// **最低层接缝**——存在的唯一理由是让「超时真的生效」可被测试（T-DNS-32）。
/// mock 打在这一层，被测的 timeout 就在它之上；打在 `MacosDnsPort` 上则是空转。
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait MacosCommandRunner: Send + Sync + 'static {
    /// **直调 argv，不经 shell。** 返回完整 `Output`（含 status / stdout / stderr）。
    async fn run(&self, program: &str, args: &[&str]) -> std::io::Result<std::process::Output>;
}

/// `tokio::process::Command`：`.env("LC_ALL", "C")`、`.kill_on_drop(true)`、
/// stdout/stderr 均 piped。**不设 shell、不拼字符串。**
pub(crate) struct TokioCommandRunner;

/// 本地读实现。**两个适配器共用同一个实例**（§3.1 表「读」行）。
pub(crate) struct LocalDnsReader { runner: Arc<dyn MacosCommandRunner> }

impl LocalDnsReader {
    /// 解析默认硬件端口名。两次 runner 调用，共用一个 `DNS_READ_BUDGET`：
    ///   1. `/sbin/route -n get default`         → 取 `interface: <bsd>` 行
    ///   2. `/usr/sbin/networksetup -listallhardwareports` → 把 `<bsd>` 映射为硬件端口名
    /// **解析在 Rust 里做，不经 grep/awk 管道。** 两次调用都检查 `output.status`；
    /// 任一非零、或解析不出**恰好一个**端口名 ⇒ `Err`，**绝不猜**。
    async fn resolve_default(&self) -> Result<DnsTarget, DnsPortError>;

    /// `/usr/sbin/networksetup -getdnsservers <target>` → 四态（§5.1 下表）。
    async fn read(&self, target: &DnsTarget) -> Result<Option<Vec<IpAddr>>, DnsPortError>;
}
```

**Local 写**：`/usr/sbin/networksetup -setdnsservers <target> <ip..|Empty>`，同样经 `MacosCommandRunner` 直调 argv、`LC_ALL=C`、`kill_on_drop(true)`、**检查 `output.status`**。

**为什么不复用上游 `nyanpasu_utils::network::macos::{set_dns, get_dns}`**（已核实，逐条）：

| 上游缺陷                                                                                     | 锚点                                                                                                                  |
| -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| 设备名被文本替换进 bash 脚本且 `$1` 未加引号——**含空格的硬件端口名今天就是坏的**，兼具注入面 | `network/mod.rs:35`（`.replace("$1", service_name)`）；`scripts/get-macos-dns.sh` 的 `networksetup -getdnsservers $1` |
| 写路径**完全不看退出码**：`let _ = ... .status()?`                                           | `network/mod.rs:47-53`                                                                                                |
| 读路径用 `filter_map` 部分解析——「一个 IP + 一行诊断文字」会被当成成功                       | `network/mod.rs:75-81`                                                                                                |
| 读路径把 `dns.is_empty()` 一律塌成 `Ok(None)`——**失败与"无 DNS 服务器"不可区分**             | `network/mod.rs:83-87`                                                                                                |

> `$2` 是**本该**分词的（多个 DNS 服务器），所以"两边加引号"是错的修法，**直接 argv 才对**。

**四态读：**

| #   | 条件                                                     | 结果                   |
| --- | -------------------------------------------------------- | ---------------------- |
| 1   | 退出码非零                                               | **`Err`**              |
| 2   | 输出匹配「无 DNS 服务器」那句                            | `Ok(None)`             |
| 3   | 输出**全部**行/词都解析为 IP                             | `Ok(Some(..))`         |
| 4   | **以上都不是**（含**混合输出**：部分 IP + 无法识别的行） | **`Err`**，不是 `None` |

> **第 3 条必须是「全部」**：用 `filter_map` 做部分解析会把「一个 IP + 一行诊断文字」当成成功。**混合输出属四态④。**

### 5.2 Phase 0：经验取得，而不是硬停

> **v1 裁定「无真机 fixture ⇒ PR-5e 不进入实施」。该裁定被推翻——它把"owner 没有 Mac"误当成"无法经验取得"。**

**已核实，两条途径都已经在仓里：**

| 途径                        | 锚点                                                                                                             | 触发方式                   |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------- | -------------------------- |
| `ci.yml` 的 `test_unit` job | `:27`（lint 矩阵）、`:122`（build 矩阵）、**`:214`（`test_unit` 矩阵含 `macos-latest`）**，`:303` 跑 `pnpm test` | **PR 上自动跑**（F33）     |
| `deps-build-macos.yaml`     | `:1-10` `workflow_dispatch`、**`:44` `runs-on: macos-latest`**                                                   | 对分支 `workflow_dispatch` |

**Phase 0 = 实施的第 0 步，产出物是证据文件，不是代码。**

**T-P0-01 —— 采集**：在分支上加一个**临时的、macOS-only 的**采集步骤（任一途径均可；`ci.yml` 途径用 `if: matrix.os == 'macos-latest'`），跑：

```bash
set +e
for LOC in C ""; do
  [ -n "$LOC" ] && export LC_ALL="$LOC" || unset LC_ALL
  echo "### LC_ALL=${LC_ALL:-<system default>}"; sw_vers; locale
  /usr/sbin/networksetup -listallhardwareports;                  echo "exit=$?"
  /sbin/route -n get default;                                    echo "exit=$?"
  DEV="$(...从上面两步解析出的硬件端口名...)";  echo "device=$DEV"
  /usr/sbin/networksetup -getdnsservers "$DEV";                  echo "exit=$?"   # ← R6 文案
  /usr/sbin/networksetup -getdnsservers "NoSuchDevice12345";     echo "exit=$?"   # ← 路线③判据
done
```

**stdout 与 stderr 分别捕获**，连同退出码、`sw_vers`、`locale`、**完整命令行**一起写进 artifact 上传。

**一次采集同时结掉四件事：**

1. **R6 的「无 DNS 服务器」文案**（四态②的字面串）——runner 上的默认设备通常没有显式 DNS，正是该文案出现的场景；
2. **路线③的判据**：`networksetup -getdnsservers <无效设备名>` 的退出码。**若为非零 ⇒ 「退出码 0 且无可解析 IP ⇒ 无服务器」成立 ⇒ 四态②不再依赖任何文案 ⇒ fixture 依赖被整个移除**（v1 §5.2 已为此保留出口，v2 把它变成一次可执行的实验）；
3. **`-listallhardwareports` 与 `route -n get default` 的输出格式**——§5.1 的 `resolve_default()` 是本 PR 新写的解析器，它和四态读面对**同一个**"不许臆造格式"的约束，v1 没有覆盖到这一点；
4. **locale 敏感性**：两轮（`LC_ALL=C` 与系统默认）文案若不同，说明 `networksetup` 受 locale 影响，**则路线③从"可选"升级为"必须"**——因为任何文案匹配都会在非英文系统上失效。

**T-P0-02 —— 清理**：采集步骤是**临时的、只存在于分支上的**。合并前必须移除。判据：`git diff --exit-code <base>..HEAD -- .github/workflows/` 为空。

**保留的禁令（v1 正确，不放松）：**

| 路径                        | 状态                                                      |
| --------------------------- | --------------------------------------------------------- |
| ①真机/CI 捕获，得到确切文案 | **Phase 0 T-P0-01**                                       |
| ②**臆造一个字面串**         | **禁止**。生产与测试共用臆造事实 = 自证；失败模式静默     |
| ③退出码判别（不依赖文案）   | **由 T-P0-01 的实验决定是否采纳**；采纳则①的 fixture 可省 |

**Phase 0 未完成时，实施阶段不得写死四态②的字面串，也不得写死 `resolve_default()` 的输出格式假设。** 这是对"不许臆造"的可执行表述，取代 v1 那条"整个 PR 不开工"的硬停。

**诚实限定（新残留）**：GitHub 托管 runner 是**一台**机器，不是用户的机器。把它的观察外推到用户机器，依据是 `LC_ALL=C` 在采集与生产两侧都设置——**而这条依据本身正是 T-P0-01 第 4 项要验证的**。若验证结果是 `networksetup` 无视 `LC_ALL`，则必须走路线③。**记为 R-C3-4。**

**fixture 要求（若走路径①）**：用**新调用形态**（直调 `networksetup`、`LC_ALL=C`）捕获，**不能用上游脚本**（其 `echo $RES` 会把换行压成空格）；记录 provenance（macOS 版本、locale、完整命令行、runner 镜像标签）；**生产匹配器独立于 fixture 文件**（不 `include_str!` 同一份），T-DNS-09 读 fixture。

---

## 6. 定序保证表

> **表与散文双向对齐**：正文每一条「X 之后 Y 一定已发生 / X 不会发生」都在此有行；每行回指正文小节。**措辞不得强于机制。**
>
> **v2 的三处弱化**（v1 措辞强于机制，逐条改）：①"回读四种组合"→**五行且在 I-DNS-1 下两两不交**；②"actor 内每次外部 I/O 有限"→**两层预算，且不保证墙钟、不保证 S3 一定被执行**；③"Service 写不会打到漂移后的设备"→**只保证我们观察的那一刻**（见 §8）。

| #   | 断言                                                              | **构造**                                                                | 正文出处   | 测试                                        |
| --- | ----------------------------------------------------------------- | ----------------------------------------------------------------------- | ---------- | ------------------------------------------- |
| 1   | S1 位于 `begin_operation()` 之后、`check_open()?` 之前            | 5d v7 §4.7 S1 冻结位置 + 5d G-SEAM-01                                   | §4.7       | T-DNS-05/06/17/18/26/27                     |
| 2   | 六个入口**各自**遍历 S1（一处实现）                               | `run_control_sequence` 唯一实现（5d §4.2）                              | §4.7       | 同上（六条独立）                            |
| 3   | **uninstall 拆除失败 ⇒ 外部 uninstall 命令零调用**                | S1 分岔的 `return Err(..)` 在 `dispatch(action)` 之前                   | §4.7       | **T-DNS-26**                                |
| 4   | **其余五个入口拆除失败 ⇒ 控制命令照发**                           | S1 分岔的 `_ => publish(..)` 不返回                                     | §4.7       | **T-DNS-27**                                |
| 5   | S2 位于 `apply_mode` 内、`self.core.run(..)` 返回 `Ok` 之后       | 5d v7 §4.7 S2 冻结位置 + 5d G-SEAM-02                                   | §4.8.1     | T-DNS-33                                    |
| 6   | **重施加由「收敛后 running + TUN 期望」驱动，非前序命令结果**     | `apply_mode` 尾部的条件式                                               | §4.8.1     | **T-DNS-33**                                |
| 7   | **收敛失败 ⇒ 不重施加，但产出降级**（5d R-C2-7）                  | `apply_mode` 失败出口的 `publish`                                       | §4.8.2     | **T-DNS-37**                                |
| 8   | S3 位于 `Shutdown` 臂内、`:609` 与 `:613` 之前                    | 5d v7 §4.7 S3 冻结位置 + 5d G-SEAM-03                                   | §4.6       | T-DNS-03                                    |
| 9   | **S3 不发消息、不取门、不做 `validate_operation`**                | 处理器内直调 `state.restore_dns()`；**facade 侧无 S3 槽位**             | §4.6       | T-DNS-03 + §8 `rg` 门禁                     |
| 10  | **S3 整体不超过 `DNS_RESTORE_BUDGET`**                            | `timeout(DNS_RESTORE_BUDGET, restore_dns())` 外层包裹                   | §3.3       | **T-DNS-38**                                |
| 11  | **记账早于写的\*\*发起\*\***（不只是早于写的返回）                | `state.dns = Some(..)` 在 `write()` 调用点之前的源码顺序                | §4.2 ②     | **T-DNS-19**                                |
| 12  | **已有覆写且设备未变 ⇒ 不重新快照**                               | §4.2 情形 (b)：不执行 `read_default()`、`previous` 原样保留             | §4.2       | T-DNS-21                                    |
| 13  | **设备变更 ⇒ 先恢复旧设备，再对新设备取快照**                     | §4.2 情形 (c) 的 c1→c3 顺序                                             | §4.2       | T-DNS-23                                    |
| 14  | **`previous ≅ desired` ⇒ 不建立覆写、零写**（I-DNS-1）            | §4.2 ①' 的提前 return                                                   | §4.2       | **T-DNS-36**                                |
| 15  | **回读消歧五行覆盖全部组合且两两不交**                            | I-DNS-1 + §4.3 五分支                                                   | §4.3       | T-DNS-01/28/29/30/35                        |
| 16  | **恢复前先归属判定；`current ∉ {previous, applied}` ⇒ 零写**      | §4.4 循环外的一次 `read` 与三分支                                       | §4.4       | **T-DNS-39**                                |
| 17  | **候选推进只由回读证实驱动**                                      | §4.4 循环后置条件三句：写的返回值只入日志                               | §4.4       | T-DNS-24/**31**                             |
| 18  | **候选序是常量 `[Local, Service]`，两个适配器都会被试**           | §4.4 的常量数组（退回单一适配器即丢一条路径）                           | §4.4       | **T-DNS-16**                                |
| 19  | **守卫的清除点恰好三处**                                          | `Restored` 两处 + `RestoreSkippedNotOurs`；其余路径保留                 | §4.4       | T-DNS-20/39                                 |
| 20  | **Service 写前校验默认设备**——**只保证我们观察的那一刻**          | `ServiceMacosDns::write` 的比对分支；**daemon 解析时刻不受保证 ⇒ R1**   | §3.1、§8   | T-DNS-14                                    |
| 21  | **Local 施加写前再解析一次默认设备**（R1b）；不等则**零写零记账** | §4.2 ③' 的提前 return（在 `write` 调用点之前）                          | §4.2       | **T-DNS-40**                                |
| 22  | **actor 内每次外部 I/O 有限，且每个处理器整体再有一层预算**       | 五处逐次 `timeout` + `kill_on_drop`；两处外层 `timeout`。**不保证墙钟** | §3.2、§3.3 | **T-DNS-32**、T-DNS-38                      |
| 23  | **B1**：`SetTunDns` 先取得许可 ⇒ `Stop` 的恢复晚于设置            | `OperationGate` FIFO：`waiters.push_back`（`gate.rs:26`）+ `grant_next` | §4.9       | **T-DNS-41**                                |
| 24  | **B2**：`Stop` 先取得许可时晚到的 `SetTunDns(Some)` 失败          | `SetTunDns` 臂的 `state.running.is_some()` 准入检查                     | §4.9       | T-DNS-13                                    |
| 25  | `Shutdown` 后不再有 `SetTunDns` 生效                              | `operation.shutdown()`（`:604`）+ `backend.take()`（`:606`）            | §4.9 A     | **owner = PR-5d T-GATE-01**（本 PR 不重复） |
| 26  | **`SetBackend` 路径的恢复发生在 `replace_backend` 之前**          | 处理器内调用点顺序（`self.mode` 在 `:282` 才改）                        | §4.4、§4.6 | **T-DNS-42**                                |
| 27  | **`Drop` 不发起任何写**                                           | `Drop` 内只有 `tracing::error!`                                         | §4.6       | T-DNS-25                                    |
| 28  | **无活跃覆写时恢复零 I/O**                                        | `restore_dns()` 开头的 `state.dns.is_none()` 提前 return                | §4.6       | **T-DNS-43**                                |

---

## 7. 测试矩阵（**第三列必须真能红**）

> **两类空转陷阱**：①状态有**多个**写入点，删其一另一仍生效；②**mock 打在被测机制之上**，删掉机制对 mock 无影响——**接缝必须低于被测机制**。
>
> **本矩阵的通用约束**：凡执行恢复/施加算法的测试，**只 mock `MacosDnsPort` 或（需要验超时时）`MacosCommandRunner`，绝不 mock 算法本身**。
>
> **未使用的编号**：T-DNS-12、15 在 v1 起就未分配；**T-DNS-07 在 v2 被删除**（与 5d T-GATE-01 逐字重复：同断言、同第三列 `gate.rs:57-59`、同"不能走 actor 集成测试"caveat）。**空号是刻意的，不是遗漏。**

| ID                      | 断言                                                                                                                                                                                                                                                                                                                                                                      | **删掉哪行会让它红**                                                                                                                                                                                                                                                                                                             |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-DNS-01                | `SetTunDns{Some}` → `write` 被调、回读 `≅ desired` → `Applied`，守卫 active 且 `unverified == false`                                                                                                                                                                                                                                                                      | 处理器里 `port.write()`                                                                                                                                                                                                                                                                                                          |
| T-DNS-02                | `Stop` 时恢复在 `backend.stop()` **之前**                                                                                                                                                                                                                                                                                                                                 | `Stop` 臂的 `restore_dns().await`（与 `backend.stop()` 对调即红）                                                                                                                                                                                                                                                                |
| T-DNS-03                | `Shutdown` 时恢复在 `backend.shutdown()` 与 **reply** 之前，**且未发出任何 `SetTunDns`**（正反两半都断言）                                                                                                                                                                                                                                                                | `Shutdown` 臂的 `restore_dns().await`；反向半段由 §8 `rg` 门禁补齐                                                                                                                                                                                                                                                               |
| T-DNS-04                | **回读比对真的会发现不一致**——比较发生在**端口之上**的算法层（`≅` 集合相等），mock 端口返回乱序/含重复的等价集合仍判相等                                                                                                                                                                                                                                                  | 算法层的 `≅` 实现（改成 `Vec` 逐元素相等即红）。**v1 把它派给适配器，位置错了**：适配器只负责把输出解析成 `Vec<IpAddr>`                                                                                                                                                                                                          |
| T-DNS-05/06/17/18/26/27 | 六个入口各自：拆 DNS 在控制动作之前（**六条独立，不合并**）                                                                                                                                                                                                                                                                                                               | `run_control_sequence` 的 S1 块；**T-DNS-26（uninstall）另断言外部命令零调用**，第三列 = S1 分岔里 `Uninstall => return Err`                                                                                                                                                                                                     |
| T-DNS-13                | `Stop` 先取得许可 → 晚到的持新守卫的 `SetTunDns(Some)` → `Err(CoreNotRunning)`                                                                                                                                                                                                                                                                                            | **`SetTunDns` 臂的 `state.running.is_some()` 准入检查**（不能点任何一处 `running = None` 赋值——多清除点，§4.9 注）                                                                                                                                                                                                               |
| T-DNS-14                | `ServiceMacosDns::write` 漂移时 → `Err(TargetDrifted)` **且未发 IPC**                                                                                                                                                                                                                                                                                                     | 写前比对分支                                                                                                                                                                                                                                                                                                                     |
| **T-DNS-16**            | **候选集完整**：Local 写失败、Service daemon 活着且 target 就是当前默认 → 经 **Service** 成功恢复                                                                                                                                                                                                                                                                         | §4.4 常量数组里的 `service` 项（退成单一适配器即红）。**执行真实恢复循环，只 mock 两个 `MacosDnsPort`**                                                                                                                                                                                                                          |
| **T-DNS-19**            | **写挂死至 `DNS_WRITE_BUDGET` 超时** → 返回 `AppliedUnverified` + 降级；**随后一次 `Stop` 必须对同一 target 发起恢复写**                                                                                                                                                                                                                                                  | `state.dns = Some(..)` 相对 `write()` **调用点**的位置——移进写的成功分支即红（超时路径不记账 ⇒ `Stop` 无恢复写）。**v1 只断言终态，移到写完成之后照样过**                                                                                                                                                                        |
| T-DNS-20                | 恢复回读校验失败 → 守卫**不清**                                                                                                                                                                                                                                                                                                                                           | `state.dns = None` 前的证实条件                                                                                                                                                                                                                                                                                                  |
| T-DNS-21                | 重复 `SetTunDns(Some(a))` → `(Some(b))`（同设备）→ 恢复得到**最初**原值                                                                                                                                                                                                                                                                                                   | §4.2 情形 (b) 的分派（删掉即落回情形 (a)，`previous` 被覆写值污染）                                                                                                                                                                                                                                                              |
| T-DNS-22                | 原值 `None` 的活跃覆写与「无覆写」可区分                                                                                                                                                                                                                                                                                                                                  | 两层 `Option` 的外层判断                                                                                                                                                                                                                                                                                                         |
| T-DNS-23                | 设备变更：先 `write(a, previous_a)` 再对 b 取快照（断言 mock 调用序）                                                                                                                                                                                                                                                                                                     | §4.2 情形 (c) 的 c1（删掉即跳过旧设备恢复）                                                                                                                                                                                                                                                                                      |
| **T-DNS-24**            | **所有候选写都 `Err` 但某次回读证实了 previous** → **返回 `Restored`**（不是 `DnsRestoreFailed`）                                                                                                                                                                                                                                                                         | §4.4 循环里「读证实即 return」那行。**v1 之前的版本把「全 `Err` ⇒ 失败」固化成测试，正是要推翻的行为**                                                                                                                                                                                                                           |
| T-DNS-25                | `Drop` 时守卫仍 active → 记 `error!` 且**不发起任何恢复**（断言两个端口 mock 零调用）                                                                                                                                                                                                                                                                                     | `Drop` 里的 `tracing::error!` + 反向断言                                                                                                                                                                                                                                                                                         |
| T-DNS-28                | 写 `Err` 且回读 `≅ previous` → **移除**守卫并返回 `Err`                                                                                                                                                                                                                                                                                                                   | §4.3 第四行分支                                                                                                                                                                                                                                                                                                                  |
| **T-DNS-29**            | **回读失败（读 `Err`）** → 守卫**保留**、`unverified` 仍为 `true`、`AppliedUnverified` + `macos_dns_readback_failed`                                                                                                                                                                                                                                                      | §4.3 第五行（兜底分支）；把它并进"移除守卫"任一行即红                                                                                                                                                                                                                                                                            |
| **T-DNS-30**            | **写 `Ok` 但回读 `≅ previous`** → `AppliedNotObserved` + 降级                                                                                                                                                                                                                                                                                                             | §4.3 第三行的合成结果分支                                                                                                                                                                                                                                                                                                        |
| **T-DNS-31**            | **写 `Ok` 但回读未证实 → 继续下一候选**（不提前成功）                                                                                                                                                                                                                                                                                                                     | 循环里「以回读结果决定 continue」那行                                                                                                                                                                                                                                                                                            |
| **T-DNS-32**            | **挂死的 DNS 子进程**：注入永不返回的 `MockMacosCommandRunner`；①`SetTunDns` 的 reply 在 `DNS_READ_BUDGET`+余量内到达；②随后发一条普通消息（如 `Status` 查询）并断言它也在余量内得到回复——**证明消息循环已恢复**                                                                                                                                                          | `LocalDnsReader` 读实现里的 `tokio::time::timeout(..)`。**接缝在 runner 层**——mock `MacosDnsPort` 会绕过被测 timeout（空转陷阱②）。**与 5d T-SD-05 的分界**：本条**不经 `shutdown()` 观察**——5d 已给 `shutdown()` 加了 `ACTOR_STOP_BUDGET`，一个有界返回的 `shutdown()` 在"actor 卡死"与"actor 正常"两种情形下都会返回，无法区分 |
| **T-DNS-33**            | **控制动作 `Err` + reconcile 成功起了核 + TUN 仍开 → 仍然重施加**。驱动**真实**的 facade → `CoreModeReconciler` → actor → DNS 端口链路；**只 mock `ServiceControlOps`（脚本化控制失败）与 `MacosDnsPort`**。前置：`state.running` 初始为 `None`；断言 mock 调用序为「后端 `run` 被观察到 → DNS `write` 被观察到」，以此证明 `running` 是被真实的 `Run` 转移置为 `Some` 的 | `apply_mode` 尾部的 `state.running.is_some() && desired_tun_enabled` 条件（挂回"控制成功"分支即红）。**mock reconciler 或 mock 重施加方法都会使本条空转**                                                                                                                                                                        |
| **T-DNS-34**            | 四态④之**混合输出**：一个 IP + 一行诊断文字 → `Err`                                                                                                                                                                                                                                                                                                                       | 「**全部**元素都须解析成功」那个判断（改成 `filter_map` 即红）                                                                                                                                                                                                                                                                   |
| **T-DNS-35**            | 写 `Err` 但回读 `≅ desired` → `AppliedUnverified` + `macos_dns_write_reported_failure`，守卫**保留**、`unverified == false`                                                                                                                                                                                                                                               | §4.3 第二行分支（并进第一行则降级不发，并进第四行则守卫被误清）                                                                                                                                                                                                                                                                  |
| **T-DNS-36**            | **`previous ≅ desired`（用户已把 DNS 设成 TUN 地址）→ `NoChange`、`write` 零调用、`state.dns` 保持 `None`**                                                                                                                                                                                                                                                               | §4.2 ①' 的提前 return（删掉即建立覆写，随后一次恢复会把 TUN 地址当作"原值"写回——BLOCKING 2 的洗白路径）                                                                                                                                                                                                                          |
| **T-DNS-37**            | **收敛失败（`core.run` 返回 `Err`）+ TUN 期望开启 → 不重施加（DNS `write` 零调用）**且**发出 `macos_dns_reapply_skipped_unconverged` 降级**；TUN 未开启时**不发**                                                                                                                                                                                                         | `apply_mode` 失败出口里的 `publish(..)`（删掉即静默 ⇒ 降级断言红）；`desired_tun_enabled` 条件（删掉即 TUN 未开时也发 ⇒ 第二半段红）                                                                                                                                                                                             |
| **T-DNS-38**            | **恢复内部挂死**（runner 永不返回）→ `Shutdown` 臂仍在 `DNS_RESTORE_BUDGET`+余量内推进到 `backend.shutdown()`（断言后端 mock 被调用），守卫**未清**、降级已发                                                                                                                                                                                                             | `Shutdown` 臂外层的 `timeout(DNS_RESTORE_BUDGET, ..)`（改成裸 await 即永久挂起，后端 `shutdown` 永不被观察到）                                                                                                                                                                                                                   |
| **T-DNS-39**            | **归属判定**：恢复前回读得到既非 `previous` 也非 `applied` 的第三方值 → **两个端口的 `write` 均零调用**、守卫被清、返回 `RestoreSkippedNotOurs`                                                                                                                                                                                                                           | 循环外归属判定的 `Ok(other) =>` 分支（删掉即无条件写，用户/DHCP/VPN 的改动被覆盖——BLOCKING 4）                                                                                                                                                                                                                                   |
| **T-DNS-40**            | **R1b**：`read_default()` 返回 A，`resolve_default()` 返回 B → `Err(TargetDrifted)`、**`write` 零调用**、**`state.dns` 仍为 `None`**                                                                                                                                                                                                                                      | §4.2 ③' 的比对与提前 return（删掉即写到已非默认的 A 上并留下记账）                                                                                                                                                                                                                                                               |
| **T-DNS-41**            | **B1 FIFO**：`SetTunDns(Some)` 先入队取得许可、`Stop` 后入队 → 观察序必为「DNS `write` → 后端 `stop`」，且 `Stop` 的恢复看到的是已设置的覆写                                                                                                                                                                                                                              | `OperationGate::acquire` 的 `waiters.push_back`（`gate.rs:26`）改成 `push_front` 即红。**直接持 `OperationGate` 构造，不经 actor 集成**                                                                                                                                                                                          |
| **T-DNS-42**            | **`SetBackend`**：恢复在 `replace_backend` **之前**发生（断言 mock 序：DNS `write` → 旧后端 `shutdown`），且恢复看到的 `origin` 是**旧** mode                                                                                                                                                                                                                             | `SetBackend` 臂里 `restore_dns().await` 相对 `replace_backend(..)` 的位置（对调即红）                                                                                                                                                                                                                                            |
| **T-DNS-43**            | `state.dns == None` 时的恢复 → `NoChange`，**两个端口 mock 零调用**                                                                                                                                                                                                                                                                                                       | `restore_dns()` 开头的 `state.dns.is_none()` 提前 return（删掉即发起一次无意义的读）                                                                                                                                                                                                                                             |
| **T-DNS-44**            | **`RestoredUnverified` 的产出**：归属读失败 → 仍发起写 → 回读也失败 → 返回 `RestoredUnverified`，守卫**保留**                                                                                                                                                                                                                                                             | 归属判定的 `Err(_) => blind = true` 分支（改成"读失败即放弃"则返回 `DnsRestoreFailed` 且零写，两处断言都红）                                                                                                                                                                                                                     |
| T-DNS-08                | 四态①：非零退出 → `Err`。fixture = **非零退出 + 可解析 IP 输出**                                                                                                                                                                                                                                                                                                          | `if !output.status.success()`                                                                                                                                                                                                                                                                                                    |
| T-DNS-09                | 四态②：「无 DNS 服务器」→ `Ok(None)`——**读 Phase 0 采集的 fixture**（§5.2）                                                                                                                                                                                                                                                                                               | 匹配该文案那行（**若 T-P0-01 判定采纳路线③，本条改为断言「退出码 0 + 零可解析 IP ⇒ `Ok(None)`」，第三列改为退出码判别那行**）                                                                                                                                                                                                    |
| T-DNS-10                | 四态③：全部可解析 → `Ok(Some(..))`                                                                                                                                                                                                                                                                                                                                        | 解析 IP 列表那行                                                                                                                                                                                                                                                                                                                 |
| T-DNS-11                | 四态④：不认识的输出 → `Err`                                                                                                                                                                                                                                                                                                                                               | 兜底分支（改成 `None` 即红）                                                                                                                                                                                                                                                                                                     |
| **T-DNS-45**            | **`resolve_default()` 的解析纪律**：`route`/`listallhardwareports` 任一非零退出 → `Err`；输出解析不出**恰好一个**端口名 → `Err`（**不猜、不取第一个**）                                                                                                                                                                                                                   | 解析器里 `output.status` 检查那行 + "恰好一个"的判断（改成 `.first()` 即红）                                                                                                                                                                                                                                                     |

---

## 8. 契约归属

| 契约                                                   | 由谁保证                  | 为什么可验证                                                                                                            |
| ------------------------------------------------------ | ------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| 非 macOS 不存在 DNS 抽象                               | **cfg / 类型**            | 非 macOS 上引用它编译不过                                                                                               |
| **恢复推进不看写的返回值**                             | **算法后置条件 + 测试**   | T-DNS-24/31；后置条件三句写在 §4.4 算法旁，本地可核                                                                     |
| **恢复不覆盖第三方的改动**                             | **循环外归属判定 + 测试** | T-DNS-39。**残余**：归属读与写之间仍有窗口（与 R1/R1b 同形）                                                            |
| **Service 写在\*\*我们观察的那一刻\*\*目标即默认设备** | **写前比对 + 返回值**     | `Err(TargetDrifted)` 可观测，T-DNS-14。**这句比 v1 弱，且必须弱**——见下方框                                             |
| **actor 不会被 DNS I/O 无限期卡住**                    | **两层 timeout**          | T-DNS-32（逐次）+ T-DNS-38（处理器整体）。**不保证墙钟**（executor 停摆不在其内）；**也不保证 S3 一定被执行**（R-C3-6） |
| 「核已停时不建立覆写」                                 | **运行时准入 + 测试**     | T-DNS-13                                                                                                                |
| **S3 不经消息、不取门**                                | **`rg` 门禁 + 测试**      | `rg -n 'SetTunDns'` 在 `core/actor/mod.rs` 的 `Shutdown` 臂内**零命中**；T-DNS-03 反向半段                              |
| DNS 路径不回头读全局                                   | **ledger 门禁**           | `core/actor/dns.rs` 的 `Config::*()` / `::global()` 计数恒 0                                                            |
| 顺序类契约                                             | **测试**                  | 控制流性质                                                                                                              |

> ### 为什么「Service 写不会打到漂移后的设备」这句话必须被弱化
>
> v1 把它写成一条保证。**机制承担不起。** 预检与 daemon 自己解析之间，默认设备可以从 A 变成 B；IPC 写的是"当前默认"，于是 **A 的 `previous` 被写到了 B 上**。此后我们对 **A** 的回读：
>
> - **检测不到** B 被写坏——我们读的是 A；
> - **修不了** B——我们的记账里根本没有 B；
> - 更糟：**若 A 此刻恰好已经等于 `previous`**（例如 Local 那一轮已经把 A 改回来了，或 A 本来就没配 DNS 而 `previous == None`），恢复循环会**返回 `Restored`**，而 B 正处在被写坏的状态。
>
> **能做而已做的**：§4.4 不再调用 `can_address`，而是让 `ServiceMacosDns::write` 的自带预检在 `target ≠ 当前默认` 时直接 `Err(TargetDrifted)` **且不发 IPC**——这消掉了"Service 系统性地写错设备"的那一类；**剩下的是预检与 daemon 解析之间的时间窗**，那需要 `NetworkSetDnsReq` 支持设备字段（上游），**本 PR 做不到**。
>
> **因此表里的措辞只声称"我们观察的那一刻"，R1 的后果描述补上了上面三条。**

---

## 9. 权限、提权与残留

### 9.1 提权：事实更正与裁定

> **v1 §9 写「`osascript` 不带 `administrator privileges`，全 crate 零命中」。这是假的**，且是**量词错误**——与 5d §2.4 记录的那次漏检同类：把"在我看过的地方零命中"写成了"在整个范围内不存在"。

**已核实的事实（逐条带锚点）：**

| 事实                                                                                                                                       | 锚点                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `utils/sudo.rs` 是一个**专门的 macOS 提权助手**：`osascript -e 'do shell script "bash <script> &> <out>" with administrator privileges'`   | `utils/sudo.rs:34-41`（`:37` 是那句字面串）                                          |
| 它**同步**执行：`std::process::Command::status()`                                                                                          | `utils/sudo.rs:41`                                                                   |
| 它把参数**用空格 join 进一条 bash 脚本行**                                                                                                 | `utils/sudo.rs:25-31`（`.join(" ")`）                                                |
| 它在**六个服务控制入口上都是活的**，每处包在 `tokio::task::spawn_blocking` 里                                                              | `control.rs:71,119,162,201,248`（macOS 分支）；`spawn_blocking` 形如 `control.rs:60` |
| `client/system_dns.rs` 的 DNS 缓存刷新脚本**同样带** `with administrator privileges`，同样同步                                             | `client/system_dns.rs:18-21`（`:20` 是那句字面串）、`:43-46`                         |
| **但 DNS 写路径本身不提权**：上游 `set_dns` 用的是不带 `administrator privileges` 的 `osascript`，且 `let _ = ... .status()?` 丢弃退出状态 | `nyanpasu-runtime/crates/nyanpasu-utils/src/network/mod.rs:47-53`                    |

**因此 v1 那句话要改成一句量词正确、且**仍然成立**的话：**

> **DNS 写路径不提权**——上游 `set_dns` 不带 `administrator privileges`，本 PR 新写的 Local 写也不带（裁定见下）。**crate 内存在提权机制（`utils/sudo.rs`、`client/system_dns.rs`），但它们不在 DNS 写路径上。**

**裁定：PR-5e 的 DNS 写路径不提权。** 三条理由，全部可核：

1. **在 S3 里它是自毁的。** `sudo()` 是同步的，从 async 调用必须经 `spawn_blocking`；而 `spawn_blocking` 的任务**结构上不可取消**——丢弃 `JoinHandle` 不会停掉闭包，因此 `timeout` 只界住等待、不界住工作。更致命的是它会**弹出交互式凭据对话框**，那个对话框**等人**，没有任何有限上界。放进 `Shutdown` 臂 = `DNS_RESTORE_BUDGET` 必然耗尽 ⇒ `ACTOR_STOP_BUDGET` 必然耗尽 ⇒ 5d §4.6.3 的 `AbandonedUnverified` ⇒ **清理被跳过**。**为了让恢复更可能成功而引入的机制，恰好会让恢复根本不运行。**
2. **恢复是自动路径。** 它在 `Stop` / `SetBackend` / 应用退出时触发。在退出路径上弹密码框是不可接受的交互。
3. **它会把 §5.1 拒绝上游代码的那个缺陷请回来。** `sudo()` 用空格 join 参数进 bash 脚本行（`:25-31`），而 **macOS 硬件端口名确实含空格**（`USB 10/100/1000 LAN`、`Thunderbolt Ethernet`、`iPhone USB`）。**修 `sudo()` 不在本 PR 范围**——它活在六个服务控制入口上，改它的引用方式就是改那六处的行为（CLAUDE.md §3）。

**这条裁定的代价，如实写出**：非管理员账户上，Local 写**必然失败**，且本 PR 不提供任何应用内的提权重试。R-C3-1 的用户指引因此只给手动路径。**记为 R-C3-5**——option-not-taken 是一次被记录的决定，不是遗漏。

### 9.2 「非管理员账户可能一直静默失效」——本 PR 让它第一次可见

`networksetup -setdnsservers` 至少需要 admin 组身份（man page + SO 双源，**不是从代码形状推断**）→ 而 DNS 写路径不提权（§9.1）→ 上游写路径又 `let _ =` 吞掉退出状态（`network/mod.rs:47`）→ **没有任何观测点**。所以「这个功能在非管理员账户上可能从来就没工作过」**不是推测，而是当前代码结构下必然无法被发现的一类失效**。

**加上退出码检查（§5.1）与回读校验（§4.3/§4.4）之后它会第一次变得可见。这不是我们引入的回归，但我们会是发现它的人。**

**判别方法**：在本 PR 之前的版本上用同一账户手动跑一次 `networksetup -setdnsservers`。

### 9.3 残留清单（**本节是唯一权威清单**）

| #          | 残留                                                                                                                                                                                                        | 性质                                                                                                                            | owner / 移除条件                                                                                                                                                  |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **R1**     | Service 模式下默认设备在**我们预检与 daemon 解析之间**变化 → 写落到 B。**对 A 的回读既检测不到也修不了 B；若 A 恰好已等于 `previous`，恢复循环会返回 `Restored` 而 B 被写坏**（§8 框）                      | **既有**；从"不可检测"改善为"可检测、窗口极窄"，**但新增了一条"成功报告掩盖 B 损坏"的路径，须在 PR 描述里写明**                 | 移除条件 = `NetworkSetDnsReq` 支持设备字段（上游 PR）                                                                                                             |
| **R1b**    | **Local 施加**的"读默认 → 写"窗口。v2 已加 ③' 再解析（§4.2），**窗口被缩短、未被关闭**                                                                                                                      | 同 R1                                                                                                                           | 同 R1；或把设备解析与写做成单次原子操作（`networksetup` 不提供）                                                                                                  |
| **R-C3-1** | 漂移已检测 + Local 不可用 → 设备 A 永久残留（§4.5）。**触发条件在 v2 扩大**：§4.2 情形 (c) 的旧设备恢复失败同样落此                                                                                         | **具名残留**，不属 R1                                                                                                           | §4.5 表；含用户可见指引。移除条件②的前提见 §9.1                                                                                                                   |
| **R-C3-2** | 强杀（SIGKILL / 任务管理器）后 DNS 覆写残留                                                                                                                                                                 | **既有**                                                                                                                        | PR-6：启动时检测并清理                                                                                                                                            |
| **R-C3-3** | **强杀残留被下次运行"洗白"成用户配置**：I-DNS-1（§4.2 ①'）只在残留**恰好等于**本次 desired 时拦住；若 TUN 设备 IP 在两次运行之间变了，旧 TUN 地址会被当作 `previous` 捕获，并在恢复时**写回去**             | **v2 新增**（v1 把它错并进 R-C3-2）。**它同时缩小了 PR-6 能修的范围**——一旦被捕获，残留与用户配置在内存里不可区分，原值已经丢了 | **owner = PR-6**。移除条件 = 施加时把 `{target, applied, previous}` **持久化到磁盘**，使下次启动能识别并撤销自己上一轮的覆写                                      |
| **R-C3-4** | **Phase 0 的观察来自 CI runner，不是用户机器**：外推依据是 `LC_ALL=C` 两侧一致，而该依据本身由 T-P0-01 第 4 项验证                                                                                          | **v2 新增**                                                                                                                     | 移除条件 = T-P0-01 判定采纳路线③（退出码判别，**不依赖任何文案**）                                                                                                |
| **R-C3-5** | **DNS 写路径不提权**（§9.1 裁定）⇒ 非管理员账户上 Local 写必然失败，且**没有应用内提权重试入口**                                                                                                            | **v2 新增；option-not-taken 的记账**                                                                                            | 移除条件 = ①`sudo()` 改为可取消的异步封装、②凭据对话框限定在**用户发起的前台路径**（绝不在 `Shutdown` 臂内）、③参数改为逐参 shell-quote。**三条都不在本 PR 范围** |
| **R-C3-6** | **关停超时 ⇒ S3 可能整个没跑**：`ACTOR_STOP_BUDGET` 耗尽时（例如 `Shutdown` 之前排着一个卡死的处理器）`Shutdown` 臂根本轮不到；5d 已撤回 `stop(None)`，且**不留 facade 侧 S3 槽位**，因此**没有第二次机会** | **v2 新增；5d R-C2-4 在本 PR 的对应面**                                                                                         | 用户可见后果与指引同 R-C3-1。移除条件 = PR-6 启动时清理；或后端 await 各自有界（5a 设计面）                                                                       |
| **R-C2-7** | 5d §4.4 第 2/4/6 行（收敛失败）到不了 S2 ⇒ 不重施加。**v2 已按 5d 的指派把它变成可见降级**（§4.8.2）                                                                                                        | **5d 冻结的契约后果，处置 owner 是本 PR**                                                                                       | 处置已交付（T-DNS-37）。**残余** = 覆写在下一次成功收敛之前始终缺席                                                                                               |
| **R6**     | 四态②的字面串未经验证                                                                                                                                                                                       | **Phase 0 前的开工约束**，不是永久残留                                                                                          | **§5.2 T-P0-01**：采集得到文案，或实验证成路线③后该分支不再依赖文案                                                                                               |

---

## 10. Exit 判据

| 要求                                                                                         | 验证                                                                                                                                                                                                                       |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Phase 0 已完成**：采集产物（stdout/stderr/退出码/provenance）存在，**或**路线③已由实验证成 | T-P0-01 的 artifact；**二者取一即可开工**（§5.2）                                                                                                                                                                          |
| **Phase 0 的临时 workflow 步骤已移除**                                                       | T-P0-02：`git diff --exit-code <base>..HEAD -- .github/workflows/` 为空                                                                                                                                                    |
| 四态读全覆盖（含混合输出）+ 解析器纪律                                                       | T-DNS-08/09/10/11/34/**45**                                                                                                                                                                                                |
| **恢复由回读证实推进**                                                                       | T-DNS-24/31                                                                                                                                                                                                                |
| **恢复不覆盖第三方改动**                                                                     | **T-DNS-39**；`RestoredUnverified` 的产出由 **T-DNS-44** 钉住                                                                                                                                                              |
| **候选序完整**                                                                               | T-DNS-16                                                                                                                                                                                                                   |
| **回读五行齐备且不交**                                                                       | T-DNS-01/28/29/30/35 + **T-DNS-36**（I-DNS-1）                                                                                                                                                                             |
| **重复施加不重新快照；设备变更先恢复**                                                       | T-DNS-21/23                                                                                                                                                                                                                |
| **记账早于写的发起**                                                                         | **T-DNS-19**（超时场景，非终态断言）                                                                                                                                                                                       |
| **actor 内 DNS I/O 两层有界**                                                                | T-DNS-32（逐次，接缝在 `MacosCommandRunner`）+ **T-DNS-38**（处理器整体）                                                                                                                                                  |
| **`ACTOR_STOP_BUDGET` 的预算义务已兑现**                                                     | §3.3：`DNS_RESTORE_BUDGET` 是**单一常量**且由 `Shutdown` 臂的外层 `timeout` 强制；5d 只需核对一个不等式。**须写进 PR 描述**                                                                                                |
| **重施加按结果状态触发；收敛失败可见降级**                                                   | T-DNS-33 + **T-DNS-37**（R-C2-7 的处置）                                                                                                                                                                                   |
| 六个入口都在控制动作前拆 DNS；uninstall 失败中止                                             | T-DNS-05/06/17/18/26/27（六条独立）                                                                                                                                                                                        |
| **恢复四个触发点保序**；S3 不发消息不取门                                                    | T-DNS-02/03/41/42 + §8 `rg` 门禁                                                                                                                                                                                           |
| **对 5d `apply_mode` 的改写不破坏 G-SEAM-02 / T-SEAM-02**                                    | §4.8.2 末尾的逐条核对；**须写进 PR 描述**（5d 的审查者不应自己去发现）                                                                                                                                                     |
| 非 macOS 不加空抽象                                                                          | cfg 门控                                                                                                                                                                                                                   |
| bindings diff 为空                                                                           | `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`                                                                                                                                                           |
| **§9.3 残留表的\*\*全部\*\*条目**逐条出现在 PR 描述里，**R-C3-1 含用户指引原文**             | 文本核对——「不修」必须是被记录的决定，不是沉默                                                                                                                                                                             |
| **提权裁定（§9.1）出现在 PR 描述里**，含三条理由与 R-C3-5                                    | 文本核对                                                                                                                                                                                                                   |
| **对 `design.md:337` 的有意偏离**（DNS 兄弟端口而非 `CoreBackend::Service`）出现在 PR 描述里 | 文本核对；`design.md` **本身不得修改**                                                                                                                                                                                     |
| **smoke 3**（macOS TUN/DNS）                                                                 | **不可由 CI 覆盖**——托管 runner 的能力边界（TUN 需签名扩展 + root），加 job 加 runner 都无效。**注意与 Phase 0 的区别**：Phase 0 采集的是 `networksetup` 的**只读输出**，不需要 TUN、不需要 root；结论进 PR 描述与发布说明 |

> ### 与 `design.md:337` 的偏离
>
> spec 写「Service 模式需要提权时由 `CoreBackend::Service` 调 IPC set_dns」，本设计用**独立兄弟端口**。
>
> **机制理由**：恢复所需的适配器**不总是当前后端那个**——§4.4 的固定候选序会在 Service 不可定向时用 Local，而 `Shutdown` 之后 `state.backend` 已是 `None`（`core/actor/mod.rs:606`，此后 `backend()` 返回 `ShuttingDown`）。**方法长在 `CoreBackend` 上则「用 Local 恢复一个 Service 时期的覆写」在类型上无从表达。**
>
> **洁净性理由**：`CoreBackend` 是全平台核进程生命周期枚举，挂 macOS-only 的 DNS 方法与拒绝往 `CoreRequest` 塞 TUN 字段同理。
>
> 两条**分开标注**：前者机制、后者洁净性。（v3 曾用 `replace_backend` 的 `take()` 论证，**已撤回**——`SetBackend` 的恢复排在 `replace_backend` 之前，那时后端还活着。）
