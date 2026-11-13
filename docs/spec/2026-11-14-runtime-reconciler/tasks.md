# 运行时对账器：期望状态模型 任务

关联设计：`design.md`（同目录）。路径缩写同 design 开头的表。

## 0. 全局约束

1. 遵守 `AGENTS.md` 与 `docs/development/*`：不加全局单例；依赖显式注入；actor 的邮箱是唯一的串行化手段，不在 actor 内再造队列、调度或准入层；进程内调用不设超时；panic 不恢复。
2. 每个提交可编译、测试通过、只做一件事。调用方在同一提交迁完，不留 re-export、别名或兼容层。被删除的类型与它的全部消费者在同一个提交里处理。
3. 源 owner 不直接依赖 `client::application_workflow`（改名后为 `client::reconciler`）；它只消费 `S/mutation/ports.rs` 里的端口，也不读取或转发别的域的快照。`pnpm run lint:backend-boundaries` 与 `pnpm run lint:architecture-ledger` 每卡都跑。
4. Rust 代码按 `docs/development/rust.md`：新错误用模块自有的 snafu 变体；不写 `Manager`、`Helper`；含义不明的 `bool` 参数用枚举；不新增 `#[allow(dead_code)]`，被本卡变成无人使用的项在本卡删除。
5. 测试按 `docs/development/testing.md`：不用 sleep 同步；用回复、闸门或 `start_paused`。每个新测试在卡里写明“删掉哪一行生产代码会让它变红”。
6. 行为变化只允许出现在 design 第 4 节列出的行里。卡面没有写到的行为差异视为缺陷。
7. 规范文档跟着代码走：一张卡改变了 `docs/development/*.md` 描述的规则或模块，就在同一张卡里改文档。
8. 每张卡的本地检查：`cargo fmt --all -- --check`；`cargo clippy -p <动到的包> --all-targets --all-features` 不新增告警；动到的包的 `cargo test --all-features`；涉及 core 时再跑 `nyanpasu-rpc`、`nyanpasu-server`、`clash-nyanpasu` 三个包的测试。动到 `cfg(unix)` 代码时补 `cargo check --target x86_64-unknown-linux-gnu`。
9. 不修改 `backend/nyanpasu-runtime/` 子模块与 `A2/` 的分层；需要它们的新能力时停下并报告。

## 1. PR 划分

| PR   | 分支                              | 卡                      | 行为变化                         |
| ---- | --------------------------------- | ----------------------- | -------------------------------- |
| PR-0 | `docs/runtime-reconciler-spec`    | 本 spec                 | 无                               |
| PR-1 | `refactor/runtime-check-local`    | R1a、R1b、R1c           | 校验改在本地执行，不再限时 45 秒 |
| PR-2 | `refactor/commit-first-mutations` | R2a、R2b、R2c、R2d、R2e | design 第 4 节中与提交有关的各行 |
| PR-3 | `refactor/reconcile-loop`         | R3a、R3b、R3c           | design 第 4 节中与对账有关的各行 |
| PR-4 | `refactor/state-engine-shrink`    | R4a                     | 无                               |
| PR-5 | `refactor/runtime-status-wire`    | R5a、R5b、R5c           | 线缆上删除不再产生的值           |

各 PR 依次叠加。每个 PR 完成后做一次双模型审查，发现折回对应提交。

---

## R1a — 构建候选不再需要独占访问

- [ ] `C/runtime.rs`：`RuntimeRevisionAllocator` 改为可克隆的共享原子计数器，`allocate(&self)`；溢出仍然 panic。
- [ ] `AW/preparation.rs`：`RuntimePreparation` 的 `prepare`、`prepare_inputs`、`prepare_candidate_inputs`、`prepare_inputs_with_policy`、`prepare_committed` 改为 `&self`；`CL/ports.rs` 的 `RuntimePreparationPort::prepare_latest` 改为 `&self`，调用方与测试同步改签名。
- [ ] 不改行为。

**验证**

- 既有测试的清单不变（`cargo test -p nyanpasu-core --all-features -- --list --format terse` 排序后比较，只多出下面新增的一条），全部通过。
- 新增一个测试：两个克隆的分配器交替分配得到严格递增且不重复的修订号。删掉共享则重复。

