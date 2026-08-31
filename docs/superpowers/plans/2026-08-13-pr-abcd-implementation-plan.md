# PR-A～D 实施编排计划（2026-08-13）

- **依据**：roadmap §6.5、控制面设计（2026-08-08 + 修订 A1–A6）、app 集成设计（2026-08-12）、审计报告 §3
- **前置已满足**：nyanpasu-runtime #390 已合并（`e899bce`，typed `CoreErrorKind` 12 kind wire 表）
- **停止线（用户裁定）**：实施到 **legacy bridge 阶段之前停止**——新 CoreActor v2 / ServiceActor / DNS 接线以"并存不接线"形态完成并可测，交用户审计；旧代码零改动、Tauri commands 不换线、v1 wire 不删。

## 0. 关键时序调整（相对 roadmap §6.5 的两处澄清）

1. **PR-B 不删 v1**。roadmap 写"IPC v2 only、删 v1"，前提是 app lockstep 立即切换。审计停止线要求 app 旧代码（v1 client 调用）在停止点仍可编译运行，因此 v1 endpoints 的删除**顺延到 bridge/清算阶段**（停止线之后）。PR-B 只做加法：v2 路由并存。最终态不变。
2. **PR-C 拆成 C-new / C-switch**。C-new（本计划范围）：新模块并存、单测齐全、不接线。C-switch（停止线之后）：Tauri commands 换线 + 删旧 GUI CoreActor 形态 + 删 v1。"吸收 ipc.rs 三 statics / 七入口"同理属 C-switch——本阶段新代码实现对应职责，旧文件不动。

## 1. 仓库与分支拓扑

| 仓库                                 | 分支                                           | 内容                |
| ------------------------------------ | ---------------------------------------------- | ------------------- |
| nyanpasu-runtime（submodule 工作树） | `feat/core-control-plane`（自 `e899bce`）      | PR-A + PR-B         |
| clash-nyanpasu                       | `refactor/core-actor-v2`（自 `pr5/1-pre` tip） | PR-C-new + PR-D-new |

- app 分支基于 `pr5/1-pre`（path deps 已切 submodule），**不做** #5070 的 rebase/merge——栈处置待用户裁定；path deps 读文件系统工作树，故 submodule pin 无需移动即可开发。
- 提交纪律：全部显式路径 add；submodule 内提交不动 app 仓 gitlink。

## 2. 现状基线核实（feat/core-control-plane @ e899bce）

已存在且**保留复用**（审计 §4 资产表的落地确认）：

- `manager/mod.rs`：`ctrl: Mutex<Ctrl>` 串行、`watch<CoreStatus>` 发布、`broadcast<LogFrame>`、epoch 分配、quarantine latch（`reject_quarantine`/`latch_quarantine`/`sweep_orphans`）、`stop_and_confirm_dead` 全路径强制、`ApplyOutcome{Noop..RolledBack,DurabilityUncertain}`。
- `error.rs`：`Error::kind() -> Option<CoreErrorKind>`（诚实缺省——不猜 kind）；R0 wire 表 12 kind 已含 `RevisionConflict`/`Quarantined`/`StopUnconfirmed`。
- `runtime_store.rs`：staged 提交/备份/epoch 清理；`spec.rs`：`InstanceSpec`（注意：仍是 `config_path` 文件路径输入）。

缺口（PR-A 要补的全部）：无 OperationId/幂等 registry、无 executor 队列（调用方 future 取消=事务暴露）、无统一 Reconcile（start/stop/apply 是分立公有方法、check 是 caller 前置门）、无 RuntimeBackend trait（`Instance` 具体类型直嵌）、无 DNS 组件。

## 3. 任务卡（对应会话任务 #2–#15）

### PR-A（runtime 仓）

