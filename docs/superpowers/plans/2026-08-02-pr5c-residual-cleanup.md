# PR-5c 实施计划 — 状态/日志、运行模式、macOS DNS、residual 清理

**日期：** 2026-08-02
**版本：** v2.1（leader 裁定 D1–D3 = A、§7 两条分别处置；**用户裁定 D4 = 路径乙**，CI 前置已核实）
**分支基线：** `refactor/core-manager-actor` @ `899b069f5`（PR-5b 阶段门已关闭：实施 7 提交 + 修复 8 提交，467 passed / 1 ignored）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/task.md` 卡 C1–C4（`:115-160`）+ 文末最终删除清单
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.3
**平台：** Windows 11 / PowerShell

> **全部事实读自 `899b069f5` 的工作树**（5b 落地后）。凡是「卡上写了但代码里不存在」的项，一律照实记为 no-op，**不为了删它而先造出来**——这是 5b 的既定做法（B3 的 `ControllerBinding` 先例）。

> **与 R0 的关系：本计划不依赖 R0 上游 PR 的合并结果。** 所有 submodule 侧锚点（`error_kind` 常量、IPC `set_dns` 形状等）**一律按当前 pin `v2.0.0-rc.1` 的实际内容写**，不预设 R0 已合并。R0 若合并后改了 `error_kind` 常量表，受影响的是 **5b 计划 A.7 的 Service 侧判据**，届时随 pin 移动一并更新——**本阶段不动 submodule pin**。

**v2 修订索引（leader 裁定）：**

| 项   | 结论                                                                                                                                                                                             | 落点            |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------- |
| D1   | **裁定 A：只删不建**；并**显式记录卡面项的收窄理由**（原意、失效前提、将来要重解的两件事）                                                                                                       | §2 D1           |
| D2   | **裁定 A**：`initial(mode)` 加参、删 `impl Default for RunType`                                                                                                                                  | §2 D2、A.1、A.2 |
| D3   | **裁定 A**：RAII 挂 actor state；**明写不覆盖强杀**，兜底去向 PR-6                                                                                                                               | §2 D3、A.3、§10 |
| D4   | **用户裁定路径乙**；CI 前置已核实并**分层作答**：有 macOS runner 且跑 `cargo test`（F33），但**无作业能跑 smoke 3**（F34）→ 按「若没有」分支降级为已知未验证风险，**分开写 CI 覆盖面与未覆盖面** | §2 D4、F33/F34  |
| R0   | 声明**本计划不依赖 R0 合并**；submodule 侧锚点按当前 pin `v2.0.0-rc.1` 写                                                                                                                        | 抬头            |
| §7 ① | `install_service` 不 reconcile 是**有意**——加注释，不改行为                                                                                                                                      | §7 ①、S7        |
| §7 ② | `uninstall_service` 绕 facade 是**缺陷**——S7 改走 facade + reconcile；**不违反 C2**                                                                                                              | §7 ②、S7        |
| 事实 | F6 精确化（`dist/` 命中是构建产物）；新增 F32（ledger bug **不跨文件**，修复范围限于一处）                                                                                                       | F6、F32         |

---

## 0. 本阶段的边界

**做（= task.md C1–C4）：**

1. **C1**：backend status/events 投影核对 + 日志面收敛 + 删 legacy `Logger` global；
2. **C2**：显式 `set_mode` / `reconcile_mode` 走 `CoreOperationGuard`、删 5 s 轮询线程与 service statics；install/update/uninstall 保持独立 controller；
3. **C3**：`MacosDnsGuard` 入 actor、与 start/stop 保序；Service backend 走 IPC `set_dns`；非 macOS 不加空抽象；
4. **C4**：删真正失去调用者的文件与 globals；Updater 不加半迁移桥；更新 roadmap/ledger。

**不做（越界即返工）：**

- 不迁 `UpdaterManager::global()` 本体（PR-6d）——本阶段只确认它的 core 耦合已收敛为「按调用传入 `CoreClient`」；
- 不动 `ProxiesGuard` / `Handle` / `Sysopt` / `WindowManager` / `Hotkey` 五个 globals（各自有 owner PR）；
- 不动 `core/clash/ws.rs` 的四条 WS 流与前端消费面（它是**活的**日志通路，见 F5）；
- 不迁 `feat::patch_verge` 的 sysproxy / systray / locale 编排（PR-6e）；
- 不以 `CoreManager::global() == 0` 作为硬指标牺牲边界（卡上明令）。

---

## 1. 已核验事实（2026-08-02，全部读自 `899b069f5`）

### 1.1 C1 —— 状态与日志

| ID  | 事实                                                                                                                                                                                                                                                                                                    | 锚点                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| F1  | **「status read 不走 mailbox RPC」在 5a/5b 已经满足**：`CoreClient::status()` 是 `status_rx.borrow().clone()`，零 mailbox；`lifecycle()` 同理。C1 该项**已达成**，本阶段只需核对不回退                                                                                                                  | `client/core.rs:146-148`、`:150-152`                        |
| F2  | `RefreshStatus` 守卫消息的**生产调用点为零**（16 处全在 `client/core.rs` 测试内）；`RefreshHint` 唯一生产调用点是 `NyanpasuClient::core_status`                                                                                                                                                         | `client/core.rs:165-174`；`client/mod.rs:483`               |
| F3  | **`Logger` global 的三个写入者全部不可达**：它们都在 `Instance::start` 内，而 `Instance::try_new` **零调用点**、`CoreManager.instance` 初始化为 `None` 后**从未被赋值**。因此 `get_clash_logs` 今天**恒返回空** `VecDeque`                                                                              | `core/clash/core.rs:186,191,200`、`:94`、`:381`             |
| F4  | `Logger` 本身**已经是 100 条 ring**（`VecDeque` + `LOGS_QUEUE_LEN = 100`，超限 `pop_front`）；`clear_log` 零调用点                                                                                                                                                                                      | `core/logger.rs:5,7-36`、`:32`                              |
| F5  | **活的日志通路是 clash WS**：`core/clash/ws.rs` 的四条流（logs/traffic/memory/connections），`ClashWsHistory.logs` 上限 **1024**，经 `ClashWsEvent` 发到前端并被 `use-clash-logs.ts` 消费                                                                                                               | `core/clash/ws.rs:24,215,246,199-210`；`clash/mod.rs:46-52` |
| F6  | **`get_clash_logs` 没有任何前端消费者**：源码侧命中共 2 处，**全在 `frontend/interface/src/ipc/bindings.ts` 的 binding 定义自身**（声明 + invoke）；另有 2 处在 `frontend/interface/dist/`，那是同一文件的**构建产物**，不是消费者。日志页走的是 WS 通路（F5）。**复审时请勿把 `dist/` 命中当成使用者** | `ipc.rs:522-526`；`bindings.ts:31-32`                       |
| F7  | **`LogFrame` 类型在 tauri crate 内不存在**；service 侧的 `/ws/events`（`EventStream` / `inspect_logs` / `retrieve_logs`）在 IPC crate 里有，但 **tauri 侧零消费**                                                                                                                                       | `nyanpasu_ipc/src/client/shortcuts.rs:73,82,110`            |
| F8  | `backend/tauri/src/logging/` 整个模块**由构造即死**：`#![allow(dead_code)]` + `setup.rs:68` 的 `setup()` 调用被注释掉                                                                                                                                                                                   | `logging/mod.rs:1`；`setup.rs:68`                           |