## R1b — 校验走本地控制面

- [ ] `AW/adapters.rs`：`CoreCheckValidator` 持有本地端点（`CoreClient::initial_endpoint()`），不再调用 `connected_endpoint()`，不再为服务宿主写候选文件；文档内联传入。
- [ ] 删除应用层 45 秒的等待包装与 `CHECK_BUDGET`、`with_budget`：等待本地校验的真实结果。保留“core-manager 自己的校验超时不是拒绝”的区分。
- [ ] 删除因此无人使用的项：`RuntimeCheckUnavailable::NoEndpoint`、`HostUnsupported`、`CandidateUnavailable` 的产生点；若 `RuntimePaths::create_candidate` 不再有调用方则删除。线缆枚举本身留到 R5a。
- [ ] `AW/tcc.rs` 中 `CheckRecord::Skipped` 的分支随产生点消失而删除。
- [ ] 改写 `AW/tests/validation.rs`：服务托管内核时校验仍在本地端点执行（断言本地端点的 `checked()` 增加、服务端点为 0）；移除“宿主没有校验能力”与“宿主不回答时等待上限到期”两条用例。

**验证**

- 上述新断言；删掉“取本地端点”改回 `connected_endpoint()` 则变红。
- design 第 4 节“校验在哪里执行”一行之外没有行为变化：`AW/tests/` 其余测试不改断言即通过，改了的逐条列在提交说明里。

## R1c — 脚本模块缓存原子写入

- [ ] `tauri/src/adapters/script_extensions/http.rs` 与 `nyanpasu-server/src/script_extensions.rs`：缓存条目用“写临时文件再原子替换”发布（复用 `nyanpasu-persistence` 的原子写）；从缓存读到的条目不再原样写回。
- [ ] 读到无法解码的缓存条目时当作未命中，重新获取。

**验证**

- 每个 shell 一个测试：两个并发加载同一 URL，任何一方读到的都是完整条目（用注入的获取闸门制造交错，不用 sleep）。去掉原子替换改回直接写则可观察到截断读。
- 损坏的缓存文件被当作未命中。

## R2a — 把应用与独立校验分开

- [ ] `AW/tcc.rs`：`try_critical` 拆成两步：`check_candidate`（构建、解析二进制、独立校验）与 `apply_prepared`（宿主移动、提交、结果分类）。变更的 Try 依次调用两者；已提交目标的路径（`apply_committed`、`retry_committed_target`）只构建并调用 `apply_prepared`，不再做独立校验。
- [ ] `CheckRecord::Unserviceable` 对已提交目标的“等待依赖”分支随之删除。

**验证**

- 启动与重试路径：本地端点的 `checked()` 不再增加，`reconciled_bytes()` 不变。
- 变更路径的全部既有测试不改断言即通过。

## R2b — Application 域提交优先

