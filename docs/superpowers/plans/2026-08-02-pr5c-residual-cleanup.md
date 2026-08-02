# PR-5c 实施计划 — 状态/日志、运行模式、macOS DNS、residual 清理

**日期：** 2026-08-02
**版本：** v1（首版，待双审）
**分支基线：** `refactor/core-manager-actor` @ `899b069f5`（PR-5b 阶段门已关闭：实施 7 提交 + 修复 8 提交，467 passed / 1 ignored）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/task.md` 卡 C1–C4（`:115-160`）+ 文末最终删除清单
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.3
**平台：** Windows 11 / PowerShell

> **全部事实读自 `899b069f5` 的工作树**（5b 落地后）。凡是「卡上写了但代码里不存在」的项，一律照实记为 no-op，**不为了删它而先造出来**——这是 5b 的既定做法（B3 的 `ControllerBinding` 先例）。

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

| ID  | 事实                                                                                                                                                                                                                       | 锚点                                                        |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| F1  | **「status read 不走 mailbox RPC」在 5a/5b 已经满足**：`CoreClient::status()` 是 `status_rx.borrow().clone()`，零 mailbox；`lifecycle()` 同理。C1 该项**已达成**，本阶段只需核对不回退                                     | `client/core.rs:146-148`、`:150-152`                        |
| F2  | `RefreshStatus` 守卫消息的**生产调用点为零**（16 处全在 `client/core.rs` 测试内）；`RefreshHint` 唯一生产调用点是 `NyanpasuClient::core_status`                                                                            | `client/core.rs:165-174`；`client/mod.rs:483`               |
| F3  | **`Logger` global 的三个写入者全部不可达**：它们都在 `Instance::start` 内，而 `Instance::try_new` **零调用点**、`CoreManager.instance` 初始化为 `None` 后**从未被赋值**。因此 `get_clash_logs` 今天**恒返回空** `VecDeque` | `core/clash/core.rs:186,191,200`、`:94`、`:381`             |
| F4  | `Logger` 本身**已经是 100 条 ring**（`VecDeque` + `LOGS_QUEUE_LEN = 100`，超限 `pop_front`）；`clear_log` 零调用点                                                                                                         | `core/logger.rs:5,7-36`、`:32`                              |
| F5  | **活的日志通路是 clash WS**：`core/clash/ws.rs` 的四条流（logs/traffic/memory/connections），`ClashWsHistory.logs` 上限 **1024**，经 `ClashWsEvent` 发到前端并被 `use-clash-logs.ts` 消费                                  | `core/clash/ws.rs:24,215,246,199-210`；`clash/mod.rs:46-52` |
| F6  | **`get_clash_logs` 没有任何前端消费者**：`frontend/interface/src` 与 `frontend/nyanpasu/src` 内只命中 binding 定义本身；日志页走的是 WS 通路                                                                               | `ipc.rs:522-526`；`bindings.ts:31-32`                       |
| F7  | **`LogFrame` 类型在 tauri crate 内不存在**；service 侧的 `/ws/events`（`EventStream` / `inspect_logs` / `retrieve_logs`）在 IPC crate 里有，但 **tauri 侧零消费**                                                          | `nyanpasu_ipc/src/client/shortcuts.rs:73,82,110`            |
| F8  | `backend/tauri/src/logging/` 整个模块**由构造即死**：`#![allow(dead_code)]` + `setup.rs:68` 的 `setup()` 调用被注释掉                                                                                                      | `logging/mod.rs:1`；`setup.rs:68`                           |

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

