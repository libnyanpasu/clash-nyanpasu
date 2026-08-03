# PR-5d 实施计划 — 运行模式探针与 macOS DNS 生命周期

**日期：** 2026-08-03
**版本：** v3（v2 被对抗审以 43/100 REJECT，七条 BLOCKING 全部上诉失败；leader 裁决另**推翻自己此前两条裁定**。本版按裁决重写，并已对着 **PR-5c 落地后的树**复核全部锚点）
**分支基线：** `refactor/core-manager-actor` @ **`a062f1019`**（v2 的锚点取自 `899b069f5`，**已作废**；主体复核在 `48c17a705` 完成，随后 5c 收尾三提交落地，差异已在 §0.5 逐条消化）
**权威 spec：** `task.md` 卡 C2、C3
**上游材料：** PR-5c v4 终态 `git show 5a02a1727:docs/superpowers/plans/2026-08-02-pr5c-residual-cleanup.md`
**平台：** Windows 11 / PowerShell（**macOS 路径无法本地验证**，见 §10）

> **本阶段是并发设计，不是清理。** 5c 的删除面靠「没有活调用者」即可证明；本阶段**每一条都要证明「每条路径都有人接、且接得住并发」**。
>
> **v2 被打回的根本原因只有一条：把「机制」写成了「结论」。** 「守卫覆盖住了」「有界等待」「可注入」——这些都是结论。v3 的硬性要求是：**凡形如「X 之后 Y 一定已发生」的断言，必须点名那个强制它的构造**（守卫、await 点、准入检查、`tokio::time::timeout` 的取消语义），并且该构造要出现在 §6 的表里。

---

## 0. 锚点复核结果（**本轮第一步，已完成**）

v2 的锚点全部取自 `899b069f5`。5c 之后已删除 `core/manager.rs`、`core/state.rs`、`Logger` global、`enum Instance`，并删掉 `core/clash/core.rs` 约 75%。复核结论如下。

### 0.1 已漂移、本版已改写的锚点

| 事实                                          | v2 锚点（`899b069f5`）                   | v3 锚点（`48c17a705`）                                                                                 | 说明               |
| --------------------------------------------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------------ | ------------------ |
| F13 `RunType::default()` 读两个 legacy global | `core/clash/core.rs:61-78`               | **`core/clash/core.rs:39-56`**                                                                         | 文件缩短，逻辑未变 |
| F12 `get_ipc_state()` 第五处生产读            | `core/clash/core.rs:70`                  | **`core/clash/core.rs:48`**                                                                            | 同上               |
| F19 覆写代码 + `previous_dns` 状态            | `core/clash/core.rs:404-457`、`:373-383` | **`core/clash/core.rs:74-126`、`:61`、`:69`**                                                          | 同上               |
| F20 读两个 global、Service/Local 双路径分叉   | `core/clash/core.rs:409,415-420,440-450` | **`core/clash/core.rs:78`（`RunType::default()`）、`:84-89`（`Config::clash()`）、`:109-118`（分叉）** | 同上               |
| §2.1 `RunType::default()` 调用点之一          | `core/clash/core.rs:409`                 | **`core/clash/core.rs:78`**                                                                            |                    |
| F15 `ServiceControlOps` 只有四个方法          | `core/actor/backend.rs:618-624`          | **`core/actor/backend.rs:619-624`**（`:618` 是 `#[async_trait]`）                                      |                    |
| F18 `MacosDnsGuard` 尚未存在的注释            | `feat.rs:417-418`                        | **`feat.rs:416-418`**（三行 TODO；**文案已由 `a062f1019` 改指 PR-5d**，见 §0.5）                       | 行号未动，文案已变 |
| F22 DNS 与 start/stop 无保序                  | `feat.rs:409-426`                        | **`feat.rs:410-412`（走 restart 的分支根本不碰 DNS）、`:415-424`（`let _ =` 吞失败）**                 | 拆成两条精确锚点   |
| F36 一次性 status 查询                        | `control.rs:351-376`                     | **`control.rs:350-376`**（`:350` 是 `#[tracing::instrument]`）                                         |                    |

### 0.2 已核实**未**漂移的锚点

`core/actor/gate.rs:20-30`、`:32-45`、`:55-60`；`core/actor/request.rs:78-92`（`:87` 取守卫、`:88` `set_backend`、`:82-85` 提前返回）；`core/actor/types.rs:44-50`；`core/service/ipc.rs:28-30`（三个 statics）、`:85-101`（`spawn_health_check`，`:97` 是 5 s）、`:103-124`（`health_check`）、`:131-138`（`target_ipc_state`）；`core/service/mod.rs:18-30`（boot 忙等在 `:26-28`）；`client/mod.rs:303-306`（bootstrap 读 `get_ipc_state()`）、`:544`；`feat.rs:383,401`；`utils/init/mod.rs:251`（update 调用点）；`.github/workflows/ci.yml:201-215,303-304`。

### 0.3 5c 已消灭的锚点（本版删除）

- `core/clash/core.rs:399`（`CoreManager::status` 内的 `RunType::default()`）——**该函数已随 5c 删除**，v2「实施前复核」的悬念**已结清**：`RunType::default()` 的生产调用点现在是 **两处**（`core/actor/types.rs:48`、`core/clash/core.rs:78`）加一处测试、一处注释。

### 0.4 复核过程中发现的、v2 漏掉的事实（**不是漂移，是遗漏**）

| ID      | 事实                                                                                                                                                                                                                                                                                                     | 锚点                                                                                                                                                   |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **F42** | **`CoreStatusView::initial()` 有两个调用点**，v2 的 D2 表只列了一个。`core/actor/mod.rs:368-370` 先调 `initial()` 再用 `observed.view.run_type = args.mode;` **把它覆盖掉**——actor 侧其实早已是注入的，`initial()` 里的 `RunType::default()` 在这条路径上是**一次白读**。D2 加参后 `:370` 那行覆盖也要删 | `client/core.rs:111`；`core/actor/mod.rs:368-370`                                                                                                      |
| **F43** | **`SetBackend` 会把核停掉**：`replace_backend` 在 `:268` 置 `self.running = None`，在 `:282` 才改 `self.mode`。因此「换后端之后重新施加 DNS」的正确时点**不是 `SetBackend` 之后，而是随后的 `Run` 之后**                                                                                                 | `core/actor/mod.rs:266-296`                                                                                                                            |
| **F44** | **`state.running: Option<CoreRequest>` 是现成的「核是否在跑」判据**：`Run` 置 `Some`（`:514`）、`Stop` 置 `None`（`:532`）、`replace_backend` 置 `None`（`:268`）、`Shutdown` 置 `None`（`:605`）                                                                                                        | `core/actor/mod.rs:57`                                                                                                                                 |
| **F45** | **`ServiceControlOps` 的六个控制入口签名不齐**：`install`/`start`/`restart` 收 `CoreModeReconciler`（因为要起轮询线程），`stop`/`update`/`uninstall` 不收；且 `update`/`uninstall` **根本不在 trait 上**                                                                                                 | `control.rs:58,106,149,188,234,283`；`backend.rs:619-624`                                                                                              |
| **F46** | **`nyanpasu-utils` 全 crate 无 `administrator privileges`**（`rg -c` 无命中）。所以 `osascript` 这一跳**不提权**，去掉它不改变权限语义                                                                                                                                                                   | `crates/nyanpasu-utils/`（全 crate grep）                                                                                                              |
| **F47** | **IPC `set_dns` 的 wire golden 确实存在**，但不在 v2 引的那个锚点上                                                                                                                                                                                                                                      | golden：`nyanpasu_ipc/tests/wire_golden.rs:282-295`；端点：`.../client/shortcuts.rs:91-96`                                                             |
| **F48** | **`pnpm test` → `cargo test --all-features` 的展开链**是两跳，v2 只引了一跳                                                                                                                                                                                                                              | `package.json:40`（`"test": "run-p test:*"`）→ `package.json:42`（`"test:backend": "cargo test --manifest-path ./backend/Cargo.toml --all-features"`） |

### 0.5 复核期间落地的 5c 收尾三提交（**逐条消化，不是「应该没影响」**）

主体复核在 `48c17a705` 完成，随后三个提交落地。逐条核过，**其中一个真的动了本计划的论据**：

| 提交        | 改了什么                                                                    | 对本计划的影响                                                                                                                                                                                                                   |
| ----------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `0e20f35ba` | `scripts/architecture-ledger.ts` 学会正确词法分析 Rust 字符字面量与裸字符串 | **无锚点影响**。本计划 §9 只用 ledger 的三步顺序，不引它的行号。**顺带受益**：§8 的两条 ledger 门禁（`core/actor/dns.rs` 的 `Config::*()` 计数、`.probe()` 计数）依赖扫描器不把注释/字符串里的字样算进去，这次修复正好加固了它们 |
| `a86478a7f` | 重写 roadmap §6.3：C2/C3 移交 PR-5d，并指名**本计划**为权威实施计划         | **真影响，已改**——见下                                                                                                                                                                                                           |
| `a062f1019` | `feat.rs` 与 `core/service/ipc.rs` 的迁移标记文案由 PR-5c 改指 PR-5d        | **行号未动**（`feat.rs:416-418`、`ipc.rs:126-128`），仅 F18 的**描述**需改：那三行不再写「等 PR-5c 建它」。已改。**`change_default_network_dns` 本体与 `let _ =` 吞错行为一行未变**，F19/F20/F22/F40/F41 全部成立                |

> **`a86478a7f` 删掉了我原先援引的那句例外条款。** v3 的 §2.5 原文写着「roadmap 原文带例外：_除非测试确实需要替换 OS command runner_」，据此为「把 `ServiceControlOps` 扩到六个方法」开脱。
>
> **那句话现在不存在了。** 新 §6.3 把整条 C2 要点删除，其中就包括「**不引入完整 `ServiceControlPort`**（除非测试确实需要替换 OS command runner）」。核实：`rg 'ServiceControlPort|ServiceController'` 在 `docs/design/actor-migration-roadmap.md` 与 `task.md` 上**均无命中**。
>
> **所以正确的结论比原先更强，不是更弱**：活着的约束只剩 `task.md` 卡 C2 的「service install/update/uninstall 保持独立 concrete controller，**不迁入 CoreActor**」，而本设计**直接满足**它——facade 调 controller，trait 不是 actor。**扩 trait 不需要任何例外条款。**
>
> **但不许因此悄悄把它当没发生过。** 该约束在 `48c17a705` 时确实存在（`git show 48c17a705:docs/design/actor-migration-roadmap.md` 第 330 行），是 `a86478a7f` 随 C2/C3 移交一并删除的。**出处与消失时点都记在这里**，§2.5 据此改写。

---

## 1. 已核验事实（编号保持原号；锚点已按 §0 更新）

### 1.1 C2 —— 运行模式

