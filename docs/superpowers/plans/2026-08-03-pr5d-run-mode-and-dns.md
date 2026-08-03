# PR-5d 实施计划 — 运行模式探针与 macOS DNS 生命周期

**日期：** 2026-08-03
**版本：** v4（v3 对抗审 **REJECT 57/100**，43→57；六条 BLOCKING 全部维持，leader 另**升级一条**为 BLOCKING 并**推翻自己对 sibling-port 的接受理由**。本版按裁决重写）
**分支基线：** `refactor/core-manager-actor` @ **`59a38dfb0`**
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/` 下**两个文件都算数**——`task.md` 卡 C2、C3（做什么）+ `design.md` §9（边界约束，其中 `:333` Service control、`:337` macOS DNS 两段直接管着本阶段）。**只读 `task.md` 会漏掉约束**，§0.5 记了一次实际漏检
**上游材料：** PR-5c v4 终态 `git show 5a02a1727:docs/superpowers/plans/2026-08-02-pr5c-residual-cleanup.md`
**平台：** Windows 11 / PowerShell（**macOS 路径无法本地验证**，见 §10）

> **v2→v3 的教训是「把机制写成结论」。v3→v4 的教训更窄也更硬：把 `Err` 当成「什么都没发生」的证据。**
>
> 一次写调用返回 `Err`，只说明**这次调用报告了失败**，**不说明外部状态没被改动**——daemon 可能已经改完 DNS 才丢了响应，本地命令可能改完才非零退出。这与老纪律「签名只能保证值到得了这里」是同一形状，只是换到了返回值上：**错误通道报告的是调用的结果，永远不是副作用的缺席。** v3 据此写了「写失败就不建守卫」，恰好制造出守卫本身要防的那个状态：DNS 变了、没有记录、无从恢复。

---

## 0. 锚点复核结果

### 0.1 已漂移、本版已改写的锚点（对 `899b069f5`）

| 事实                                          | v2 锚点                                  | v4 锚点                                                                       | 说明                                                      |
| --------------------------------------------- | ---------------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------- |
| F13 `RunType::default()` 读两个 legacy global | `core/clash/core.rs:61-78`               | **`core/clash/core.rs:39-56`**                                                | 5c 削掉该文件约 75%                                       |
| F12 `get_ipc_state()` 第五处生产读            | `core/clash/core.rs:70`                  | **`core/clash/core.rs:48`**                                                   | 同上                                                      |
| F19 覆写代码 + `previous_dns` 状态            | `core/clash/core.rs:404-457`、`:373-383` | **`core/clash/core.rs:74-126`、`:61`、`:69`**                                 | 同上                                                      |
| F20 读两个 global、Service/Local 双路径分叉   | `core/clash/core.rs:409,415-420,440-450` | **`core/clash/core.rs:78`、`:84-89`、`:109-118`**                             | 同上                                                      |
| F15 `ServiceControlOps` 只有四个方法          | `core/actor/backend.rs:618-624`          | **`core/actor/backend.rs:619-624`**                                           | `:618` 是 `#[async_trait]`                                |
| F18 `MacosDnsGuard` 尚未存在的注释            | `feat.rs:417-418`                        | **`feat.rs:416-418`**                                                         | 行号未动，**文案已由 `a062f1019`/`59a38dfb0` 改指 PR-5d** |
| F22 DNS 与 start/stop 无保序                  | `feat.rs:409-426`                        | **`feat.rs:410-412`（restart 分支不碰 DNS）、`:415-424`（`let _ =` 吞失败）** | 拆成两条精确锚点                                          |
| F36 一次性 status 查询                        | `control.rs:351-376`                     | **`control.rs:350-376`**                                                      | `:350` 是 `#[tracing::instrument]`                        |
| **F42 actor 内多余的 mode 覆盖赋值**          | v3 写 `mod.rs:370`                       | **`core/actor/mod.rs:371`**                                                   | **v3 差一行，审查者纠正，已核实**                         |

### 0.2 已核实**未**漂移的锚点

`core/actor/gate.rs:20-30`、`:32-45`、`:55-60`；`core/actor/request.rs:78-92`（`:87` 取守卫、`:88` `set_backend`、`:82-85` 提前返回）；`core/actor/types.rs:44-50`、`:68-79`；`core/actor/mod.rs:185-190`、`:224-230`、`:266-296`、`:436`、`:500-539`、`:603-615`；`core/service/ipc.rs:28-30`、`:85-101`（`:97` 是 5 s）、`:103-124`（`:108` 是警告条件）、`:131-138`；`core/service/mod.rs:18-30`；`core/service/compat.rs:15-27`、`:29-52`、`:55-57`；`client/mod.rs:303-306`、`:455-465`、`:504-539`、`:544`；`client/core.rs:277-283`；`client/rebuild.rs:150-167`；`feat.rs:383,401`；`utils/init/mod.rs:251`；`utils/help.rs:263`；`.github/workflows/ci.yml:201-215,303-304`。

### 0.3 5c 已消灭的锚点

- `core/clash/core.rs:399`（`CoreManager::status` 内的 `RunType::default()`）——函数已删。`RunType::default()` 的生产调用点现为**两处**（`core/actor/types.rs:48`、`core/clash/core.rs:78`）加一处测试、一处注释。

### 0.4 卡面与前版都没有的事实

| ID      | 事实                                                                                                                                                                                                                                                       | 锚点                                                        |
| ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| **F42** | **`CoreStatusView::initial()` 有两个调用点**。`core/actor/mod.rs:367-371` 先调 `initial()` 再用 `observed.view.run_type = args.mode;`（**`:371`**）覆盖——actor 侧早已是注入的，`initial()` 里的 `RunType::default()` 在这条路径上是**一次白读**            | `client/core.rs:111`；`core/actor/mod.rs:367-371`           |
| **F43** | `replace_backend` 置 `self.running = None`（`:268`）在前、改 `self.mode`（`:282`）在后                                                                                                                                                                     | `core/actor/mod.rs:266,268,282`                             |
| **F44** | **`state.running: Option<CoreRequest>` 是现成的「核是否在跑」判据**：`Run` 置 `Some`（`:514`）、`Stop` 置 `None`（`:532`）、`Shutdown` 置 `None`（`:605`）                                                                                                 | `core/actor/mod.rs:57`                                      |
| **F45** | 六个控制入口签名不齐；`update`/`uninstall` **不在 trait 上**                                                                                                                                                                                               | `control.rs:58,106,149,188,234,283`；`backend.rs:619-624`   |
| **F46** | **`nyanpasu-utils` 全 crate 无 `administrator privileges`**                                                                                                                                                                                                | `crates/nyanpasu-utils/`（全 crate grep）                   |
| **F47** | IPC `set_dns` 的 wire golden 存在，但不在 v2 引的锚点上                                                                                                                                                                                                    | golden：`nyanpasu_ipc/tests/wire_golden.rs:282-295`         |
| **F48** | `pnpm test` → `cargo test --all-features` 是两跳                                                                                                                                                                                                           | `package.json:40` → `:42`                                   |
| **F49** | **install 之后服务会自己起来**，且紧接着拉起 health checker                                                                                                                                                                                                | `control.rs:99-102`                                         |
| **F56** | **IPC 里根本没有 DNS 读端点。** `nyanpasu_ipc/src/api/network/` 只有 `mod.rs` 与 `set_dns.rs`；全 crate 唯一 network 端点常量是 `NETWORK_SET_DNS_ENDPOINT`；`get_dns`/`read_dns` 在 `nyanpasu_ipc/src` 与 `crates/nyanpasu-service-runtime/src` **零命中** | `nyanpasu_ipc/src/api/network/`（目录列举）；`set_dns.rs:5` |
| **F57** | **`commit()` 在观察到 `Stopped` 时也会清 `running`**（`:224-227`）。因此 `replace_backend` 里那句显式 `self.running = None` **不是唯一清除点**——紧随其后的 `commit(synthetic_stopped)`（`:270`）同样会清                                                   | `core/actor/mod.rs:224-230,270`                             |
| **F58** | **facade 已有 `NyanpasuClient::shutdown()`**，且已是「有序两步」（先 rebuild worker 后 core），带 PR-5a S11 契约注释；生产入口是 `utils/help.rs:263`                                                                                                       | `client/mod.rs:455-465`；`utils/help.rs:263`                |
| **F59** | **`ServiceCompat::Unknown` 有两个来源**：`status != Running`（`compat.rs:32-34`）与 `status == Running` 但 `info.server` 为 `None`（`:35-37`）。**丢掉 `ServiceStatus` 就再也分不出这两种 `Unknown`**                                                      | `core/service/compat.rs:29-52`                              |
| **F60** | **`RebuildCoordinator::shutdown` 就是「关准入 + 等在飞」的现成范式**：先 `active.store(false)` 再 `done_rx.await`；契约明写「已在飞的 rebuild 允许跑完」                                                                                                   | `client/rebuild.rs:150-167`                                 |

### 0.5 复核期间落地的 5c 收尾五提交

主体复核在 `48c17a705` 完成，其后五个提交落地：

| 提交                     | 改了什么                                                                  | 影响                                                                                                                                                  |
| ------------------------ | ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `0e20f35ba`、`b3fe68035` | ledger 扫描器：Rust 字符字面量/裸字符串词法、不再把字面量里的指标文本计数 | **无锚点影响**，且**加固**了 §8 依赖的两条 ledger 门禁                                                                                                |
| `a86478a7f`              | 重写 roadmap §6.3：C2/C3 移交 PR-5d，指名本计划为权威实施计划             | 见下方勘误                                                                                                                                            |
| `a062f1019`、`59a38dfb0` | `feat.rs` 与 `core/service/ipc.rs` 的迁移标记改指 PR-5d、修正陈旧 reason  | **行号未动**（`feat.rs:416-418`、`ipc.rs:126-128`）；`change_default_network_dns` 本体与 `let _ =` 吞错行为**一行未变**，F19/F20/F22/F40/F41 全部成立 |

> **勘误（v3 曾写反，保留记录）**：v3 初稿称「不引入完整 `ServiceControlPort`」只存在于 roadmap 且已被 `a86478a7f` 删除，证据是 `rg` 零命中。**那次 `rg` 只扫了 roadmap 与 `task.md` 两个文件，却把结论写成了「该约束不存在」。**
>
> **真相**：正本在 **`design.md:333`**，受版本控制，`git log 48c17a705..HEAD -- .../design.md` **为空**，`a86478a7f --stat` 只有 roadmap 一个文件。**roadmap 持有的是拷贝，约束一直活着。**
>
> **方法论**：「在 X 与 Y 上零命中」与「该约束不存在」是两个命题，只有后者能撑论证。**下断言前先把量词范围定死，再让检索覆盖整个范围。** 计划头部原先只写「权威 spec：`task.md`」，把同目录 `design.md` 结构性排除在视野外，是这次漏检的真成因，已改正。

---

## 1. 已核验事实

### 1.1 C2 —— 运行模式

