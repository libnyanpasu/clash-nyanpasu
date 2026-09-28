# T10 启动、后台源、恢复与关闭生命周期

基线：L2 `refactor/remove-legacy-config` 的 head（其下 L1 `refactor/typed-config-ipc`，起点 `main@4f59ca781`）。实施分支 `feat/tcc-startup-shutdown`，本地 stacked，未推送、未合并。设计：[`docs/superpowers/specs/2026-09-25-tcc-t10-lifecycle/design.md`](../superpowers/specs/2026-09-25-tcc-t10-lifecycle/design.md)（提交 `docs(tcc): design the T10 startup and shutdown lifecycle`，codex 评审四轮后批准）。

## 范围与假设

- 按设计 §11 拆成四个任务，顺序为 6a1 → 6a2 → 6b → 7，每个提交单独构建：
  - 6a1：写前恢复模型；
  - 6a2：StartupReconcile、所有权与 lifecycle 恢复；
  - 6b：后台生产者门控与来源回执；
  - 7：单飞有序关闭。
- 源状态仍由各域 actor 持有，workflow 只读快照。生产者任务、下载和 widget 子进程由注入的 `ProducerTasks` 或所属 actor 持有，不新增 `::global()` 或可变 static。
- 不改 `nyanpasu-core`、`nyanpasu-runtime`，不新增 IPC。UI 只在已有的 Configuration status 中增加来源行。

## 提交

- 6a1：
  - `fix(service): own elevated command helpers past their timeout`
  - `fix(workflow): recover isolation from a live attempt and pending action`
  - `feat(workflow): count dependency waits on deferred targets apart`
  - `test(workflow): interrupt attempts at every stage and recover them`
- 6a2：
  - `feat(workflow): reconcile the runtime at startup under a proven owner`
  - `refactor(workflow): remove the boot probe and host restore commands`
- 6b：
  - `feat(profiles): hold background producers until startup has reconciled`
  - `feat(setup): track the boundary producers so shutdown can stop them`
  - `feat(profiles): report background source receipts in ConfigurationStatus`
- 7：
  - `fix(widget): keep the widget child owned until its exit is confirmed`
  - `feat(workflow): close admission for shutdown without stopping the core`
  - `feat(client): shut down in one ordered single-flight run on every exit`

## 实施要点

1. **写前恢复模型（6a1）**
   - workflow 持有一个 `LiveAttempt` 槽，facade 持有一个 `PendingAction` 槽。
   - `PendingAction` 在每个会改变外部状态的 await 之前写入；只在正面观测到完成，或确定未提交时清除。
   - 隔离的定义：执行域空闲，且任一槽非空。
   - ServiceActor 保留超时 helper 的句柄，并提供 `CommandSettled`。helper 未结束时，拒绝新的变更命令。
2. **StartupReconcile（6a2）**
   - `Command::StartupReconcile`（`client/application_workflow/startup.rs`）的流程：取证 → `plan_owner` → 满足 stop 意图，并停掉没有回执的残留实例 → `try_critical` 应用最新 desired → 恰好一次 `publish_full`。
   - 启动失败留下 Reestablish 目标，RetryNow 和重试预算照常生效。
   - `Ownership { Unproven, Established }`：显式启动补建所有权，内部和自动路径跳过或拒绝。
   - 删除的内容：启动时的 `probe_service` / `restore_execution_host` 命令、旧的启动 effects 管线；启动内部的 handoff 不再逐命令发 committed 通知（lifecycle 命令自身的通知保留，见 T11 记录）。
3. **后台源（6b）**
   - `ProducerGate { Held, Running, Stopped }`。composition root 在 StartupReconcile 之后调用 `start_background_sources`。
   - 下载受跟踪、可取消。刷新完成消息必须匹配 `RefreshAttemptToken`。
   - 每个 profile 保留最新一条 `SourceStatus`，投影到 `ConfigurationStatus.sources`；前端只渲染不健康的行。
4. **关闭（7）**
   - `NyanpasuClient::shutdown` 是单飞编排（`client/app_lifecycle.rs`），按计划 §11.3 的顺序执行：Closing 准入 → 停生产者 → 等待 tracked TCC → SealEffects → 恢复自有系统代理 → 按策略停 core → 最后保存 SessionState → drain actors。
   - 先发出全部请求，再等待确认；截止时间用饱和运算。结构化报告列出未完成项，重复调用返回首次报告。
   - typed client 只暴露不透明的 `begin_terminate()`。
   - widget 子进程在 spawn 后立即登记；删除阻塞的 `Drop`。

## 改变行为的 leader 裁定