| ID      | 事实                                                                                                                                   | 锚点                                                                                                                                |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| F9      | `pending_run_type` 在 Rust 源码中**不存在**（仅设计文档命中）→ 卡面该项是 **no-op**                                                    | 全仓 `rg`（命中仅 `docs/`）                                                                                                         |
| F10     | 「reconcile 走 `CoreOperationGuard`」**已满足**                                                                                        | `core/actor/request.rs:87`                                                                                                          |
| F11     | 5 s 轮询与三个 statics 全在一个文件；`spawn_health_check` **定义**在 `ipc.rs`，**四处 spawn 调用点在别处**                             | 定义与 statics：`core/service/ipc.rs:28-30,85-101`（`:97` 是 5 s）；**调用点：`control.rs:101,229,324` + `core/service/mod.rs:25`** |
| F12     | `get_ipc_state()` **5 处生产读**                                                                                                       | `feat.rs:383,401`；`client/mod.rs:305,544`；`core/clash/core.rs:48`                                                                 |
| F13     | `RunType::default()` 读两个 legacy global，且被 `CoreStatusView::initial()` 调用——**删 statics 的主阻塞点**                            | `core/clash/core.rs:39-56`；`core/actor/types.rs:44-50`                                                                             |
| F14     | `set_backend` **生产调用点恰好一个**；**不存在 `set_mode`**                                                                            | `core/actor/request.rs:88`                                                                                                          |
| F15     | `ServiceControlOps` 只有 install/start/stop/restart；**update / uninstall 不在 trait 上**                                              | `core/actor/backend.rs:619-624`                                                                                                     |
| F16     | `uninstall_service` **绕过 facade**（Tauri 命令直调自由函数）；`install_service` 在 facade 上**不 reconcile**                          | `ipc.rs:936-937`；`client/mod.rs:504-510`                                                                                           |
| F35     | **`IPC_STATE` 初值 `Disconnected`**，bootstrap 在任何 health check 之前读它 → **今天 bootstrap 恒判 `Normal`**，靠首次轮询**异步纠正** | `core/service/ipc.rs:28`；`client/mod.rs:303-306`                                                                                   |
| F36     | **探针两半已存在**：`control::status()`（子进程）+ **纯函数** `target_ipc_state()`；`health_check` = 两半 + 循环                       | `control.rs:350-376`；`ipc.rs:131-138`、`:103-124`                                                                                  |
| F45     | 六个控制入口签名不齐，update/uninstall 不在 trait 上                                                                                   | 见 §0.4                                                                                                                             |
| **F49** | **`install_service` 之后服务会自己起来**——代码注释明说，且紧接着就拉起 health checker                                                  | `control.rs:99-102`                                                                                                                 |

> **F49 直接推翻 v2 §2.5 的裁定。** v2 写的「装服务不等于起服务，没有可 reconcile 的对象」在基线上就是**假的**。见 §2.5。

### 1.2 C3 —— macOS DNS

| ID      | 事实                                                                                                                                                                        | 锚点                                                                                                                                                |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| F18     | **`MacosDnsGuard` 不存在**（仅一条迁移标记；`a062f1019` 后该标记指向 **PR-5d**，即**本计划就是它的移除条件**）                                                              | `feat.rs:416-418`                                                                                                                                   |
| F19     | 真正的覆写代码是 `CoreManager::change_default_network_dns` + `previous_dns` 状态                                                                                            | `core/clash/core.rs:74-126`、`:61`、`:69`                                                                                                           |
| F20     | 它读两个 global，且 **Service / Local 双路径在此分叉**                                                                                                                      | `core/clash/core.rs:78`、`:84-89`、`:109-118`                                                                                                       |
| F21     | **IPC `set_dns` 已上线**：端点 + wire golden 均在，但**是两个锚点**                                                                                                         | 端点：`nyanpasu_ipc/src/client/shortcuts.rs:91-96`；**wire golden：`nyanpasu_ipc/tests/wire_golden.rs:282-295`（`the_set_dns_request_is_pinned`）** |
| F22     | **DNS 与 start/stop 今天毫无保序**；**走 restart 的分支根本不碰 DNS**；失败被 `let _ =` 吞掉                                                                                | `feat.rs:410-412`；`:415-424`                                                                                                                       |
| F23     | **退出不恢复 DNS**——覆写跨崩溃/退出泄漏（**5c 之前就存在的缺陷**）。两处「本该恢复而没恢复」的位置：退出清理只 reset sysproxy；actor `Shutdown` 不碰 DNS                    | `utils/resolve.rs:288-291`；`client/core.rs:277-283`                                                                                                |
| F24     | `SystemDnsCache` 只管 flush，**与 TUN 的 DNS 覆写生命周期无关**，勿混淆                                                                                                     | `client/system_dns.rs:4-7`                                                                                                                          |
| F40     | **Local 写路径不提权**：`osascript` 调用**不带** `with administrator privileges`（F46 已在全 crate 核实），脚本本体只有 `networksetup -setdnsservers $1 $2`                 | `crates/nyanpasu-utils/src/network/mod.rs:27-55`；`.../scripts/set-macos-dns.sh:3`                                                                  |
| F41     | **读路径不检查退出码**，空/不可解析 stdout → `Ok(None)`。因此**当原始 DNS 本就是 `None` 时，一次失败的读会与期望值「相等」**——回读校验在该情形下把失败误报成成功            | `crates/nyanpasu-utils/src/network/mod.rs:57-88`（判定在 `:81-87`）                                                                                 |
| **F50** | **设备名是被文本拼进 bash 脚本的**：`include_str!(...).replace("$1", service_name)` 写临时文件后 `osascript -e "do shell script \"bash <path>\""`；脚本里 **`$1` 未加引号** | `crates/nyanpasu-utils/src/network/mod.rs:27-55`；`scripts/set-macos-dns.sh:3`                                                                      |
| **F51** | **Service 侧 DNS 的线上契约里没有设备**：`NetworkSetDnsReq { dns_servers: Option<Vec<Cow<IpAddr>>> }`；服务端**每次请求**自行解析当前默认硬件端口                           | `nyanpasu_ipc/src/api/network/set_dns.rs:8-11`；`crates/nyanpasu-service-runtime/src/server/routing/network.rs:26`、`:39`                           |
| **F52** | **上游读脚本会把换行压成空格**：`RES=$(networksetup -getdnsservers $1); echo $RES`——`echo $RES` 未加引号。我们改为直调 `networksetup` 后，输出是**换行分隔**的原始格式      | `crates/nyanpasu-utils/src/network/scripts/get-macos-dns.sh:3-4`                                                                                    |

### 1.3 smoke / CI

| ID  | 事实                                                                                                   | 锚点                                                                                                                          |
| --- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| F33 | CI **有** macOS runner 且在 PR 上跑 `cargo test --all-features` → **cfg 门控单测真实运行**。展开链两跳 | `.github/workflows/ci.yml:201-215`（矩阵含 `macos-latest`）、`:303-304`（`pnpm test`）；`package.json:40` → `package.json:42` |
| F34 | **但没有任何作业能跑 smoke 3**——无作业启动应用；TUN 需签名扩展 + root，**是能力边界非配置缺失**        | `ci.yml`（全仓仅 `:304` 一处测试调用）                                                                                        |

### 1.4 平台事实（**从文档确立，不从代码形状推断**）

| ID      | 事实                                                                                                                                             | 依据                                                                                                                                                                                                                                                                                                                                                                                                                                  | 置信度                                                   |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| **F53** | **`networksetup` 的写操作至少需要 admin 组身份**；若系统开启「Require an administrator password to access system-wide preferences」，则需要 root | ①`networksetup` man page（ss64 转录：_"requires at least admin privileges to change network settings. If the 'Require an administrator password…' option is selected… then root privileges are required"_，https://ss64.com/mac/networksetup.html）；②Stack Overflow 11819336 记录了真实的授权弹窗 _"networksetup is trying to modify the system network configuration. Type your password to allow this."_，公认解法是 `sudo` / suid | **高**（两个独立来源一致；来源二是社区证据不是一手文档） |
| **F54** | **`osascript` 那一跳不改变权限**：它以同一用户身份执行，且不带 `administrator privileges`（F46 已在全 crate 核实无该字符串）                     | F46 + F53                                                                                                                                                                                                                                                                                                                                                                                                                             | **高**                                                   |
| **F55** | **`networksetup -getdnsservers` 在无 DNS 时输出的确切文案，本轮未能从可引用来源确立**                                                            | 三轮检索（man page 转录 / Apple 讨论区 / SE）**均未给出该字面串**                                                                                                                                                                                                                                                                                                                                                                     | —                                                        |

> **F55 是本计划唯一一处「查了但没查到」，必须显式记账。** 它直接决定 T-DNS-09 的形态（§7.3）：**不许拍一个字面串**——那会让生产与测试共用同一个臆造常量，测试变成自证。

---

## 2. 已裁定事项

### 2.1 D2 = A —— `CoreStatusView::initial(mode)` 加参、删 `impl Default for RunType`

`RunType::default()` 读 `Config::verge()` + `get_ipc_state()` 却被 `CoreStatusView::initial()` 调用，是典型隐藏依赖（F13）。

**`RunType::default()` 的调用点（复核后的完整清单）：**

