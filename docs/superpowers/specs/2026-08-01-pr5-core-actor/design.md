# PR-5 — CoreActor 迁移简化任务

关联设计：`pr5-coreactor-simplified-design.md`

## 0. 全局约束

1. app 只新增一个跨步骤排他机制：`OperationId` + `CoreOperationGuard`；迁移完成后删除 `rebuild_gate`、`clash_patch_gate` 和 legacy lifecycle mutex。
2. `nyanpasu-core-manager` / service 内部 mutex、runtime directory lock 不属于替换范围。
3. 不新增 `CoreEngine` trait/factory、Engine\* 类型镜像、第二层恢复循环或 actor-held clash-api client。
4. 所有配置应用走 full-config `apply_config`；runtime 决定 patch/reload/restart/switch/rollback。
5. 运行配置采用 commit-first；`RolledBack` 表示 desired 与 Applied 暂时分离，不执行应用层二次回滚。
6. 测试只使用 TempDir；并发测试使用 RPC/barrier，不使用 sleep 断言顺序。

## R0 — nyanpasu-runtime 小型协议收敛 PR

- [ ] 在 `nyanpasu-core-metadata` 增加 `CoreErrorKind` enum，serde 字符串保持现有 `error_kind` 值。
- [ ] `nyanpasu-core-manager::Error::kind()` 返回该 enum；durability wrapper 递归返回 source kind。
- [ ] service error envelope 与 IPC client 改为复用 enum，wire golden 保持不变。
- [ ] 不新建 crate，不引入通用 CoreEngine trait。

**Exit**

- manager/service/client 不再各自维护第二份 error-kind match/string 表；
- v2 wire golden 全绿；
- submodule bump 后 clash app 可直接消费 typed kind。

## PR-5-pre — 依赖与 daemon 兼容门

### P1 — path dependency + lockstep

- [ ] `backend/Cargo.toml` 加 `exclude = ["nyanpasu-runtime"]`；utils/ipc/core-manager/core-metadata 使用 submodule path。
- [ ] 删除旧 git patch/source 注释；更新 Cargo.lock。
- [ ] 记录发布二进制体积变化。

### P2 — ServiceCompat

- [ ] 保留 major 版本 fail-closed 分类；旧 v1 daemon 永不进入 Service backend。
- [ ] status 返回 additive compat 信息；bindings regen。
- [ ] 保留启动时自动 update service 的既有语义。

**Exit**

- `cargo metadata`、workspace tests、bindings、architecture ledger 绿；
- v1.4.5 fixture 下没有 `/core/*` 请求；
- 本 PR 不引入 CoreActor。

## PR-5a — 最小 CoreActor + `OperationId` + backend enum

### A1 — CoreBackend

- [ ] 新增封闭 `CoreBackend::{Local, Service, #[cfg(test)] Test}`。
- [ ] Local 直接包装 `nyanpasu_core_manager::CoreManager`；Service 使用实例化 IPC Client，禁止 `service_default()`。
- [ ] 只实现 check/start/apply/stop/restart/recover 和状态/日志订阅所需方法。
- [ ] Local apply outcome 转换为现有 `CoreApplyData`；不定义 `ApplyReport`。

### A2 — 取消安全 `OperationId`

- [ ] `CoreClient` 在发送 `AcquireOperation` 前分配 `OperationId`，并构造 pending `CoreOperationGuard`。
- [ ] actor 维护 active operation + FIFO waiters；`ReleaseOperation` 同时支持释放 active operation 和取消 waiter。
- [ ] mutation 校验 active `OperationId`；status/log/lifecycle read 不要求 `CoreOperationGuard`。
- [ ] shutdown 拒绝全部 waiters并关闭 backend。
- [ ] 不实现 TTL、auto-steal、watchdog 或 delayed recovery。

### A3 — 接入现有生命周期 seam

- [ ] 先让 `CoreClient/CoreOperationGuard` 实现现有 `CoreLifecyclePort/CoreLifecycleLease` 兼容 seam；旧 trait 名称不扩散到新代码，使 rebuild/change-core 调用点暂不重写。
- [ ] setup 注入 CoreClient；start/stop/restart/status 改走 actor。
- [ ] 删除 legacy `CoreManager::lifecycle_lock` 和裸线程递归 recover；自动恢复只由 runtime/daemon 负责。

**Exit**

- operation 测试：FIFO、等待取消、刚获批取消、stale `ReleaseOperation`、wrong-id mutation、shutdown drain；
- Local/Service 基本生命周期 parity 测试；
- legacy core 生命周期不再被新调用点使用。

