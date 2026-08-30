# v2 实施审计入口（2026-08-13，停止线交付）

按用户指令实施到 **legacy bridge 阶段之前停止**。本文是审计的入口清单：代码在哪、对照哪节设计、测试怎么跑、哪些是已知偏离与限制。旧代码零改动（除 `core/mod.rs` 一行模块声明与 Cargo 依赖增行）、Tauri commands 未换线、v1 wire 未删。

## 1. 代码位置

| 分支                                                                 | 内容                                                      | 提交                                                                                                                                                                                                    |
| -------------------------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| submodule runtime `main`（`feat/core-control-plane` 已由 #391 合入） | PR-A+PR-B 全部 + 2026-08-27 修复轮                        | gitlink 现为 `6717e44`，即 #391 的 merge commit，其 tree 与被合入的 `e523ada` 相同；`git -C backend/nyanpasu-runtime describe --tags` 报 `v2.0.0-rc.1-29-g6717e44`，即自 `v2.0.0-rc.1` 标签起 29 个提交 |
| app `refactor/core-actor-v2`（自 `pr5/1-pre` tip）                   | C1/C2/C3/D1 + 2026-08-27/2026-08-30 两轮复审修复（见 §5） | 分支已重组为 7 个原子提交，不再按 hash 索引（见 §5 顶部说明）                                                                                                                                           |

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
| `tests.rs` + service_actor 内联 tests | —                               | 54 个测试全绿（含 2026-08-30 复审轮新增回归测试；见 §2）                                                                                                                   |

## 2. 测试跑法