| 位置                                | 处置                                                                                                                                                                                   |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core/actor/types.rs:48`            | D2 主目标，改为参数                                                                                                                                                                    |
| `core/clash/core.rs:78`             | macOS DNS 路径分叉 → 随 C3 迁走（该 `CoreManager` 整体消失，见 §4）                                                                                                                    |
| `client/core.rs:1211`               | **测试**，必然改动：断言注入的 mode，**并连名字一起改**为 `initial_watch_snapshot_reflects_the_injected_mode`（旧名里的「legacy empty status」在 D2 之后不再是参照物，**命名即契约**） |
| `client/process_core_bridge.rs:251` | 注释里的警告，删后**悬空**，顺手清理或改写                                                                                                                                             |

**`CoreStatusView::initial()` 的调用点（F42，v2 漏掉一个）：**

| 位置                    | 处置                                                                                                               |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `client/core.rs:111`    | 改为 `initial(args.mode)`                                                                                          |
| `core/actor/mod.rs:368` | 改为 `initial(args.mode)`，**并删掉 `:370` 那行 `observed.view.run_type = args.mode;` 覆盖**——加参之后它是重复赋值 |

### 2.2 D3 = A（含修正形态）—— DNS guard 挂 actor state，但 `Drop` 不恢复

**主路径**（`Stop` / `Shutdown` / `SetBackend`）：actor 处理器内**显式 `await` 恢复**，**在**后端动作与 reply **之前**完成。

**`Drop`**：**只记 `tracing::error!`，措辞按不变量破坏写**（`"reached Drop with DNS override still active — main-path restoration was missed"`），**不尝试任何恢复**。

> **为什么不做「尽力而为的同步 Drop」**：Service 侧同步做不到、Local 侧能做——**那半个兜底恰好在开发者最常用的模式下生效**。开发日常跑 Local，兜底在 Local 上有效 → **主路径漏了恢复也不会被发现**；等到 Service 模式（用户实际部署、开发者最少跑的那条）才暴露。**一个「在你测得到的地方生效、在你测不到的地方失效」的兜底，是反向选择的一半。**

**恢复失败去向**：degradation sink（`DegradationPhase::CoreLifecycle`、`code = "macos_dns_restore_failed"`）。`Degradation { phase, code, message, retryable }` 的形状见 `client/runtime.rs:376-382`，`CoreLifecycle` 见 `:398`。

**`Drop` 不覆盖强杀**（SIGKILL / 任务管理器）——**如实写明**，兜底（启动时检测并清理残留覆写）属 **PR-6**，不在本阶段。

### 2.3 D4 —— smoke 3 记为「未在本地验证**且不可由 CI 覆盖**」

用户裁定路径乙。**不是「CI 暂未配置」，是托管 runner 的能力边界**（F34）：加 job、加 runner 都解决不了，需**自托管 mac 且预先批准网络扩展**。

**CI 覆盖的**：cfg 门控单测（顺序、降级等**逻辑**契约，F33）。
**未验证的（逐条点名）**：①真实 TUN 开关是否触发覆写；②真实 `networksetup` / IPC `set_dns` 是否成功改写系统 DNS；③关 TUN 与正常退出后 DNS **是否真的恢复**；④Service 与 Local 两条路径在真机上是否一致；⑤**F55 的无-DNS 文案**（新增，见 §7.3）。

**结论必须显式出现在 PR 描述与发布说明里，不允许沉默跳过。**

### 2.4 `SetTunDns` 的准入 —— **两条规则，各带测试**（v2 只有散文）

见 §4.3 的完整准入设计。此处只记裁定：`SetTunDns` **参与守卫**，且**「`Shutdown` 之后拒绝」与「`Stop` 之后拒绝开启」是两条不同的规则、由两个不同的构造强制**，不能合并。返回 `Err` 而非静默丢弃——静默丢弃会让调用方以为设置成功。

### 2.5 §7 两处不对称 —— **v2 的裁定被推翻，两处都是缺陷**

> **更正：v2 §2.5 说「`install_service` 不 reconcile 是有意的，因为装服务不等于起服务」。F49 证明这在基线上就是假的**——`control.rs:99-102` 的注释明说「大多数平台上服务安装后会自动启动」，紧接着就拉起 health checker。v2 还与自己的 §3.2、T-MODE-02 互相矛盾。**该 carve-out 整条删除。**

| 项                                         | v3 裁定                                                                                                                                                                                    |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `install_service` 在 facade 上不 reconcile | **缺陷，改为与另外五个同形**                                                                                                                                                               |
| `uninstall_service` 绕过 facade            | **缺陷，改走 facade**——①违反「Tauri 命令是薄适配器」；②**实质风险**：核在 Service 模式运行时卸载服务会让当前后端失效。**不违反 C2 卡**：C2 禁的是「迁入 CoreActor」，**facade 不是 actor** |

**六个入口如何统一到同一形态**（回答裁决 §2 要求的「说清每一个怎么到齐」）：

| 入口                              | 今天的签名                                          | 今天缺什么                                                  | 到齐的动作                                                             |
| --------------------------------- | --------------------------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------- |
| `install_service` `control.rs:58` | 收 `CoreModeReconciler`（只为在 `:100-102` 起轮询） | facade 不 reconcile                                         | **删参**（轮询消失后无人需要它）；facade 走统一序列                    |
| `start_service` `:188`            | 收 reconciler（`:228-230` 起轮询）                  | —                                                           | **删参**；facade 序列                                                  |
| `restart_service` `:283`          | 收 reconciler（`:323-325` 起轮询）                  | DNS 不拆（F22）                                             | **删参**；facade 序列 + DNS 拆除                                       |
| `stop_service` `:234`             | 不收                                                | —                                                           | 上 trait；facade 序列                                                  |
| `update_service` `:106`           | 不收                                                | 不在 trait 上；调用点在 `utils/init/mod.rs:251` 而非 facade | 上 trait；**改由 facade 调用**；facade 序列 + **有界等待就绪**（§3.4） |
| `uninstall_service` `:149`        | 不收                                                | 不在 trait 上；被 `ipc.rs:936-937` 直调                     | 上 trait；**改由 facade 调用**；facade 序列                            |

**结果：六个入口签名一致（`async fn(&self) -> anyhow::Result<()>`），六个都在 `ServiceControlOps` 上。**

> **这与「不引入完整 `ServiceControlPort`」冲突吗？不冲突，而且理由比 v3 初稿写的更直接。**
>
> 那条约束**只存在于 roadmap，且已被 `a86478a7f` 随 C2/C3 移交一并删除**（§0.5 记了出处与消失时点；现`rg 'ServiceControlPort|ServiceController'` 在 roadmap 与 `task.md` 上均为 0）。**活着的约束**是 `task.md` 卡 C2 的「service install/update/uninstall 保持独立 concrete controller，**不迁入 CoreActor**」——本设计直接满足：**facade 调 controller，`ServiceControlOps` 不是 actor**。
>
> 因此扩 trait**不依赖任何例外条款**。但「六个方法的 trait」相对 roadmap 的历史意图仍是扩大面，**必须写进 PR 描述**，不是默默扩。触发它的是 T-MODE-02：**六个控制动作各自独立断言 probe+reconcile**，需要能替换 OS command runner。

---

## 3. C2 设计 —— 服务状态探针与调用点

### 3.1 探针（一次性、经兼容门控、**注入路径已具体到字段**）

```rust
// core/service/probe.rs（新）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait ServiceProbe: Send + Sync + 'static {
    /// 一次性查询。失败按 fail-closed 处理为 Disconnected（与今天 health_check
    /// 的 Err 分支同语义）。ServiceCompat 一并返回——警告职责的接手方靠它（§3.3）；
    /// Option<anyhow::Error> 是探针失败的原因，接手方见 §3.3。
    async fn probe(&self) -> (IpcState, ServiceCompat, Option<anyhow::Error>);
}

pub(crate) struct OsServiceProbe;   // control::status() + target_ipc_state()
```

`target_ipc_state` 与 `ServiceCompat` **一行不改**——PR-5-pre 已审的 fail-closed 门，探针只是宿主。

**注入路径（逐跳点名，v2 只说了「可注入」）：**

```text
composition root: client/mod.rs::try_new_with_args
  └─ ClientSetupArgs { .., probe: Arc<dyn ServiceProbe>, .. }      ← 新字段，紧挨现有 service_control（client/mod.rs:85）
       ├─ ①bootstrap 自用：client/mod.rs:303-306 的 get_ipc_state() 换成 probe.probe().await
       └─ NyanpasuClientInner { .., probe: Arc<dyn ServiceProbe> } ← 新字段，紧挨 service_control（client/mod.rs:257）
            └─ core_mode_reconciler()（client/mod.rs:467-473）在字面量里加 probe: self.inner.probe.clone()
                 └─ CoreModeReconciler { core, application, requests, clash_config, probe }（core/actor/request.rs:70-75）
```

`CoreModeReconciler` 是 `#[derive(Clone)]`，加 `Arc<dyn ServiceProbe>` 不破坏 Clone。测试侧沿用 `test_service_control()`（`client/mod.rs:2767`）的模式加一个 `test_service_probe()`。

**`OsServiceProbe` 必须设 `.kill_on_drop(true)`。** 今天 `control::status()`（`control.rs:352-356`）没设。有界等待（§3.4）靠丢弃 future 取消 await，但 `tokio::process::Command` 默认**不**杀子进程——不设这个 flag，「有界」就只界住了我们自己的等待，界不住残留的子进程。**这是一条生产代码行，不是注释。**

### 3.2 六个控制入口 + 三个非控制入口 = 九处调用点

**统一形态（六个控制入口全部照此，无例外）：**

```text
guard = core.begin_operation().await?        ← 取守卫
  ├─ 拆 DNS（await；见 §4.4，六个都拆）
  ├─ service_control.<action>().await        ← 外部控制动作
  ├─ reconciler.reconcile_with(&guard).await ← **在守卫内**探针 + 应用
  └─ drop(guard)
```

| #   | 位置                                                      | 今天怎么拿模式                                  | 改为                                                                                                    |
| --- | --------------------------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| 1   | **bootstrap**（`client/mod.rs:303`）                      | `get_ipc_state()`（**恒 `Disconnected`**，F35） | `probe()` 一次——**顺带修掉 F35 这个既有缺陷**。**这是唯一不在守卫内的探针**，理由见 §3.5                |
| 2   | **install**（facade `client/mod.rs:504-510`）             | 不 reconcile（F16）                             | 统一形态                                                                                                |
| 3   | **start**（`:512-521`）                                   | 轮询 + `reconcile(get_ipc_state())`             | 统一形态                                                                                                |
| 4   | **restart**（`:530-539`）                                 | 同上                                            | 统一形态                                                                                                |
| 5   | **stop**（`:523-528`）                                    | 同上                                            | 统一形态                                                                                                |
| 6   | **uninstall**（今天在 `ipc.rs:936-937`）                  | 无                                              | **迁到 facade** + 统一形态                                                                              |
| 7   | **update**（今天在 `utils/init/mod.rs:251`）              | 轮询（v1→v2 升级后靠它发现 v2）                 | **迁到 facade** + 统一形态，且控制动作与 reconcile 之间插**有界等待就绪**（§3.4）——**直接关系 smoke 2** |
| 8   | **`enable_service_mode` 配置变更后**                      | 轮询 + reconcile（有 §3.6 的洞）                | `reconcile()`（自取守卫版，内部探针）                                                                   |
| 9   | **boot 的 `init_service`**（`core/service/mod.rs:18-30`） | 起轮询线程 + 忙等 100 ms                        | `reconcile()`，**删忙等与整个函数**                                                                     |

### 3.3 探针输出的两个接手方（**都要点名**）

| 输出                                    | 接手方                                                                 | 动作                                                                                                             |
| --------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `ServiceCompat::Incompatible`           | **`CoreModeReconciler::reconcile_with` 内、`classify` 之前的唯一一处** | `tracing::warn!`（smoke 2 要的就是这一条）。**九处调用点不各自发**，否则刷屏                                     |
| `Option<anyhow::Error>`（探针自身失败） | **同一处**                                                             | `tracing::warn!` **+ degradation**：`phase = CoreLifecycle`、`code = "service_probe_failed"`、`retryable = true` |

> 第二行是裁决 §9 点名要补的。**没有接手方 = 普通探针失败会静默变成 Local**，用户看到「服务模式没生效」而日志里什么都没有。轮询删掉之后 reconcile 的调用频次是有界的（九处），不存在刷屏风险。

### 3.4 update 的有界等待就绪（**裁决 §6：必须能界住一个永不返回的探针**）