- [ ] `S/mutation/ports.rs`：新增 `RuntimeGate`（`check`、`committed`）与 `RuntimeCandidate`（design 3.2）。`DomainChange` 从 `AW/mutation.rs` 移到 `S/mutation/`。
- [ ] `S/mutation/mod.rs`：`MutationCoordinator` 的 `Ready` 连接增加 `runtime: Arc<dyn RuntimeGate>` 与取用方法；测试用的隔离连接提供一个放行的替身。旧的 `participant()` 在 R2e 之前保留给尚未切换的域。
- [ ] `AW/check.rs`（新）：候选校验服务，实现 `RuntimeGate`（design 3.2 的五步）。它不经过 workflow 邮箱。`committed` 向 workflow 投递 `Message::SourceCommitted`。组合根（`C/mod.rs`）构造并注入。
- [ ] `AW/mod.rs`：新增 `Message::Checked` 与 `Message::SourceCommitted { domain, version }`。后者的处理：读取三域快照与引擎版本；workflow 被隔离或正在关闭时只发布结论；否则以已提交配置为目标做一次应用（R2a 的 `apply_prepared`；允许按接管规则换宿主；不扣自动预算；失败后沿用现有延期目标的调度）。目标身份等于 `Checked` 留下的候选时复用该候选。
- [ ] `AW/mod.rs`：新增状态 watch `ReconcilerStatus`（本卡需要 `seq`、`active`、`seen`、`conclusion`、`degradations`）。只有读取了已提交快照的处理更新 `seen`，值是它开始时读到的版本。客户端新增 `settled(domain, version)`（design 3.4：读取、等待 `changed()`、处理 watch 关闭）。
- [ ] 断连：workflow 记住上次生效的 mode 与 `Profiles.current`；新旧两条路径应用成功时都更新它；`SourceCommitted` 的应用按 design 3.7 判断并执行。
- [ ] daemon 释放：按 design 3.5“维护项”实现 `release_owed`，取代 Confirm 里的释放对 Application 域的作用。
- [ ] `S/application.rs`：提交序列改为 design 3.3。`ApplicationSnapshot` 携带引擎版本与 `reaches_runtime`，去掉内部组装的 `receipt` 与 `degradations`。
- [ ] `C/mod.rs`、`C/application.rs`：`patch_app_config` 在提交后按 design 3.4 等待并组装 `MutationOutcome`。新增 `NyanpasuClient::set_service_mode`（design 3.6）与 workflow 命令 `PrepareService`；`nyanpasu-rpc/src/commands/host.rs` 的 `set_service_mode` 改为调用它。
- [ ] 校验被拒绝映射为 `CommitAborted::RuntimeRefused { runtime: Untouched }`；校验不可用映射为 `RuntimeFailed { runtime: Untouched }`。

**测试**（均为新增或改写；每条写明守护的生产行）

- owner 级，`RuntimeGate` 用替身：
  - 影响运行时的 patch 依次调用 校验、写入、effects 通知、`committed`。
  - 校验拒绝：文件内容与版本不变，没有 effects 通知，没有 `committed`。
  - 写入失败：没有 `committed`。
  - 不影响运行时的 patch（如语言）：不调用校验，不调用 `committed`。
- `settled`：
  - 待决动作未结清时提交：等待以 `Unknown` 结束，不挂起。
  - workflow 停止后调用：以“已关闭”结束。
  - 一轮进行中另一个域提交：该轮结束时发布的版本不包含后到的提交，等待者继续等到下一轮。
- facade 级，真实 workflow 与 `TestControlEndpoint`：
  - 切换内核，端点拒绝应用：返回 `CommittedDegraded`，`runtime == Deferred`，已提交值是新内核，端点仍报告旧修订。
  - 切换内核，候选无效（构建失败）：返回 `RuntimeRefused`，已提交值不变，端点没有收到提交。
  - 打开服务模式，提权被拒绝：返回错误，偏好未提交。
  - 打开服务模式成功：daemon 的安装与启动发生在偏好提交之前；内核最终在服务宿主上。
  - 打开服务模式，daemon 就绪后候选无效：返回提交错误，偏好未提交，daemon 仍在运行。
  - 关闭服务模式：内核回到本地，daemon 被停止；内核已被用户停止时同样释放 daemon。
  - 一次应用被挂起时（现有的挂起端点），`patch_app_config` 修改语言立即完成。
- 混合状态：旧路径的 Clash 变更正在 Try 中挂起时，Application 提交完成并排在其后应用；旧变更的源写入失败并 Cancel 之后，Application 的应用仍然进行。

## R2c — Clash 域提交优先

- [ ] `S/clash_config.rs`：`Patch`、`PatchOverrides`、`Replace` 的提交序列改为 design 3.3。
- [ ] `C/mod.rs`、`C/clash_config.rs`：`patch_clash_config`、`patch_runtime_overrides` 按 design 3.4 组装结果。
- [ ] `C/mod.rs` 的 `patch_runtime_overrides`：patch 只包含 mode 且等于已提交的 mode 时，不提交，向 workflow 发 `InterruptConnections` 并把关闭失败作为降级项返回（design 3.7）。这是 RPC 与热键共用的入口；`C/hotkey/mod.rs` 不需要自己的分支。
- [ ] 把 `AW/tests/connection_policy.rs` 中经由 Clash 域的用例改为新路径下的可观察断言（端点事件顺序）。逐条处置按本文件第 2 节的表。