| 卡  | 内容                                                                                                                                                                                                                                                                                                                                                                                                                   | 验证                                                  |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| A1  | model 层：`OperationId([u8;16])`、`CoreCommand{Reconcile,Stop,Recover,Shutdown,Check}`+envelope、`OperationState`、`ConfigInput{Inline}`（legacy path adapter 在 host 边界）、`CoreErrorKind` 超集扩展（admission 层：`ShuttingDown`/`QueueFull`/`OperationConflict`；app 路由层 `BackendUnavailable`）。**Rust API 错误保持 `Option<CoreErrorKind>` 的 R0 诚实语义**（设计 §25 "kind 必在"按 R0 原则收窄——不猜 kind） | cargo test（wire 表 golden 更新）                     |
| A2  | `RuntimeBackend`/`RuntimeInstance` trait 边界：以现 `Instance` 公有面（`wait_ready`/`pid`/`state`/`controller`/`epoch`/`stop_and_confirm_dead`）为准最小化；process 实现=现有代码平移；`StopProof` 显式化                                                                                                                                                                                                              | trait 化后现有测试全绿                                |
| A3  | Orchestrator：`Reconcile` 统一 mutating 入口（dispatch：未运行→start 路径 / spec 变→switch / 否则 apply 分类），**内部 check**（`InvalidConfig`/`ConfigCheckFailed` 于任何提交点前干净中止）；CAS `expected_applied`；十阶段信封中 ⑤–⑩ 即现有 apply/switch/publish 资产                                                                                                                                                | 单测覆盖审计 §3.2 的 orchestrator 段                  |
| A4  | `ControlExecutor`：bounded mpsc 单 task；operation registry（同 id+digest→同 op / 同 id 异 payload→`OperationConflict` / 有界 cache）；`Check` 独立 semaphore；`QueueFull`；closing latch；reply drop 不取消；panic→host fatal                                                                                                                                                                                         | 并发测试：caller 取消后事务跑到终态；幂等重发取原结果 |
| A5  | `CoreControl` handle：`submit→OperationHandle{wait}`/`status`/`subscribe_events`/`subscribe_logs`；builder 显式注入                                                                                                                                                                                                                                                                                                    | 单测                                                  |
| A6  | DNS：`DnsController` trait + `DnsOverrideRecord`（owner_generation/runtime_epoch/state；read-back 推进）+ 持久化 + orphan reconcile；orchestrator 固定挂点（start 尾/stop 头/事务内报告）；fake 供测试；`cfg(target_os="macos")` scutil 骨架——**本机（Windows）不可实测，Phase-0 spike 判据留注释，如实标注**                                                                                                          | fake 断言挂点次序与 record 推进                       |
| A7  | contract tests：对 `CoreControl` 泛型套件跑审计 §3.2 边界矩阵（admission/幂等/CAS/干净中止零副作用/StopProof→Quarantined→Recover/RolledBack=成功事务/取消隔离）；复用现有 fake-core 进程测试基建，embedded fake 仅证 trait 边界诚实                                                                                                                                                                                    | cargo test 全绿                                       |

### PR-B（runtime 仓，加法）

| 卡  | 内容                                                                                                                                 | 验证                            |
| --- | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------- |
| B1  | IPC v2：`/v2/core/*`（submit/operation query/status/events seq 有序）+ portable DTO + kind 直传；fail-closed 版本门保留；**v1 不删** | DTO roundtrip                   |
| B2  | service host：v2 路由接 daemon 内 `CoreControl` 实例（submit-query；断线不取消）；ACL 沿用                                           | contract 套件经 IPC client 跑通 |

### PR-C-new / PR-D-new（app 仓，并存不接线）

| 卡  | 内容                                                                                                                                                  | 验证                            |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| C1  | 分支 + `RuntimeIntentBuilder` 纯服务（snapshots→bytes+digest+artifact+revision+effects）                                                              | 纯值单测                        |
| C2  | CoreActor v2：4+1 消息、`EndpointSlot{Connected,HandingOff,Degraded}`、I-R1/2/3、逐字段投影 watch、订阅泵                                             | fake CoreControl 测试，无 sleep |
| C3  | ServiceActor：§6 全量（消息面/状态机/探针 timeout/版本门唯一实现点/endpoint watch 供给/有界重启+latch/启动对账 UAC 语义/Uninstall 自查守卫）          | fake daemon 适配器全迁移测试    |
| D1  | 显式 handoff 三阶段 + `ControllerGeneration` fencing（host 拒旧 gen）+ `CommittedDegraded`                                                            | 四路矩阵测试                    |
| D2  | DNS 接线核对（两 host 同组件 + handoff 源 Stop 内 restore）+ facade 编排（§6.4 时序 + shutdown shared-future；**不接 Tauri commands**）+ 审计材料汇总 | 停止线交付                      |

## 4. 停止线交付物

1. 两仓分支各自 `cargo test` 全绿 + fmt/clippy 干净；
2. 审计入口文档：新模块路径清单、与设计逐节对照表、测试运行方式、已知限制（macOS DNS 未实测、v1 并存清单=bridge 阶段删除表）；
3. 不做：Tauri 换线、旧代码删除、v1 删除、#5070 rebase/merge、PR 创建/推送（除非用户另行指示）。

## 5. 风险与如实声明

- **macOS DNS**：Windows 环境只能交付结构（trait/record/挂点/fake 测试）+ `cfg(macos)` 编译骨架；scutil 行为验收需 macOS spike（设计已预留 Phase-0 判据）。
- **service host 集成测试**：named pipe 本机可跑，但提权路径（install/UAC）不在自动化内——ServiceActor 用 fake 适配器测。
- **设计 §25 与 R0 的 kind 必在/可缺省矛盾**：按 R0 原则收窄（见 A1 卡），已在本计划显式记录，审计时可复核。