`update_service()` **只等更新进程退出、不等 daemon 就绪**。删掉轮询后，一次立即 probe 可能把模式**永久判成 `Normal`**（没有轮询再来纠正了）。

```rust
// CoreModeReconciler::await_service_ready(&self, guard: &CoreOperationGuard) -> ReadyOutcome
let deadline = Instant::now() + READY_BUDGET;
let mut backoff = INITIAL_BACKOFF;
loop {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { return ReadyOutcome::TimedOut; }
    // 关键：per-attempt 预算取 min(remaining, PER_PROBE_BUDGET)。
    // tokio::time::timeout 到点会**丢弃**这个 future——丢弃即取消，
    // 因此一个永不返回的 probe 也**不可能活过 deadline**。
    match tokio::time::timeout(remaining.min(PER_PROBE_BUDGET), self.probe.probe()).await {
        Ok((IpcState::Connected, compat, _)) if compat.allows_service_backend() =>
            return ReadyOutcome::Ready,
        _ => {
            tokio::time::sleep(backoff.min(deadline.saturating_duration_since(Instant::now()))).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }
}
```

四条要求逐条落位：

| 裁决要求                                                             | 本设计里的构造                                                                                                                                                                  |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| (a) 外层 deadline 要能**取消在飞的 probe**，不是只在两次轮询之间检查 | `tokio::time::timeout(remaining.min(PER_PROBE_BUDGET), fut)` —— 到点丢弃 `fut`，丢弃即取消。**加上 `OsServiceProbe` 的 `.kill_on_drop(true)`（§3.1），子进程也一并终止**        |
| (b) 明确的轮询间隔 / 退避                                            | 指数退避 `INITIAL_BACKOFF` ×2 封顶 `MAX_BACKOFF`；且 sleep 时长再与剩余预算取 min，避免最后一次睡过头                                                                           |
| (c) 超时后**仍持守卫**降级到 Local，不是 `Err`                       | `ReadyOutcome::TimedOut` → `reconciler.force_local_with(&guard)`（§3.5 第三个签名）+ degradation `code = "service_update_not_ready"`、`retryable = true`。控制动作本身返回 `Ok` |
| (d) 用一个**永不返回**的 probe 证明能及时降级                        | T-MODE-05（§7）                                                                                                                                                                 |

**为什么超时是 degraded 而不是 `Err`：** 更新进程本身成功退出了。返回 `Err` 等于告诉用户「更新失败了」，那是假的——更新完成了，只是服务还没应答。而 Local 是一个**合法运行状态**（PR-5-pre 的 fail-closed 门本就如此设计）。与 5b 的 I-A 同源：**已经成功的事不许报成失败；没做成的后置副作用报降级。**

**常量的来源必须分开记**（裁决要求「界要从测量来，不是拍的」）：

| 常量                              | 是否需要实测 | 依据                                                                                                 |
| --------------------------------- | ------------ | ---------------------------------------------------------------------------------------------------- |
| `READY_BUDGET`                    | **是**       | 实测 daemon 从 `update_service()` 返回到 `status()` 报兼容的耗时，取上界留余量；**依据写进实施报告** |
| `PER_PROBE_BUDGET`                | **是**       | 实测一次正常 `control::status()` 子进程往返耗时的上界                                                |
| `INITIAL_BACKOFF` / `MAX_BACKOFF` | 否           | 不是正确性边界（正确性由 `READY_BUDGET` 单独界住），只影响探测密度。**如实标注为选定值，不假装实测** |

### 3.5 reconcile 的三个签名（**裁决 §3：线性化点在 probe，不在 publish**）

v2 的形态有一个致命缺陷：**薄包装在接受一个已算好的 probe 结果之后才取守卫**，那保留了原缺陷——probe → 输给一个有守卫的 stop → 回来一个陈旧的 `Connected` → 发布 `Service`。

```rust
impl CoreModeReconciler {
    /// 自取守卫 → **在守卫内探针** → 应用。九处调用点里唯一的无守卫入口（#8、#9）。
    pub(crate) async fn reconcile(&self) -> anyhow::Result<()>;

    /// 已持守卫：**在守卫内探针** → 应用。控制动作（#2..#7）用这个。
    /// **注意没有 IpcState 参数。**
    pub(crate) async fn reconcile_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()>;

    /// 已持守卫、**不探针**、直接落到 Local。**仅供 §3.4 的超时分支**。
    pub(crate) async fn force_local_with(&self, guard: &CoreOperationGuard) -> anyhow::Result<()>;
}
```

**强制构造：`reconcile` / `reconcile_with` 都没有 `IpcState` 参数——调用方在类型上就无法喂进一个陈旧探针结果。** 这是签名能给的那一类保证（「这个值到得了这里」的对偶：这个值到不了这里），不是约定。

今天 `reconcile(&self, ipc_state)` 在 `request.rs:78-92`，两个调用点（`ipc.rs:78`、`client/mod.rs:544`）都传 `get_ipc_state()`——**两处随本 PR 一起消失**。

**「任何探针都不许在守卫外开始」是「不会去做某事」型契约，签名管不了**，因此落到 §9 的 ledger 门禁：

```text
rg -n '\.probe\(\)' backend/tauri/src  →  恰好三处：
  ① core/actor/request.rs  reconcile_with 内
  ② core/actor/request.rs  await_service_ready 内（该函数只在持守卫时被调用）
  ③ client/mod.rs          bootstrap
```

**bootstrap（③）是唯一的守卫外探针，理由是真排除而不是指望**：它发生在 `client/mod.rs:303`，而 `CoreClient::new(...)` 在 `:312`——**actor 那时还不存在**，没有任何别的操作能在飞，也没有守卫可取。两行在同一个 `async move` 块里，源码顺序即执行顺序。

`force_local_with` 同样上 `rg` 门禁：**恰好一处调用点**（§3.4 的超时分支）。

### 3.6 修 Service→Normal 缺口

今天（`request.rs:82-85`）提前返回导致 `classify(true, ..)` 硬编码，**用户关闭服务模式后 reconcile 什么都不做、后端停留在 Service**。

**改法**：删掉提前返回，把真值送进 `classify`。`classify` 本身**不改**——它已经正确（`core/clash/core.rs:30-36`），缺的只是有人把 `false` 喂给它。

### 3.7 步骤顺序：**先建后删**，但**不是「双轨并行」**

> **5c v4 曾把这一步写成「轮询仍在跑、双轨等价、任一步可独立回滚」——那是错的**：**两个生产者同时写同一状态而无定序，比一个更糟**。单生产者的错误是确定性的、可复现的；双生产者的错误是竞态的。**「保留旧机制」看似保守，实际引入了一个新的失效模式。**

- **S-a**：建探针 + 修 3.6 的缺口 + 接上九处调用点，**同时在同一步停掉轮询的 reconcile 派发**——**关键是任何时刻只有一个模式生产者**；
- **S-b**：删轮询线程与三个 statics、`RunType::default()`（D2）、`core/service/mod.rs::init_service`。

5c 携带的 `KILL_FLAG` weak-CAS 缺陷（`control.rs:274`）**随轮询线程删除而消失，不单独修**（5c §10.1 已记账）。

---

## 4. C3 设计 —— DNS 端口、状态机与恢复

> v2 在 `:293-300` 写着「待设计」，同时第 4 行宣称五条已知问题全部定稿——**这条自相矛盾本身就足以拦住实施**。§4 把整个模型写完。

### 4.1 端口（macOS-only，注入式，**不对 Service 撒谎**）

裁决 §5 的约束：**Service 侧的线上契约里没有设备**（F51），所以 `set(device, dns)` 这种签名是**假保证**。同时 Local 侧**确实**能指定设备。端口必须把这个不对称如实建模，且**不能接一个自己会悄悄忽略的设备参数**。

```rust
// core/actor/dns.rs —— 整个文件 #[cfg(target_os = "macos")]

/// 一次覆写所针对的目标。**它只能由端口自己产出**，调用方不构造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DnsTarget {
    /// Local：适配器解析到的硬件端口名。我们能命名它，也能在之后重新指向它。
    Device(String),
    /// Service：IPC 契约里没有设备字段，daemon **每次请求**自解析当前默认设备（F51）。
    /// 我们既命名不了它，也检测不到它变了。
    ServerResolvedDefault,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DnsPortError {
    #[error("this backend cannot address the recorded DNS target: {0:?}")]
    TargetNotAddressable(DnsTarget),
    #[error(transparent)]
    Io(#[from] anyhow::Error),
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait MacosDnsPort: Send + Sync + 'static {
    /// 读「当前默认目标」及其 DNS。返回的 DnsTarget 供守卫记录。
    async fn read_default(&self) -> Result<(DnsTarget, Option<Vec<IpAddr>>), DnsPortError>;
    /// 读指定目标的 DNS（回读校验用）。
    async fn read(&self, target: &DnsTarget) -> Result<Option<Vec<IpAddr>>, DnsPortError>;
    /// 写指定目标。**无法寻址该目标时返回 TargetNotAddressable，绝不静默忽略。**
    async fn write(&self, target: &DnsTarget, dns: Option<Vec<IpAddr>>) -> Result<(), DnsPortError>;
}

pub(crate) struct LocalMacosDns;                  // 直调 networksetup（§4.6）
pub(crate) struct ServiceMacosDns { client: .. }; // IPC set_dns（F21）
```

**两个适配器的能力如实写在行为里，不写在签名里能写的部分则写在签名里：**

|                                    | `LocalMacosDns`             | `ServiceMacosDns`                                      |
| ---------------------------------- | --------------------------- | ------------------------------------------------------ |
| `read_default` 产出                | `Device(<hardware port>)`   | **恒** `ServerResolvedDefault`                         |
| `write(Device(a), ..)`             | 写 `a`                      | **`Err(TargetNotAddressable)`** ——大声拒绝，不静默忽略 |
| `write(ServerResolvedDefault, ..)` | `Err(TargetNotAddressable)` | 发 IPC，daemon 自解析                                  |

> **为什么选 enum 形而不是 per-backend trait**：守卫要在**跨后端切换**时判断「旧目标新后端还寻不寻得到」（§4.5 的中止判据），这需要一个**跨后端可比较的目标类型**。两个互不相干的 trait 表达不出这个比较。
>
> **「不接一个会被悄悄忽略的参数」怎么落实的**：`write` 接的是端口自己产出的 `DnsTarget`，而不是一个字符串设备名。Service 适配器拿到 `Device(_)` 时**返回错误**——拒绝是可观测、可测试的（T-DNS-14），静默忽略不是。

**已知残留（裁决 §5 要求点名 owner 与移除条件）：**