**测试**

- owner 级四条，同 R2b。
- facade 级：
  - 修改端口，端点处于过渡态：提交成功，结果 `Deferred`；端点回到运行态后重试即生效。
  - mode 由 rule 改为 global 且 `on_mode_change`：事件顺序为先应用后断连，只断一次。
  - 经由 `patch_runtime_overrides` 重复设置当前 mode：没有应用，连接被关闭一次。
  - patch 同时包含未变的 mode 与另一个变化的覆盖项：应用一次，连接不关闭。
  - 重复设置当前 mode 而关闭失败：返回 `CommittedDegraded`，带断连失败的降级项。
  - 应用被挂起时修改 Application 域的 `enable_builtin_enhanced`：其提交在应用结束之前完成（文件已写入、快照已发布）。这是“单一 actor 不阻塞其他域”的直接测试。

## R2d — Profiles 域提交优先

- [ ] `ProfileContentLock`（design 3.3）：组合根创建，注入 Profiles owner 与构建输入的捕获。捕获在一次读锁内读取已提交的 Profiles 快照与它引用的文件；owner 所有替换或补偿受管文件的路径（含物化恢复）都在写锁内。
- [ ] `S/profiles/actor.rs`：`persist_candidate` 一族改为 design 3.3 的序列。`commit_with_resources` 一族改为：取得候选字节 → 校验（`RuntimeCandidate::staged`）→ 准备资源与清理 → 持写锁执行本地写与文档写 → 发布 → 完成与延后清理。
- [ ] 计划内的订阅刷新、外部文件变化同样先校验后提交，并由 owner 自己发出 `committed`；校验拒绝记录到该来源的 `sources` 条目，位置与今天 Try 被拒绝时相同。来源条目里的 `operation_id` 沿用 owner 生成的提交标识。
- [ ] `C/profiles.rs`、`C/mod.rs`：`after_commit` 一族按 design 3.4 组装结果；`create_profile`、`import_profile` 的自动激活沿用。
- [ ] 改写 `AW/tests/connection_policy.rs` 中经由 Profiles 域的用例，处置按第 2 节的表。

**测试**

- owner 级：
  - 保存当前 profile 的文件内容：校验收到的 `staged` 是新内容；校验拒绝时磁盘上的文件与 `profiles.yaml` 都不变，没有遗留的暂存物。
  - 保存不在当前闭包内的 profile：不校验。
  - 激活 profile：校验、写入、通知的顺序。
  - 文件已替换、文档写入被挂住时发起内容捕获：写入失败的情形下，捕获在补偿完成后才返回，读到旧文档与旧内容；写入成功的情形下，读到新文档与新内容。两种情形都不会得到“旧文档配新内容”。
- facade 级：
  - 激活的 profile 通过校验但端点拒绝应用：已提交的 `current` 是新 profile，结果 `Deferred`，端点仍是旧修订。
  - 激活的 profile 无效：`RuntimeRefused`，`current` 不变。
  - 订阅刷新得到无效内容：不提交，来源状态记录拒绝原因。
  - 选择变化且 `on_profile_change`：先应用后断连；实例被替换时不断连。

## R2e — 删除参与者路径