| ID      | 事实                                                                                                                                                                                      | 锚点                                                                                                                   |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| F9      | `pending_run_type` 在 Rust 源码中**不存在**                                                                                                                                               | 全仓 `rg`（命中仅 `docs/`）                                                                                            |
| F10     | 「reconcile 走 `CoreOperationGuard`」已满足                                                                                                                                               | `core/actor/request.rs:87`                                                                                             |
| F11     | 5 s 轮询与三个 statics 在一个文件；`spawn_health_check` **定义**在 `ipc.rs`，**四处调用点在别处**                                                                                         | 定义与 statics：`ipc.rs:28-30,85-101`（`:97` 是 5 s）；**调用点：`control.rs:101,229,324` + `core/service/mod.rs:25`** |
| F12     | `get_ipc_state()` **5 处生产读**                                                                                                                                                          | `feat.rs:383,401`；`client/mod.rs:305,544`；`core/clash/core.rs:48`                                                    |
| F13     | `RunType::default()` 读两个 legacy global，被 `CoreStatusView::initial()` 调用                                                                                                            | `core/clash/core.rs:39-56`；`core/actor/types.rs:44-50`                                                                |
| F14     | `set_backend` 生产调用点恰好一个；**不存在 `set_mode`**                                                                                                                                   | `core/actor/request.rs:88`                                                                                             |
| F16     | `uninstall_service` **绕过 facade**；`install_service` 在 facade 上**不 reconcile**                                                                                                       | `ipc.rs:936-937`；`client/mod.rs:504-510`                                                                              |
| F35     | **`IPC_STATE` 初值 `Disconnected`**，bootstrap 在任何 health check 之前读它 → **今天 bootstrap 恒判 `Normal`**                                                                            | `ipc.rs:28`；`client/mod.rs:303-306`                                                                                   |
| F36     | 探针两半已存在：`control::status()`（子进程）+ 纯函数 `target_ipc_state()`                                                                                                                | `control.rs:350-376`；`ipc.rs:131-138`、`:103-124`                                                                     |
| **F61** | **今天的三个 facade 控制方法「无论控制成败都 reconcile」**：`let control = …; self.reconcile_service_mode().await; control?;`——reconcile **先跑**，控制错误**之后**才返回                 | `client/mod.rs:512-538`                                                                                                |
| **F62** | **今天的警告条件是合取式**：`info.status == ServiceStatus::Running && !compat.allows_service_backend()`。结合 F59，它覆盖 Running 下的 `Unknown` / `Incompatible` / `Unparsable` **三种** | `ipc.rs:108`                                                                                                           |

### 1.2 C3 —— macOS DNS

| ID      | 事实                                                                                                             | 锚点                                                                    |
| ------- | ---------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| F18     | `MacosDnsGuard` 不存在（仅一条迁移标记，现指向 PR-5d = **本计划就是它的移除条件**）                              | `feat.rs:416-418`                                                       |
| F19     | 真正的覆写代码是 `CoreManager::change_default_network_dns` + `previous_dns`                                      | `core/clash/core.rs:74-126`、`:61`、`:69`                               |
| F20     | 读两个 global，Service / Local 双路径在此分叉                                                                    | `core/clash/core.rs:78`、`:84-89`、`:109-118`                           |
| F21     | IPC `set_dns` 已上线（端点 + wire golden，**两个锚点**）                                                         | `shortcuts.rs:91-96`；`tests/wire_golden.rs:282-295`                    |
| F22     | DNS 与 start/stop 今天毫无保序；**restart 分支根本不碰 DNS**；失败被 `let _ =` 吞掉                              | `feat.rs:410-412`；`:415-424`                                           |
| F23     | 退出不恢复 DNS——覆写跨崩溃/退出泄漏（**既有缺陷**）                                                              | `utils/resolve.rs:288-291`；`client/core.rs:277-283`                    |
| F24     | `SystemDnsCache` 只管 flush，与 TUN 覆写生命周期无关                                                             | `client/system_dns.rs:4-7`                                              |
| F40     | **Local 写路径不提权**：`osascript` 不带 `administrator privileges`                                              | `nyanpasu-utils/src/network/mod.rs:27-55`；`scripts/set-macos-dns.sh:3` |
| F41     | **读路径不检查退出码**，空/不可解析 stdout → `Ok(None)`                                                          | `nyanpasu-utils/src/network/mod.rs:57-88`（判定在 `:82-87`）            |
| F50     | 设备名被文本拼进 bash：`include_str!(..).replace("$1", service_name)`，脚本里 **`$1` 未加引号**                  | `network/mod.rs:27-55`；`scripts/set-macos-dns.sh:3`                    |
| F51     | **Service 写的线上契约里没有设备**：`NetworkSetDnsReq { dns_servers }`；服务端**每次请求**自解析当前默认硬件端口 | `set_dns.rs:9-11`；`routing/network.rs:26`、`:39`                       |
| F52     | 上游读脚本 `echo $RES` 未加引号 → 换行被压成空格                                                                 | `scripts/get-macos-dns.sh:3-4`                                          |
| **F56** | **IPC 没有 DNS 读端点**（见 §0.4）                                                                               | `nyanpasu_ipc/src/api/network/`                                         |

### 1.3 smoke / CI

| ID  | 事实                                                                            | 锚点                                                    |
| --- | ------------------------------------------------------------------------------- | ------------------------------------------------------- |
| F33 | CI 有 macOS runner 且在 PR 上跑 `cargo test --all-features`                     | `ci.yml:201-215`、`:303-304`；`package.json:40` → `:42` |
| F34 | **但没有任何作业能跑 smoke 3**——TUN 需签名扩展 + root，**是能力边界非配置缺失** | `ci.yml`                                                |

### 1.4 平台事实（**从文档确立，不从代码形状推断**）

| ID      | 事实                                                                                                                                              | 依据                                                                                                                                                                                         | 置信度                               |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| F53     | **`networksetup` 的写操作至少需要 admin 组身份**；若开启「Require an administrator password to access system-wide preferences」则需 root          | ①man page（ss64 转录：_"requires at least admin privileges to **change** network settings…"_ https://ss64.com/mac/networksetup.html）；②SO 11819336 记录真实授权弹窗，公认解法是 `sudo`/suid | **高**（双源一致；来源二是社区证据） |
| F54     | **`osascript` 那一跳不改变权限**（同用户身份执行，且不带 `administrator privileges`）                                                             | F46 + F53                                                                                                                                                                                    | **高**                               |
| F55     | **`networksetup -getdnsservers` 无 DNS 时的确切文案，未能从可引用来源确立**                                                                       | 三轮检索均未给出该字面串                                                                                                                                                                     | —                                    |
| **F63** | **`networksetup` 的读操作是否需要 admin，同样未能确立。** man page 那句的动词是 **change**，只谈改；**没有**任何来源正面陈述 get 子命令的权限要求 | 两轮定向检索（man page 转录 / SE / GitHub 脚本）无正面陈述                                                                                                                                   | **未确立**                           |

> **F63 必须显式记账。** 直觉上「读大概率不需要提权」，但**这正是本文件上反复出错的那一类推断**。§4.6 的设计因此被写成**对 F63 的答案不敏感**：读被拒绝就是四态①的 `Err`，与任何其它读失败同路。**F63 只决定「验证在多少账户上真能用」，不决定设计是否安全。**

---

## 2. 已裁定事项

### 2.1 D2 = A —— `CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`

| `RunType::default()` 调用点                 | 处置                                                                                                                                                   |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `core/actor/types.rs:48`                    | D2 主目标，改为参数                                                                                                                                    |
| `core/clash/core.rs:78`                     | 随 C3 迁走（该 `CoreManager` 整体消失）                                                                                                                |
| `client/core.rs:1211`（测试）               | 断言注入的 mode，**并改名**为 `initial_watch_snapshot_reflects_the_injected_mode`——旧名的「legacy empty status」在 D2 之后不再是参照物，**命名即契约** |
| `client/process_core_bridge.rs:251`（注释） | 删后悬空，顺手清理                                                                                                                                     |

| `CoreStatusView::initial()` 调用点（F42） | 处置                                                  |
| ----------------------------------------- | ----------------------------------------------------- |
| `client/core.rs:111`                      | 改 `initial(args.mode)`                               |
| `core/actor/mod.rs:367`                   | 改 `initial(args.mode)`，**并删掉 `:371` 的覆盖赋值** |

### 2.2 D3 = A —— DNS guard 挂 actor state，但 `Drop` 不恢复

**主路径**（`Stop` / `Shutdown` / `SetBackend`）：处理器内**显式 `await` 恢复**，在后端动作与 reply **之前**完成。

**`Drop`**：**只记 `tracing::error!`，措辞按不变量破坏写**，**不尝试任何恢复**。

> **为什么不做「尽力而为的同步 Drop」**：Service 侧同步做不到、Local 侧能做——**那半个兜底恰好在开发者最常用的模式下生效**，会系统性地把主路径 bug 藏到用户实际部署的模式才暴露。**「在你测得到的地方生效、在你测不到的地方失效」的兜底是反向选择。**

**恢复失败去向**：degradation sink（`DegradationPhase::CoreLifecycle`、`code = "macos_dns_restore_failed"`）。`Degradation` 形状见 `client/runtime.rs:376-382`，`CoreLifecycle` 见 `:398`。

**`Drop` 不覆盖强杀**（SIGKILL / 任务管理器）——如实写明，兜底属 **PR-6**。

### 2.3 D4 —— smoke 3 记为「未在本地验证**且不可由 CI 覆盖**」

**不是「CI 暂未配置」，是托管 runner 的能力边界**（F34）：加 job、加 runner 都解决不了，需自托管 mac 且预先批准网络扩展。

**CI 覆盖的**：cfg 门控单测（顺序、降级等逻辑契约，F33）。
**未验证的**：①真实 TUN 开关是否触发覆写；②真实 `networksetup` / IPC `set_dns` 是否成功改写系统 DNS；③关 TUN 与正常退出后 DNS 是否真的恢复；④Service 与 Local 两条路径在真机上是否一致；⑤F55 的无-DNS 文案；⑥**F63 的读权限**。

**结论必须显式出现在 PR 描述与发布说明里。**

### 2.4 §7 两处不对称 —— **两处都是缺陷**

> **更正**：v2 §2.5 曾称「`install_service` 不 reconcile 是有意的，因为装服务不等于起服务」。**F49 证明这在基线上就是假的**（`control.rs:99-102` 明写多数平台自动启动并拉起 health checker）。**该 carve-out 整条删除。**

| 项                                         | 裁定                                                                                                   |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `install_service` 在 facade 上不 reconcile | **缺陷，改为与另外五个同形**                                                                           |
| `uninstall_service` 绕过 facade            | **缺陷，改走 facade**——①违反「Tauri 命令是薄适配器」；②核在 Service 模式运行时卸载服务会让当前后端失效 |

**六个入口如何统一：**

| 入口                              | 今天                                      | 到齐的动作                                                 |
| --------------------------------- | ----------------------------------------- | ---------------------------------------------------------- |
| `install_service` `control.rs:58` | 收 reconciler（只为在 `:100-102` 起轮询） | **删参**；facade 走统一序列                                |
| `start_service` `:188`            | 收 reconciler                             | **删参**；facade 序列                                      |
| `restart_service` `:283`          | 收 reconciler；**DNS 不拆**（F22）        | **删参**；facade 序列 + DNS 拆除                           |
| `stop_service` `:234`             | 不收                                      | 上 trait；facade 序列                                      |
| `update_service` `:106`           | 不收；调用点在 `utils/init/mod.rs:251`    | 上 trait；**改由 facade 调用**；facade 序列 + 有界等待就绪 |
| `uninstall_service` `:149`        | 不收；被 `ipc.rs:936-937` 直调            | 上 trait；**改由 facade 调用**；facade 序列                |

**结果：六个签名一致（`async fn(&self) -> anyhow::Result<()>`），六个都在 `ServiceControlOps` 上。**

> **这与 `design.md:333`「不引入完整 `ServiceControlPort`，除非测试确实需要替换 OS command runner」冲突吗？不冲突。** 该约束**依然生效**，下面两条论证**各自独立成立**：
>
> **论证一（判例，主）**：`plans/2026-08-02-pr5a-core-actor.md:1037` 已确立读法——「design §9 说的"不引入完整 `ServiceControlPort`"针对的是把 service 管理**迁进 CoreActor**；这里只给既有函数加一层可测边界，所有权仍在 `core::service::control`，不迁移」。5a 正是在此读法下建起今天的四方法 trait，**经十二轮对抗审查无人异议**。PR-5d 补上 `update`/`uninstall`，**六个具体函数一行不搬**，仍是「给既有函数加一层可测边界」。**这条论证与任何文档增删无关。**
>
> **论证二（例外条款，条件已证成）**：即便按最严读法，例外是「除非测试确实需要替换 OS command runner」。**例外只有条件被证成才算数**：
>
> | 测试            | 为什么必须能替换 runner                                                           | 缺哪个方法就写不出来  |
> | --------------- | --------------------------------------------------------------------------------- | --------------------- |
> | T-MODE-02       | 六个控制动作**各自独立断言** probe+reconcile                                      | `update`、`uninstall` |
> | T-DNS-06        | uninstall 拆 DNS 顺序 + **失败时中止卸载**；中止分支要断言 uninstall **未被调用** | `uninstall`           |
> | T-DNS-18        | update 拆 DNS 顺序                                                                | `update`              |
> | T-MODE-04/05    | 有界等待的成功/超时/挂死三路，需在无真实 daemon 下让 update 返回                  | `update`              |
> | **T-CTL-01…04** | **控制失败的四种处置**（§3.5），需让控制动作**按脚本失败**                        | 六个全部              |
>
> **为什么真实 runner 不行**：`update_service`（`control.rs:106-147`）与 `uninstall_service`（`:149-186`）经 `runas`/`sudo` 提权调真实服务二进制。CI 三平台上要么二进制不存在、要么触发提权交互——**这类测试在 CI 里根本跑不起来**（与 F34 同类能力边界）。
>
> **既有四方法已够用的**（如实划清）：T-DNS-05（stop）、T-DNS-17（restart）、T-MODE-03。
>
> **仍须写进 PR 描述**：trait 由四扩到六是**对既有边界的可见扩大**。