### 1.2 C2 —— 运行模式

| ID  | 事实                                                                                                                                                                                                                               | 锚点                                                     |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| F9  | **`pending_run_type` 在 Rust 源码中不存在**——全仓仅命中三份设计文档。5a/5b 已消除，C2 该项对当前代码是 **no-op**                                                                                                                   | 全仓 grep 仅 `task.md:126`、`design.md:197`、roadmap:330 |
| F10 | **「reconcile 走 `CoreOperationGuard`」也已满足**：`CoreModeReconciler::reconcile` 内部就是 `begin_operation()` → `set_backend` → `run`                                                                                            | `core/actor/request.rs:78-92`（`:87` 取 guard）          |
| F11 | 5 s 轮询线程与三个 statics 全在一个文件：`IPC_STATE` / `KILL_FLAG` / `HEALTH_CHECK_RUNNING`；`spawn_health_check` 有 **4 处 spawn**（boot + install/start/restart 各一）                                                           | `core/service/ipc.rs:28-30,85-101`（`:97` 是 5 s sleep） |
| F12 | `get_ipc_state()` 有 **5 处生产读**：`feat.rs:383`、`feat.rs:401`、`client/mod.rs:305`、`client/mod.rs:544`、**`core/clash/core.rs:70`（在 `RunType::default()` 内）**                                                             | 见左                                                     |
| F13 | **`RunType::default()` 读两个 legacy global**（`Config::verge()` + `get_ipc_state()`），而它被 **`CoreStatusView::initial()` 调用**——actor 自己的初始快照构造就摸了这两个 global（删 statics 的**主阻塞点**）                      | `core/clash/core.rs:61-78`；`core/actor/types.rs:48`     |
| F14 | `set_backend` 的**生产调用点恰好一个**：`CoreModeReconciler::reconcile`（其余 6 处在测试内）。**没有名为 `set_mode` 的方法**——卡上的 `set_mode` 对应现有的 `set_backend`                                                           | `core/actor/request.rs:88`                               |
| F15 | `ServiceControlOps` trait **只有 install/start/stop/restart 四个方法**；**`update` 与 `uninstall` 不在 trait 上**，是自由函数                                                                                                      | `core/actor/backend.rs:618-624`                          |
| F16 | **`uninstall_service` 绕过 `NyanpasuClient`**：`ipc.rs:933-935` 直接调 `service::control::uninstall_service()`；而 `install_service`（`client/mod.rs:504-510`）**不调 `reconcile_service_mode`**，与 start/stop/restart 三者不对称 | 见左                                                     |
| F17 | `KILL_FLAG` 在 `stop` 路径用的是 **`compare_exchange_weak`**（可能伪失败），而 uninstall 路径用的是 strong CAS——与 PR-5-pre 修过的 weak-CAS 同类                                                                                   | `core/service/control.rs:274` vs `:179`                  |

### 1.3 C3 —— macOS DNS