```bash
# runtime 仓（2026-08-30 本轮未改动 submodule，计数不变：484 全绿 / 24 ignored；PowerShell 下加 --config build.rustc-wrapper=''）
cd backend/nyanpasu-runtime && cargo test --workspace --all-features
# 重点套件：control_plane / fake_backend / dns_override / reconcile / routing

# app 仓（2026-08-30 复审轮后：447 passed / 0 failed / 1 ignored；其中 core::actor_v2::* 54 个、core::service::*（compat+ipc）14 个）
cd backend && cargo build -p fake-core   # process_core_bridge 的 11 个测试需要它，否则必红
cd backend && cargo test -p clash-nyanpasu --lib

# `cargo check` 在本 workspace 不可用（确定性 ICE，见 §5 已知环境问题）；要纯编译检查改用：
cd backend && cargo build -p clash-nyanpasu --lib
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

| #   | 事项                                                                                                                                                                                                                                                                                                                                                                                                                    | 去向                                                                        |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| L1  | macOS DNS scutil 实现在 Windows 上零编译零验证；仅结构+fake 测试可信。修复轮补了 `kill_on_drop`（编排超时丢弃 future 时不留子进程），**这不构成验证**                                                                                                                                                                                                                                                                   | Phase-0 spike（判据在 `dns.rs` doc）                                        |
| L2  | quarantine recovery 的死亡证明走 pid-file（进程后端机制），未经 RuntimeBackend trait——fake backend 下 Recover 诚实地失败                                                                                                                                                                                                                                                                                                | 后续把证明路由进 trait（`fake_backend.rs` 测试注释）                        |
| L3  | `CoreKind→CoreType` wire 映射有损（alpha 通道塌缩、Meow 无 wire 表示）；bridge 阶段 facade 须携带 intent 的原 `CoreType`                                                                                                                                                                                                                                                                                                | `endpoint.rs::app_core_kind_to_type` doc                                    |
| L4  | facade 编排（reconcile_core/service_mode 时序/shutdown shared-future）**未实现**——§6.4 时序已定，属 bridge 前的最后一块                                                                                                                                                                                                                                                                                                 | 审计后实施                                                                  |
| L5  | ~~v2 submit admission 错误经 R 信封丢 retryable~~ **已收口**：R 信封加法字段 `retryable`（R6），app 侧 `map_client_error` 保真（Step 7.2）；旧 daemon 无该字段时回落到 `CoreErrorKind::default_retryable`                                                                                                                                                                                                               | 已闭                                                                        |
| L6  | app 共享 target 曾出现损坏 rlib+usvg ICE（环境问题非代码），停止线的两个 app 提交按既有先例 `--no-verify`。修复轮的三个 app 提交未再 `--no-verify`：pre-commit 的 `clippy --all-targets --all-features` 与 `fmt` 均实跑通过。**2026-08-30 复审轮的提交同样未 `--no-verify`**：共享 target 又出现 `拒绝访问 (os error 5)` 式增量损坏，改用干净的隔离 target dir 让 pre-commit 的 clippy 真跑通过（详见 §5 已知环境问题） | 用户知悉                                                                    |
| L7  | Service 泵/事件流为轮询+断线报告；daemon ws 事件流接入留待 PR-D 精化                                                                                                                                                                                                                                                                                                                                                    | OQ-6                                                                        |
| L8  | `DnsController` 契约仅支持**结构性拥有**的 override（restore 删除自有产物，不回放 `record.previous`）。写回型机制（`networksetup -setdnsservers` 一类）在"首次 apply 副作用已落地但调用未返回"的窗口内不可恢复：预记录此时还没有基线可写回                                                                                                                                                                              | 加入写回型控制器前先做 prepare/commit 拆分（`dns.rs` trait doc 有完整判据） |
| L9  | uninstall 与 service 侧 `Reconcile` 跨两个 actor 的竞态：`ServiceActor` 的 uninstall 守卫与 daemon 自己的 control plane 是两个独立的所有者，守卫探测和 uninstall 之间被放行的一次 `Reconcile` 不会被排除。今天仅靠 facade 时序封闭（uninstall 只有从 Service 交接走之后才可达）；结构性互斥是 bridge 阶段的工作                                                                                                         | bridge 阶段实施结构性互斥                                                   |
| L10 | runtime submodule 的 `nyanpasu_service/Cargo.toml` 仍声明 `2.0.0-rc.1`，而 gitlink 已领先 28 个提交；`scripts/check.ts:674-710` 按该 crate 版本号推导 sidecar 下载，于是 `pnpm prepare:check` 拉到的是 rc.1 daemon——比 app 编译所依赖的 IPC 协议更旧的二进制，却能通过只比较 major 版本的 compat gate（major == 2），同时缺 `/v2/core/*` 路由。今天无害（actor_v2 未接线，出货路径是 v1）；bridge 一旦落地即成活陷阱    | 修复是 submodule 版本号提升，非本仓改动                                     |
| L11 | 调用方预算只框定一条消息自身的执行分支耗时，从不框定它在串行、无界 mailbox 里排在其它消息后面等待的时间。给一个无深度上限的队列定一个"正确"的有限数字没有依据，所以改由超时的**错误**携带恢复契约：submit 的超时可重试，并点名同 id 幂等重试；handoff、shutdown 与需要提权的 service 命令仍不可重试，但会声明工作可能仍在进行中                                                                                         | 设计已定案，不再谋求扩大调用方超时以覆盖排队（见 §5）                       |
| L12 | ledger 词法器现在能正确处理注释、转义、字符字面量与生命周期撇号的区分、原始字符串（`r`/`br`/`cr`，任意 `#` 计数）与嵌套块注释，但一个跨行的双引号字符串（内含真实换行）仍不能跨行延续，且大括号/条目扫描器每次调用都从全新词法状态起步。`backend/` 下今天不存在这两种形状                                                                                                                                               | 若出现需扩展词法状态跨调用保持                                              |
| L13 | **已接受的 Minor**：`Connected` 状态下连续两次 shutdown，第二个调用方会收到"router 已消失"；真正的修复需要一个终态 `ShutDown` slot 状态，属 bridge 阶段工作。今天因退出时只有单一 facade 调用点而不可达                                                                                                                                                                                                                 | bridge 阶段补终态 `ShutDown` slot state                                     |

## 5. 审查发现 → 修复提交映射（2026-08-27 修复轮）

停止线交付后的 v2 实施审查（计划 `.claude/plan/v2-review-remediation.md`）逐项落地。每个修复都带一个**先在基线上失败**的回归测试；下表是审计从"发现编号"直达"改了什么"的索引。

**app 侧提交索引说明**：app 分支已重组为 7 个原子提交，本节及全文不再以 app 侧提交哈希索引——app 仓 actor_v2 的全部修复（含下表与「2026-08-30 复审轮（codex reviewer ×3 轮）」）收敛进单个提交 `feat(core): add CoreActor v2, the ServiceActor, and the runtime intent builder (unwired)`；compat gate 相关修复收敛进 `feat(service): add a fail-closed daemon compat gate`。runtime 仓（submodule）的提交哈希不受影响，稳定，仍如下表按哈希索引。

### runtime 仓（`feat/core-control-plane`，已随 #391 合入 `main`；下列哈希在 `main` 上仍然可达）

| 提交      | 发现                       | 修复                                                                                                                                                                                                                                                                                                                                                                            |
| --------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `0580bac` | M-1 / Minor-R1/R2/R3       | admission 与入队合并进同一临界区（失败即回滚，不留幽灵条目）；registry 容量硬钳到 `queue+1`；`expected_digest` 进身份（纠正后的同 id 是冲突不是附着）；check 源文件按 payload 摘要+操作 id 命名                                                                                                                                                                                 |
| `4e70b15` | M-2                        | executor 逐操作 `tokio::spawn`：一次 panic 不再让每个在途操作永远 Queued；`ExecutorExit::Died` 排空队列并给出终态                                                                                                                                                                                                                                                               |
| `070cfae` | C-2                        | daemon shutdown 走 executor（closing latch 对新提交生效），server `select!` 增第三臂监督 executor 退出                                                                                                                                                                                                                                                                          |
| `452cd75` | M-4                        | 线上 core type echo 只在"新注册且非 RolledBack 的 reconcile"上提交                                                                                                                                                                                                                                                                                                              |
| `c4155b9` | M-5 / M-6 / M-7 / L8 / D-1 | DNS 记录原子发布 + fail-closed（写不进就不 override）；`previous` 基线只在首次取得所有权时捕获；每次控制器调用（含建构期 orphan restore）都有 `dns_timeout` 上界；被拒绝且什么都没改的事务跳过 converge，失败且什么都没留下的事务仍 converge                                                                                                                                    |
| `18169b7` | M-9 前置 / L5              | `R` 信封加法携带 `retryable`；`CoreErrorKind::default_retryable` 提升为公有                                                                                                                                                                                                                                                                                                     |
| `4311ed8` | 复审 R#1/2/3/4/5           | `Registry::set` 改 `send_replace`（`send` 在无接收者时**不写入**，fire-and-forget 提交的操作会永远停在 `Queued`）；drain 改 `recv().await`（`close()` 不作废已发出的 `reserve` 许可，`try_recv` 会漏掉）；echo 按 admission 临界区内分配的执行序号应用；DNS 基线判据改为“记录带 interface”（`RestorePending` 与空 `previous` 都是合法基线）；记录发布加 flush/sync_all/sync_dir |

### app 仓 `refactor/core-actor-v2`

| 改动位置                                  | 发现                                                | 修复                                                                                                                                                                                                                                                                                                                                                                               |
| ----------------------------------------- | --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`（状态与错误映射）                | C-1 / M-9 / Minor-A1                                | `CoreStatusSnapshot.state` 变 `Option`（未知不再被合成为 `Stopped`，因而不再冒充停止证明）；`map_client_error` 保真服务端 kind 与 retryable，未知 kind 保持 `None`；停止证明要求 `Stopped` 输出，失败侧沿用线上 retryable                                                                                                                                                          |
| `mod.rs`（handoff 相位）                  | M-7 / CS-3 / Missed-4 / C-3 / M-12 / M-8 / Minor-A2 | handoff 显式相位 `HandingOff` + `HandoffStopped` 续传（期间 submit 立即拒绝而非排队）；handoff 期间的 shutdown 延后到同一个 stop 结算；Degraded 跨 host 采纳被拒（无证明）；`ShutdownReport.stop` 变 `Result`；adopt 清空快照；泵/预检有界 + `post_stop`                                                                                                                           |
| `service_actor.rs`（Uninstall 守卫）      | M-10 / M-11 / Minor-A3                              | Uninstall 守卫读 `CoreStateDetail` 且未知即拒（粗态 `CoreState` 不参与判断）；`Exhausted` 成严格闩锁；过渡相位失败后回探                                                                                                                                                                                                                                                           |
| `mod.rs` + `service_actor.rs`（复审修复） | 复审 A#1~A#6                                        | stop 的每次调用都有上界（`stop_wait` 也改为可注入）；admission 与终态都要求 operation id 相符；`pending_shutdown` 改 `Vec`（第二个 shutdown 曾顶掉第一个的 reply port）；handoff 任务收归 actor state 并在 `post_stop` 取消；Uninstall 的“探不到/无 detail”两种未知改为 `kind: None`（`AlreadyRunning` 是断言而非未知）；FakeEndpoint 的 admission 统一为 `Queued`，闸门移到长轮询 |

### 复审（Phase 5，codex reviewer 双仓各一轮）

runtime 侧 `4311ed8` 与 app 侧「`mod.rs` + `service_actor.rs`（复审修复）」行（见上表）来自对上表全部改动的独立复审。复审确认并已修的高危项见上表；下面是复审提出但本轮**未做**的，连同理由：

| 项                                                      | 复审意见                                                                                       | 处置                                                                                                                                                                                                                                                                     |
| ------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| M-1 的回归测试不判别                                    | `a_full_queue_answers_queue_full` 在旧实现下同样通过（旧代码入队失败后也会 `registry.remove`） | 属实。竞态窗口由结构消除；可判别的测试需要在“注册”与“入队失败”之间加生产侧测试钩子，本轮不加                                                                                                                                                                             |
| DNS 记录原子性测试不判别                                | 写完再读，`fs::write` 也能通过                                                                 | 属实。已改名为 `the_published_record_parses_and_leaves_no_staging_file`（这才是它证明的）；原子发布本身由 `atomic_fs` 的 `atomic_replace_readers_only_observe_complete_documents` 覆盖。补写的并发读者测试在 Windows 上同样不判别（小文件 `fs::write` 亦不可分割），已删 |
| handoff 续传消息缺唯一 id                               | generation 不是 handoff 的唯一标识，重试型生产者会碰撞                                         | 当前只有一个不重试的生产者，属健壮性储备而非可达路径；续传若引入重试再加                                                                                                                                                                                                 |
| `succeeded_recovered_is_not_stop_proof` 的 `waits == 1` | —                                                                                              | 该断言是测试质量强化（fake 不再在 admission 处给出终态），无法靠“还原生产代码”变红；它挡的是未来“不等待即采信”的实现                                                                                                                                                     |

未纳入本轮（决策项）：D-4 推送、D-7 handoff `force` 旗标、D-8 submodule 指针推进。

### 2026-08-30 复审轮（codex reviewer ×3 轮）

在 2026-08-27 修复轮基础上，针对 app 仓 actor_v2 与 `scripts/architecture-ledger.ts` 又跑了三轮 codex 复审；第三轮报告 **PASS，无残留 Critical/Major**。下表 (a) 是本轮修的发现 → 改了什么，(b) 是复审提出但本轮**未做**的，连同理由。

#### (a) 本轮修复

| 本轮修复                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1 ledger 漏计：一个出现在 `//` 行注释里的 `/*` 打开了一个没人关闭的块注释，导致 `core/clash/core.rs` 里提到 `/core/*` 路径的一句文档注释盖住了整个文件的其余部分（漏计 3 处 `Config::verge()`、1 处 `Config::clash()`、2 处 `CoreManager::global()`、3 处 `Logger::global()`、1 处 `Self::global()`，这正是快照与实际之间的全部差额）。修复：按源码顺序词法化 `//` 与 `/*`；随后加固到能处理转义、字符字面量 vs 生命周期撇号的区分、原始字符串与嵌套块注释，并把同一词法器共享给 `#[cfg(test)]` 大括号/条目扫描器（该扫描器有同样的缺陷，使 `test_real_dirs` 这个硬性门禁本身也漏计） |
| F2 规范性的 2026-08-12 app 集成设计此前只存在于另一分支，本仓无法对照它声称实现的文档做核验。已移植进本仓，并在 roadmap §6 与 PR-5 task 文件补了取代说明                                                                                                                                                                                                                                                                                                                                                                                                                               |
| F3 调用方超时是固定数字，而它等待的内部路径由注入的界限构建：`change_host` 在 100s 最坏情形上 90s 就放弃。三个调用方预算改为按各自路径逐段推导，算术写在旁边                                                                                                                                                                                                                                                                                                                                                                                                                           |
| F4 `Submit` handler 无界等待 endpoint，且 ipc client 本身也不设请求超时，一次挂起会让 actor 单一的串行 mailbox 永久卡住，包括后续的 shutdown                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| F5 探测契约承诺"挂起时也要给出 `Stopped` 形状的答案"，而 uninstall 守卫把 `Stopped` 读作"未持有 core"——这让契约本想兜底的那种情形，恰好把一个不可达的 daemon 变成了在运行中的 core 之下执行 uninstall。`probe` 现在返回 `Result`，失败或超时是 `ServicePhase::Unknown`，uninstall 据此拒绝，`EndpointDown` 也不再据此启动重启                                                                                                                                                                                                                                                          |
| F6 `ServiceActor` 发起的其余 adapter 调用都没有边界，`pre_start` 也不例外；现在都跑在注入的 `command_timeout` 下                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| F7 `EndpointDown` 会从除 `Ready` 外的所有相位重启 daemon，包括已在运行的不兼容 daemon、未安装的 daemon 与状态未知的 daemon；改为只有确认探测到"已停止"才重启                                                                                                                                                                                                                                                                                                                                                                                                                           |
| F8 普通 submit 不校验回显的操作 id，尽管 stop 路径已经校验；现在统一按 stop 路径处理                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| F9 健康检查每 5s 轮询一次，对不兼容 daemon 持续在每次轮询都告警，长会话下淹没其它日志；改为状态锁存，只在进入该状态时报一次，daemon 恢复响应后重新武装                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| F10 `ServiceEndpoint` 持有一个 `&'static` 的 ipc client，迫使组合方去泄漏一个实例；client 本身可 `Clone`，改为按值持有                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Round 2：`ServiceActor` 客户端的整体超时（一次命令边界 + 30s）比自身 handler 实际要跑的路径更短（2/4/6 段调用在生产边界下分别是 200s/400s/600s），导致提权操作还在跑时调用方就先收到失败；同时调用方超时对一个可能仍被接纳的 submit 回答不可重试的 `Internal`，堵死唯一安全的恢复路径。修复：每条消息的预算改由注入边界逐段推导；submit 的超时答案改为可重试并点名同 id 重试，handoff/shutdown/提权命令仍不可重试但声明工作可能仍在进行                                                                                                                                                |
| Round 3：测试区域掩码仍走 F1 之前的旧剥离器，同样会被行注释里的 `/*`、生命周期撇号误伤；另发现 `cr"` 原始 C 字符串未被识别为原始字符串开符，且字面量内容会被原样计入扫描文本（字面量里写的 `Config::global()` 会被误计为调用，字面量里的花括号会被误计为大括号）                                                                                                                                                                                                                                                                                                                       |
| Leader-found：`backend/Cargo.toml` 的注释仍声称 submodule 钉在 `v2.0.0-rc.1` tag，而 gitlink 早已前移（见 L10）；已改口                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

#### (b) 复审提出但本轮未做

| 项                                | 复审意见                                                       | 处置                                                                                 |
| --------------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 扩大调用方预算以覆盖排队          | 建议把调用方超时也扩到能盖住 mailbox 排队等待的时间            | 拒绝：无界深度的队列没有一个有依据的有限数字；改为让超时的错误携带恢复契约（见 L11） |
| `ServiceClient` 超时改为可重试    | 建议让提权 service 命令的超时也标记可重试                      | 拒绝：这些命令不是 id 幂等的，盲目重试会排入第二条提权命令                           |
| 改写 `core/clash/core.rs` 的注释  | 建议把触发 ledger 漏计的 `/core/*` 一句文档注释改写掉          | 拒绝：一个只因注释被改写才能工作的扫描器算不上修复；该文件保留原状，兼作回归用例     |
| ledger 词法器支持跨行非原始字符串 | 指出双引号字符串内含真实换行时仍不能跨行延续                   | 见 L12；`backend/` 下不出现此形状，本轮不做                                          |
| runtime crate 版本号提升          | 指出 L10 的根因是 submodule 内 `nyanpasu_service` 版本号未跟上 | submodule 侧工作，不属本仓改动范围                                                   |

除预算类修复外，本轮每个代码修复都带一个**在修复前的代码上会失败**的回归测试；预算修复（F3/Round 2）的判据是算术而非计时测试——收缩注入的界限会把真实最坏情形也一并缩到旧的固定预算之下，计时测试无法判别这类改动，这一点在此明说。

### 已知环境问题

`cd backend/nyanpasu-runtime && cargo clippy --workspace --all-targets --all-features` 在本机确定性 ICE：`clash-api/src/api/connections.rs:208` 的 opaque type（`OpaqueTypeStorage`）。与本轮改动无关——`clash-api` 未被触及；同参数的 `cargo check` / `cargo test` 全通过，逐 crate `cargo clippy -p <crate> --all-targets --all-features` 也全通过；app 仓 pre-commit 的 `cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features`（feature 组合不同）正常通过。

**2026-08-30 复审轮追加**：共享的 `backend/target` 反复无法收尾增量会话，报 `拒绝访问 (os error 5)`，产生的损坏让每个 `--emit=metadata` 构建都不可靠——`cargo check -p clash-nyanpasu --lib` 在 `boa_engine` 内部确定性 ICE，pre-commit hook 的 clippy 也先后在 `tungstenite` 的增量数据、`nyanpasu-core-metadata` 的 rmeta、`clash-api` 的 drop-check 上 ICE。`cargo build` / `cargo test` 全程未受影响（447 全绿）。因此 clippy 改用一个干净的隔离 target dir 跑（`CARGO_TARGET_DIR=G:/Programs/Rust/_clippy-target-pr5 CARGO_INCREMENTAL=0`），该目录下 `cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features` 在 900 余个 crate 上 exit 0；每个提交都导出了这个目录，让 pre-commit hook 真跑了一遍。全程未 pin 或更换工具链。

另记 ledger 总数：`main` 为 120/80/18，本分支为 120/80/19（多出的那个 marker 是 compat gate 自己的 `TODO(actor-migration)`）；分支中途出现过的 116/74 一对纯粹是 F1 描述的扫描器伪影。

## 6. 与既有 PR 栈的关系

`#5070–#5074` 处置仍待用户裁定（本实施未动它们）；本工作基于 `pr5/1-pre` 分支内容但未 rebase/merge #5070。"submodule pin 未移动" 已不成立：gitlink 现已推进到 `6717e44`（见 §1），干净 checkout 因此可以直接构建，不再依赖一个脏的 submodule 工作树。

## 7. 独立测绘交叉核对（runtime-mapper 代理，基于 PR-A 前的 `e899bce` 快照）

一个独立的探索代理对 pre-PR-A 树做了全量测绘，其结论与本实施相互印证，可作审计旁证：

- 其"缺失清单"逐项即本次填补：无 OperationId/幂等键 → A1；无 executor 队列（公有方法即队列，一把 `ctrl` Mutex 横跨进程 spawn 与 fs I/O，RPC handler 直调、无取消隔离）→ A4；回滚补偿逐路径手写无通用事务抽象 → A3 的 `reconcile` 统一入口；无 RuntimeBackend/RuntimeInstance 接缝 → A2；core-manager 内无任何 DNS 组件 → A6；v1 wire 零版本协商机制 → 按修订 A1 裁定，fail-closed 门不建在 wire 握手而建在 app 侧 `ServiceCompat` 主版本比较（ServiceActor 为唯一实现点）。
- 其"已有资产"确认了保留决策：`stop_and_confirm_dead` 是真 StopProof 原语（超时+独立 pid-file 权威兜底）；quarantine latch-until-recovered；publish 路径的 epoch fencing（`apply_epoch_status` 拒陈旧 epoch 帧）正是 D1 generation fencing 的同构先例；`IpcOperation` 契约对称性被 v2 三个新 op 沿用；fake-core 进程基建被 A7/控制面测试复用。
- 另两处其标记的既有全局单例（`Logger::global()`、`Client::service_default()` OnceLock）为 pre-existing 边界代码，不在本次范围；`ServiceEndpoint::new` 以参数注入 client，bridge 阶段组合时不必经由该单例。