---

## 3. C2 设计 —— 探针、调用点与失败处置

### 3.1 探针（一次性、经兼容门控、**自带有界性**）

> **v3 的 `(IpcState, ServiceCompat, Option<Error>)` 元组有两个洞**：①丢掉 `ServiceStatus` 就无法复现基线警告条件（F59 + F62）；②有界性只做在 `await_service_ready` 里，**其余三处调用点各自裸 await**。

```rust
// core/service/probe.rs（新）
pub(crate) struct ProbeOutcome {
    pub state: IpcState,
    pub compat: ServiceCompat,
    /// `None` = 这次探针自身失败（子进程错误 / 超时），连 status 都没拿到。
    /// 保留它才能复现基线的合取式警告条件（F62），因为 `ServiceCompat::Unknown`
    /// 同时来自「没在跑」与「在跑但没上报 server」两种情形（F59）。
    pub daemon_status: Option<ServiceStatus>,
    pub error: Option<anyhow::Error>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait ServiceProbe: Send + Sync + 'static {
    /// 一次性查询，**自身有界**：实现内部对 status 子进程套 PER_PROBE_BUDGET。
    /// 失败与超时都按 fail-closed 处理为 Disconnected（与今天 health_check 的
    /// Err 分支同语义）。
    async fn probe(&self) -> ProbeOutcome;
}

pub(crate) struct OsServiceProbe;
```

**有界性放在实现内部，不放在调用点：**

```rust
// OsServiceProbe::probe
match tokio::time::timeout(PER_PROBE_BUDGET, control::status()).await {
    Ok(Ok(info))  => { let (state, compat) = target_ipc_state(&info);
                       ProbeOutcome { state, compat, daemon_status: Some(info.status), error: None } }
    Ok(Err(e))    => ProbeOutcome { state: Disconnected, compat: Unknown, daemon_status: None, error: Some(e) }
    Err(_elapsed) => ProbeOutcome { state: Disconnected, compat: Unknown, daemon_status: None,
                                    error: Some(anyhow!("probe timed out")) }
}
```

> **为什么必须在源头**：「每个调用方都要包一层 timeout」是**「不会忘记」型契约**，而那正是**无法强制**的一类（与 §8 的口诀同源）。四个调用点里有三个（`reconcile`、`reconcile_with`、bootstrap）**持着操作许可或阻塞着启动**，一次挂死的 `status --json` 会把核心门**无限期占住**——新的取门请求会超时，而**活跃那个永不释放**。源头一处 timeout 消灭整类问题。

**`control::status()` 另须设 `.kill_on_drop(true)`**（`control.rs:352-356` 今天没设）：`timeout` 丢弃 future 只取消我们的等待，**`tokio::process::Command` 默认不杀子进程**。两者缺一，「有界」就只界住了自己。

**`target_ipc_state` 与 `ServiceCompat` 一行不改**——PR-5-pre 已审的 fail-closed 门。

**注入路径（逐跳）：**

```text
composition root: client/mod.rs::try_new_with_args
  └─ ClientSetupArgs { .., probe: Arc<dyn ServiceProbe>, .. }      ← 新字段，紧挨 service_control（client/mod.rs:85）
       ├─ ①bootstrap 自用：client/mod.rs:303-306 的 get_ipc_state() 换成 probe.probe().await
       └─ NyanpasuClientInner { .., probe }                        ← 新字段，紧挨 service_control（:257）
            └─ core_mode_reconciler()（:467-473）字面量加 probe: self.inner.probe.clone()
                 └─ CoreModeReconciler { core, application, requests, clash_config, probe }（request.rs:70-75）
```

`CoreModeReconciler` 是 `#[derive(Clone)]`，加 `Arc<dyn ServiceProbe>` 不破坏 Clone。测试侧沿用 `test_service_control()`（`client/mod.rs:2767`）的模式加 `test_service_probe()`。

### 3.2 九处调用点

**统一形态（六个控制入口，无例外）：**

```text
admission.enter()?                             ← §4.9 的准入许可（shutdown 已关则 Err）
guard = core.begin_operation().await?
  ├─ 拆 DNS（await；§4.4，六个都拆）
  ├─ admission.check_open()?                   ← 紧贴外部命令之前再查一次（§4.9）
  ├─ result = service_control.<action>().await ← **不早退**，见 §3.5
  ├─ [仅 update 且 result.is_ok()] 有界等待就绪（§3.4）
  ├─ reconciler.reconcile_with(&guard).await   ← **无论 result 成败都跑**（F61）
  ├─ [若 TUN 仍需开启] 重新施加 DNS（§4.5）
  └─ drop(guard) / drop(admission permit)
```

| #   | 位置                                                      | 今天                                        | 改为                                                                    |
| --- | --------------------------------------------------------- | ------------------------------------------- | ----------------------------------------------------------------------- |
| 1   | **bootstrap**（`client/mod.rs:303`）                      | `get_ipc_state()`（恒 `Disconnected`，F35） | `probe()` 一次——**顺带修掉 F35**。**唯一不在守卫内的探针**，理由见 §3.6 |
| 2   | **install**（facade `:504-510`）                          | 不 reconcile（F16）                         | 统一形态                                                                |
| 3   | **start**（`:512-521`）                                   | 轮询 + `reconcile(get_ipc_state())`         | 统一形态                                                                |
| 4   | **restart**（`:530-539`）                                 | 同上                                        | 统一形态                                                                |
| 5   | **stop**（`:523-528`）                                    | 同上                                        | 统一形态                                                                |
| 6   | **uninstall**（今在 `ipc.rs:936-937`）                    | 无                                          | **迁到 facade** + 统一形态                                              |
| 7   | **update**（今在 `utils/init/mod.rs:251`）                | 轮询                                        | **迁到 facade** + 统一形态 + 有界等待——**直接关系 smoke 2**             |
| 8   | **`enable_service_mode` 配置变更后**                      | 轮询 + reconcile（有 §3.7 的洞）            | `reconcile()`（自取守卫版）                                             |
| 9   | **boot 的 `init_service`**（`core/service/mod.rs:18-30`） | 起轮询线程 + 忙等 100 ms                    | `reconcile()`，**删忙等与整个函数**                                     |

### 3.3 探针输出的接手方（**复现基线语义，不是收窄它**）

| 输出条件                                                                                                                                                                              | 接手方                                                 | 动作                                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| **`daemon_status == Some(Running) && !compat.allows_service_backend()`**——**逐字复现 `ipc.rs:108` 的合取式**，覆盖 Running 下的 `Unknown`/`Incompatible`/`Unparsable` 三种（F59/F62） | **`CoreModeReconciler` 内、`classify` 之前的唯一一处** | `tracing::warn!(?compat, ..)`（smoke 2 要的就是这一条）                                                          |
| `error.is_some()`（探针自身失败）                                                                                                                                                     | **同一处**                                             | `tracing::warn!` **+ degradation**：`phase = CoreLifecycle`、`code = "service_probe_failed"`、`retryable = true` |

> **v3 只 warn `Incompatible`，是对基线的收窄。** 一个上报 Running 却不给 server 信息的 daemon（`Unknown`）、或版本串非法的 daemon（`Unparsable`），今天都会告警，v3 之后都会静默。**迁移不得顺手砍掉可观测性。**

**`await_service_ready` 的每一次非就绪结果也必须走同一个接手方**（v3 用 `_` 通配把它们全丢了）：轮询期间观察到的不兼容/探针错误要么即时告警、要么在收敛时汇总输出一次，**不能在 `force_local_with` 之前被丢弃**——那正是 smoke 2 要看的诊断。

### 3.4 update 的有界等待就绪

```rust
// CoreModeReconciler::await_service_ready(&self, guard: &CoreOperationGuard) -> ReadyOutcome
let deadline = Instant::now() + READY_BUDGET;
let mut backoff = INITIAL_BACKOFF;
let mut last: Option<ProbeOutcome> = None;
loop {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { return ReadyOutcome::TimedOut { last }; }
    let outcome = self.probe.probe().await;      // 探针自身已有界（§3.1）
    self.report_probe_diagnostics(&outcome);     // ← §3.3 的同一接手方，不再吞
    if outcome.state == IpcState::Connected && outcome.compat.allows_service_backend() {
        return ReadyOutcome::Ready;
    }
    last = Some(outcome);
    tokio::time::sleep(backoff.min(deadline.saturating_duration_since(Instant::now()))).await;
    backoff = (backoff * 2).min(MAX_BACKOFF);
}
```

| 要求                             | 构造                                                                                                                                          |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| 外层 deadline 能**取消在飞探针** | 探针自身的 `PER_PROBE_BUDGET`（§3.1）保证单次必然返回；外层 `READY_BUDGET` 因此必然可达                                                       |
| 明确退避                         | 指数退避封顶 `MAX_BACKOFF`；sleep 再与剩余预算取 min                                                                                          |
| 超时后**仍持守卫**降级到 Local   | `ReadyOutcome::TimedOut` → `force_local_with(&guard)` + degradation `service_update_not_ready`、`retryable = true`。**控制动作本身返回 `Ok`** |
| 诊断不丢                         | 每轮 `report_probe_diagnostics`                                                                                                               |

**为什么超时是 degraded 而不是 `Err`**：更新进程本身成功退出了。返回 `Err` 等于告诉用户「更新失败了」，而那是假的。Local 是**合法运行状态**（PR-5-pre 的 fail-closed 门本就如此）。与 5b 的 I-A 同源：**已经成功的事不许报成失败；没做成的后置副作用报降级。**

**常量来源分开记**：

| 常量                              | 实测？ | 依据                                                                                                 |
| --------------------------------- | ------ | ---------------------------------------------------------------------------------------------------- |
| `READY_BUDGET`                    | **是** | 实测 daemon 从 `update_service()` 返回到 `status()` 报兼容的耗时，取上界留余量，**依据写进实施报告** |
| `PER_PROBE_BUDGET`                | **是** | 实测一次正常 `control::status()` 子进程往返上界                                                      |
| `QUIESCE_BUDGET`（§4.9）          | **是** | 实测一次最慢控制动作（`update`）的正常耗时上界                                                       |
| `INITIAL_BACKOFF` / `MAX_BACKOFF` | 否     | 不是正确性边界（正确性由前三者界住），**如实标注为选定值**                                           |

### 3.5 控制动作失败的处置（**v3 只画了成功路径**）

**基线行为必须保留**（F61）：`start`/`stop`/`restart` 今天**无论控制成败都 reconcile**，控制错误**之后**才返回。理由是实的：runner 可能**部分**启动/停止/替换了 daemon 后才非零退出——立即返回会把陈旧的后端判断留到某个无关的后续操作才纠正。

**完整处置表：**