| ID  | 事实                                                                                                                                                                                        | 锚点                                             |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| F18 | **`MacosDnsGuard` 不存在**——全仓仅两条「等 PR-5c 建它」的注释                                                                                                                               | `feat.rs:417-418`                                |
| F19 | 真正的 DNS 覆写代码是 `CoreManager::change_default_network_dns`（`#[cfg(macos)]`），它持有的状态是 `CoreManager.previous_dns: Mutex<Option<Vec<IpAddr>>>`                                   | `core/clash/core.rs:404-457`、`:373-383`         |
| F20 | 该函数**读两个 legacy global**：`Config::clash().latest().get_tun_device_ip()` 与 `RunType::default()`；双路径 = Service 走 IPC `set_dns`、否则走 `nyanpasu_utils::network::macos::set_dns` | `core/clash/core.rs:409,415-420,440-450`         |
| F21 | **IPC `set_dns` 存在且已上线**：`Client::set_dns(&NetworkSetDnsReq)`，端点 `/network/set_dns`，wire golden 已钉                                                                             | `nyanpasu_ipc/src/client/shortcuts.rs:91-96`     |
| F22 | **今天 DNS 与 start/stop 之间没有任何保序**：唯一调用点在 `patch_verge` 的 TUN 分支 `else` 侧；**走 restart 的那条路径根本不碰 DNS**；失败以 `let _ = ...inspect_err()` 吞掉                | `feat.rs:409-426`（`:419` 调用）                 |
| F23 | **退出时不恢复 DNS**：`resolve.rs:290` 只重置 sysproxy；`CoreClient::shutdown` / `NyanpasuClient::shutdown` 都没有 DNS 步骤——**覆写会跨崩溃/退出泄漏**                                      | `utils/resolve.rs:290`；`client/core.rs:277-283` |
| F24 | `SystemDnsCache` 是**只管 flush 的另一回事**（`ipconfig /flushdns` / `dscacheutil`），与 TUN 的 DNS 覆写生命周期无关，**不要混为一谈**                                                      | `client/system_dns.rs:4-7`                       |

### 1.4 C4 —— residual 与 ledger

| ID  | 事实                                                                                                                                                                                                                                                                                                                                                           | 锚点                                                                   |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| F25 | **`attach_core_port` 不存在**——全仓零命中。C4 该项是 **no-op**                                                                                                                                                                                                                                                                                                 | 全仓 grep                                                              |
| F26 | Updater 的 core 耦合**已经是收敛形态**：`CoreClient` 按调用传入（`update_core(&core_type, core)`），manager 本身不持有；唯一残留是 `client/mod.rs:562` 那行 `UpdaterManager::global()`                                                                                                                                                                         | `core/updater/mod.rs:223`；`client/mod.rs:558-567`                     |
| F27 | **两个文件已完全失去调用者**：`core/manager.rs`（84 行，`grant_permission` / `escape` 均零调用）与 `core/state.rs`（233 行，`#![allow(dead_code)]`）                                                                                                                                                                                                           | 见左 + `core/mod.rs:8,20`                                              |
| F28 | `core/clash/core.rs`（481 行）**约 75% 已死**：`enum Instance` 及其整个 impl、`CoreManager.instance` 字段、`CoreManager::status`。**活的只有** `RunType`、`find_binary_path`、`change_default_network_dns`                                                                                                                                                     | `core/clash/core.rs:80-368`、`:387-402`                                |
| F29 | 删掉 `manager.rs` 与 `Instance` 后，`find_binary_path` 只剩**一个**活调用者 `utils/dirs.rs:345`；且 `setup.rs:90-103` 已有可注入的替代 `OsCoreBinaryResolver`                                                                                                                                                                                                  | 见左                                                                   |
| F30 | ledger 现值：`config_calls` 102、`service_globals` 58、`migration_markers` 15、`legacy_dto_refs` 299、`test_real_dirs` 0；gate 当前为绿                                                                                                                                                                                                                        | `scripts/architecture-ledger.snapshot.json`                            |
| F31 | **ledger 有 bug：`core/clash/core.rs` 第 52 行起全部对 ledger 不可见。** 块注释追踪器在 `:51` 的 doc 注释里看到字面量 `/core/*` 就置 `inBlockComment = true`，而该文件**全文没有 `*/`**（实测 0 处）。后果：`Logger::global()` 实际 4 处只报 1 处、`config_calls` 少算约 3 处                                                                                  | `scripts/architecture-ledger.ts:493-507`；`core/clash/core.rs:51`      |
| F32 | **该 bug 的损害限于这一个文件**：`inBlockComment` 声明在**逐文件处理函数体内**（`:485`），每个文件重新初始化为 `false`，**不跨文件泄漏**。因此修复范围就是这一处逻辑，**不需要扩大到全仓复查**                                                                                                                                                                 | `scripts/architecture-ledger.ts:485`                                   |
| F33 | **CI 有 macOS runner 且会在 PR 上跑后端测试**：`ci.yml` 的 `test_unit` 作业矩阵含 `macos-latest`，触发条件是 `pull_request` 到 `main` / `dev` / `release-*`，执行 `pnpm test` → `run-p test:*` → `test:backend` = `cargo test --all-features`。因此 **`#[cfg(target_os = "macos")]` 门控的单测会在 CI 上真实运行**                                             | `.github/workflows/ci.yml:201-215,303-304`；`package.json:40,42`       |
| F34 | **但没有任何作业能跑 smoke 3 本身**：全部 workflow 内只有一处测试调用（`ci.yml:304`），**没有任何作业启动应用**；而 TUN 需要签名的系统/网络扩展 + root，DNS 覆写路径还要经 `osascript` 提权（`client/system_dns.rs:41-49`）——GitHub 托管的 macOS runner **无法安装/批准网络扩展，也无法非交互提权**。`ci.yml:115` 自己留着 `TODO: support test cross-platform` | `.github/workflows/ci.yml`（全仓仅 `:304` 一处测试调用）；`ci.yml:115` |