| ID  | 事实                                                                                                                                                                                                                                                                          | 锚点                                                              |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| F25 | **`attach_core_port` 不存在**——全仓零命中。C4 该项是 **no-op**                                                                                                                                                                                                                | 全仓 grep                                                         |
| F26 | Updater 的 core 耦合**已经是收敛形态**：`CoreClient` 按调用传入（`update_core(&core_type, core)`），manager 本身不持有；唯一残留是 `client/mod.rs:562` 那行 `UpdaterManager::global()`                                                                                        | `core/updater/mod.rs:223`；`client/mod.rs:558-567`                |
| F27 | **两个文件已完全失去调用者**：`core/manager.rs`（84 行，`grant_permission` / `escape` 均零调用）与 `core/state.rs`（233 行，`#![allow(dead_code)]`）                                                                                                                          | 见左 + `core/mod.rs:8,20`                                         |
| F28 | `core/clash/core.rs`（481 行）**约 75% 已死**：`enum Instance` 及其整个 impl、`CoreManager.instance` 字段、`CoreManager::status`。**活的只有** `RunType`、`find_binary_path`、`change_default_network_dns`                                                                    | `core/clash/core.rs:80-368`、`:387-402`                           |
| F29 | 删掉 `manager.rs` 与 `Instance` 后，`find_binary_path` 只剩**一个**活调用者 `utils/dirs.rs:345`；且 `setup.rs:90-103` 已有可注入的替代 `OsCoreBinaryResolver`                                                                                                                 | 见左                                                              |
| F30 | ledger 现值：`config_calls` 102、`service_globals` 58、`migration_markers` 15、`legacy_dto_refs` 299、`test_real_dirs` 0；gate 当前为绿                                                                                                                                       | `scripts/architecture-ledger.snapshot.json`                       |
| F31 | **ledger 有 bug：`core/clash/core.rs` 第 52 行起全部对 ledger 不可见。** 块注释追踪器在 `:51` 的 doc 注释里看到字面量 `/core/*` 就置 `inBlockComment = true`，而该文件**全文没有 `*/`**（实测 0 处）。后果：`Logger::global()` 实际 4 处只报 1 处、`config_calls` 少算约 3 处 | `scripts/architecture-ledger.ts:493-507`；`core/clash/core.rs:51` |

---

## 2. 需 leader 裁定的决策点

### D1 —— C1 的「100 条 `LogFrame` ring」到底建不建

卡上写「actor 维护 100 条 `LogFrame` ring；`get_clash_logs` 从 raw 渲染」。但三条事实叠加后，这件事的性质变了：

- `Logger` **已经是** 100 条 ring（F4），只是**没有写入者**（F3）——它不是「缺一个 ring」，是「ring 没有数据源」；
- `get_clash_logs` **没有任何前端消费者**（F6），今天恒返回空；
- `LogFrame` 类型在 tauri 侧不存在（F7），要建就得同时接上数据源——唯一现成的源是 service 的 `/ws/events`，**而它只在 Service 模式下有**；
- 真正在给前端供日志的是 clash WS（F5），1024 条，前端已消费。

三条路：

- **推荐 A：只删不建。** 删 `Logger` global 与三个不可达写入者；`get_clash_logs` 保留命令与 wire（避免 bindings 变化），内部改为返回空并标 `#[deprecated]` 注释，去向记 PR-6/7。理由：为一个**没有消费者**的命令建一条**只在 Service 模式有数据**的新通路，是 CLAUDE.md §2 的推测性构建；日志能力今天由 WS 通路实际承担。
- **选项 B：删 `Logger`，同时删 `get_clash_logs` 命令与 binding。** 更彻底，但**会改 bindings**（少一个命令），需要确认前端确实无人调用（F6 已证）——代价是本阶段多一处 wire 变化。
- **选项 C：照卡建 ring + 接 `/ws/events`。** 完整实现卡面，但引入「Local 模式无日志」的不对称，且服务对象不存在。**不推荐**。

**推荐 A。** 若裁 B，S? 增加一条 bindings 差异；若裁 C，需先回答「Local 模式的 frames 从哪来」。

### D2 —— C2 删 statics 的**前置**：`RunType::default()` 怎么办

`RunType::default()` 读 `Config::verge()` + `get_ipc_state()`（F13），而它被 `CoreStatusView::initial()` 调用——**只要它还在，`IPC_STATE` 就删不掉**。

- **推荐 A：给 `CoreStatusView::initial(mode: RunType)` 加参数**，由 `pre_start` 传入 `args.mode`（actor 本来就有 `mode` 字段）。`RunType::default()` 随 `core/clash/core.rs` 一起删。理由：actor 的初始快照本来就该用注入的 mode，而不是回头读全局。
- **选项 B**：保留 `RunType::default()` 但改为不读 global（返回 `Normal`）。改动更小，但留下一个语义可疑的 `Default`。

**推荐 A。**

### D3 —— C3 的 `MacosDnsGuard` 放在 actor 的哪一层

DNS 覆写今天与 start/stop **完全无序**（F22），且**退出不恢复**（F23）。要「保序」就得定义与哪些事件保序。