- [ ] 删除：`AW/participant.rs`；`AW/policy.rs`；`AW/tcc.rs` 中的 `run_mutation`、`check_candidate`、`await_decision`、`confirm`、`cancel`、`restore`、`compensate_handoff`、`restore_recovery_target`、三种 `EvidenceGap` 拒绝、`switching_on` 分支；`AW/attempt.rs` 中的 `SourceDecision` 来源、`AppliedVerdict` 与 `recover_source_decision`；`AW/mutation.rs` 中的 `MutationRequest`、`RuntimePrepareOutcome::ack`、未被构造的阶段变体；`Message::BeginMutation`。
- [ ] 删除：`S/mutation/` 的 `MutationParticipantFactory`、`MutationIntent`、`CommandClass`、`MutationHints`、`RequestedRuntimeFields`、`ActivationIntent`、`ContentDigest`、`StagedResourceToken`、`RuntimeImpact`、`MutationReceipt`、`MutationOutcomeKind`、`MutationConclusion`、`CheckRecord`、`Settlement`、`MutationCoordinator::{participant, committed}`、`classify_commit_aborted` 的参与者分支、测试用的 `IsolatedParticipant`。
- [ ] 同一提交迁移它们的消费者：`MutationJournal.completed` 改为最近轮次的记录，`C/configuration_status.rs` 的 `recent_operations` 从它投影；`C/mod.rs:1862` 一带的 `ParkedPrepare` 测试改为挂住 `RuntimeGate` 的替身；`C/runtime_error.rs` 的 `ack_of`、`refusal_of` 删除。
- [ ] `S/mutation/impact.rs`：留下三个域的投影、`application_target` 等三个身份函数、`selection_changed`，以及 `reaches_runtime(previous, candidate, touched)`。三个 owner 改用它。
- [ ] `apply_prepared` 的返回类型收窄为 `Applied | Refused | Waiting | Unknown`，不再带策略与可恢复基线。
- [ ] 删除 `AW/tests/participant_factory.rs`；`AW/tests/mutations.rs` 中断言回执、决定、Cancel、证据拒绝与命令策略的用例删除，其余改为经由 facade 或 owner 的断言；`AW/tests/closing.rs` 改写三条依赖 Try 阶段的用例，并新增“关闭时已提交、尚未应用”的结果。
- [ ] 文档：`docs/development/architecture.md` “State and configuration” 第 4 条改为“提交前的校验不改变运行中的任何东西；运行中的内核由对账器收敛到已提交配置”，并改写提到参与者工厂与两种内部回执的段落；`docs/development/backend-packages.md:106` 一带同步；`docs/development/rust.md` 5.2 中引用 `tcc.rs` 的 `explicit: bool` 的例子保留到 R3b。

**验证**

- `rg -n "DecisionHandle|StateParticipant|replace_if_version_with_participant|on_prepare" backend/nyanpasu-core/src` 无匹配（退出码 1）。
- `rg -n "MutationHints|CommandClass|CommandPolicy|EvidenceGap" backend/nyanpasu-core/src` 只剩线缆类型的引用，逐条列在提交说明里。已提交目标的恢复仍在使用的基线（`RestorableBaseline`、`LiveAttempt`）留到 R3b。
- 同值重提（patch 里出现运行时字段但值不变）不调用校验、不触发应用。
- design 第 4 节 PR-2 负责的各行各有一个通过的 facade 级测试，清单写进 PR 描述。

## R3a — 一套应用顺序

- [ ] `apply_prepared` 按 design 3.5“应用与发布”的矩阵实现每一种结果的记录、端口与产物处理，包括 `NotSubmitted` 的两类。
- [ ] `Start` 命令在所有者已证明时、服务端点恢复后的还原、替换内核二进制后的重启改为“已提交目标的一次应用”：编排上移到 workflow，在该命令已有的那个 `LiveAttempt` 之内调用 `apply_prepared`，不另开尝试、不经过重试入口。它们不再调用 `CL/workflow.rs` 的 `reconcile` 与 `apply_runtime`；这两个函数与 `RuntimePreparationPort::prepare_latest` 删除，仍调用它们的测试（`AW/tests/connection_policy.rs:1018`、`AW/tests/validation.rs` 六处）改用保留的构建入口。
- [ ] 替换二进制：停止、隔离恢复、安装三步不变；欠下重启时通知进度“正在重启”后应用；所有权未建立时由现有的重建路径处理，`Output::RestartWithheld` 删除。
- [ ] 回执核对改为 design 3.5“回执核对”的规则（不比较路由代数）。

**验证**

- `rg -n "fn apply_runtime|prepare_latest|RestartWithheld" backend/nyanpasu-core/src` 无匹配。
- `Start` 在所有者已证明时：运行时产物文件在端点接受之后才写出（现有的记录型构建端口可断言顺序）。
- 应用成功后已确认的端口可读（过渡已关闭）；回滚后仍是旧端口；结果未知后不可读。
- 连接断开又接上同一个运行中的内核：没有停止，没有应用，之后的保存正常应用。
- `NotSubmitted(Internal)`：记为被拒绝；之后的节拍里端点没有收到新的提交。