---

## 2. 需 leader 裁定的决策点

### D1 —— C1 的「100 条 `LogFrame` ring」到底建不建 —— **裁定 A（只删不建）**

卡上写「actor 维护 100 条 `LogFrame` ring；`get_clash_logs` 从 raw 渲染」。但三条事实叠加后，这件事的性质变了：

- `Logger` **已经是** 100 条 ring（F4），只是**没有写入者**（F3）——它不是「缺一个 ring」，是「ring 没有数据源」；
- `get_clash_logs` **没有任何前端消费者**（F6），今天恒返回空；
- `LogFrame` 类型在 tauri 侧不存在（F7），要建就得同时接上数据源——唯一现成的源是 service 的 `/ws/events`，**而它只在 Service 模式下有**；
- 真正在给前端供日志的是 clash WS（F5），1024 条，前端已消费。

三条路：

- **裁定 A：只删不建。** 删 `Logger` global 与三个不可达写入者；`get_clash_logs` 保留命令与 wire（避免 bindings 变化），内部改为返回空并标注去向。理由：为一个**没有消费者**的命令建一条**只在 Service 模式有数据**的新通路，是 CLAUDE.md §2 的推测性构建；日志能力今天由 WS 通路实际承担。
- **选项 B（未采纳）：删 `Logger`，同时删 `get_clash_logs` 命令与 binding。** 更彻底，但**会改 bindings**（少一个命令），需要确认前端确实无人调用（F6 已证）——代价是本阶段多一处 wire 变化。
- **选项 C（未采纳）：照卡建 ring + 接 `/ws/events`。** 完整实现卡面，但引入「Local 模式无日志」的不对称，且服务对象不存在。**不推荐**。

> **卡面项的收窄必须留痕（leader 要求，不允许静默丢弃）：**
>
> task.md 卡 C1 写「actor 维护 100 条 `LogFrame` ring；`get_clash_logs` 从 raw 渲染」，其**原意是让核进程日志走 actor**。该原意的前提是「这条日志路径是活的」——**经核实不成立**（F3：写入者不可达；F6：命令无消费者；F4：ring 本身早已存在）。因此本阶段**只删不建**。
>
> **若将来真要做核进程日志**，需要重新设计两件事：**数据源**（Local 模式下核的 stdout/stderr 从哪采——`Instance::start` 那条路已死；Service 模式可用 `/ws/events`，但两者不对称）与**消费者**（今天前端只消费 WS 通路）。**这不是「漏了」，是「查证后主动收窄」**——记录在此，后来者不必重新发现一遍。

### D2 —— C2 删 statics 的**前置**：`RunType::default()` 怎么办 —— **裁定 A**

`RunType::default()` 读 `Config::verge()` + `get_ipc_state()`（F13），而它被 `CoreStatusView::initial()` 调用——**只要它还在，`IPC_STATE` 就删不掉**。

- **裁定 A：给 `CoreStatusView::initial(mode: RunType)` 加参数**，由 `pre_start` 传入 `args.mode`（actor 本来就有 `mode` 字段）。`RunType::default()` 随 `core/clash/core.rs` 一起删。理由：actor 的初始快照本来就该用注入的 mode，而不是回头读全局。
- **选项 B（未采纳）**：保留 `RunType::default()` 但改为不读 global（返回 `Normal`）。改动更小，但留下一个语义可疑的 `Default`。

**leader 裁定 A**：这正是整个迁移的方向——依赖显式传入而不是从全局捞。`RunType::default()` 读两个 global 却被 `CoreStatusView::initial()` 调用，是典型的隐藏依赖。

### D3 —— C3 的 `MacosDnsGuard` 放在 actor 的哪一层 —— **裁定 A**

DNS 覆写今天与 start/stop **完全无序**（F22），且**退出不恢复**（F23）。要「保序」就得定义与哪些事件保序。

- **裁定 A：RAII guard 挂在 actor state 上**，`Drop`/`shutdown` 时恢复。顺序契约：`Run` 成功后启用、`Stop` 之前恢复、`Shutdown` 必恢复。
- **选项 B（未采纳）**：只做显式 enable/disable 两条守卫消息，不做 RAII。更简单但**恢复不了崩溃路径**（虽然 RAII 也救不了 SIGKILL，见风险表）。

**leader 裁定 A**，并**特别赞成「明写不覆盖强杀」**：RAII 在 SIGKILL / 任务管理器结束进程时根本不会运行，如实写明比假装覆盖全部退出路径诚实得多——与 5b 学到的「诚实降级比强行自洽好」是同一条。**覆盖强杀是独立的一件事**（启动时检测并清理残留覆写），**不在 5c 范围**，去向见 §10。