> **Service 模式下，若默认网络在「开启覆写」与「恢复」之间发生变化，恢复会写到新接口，原接口的覆写留着。**
> **保证只到这里**：Service 模式下快照与恢复都由服务端解析「当前默认设备」，**只要默认设备不变就自洽**。
> **这是既有限制，不是 5d 引入的**——今天 `core/clash/core.rs:109-118` 走的就是同一条只带地址的 IPC。**5d 也不修它**（`ServerResolvedDefault` 让它变得**可见且有名字**，仅此而已）。
> **owner**：本计划 §10 的残留清单。**移除条件**：`NetworkSetDnsReq` 增加可选设备字段且 daemon 遵从（上游 `nyanpasu-runtime` PR）。
> **为什么不在 5d 里扩 IPC**：那会是叠在仍未合并的 R0 之上的**第二个上游 PR**，而它关掉的窗口很窄。列为后续候选，不是 5d 的阻塞项。

**fake 必须按序记录** enable / restore / 与后端动作的相对次序，供测试断言**顺序**而非终态。

**不违反 D3 的「非 macOS 不加空抽象」**：整个文件在 `#[cfg(target_os = "macos")]` 下，**非 macOS 平台上这些类型根本不存在**。

### 4.2 状态与消息（**七个问题逐条回答**）

```rust
// CoreActorState 新增（core/actor/mod.rs:52-69）
#[cfg(target_os = "macos")]
pub(crate) dns: Option<DnsOverride>,
#[cfg(target_os = "macos")]
pub(crate) dns_ports: DnsPorts,

pub(crate) struct DnsPorts {
    local:   Arc<dyn MacosDnsPort>,
    service: Arc<dyn MacosDnsPort>,
}

pub(crate) struct DnsOverride {
    /// 快照时端口产出的目标。
    target: DnsTarget,
    /// 覆写**之前**的原始 DNS。`None` 是合法值（原本就没配）。
    previous: Option<Vec<IpAddr>>,
    /// 建立覆写时所用的后端身份。恢复按它选适配器，**不按 state.mode**。
    backend: RunType,
    /// 写成功但回读未通过 → true。守卫**保持 active**，操作报降级。
    unverified: bool,
}

// CoreActorMessage 新增
SetTunDns {
    operation: OperationId,
    /// Some(ip) = 开 TUN，把 DNS 指到该地址；None = 关 TUN，恢复原值。
    /// TUN 设备 IP 由 client 侧从 clash config 算好传入——**actor 不读任何配置全局**。
    desired: Option<IpAddr>,
    reply: RpcReplyPort<Result<DnsOutcome, CoreActorError>>,
}

pub(crate) enum DnsOutcome { Applied, AppliedUnverified, NoChange, Restored, RestoredUnverified }

// CoreActorError 新增（core/actor/types.rs:68-79）
#[error("core is not running; refusing to install a DNS override")]
CoreNotRunning,
#[error("DNS restoration failed; refusing to replace the backend")]
DnsRestoreFailed,
```

**注入路径**：`ClientSetupArgs`（新 `#[cfg(target_os="macos")] dns_ports`）→ `CoreClientArgs`（`client/core.rs:39-43`）→ `CoreClient::spawn`（`:105-119`）→ `CoreActorArgs`（`core/actor/mod.rs:37-50`）→ `CoreActorState`。**与 `requests` / `degradation` 走同一条既有路径，不新开机制。**

**为什么不扩 `CoreRequest`**：它是 run/check/apply 三条路共用的**全平台**进程描述；塞 macOS-only 的 TUN 字段会污染两条无关路径。

**裁决 §7 的七问逐条落位：**

| #   | 问题                                                           | 机制                                                                                                                                                                                                                                                                             |
| --- | -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 写成功但回读失败——DNS 可能**已经变了**                         | 保留 `Some(DnsOverride{ unverified: true })`，返回 `AppliedUnverified`，degradation `macos_dns_readback_failed`。**守卫不清**                                                                                                                                                    |
| 2   | 守卫只在**验证过的恢复**之后才清                               | `state.dns = None` 只出现在**一条**分支上：`read(target)` 返回 `Ok(v)` 且 `v` 与 `previous` **集合相等**。其余分支一律保留 `Some(.. unverified: true)`                                                                                                                           |
| 3   | 重复 `SetTunDns(Some(..))` 必须保住**最初**的原值              | 处理器开头：`if state.dns.is_some() { /* 不重新快照 */ }`——只写新 TUN 地址，`previous` 原封不动                                                                                                                                                                                  |
| 4   | 设备变了要先把**旧设备**恢复再取新快照                         | 每次 `SetTunDns(Some)` 先 `read_default()`，把返回的 target 与 `override.target` 比。不等 → 先 `write(&override.target, previous)` 恢复旧目标（Local 能寻址），再对新 target 取快照。**Service 侧 `ServerResolvedDefault` 恒等，该分支永不触发——这正是 §4.1 那条残留限制的形状** |
| 5   | 恢复失败发生在 `SetBackend` **之前**：中止还是保留旧适配器身份 | **中止**。见 §4.5                                                                                                                                                                                                                                                                |
| 6   | `SetBackend` 成功后若仍需要 TUN，DNS 要**重新施加**            | 见 §4.5。**时点是 `Run` 之后，不是 `SetBackend` 之后**（F43）                                                                                                                                                                                                                    |
| 7   | 「原值是 `None` 的活跃覆写」要与「没有覆写」可区分             | 外层 `Option<DnsOverride>` 表达「有没有覆写」，内层 `previous: Option<Vec<IpAddr>>` 表达「原值是不是 None」。**两层 Option，结构上不可混淆**                                                                                                                                     |

### 4.3 `SetTunDns` 的准入 —— **两条规则，两个构造**（裁决 §1）

> **更正：v2 §5 #4 说「一个 `CoreOperationGuard` 横跨拆除与外部控制动作，就能挡住另一条 `SetTunDns`」。这是假的。** `OperationGate`（`gate.rs:20-30`、`:32-45`）只做两件事：FIFO 发放 operation id、`is_active` 校验。**它不是 actor 级互斥锁**——守卫在外面被持有的同时，actor 照常处理别的消息。

**裁定：`SetTunDns` 携带 `OperationId`，与 `Stop`/`Run`/`SetBackend` 同形，由 `CoreActorState::validate_operation`（`core/actor/mod.rs:185-190`）校验。**

**为什么是「携带 id」而不是「自己取门」**（裁决要求选一个并说理）：`OperationGate::acquire` 在门被占时把请求塞进 `waiters`（`gate.rs:25-28`），**只有另一条 `ReleaseOperation` 消息被处理时才会发放**（`mod.rs:436` 与 `gate.rs:73-83`）。ractor 逐条串行处理消息，所以在 `handle()` 里 `await` 一个发放**永远等不到**——自取门是**构造性死锁**。携带 id 是唯一不死锁的形态，也和现有六条守卫消息一致。

**两条准入规则（裁决点名要的两个 `Stop` 竞态）：**

| #   | 场景                                                               | 规则                                                            | **强制它的构造**                                                                                                                                                                                                                           |
| --- | ------------------------------------------------------------------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A   | `Shutdown` 已开始后到达的 `SetTunDns`                              | `Err(ShuttingDown)`                                             | `Shutdown` 处理器 `mod.rs:604` 调 `state.operation.shutdown()`，`gate.rs:55-60` 把 `active` 置 `None` → 此后**任何** id 都 `StaleOperation`；且 `:606` `state.backend.take()` 使 `state.backend()` 返回 `ShuttingDown`（`mod.rs:175-183`） |
| B1  | `SetTunDns` **先**拿到许可，`Stop` 排在后面                        | `Stop` 的恢复在其处理器内发生，晚于 DNS 设置                    | `OperationGate` FIFO（`gate.rs:73-83`，已有 5 条单测 `gate.rs:121-207`）                                                                                                                                                                   |
| B2  | `Stop` **先**拿到许可，晚到的 `SetTunDns(Some)` 持**自己的新守卫** | **`Err(CoreNotRunning)`** ——FIFO 本身挡不住它，它的 id 是合法的 | **新增准入检查 `state.running.is_some()`**（F44：`Run` 置 `Some` `mod.rs:514`，`Stop` 置 `None` `:532`）。**仅对 `desired = Some(..)` 生效**——`desired = None`（拆除）在核已停时仍必须被允许，那正是恢复路径                               |

> **B2 是 v2 完全没有的那条。** 只靠 FIFO，晚到的 `SetTunDns` 会在 `Stop` 之后老老实实执行，**重新建立一个背后没有核的覆写**。`validate_operation` 也拦不住它——它的守卫是新的、合法的。**必须是第二个构造。**

### 4.4 顺序：控制动作前先拆 DNS —— **一条规则，六个入口，无例外**

> v2 只覆盖 stop 与 uninstall。`restart_service` 同样会把 daemon 拉下来，`update_service` 可能替换/重启它；F22 本身就指出 restart 路径今天根本不碰 DNS。

**规则：六个服务控制入口，在调用外部控制动作之前，都先在同一守卫内 `await` 拆除 DNS 覆写。**

```text
guard  →  拆 DNS（await，IPC 尚在）  →  service_control.<action>()  →  [update: 有界等待]  →  reconcile_with(&guard)
```

**为什么是「六个」而不是裁决点名的「四个」（stop / uninstall / restart / update）**：要把 install 排除在外，就得证明 `nyanpasu-service install` 在已有 daemon 在跑时不会把它换掉/重启——**我核不了这一点，而它是「不会去做某事」型断言，从名字推不出来**。把规则铺到六个的代价是：在没有活跃覆写时多走一次 `state.dns.is_none()` 的判断（无副作用的 no-op）。**用一次 no-op 换掉一条无法验证的前提，划算。因此本规则没有需要点名的例外。**

**拆除失败时的分岔：**

| 场景                                          | 处置                                        | 理由                                                                                                                                      |
| --------------------------------------------- | ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **uninstall**                                 | **中止卸载**，返回 `Err` + **用户可见错误** | 卸载**不可逆**，而拆 DNS 失败说明我们**当下正处在一个连自己的写都验证不了的状态**——在这种状态下执行不可逆操作，是把已知的不确定性固化下来 |
| **stop / restart / update / install / start** | **继续，产出 degradation**                  | 服务可再启动、通道会回来，泄漏可恢复；为拆 DNS 失败就让用户停不掉服务代价不成比例，还可能把人锁死                                         |

**判别原则：失败会让泄漏变成永久的 → 中止；泄漏仍可恢复 → 继续并降级。**

**中止 uninstall 的用户可见错误必须说清三件事**：**做了什么**（没有卸载）、**为什么**（DNS 覆写未能拆除，继续卸载会永久残留）、**怎么办**（重试；或先手动关闭 TUN 再卸载）。**只返回 `Err` 不够**——用户可见的失败，**措辞本身就是功能的一部分**。

### 4.5 `SetBackend`：先用旧适配器恢复，成功后在 `Run` 之后重新施加