| #   | 控制动作  | 就绪等待（仅 update） | reconcile                    | DNS 重施加 | **返回**                                          | degradation                                          |
| --- | --------- | --------------------- | ---------------------------- | ---------- | ------------------------------------------------- | ---------------------------------------------------- |
| 1   | `Ok`      | Ready / 不适用        | `Ok`                         | `Ok`       | `Ok`                                              | —                                                    |
| 2   | `Ok`      | Ready / 不适用        | `Ok`                         | `Err`      | **`Ok`**                                          | `macos_dns_reapply_failed`                           |
| 3   | `Ok`      | Ready / 不适用        | `Err`                        | 跳过       | **reconcile 的 `Err`**                            | —                                                    |
| 4   | `Ok`      | **TimedOut**          | `force_local_with` `Ok`      | 跳过       | **`Ok`**                                          | `service_update_not_ready`                           |
| 5   | `Ok`      | TimedOut              | `force_local_with` **`Err`** | 跳过       | **`Err`**                                         | `service_update_not_ready` + `mode_reconcile_failed` |
| 6   | **`Err`** | **跳过**              | **照跑**                     | 跳过       | **控制动作的 `Err`**（优先于 reconcile 的 `Err`） | reconcile 若也失败 → `mode_reconcile_failed`         |

**三条定则：**

1. **控制错误优先级最高**——与基线 `control?` 在 reconcile 之后的写法一致。用户问的是「我这次操作成没成」。
2. **控制失败后跳过就绪等待**：`update_service` 都失败了，等一个新 daemon 就绪没有意义；直接 reconcile 去观察现实。
3. **控制失败后仍然 reconcile**：这是**唯一**能把「部分生效」的现实同步回来的机会。

**第 2 行为什么返回 `Ok`**：控制动作与模式收敛都成功了，只是 TUN 的 DNS 没能重新施加。**已经成功的事不许报成失败**；DNS 缺失以降级呈现，守卫状态按 §4.2 保留。

### 3.6 reconcile 的三个签名

```rust
impl CoreModeReconciler {
    /// 自取守卫 → **在守卫内探针** → 应用。唯一的无守卫入口（#8、#9）。
    pub(crate) async fn reconcile(&self) -> anyhow::Result<()>;
    /// 已持守卫：**在守卫内探针** → 应用。控制动作（#2..#7）用这个。**没有 IpcState 参数。**
    pub(crate) async fn reconcile_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()>;
    /// 已持守卫、**不探针**、直接落 Local。**仅供 §3.4 超时分支**。
    pub(crate) async fn force_local_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()>;
}
```

**强制构造**：`reconcile`/`reconcile_with` **没有 `IpcState` 参数**——调用方在类型上就无法喂进陈旧探针结果。这是签名能给的那一类保证。

**「任何探针都不许在守卫外开始」是「不会去做某事」型契约**，落到 §9 的 ledger 门禁：`rg -n '\.probe\(\)'` 恰好三处（`reconcile_with` 内、`await_service_ready` 内、bootstrap）。

**bootstrap 是唯一守卫外探针，理由是真排除**：它在 `client/mod.rs:303`，而 `CoreClient::new` 在 `:312`——**actor 那时还不存在**，没有任何别的操作能在飞，也没有守卫可取。两行同在一个 `async move` 块，源码顺序即执行顺序。

`force_local_with` 同样上 `rg` 门禁：**恰好一处调用点**。

### 3.7 修 Service→Normal 缺口

今天 `request.rs:82-85` 提前返回导致 `classify(true, ..)` 硬编码，**用户关闭服务模式后 reconcile 什么都不做**。改法：删掉提前返回，把真值送进 `classify`。`classify` 本身**不改**（`core/clash/core.rs:30-36` 已正确）。

### 3.8 步骤顺序：先建后删，**不是双轨并行**

> **两个生产者同时写同一状态而无定序，比一个更糟。** 单生产者的错误是确定性的；双生产者的错误是竞态的。

- **S-a**：建探针 + 修 3.7 缺口 + 接上九处调用点，**同一步停掉轮询的 reconcile 派发**；
- **S-b**：删轮询线程与三个 statics、`RunType::default()`（D2）、`core/service/mod.rs::init_service`。

5c 携带的 `KILL_FLAG` weak-CAS 缺陷（`control.rs:274`）**随轮询线程删除而消失，不单独修**。

---

## 4. C3 设计 —— DNS 端口、状态机与生命周期

### 4.1 端口 —— **读永远本地，写按模式分叉**

> **BLOCKING（leader 从 Suggestions 升级）：`nyanpasu_ipc` 里根本没有 DNS 读端点**（F56）。v3 给 `ServiceMacosDns` 写了 `read_default` / `read`，**它们没有任何实现机制**。而回读校验是整个设计的骨干——Service 侧读不了，`unverified` 就永远解不开，守卫没有出口。

**三条路线的取舍：**

| 路线                                                          | 评价                                                                                                      |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| ①承认 Service 写不可验证，显式定义守卫与四态在该模式下的含义  | 诚实，但**放弃**了 Service 模式的全部验证能力，而 Service 是用户实际部署的那条路                          |
| ②上游加读端点                                                 | **否**。R0 仍未合并，这会是叠在它之上的**第二个上游 PR**；leader 为设备字段否掉过同样的成本，此处成本不变 |
| **③读一律走本地 `networksetup -getdnsservers`，两个模式共用** | **采纳**。无需线上变更；且见下方三条附带收益                                                              |

**为什么③在 Service 模式下也成立（有依据，不是「大概行」）：**

- **同一台机器、同一个函数**。`nyanpasu-service` 是本机特权 helper，服务端解析默认设备用的正是 `get_default_network_hardware_port()`（`routing/network.rs:26`）。我们在客户端调**同一个函数**，得到**同一个答案**，除非默认设备在两次调用之间变了——**那正是 R1 那个既有窗口，不是新增的**。
- **读不依赖 daemon**。daemon 挂了、被 stop 了、被 uninstall 了，读**照样能用**。这一条直接救了 §4.4 的死锁序列。
- **权限未确立（F63），但设计对它不敏感**：读被拒绝 → 非零退出 → 四态① `Err` → 与任何其它读失败同路（守卫保留、降级）。

```rust
// core/actor/dns.rs —— 整个文件 #[cfg(target_os = "macos")]

/// 一次覆写所针对的目标：**本地解析到的硬件端口名**。
/// 读与 Local 写都按它定向；Service 写**无法**定向（F51），只能在写之前
/// 校验「当前默认设备仍是它」——见 ServiceMacosDns::write。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DnsTarget(pub String);

#[derive(Debug, thiserror::Error)]
pub(crate) enum DnsPortError {
    /// Service 写之前发现默认设备已经不是记录的那个。**拒绝写，不猜。**
    #[error("default device drifted: recorded {recorded}, observed {observed}")]
    TargetDrifted { recorded: String, observed: String },
    #[error(transparent)]
    Io(#[from] anyhow::Error),
}

/// 本地读实现，**两个后端共用**（F56：IPC 没有读端点）。四态见 §4.8。
pub(crate) struct LocalDnsReader;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait MacosDnsPort: Send + Sync + 'static {
    /// 解析当前默认硬件端口并读它的 DNS。**总是本地执行。**
    async fn read_default(&self) -> Result<(DnsTarget, Option<Vec<IpAddr>>), DnsPortError>;
    /// 读指定目标。**总是本地执行**，因此 daemon 不在也能用。
    async fn read(&self, target: &DnsTarget) -> Result<Option<Vec<IpAddr>>, DnsPortError>;
    /// 写指定目标。Local 直接定向；Service 先校验默认设备未漂移，再发 IPC。
    async fn write(&self, target: &DnsTarget, dns: Option<Vec<IpAddr>>) -> Result<(), DnsPortError>;
}

pub(crate) struct LocalMacosDns   { reader: LocalDnsReader }
pub(crate) struct ServiceMacosDns { reader: LocalDnsReader, client: .. }
```

|                         | `LocalMacosDns`                                     | `ServiceMacosDns`                                                            |
| ----------------------- | --------------------------------------------------- | ---------------------------------------------------------------------------- |
| `read_default` / `read` | 本地 `networksetup`                                 | **同左**（共用 `LocalDnsReader`）                                            |
| `write(target, dns)`    | `networksetup -setdnsservers <target> ..`，直接定向 | 先本地解析当前默认设备；**≠ `target` 则 `Err(TargetDrifted)`**；相等才发 IPC |

> **③相对 v3 的实质改进：R1 从「构造上不可检测」变成「可检测、仍不可阻止」。**
>
> v3 用 `DnsTarget::ServerResolvedDefault` 表达「Service 模式连设备名都拿不到」，代价是**默认设备漂移时我们毫不知情，会把旧设备的 DNS 写到新设备上**——那不只是没恢复成功，是**主动破坏另一个接口的配置**。
>
> ③之后我们**总能拿到设备名**，于是可以在写之前比对；检测到漂移就**拒绝写**并报降级。**残余是 TOCTOU**：我们的校验与 daemon 的解析之间仍有窗口，无法消除（除非走路线②）。**但窗口从「无限期静默写错接口」缩到「一次校验与一次 IPC 之间」。**

**fake 必须按序记录** enable / restore / 与后端动作的相对次序，供测试断言**顺序**而非终态。

**不违反 D3 的「非 macOS 不加空抽象」**：整个文件在 `#[cfg(target_os = "macos")]` 下。

> ### 与 `design.md:337` 的偏离 —— **v3 的第二条理由已被推翻，重新推导**
>
> `design.md` §9 写「Service 模式需要提权时**由 `CoreBackend::Service` 调 IPC set_dns**」。本设计用**独立的兄弟端口**。
>
> **先撤回 v3 的错误论证。** v3 说「`replace_backend` 会 `take()` 掉旧后端，恢复通道住在后端里就会消失」。**审查者指出这站不住**：恢复是**在调用 `replace_backend` 之前**完成的，那一刻旧后端还活着。leader 已接受该反驳，并指出自己**只核了锚点没核论证**。该理由作废。
>
> **重新推导后，两条理由成立：**
>
> 1. **（机制，新）恢复所需的适配器不总是当前后端那个，有时根本没有后端。** §4.4 的裁定是 teardown 失败后 Stop 仍继续；随后 daemon 已下线，恢复只能走 **Local** 写——而此时 `state.mode` 可能仍是 Service、`state.backend` 在 `Shutdown` 后更是 `None`（`mod.rs:606`，此后 `backend()` 返回 `ShuttingDown`，`:175-183`）。**若 DNS 方法长在 `CoreBackend` 上，「用 Local 适配器恢复一个 Service 时期建立的覆写」在类型上就无从表达**，而那恰是 §4.4 死锁序列唯一的出路。兄弟端口独立于后端的替换与消失。
> 2. **（洁净性）`CoreBackend` 是全平台的核进程生命周期枚举**，挂 macOS-only 的 DNS 方法，与 §4.2 拒绝往 `CoreRequest` 塞 TUN 字段是同一个理由。
>
> **理由 1 是机制性的、理由 2 是洁净性的，如实分开标注。** 若将来 §4.4 改成「teardown 失败即中止 Stop」，理由 1 会随之失效，**届时应当如实说只剩洁净性**。
>
> **命名对应**：design 里的「小型 `MacosDnsGuard`」在本设计中是 `CoreActorState.dns: Option<DnsOverride>`（§4.2）。

### 4.2 状态与消息

```rust
// CoreActorState 新增（core/actor/mod.rs:52-69）
#[cfg(target_os = "macos")] pub(crate) dns: Option<DnsOverride>,
#[cfg(target_os = "macos")] pub(crate) dns_ports: DnsPorts,

pub(crate) struct DnsPorts { local: Arc<dyn MacosDnsPort>, service: Arc<dyn MacosDnsPort> }

pub(crate) struct DnsOverride {
    /// 快照时本地解析到的设备。
    target: DnsTarget,
    /// 覆写**之前**的原始 DNS。`None` 是合法值（原本就没配）。
    previous: Option<Vec<IpAddr>>,
    /// 建立覆写时的后端身份。**仅用于诊断**——恢复选哪个适配器见 §4.5，
    /// 按「谁还够得着」而不是按这个字段。
    origin: RunType,
    /// 写已发出但尚未被回读证实。守卫**保持 active**。
    unverified: bool,
}

// CoreActorMessage 新增
SetTunDns {
    operation: OperationId,
    /// Some(ip) = 开 TUN 并把 DNS 指到该地址；None = 关 TUN，恢复原值。
    /// TUN 设备 IP 由 client 侧从 clash config 算好传入——**actor 不读配置全局**。
    desired: Option<IpAddr>,
    reply: RpcReplyPort<Result<DnsOutcome, CoreActorError>>,
}

pub(crate) enum DnsOutcome { Applied, AppliedUnverified, NoChange, Restored, RestoredUnverified }

// CoreActorError 新增（core/actor/types.rs:68-79）
CoreNotRunning,      // 核已停，拒绝建立覆写
DnsRestoreFailed,    // 所有可用适配器都恢复不了
```