### D4 —— smoke 3（macOS TUN/DNS）—— **用户裁定：路径乙（显式记为未本地验证）**

用户已裁路径乙。随之生效的硬前置是「**先核实 CI 是否真有能跑 TUN 的 macOS runner**」——**已核实，结论是分层的**：

| 问题                    | 结论                           | 依据                                                                                                          |
| ----------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| CI 有 macOS runner 吗？ | **有**，且会在 PR 上跑后端测试 | F33（`ci.yml` `test_unit` 矩阵含 `macos-latest`，`cargo test --all-features`）                                |
| 它能跑 smoke 3 吗？     | **不能**                       | F34（全仓无任何作业启动应用；TUN 需签名网络扩展 + root，DNS 覆写还要 `osascript` 提权，托管 runner 都做不到） |

**因此按裁定的「若没有」分支处理：不写「移交 CI」，降级为「已知未验证风险」。** 但结论不是一句「没有 runner」——**CI 实际覆盖了一部分**，出口判据必须把覆盖面与未覆盖面分开写，否则会低估已有保障、也会模糊真实风险面。

> **这不是「CI 暂未配置」，是托管 runner 在能力上做不到——加 job、加 runner 都解决不了。** 要真跑 smoke 3，需要**自托管 macOS 机器且预先批准网络扩展**（签名的系统/网络扩展需要人工批准一次，非交互环境无法完成；DNS 覆写还要提权）。**这个区别必须写明**：否则下一个人会以为「加个 macOS job 就能补上」，白花一轮才发现补不上。

#### smoke 3 的出口判据（照此写入 **PR 描述**与**发布说明**）

> **结论：smoke 3 未在本地验证（开发机为 Windows 11），且不可由 CI 覆盖。**
>
> **CI 覆盖的部分**：`#[cfg(target_os = "macos")]` 门控的 DNS guard 单测（T-DNS-01…04）**会在 CI 的 macOS runner 上真实编译并运行**（F33）——即 guard 的**建立/恢复顺序、失败降级**这些**逻辑**契约有自动化保障。
>
> **未验证的部分（已知风险，明确列出）**：以下**真实系统行为**未在任何环境验证——
>
> 1. 真实 TUN 开关是否按预期触发 DNS 覆写；
> 2. 真实 `networksetup` / IPC `set_dns` 是否成功改写系统 DNS；
> 3. 关闭 TUN 与正常退出应用后系统 DNS 是否**真的恢复**；
> 4. Service 模式与 Local 模式两条 DNS 路径（F20）在真机上是否一致。
>
> **原因**：开发机为 Windows 11；而 CI 的**托管** macOS runner 无法安装/批准签名网络扩展、也无法非交互提权（F34）——**这是能力边界，不是配置缺失**，补 job 无效，需自托管 mac 且预先批准扩展。
>
> **风险面（与 D3 的强杀缺口合并陈述——两者是同一个风险面的两半，分开写会低估）**：
>
> 系统 DNS 被改写后**未能恢复**，会在应用退出后仍然生效、影响全机解析。它有**两条**未闭合的路径：**①端到端未验证**——覆写与恢复的真实系统行为没有任何环境验证过（本条）；**②强杀不覆盖**——D3=A 的 RAII guard 在 SIGKILL / 任务管理器结束进程时根本不运行（§2 D3）。
>
> 两条叠加的净效果是：**「退出后 DNS 残留」这一后果既没有被测试证伪、也没有被机制完全堵住**。需要说明的是**该泄漏在 5c 之前就存在**（F23：今天连正常退出都不恢复），本阶段的 guard **缩小**了它（正常退出与 Stop 路径现在有恢复）**但不消除**；彻底兜底（启动时检测并清理残留覆写）见 §10 的 PR-6。

**共同硬要求**：**结论必须显式出现在 PR 描述里，不允许沉默跳过。**

#### smoke 2（Windows v1 daemon 升级 → v2 Service；拒绝升级时 fail-closed Local）

**本机可跑，因此不适用上面的降级路径——该跑就跑。** 它需要装/卸真实系统服务，步骤见 S8 的门禁清单之后。**若因环境原因当时跑不成，同样显式记录，不许默认通过。**

---

## 3. 实施步骤

> 每步给出编辑内容 → 验证 → 通过判据。已知坑沿用 5b：共享 target 的 kache 污染会造成本地 clippy 假红，用独立 `--target-dir` 复验再判定。

### S1 — 先修 ledger 的块注释 bug（**必须最先做**）

`scripts/architecture-ledger.ts:493-507` 的块注释追踪器把 `core/clash/core.rs:51` 的 `/core/*` 当成块注释开始，而该文件没有 `*/`，于是 **52–481 行整段对 ledger 不可见**（F31）。

**为什么必须最先做**：本阶段要删的正是这个文件的大部分。若 bug 不修，删除后的 ledger 差异会**比真实清理小**，`byKey` 对不上，无法用「差异恰好是这些」做判据——判据本身失效。

**改法**：`inBlockComment` 每文件重置（`:485`），并让块注释检测跳过**行注释内**的 `/*`。

**验证：** 修好后先跑一次 `pnpm architecture-ledger`，记录**真实基线**（预期 `service_globals` 58→61、`config_calls` 102→约 105）；这个新基线才是后续删除的比较基准。**本步单独成 commit 且单独更新一次 snapshot**，与后续删除的 delta 分开。