**恢复用哪个适配器**：`DnsOverride.backend`（记录值），**不是** `state.mode`。理由是双保险：①`replace_backend` 在 `mod.rs:282` 才改 `self.mode`，而恢复发生在调用 `replace_backend` **之前**，所以此刻 `state.mode` 仍是旧值；②即便将来有人调换了顺序，按记录值选仍然对。测试打掉第②层（T-DNS-16 断言选的是记录值）。

**恢复失败 → 中止切换**（裁决 §7 第 5 问的二选一）：`SetBackend` 返回 `Err(DnsRestoreFailed)`，**不调 `replace_backend`**，`state.dns` 保留（`unverified: true`）。

> **为什么中止而不是「守卫保留旧适配器身份」**：Local→Service 时记录的目标是 `Device(a)`，而 `ServiceMacosDns::write(Device(a), ..)` 只能 `Err(TargetNotAddressable)`（§4.1）——**换过去之后就再也恢复不了了，泄漏变永久**。这正是 §4.4 的判别原则。反方向（Service→Local）虽然还有救，但**统一中止**省掉一条按方向分叉的规则，而分叉规则正是这类设计出错的地方。

**成功切换后的重新施加，时点由 F43 钉死：**

```text
guard
 ├─ SetTunDns(None)          ← 用 override.backend 选的旧适配器恢复；失败即中止
 ├─ SetBackend(mode)         ← replace_backend：running := None（:268），mode := new（:282）
 ├─ Run(request)             ← running := Some（:514）        ★ 核在这里才重新起来
 └─ SetTunDns(Some(tun_ip))  ← 若 TUN 仍然要开：用**新**适配器重新施加
```

**强制这个时点的构造，就是 §4.3 的准入规则 B2**：在 `SetBackend` 与 `Run` 之间 `state.running` 是 `None`，此刻发出的 `SetTunDns(Some)` 会被 `CoreNotRunning` 拒掉。**顺序不是靠自觉，是靠一条会报错的准入检查。**

`desired`（TUN 是否开、TUN 设备 IP）由 `CoreModeReconciler` 算好传入：它需要新增一个 `clash_config: ClashConfigClient` 字段（`NyanpasuClientInner.clash_config`，`client/mod.rs:247`，注入方式同 §3.1），TUN 开关读 `application`（已有字段）。**actor 侧一行配置全局都不读。**

### 4.6 Local 适配器：**在我们自己的 crate 里直调 `networksetup`**（裁决 §8）

**不复用上游 `nyanpasu_utils::network::macos::{set_dns, get_dns}`。** 三条已核实的理由（F50 / F41 / F46）：

1. **设备名被文本拼进 bash 脚本**（F50）——设备来源一旦不是硬编码字面量，就是命令注入面；
2. **macOS 硬件端口名常含空格**（`USB 10/100/1000 LAN`），脚本里 `$1` 未加引号会被词法分割——**这条今天就是坏的**，只是 `Wi-Fi` 这种单词端口把它盖住了；
3. **读路径不看退出码**（F41），非零退出 + 空 stdout 塌缩成 `Ok(None)`。

```rust
// LocalMacosDns::write 的形状
Command::new("networksetup")
    .env("LC_ALL", "C")                 // 见 §7.3：文案匹配要求可复现的 locale
    .arg("-setdnsservers")
    .arg(device)                        // 直接 argv，不经 shell
    .args(dns_args)                     // 多个服务器 = 多个 argv；无 DNS 时是单个 "Empty"
    .kill_on_drop(true)
    .output().await?;
// 然后**检查 output.status**
```

> **注意 `$2` 是「本该」被词法分割的**（多个 DNS 服务器要变成多个参数）——所以「两边都加引号」是错的修法，**直接 argv 才是对的**。

**这条比复用上游更短**：省掉临时目录、临时文件、`include_str!` 替换、`osascript` 包一层；同时消灭注入面、修好空格、并且**白拿到四态读所需的退出码**。**无需上游改动。**

**权限（裁决点名要求确立而非假设，见 F53/F54）：**

- `networksetup` 的写操作**至少需要 admin 组身份**；系统若开启「Require an administrator password to access system-wide preferences」，则需要 root。
- **`osascript` 那一跳不提权**（F46/F54），所以去掉它**不改变**权限语义——新旧两种形态在非 admin 账户上都会失败。
- **写被拒绝时的行为**：`output.status` 非零 → `Err(DnsPortError::Io(..))` → 走 §4.4/§4.5 的失败分岔（拆除失败：uninstall 中止、其余降级；施加失败：`SetTunDns` 返回 `Err`，**不建立守卫**）。**关键是它第一次变得可见**——见 §10 那条风险。
- **设计对「到底要不要 root」这个问题不敏感——这是有意的。** F53 已确立「至少要 admin 组」，但**具体账户是否在 admin 组、系统是否开了那个安全选项，是运行期事实，规划期确立不了**。两种情形下本设计的行为**完全相同**：
  - **不需要提权（账户是 admin、选项关闭）** → 写成功 → 回读校验通过 → 守卫建立。
  - **需要而没有（账户非 admin，或选项开启）** → `output.status` 非零 → `Err` → 上面那条失败分岔 → **degradation 里带上 stderr**。
- **明确不做「权限预检」**：预检会引入第二个真相来源，它可以和真实写入结果不一致（预检说行、写仍然失败，或反之）。**唯一判据是那次真实写入的退出码。** 这也是 §4.8 四态①存在的理由。

### 4.7 写回读回校验

**(a) 必须是语义比较**：解析成 `IpAddr` 后比较**集合**（忽略顺序与重复），解析失败即视为不一致。**做不到语义比较就不要做这个校验**——文本比较会产生**假失败**，那比不校验更糟：会把成功的操作报成失败，然后有人为了让它绿而删掉校验。

**(b) 失败进 degradation，不静默**；且测试**必须走真实适配器的校验路径**——测的是「**回读比对真的会发现不一致**」，不是「适配器会传播 `Err`」。

**(c) 回读失败 ≠ 写失败**：写返回 `Ok` 但回读不通过时，DNS **可能已经变了**，所以守卫**保持 active**、`unverified = true`（§4.2 第 1 问）。

**TOCTOU 不在范围**：本校验的语义是「**我们的写有没有生效**」，并发的外部变更不属于它要回答的问题。

### 4.8 读实现必须分四态

> **「没有配置 DNS」必须被正向识别，不能从「输出空 / 解析不了」推断。** `networksetup -getdnsservers` 在无 DNS 时输出的是**一句可识别的文字**，不是空串——**正是「把不可解析当成 `None`」这一步制造了原来的 bug**（F41）。

| #   | 条件                          | 结果                       |
| --- | ----------------------------- | -------------------------- |
| 1   | 退出码非零                    | **`Err`**（读失败）        |
| 2   | 输出匹配「无 DNS 服务器」那句 | `Ok(None)`                 |
| 3   | 输出解析出 IP 列表            | `Ok(Some(..))`             |
| 4   | **以上都不是**                | **`Err`**，**不是 `None`** |

**第 4 条是关键：不认识的输出是错误，不是「没有」。** 四态各配一条断言（T-DNS-08…11）。

**第 2 条的字面串在规划期未能确立（F55）——处置见 §7.3，不许拍。**

---

## 5. 五条已知问题的最终去向

| #   | 问题                                   | v3 去向                                                                                      |
| --- | -------------------------------------- | -------------------------------------------------------------------------------------------- |
| #2  | C2 不可线性化                          | §3.5（三个签名、无 `IpcState` 参数、`rg` 门禁钉住探针位置）+ §3.4（有界等待）                |
| #3  | `health_check` 的警告职责没人接手      | §3.3（**两个**接手方：不兼容警告 + 探针失败可观测性）                                        |
| #4  | 拆 DNS 的守卫跨度与 `reconcile()` 死锁 | §3.5（`reconcile_with` 避免递归取守卫）+ **§4.3（守卫本身挡不住并发消息，靠准入规则 A/B2）** |
| #5  | 读路径不检查退出码                     | §4.6（自己实现）+ §4.8（四态）                                                               |
| #6  | 适配器接线不全                         | **§4.1 / §4.2 全部写完，v2 的「待设计」已消除**                                              |

**#5 的两个被否方案**（保留原裁定）：

| 方案                               | 为什么否                                                                                                                        |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| (b) 只在 `expected != None` 时校验 | **`None` 正是关 TUN 的主路径，也正是泄漏场景本身。** 一个「在最需要它的地方主动关闭」的校验**比没有校验更糟**——它给出覆盖的假象 |
| (c) 哨兵区分                       | 多一次写入、多一个中间态，且**哨兵自身的写入同样不可验证**——把问题往后推一格                                                    |

---

## 6. 定序保证表（**每条「X 之后 Y 一定已发生」都点名机制**）

> **本表不允许带着「提案中」进实施。** 新增行同样必须在定稿前变成「已裁定」或被删掉。

| 断言                                                               | **靠什么构造保证**                                                                                                        | 锚点                                                                   | 测试                   |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------- |
| 两次控制动作的模式结论不交错                                       | `OperationGate` FIFO + **三步全在同一守卫内**                                                                             | `gate.rs:73-83`                                                        | T-MODE-03              |
| **控制动作 → probe → reconcile 三步不可拆**                        | `reconcile_with(&guard)` **没有 `IpcState` 参数**，调用方无法喂陈旧结果；`rg` 门禁钉死 `.probe()` 只有三处                | §3.5                                                                   | T-MODE-03              |
| **bootstrap 的守卫外探针是安全的**                                 | 它执行于 `CoreClient::new` **之前**，actor 尚不存在 → 无并发对象、无守卫可取                                              | `client/mod.rs:303` vs `:312`（同一 `async move` 块，源码顺序）        | T-PROBE-02             |
| 恢复发生在后端动作与 reply 之前                                    | actor 处理器内的 `await` 点（源码顺序）                                                                                   | §2.2                                                                   | T-DNS-02/03            |
| 拆 DNS 发生在**六个**服务控制动作之前                              | 同一守卫内的调用点顺序                                                                                                    | §4.4                                                                   | T-DNS-05/06/17/18      |
| **`Shutdown` 后不再有 `SetTunDns` 生效**                           | `state.operation.shutdown()` 清 `active` → `validate_operation` 恒 `StaleOperation`；且 `backend.take()` → `ShuttingDown` | `mod.rs:604,606`；`gate.rs:55-60`                                      | T-DNS-07               |
| **`Stop` 先拿到许可时，晚到的 `SetTunDns(Some)` 失败**             | **准入检查 `state.running.is_some()`**（FIFO 挡不住它）                                                                   | `mod.rs:532`（`Stop` 置 `None`）                                       | **T-DNS-13**           |
| **`SetTunDns` 先拿到许可时，`Stop` 在其后恢复**                    | `OperationGate` FIFO                                                                                                      | `gate.rs:73-83`                                                        | **T-DNS-12**           |
| **写 → 回读校验**                                                  | 处理器内 `write().await` 之后**紧跟** `read().await` 的源码顺序；回读不过 → `unverified = true`、守卫不清                 | §4.7                                                                   | T-DNS-04、**T-DNS-19** |
| **`SetBackend` 前用「旧」适配器恢复**                              | 恢复调用位于 `state.replace_backend(mode)` **之前**，且按 `DnsOverride.backend` 选适配器（不按 `state.mode`）             | `mod.rs:282`（`mode` 在 `replace_backend` 内才改）                     | **T-DNS-16**           |
| **切换成功后若仍需 TUN，在 `Run` 之后重新施加**                    | 准入规则 B2：`SetBackend` 与 `Run` 之间 `state.running` 是 `None`，此时的 `SetTunDns(Some)` 会被 `CoreNotRunning` 拒      | `mod.rs:268`（`replace_backend` 置 `None`）、`:514`（`Run` 置 `Some`） | **T-DNS-15**           |
| **update 之后：要么观察到兼容的 Service，要么以 Local 收敛并降级** | 有界等待 + `tokio::time::timeout` 的**丢弃即取消**；超时分支 `force_local_with(&guard)`                                   | §3.4                                                                   | T-MODE-04/05           |
| 任何时刻只有一个模式生产者                                         | S-a 同步停掉轮询派发                                                                                                      | §3.7                                                                   | `rg` 判据（§11）       |