- **推荐 A：RAII guard 挂在 actor state 上**，`Drop`/`shutdown` 时恢复。顺序契约：`Run` 成功后启用、`Stop` 之前恢复、`Shutdown` 必恢复。
- **选项 B**：只做显式 enable/disable 两条守卫消息，不做 RAII。更简单但**恢复不了崩溃路径**（虽然 RAII 也救不了 SIGKILL，见风险表）。

**推荐 A**，并在计划里写明它**不能**覆盖强杀。

### D4 —— smoke 3（macOS TUN/DNS）本机跑不了：两条路径，**不替用户选**

Exit 要三个 smoke，其中 smoke 3 是 macOS 的 TUN 开关与 DNS 恢复，**本机是 Windows 11**。两条路径的**具体写法**如下，请用户择一：

|               | **路径甲：找 mac 实机验**                                                                                                                                     | **路径乙：显式记为「未本地验证」移交 CI**                                                                                                                                                                     |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Exit 判据写法 | 「smoke 3 由 `<执行人>` 在 macOS `<版本>` 上执行，记录：TUN 开→`scutil --dns` 显示覆写生效；TUN 关→恢复原 DNS；应用退出→DNS 已恢复。三条截图或命令输出入 PR」 | 「smoke 3 **未在本地验证**（开发机为 Windows 11）。合并门改为：CI macOS runner 上跑 `<具体 job 名>`，断言 `<具体判据>`；**若 CI 无 macOS runner 或该 job 不存在，则本条降级为「已知未验证」并写入 PR 描述**」 |
| 前置          | 需要一台能装本应用的 mac + 愿意跑的人                                                                                                                         | **需要先核实 CI 是否真有能跑 TUN 的 macOS runner**——不能假设                                                                                                                                                  |
| 风险          | 人工步骤不可重复                                                                                                                                              | TUN 需要网络扩展权限，CI runner 未必给；若不给则本条实际等于「不验证」                                                                                                                                        |
| 我的判断      | ——                                                                                                                                                            | **路径乙必须先做那次核实**，否则「移交 CI」会变成一句没有落点的话                                                                                                                                             |

**我不推荐哪一条**——这是用户对「合并前要多少验证」的偏好。但两条路径都要求：**smoke 3 的结论必须显式出现在 PR 描述里**，不允许沉默跳过。

> smoke 2（Windows v1 daemon 升级到 v2 Service，拒绝升级时 fail-closed Local）**本机可跑但需真实服务环境**——它需要安装/卸载真实系统服务。S? 会给出具体步骤；若用户不愿在开发机装服务，同样适用上表的两条路径写法。

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

**顺带处理 F16 的两处不对称**（`uninstall_service` 绕过 facade、`install_service` 不 reconcile）：**先向 leader 报告，不擅自改**——它们可能是有意的（见 §6）。

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

## 7. 需 leader 确认的两处既有不对称（**我不擅自改**）

1. **`install_service` 不调 `reconcile_service_mode`**（F16），而 start/stop/restart 三者都调。若是有意（装完还没起服务、无可 reconcile），计划里加一句说明；若是漏，S7 补上。
2. **`uninstall_service` 完全绕过 `NyanpasuClient`**（`ipc.rs:933-935` 直调），是唯一一个不走 facade 的 service 命令。它与 §0「保持独立 controller」是否冲突，请裁定。

---

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

| 项                                                                | 去向                                  |
| ----------------------------------------------------------------- | ------------------------------------- |
| `UpdaterManager::global()` 本体                                   | **PR-6d**（F26：core 耦合已收敛）     |
| `ProxiesGuard` / `Handle` / `Sysopt` / `WindowManager` / `Hotkey` | 各自 owner PR                         |
| `core/clash/ws.rs` 四条流与前端消费面                             | **不动**（F5：活的日志通路）          |
| `backend/tauri/src/logging/` 死模块                               | 与本阶段无关，登记待清（PR-7）        |
| 启动时检测并恢复残留 DNS 覆写                                     | **PR-6**（本阶段的 guard 不覆盖强杀） |
| `attach_core_port`                                                | **不存在**（F25），C4 该项记为 no-op  |
| `pending_run_type`                                                | **不存在**（F9），C2 该项记为 no-op   |

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