- **R8**：期望 service 模式、启动时 daemon 却未就绪时，不启动 core。状态报 WaitingDependency 并自动重探，不静默回退到 Local。依据是 TCC 计划 §2.3、§11.2 和 V37：不启动所有权未证实的实例。代价：daemon 损坏的用户在修好 daemon 或关闭 service 模式之前没有代理。
- **R20**：用户 stop 之后的显式启动，只在必须重建所有权时撤回 stop 意图。所有权已建立时，显式启动保留原有的 reconcile 意图语义。两条路径成功时都必须启动 core，两条路径都有测试。
- **R21**：StartupReconcile 中途 panic，仍然恰好执行一次 `publish_full`（设计 S13）。托盘、热键、locale、logger、widget 不依赖启动成功。
- **R22**：定时 tick 并入在途刷新时不写回执，结果由在途尝试写入。否则 Superseded（Healthy）会覆盖 Failed / Rejected，隐藏失败（V33）。
- 结构性裁定：
  - R10：单一写前模型；
  - R17：ServiceCommand 的 BackendUnavailable 只在 `command_settled()` 为真时清除；
  - R18：gate 只回收、不探测；
  - R19：recover 保持剩余目标的健康和计划；
  - R23：不暴露 raw actor handle。

## 其他行为差异

- 前一会话遗留的 service core，在任何启动之前被停掉（desired Service）或交还（desired Local），始终只保留一个实例。
- 启动的 full publish 移到窗口创建之前；直接的 `update_systray` emit 删除，托盘首次由 `Tray(Full)` 构建。
- 所有权未证实时，显式重启（`restart_sidecar`、`enhance_profiles`、托盘）会重建所有者：先停掉没有回执的在跑 core，再启动。service 模式下 daemon 不在时，返回可重试的 BackendUnavailable。
- lifecycle 命令 panic 或丢失回复后，恢复走 reestablish，不再要求重启应用。收到 router 回复的 handoff 不再隔离，只有丢失回复（`Internal`）才隔离。
- 定时刷新 fetch panic 记为 Failed。此前该刷新会永远停在进行中。
- 关闭：
  - 主窗口几何改为最后保存，3 s 内未完成报 Incomplete；
  - 系统代理恢复在 settle 之后、core 停止之前；
  - `restart_application` 不再清理两次；
  - widget `apply(Disabled)` 有 3 s 上界；
  - Closing 本身不再停 core。
- `commit_file_first` 的一切错误都按失败的步骤归入 `subscription_rejected` / `external_source_rejected`，其中包括 persist IO 和版本冲突。已补记到设计 §3。

## 验证记录

各任务在自己的 head（上列该任务的最后一个提交）上运行（数字取自任务报告）：

- 6a1：`cargo test --manifest-path backend/Cargo.toml --all-features -p clash-nyanpasu --lib -- --test-threads=8` 为 817 passed、0 failed、1 ignored。recovery + facade + service_actor 连续 5 轮，每轮 54 passed。
- 6a2：同命令 849 passed、0 failed、1 ignored。startup + recovery 连续 5 轮，每轮 46 passed。
- 6b：同命令 868 passed、0 failed、1 ignored。`pnpm typecheck` 退出 0，`pnpm test:frontend` 38/38，bindings 导出通过且无 diff。
- 7：同命令 902 passed、0 failed、1 ignored。关闭覆盖集 64 passed。`rg -n "shutdown_core|shutdown_application_effects|impl Drop for WidgetManager|actor_cell" backend/tauri/src` 无输出。
- 设计 §6 测试矩阵列出的测试均已实现，包括 CCG 评审 1 补充的行。变异检查被测试捕获：6a1 6 项、6a2 13 项、6b 7 项、7 M1–M10。每轮 `pnpm lint:architecture-ledger` 均通过。
- 全栈最终验证在 L5 `refactor/tcc-t11-cleanup` 上运行，见 [T11 记录](2026-09-25-tcc-t11-implementation.md)：workspace 1253 passed、0 failed、2 ignored。

## 边界

- 以下实机 smoke 未执行，待维护者：service 模式、daemon 缺失、残留实例、关闭时序。不能用单元测试代替。
- 全部验证只在本机 macOS 上运行。L3 没有新增平台专属代码，但 Windows / Linux 的行为仍待 draft PR 推送后的三平台 CI。
- 已知未修的次要问题：converge 的 install 在冲突原因里标为 `EnsureReady`。
- 不承诺跨崩溃 exactly-once，不扫描进程，不在不重启的前提下复用被 adopt 的实例（设计 §10）。
- `application_workflow::tests::mutations::an_expired_ack_keeps_the_domain_until_the_handoff_is_compensated` 曾在 main 基线上、并发 cargo 负载下的 workspace 运行中挂起 30 分钟以上，同一用例在 `--lib` 运行中通过。T11 的最终 workspace 运行中它也通过；未定位挂起原因。