## R3b — 单一对账循环

- [ ] `AW/plan.rs`（新）：`Facts`、`CoreFact`、`Step`、`Conclusion`、`Trigger` 与纯函数 `plan`（design 3.5 的判定表）；现有的 `plan_owner`、`OwnerPlan`、`ServiceEvidence` 移入本文件。
- [ ] `AW/mod.rs`：`Command` 改为 design 3.5 的命令集；`StartupReconcile`、`FollowService`、`RetryRuntime`、`SourceCommitted`、`ServiceReadiness` 都执行 `reconcile(trigger)`。`Startup` 只运行一次，之后返回第一次的报告；首轮无论结论如何都 `publish_full` 一次。
- [ ] `AW/startup.rs` 重排为观察与执行：一轮的八步（design 3.5）；`Owner(..)` 与 `Stop { .. }` 的执行沿用现有的 `adopt_ready_service`、`move_execution_host`、`stop_owner`。
- [ ] 记录按 design 3.5：`Attempt` 取代 `DeferredTarget`；`run` 取代从观察推断的 `StoppedByUser` 与 `CoreIntent::Stopped`、`CoreIntent::Restore`；删除 `LiveAttempt`、`AttemptStage`、`AttemptOrigin`、隔离判定、`recover`、`recover_by_reestablishing`、`retry_committed_target`、`retry_reestablish`、`AttemptCharge`、`DEFERRED_RETRY_BUDGET`、收敛定时器，以及 `C/convergence.rs` 中只为运行时调度服务的部分（`REESTABLISH_WAIT_DELAYS`、`OutcomeClass`、`next_wait`）。Effects 仍在使用的 `RetryBudget` 与 `RETRY_DELAYS` 保留。
- [ ] 每轮开始先 `consume_settled_action()`；未结清时结论为 `Unknown`。“连续两次未知”按 design 3.5 计数。
- [ ] `Tick`：按 design 3.5“触发”的两条实现。服务端点恢复的到期条件改为只看端点是否处于 `Degraded { desired: Service }`（`ServiceRecovery` 删除 `intent` 字段，其余不变）；重连成功后跑一轮。上一轮结论为 `Waiting` 或 `Unknown` 时再跑一轮。服务端点恢复的次数上限、抑制与重新武装的既有测试保留。
- [ ] 回执核对通过的运行实例：重新记为已确认并关闭端口过渡（design 3.5“回执核对”）。
- [ ] `BinaryReplaced` 欠下重启时把 `run` 设为 Running。
- [ ] 维护项按 design 3.5。
- [ ] `C/configuration_status.rs`：从 `ReconcilerStatus` 投影；`MutationJournal` 删除。`subscribe_configuration_changes` 的第一个接收端改为 `ReconcilerStatus` 的 watch，`nyanpasu-rpc` 的转发与本地订阅同步改类型。
- [ ] `StartupReport` 保留；`StartupOutcome` 由首轮结论映射：`Converged`、`Inactive` → `Ready`；`Waiting`、`NotInEffect` → `ReadyDegraded`；`Unknown` → `RecoveryRequired`。消费者同步：`C/app_lifecycle.rs`、`C/test_support.rs`、`tauri/src/utils/resolve.rs`、`nyanpasu-server/src/run.rs` 与它的重复启动测试。
- [ ] 文档：`docs/development/rust.md` 5.2 的 `explicit: bool` 例子改为仍然存在的例子或删除。

**测试**