### S2 — C4：删两个完全死掉的文件

删 `core/manager.rs`（84 行）与 `core/state.rs`（233 行）及 `core/mod.rs:8,20` 的 `pub mod`（F27）。

**验证：** `cargo check`；`rg 'core::manager|core::state|ManagedState' backend/tauri/src` 为 0。

### S3 — C1：删 `Logger` global（按 D1=A）

删 `core/logger.rs` 整个文件与三个不可达写入者（它们随 S5 的 `Instance` 一起走）。`get_clash_logs` 按 D1 的裁定处理。

**验证：** `rg 'Logger::global|core::logger' backend/tauri/src` 为 0；bindings 差异按 D1 裁定预期。

### S4 — C3：`MacosDnsGuard` 入 actor（按 D3=A）

新建 `core/actor/dns.rs`（`#[cfg(target_os = "macos")]` 整体门控，**非 macOS 不留空抽象**，卡上明令）。把 `change_default_network_dns` 的逻辑与 `previous_dns` 状态搬入，改为 RAII guard 挂在 `CoreActorState`。

顺序契约（附录 A.3 声明一次）：`Run` 成功后启用 → `Stop` 之前恢复 → `Shutdown` 必恢复。两个 legacy global 读（F20）改为注入：TUN device IP 由调用方传入，mode 用 actor 的 `state.mode`。

`feat.rs:409-426` 的调用点改为经 client 的守卫方法；**失败不再吞掉**——按 5b 的 post-commit 模型给降级（`phase = CoreLifecycle`）。

**验证：** `rg 'change_default_network_dns' backend/tauri/src` 只剩 actor 内一处；`rg 'previous_dns' ` 同理。

### S5 — C4：删 `core/clash/core.rs` 的死面

删 `enum Instance` 及其 impl、`CoreManager.instance`、`CoreManager::status`、`CoreManager::global()`（F28）。`RunType` 迁到 `core/actor/`（它是活的且 actor 在用），`find_binary_path` 按 F29 处理（唯一活调用者 `utils/dirs.rs:345`；`setup.rs:90-103` 已有注入式替代）。

**验证：** `rg 'CoreManager' backend/tauri/src` 为 0；`cargo check`。

### S6 — C2：删 5 s 轮询线程与三个 statics

按 D2=A 先给 `CoreStatusView::initial(mode)` 加参数，切断 `RunType::default()` 这条阻塞（F13）。然后删 `core/service/ipc.rs` 的 `IPC_STATE` / `KILL_FLAG` / `HEALTH_CHECK_RUNNING`、`spawn_health_check` 与 4 处 spawn（F11）、`on_ipc_state_changed` 的 detached 线程。

`get_ipc_state()` 的 5 处生产读（F12）改为：模式判定统一走 actor 的 `state.mode` / `CoreClient::status().run_type`。

**同时修 F17 的 weak CAS**（`control.rs:274` → strong），与 PR-5-pre 的同类修复一致。

**验证：** `rg 'IPC_STATE|KILL_FLAG|HEALTH_CHECK_RUNNING|spawn_health_check|get_ipc_state' backend/tauri/src` 为 0。

### S7 — C2：`set_mode` / `reconcile_mode` 的显式化

F10 已证「reconcile 走 guard」满足，F14 已证 `set_backend` 是现有名字。本步只做**命名与调用面对齐**：facade 暴露显式 `set_mode` / `reconcile_mode`，内部转 `set_backend`；install/update/uninstall 保持独立 controller **不迁入 actor**（卡上明令）。

**F16 的两处不对称按 §7 的裁定分别处置**：① `install_service` 不 reconcile 是**有意**的，只加一行注释说明、不改行为；② `uninstall_service` 是**缺陷**，改为经 facade（`client.uninstall_service()`）并在其中调 `reconcile_service_mode`，与 stop/restart 同形。**这不违反 C2**——C2 禁的是「迁入 CoreActor」，facade 不是 actor（§7 ② 有详述）。

**验证：** `rg 'service::control::uninstall_service' backend/tauri/src/ipc.rs` 为 0（改走 facade）；`install_service` 行为**零变化**（只多一行注释）。

### S8 — 门禁

```powershell
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
pnpm test:backend
git diff frontend/interface/src/ipc/bindings.ts
pnpm lint:ts
pnpm architecture-ledger
pnpm lint:architecture-ledger
```

**ledger 预期**：以 **S1 修好后的新基线**为准（不是 F30 的 102/58/15）。`service_globals` 预期下降 `Logger`(4) + `CoreManager`(1)；`config_calls` 预期下降 `core/clash/core.rs` 内那几处；`test_real_dirs` **必须仍为 0**。

**bindings 预期**：按 D1 裁定——A 则**零变化**，B 则少一个 `getClashLogs`。

---

## 4. 测试矩阵

> **每条测试必须写明「它能抓住哪一行生产代码的删除」**——这是 5b 阶段审查得出的唯一可操作的非空转判据（有三条测试写了断言但删掉对应生产分支照样绿）。下表第三列即为此。