> **上一行在 v2 里写的是「update 之后模式反映的是 v2 daemon」——那在超时分支上是假的**（裁决 §9 点名）。已改成上表的措辞。

---

## 7. 测试矩阵

> **第三列是断言**：删掉那行生产代码，这条测试真的会红吗？**填不出第三列的测试不进矩阵。**

### 7.1 C2

| ID             | 断言                                                                                                                            | **删掉哪行会让它红**                                                            |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| T-PROBE-01     | 兼容门 fail-closed：daemon 在跑但不放行 → 探针返回 `Disconnected`                                                               | 探针里调 `target_ipc_state()` 的那行                                            |
| T-PROBE-02     | **bootstrap 用探针真值而非 `Disconnected` 默认**（修 F35）                                                                      | `client/mod.rs:303` 的 `probe().await` 调用行                                   |
| T-PROBE-03     | 不兼容时**发出警告**（smoke 2 要求）                                                                                            | `reconcile_with` 内 `classify` 之前的 `tracing::warn!` 行                       |
| **T-PROBE-04** | **探针失败时发出 degradation `service_probe_failed`**，而不是静默变 Local                                                       | `reconcile_with` 内处理 `Option<anyhow::Error>` 的 `degradation.publish(..)` 行 |
| T-MODE-01      | 关闭 `enable_service_mode` → 得 `Normal` 并 `set_backend`                                                                       | `request.rs:82-85` 删掉提前返回、送真值进 `classify` 那行                       |
| T-MODE-02      | **六个**控制动作后各探测一次——**逐条独立断言，不合并**                                                                          | 各自 facade 方法里的 `reconcile_with(&guard)` 行                                |
| T-MODE-03      | **#2 的竞态**：start→stop 序列下终态为 `Normal`，晚到的 probe 不翻转                                                            | `reconcile_with` 的 `guard` 参数（去掉守卫跨度即红）                            |
| **T-MODE-04**  | **有界等待成功路径**：脚本化探针在第 N 次返回兼容 → 得 `Service`，无降级                                                        | `await_service_ready` 的循环体                                                  |
| **T-MODE-05**  | **永不返回的探针**：在 `READY_BUDGET` 的小倍数内返回，且结果是 **Local + `service_update_not_ready` 降级**，控制动作本身仍 `Ok` | `tokio::time::timeout(..)` 那行（改成裸 `.await` 即挂死超时）                   |

### 7.2 C3 —— 生命周期与并发

| ID           | 断言                                                                                                                                              | **删掉哪行会让它红**                                                                     |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| T-DNS-01     | `SetTunDns{Some}` → 适配器 `write` 被调，guard 记为 active                                                                                        | 处理器里 `port.write()` 那行                                                             |
| T-DNS-02     | **顺序**：`Stop` 时恢复在 `backend.stop()` **之前**                                                                                               | `Stop` 臂里 `restore().await` 早于 `backend.stop()` 那行（对调即红）                     |
| T-DNS-03     | **顺序**：`Shutdown` 时恢复在后端动作与 **reply** 之前                                                                                            | `Shutdown` 臂的 `restore().await` 行                                                     |
| T-DNS-04     | **回读比对真的会发现不一致**（走真实适配器的比较逻辑）                                                                                            | 适配器里集合比较那行                                                                     |
| T-DNS-05     | Service **stop**：拆 DNS 在 `stop()` 之前                                                                                                         | facade `stop_service` 里的拆 DNS 行                                                      |
| T-DNS-06     | Service **uninstall**：同上顺序 + **失败时中止卸载**                                                                                              | facade `uninstall_service` 里的拆 DNS 行与中止分支                                       |
| T-DNS-07     | `Shutdown` 后到达的 `SetTunDns` → `Err(ShuttingDown)` 而非静默丢弃                                                                                | `Shutdown` 臂的 `state.operation.shutdown()` 行                                          |
| **T-DNS-12** | **`SetTunDns` 先取得许可** → `Stop` 在其之后恢复                                                                                                  | `Stop` 臂的 `restore().await` 行                                                         |
| **T-DNS-13** | **`Stop` 先取得许可** → 后到的、持新守卫的 `SetTunDns(Some)` → `Err(CoreNotRunning)`                                                              | **`state.running.is_some()` 准入检查那行**（删掉即变成「成功建立一个背后没有核的覆写」） |
| **T-DNS-14** | `ServiceMacosDns::write(Device(_), ..)` → `Err(TargetNotAddressable)`，**不是静默成功**                                                           | `ServiceMacosDns::write` 里的 target 匹配分支                                            |
| **T-DNS-15** | `SetBackend` 成功且 TUN 仍开 → **`Run` 之后**用**新**适配器重新施加；且 `SetBackend` 与 `Run` 之间发出的 `SetTunDns(Some)` 被 `CoreNotRunning` 拒 | `replace_backend` 的 `self.running = None`（`mod.rs:268`）+ 准入检查行                   |
| **T-DNS-16** | `SetBackend` 的恢复用 **`DnsOverride.backend` 记录的**适配器，不是 `state.mode`                                                                   | 适配器选择处读 `override.backend` 那行（改成读 `state.mode` 即红）                       |
| **T-DNS-19** | **写成功 + 回读失败** → 守卫**仍 active**、`unverified = true`、返回 `AppliedUnverified` + 降级                                                   | 回读失败分支里「不清守卫」那段（改成 `state.dns = None` 即红）                           |
| **T-DNS-20** | **恢复的回读校验失败** → 守卫**不清**                                                                                                             | `state.dns = None` 前的校验条件那行                                                      |
| **T-DNS-21** | 重复 `SetTunDns(Some(a))` → `SetTunDns(Some(b))` → 恢复得到**最初**的原值，不是 `a`                                                               | 「已有覆写则不重新快照」那行                                                             |
| **T-DNS-22** | 原值为 `None` 的活跃覆写，与「无覆写」可区分：前者 `Stop` 时**会**调一次 `write(.., None)`，后者不调                                              | 两层 `Option` 的外层判断行                                                               |
| **T-DNS-23** | 设备变更：`Device(a)` 活跃时 `read_default()` 返回 `Device(b)` → 先 `write(Device(a), previous)` 再对 `b` 取快照                                  | target 比较那行                                                                          |
| **T-DNS-24** | `SetBackend` 前恢复失败 → **不调 `replace_backend`**，返回 `Err(DnsRestoreFailed)`，`state.dns` 保留                                              | 中止分支的 `return` 行                                                                   |
| **T-DNS-17** | **restart**：拆 DNS 在 `restart()` 之前                                                                                                           | facade `restart_service` 里的拆 DNS 行                                                   |
| **T-DNS-18** | **update**：拆 DNS 在 `update()` 之前                                                                                                             | facade `update_service` 里的拆 DNS 行                                                    |
| **T-DNS-25** | `Drop` 时若守卫仍 active → **记 `tracing::error!` 且不发起任何恢复**（断言适配器**零调用**）                                                      | `Drop` 里的 `tracing::error!` 行；反向断言钉住「不恢复」                                 |

> **T-DNS-05/06/17/18 不合并。** 它们是四个独立调用点，合并测则删掉其中一处另一处仍绿。

### 7.3 C3 —— 四态读（**裁决 §9 点名要修的两条**）

| ID       | 断言                                                                                                              | **删掉哪行会让它红**                        |
| -------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| T-DNS-08 | 四态①：**退出码非零 → `Err`**。**fixture 必须是「非零退出码 **+** 可解析的 IP 输出」**（例如 `8.8.8.8\n1.1.1.1`） | 读实现里 `if !output.status.success()` 那行 |
| T-DNS-09 | 四态②：**「无 DNS 服务器」那句 → `Ok(None)`**——正向识别，不靠空串推断                                             | 匹配该文案那行                              |
| T-DNS-10 | 四态③：正常输出 → `Ok(Some(..))`                                                                                  | 解析 IP 列表那行                            |
| T-DNS-11 | 四态④：**不认识的输出 → `Err`，不是 `None`**——**这条最关键**，它正是原 bug 的形状                                 | 兜底分支那行（改成返回 `None` 即红）        |

**T-DNS-08 为什么这么选 fixture**：v2 的版本可能**空转**——若非零退出的 fixture 带的是空/垃圾 stdout，删掉退出码检查之后仍会落进四态④的 `Err` 分支，测试照绿。换成「非零退出码 + 可解析 IP」之后，删掉退出码检查会让它返回 `Ok(Some(..))`，**由绿变错**。

**T-DNS-09 的前置条件（F55：字面串在规划期未能确立）：**

1. **禁止**在生产与测试里共用一个臆造的字面常量——那样测试只证明「我们和自己一致」。
2. fixture 必须是**真机捕获物**，提交到 `backend/tauri/src/core/actor/fixtures/macos-getdnsservers-none.txt`，并在同目录记录 provenance：**macOS 版本、`LC_ALL=C`、完整命令行**。
3. **捕获必须用新的调用形态**（直调 `networksetup`），**不能用上游脚本**——上游 `echo $RES` 会把换行压成空格（F52），那是另一个生产者的输出。
4. 生产侧的匹配器**独立于** fixture 文件（不用 `include_str!` 同一文件），测试读 fixture、生产读自己的匹配器；匹配器与现实一旦漂移，测试就红。
5. `LC_ALL=C` **在生产代码里也要设**（§4.6），否则 fixture（在 `C` locale 下捕获）与生产（在用户 locale 下运行）不对应。
6. **若实施期拿不到真机**：**T-DNS-09 直接删除，不许用臆造串顶替**；同时把「四态②未被测试覆盖」写进 §11 的 Exit 判据与 PR 描述。四态①③④不受影响。