- `plan` 的表驱动测试：判定表九行各至少一格；`plan_owner` 八行原样保留；触发种类 × 上次尝试（无、同目标被拒绝、同目标未知、不同目标被拒绝、同目标不同宿主被拒绝）的组合；服务端点读不到而 daemon 已确认停止时得到 `Owner(HandBackToLocal)`。
- `AW/tests/startup.rs`、`recovery.rs`、`service_recovery.rs`、`mod.rs`：断言改为可观察结果（端点调用顺序、状态结论、所有权）。依赖预算、退避序列与隔离拒绝的用例删除，逐条列在提交说明里并注明对应 design 第 4 节的哪一行。
- 新增：
  - 应用失败后不自动重试：推进时间 60 秒，端点没有新的提交；显式重试后有一次。
  - 结果未知后自动继续：结清动作后的下一个节拍重新应用；第二次未知后停在 `NotInEffect`。
  - 内核以失败原因停止，目标未变：不再启动；提交新目标后启动。
  - 过渡态：结论 `Waiting`，节拍到来且状态稳定后应用。
  - 服务宿主上启动失败：回落到本地并在本地运行；旧配置仍在服务宿主上运行时的应用被拒绝则不回落。
  - 显式启动失败后提交新目标：内核启动。
  - 关闭服务模式而内核已被用户停止：daemon 被释放。
  - 强制重新应用当前目标得到未知结果，结清后旧实例未变：结论 `Converged`，已确认的端口可读。
  - 显式停止后替换选中内核的二进制：内核启动；替换未选中的内核：不启动。
  - 服务宿主上的内核无故停止：先在服务宿主上启动；启动失败后回落本地。

## R3c — 内核状态作为触发

- [ ] `AW/mod.rs`：订阅 `CoreObserver` 的 watch，按 `(host, generation, connectivity, state, revision)` 去重后投递 `Message::CoreChanged`；关闭时退出。
- [ ] 对账器自己引起的状态变化不需要特殊处理：再跑一轮得到 `Idle(Converged)`。

**测试**

- 端点从过渡态变为运行态并发布：不等节拍就应用了待生效的目标。
- 端点连续发布相同状态 10 次：没有新的一轮（状态 `seq` 不变）。
- 内核以失败原因停止并发布：一轮之后结论为 `NotInEffect`，端点没有收到启动。

## R4a — 收缩状态引擎

- [ ] 先盘点：列出 `E/` 的每个公开项在 `backend/` 内的生产消费者与测试消费者，写进提交说明。
- [ ] 删除没有生产消费者的部分：单次参与者、`DecisionHandle` 与 `StateDecision`、`replace_if_version_with_participant`、只为参与者存在的 ACK 策略与回滚通知。保留 `PersistentStateManager` 的加载、`upsert`、`replace_if_version`、`replace_if_version_with_local_write`，快照与版本；“写入返回之前快照已发布”的顺序保持不变并有测试。
- [ ] 常驻订阅者机制若只剩测试消费者，一并删除，测试随之删除或改为针对保留的接口。`upsert` 返回的 `PrepareReport` 没有消费者时改为 `()`，调用方同步。
- [ ] 除上一条点名的返回类型外，不改保留接口的签名。

**验证**

- `cargo test -p nyanpasu-state-engine` 通过；`cargo test -p nyanpasu-core --all-features` 的测试清单与 R3c 之后相同。
- `pnpm run lint:backend-boundaries` 通过。

## R5a — 线缆

- [ ] `V/`：删除不再产生的值：`RuntimeAftermath` 的 `RolledBack`、`RollbackFailed`、`Unknown`（只剩一个变体时删除该类型及携带它的字段）；`EvidenceGap`；`RuntimeCheckUnavailable` 中不再产生的变体；`RuntimeError` 中不再构造的变体（`Isolated`、`SourceSettled`、`UnsettledBaseline`、`CommittedAfterRefusal` 等，以编译器与 `rg` 的结果为准）；`RuntimeCommitStatus::RecoveryRequired` 与 `DegradationReason::RuntimeRecoveryRequired`（并入 `Deferred`）；`RuntimeConvergence` 的 `attempts`、`automatic_remaining`；没有生产者的 `DegradationPhase`；`OperationStatus.conclusion`。`ConvergenceHealth` 与 effects 共用，只删除两边都不再产生的值。
- [ ] `nyanpasu-rpc-protocol` 的接收镜像、`receiving_tests`、契约基线（新增带日期的基线文件，不改旧文件）同步。
- [ ] 重新导出 bindings；`frontend/nyanpasu/src/utils/ipc-error.ts`、调试页 `configuration-status.tsx` 与六种语言的文案同步，删除不再可达的文案键。