| ID        | 断言                                                               | **删掉哪行生产代码会让它红**                                       |
| --------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| T-DNS-01  | `Run` 成功后 DNS 覆写生效（guard 已建立）                          | S4 中 `Run` 处理器里建立 guard 的那一行                            |
| T-DNS-02  | `Stop` 之前 DNS 已恢复（顺序，不只是「最终恢复了」）               | S4 中 `Stop` 处理器里 `drop(guard)` 先于 `backend.stop()` 的那一行 |
| T-DNS-03  | `Shutdown` 必恢复                                                  | S4 中 shutdown 路径的恢复调用                                      |
| T-DNS-04  | DNS 设置失败 → 降级而非静默（F22 的吞错今天存在）                  | S4 中把 `Err` 转成 degradation 的那一行（删掉它会退回 `let _ =`）  |
| T-MODE-01 | `CoreStatusView::initial(mode)` 用注入的 mode，**不读任何 global** | D2=A 的参数化那一行（删掉参数退回 `RunType::default()` 即红）      |
| T-MODE-02 | 模式判定读 actor 的 mode 而非 `get_ipc_state()`                    | S6 中改判定源的那一行                                              |
| T-SVC-01  | `stop` 的 KILL_FLAG CAS 在竞争下不伪失败                           | F17 的 `weak` → `strong` 那一行                                    |
| T-LOG-01  | `get_clash_logs` 的契约（按 D1 裁定）                              | 按裁定填                                                           |

**回归（期望零改动通过）**：`client/core.rs` 与 `client/mod.rs` 的 5a/5b 测试全套（467 条基线）——**若被迫修改，说明范围溢出，停下核查**。

---

## 5. 契约归属：签名保证 vs 测试保证

> 5b 的教训：有些保证由类型系统承担比由测试承担更强（那轮驳回了「为了可测而把 `-> T` 改回 `-> Result<T>`」的建议——为了测它而重新引入失败模式，等于先造出失败再测它）。本阶段逐条声明归属。

| 契约                                  | 由谁保证     | 说明                                                                                                |
| ------------------------------------- | ------------ | --------------------------------------------------------------------------------------------------- |
| 非 macOS 平台不存在 DNS 抽象          | **签名/cfg** | 整个 `core/actor/dns.rs` 由 `#[cfg(target_os = "macos")]` 门控，非 macOS 上**类型不存在**——无需测试 |
| `CoreStatusView::initial` 不读 global | **签名**     | mode 变成参数后，函数体内**没有**可读 global 的路径；T-MODE-01 是冗余保险                           |
| DNS guard 与 start/stop 的**顺序**    | **测试**     | 顺序是控制流性质，类型系统表达不了 → T-DNS-02 必须存在                                              |
| `get_ipc_state` 归零                  | **rg 判据**  | 删除类不变量用 grep 判据比测试更直接                                                                |
| Updater 的 core 耦合不回退为持有      | **签名**     | `update_core(&core_type, core)` 按调用传入，manager 无字段可存                                      |

---

## 6. 「永远发生在某阶段」类断言的复核

> 5b 实施期发现：leader 裁定的「守卫获取永远是 pre-commit」是错的——commit-first 模型下守卫结构性地在提交之后获取。**本节把本计划里所有此类断言逐条对着调用图列出**，供审查直接核。

| 断言                                          | 依据                             | 复核结论                                                                                                                     |
| --------------------------------------------- | -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| 「`Logger` 的写入者永不执行」                 | F3（`Instance::try_new` 零调用） | **成立**——但依赖「没有第二条构造 `Instance` 的路径」，实施时须再 grep 一次                                                   |
| 「`reconcile` 总在 guard 下」                 | F10（`request.rs:87`）           | **成立**，单一实现                                                                                                           |
| 「`set_backend` 只有一个生产调用点」          | F14                              | **成立**（其余 6 处在测试）                                                                                                  |
| 「DNS 覆写总在 TUN 分支」                     | F22                              | **成立但有洞**——restart 路径**不碰** DNS，这本身是缺陷，S4 要修                                                              |
| 「install/start/stop/restart 都会 reconcile」 | F16                              | **不成立**——`install_service` **不**调 `reconcile_service_mode`，`uninstall` 更是绕过 facade。**这是 §7 要 leader 裁的一条** |

---

## 7. 两处既有不对称 —— leader 已裁，分别处置

§6 的量词复核抓出这两条，性质**不同**，不能当成一类：

### ① `install_service` 不调 `reconcile_service_mode` —— **有意，加说明即可，不改行为**

裁定理由：**装服务不等于起服务**——运行中的后端没有发生变化，**没有可 reconcile 的对象**；模式会在服务真正启动时被拾起（start/stop/restart 三处都调了 `reconcile_service_mode`）。

**动作**：在 `client/mod.rs:504-510` 加一行注释说明该不对称是有意的，**不改行为、不加调用**。

### ② `uninstall_service` 绕过 `NyanpasuClient` —— **是缺陷，S7 补上**

`ipc.rs:933-935` 直接调 `service::control::uninstall_service()`，而同组的 `install_service`（`ipc.rs:926-928`）走的是 `client.install_service()`。两条理由：

1. **违反 CLAUDE.md §12**——Tauri 命令应是薄适配器，不直接调具体 controller；
2. **实质风险**：核正在 **Service 模式运行**时卸载服务，会让当前后端失效，**此时不 reconcile 是真缺口**，不只是架构洁癖。