**注入路径**：`ClientSetupArgs`（新 `#[cfg(target_os="macos")] dns_ports`）→ `CoreClientArgs`（`client/core.rs:39-43`）→ `CoreClient::spawn`（`:105-119`）→ `CoreActorArgs`（`core/actor/mod.rs:37-50`）→ `CoreActorState`。**与 `requests`/`degradation` 同一条既有路径。**

**不扩 `CoreRequest`**：它是 run/check/apply 共用的**全平台**进程描述，塞 macOS-only 的 TUN 字段会污染两条无关路径。

#### 施加的顺序 —— **先记账，再写**

> **裁定原则（必须写进代码注释）：错误通道报告的是调用的结果，永远不是副作用的缺席。**
>
> `write` 返回 `Err` 时，DNS **可能已经改了**：Service 侧 daemon 可能改完才丢响应；Local 侧命令可能改完才非零退出。**v3 的「写失败就不建守卫」因此制造出守卫本来要防的那个状态：DNS 变了、没有记录、Stop 与 Shutdown 都无从恢复。**

```text
① read_default()  → (target, previous)          ← 拿不到就直接 Err，什么都没做，安全
② state.dns = Some(DnsOverride{ target, previous, origin: state.mode, unverified: true })
                                                 ← **在写之前记账**
③ write(&target, Some(tun_ip))                   ← 成败都不改变②已经记下的恢复意图
④ read(&target) 消歧：
     == desired   → unverified = false；返回 Applied（③若为 Err 则 AppliedUnverified + 降级）
     == previous  → 写确实没生效 → **移除守卫**（无可恢复）→ 返回 ③ 的 Err
     其它 / 读失败 → **保留守卫、unverified 维持 true** → AppliedUnverified + 降级
```

**②在③之前是本节的全部要点。** 第④步的回读是**唯一**能把「写没生效」与「写生效了但报错」区分开的机制；在能区分之前，**必须假设已经改了**。

**七个状态机问题的落点：**

| #   | 问题                                         | 机制                                                                                                                                            |
| --- | -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 写成功但回读失败                             | 保留 `Some(.. unverified: true)`，`AppliedUnverified` + `macos_dns_readback_failed`                                                             |
| 2   | 守卫只在**验证过的恢复**之后才清             | `state.dns = None` 只出现在一条分支：`read(target)` 返回 `Ok(v)` 且 `v` 与 `previous` **集合相等**                                              |
| 3   | 重复 `SetTunDns(Some(..))` 保住**最初**原值  | 处理器开头 `if state.dns.is_some()` → **不重新快照**，只写新地址                                                                                |
| 4   | 设备变了先恢复旧设备再取新快照               | 每次 `SetTunDns(Some)` 先 `read_default()`，与 `override.target` 比；不等 → 先 `write(&override.target, previous)` 恢复旧设备，再对新设备取快照 |
| 5   | 恢复失败发生在 `SetBackend` 之前             | 见 §4.5（**已改为逐适配器穷尽后才中止**）                                                                                                       |
| 6   | `SetBackend` 成功后仍需 TUN 则重新施加       | 见 §4.5，**时点是 `Run` 之后**（F43），由**源码顺序**保证                                                                                       |
| 7   | 「原值 `None` 的活跃覆写」与「无覆写」可区分 | 外层 `Option<DnsOverride>` 表达有无覆写，内层 `previous: Option<..>` 表达原值——**两层 Option，结构上不可混淆**                                  |

### 4.3 `SetTunDns` 的准入 —— 两条规则，两个构造

> **`OperationGate` 不是 actor 级互斥**（`gate.rs:20-30`、`:32-45`）：它只做 FIFO 发号 + `is_active` 校验。**外部持 guard 期间，actor 照常处理别的消息。**

**`SetTunDns` 携带 `OperationId`，由 `validate_operation`（`mod.rs:185-190`）校验，与 `Stop`/`Run`/`SetBackend` 同形。**

**为什么是「携带 id」而不是「自己取门」**：`OperationGate::acquire` 在门被占时把请求塞进 `waiters`（`gate.rs:25-28`），**只有另一条 `ReleaseOperation` 消息被处理时才发放**（`mod.rs:436`、`gate.rs:73-83`）。ractor 逐条串行处理消息，所以在 `handle()` 里 `await` 一个发放**永远等不到**——自取门是**构造性死锁**。

| #   | 场景                                                           | 规则                       | **强制构造**                                                                                                                                                                                                            |
| --- | -------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A   | `Shutdown` 已开始后到达的 `SetTunDns`                          | `Err(ShuttingDown)`        | `Shutdown` 处理器 `mod.rs:604` 调 `state.operation.shutdown()`，`gate.rs:55-60` 把 `active` 置 `None` → 此后任何 id 都 `StaleOperation`；`:606` `state.backend.take()` 使 `backend()` 返回 `ShuttingDown`（`:175-183`） |
| B1  | `SetTunDns` 先拿到许可，`Stop` 排在后面                        | `Stop` 的恢复晚于 DNS 设置 | `OperationGate` FIFO（`gate.rs:73-83`，已有 5 条单测）                                                                                                                                                                  |
| B2  | `Stop` 先拿到许可，晚到的 `SetTunDns(Some)` 持**自己的新守卫** | **`Err(CoreNotRunning)`**  | **准入检查 `state.running.is_some()`**（F44）。**仅对 `desired = Some(..)` 生效**——`desired = None`（拆除）在核已停时仍必须允许，那正是恢复路径                                                                         |

> **B2 是 FIFO 挡不住的那条**：晚到者的守卫是新的、合法的，`validate_operation` 会放行；只有 `state.running` 这条准入能拦住它**重新建立一个背后没有核的覆写**。
>
> **注意 F57**：`state.running` 的清除点**不止**一处显式赋值——`commit()` 观察到 `Stopped` 时也会清（`:224-227`）。**这加强而非削弱 B2**（多条路径都会让准入生效），但它使「删掉某一行就会红」的判据失效，见 §7 T-DNS-13/15 的第三列。

### 4.4 顺序：控制动作前先拆 DNS —— 一条规则，六个入口

**规则：六个服务控制入口，在调用外部控制动作之前，都先在同一守卫内 `await` 拆除 DNS 覆写。**

**为什么是六个而不是四个**：要把 install 排除在外，就得证明 `nyanpasu-service install` 在已有 daemon 在跑时不会把它换掉/重启——**我核不了这一点，而它是「不会去做某事」型断言**。铺到六个的代价是无活跃覆写时多一次 `state.dns.is_none()` 的 no-op 判断。**用一次 no-op 换掉一条无法验证的前提，划算。因此本规则没有需要点名的例外。**

**拆除失败时的分岔：**

| 场景          | 处置                                    | 理由                                                                                                      |
| ------------- | --------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| **uninstall** | **中止卸载**，返回 `Err` + 用户可见错误 | 卸载**不可逆**；拆 DNS 失败说明我们**当下连自己的写都验证不了**，此时执行不可逆操作是把已知的不确定性固化 |
| **其余五个**  | **继续，产出 degradation**              | 服务可再启动、通道会回来，泄漏可恢复；为拆 DNS 失败就让用户停不掉服务代价不成比例                         |

**判别原则：失败会让泄漏变成永久的 → 中止；泄漏仍可恢复 → 继续并降级。**

**中止 uninstall 的用户可见错误必须说清三件事**：做了什么（没有卸载）、为什么（DNS 覆写未能拆除，继续卸载会永久残留）、怎么办（重试；或先手动关闭 TUN 再卸载）。**用户可见的失败，措辞本身就是功能的一部分。**

#### 「拆除失败后继续 Stop」与「SetBackend 中止」的矛盾

审查者给的序列是真的：Service 模式拆除失败 → Stop 成功、daemon 与 IPC 通道消失 → probe 判 `Disconnected` → reconcile 目标 Local → `SetBackend` 重试恢复走 `ServiceMacosDns` → 通道没了必失败 → 无条件中止 → **卡在陈旧 Service 状态、DNS 被覆写、背后没有核**。

**根因不是两条规则本身矛盾，是 v3 把「恢复用哪个适配器」冻结在了快照时刻。** §4.5 改掉这一条之后矛盾消失：daemon 下线后恢复改走 **Local 写**（目标设备名我们有，因为读一直是本地的），不再依赖那条已经消失的通道。

### 4.5 `SetBackend`：恢复用**够得着的**适配器，成功后在 `Run` 之后重新施加

> **v3 的规则是「用 `DnsOverride` 记录的后端恢复」。那条规则制造了 §4.4 的死锁。改掉。**

**新规则：恢复时按「哪个适配器还够得着这个目标」选，而不是按覆写是谁建的。**

因为读永远本地（§4.1），`DnsTarget` **总是一个具名设备**，于是：

```text
restore(target, previous):
  候选顺序 = [ 与 state.mode 匹配的适配器（若其通道可用）, LocalMacosDns ]   ← 去重
  依次尝试 write(&target, previous)：
    任一成功 → 回读校验 → 通过则清守卫；不通过则保留 unverified
    全部失败 → Err(DnsRestoreFailed)
```

- **正常 Service 路径**：`state.mode == Service` 且通道在 → 走 IPC（有权限），与今天一致。
- **§4.4 的死锁路径**：Stop 之后 mode 已 reconcile 成 Local → 首选就是 Local → **能写到设备 D**，矛盾消解。
- **Local 写在非 admin 账户会失败**：两个候选都失败 → `DnsRestoreFailed` → 中止 `SetBackend`。**这才是真正「所有出路都堵死」的情形，中止在此恰当。**

`DnsOverride.origin` 因此**降级为纯诊断字段**（降级消息里说明覆写是谁建的），不再参与适配器选择。

**成功切换后的重新施加，时点由 F43 钉死：**

```text
guard
 ├─ SetTunDns(None)          ← §4.5 的恢复；穷尽候选后仍失败才中止
 ├─ SetBackend(mode)         ← replace_backend：running := None（:268），mode := new（:282）
 ├─ Run(request)             ← running := Some（:514）        ★ 核在这里才重新起来
 └─ SetTunDns(Some(tun_ip))  ← 若 TUN 仍需开启：用**新**适配器重新施加
```

> **保证这个时点的构造是「同一守卫内的源码顺序」，不是 `CoreNotRunning`。** 审查者说得对：`CoreNotRunning` 只能**拒绝过早**的重新施加，**不能保证后面真有一次重新施加**。真正的保证是 `CoreModeReconciler` 在持守卫期间**按上述四步的源码顺序依次 `await`**——第四步存在与否由源码决定，由 T-DNS-15 钉住。`CoreNotRunning` 是**附加的**安全网，防止别人在中间插一条。

`desired`（TUN 是否开、TUN 设备 IP）由 `CoreModeReconciler` 算好传入：新增 `clash_config: ClashConfigClient` 字段（`NyanpasuClientInner.clash_config`，`client/mod.rs:247`），TUN 开关读 `application`。**actor 侧一行配置全局都不读。**

### 4.6 Local 写：在我们自己的 crate 里直调 `networksetup`

**不复用上游 `nyanpasu_utils::network::macos::{set_dns, get_dns}`。** 三条已核实的理由：

1. **设备名被文本拼进 bash 脚本**（F50）——设备来源一旦不是硬编码字面量就是注入面；
2. **macOS 硬件端口名常含空格**（`USB 10/100/1000 LAN`），脚本里 `$1` 未加引号会被词法分割——**这条今天就是坏的**，只是 `Wi-Fi` 这种单词端口把它盖住了；
3. **读路径不看退出码**（F41），非零退出 + 空 stdout 塌缩成 `Ok(None)`。