**验证**

- 每个被删除的变体：按“类型名::变体名”与生成的 TypeScript 标签在 `backend/` 与 `frontend/` 手写代码中搜索，无匹配。
- `pnpm typecheck`（逐包）、前端单元测试、`pnpm lint` 通过。

## R5b — 规范文档收尾

- [ ] 通读 `docs/development/architecture.md`、`backend-packages.md`、`rust.md`、`testing.md`，改掉仍描述旧模型的句子。
- [ ] `AGENTS.md` 不复述被改的规则，确认无需改动。

## R5c — 模块改名

- [ ] `client/application_workflow/` → `client/reconciler/`；`ApplicationWorkflowClient` → `RuntimeReconcilerClient`，actor、参数、测试夹具与其他 crate 的引用同步。`core_lifecycle/` 不动。
- [ ] `scripts/src/architecture-ledger/` 中引用旧路径的策略与夹具、`docs/development/*.md` 同步；`pnpm run test:scripts`、`pnpm run test:architecture-ledger` 通过。

**验证**

- `rg -n "application_workflow|ApplicationWorkflow" backend scripts docs/development` 无匹配。
- 改名前后 `cargo test -p nyanpasu-core --all-features -- --list --format terse` 的清单只有模块前缀不同。

## 2. `connection_policy.rs` 二十条用例的处置

| 用例                                                                                  | 处置                                                                          |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `mode_interruption_follows_same_instance_reconcile_once`                              | 第二次不再应用；仍关闭一次连接（R2c）                                         |
| `disabled_policy_and_non_mode_patches_do_not_close_connections`                       | 不变                                                                          |
| `failed_reconcile_never_closes_connections`                                           | 不变                                                                          |
| `replacement_at_the_same_url_never_receives_source_interruption`                      | 不变，含“替换未被报告”的降级                                                  |
| `close_failure_is_committed_degraded_and_not_replayed`                                | 不变                                                                          |
| `missing_source_is_degraded_but_confirmed_stopped_startup_needs_no_close`             | 运行中的来源缺失仍是降级；对已停止内核的断言随“意外停止后保存”一行改变（R3b） |
| `lifecycle_work_cannot_overtake_pending_interruption`                                 | 运行时动作的顺序不变；源提交可以更早完成                                      |
| `controller_rotation_invalidates_source_without_closing_through_new_credentials`      | 不变                                                                          |
| `profile_activation_and_deselection_interrupt_only_actual_current_changes`            | 实际变化仍关闭；重复选择不再应用                                              |
| `profile_autoactivation_uses_policy_and_does_not_interrupt_an_existing_selection`     | 不变                                                                          |
| `profile_reconcile_rejects_selection_but_interruption_failure_preserves_commit`       | 应用失败的断言按“通过校验但应用失败”一行改变；关闭失败的断言不变              |
| `profile_replacement_never_receives_source_interruption`                              | 不变                                                                          |
| `profile_mutations_cannot_overtake_pending_interruption`                              | 运行时顺序不变；对旧快照的断言按提交优先改变                                  |
| `rejected_profile_activation_does_not_reconcile_or_interrupt`                         | 不变                                                                          |
| `profile_missing_source_is_degraded_but_stopped_core_needs_no_interruption`           | 同第六条                                                                      |
| `profile_policy_and_noop_gates_do_not_acquire_a_source`                               | 不取得来源的断言不变；多余应用的次数减少                                      |
| `cancelled_profile_waiter_keeps_admission_and_reconcile_does_not_replay_interruption` | owner 发起的工作仍然完成、不重放；去掉对准入与旧回执的断言                    |
| `shutdown_rejects_a_queued_profile_apply_and_waits_for_close`                         | 拆成两条：关闭时尚未提交的被拒绝；已提交而应用排队的返回“已关闭”              |
| `profile_interruption_serializes_mode_host_and_binary_operations`                     | 宿主、二进制与运行时动作仍然串行；“Clash 源未变”的断言按提交优先改变          |
| `committed_runtime_inputs_survive_newer_profile_and_channel_state`                    | 冻结输入的行为不变；改用保留的构建入口（R3a）                                 |