## PR-5b — 单一 runtime apply 管线

### B1 — Promoted / Applied 入 actor

- [ ] `CheckAndPromote` 在 `CoreOperationGuard` 下校验 candidate hash、dry-run、原子 promote，并推进 Promoted。
- [ ] `ApplyPromoted/StartPromoted` 根据 backend outcome 推进 Applied。
- [ ] CoreClient 通过 watch 暴露 lifecycle；删除 client `RuntimeLifecycleStore` 和 publish/restore helpers。

### B2 — `CoreOperationGuard` 替换 app locks

- [ ] 所有 rebuild/regenerate/start/change-core 在读取 snapshots 前取得 `CoreOperationGuard`，事务结束后释放。
- [ ] 删除 `rebuild_gate`。
- [ ] 保留 rebuild worker 的 capacity-1 coalesce；新 commit 在当前 build 期间发生时触发下一次 rebuild。

### B3 — 删除 API-first patch/补偿层

- [ ] `patch_running_config` 改为 typed desired commit + 统一 rebuild/apply。
- [ ] 删除 `RunningConfigPatchPort`、legacy bridge、`clash_patch_gate`、ControllerBinding/cache、ConfigPatch mapper、compensation plan/fence。
- [ ] 根据 `CoreApplyData` 映射：success 推进 Applied；RolledBack/错误保持 Applied；warning 返回 degraded。

### B4 — 简化 ChangeCore

- [ ] `ApplicationClient::patch(core = new)`，复用统一 apply 管线。
- [ ] RolledBack 时保留 desired 新核与 Promoted 新配置，返回 `CommittedDegraded(CoreRollback)`。
- [ ] 删除 legacy draft/discard、rollback rebuild、product restore、old-core 第二次 restart、专用 `ChangeCoreReport`。
- [ ] 前端复用通用 `RuntimeApplyReport`/MutationOutcome 展示 degraded。

**Exit**

- `rg 'rebuild_gate|clash_patch_gate|RunningConfigPatchPort|LegacyRunningConfigPatchBridge'` 为 0；
- apply parity：Patched/Reloaded/Restarted/Switched/RolledBack/Warning；
- change-core rollback 测试断言 desired=new、Promoted=new、Applied=old；
- 两个并发 rebuild 不重叠，后一个在 `OperationGate` FIFO 后读取最新 snapshot。

## PR-5c — 必要清理，不做指标驱动半迁移

### C1 — 状态与日志

- [ ] backend status/events 投影到 actor watch snapshot；status read 不走 mailbox RPC。
- [ ] actor 维护 100 条 `LogFrame` ring；`get_clash_logs` 从 raw 渲染；可 additive 暴露 frames。
- [ ] 删除 legacy Logger global。

### C2 — 运行模式

- [ ] app config/service control 完成后显式调用 `set_mode/reconcile_mode`；该操作正常排队取得 `CoreOperationGuard`。
- [ ] 删除 `pending_run_type` 设计、service 轮询线程及相关 statics。
- [ ] service install/update/uninstall 保持独立 concrete controller，不迁入 CoreActor。

### C3 — macOS DNS

- [ ] actor 内 `MacosDnsGuard` 与 start/stop 保序；Service backend 使用 IPC set_dns。
- [ ] 非 macOS 不增加空 `NetworkDnsPort` 抽象。

### C4 — residual 与 smoke

- [ ] 删除真正已失去调用者的 core/service/logger 文件与 globals。
- [ ] Updater 不增加 `attach_core_port` 半迁移桥；保留 PR-6d owner 的单一 residual。
- [ ] 更新 roadmap/ledger，不以 `CoreManager::global() == 0` 作为牺牲边界的硬指标。

**Exit**

- smoke 1：Local 模式 patch/restart/core-switch rollback；
- smoke 2：Windows v1 daemon 自动升级后进入 v2 Service 模式；拒绝升级时 fail-closed Local；
- smoke 3：macOS TUN 开关与 DNS 恢复；
- fixed-port 占用作为自动化集成测试，不要求单独手工 smoke；
- `test_real_dirs == 0`。

## 最终删除清单

- [ ] app `rebuild_gate` / `clash_patch_gate`；
- [ ] legacy lifecycle mutex / borrowed `CoreLifecycleLease` implementation；
- [ ] CoreEngine/Engine\* 计划类型（不实施）；
- [ ] actor 二次恢复策略（不实施）；
- [ ] RunningConfigPatchPort 与 GUI clash-api patch 补偿；
- [ ] ChangeCore 专用回滚编排和专用 wire；
- [ ] Logger global 与 service 状态轮询 statics。