```rust
Command::new("networksetup")
    .env("LC_ALL", "C")          // §7.4：文案匹配要求可复现 locale
    .arg("-setdnsservers")
    .arg(&target.0)              // 直接 argv，不经 shell
    .args(dns_args)              // 多服务器 = 多 argv；无 DNS 时单个 "Empty"
    .kill_on_drop(true)
    .output().await?;
// 然后**检查 output.status**
```

> `$2` 是**本该**被词法分割的（多个 DNS 服务器要变成多个参数），所以「两边都加引号」是错的修法，**直接 argv 才是对的**。

**这条比复用上游更短**：省掉临时目录、临时文件、`include_str!` 替换、`osascript` 包一层；同时消灭注入面、修好空格、**白拿到四态读所需的退出码**。**无需上游改动。**

**权限（F53/F54/F63）：**

- 写至少需要 admin 组身份；系统开启那个安全选项则需 root。
- **`osascript` 那一跳不提权**（F46/F54），去掉它**不改变**权限语义。
- **读是否需要权限未确立（F63）**，man page 那句只谈 change。**不假设。**
- **设计对两个答案都不敏感**：任何写或读被拒绝 → 非零退出 → `Err` → 走既定失败分岔。
- **明确不做权限预检**：预检是第二个真相来源，可以和真实调用结果不一致。**唯一判据是那次真实调用的退出码。**

### 4.7 写回读回校验

**(a) 必须是语义比较**：解析成 `IpAddr` 后比较**集合**（忽略顺序与重复），解析失败即视为不一致。**做不到语义比较就不要做这个校验**——文本比较会产生**假失败**，那比不校验更糟：会把成功的操作报成失败，然后有人为了让它绿而删掉校验。

**(b) 失败进 degradation，不静默**；测试**必须走真实适配器的比较逻辑**。

**(c) 回读失败 ≠ 写失败**：见 §4.2 的四步，守卫保留、`unverified` 维持。

**TOCTOU 不在范围**：本校验回答的是「**我们的写有没有生效**」，并发的外部变更不属于它。

### 4.8 读实现必须分四态

> **「没有配置 DNS」必须被正向识别，不能从「输出空 / 解析不了」推断。** **正是「把不可解析当成 `None`」这一步制造了原来的 bug**（F41）。

| #   | 条件                          | 结果                       |
| --- | ----------------------------- | -------------------------- |
| 1   | 退出码非零                    | **`Err`**（读失败）        |
| 2   | 输出匹配「无 DNS 服务器」那句 | `Ok(None)`                 |
| 3   | 输出解析出 IP 列表            | `Ok(Some(..))`             |
| 4   | **以上都不是**                | **`Err`**，**不是 `None`** |

**第 4 条是关键：不认识的输出是错误，不是「没有」。**

#### 分支②的匹配串从哪来（**F55：规划期未能确立**）

审查者指出：删掉 T-DNS-09 只是去掉一条空转测试，**没有回答生产代码里那个匹配串从哪来**。答案分两种情形，**都要写进 Exit**：

| 情形                   | 分支②怎么实现                                                                                                                                                                                                       | 后果                                                                                                                                                                                         |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **能拿到真机**（首选） | 用**新调用形态**（直调 `networksetup`、`LC_ALL=C`）捕获真实输出，存为 fixture 并记 provenance（macOS 版本、locale、完整命令行）；生产匹配器**独立于** fixture 文件（不 `include_str!` 同一份），T-DNS-09 读 fixture | 四态完整，验证可用                                                                                                                                                                           |
| **拿不到真机**         | **分支②不实现**——匹配失败即落分支④ `Err`。**不许拍一个字面串**：生产与测试共用臆造常量 = 自证                                                                                                                       | **「原值本就是 `None`」的恢复将恒为 `unverified`**：读不出「无 DNS」，就无法证实恢复到位。守卫保留、每次报降级。**安全但吵**，且 §11 必须点名「四态②未实现、restore-to-None 无法验证（R6）」 |

> **不得用第三条路**（拍一个看起来对的串）。那会让 T-DNS-09 与生产共享同一个臆造事实，**测试通过恰好证明不了任何事**，而失败模式是静默的：真机上匹配不中 → 落④ → 读失败 → 与「没实现」同样吵，但我们**以为**自己实现了。

### 4.9 `Shutdown` 的静默期协议

> **不把 `Shutdown` 变成守卫消息。** 那是诱人但错误的修法：**关停必须能在一个操作卡住时仍然生效**，否则一次挂死的控制操作会让 app 关不掉。

**问题的确切形状**（已核实）：facade 的控制序列在**外部**持有许可，而 `CoreClient::shutdown()` 发的是**无守卫**的 `Shutdown`（`client/core.rs:277-283`），actor 立即 `state.operation.shutdown()` 清掉活跃操作（`mod.rs:604`）。于是：

1. `start_service` 取得守卫、拆完 DNS；
2. `Shutdown` 插进来，清门、恢复 DNS、关后端、回复、停 actor；
3. 原持有者继续执行**外部** `start`/`install`/`restart`/`update` 命令；
4. 它随后的 `reconcile_with` 因 actor 已不在而失败。

**OS 服务因此可能在「已经 await 过的关停」之后被启动或替换，而没有任何 actor 还能收敛它。** `operation.shutdown()` 只能作废**后续 actor 消息**，**管不了一个已经在跑的 facade future 及其下一条外部命令**。

**协议放在 facade**（准入是 facade 概念；F58 显示 `NyanpasuClient::shutdown()` 已是有序多步，F60 给了现成范式）：

```rust
// NyanpasuClient 新增字段
struct ControlAdmission {
    closed: AtomicBool,
    /// 1 个许可，被整条控制序列持有（含外部命令）。与 actor 的 OperationGate 是
    /// 两个不同作用域：gate 管 actor 消息，这个管 facade future。
    inflight: Arc<tokio::sync::Semaphore>,
}
```

```text
NyanpasuClient::shutdown()：
 ① rebuild.shutdown().await                      ← 既有第一步，不动
 ② admission.close()                             ← 关准入：此后六个控制入口立即 Err(ShuttingDown)
 ③ 有界等待在飞控制序列退出：
      tokio::time::timeout(QUIESCE_BUDGET, admission.inflight.acquire()).await
      超时 → warn + degradation `shutdown_quiesce_timeout`，继续 ④
 ④ 恢复 DNS（await）                              ← 此时无控制序列在飞，且后端仍在
 ⑤ core_client.shutdown().await                  ← 既有最后一步
```

**为什么需要与 `OperationGate` 并存的第二个机制**（否则就是重复造轮子）：`OperationGate` 的许可只约束 **actor 消息**；`Shutdown` 本身是无守卫消息且会**主动清空**活跃许可，所以 gate 在关停面前不提供任何排他性。`ControlAdmission` 约束的是 **facade future 的存续**，包括那条 actor 完全看不见的外部 OS 命令。**两者作用域不同，不是冗余。**

**残留（如实记账，不谎称关闭）：**

| 残留   | 形状                                                           | 为什么不能消除                                                                                            |
| ------ | -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| **R4** | ③ 超时后，被放弃的控制序列**仍可能**在关停之后完成它的外部命令 | 外部命令是 `runas`/`sudo` 起的**特权子进程**，在 `spawn_blocking` 里跑，**无法取消**。只能有界等待 + 记账 |
| **R5** | ② 与「紧贴命令前的 `check_open()`」之间是 TOCTOU               | 检查与使用之间永远有窗口。**从「无限期」缩到「一次检查到一次 spawn」**，不是消除                          |

---

## 5. 五条已知问题的最终去向

| #   | 问题                                   | 去向                                                                                      |
| --- | -------------------------------------- | ----------------------------------------------------------------------------------------- |
| #2  | C2 不可线性化                          | §3.6（三个签名、无 `IpcState` 参数、`rg` 门禁）+ §3.4（有界等待）+ **§4.9（关停静默期）** |
| #3  | `health_check` 的警告职责没人接手      | §3.3（**合取式复现**，覆盖三种 compat + 探针失败 + 就绪轮询期观察）                       |
| #4  | 拆 DNS 的守卫跨度与 `reconcile()` 死锁 | §3.6（避免递归取守卫）+ §4.3（守卫挡不住并发消息，靠准入 A/B2）                           |
| #5  | 读路径不检查退出码                     | §4.6（自己实现）+ §4.8（四态 + 分支②来源）                                                |
| #6  | 适配器接线不全                         | §4.1 / §4.2 全部写完                                                                      |

**#5 的两个被否方案**：

| 方案                               | 为什么否                                                                                                                        |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| (b) 只在 `expected != None` 时校验 | **`None` 正是关 TUN 的主路径，也正是泄漏场景本身。** 一个「在最需要它的地方主动关闭」的校验**比没有校验更糟**——它给出覆盖的假象 |
| (c) 哨兵区分                       | 多一次写入、多一个中间态，且**哨兵自身的写入同样不可验证**                                                                      |

---

## 6. 定序保证表