### 7.4 回归契约

区分**存活测试被迫修改**（不允许，停下核查）与**被删模块自带单测随属主消失**（预期）。

**已知必改：**

- `client/core.rs:1207-1214` `initial_watch_snapshot_matches_legacy_empty_status` → 断言注入 mode **并改名**为 `initial_watch_snapshot_reflects_the_injected_mode`（§2.1）。
- `core/service/ipc.rs:140-187` 的两条 `target_ipc_state` 单测：`target_ipc_state` 本身不动，**测试随文件重整迁到 `core/service/probe.rs`**，断言不变。

---

## 8. 契约归属

> **判别口诀**：签名能保证的只有**「值到得了这里」**（及其对偶「值到不了这里」）与**「类型在此平台不存在」**；凡「**不会去做某事**」一律靠测试 / 门禁 / `rg`，**且必须说得出怎么验**。

| 契约                                       | 由谁保证                  | 为什么可验证                                                                                                    |
| ------------------------------------------ | ------------------------- | --------------------------------------------------------------------------------------------------------------- |
| 非 macOS 不存在 DNS 抽象                   | **cfg / 类型**            | 非 macOS 上**引用它编译不过**——真正的类型级保证                                                                 |
| **调用方无法把陈旧探针结果喂给 reconcile** | **签名**                  | `reconcile` / `reconcile_with` **没有 `IpcState` 参数**——「这个值到不了这里」，是签名能给的那一类               |
| **Service 端口拿到设备目标时不会静默忽略** | **返回值 + 测试**         | `Err(TargetNotAddressable)` 是可观测返回值，T-DNS-14 钉住。**不是签名保证**——签名只能说「它接一个 `DnsTarget`」 |
| **任何探针都不在守卫外开始**               | **ledger / `rg` 门禁**    | `rg -n '\.probe\(\)'` 恒为三处且位置固定——**计数可数**。签名做不到                                              |
| **`force_local_with` 只在超时分支用**      | **`rg` 门禁**             | 恰好一处调用点                                                                                                  |
| DNS 路径选择不回头读全局                   | **ledger 门禁**           | `core/actor/dns.rs` 的 `Config::*()` / `::global()` 计数恒为 0                                                  |
| 顺序类契约                                 | **测试**                  | 控制流性质，类型系统表达不了 → T-DNS-02/03/05/06/12/15/16/17/18                                                 |
| **「核已停时不建立覆写」**                 | **运行时准入检查 + 测试** | `state.running.is_some()` 是可读字段，T-DNS-13 钉住                                                             |
| `get_ipc_state` / statics 归零             | **`rg` 判据**             | 删除类不变量                                                                                                    |

---

## 9. 门禁

1. **「diff 应为空」形态的判据，只要跑在中间提交之后，必须与基线比**：`git diff --exit-code <base>..HEAD -- <path>`；
2. **ledger 三步顺序**：report 核对 → `--write-snapshot` → gate 比对。**顺序本身是判据的一部分**；
3. **删模块要有「模块不存在」断言**，不能只查调用点归零（本阶段涉及 `core/service/mod.rs::init_service`、`core/service/ipc.rs` 的轮询部分）。

**bindings 预期（v2 写的是「待定」，v3 定死）：**

| 变更                                                          | wire 影响                                                                                                        |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `uninstall_service` 从 `ipc.rs:936-937` 直调改为走 facade     | **命令名与签名都不变**（`uninstall_service() -> Result`），`frontend/interface/src/ipc/bindings.ts:233` **不动** |
| `update_service` 调用点从 `utils/init/mod.rs:251` 迁到 facade | **不在命令面上**，无 wire 影响                                                                                   |
| `SetTunDns` / `MacosDnsPort` / `ServiceProbe`                 | **全部 `pub(crate)`，不出现在命令面**                                                                            |
| `set_mode` / `reconcile_mode`                                 | **不新增命令**——模式变更是六个既有服务控制命令的**内部**后果                                                     |

**结论：本 PR 的 bindings diff 恰好为空。** 判据：`git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`（与 CI 的 `ci.yml:306-308` 同形）。**这是一条可证伪的预期，不是「待定」。**

---

## 10. 风险与已知残留

| 风险                                       | 概率 | 影响                    | 缓解                                                                                                        |
| ------------------------------------------ | ---- | ----------------------- | ----------------------------------------------------------------------------------------------------------- |
| ~~锚点漂移~~                               | —    | —                       | **已在 §0 复核完毕并改写**，不再是待办                                                                      |
| **DNS 覆写在非管理员账户可能一直静默失效** | 中   | **误判为 5d 弄坏的**    | 见下（**必须预先写明**）                                                                                    |
| **F55：无-DNS 文案未确立**                 | 中   | T-DNS-09 无法非自证地写 | §7.3 的六条前置；拿不到真机则**删测并记账**，不拍字面串                                                     |
| `Drop` 不覆盖强杀 → DNS 残留               | 中   | 退出后全机解析受影响    | 如实写明；兜底属 PR-6                                                                                       |
| smoke 3 不可验证                           | 高   | Exit 判据不可满足       | D4 已裁：记为已知未验证风险，结论进 PR 描述与发布说明                                                       |
| **`READY_BUDGET` 实测不到**                | 中   | 有界等待的界没有依据    | 若实测条件不具备，**如实标注为选定值并写进 PR 描述**，不假装实测（§3.4 的表已把「实测项」与「选定项」分开） |

### 10.1 已知残留（**不由 5d 修，但要有名字、有 owner、有移除条件**）

| #   | 残留                                                                       | 性质                                                                                                                              | owner / 移除条件                                                                                                                                                                 |
| --- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | **Service 模式下默认网络在覆写期间变更 → 恢复写到新接口，原接口留残**      | **既有**（今天 `core/clash/core.rs:109-118` 就这样）。**5d 不引入、不修复**，只让它有了名字（`DnsTarget::ServerResolvedDefault`） | 本表。**移除条件**：`NetworkSetDnsReq` 增加可选设备字段且 daemon 遵从（上游 `nyanpasu-runtime` PR）。**不在 5d 做**：会成为叠在未合并 R0 之上的第二个上游 PR，而它关掉的窗口很窄 |
| R2  | **强杀（SIGKILL / 任务管理器）后 DNS 覆写残留**                            | **既有**（F23）                                                                                                                   | PR-6：启动时检测并清理残留覆写                                                                                                                                                   |
| R3  | **`update` 的有界等待超时后，daemon 可能稍后才就绪，而此时已收敛到 Local** | **5d 引入的取舍**（今天靠 5 s 轮询最终纠正）                                                                                      | 用户下次触发任一服务控制动作时会重新 probe 并纠正；degradation 文案里点明「当前以 Local 模式运行」。**不加后台重试**——那会把第二个模式生产者请回来（§3.7）                       |

> ### 关于「非管理员账户可能一直静默失效」
>
> **推理链**：`networksetup -setdnsservers` 至少需要 admin 组身份（**F53，已从 man page 与社区证据确立，不是从代码形状推断**）→ 但代码**不提权**（F40/F46/F54）→ 失败被 `let _ =` 吞掉（F22）→ **没有任何观测点**。所以「这个功能在非管理员账户上可能从来就没工作过」**不是推测，而是当前代码结构下必然无法被发现的一类失效**——不是「碰巧没人报」，是**报不出来**。
>
> **加上退出码检查与回读校验之后它会第一次变得可见。** 这**不是我们引入的回归，但我们会是发现它的人**。
>
> **判别方法**（供冒烟时立即区分）：**在 5d 之前的版本上用同一账户手动跑一次 `networksetup -setdnsservers`**，看是否需要授权——能立刻分辨「5d 弄坏的」还是「5d 让它第一次可见」。
>
> **这也是本阶段的一项真实收益**：C3 的价值不只是保序，还包括**让一条此前不可观测的路径变得可观测**。
>
> 与 5b 那条纪律方向相反但同源：那次是**别把既有缺陷算成我们引入的**，这次是**别把即将暴露的既有缺陷当成我们弄坏的**。

---

## 11. Exit 判据

| 要求                                                       | 验证                                                                                                                                |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| 显式模式收敛全部走守卫                                     | T-MODE-01/02/03；`rg -n '\.probe\(\)'` 恰好三处且位置为 §3.5 所列                                                                   |
| **`reconcile` 家族无 `IpcState` 参数**                     | 签名核对（编译期即拦）                                                                                                              |
| 删 `pending_run_type` 设计                                 | **no-op**（F9：不存在）                                                                                                             |
| 删轮询线程与 statics                                       | `rg 'IPC_STATE\|KILL_FLAG\|HEALTH_CHECK_RUNNING\|spawn_health_check\|get_ipc_state'` 为 0                                           |
| 删 `impl Default for RunType`                              | `rg 'RunType::default'` 为 0；`CoreStatusView::initial` **两个**调用点都传参（F42）                                                 |
| **六个服务控制入口签名一致且全在 `ServiceControlOps` 上**  | 结构核对；活着的约束只有 `task.md` C2 的「不迁入 CoreActor」，facade 调 controller 即满足（§2.5）；**扩到六个方法仍须写进 PR 描述** |
| install/update/uninstall 保持独立 controller，不迁入 actor | 结构核对；facade 调 controller **不违反**该约束                                                                                     |
| **六个入口都在控制动作前拆 DNS**                           | T-DNS-05/06/17/18 + install/start 两条同形断言                                                                                      |
| `MacosDnsGuard` 与 start/stop/backend-switch 保序          | T-DNS-02/03/12/15/16                                                                                                                |
| **核已停时不会建立新覆写**                                 | T-DNS-13                                                                                                                            |
| **有界等待能界住永不返回的探针**                           | T-MODE-05                                                                                                                           |
| Service backend 用 IPC `set_dns`                           | T-DNS 双适配器 parity + T-DNS-14                                                                                                    |
| 非 macOS 不加空抽象                                        | cfg 门控——非 macOS 上类型不存在                                                                                                     |
| **bindings diff 为空**                                     | `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`                                                                    |
| **四态读全覆盖**                                           | T-DNS-08/10/11 必过；**T-DNS-09 视 §7.3 的真机 fixture 而定——拿不到就删测并在 PR 描述里点名「四态②未覆盖」**                        |
| **smoke 2**（v1→v2 升级 + 拒绝升级 fail-closed Local）     | 本机可跑，**须真实服务环境**；**它是 C2 的真正验收点**——C2 迁移不完整会正好打断它**而 `rg` 门禁全绿**                               |
| **smoke 3**（macOS TUN/DNS）                               | **未在本地验证且不可由 CI 覆盖**（D4）；结论进 PR 描述与发布说明                                                                    |
| **R1/R2/R3 三条残留**逐条出现在 PR 描述里                  | 文本核对——**「不修」必须是被记录的决定，不是沉默**                                                                                  |
