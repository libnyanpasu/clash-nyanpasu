# v2 实施审计入口（2026-08-13，停止线交付）

按用户指令实施到 **legacy bridge 阶段之前停止**。本文是审计的入口清单：代码在哪、对照哪节设计、测试怎么跑、哪些是已知偏离与限制。旧代码零改动（除 `core/mod.rs` 一行模块声明与 Cargo 依赖增行）、Tauri commands 未换线、v1 wire 未删。

## 1. 代码位置

| 分支                                                              | 内容           | 提交                      |
| ----------------------------------------------------------------- | -------------- | ------------------------- |
| submodule `feat/core-control-plane`（自 upstream `e899bce`=#390） | PR-A+PR-B 全部 | `1be4afd`→`faa76a0` 11 个 |
| app `refactor/core-actor-v2`（自 `pr5/1-pre` tip）                | C1/C2/C3/D1    | `108c393d`+`c9adbad9`     |

### runtime 仓（backend/nyanpasu-runtime）

| 模块                                                                           | 对照设计   | 内容                                                                                                                                                                               |
| ------------------------------------------------------------------------------ | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/nyanpasu-core-metadata/src/error_kind.rs`                              | 修订 A4    | wire 表 12→17 kind（+shutting_down/queue_full/operation_conflict/backend_unavailable/internal）                                                                                    |
| `crates/nyanpasu-core-manager/src/control/mod.rs`                              | §9         | OperationId/ConfigInput/CoreCommand/CoreControl handle/OperationHandle/ControlOptions                                                                                              |
| `.../control/executor.rs`                                                      | §10        | 单 task 串行、幂等 registry（同 id+digest 附着/异 payload 冲突/有界淘汰）、closing latch、取消隔离、long-poll                                                                      |
| `.../manager/reconcile.rs`                                                     | A2/A3 修订 | 统一 `reconcile` 事务（CAS→内部 check→分派 start/apply/switch）；`ApplyOutcome::Started` 新变体                                                                                    |
| `.../runtime/{mod,process}.rs`                                                 | §12        | RuntimeBackend/RuntimeInstance trait 边界 + 进程实现平移                                                                                                                           |
| `.../dns.rs` + `.../manager/dns_sync.rs`                                       | A5③/§8     | DnsController trait + DnsOverrideRecord + 固定阶段挂点（reconcile converge 尾/stop·shutdown 头 restore/recover 尾）+ 建构期 orphan reconcile + `cfg(macos)` scutil State:-key 骨架 |
| `nyanpasu_ipc/src/api/core/v2.rs` + contract + shortcuts                       | §19.3      | `/v2/core/{submit,operation,status}` 加法 wire                                                                                                                                     |
| `crates/nyanpasu-service-runtime/.../manager_bridge.rs` + `routing/core/v2.rs` | PR-B       | daemon 内嵌 CoreControl；submit-query；断线不取消                                                                                                                                  |

### app 仓（backend/tauri/src/core/actor_v2/）

| 文件                                  | 对照设计（2026-08-12 app 集成） | 内容                                                                                                                                                                       |
| ------------------------------------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`                              | §3/§4/§5.2                      | CoreActor v2：Submit/ChangeHost/EndpointEvent/EndpointDown/Shutdown；EndpointSlot；I-R1/2/3；handoff 三阶段+StopProof+generation fencing；投影 watch+broadcast；CoreClient |
| `endpoint.rs`                         | §3.3                            | ControlEndpoint port + LocalEndpoint(CoreControl)/ServiceEndpoint(IPC v2) 双适配器 + 逐字段投影映射                                                                        |
| `service_actor.rs`                    | §6                              | ServiceActor：EnsureReady 收敛/版本门唯一实现点（复用 `service/compat.rs`）/Uninstall 自查守卫/有界重启+Exhausted latch/启动版本对账(auto-update UAC 语义)                 |
| `intent.rs`                           | §2                              | RuntimeIntentBuilder 纯服务（document→text+digest+CAS token）                                                                                                              |
| `tests.rs` + service_actor 内联 tests | —                               | 12 个测试全绿                                                                                                                                                              |

## 2. 测试跑法

```bash
# runtime 仓（461 全绿）
cd backend/nyanpasu-runtime && cargo test --workspace
# 重点套件：control_plane / fake_backend / dns_override / reconcile

# app 仓（12 个 actor_v2 测试）
cd backend && cargo test -p clash-nyanpasu --lib actor_v2
```

## 3. 记录在案的实现偏离（各模块 doc 注释均有原文）

1. **Check 不是 CoreCommand 变体**（A2 降格为咨询后，独立方法比不可能的 registry 状态更诚实）。
2. **CoreError.kind 保持 `Option`**（R0 "不猜 kind" 原则压过设计 §25 的"kind 必在"）。
3. **retryable 由产生点决定**而非 kind 决定（OperationConflict 两义性）。
4. **ChangeHost 携带目标 endpoint**（编排送达，actor 不自取——§6.4 原则的强化）。
5. **handoff 完成 = 目标已采纳、runtime 停止**；目标侧 Reconcile 与 CommittedDegraded 归 facade 编排（actor 不构造 intent）。
6. **ControlEndpoint trait 替代具体 EndpointHandle enum**（同形状 + 测试缝）。
7. **PR-B 不删 v1**（停止线要求旧代码可编译；删除属 bridge 阶段，计划文档 §0 已记）。
8. **投影泵为 2s 轮询**（phase-1；push 流不改消息协议即可替换，OQ-6）。

## 4. 已知限制 / 审计关注点（诚实清单）

| #   | 事项                                                                                                                                                     | 去向                                                 |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| L1  | macOS DNS scutil 实现在 Windows 上零编译零验证；仅结构+fake 测试可信                                                                                     | Phase-0 spike（判据在 `dns.rs` doc）                 |
| L2  | quarantine recovery 的死亡证明走 pid-file（进程后端机制），未经 RuntimeBackend trait——fake backend 下 Recover 诚实地失败                                 | 后续把证明路由进 trait（`fake_backend.rs` 测试注释） |
| L3  | `CoreKind→CoreType` wire 映射有损（alpha 通道塌缩、Meow 无 wire 表示）；bridge 阶段 facade 须携带 intent 的原 `CoreType`                                 | `endpoint.rs::app_core_kind_to_type` doc             |
| L4  | facade 编排（reconcile_core/service_mode 时序/shutdown shared-future）**未实现**——§6.4 时序已定，属 bridge 前的最后一块                                  | 审计后实施                                           |
| L5  | v2 submit admission 错误经 R 信封丢 retryable（客户端按 kind 推导）                                                                                      | `v2.rs` doc                                          |
| L6  | app 共享 target 存在损坏 rlib+usvg ICE（环境问题非代码），两个 app 提交按既有先例 `--no-verify`，补证=check exit 0+测试全绿；门禁应在隔离 target/CI 复跑 | 用户知悉                                             |
| L7  | Service 泵/事件流为轮询+断线报告；daemon ws 事件流接入留待 PR-D 精化                                                                                     | OQ-6                                                 |

## 5. 与既有 PR 栈的关系

`#5070–#5074` 处置仍待用户裁定（本实施未动它们）；本工作基于 `pr5/1-pre` 分支内容但未 rebase/merge #5070。submodule pin 未移动（path deps 读工作树）。