| 断言                                                                                             | **靠什么构造保证**                                                                                                             | 锚点                                             | 测试                        |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------ | --------------------------- |
| 两次控制动作的模式结论不交错                                                                     | `OperationGate` FIFO + 三步同守卫                                                                                              | `gate.rs:73-83`                                  | T-MODE-03                   |
| 控制动作 → probe → reconcile 三步不可拆                                                          | `reconcile_with(&guard)` **无 `IpcState` 参数**；`rg` 门禁钉死 `.probe()` 三处                                                 | §3.6                                             | T-MODE-03                   |
| bootstrap 的守卫外探针安全                                                                       | 执行于 `CoreClient::new` **之前**，actor 尚不存在                                                                              | `client/mod.rs:303` vs `:312`                    | T-PROBE-02                  |
| **探针必然在有限时间内返回**                                                                     | **`OsServiceProbe::probe` 内部的 `timeout` + `kill_on_drop`**，不是「每个调用方记得包一层」                                    | §3.1                                             | **T-PROBE-06**              |
| **控制失败时仍然 reconcile，且控制错误优先返回**                                                 | §3.5 处置表的源码顺序（reconcile 在前、`control?` 在后），与基线 F61 同形                                                      | `client/mod.rs:512-538`                          | **T-CTL-01…04**             |
| 恢复发生在后端动作与 reply 之前                                                                  | 处理器内的 `await` 点（源码顺序）                                                                                              | §2.2                                             | T-DNS-02/03                 |
| 拆 DNS 发生在**六个**控制动作之前                                                                | 同一守卫内的调用点顺序                                                                                                         | §4.4                                             | T-DNS-05/06/17/18/**26/27** |
| **`Shutdown` 不会越过在飞的控制序列**                                                            | **`ControlAdmission`：关准入 → 有界 drain → 才恢复 DNS 并停后端**；控制入口在外部命令前再查一次                                | §4.9                                             | **T-SD-01/02**              |
| `Shutdown` 后不再有 `SetTunDns` 生效                                                             | `operation.shutdown()` 清 `active` → 恒 `StaleOperation`；`backend.take()` → `ShuttingDown`                                    | `mod.rs:604,606`；`gate.rs:55-60`                | T-DNS-07                    |
| `Stop` 先拿到许可时晚到的 `SetTunDns(Some)` 失败                                                 | 准入检查 `state.running.is_some()`（FIFO 挡不住）                                                                              | `mod.rs:532`；**另见 F57 第二清除点 `:224-227`** | T-DNS-13                    |
| `SetTunDns` 先拿到许可时 `Stop` 在其后恢复                                                       | `OperationGate` FIFO                                                                                                           | `gate.rs:73-83`                                  | T-DNS-12                    |
| **写之前先记账**（`Err` 不代表没生效）                                                           | 处理器内 `state.dns = Some(..)` **早于** `write()` 的源码顺序                                                                  | §4.2 四步                                        | **T-DNS-19/28**             |
| 写 → 回读消歧                                                                                    | `write()` 之后紧跟 `read()` 的源码顺序                                                                                         | §4.2 ④                                           | T-DNS-04/19                 |
| **守卫只在验证过的恢复之后才清**                                                                 | `state.dns = None` 只出现在「回读集合相等」那一条分支上                                                                        | §4.2 问题 2                                      | **T-DNS-20**                |
| **设备变更时先恢复旧设备再取新快照**                                                             | `SetTunDns(Some)` 开头 `read_default()` 与 `override.target` 比对的分支                                                        | §4.2 问题 4                                      | **T-DNS-23**                |
| **恢复穷尽候选适配器后才中止 `SetBackend`**                                                      | §4.5 候选序列的循环 + 全失败才 `Err(DnsRestoreFailed)`                                                                         | §4.5                                             | **T-DNS-24/29**             |
| 切换成功后若仍需 TUN，在 `Run` 之后重新施加                                                      | **`CoreModeReconciler` 持守卫期间四步的源码顺序**（`CoreNotRunning` 只是防插队的安全网，**不构成「后面一定还有一次」的保证**） | §4.5                                             | **T-DNS-15**                |
| update 之后：**要么观察到兼容 Service，要么以 Local 收敛并降级；若降级动作本身失败则返回 `Err`** | 有界等待 + `force_local_with`；**该调用也可能失败**，落 §3.5 第 5 行                                                           | §3.4 / §3.5                                      | T-MODE-04/05、**T-CTL-03**  |
| 任何时刻只有一个模式生产者                                                                       | S-a 同步停掉轮询派发                                                                                                           | §3.8                                             | `rg` 判据（§11）            |

---

## 7. 测试矩阵

> **第三列是断言**：删掉那行生产代码，这条测试真的会红吗？**填不出第三列的测试不进矩阵。**
>
> **注意 F57 类陷阱**：某些状态有**多个**写入点，删掉其中一个另一个仍会生效，测试照绿。第三列必须点到**唯一**的那一行，或改用行为级构造。

### 7.1 C2

| ID             | 断言                                                                                                         | **删掉哪行会让它红**                                                                  |
| -------------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| T-PROBE-01     | 兼容门 fail-closed：daemon 在跑但不放行 → 探针 `Disconnected`                                                | 探针里调 `target_ipc_state()` 那行                                                    |
| T-PROBE-02     | bootstrap 用探针真值而非 `Disconnected` 默认（修 F35）                                                       | `client/mod.rs:303` 的 `probe().await`                                                |
| T-PROBE-03     | **Running + `Unparsable` 也告警**（不只 `Incompatible`）                                                     | 警告条件里的 `!compat.allows_service_backend()`（改成 `matches!(Incompatible)` 即红） |
| **T-PROBE-05** | **Running + `Unknown`（有 status 无 server）告警；Stopped + `Unknown` 不告警**                               | `ProbeOutcome.daemon_status` 字段本身（去掉它两种 `Unknown` 就分不开，必红）          |
| T-PROBE-04     | 探针失败发出 `service_probe_failed` 降级                                                                     | 处理 `error` 的 `degradation.publish(..)` 行                                          |
| **T-PROBE-06** | **挂死的 `status` 在 `PER_PROBE_BUDGET` 内返回**（不靠调用方包 timeout）                                     | `OsServiceProbe::probe` 里的 `tokio::time::timeout(..)`                               |
| T-MODE-01      | 关闭 `enable_service_mode` → 得 `Normal` 并 `set_backend`                                                    | `request.rs:82-85` 删提前返回后送真值那行                                             |
| T-MODE-02      | 六个控制动作后**各自**触发 probe+reconcile——逐条独立断言。**断言「至少一次」而非「恰好一次」**               | 各自 facade 方法里的 `reconcile_with(&guard)`                                         |
| T-MODE-03      | start→stop 序列下终态 `Normal`，晚到 probe 不翻转                                                            | `reconcile_with` 的 `guard` 参数                                                      |
| T-MODE-04      | 有界等待成功路径：脚本探针第 N 次兼容 → `Service`，无降级                                                    | `await_service_ready` 循环体                                                          |
| T-MODE-05      | **永不返回的探针**：在 `READY_BUDGET` 小倍数内返回，结果 Local + `service_update_not_ready`，控制动作仍 `Ok` | `OsServiceProbe::probe` 的 timeout（与 T-PROBE-06 同源）                              |

> **T-MODE-02 的「恰好一次」是 v3 的错**（审查者纠正）：正确的 update 路径会在 `await_service_ready` 内探针，**随后 `reconcile_with` 里还要再探一次**——即便立即就绪也至少两次。断言改为「至少一次，且 reconcile 用的是守卫内那次的结果」。

### 7.2 控制失败处置（§3.5 新增）

| ID           | 断言                                                                          | **删掉哪行会让它红**                                            |
| ------------ | ----------------------------------------------------------------------------- | --------------------------------------------------------------- |
| **T-CTL-01** | 控制 `Err` + reconcile `Ok` → 返回**控制的** `Err`，且 reconcile **确实跑过** | `reconcile_with` 调用行位于 `control?` **之前**（改成早退即红） |
| **T-CTL-02** | 控制 `Err` + reconcile `Err` → 返回**控制的** `Err`，reconcile 失败进降级     | 错误优先级那行（返回 reconcile 的 `Err` 即红）                  |
| **T-CTL-03** | 控制 `Ok` + 就绪超时 + `force_local_with` **失败** → 返回 `Err` + 两条降级    | §3.5 第 5 行的失败分支                                          |
| **T-CTL-04** | 控制 `Err`（update）→ **跳过**就绪等待（断言 `await_service_ready` 零调用）   | 就绪等待外层的 `result.is_ok()` 条件                            |

### 7.3 C3 —— 生命周期与并发

| ID           | 断言                                                                                                                    | **删掉哪行会让它红**                                                                                                                                         |
| ------------ | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| T-DNS-01     | `SetTunDns{Some}` → 适配器 `write` 被调，guard 记为 active                                                              | 处理器里 `port.write()`                                                                                                                                      |
| T-DNS-02     | **顺序**：`Stop` 时恢复在 `backend.stop()` **之前**                                                                     | `Stop` 臂 `restore().await` 早于 `backend.stop()`（对调即红）                                                                                                |
| T-DNS-03     | **顺序**：`Shutdown` 时恢复在后端动作与 **reply** 之前                                                                  | `Shutdown` 臂的 `restore().await`                                                                                                                            |
| T-DNS-04     | 回读比对真的会发现不一致（走真实比较逻辑）                                                                              | 适配器里集合比较那行                                                                                                                                         |
| T-DNS-05     | **stop**：拆 DNS 在 `stop()` 之前                                                                                       | facade `stop_service` 的拆 DNS 行                                                                                                                            |
| T-DNS-06     | **uninstall**：同上顺序 + **失败时中止卸载**（断言 uninstall **未被调用**）                                             | `uninstall_service` 的拆 DNS 行与中止分支                                                                                                                    |
| T-DNS-17     | **restart**：拆 DNS 在 `restart()` 之前                                                                                 | facade `restart_service` 的拆 DNS 行                                                                                                                         |
| T-DNS-18     | **update**：拆 DNS 在 `update()` 之前                                                                                   | facade `update_service` 的拆 DNS 行                                                                                                                          |
| **T-DNS-26** | **install**：拆 DNS 在 `install()` 之前                                                                                 | facade `install_service` 的拆 DNS 行                                                                                                                         |
| **T-DNS-27** | **start**：拆 DNS 在 `start()` 之前                                                                                     | facade `start_service` 的拆 DNS 行                                                                                                                           |
| T-DNS-07     | **`Shutdown` 把等待中的取门请求全部以 `ShuttingDown` 排空**                                                             | **`gate.rs:57-59` 的 `waiters.drain(..)` 循环**（删掉即：等待者永远收不到回复，测试挂起/超时）                                                               |
| T-DNS-12     | `SetTunDns` 先取得许可 → `Stop` 在其后恢复                                                                              | `Stop` 臂的 `restore().await`                                                                                                                                |
| T-DNS-13     | `Stop` 先取得许可 → 后到的持新守卫的 `SetTunDns(Some)` → `Err(CoreNotRunning)`                                          | **`SetTunDns` 臂里的 `state.running.is_some()` 准入检查**（不能点 `Stop` 里的赋值，F57 有第二清除点）                                                        |
| **T-DNS-14** | `ServiceMacosDns::write` 在默认设备漂移时 → `Err(TargetDrifted)`，**且未发出 IPC**                                      | 写前的设备比对分支                                                                                                                                           |
| T-DNS-15     | `SetBackend` 成功且 TUN 仍开 → **`Run` 之后**用**新**适配器重新施加                                                     | **`CoreModeReconciler` 里第四步 `SetTunDns(Some(..))` 那一行**（删掉即无人重新施加；不再点 `replace_backend` 的 `running = None`，F57 表明那行删掉也不会红） |
| **T-DNS-16** | **恢复按「够得着」选适配器**：Service 期建立的覆写，在 mode 已收敛 Local、IPC 通道不可用时，**经 Local 适配器成功恢复** | §4.5 候选序列里的 Local 回退项（删掉即 `DnsRestoreFailed`）                                                                                                  |
| T-DNS-19     | **写返回 `Err` 但回读显示 desired 已生效** → 守卫**保留**、`unverified`、报降级                                         | **`state.dns = Some(..)` 早于 `write()` 的源码顺序**（把记账挪到写成功之后即红）                                                                             |
| **T-DNS-28** | **写返回 `Err` 且回读显示仍是 previous** → **移除**守卫并返回 `Err`（不留假守卫）                                       | 回读消歧的 `== previous` 分支                                                                                                                                |
| T-DNS-20     | 恢复的回读校验失败 → 守卫**不清**                                                                                       | `state.dns = None` 前的校验条件                                                                                                                              |
| T-DNS-21     | 重复 `SetTunDns(Some(a))` → `SetTunDns(Some(b))` → 恢复得到**最初**原值                                                 | 「已有覆写则不重新快照」那行                                                                                                                                 |
| T-DNS-22     | 原值为 `None` 的活跃覆写与「无覆写」可区分：前者 `Stop` 时**会**调一次 `write(.., None)`                                | 两层 `Option` 的外层判断行                                                                                                                                   |
| T-DNS-23     | 设备变更：`Device(a)` 活跃时 `read_default()` 返回 `b` → 先 `write(a, previous)` 再对 `b` 取快照                        | target 比较那行                                                                                                                                              |
| T-DNS-24     | **所有候选适配器都失败**时 `SetBackend` 中止：不调 `replace_backend`，返回 `Err(DnsRestoreFailed)`，`state.dns` 保留    | 中止分支的 `return`                                                                                                                                          |
| **T-DNS-29** | **死锁全序列**：Service 拆除失败 → Stop 继续 → reconcile Local → `SetBackend` 经 Local **成功恢复**                     | §4.5 候选序列（回到 v3 的「按 `origin` 选」即红）                                                                                                            |
| T-DNS-25     | `Drop` 时守卫仍 active → 记 `tracing::error!` 且**不发起任何恢复**（断言适配器零调用）                                  | `Drop` 里的 `tracing::error!` + 反向断言                                                                                                                     |
| **T-SD-01**  | `Shutdown` 落在拆 DNS 之后、外部命令之前 → **外部命令不被调用**                                                         | 外部命令前的 `admission.check_open()?`                                                                                                                       |
| **T-SD-02**  | 控制序列卡在外部命令里 → `shutdown` 在 `QUIESCE_BUDGET` 内返回 + `shutdown_quiesce_timeout` 降级                        | `tokio::time::timeout(QUIESCE_BUDGET, ..)`（改成裸 await 即挂死）                                                                                            |

> **T-DNS-05/06/17/18/26/27 六条不合并**——六个独立调用点，合并测则删掉其中一处另一处仍绿。

### 7.4 C3 —— 四态读

| ID       | 断言                                                                             | **删掉哪行会让它红**             |
| -------- | -------------------------------------------------------------------------------- | -------------------------------- |
| T-DNS-08 | 四态①：**退出码非零 → `Err`**。fixture 必须是「非零退出码 **+** 可解析 IP 输出」 | `if !output.status.success()`    |
| T-DNS-09 | 四态②：「无 DNS 服务器」→ `Ok(None)`——**仅在拿到真机 fixture 时存在**（§4.8）    | 匹配该文案那行                   |
| T-DNS-10 | 四态③：正常输出 → `Ok(Some(..))`                                                 | 解析 IP 列表那行                 |
| T-DNS-11 | 四态④：**不认识的输出 → `Err`，不是 `None`**                                     | 兜底分支（改成返回 `None` 即红） |

**T-DNS-08 的 fixture 选择理由**：若非零退出的 fixture 带空/垃圾 stdout，删掉退出码检查后仍会落四态④的 `Err`，测试照绿=空转。换成「非零退出 + 可解析 IP」后，删检查会返回 `Ok(Some(..))`，**由绿变错**。

### 7.5 回归契约

区分**存活测试被迫修改**（不允许，停下核查）与**被删模块自带单测随属主消失**（预期）。

**已知必改**：

- `client/core.rs:1207-1214` `initial_watch_snapshot_matches_legacy_empty_status` → 断言注入 mode **并改名**为 `initial_watch_snapshot_reflects_the_injected_mode`。
- `core/service/ipc.rs:140-187` 的两条 `target_ipc_state` 单测：函数不动，**测试随文件重整迁到 `core/service/probe.rs`**，断言不变。

---

## 8. 契约归属

> **口诀**：签名只能保证**「这个值到得了这里」**（及其对偶「到不了这里」）与**「这个类型在此平台不存在」**；凡「**不会去做某事**」一律靠测试 / 门禁 / `rg`。
>
> **v4 新增一条同源的**：**返回值的错误通道只报告调用的结果，不报告副作用的缺席**（§4.2）。

| 契约                                   | 由谁保证                      | 为什么可验证                                                                 |
| -------------------------------------- | ----------------------------- | ---------------------------------------------------------------------------- |
| 非 macOS 不存在 DNS 抽象               | **cfg / 类型**                | 非 macOS 上引用它编译不过                                                    |
| 调用方无法把陈旧探针结果喂给 reconcile | **签名**                      | `reconcile`/`reconcile_with` **没有 `IpcState` 参数**                        |
| **探针必然在有限时间内返回**           | **实现内部的 `timeout`**      | 单点可验（T-PROBE-06）；**不是**「每个调用方都记得包一层」那种不可强制的契约 |
| Service 写不会打到漂移后的设备         | **写前比对 + 返回值**         | `Err(TargetDrifted)` 可观测，T-DNS-14 钉住。**残余 TOCTOU 已记为 R1**        |
| 任何探针都不在守卫外开始               | **ledger / `rg` 门禁**        | `rg -n '\.probe\(\)'` 恒三处且位置固定                                       |
| `force_local_with` 只在超时分支用      | **`rg` 门禁**                 | 恰好一处调用点                                                               |
| DNS 路径选择不回头读全局               | **ledger 门禁**               | `core/actor/dns.rs` 的 `Config::*()` / `::global()` 计数恒 0                 |
| 顺序类契约                             | **测试**                      | 控制流性质，类型系统表达不了                                                 |
| 「核已停时不建立覆写」                 | **运行时准入检查 + 测试**     | `state.running.is_some()` 可读，T-DNS-13 钉住                                |
| **关停不会越过在飞控制序列**           | **`ControlAdmission` + 测试** | 有界 drain 可测（T-SD-02）；**R4/R5 两个残余已记账，不谎称关闭**             |
| `get_ipc_state` / statics 归零         | **`rg` 判据**                 | 删除类不变量                                                                 |

---

## 9. 门禁

1. **「diff 应为空」形态的判据，只要跑在中间提交之后，必须与基线比**：`git diff --exit-code <base>..HEAD -- <path>`；
2. **ledger 三步顺序**：report 核对 → `--write-snapshot` → gate 比对；
3. **删模块要有「模块不存在」断言**（本阶段涉及 `core/service/mod.rs::init_service`、`ipc.rs` 的轮询部分）。

**bindings 预期：**

| 变更                                                                                | wire 影响                                            |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------- |
| `uninstall_service` 改走 facade                                                     | **命令名与签名不变**，`bindings.ts:233` 不动         |
| `update_service` 调用点迁到 facade                                                  | 不在命令面上，无影响                                 |
| `SetTunDns` / `MacosDnsPort` / `ServiceProbe` / `ProbeOutcome` / `ControlAdmission` | **全部 `pub(crate)`**，不出现在命令面                |
| `set_mode` / `reconcile_mode`                                                       | **不新增命令**——模式变更是六个既有命令的**内部**后果 |

**结论：本 PR 的 bindings diff 恰好为空。** 判据：`git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`（与 `ci.yml:306-308` 同形）。

---

## 10. 风险与已知残留

| 风险                                       | 概率 | 影响                          | 缓解                                           |
| ------------------------------------------ | ---- | ----------------------------- | ---------------------------------------------- |
| **DNS 覆写在非管理员账户可能一直静默失效** | 中   | **误判为 5d 弄坏的**          | 见下                                           |
| **F55/F63 两个平台事实未确立**             | 中   | 分支②可能无法实现；读权限未知 | §4.8 的两情形表；§4.6 的「对答案不敏感」设计   |
| `Drop` 不覆盖强杀 → DNS 残留               | 中   | 退出后全机解析受影响          | 如实写明；兜底属 PR-6                          |
| smoke 3 不可验证                           | 高   | Exit 判据不可满足             | D4 已裁                                        |
| 三个预算常量实测不到                       | 中   | 界没有依据                    | **如实标注为选定值并写进 PR 描述**，不假装实测 |

### 10.1 已知残留（**有名字、有 owner、有移除条件**）

| #      | 残留                                                                      | 性质                                                                                                        | owner / 移除条件                                                                                                                     |
| ------ | ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| R1     | Service 模式下默认设备在**我们校验与 daemon 解析之间**变化 → 写落到新接口 | **既有**（今天 `core/clash/core.rs:109-118` 同样）。**5d 不修**，但从「不可检测」改善为「可检测、窗口极窄」 | 本表。移除条件 = `NetworkSetDnsReq` 增加可选设备字段且 daemon 遵从（上游 PR）。**不在 5d 做**：R0 未合并时不开第二个上游 PR          |
| R2     | 强杀后 DNS 覆写残留                                                       | **既有**（F23）                                                                                             | PR-6：启动时检测并清理残留覆写                                                                                                       |
| R3     | update 有界等待超时后 daemon 可能稍后才就绪，而已收敛 Local               | **5d 引入的取舍**（今天靠 5 s 轮询最终纠正）                                                                | 用户下次触发任一服务控制动作时重新 probe 纠正；降级文案点明「当前以 Local 模式运行」。**不加后台重试**——那会把第二个模式生产者请回来 |
| **R4** | 关停静默期超时后，被放弃的控制序列仍可能完成其外部命令                    | **5d 新引入的有界窗口**（今天是**无界**的同类问题，且更糟：连准入都没有）                                   | 本表。移除条件 = 外部命令改为可取消（需 `runas`/`sudo` 侧支持，不在本仓）                                                            |
| **R5** | `check_open()` 与外部命令 spawn 之间的 TOCTOU                             | 同上                                                                                                        | 与 R4 同条件                                                                                                                         |
| **R6** | **拿不到真机时四态②不实现** → restore-to-None 恒 `unverified`             | 取决于实施期硬件可得性                                                                                      | §4.8；移除条件 = 捕获真机 fixture                                                                                                    |

> ### 关于「非管理员账户可能一直静默失效」
>
> **推理链**：`networksetup -setdnsservers` 至少需要 admin 组身份（**F53，已从 man page + 社区证据确立，不是从代码形状推断**）→ 但代码**不提权**（F40/F46/F54）→ 失败被 `let _ =` 吞掉（F22）→ **没有任何观测点**。所以「这个功能在非管理员账户上可能从来就没工作过」**不是推测，而是当前代码结构下必然无法被发现的一类失效**——不是「碰巧没人报」，是**报不出来**。
>
> **加上退出码检查与回读校验之后它会第一次变得可见。这不是我们引入的回归，但我们会是发现它的人。**
>
> **判别方法**：在 5d 之前的版本上用同一账户手动跑一次 `networksetup -setdnsservers`，看是否需要授权。
>
> 与 5b 那条纪律方向相反但同源：那次是**别把既有缺陷算成我们引入的**，这次是**别把即将暴露的既有缺陷当成我们弄坏的**。

---

## 11. Exit 判据

| 要求                                                                                | 验证                                                                                                                                                                                        |
| ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 显式模式收敛全部走守卫                                                              | T-MODE-01/02/03；`rg -n '\.probe\(\)'` 恰好三处且位置为 §3.6 所列                                                                                                                           |
| `reconcile` 家族无 `IpcState` 参数                                                  | 签名核对（编译期即拦）                                                                                                                                                                      |
| **探针自身有界**                                                                    | T-PROBE-06；`OsServiceProbe` 内有 `timeout` 且 `status()` 设 `kill_on_drop`                                                                                                                 |
| **警告覆盖面不小于基线**                                                            | T-PROBE-03/05（Running 下三种 compat 全覆盖，且 Stopped-`Unknown` 不误报）                                                                                                                  |
| 删 `pending_run_type` 设计                                                          | **no-op**（F9）                                                                                                                                                                             |
| 删轮询线程与 statics                                                                | `rg 'IPC_STATE\|KILL_FLAG\|HEALTH_CHECK_RUNNING\|spawn_health_check\|get_ipc_state'` 为 0                                                                                                   |
| 删 `impl Default for RunType`                                                       | `rg 'RunType::default'` 为 0；`CoreStatusView::initial` **两个**调用点都传参，`mod.rs:371` 覆盖赋值已删                                                                                     |
| 六个服务控制入口签名一致且全在 `ServiceControlOps` 上                               | 结构核对：六个具体函数**仍在 `core::service::control`**，所有权未搬 = 满足 `design.md:333`（按 5a `:1037` 已确立读法）；例外条款条件另有独立证成（§2.4 表）；**扩到六个方法须写进 PR 描述** |
| **六个入口都在控制动作前拆 DNS**                                                    | T-DNS-05/06/17/18/26/27（**六条独立**）                                                                                                                                                     |
| **控制失败的四种处置**                                                              | T-CTL-01…04                                                                                                                                                                                 |
| **关停静默期**                                                                      | T-SD-01/02；**R4/R5 必须出现在 PR 描述里**                                                                                                                                                  |
| `MacosDnsGuard` 与 start/stop/backend-switch 保序                                   | T-DNS-02/03/12/15/16                                                                                                                                                                        |
| **写失败不会留下无记录的覆写**                                                      | T-DNS-19/28                                                                                                                                                                                 |
| **死锁序列已解**                                                                    | T-DNS-29                                                                                                                                                                                    |
| 核已停时不会建立新覆写                                                              | T-DNS-13                                                                                                                                                                                    |
| 有界等待能界住永不返回的探针                                                        | T-MODE-05                                                                                                                                                                                   |
| Service backend 用 IPC `set_dns` 写、**本地读**                                     | T-DNS 双适配器 parity + T-DNS-14；**F56 已记明 IPC 无读端点**                                                                                                                               |
| 非 macOS 不加空抽象                                                                 | cfg 门控                                                                                                                                                                                    |
| bindings diff 为空                                                                  | `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`                                                                                                                            |
| **四态读**                                                                          | T-DNS-08/10/11 必过；**T-DNS-09 与生产分支②同存亡**——拿不到真机则两者都不实现，且 PR 描述必须点名「四态②未实现、restore-to-None 恒 unverified（R6）」                                       |
| **smoke 2**（v1→v2 升级 + 拒绝升级 fail-closed Local）                              | 本机可跑，**须真实服务环境**；**它是 C2 的真正验收点**                                                                                                                                      |
| **smoke 3**（macOS TUN/DNS）                                                        | **未在本地验证且不可由 CI 覆盖**（D4）                                                                                                                                                      |
| **R1–R6 六条残留**逐条出现在 PR 描述里                                              | 文本核对——**「不修」必须是被记录的决定，不是沉默**                                                                                                                                          |
| **两处对 `design.md` 的有意偏离**（六方法 trait；DNS 兄弟端口）逐条出现在 PR 描述里 | 文本核对；`design.md` **本身不得修改**（基线不能中途搬动）                                                                                                                                  |