**动作**：S7 把它改为经 facade（`client.uninstall_service()`），并在其中调 `reconcile_service_mode`——与 stop/restart 同形。

> **它不与卡面 C2 冲突**：C2 说的是「install/update/uninstall 保持独立 concrete controller、**不迁入 CoreActor**」。**经由 facade 调用具体 controller 完全符合该约束——facade 不是 actor。** 这句话要留着，否则复审者会以为 S7 违了卡。

## 8. 风险与回滚

| 风险                                   | 概率 | 影响                 | 缓解                                                                        |
| -------------------------------------- | ---- | -------------------- | --------------------------------------------------------------------------- |
| ledger bug 未先修 → 删除后差异对不上   | 高   | 判据失效、误判为回归 | S1 强制最先做且单独成 commit + 单独 snapshot                                |
| 删 `core/clash/core.rs` 牵出未知调用者 | 中   | 编译红               | F28/F29 已逐项列出活面；`cargo check` 逐步验证                              |
| DNS guard 救不了强杀（SIGKILL / 断电） | 中   | DNS 覆写残留         | **计划里明写它不覆盖强杀**；恢复兜底属 PR-6（启动时检测并恢复），本阶段不做 |
| 删 statics 后模式判定漂移              | 中   | Service/Local 判错   | T-MODE-02 钉判定源；smoke 2 覆盖真实升级路径                                |
| smoke 3 无法本地执行                   | 高   | Exit 判据不可满足    | **D4 的两条路径，用户择一**；无论哪条，结论必须出现在 PR 描述               |

---

## 9. 提交切分建议

1. `fix(scripts): reset the ledger block-comment state per file` —— S1（**含 snapshot 单独更新**）；
2. `refactor(core): delete the unreachable manager and state modules` —— S2；
3. `refactor(core): delete the legacy logger global` —— S3；
4. `feat(core): own the macos dns override in the actor` —— S4 + T-DNS；
5. `refactor(core): delete the dead core manager surface` —— S5；
6. `refactor(core): delete the service polling statics` —— S6 + T-MODE + T-SVC；
7. `refactor(client): make run-mode changes explicit` —— S7 + S8。

第 1 步**必须单独且最先**：它改的是判据本身，与被判据衡量的删除混在一起就无法归因。

---

## 10. 明确 out-of-scope（登记去向）

| 项                                                                | 去向                                                    |
| ----------------------------------------------------------------- | ------------------------------------------------------- |
| `UpdaterManager::global()` 本体                                   | **PR-6d**（F26：core 耦合已收敛）                       |
| `ProxiesGuard` / `Handle` / `Sysopt` / `WindowManager` / `Hotkey` | 各自 owner PR                                           |
| `core/clash/ws.rs` 四条流与前端消费面                             | **不动**（F5：活的日志通路）                            |
| `backend/tauri/src/logging/` 死模块                               | 与本阶段无关，登记待清（PR-7）                          |
| 启动时检测并恢复残留 DNS 覆写（**强杀 / 断电后的兜底**）          | **PR-6**——D3=A 的 RAII 明确不覆盖强杀，这是独立的一件事 |
| `attach_core_port`                                                | **不存在**（F25），C4 该项记为 no-op                    |
| `pending_run_type`                                                | **不存在**（F9），C2 该项记为 no-op                     |

---

## 11. 附录 A — 接线单点声明（normative）

> 沿用 5a/5b 的反漂移机制：本附录是 5c 全部新增/变更类型的**唯一声明处**，其它小节只引用。

### A.1 `CoreStatusView::initial` 的签名变化（D2=A）

```rust
// core/actor/types.rs
- pub(crate) fn initial() -> Self            // 内部调 RunType::default()，读两个 global
+ pub(crate) fn initial(mode: crate::core::RunType) -> Self   // 由 pre_start 传 args.mode
```

### A.2 `RunType` 的新家（S5）

```rust
// 从 core/clash/core.rs:35-78 迁入 core/actor/mode.rs（新文件）
pub(crate) enum RunType { Normal, Service, Elevated }
pub(crate) fn classify(enable_service: bool, ipc_state: ..) -> RunType;   // 纯函数，保留
// impl Default for RunType —— **删除**（D2=A：它是 F13 的阻塞点）
```

### A.3 macOS DNS guard（D3=A；整体 cfg 门控）

```rust
// core/actor/dns.rs —— 整个文件 #[cfg(target_os = "macos")]
pub(crate) struct MacosDnsGuard {
    previous: Option<Vec<IpAddr>>,   // 从 CoreManager.previous_dns 迁入
    device: String,
}

// 顺序契约（本处声明一次，其它小节只引用）：
//   Run 成功后   → 建立 guard（覆写生效）
//   Stop 之前    → drop guard（恢复）——顺序由 T-DNS-02 钉住
//   Shutdown     → 必恢复
//   **不覆盖强杀**（SIGKILL / 断电）——启动时兜底恢复属 PR-6
```

### A.4 facade 的显式模式 API（S7）

```rust
// NyanpasuClient
pub async fn set_mode(&self, mode: RunType) -> Result<()>;        // 内部转 set_backend
pub async fn reconcile_mode(&self) -> Result<()>;                 // 内部转 CoreModeReconciler
// install / update / uninstall —— **不迁入 actor**（卡上明令），保持独立 controller
```
