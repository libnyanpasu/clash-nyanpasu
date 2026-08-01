# PR-5a 实施计划 — 最小 CoreActor + `OperationId` + `CoreBackend` enum

**日期：** 2026-08-02
**分支基线：** `refactor/core-manager-actor` @ `4583048b5`（含 PR-5-pre 三提交：`4f22eaddb` 依赖切换 / `cca7f654f` 兼容门 / `4583048b5` ledger 同步）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/design.md` §3–§6、同目录 `task.md` 卡 A1/A2/A3
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.1；必答项 §6.4 RQ-02 / RQ-04
**平台：** Windows 11 / PowerShell
**版本：** v10（2026-08-02）——v1–v9 均经 codex 对抗审查 **REJECT**；v10 补齐 watch 通道的编译级接线，并推翻 round-7 的"预构造请求"裁定以消除 Updater 换核竞态

---

## 0.1 审查处置表（九轮）

历轮发现**全部经本人复核源码确认成立**（无一条被驳回）。第六轮后 leader 以**范围裁定**收敛，第八轮再以**裁定 A-v2** 修正其中一处过度裁剪。

> **v3 的教训**：第三轮修订时会话中断，重写只传播到部分小节，下游多处停留在 v2，导致三审看到自相矛盾的文档。**第四轮起改为逐项定点 Edit，不做整文件重写**，每完成一项即汇报。

| #   | 级别   | 问题                                                                          | 处置                                                                                                                                 | 落点                                    |
| --- | ------ | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------- |
| 1   | High   | Updater 依赖 S10 要删的 lease API → **必然编译失败**                          | 改用注入的 `CoreClient` + guard 迁移 `replace_core`；不加全局桥；UpdaterActor 仍归 6d                                                | 事实 A16f；**S9.1**；§0；§8             |
| 2   | High   | `start` / `restart` 都不是 request-bearing 原语，全新 backend 上 restart 无效 | 收敛为**单一 `run(request)`** 迁移原语；Local 用 `switch(spec)` 一次调用覆盖两种情形，Service 用 stop-then-start                     | 事实 R5b/R5c、DV-F；S3、S5、S7、S8      |
| 3   | High   | 组合根注入图不可实现（`core` 在 `block_on` 前就被解构）                       | `CoreClient` 在 typed 快照后于 block 内构造；typed 字段入 `NyanpasuClientInner`；`args.core` 改 `Option`；两个无注入点消费者显式穿线 | 事实 A17f；**S8** 全节重写              |
| 4   | Medium | `events()` 不重连；过期 `/status` 可回退 revision；start 不返回 revision      | actor 自管可取消重连循环 + **generation 栅栏** + start 后同步回填                                                                    | **RQ-02** 重写；T-RV-02/04/05           |
| 5   | Medium | `shutdown()` 不释放目录锁，换槽会拿不到锁                                     | 槽位改 `Option`；六步换槽协议（取消→join→shutdown→drop 全部 `Arc`→构造→写回）；定义无 backend 失败态                                 | 事实 R6b；**D2** 重写；T-BK-08/09       |
| 6   | Medium | 漏第二条裸线程恢复；DNS 是第二个 residual；`resolve_reset` 已先停一次核       | S10 删两条路径；DNS **allowlist 到 5c**（leader 裁定）；`resolve_reset` 移除停核（已验证仅一个调用者）                               | 事实 A18f/A20f；S9.2、S10、**S11** 重写 |
| 7   | Medium | 只算 flag，未满足 design §5 的"发布一次"                                      | 新增窄 `CoreDegradationSink`（单方法、可 mock）+ per-episode latch + 复位规则                                                        | **D5** 重写；T-BK-06/07                 |
| 8   | Medium | parity 允许降级为 TestBackend，等于架空 A-Exit                                | 承诺真实双端：fake-core 补 `GET /version`（或新增探针 bin）+ 真实 IPC roundtrip harness；**禁止降级**                                | 事实 A19f；**S12.1 / S12.2**            |
| 9   | Low    | `ConfigRevision` 字段描述、seam 覆盖遗漏、DV-B 指错                           | T-RV-03 拆成 03a/03b 两个转换；A9f 补第 4 个 seam 实现；DV-B 改指 S4                                                                 | A9f、DV-B、T-RV-03a/03b                 |
| 10  | —      | **（自查）** S13 的 ledger 基线用了 5-pre **之前**的快照数字                  | 改用 2026-08-02 实测基线 116/74/19/300/0，并给出逐项加减预期（19 → 17）                                                              | **S13**                                 |

**第二轮（v2 → v3）发现的处置：**

| #   | 级别   | 问题                                                                | 处置                                                                                                 | 落点                   |
| --- | ------ | ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------- |
| 11  | High   | Updater 注入链只说"构造点会接受"，且类型名 `UpdaterInstance` 不存在 | 写出真实四段链与逐点签名；`CoreClient` 只随调用传递                                                  | 事实 A16g；S9.1        |
| 12  | Medium | Service run 判定读了有损的 `CoreState`                              | 改读 `CoreInfos.detail`，只有 `Stopped` 是终止态；`detail` 缺失时保守当作在跑；只抑制 `not_started`  | 事实 R5d；S3           |
| 13  | High   | 适配器缺 core-type 来源；health-check 穿线漏 3 个消费者             | **leader 裁定**：注入 `ApplicationClient` + `RuntimePaths`，每次现读 typed 快照；穿线表补到 6 个入口 | 事实 A21f/A22f；S7、S8 |
| 14  | High   | revision 栅栏挡不住同连接乱序；`run` 返回 `()` 却声称同步刷新       | 引入两级机制；`run` 改返回 `CoreStatusView` 并在同一消息处理内提交                                   | RQ-02；S3              |
| 15  | Medium | latch 在 `recover` 成功时复位（`recover` 只清 quarantine）          | 复位条件收紧为"观察到活跃态"或"backend 身份变化"两条                                                 | D5                     |
| 16  | Medium | parity 允许裸 `#[ignore]`；fake-core barrier 未规划                 | 见第三轮 #21/#22（v3 未完成，第四轮补齐）                                                            | S12.1 / S12.2          |

**第三轮（v3 → v4）发现的处置：**

| #   | 级别   | 问题                                                                                | 处置                                                                                                                                                                                                                                                  | 落点                     |
| --- | ------ | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| 17  | Medium | Service 判定改对了但**没加测试**                                                    | T-BK-10（6 个 `CoreStateDetail` 变体 + `None` 共 7 例，表驱动）、T-BK-11（stop 竞态到 `not_started`）                                                                                                                                                 | S12 测试表               |
| 18  | High   | 穿线仍只列 2 个消费者；适配器 core-type 来源仍未解决                                | S8(3) 换成 6 行表（含 `control.rs:97/225/320` 与其 IPC 入口）；S7 增 `CoreLifecycleAdapter` 三依赖定义                                                                                                                                                | S8、S7                   |
| 19  | High   | 观察版本协议**对流式帧不可实现**（第 2 帧起会被自己的栅栏误杀）                     | 拆成两套：推送帧只带 `ConnectionId`、actor 自增版本；`RefreshToken` 只用于请求/响应。加 T-RV-08 作回归钉                                                                                                                                              | RQ-02；S5；T-RV-04/06/08 |
| 20  | Medium | 无生产 sink 实现；`phase`/`message` 未定；latch 读投影后的两值状态看不见 `Starting` | 定义 `TauriCoreDegradationSink` + 四字段 DTO 表；**新增 `DegradationPhase::CoreLifecycle` 变体**（leader 2026-08-02 裁定：`RuntimeApply` 是语义谎言，接受一处声明在案的 bindings 新增）；latch 改为**投影前**读 crate-private 忠实视图。加 T-BK-12/13 | D5；S12 测试表；S13      |
| 21  | Medium | S12.1 停留在 v2（未规划 barrier / 端口 / 环境）                                     | 推荐新增不需父进程 barrier 的 `manager-probe-core` bin；备选路径写全 6 步 barrier 生命周期与串行化要求                                                                                                                                                | S12.1                    |
| 22  | Medium | S12.2 仍允许裸 `#[ignore]`                                                          | CI 支持平台必须常规运行；确需跳过则必须配显式门禁脚本**且进 CI**                                                                                                                                                                                      | S12.2                    |
| 23  | —      | **（自查）** `run()` 的返回类型与 S3 方法表不一致                                   | S3 的 `run` 签名同步改为返回 `CoreStatusView` 并加注释说明原因                                                                                                                                                                                        | S3                       |
| 24  | Low    | T-BK-08 同模式换槽可能 no-op（假阳性）                                              | 改 Local→Service→Local，并断言 backend 身份确实变化（构造计数 / `Arc::ptr_eq` 判否）                                                                                                                                                                  | S12 测试表               |
| 25  | Low    | S9 陈旧路由（`Start` 消息、`resolve.rs` 的 `Stop`）                                 | 改为 `Run { request }` / "按 S11 删除该行停核"                                                                                                                                                                                                        | S9                       |
| 26  | Low    | `runtime_path` 措辞说 app 读不到（Local 其实读得到）                                | 改为"为保持与 IPC 同构的单一表示、不泄漏 runtime 内部路径而丢弃"；不可访问性只对 Service 侧成立                                                                                                                                                       | RQ-02                    |
| 27  | Medium | `core_client()` 是内部 client 访问器，不是领域 facade                               | **leader 裁定**：改为 `NyanpasuClient::update_core(core_type)` 领域方法，不暴露访问器                                                                                                                                                                 | S9.1                     |
| 28  | Low    | 文档版本与处置表未推进                                                              | 本节（三轮全覆盖）+ 头部 v4                                                                                                                                                                                                                           | 头部、§0.1               |

**第四轮（v4 → v5）发现的处置：**

| #   | 级别   | 问题                                                                                | 处置                                                                                                                          | 落点                  |
| --- | ------ | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| 29  | —      | **（自查）** Exit 判据仍写 `T-BK-01…09`，把新增的 latch/DTO/判定测试排除在验收外    | 改为 `T-BK-01…13`                                                                                                             | §5 Exit 表            |
| 30  | High   | 消息集缺 `ConnectionOpened` / `RefreshCompleted`；S5 注释仍写"必须带 token"自相矛盾 | 补两条消息 + 三步激活顺序；注释改为"只带连接身份、actor 自增 version"；慢刷新独立 `tokio::spawn` 回投，不阻塞 mailbox         | S5；RQ-02；T-RV-09/10 |
| 31  | High   | `Run` 前排队的当前连接帧会覆盖 post-run 提交，T-RV-07 失效                          | **把 `Run` 当作一次连接轮换**：调 backend 前先换 `ConnectionId`，早于该点排队的帧全部作废。T-RV-07 重定向到该交错             | RQ-02；T-RV-07        |
| 32  | High   | S8 仍写 `core_client.as_lifecycle_port()`，少 `ApplicationClient` / `RuntimePaths`  | 构造行改 `CoreLifecycleAdapter::new(core, application, runtime_paths)`；S7 开头改为"由适配器实现"而非"给 CoreClient 实现"     | S7；S8                |
| 33  | High   | 三个 service IPC 命令拿不到私有 `CoreClient`（且正确地禁止访问器）                  | **leader 裁定**：facade 加 `install_service` / `start_service` / `restart_service` 三个领域方法；命令退化为薄适配器           | S8(4)                 |
| 34  | Medium | fake-core 备选路径**是错的**——RELEASE 即退出，release 后 HTTP 服务已消失            | **leader 裁定**：整段删除，`manager-probe-core` 为唯一 parity 路径；保留一段"为何无效"的说明防止重蹈                          | S12.1                 |
| 35  | Medium | `message` 截断规则与 T-BK-13 互相矛盾                                               | **leader 裁定**：整条最终 message 上限 512 字节，只截 reason 段，按 UTF-8 字符边界并补 `…`                                    | D5 DTO 表             |
| 36  | Low    | 两处仍要求 bindings **完全**零 diff，与 S13 的"恰好新增 `core_lifecycle`"矛盾       | 两处改为"除已声明的 `core_lifecycle` 联合成员外无其它 diff"                                                                   | S8；S9.1              |
| 37  | Low    | Exit / RQ 摘要未点名 T-RV-08 与 T-SM-01/02                                          | RQ-02 摘要改 `T-RV-01/02/03a/03b/05/06/07/08`；Exit 表逐项点名并新增一行 seam 回归判据                                        | RQ-02；§5 Exit 表     |
| 38  | Low    | 回滚/提交切分仍以 "改 fake-core 加 `/version`" 为主语，未覆盖新 bin 路径所需文件    | 回滚清单补 `manager_probe_core.rs` / `Cargo.toml` `[[bin]]` / `control.rs` / `runtime.rs` / `event_sink.rs` 等；commit 1 改名 | §6 回滚；§7 切分      |

**第五轮（v5 → v6）发现的处置：** 剩余缺口全部集中在一个子系统，因此本轮改为**整体规范化**而非继续打补丁——新增 **§2.1「观察协议规范」**（状态表 × 消息集 × 转换规则 R1–R7 × 不变量 I1–I3），RQ-02 / S3 / S5 一律改为引用。

| #   | 级别   | 问题                                                                             | 处置                                                                                                                                                       | 落点               |
| --- | ------ | -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| 39  | High   | `Run` 轮换后旧订阅任务永远盖死 ID，此后推送全被丢弃                              | **裁定**：每次 `Run`（成功**或**失败）后 cancel + restart 订阅，新任务重新握手 = §2.1 **R6** / 不变量 **I1**                                               | §2.1；T-RV-11      |
| 40  | High   | `ConnectionOpened` 无世代栅栏；被取消任务的握手可能"复活"                        | **裁定**：加 `subscription_epoch`，回 `Result<ConnectionId, StaleSubscription>` = §2.1 **R1**；join 不清 mailbox 已写明                                    | §2.1；T-RV-12      |
| 41  | High   | 握手 await 不可取消 → SetBackend join 任务、任务等 handler 的死锁                | **裁定**：`tokio::select!` + cancel token = §2.1 **R7**                                                                                                    | §2.1；T-RV-14      |
| 42  | High   | `status()` 的读语义未定（缓存 vs 等刷新）                                        | **裁定**（design §6）：一律返回缓存 watch 快照；`RefreshCompleted` 无 caller reply port，纯内部缓存维护 = §2.1 **R5/I3**                                   | §2.1；T-RV-13      |
| 43  | High   | D5 要求投影前的忠实生命周期，但没有任何类型承载它                                | 引入 crate-private `BackendObservation { view, lifecycle }`，贯穿推送帧 / `RefreshCompleted` / `run()` 返回值                                              | §2.1.3；S3         |
| 44  | High   | facade 漏 `stop_service`；且是 `control::*` 直通而非完整 typed 事务              | 补齐**六个**方法（四 service + `core_status` + `restart_core`），写出完整事务；**保留**"控制失败仍尝试拉核"的既有语义                                      | S8(4)；T-FA-01…04  |
| 45  | Medium | parity 会定位到错误的二进制；gate 命令在无 Cargo.toml 的仓库根执行               | 新增 `resolve_probe_bin_path` / `require_probe_bin_path` 具名 resolver；gate 改 `--manifest-path .ackend\Cargo.toml -p fake-core --bin manager-probe-core` | S12.1；S13         |
| 46  | Low    | T-RV-07 在单 FIFO mailbox 下不可构造                                             | 重定向到可构造交错（backend 内轮换后 barrier 暂停 → 投旧 ID 帧）                                                                                           | T-RV-07            |
| 47  | Low    | T-RV-09/10 未进摘要与 Exit；风险表仍写已删除的"路径 A/B"                         | 范围改 **T-RV-01/02/03a/03b/05/06/07/08**；风险行改写为单一探针 bin 路径                                                                                   | RQ-02 摘要；§5；§6 |
| 48  | —      | **（自查）** S2 仍写请求路径来自 `dirs::*()`，与 S7 的注入式 `RuntimePaths` 冲突 | S2 改为"路径一律来自注入的 `RuntimePaths` / `PathResolver`，禁 `dirs::*()`；core type 来自 typed 快照"                                                     | S2                 |

**第六轮（v6 → v7）：leader 范围裁定 + 幸存发现。**

> **裁定：5a 整体移除推送式观察。** 六轮审查的发现高度集中在同一子系统（订阅流 / 连接身份 / 陈旧帧栅栏），v6 的 §2.1 规范化仍被查出"协议自身会接受它本该拒绝的陈旧帧"。依据 task.md：**"status read 不走 mailbox RPC" 与 watch 投影明确写在 C1 卡**，A1–A3 与 A-Exit 无一需要实时推送。因此把整套机制移出 5a——**陈旧帧问题在没有推送流时不存在**。新模型见 §2.1（两个观察来源 + guard 下刷新 = 构造上无竞态）。

| #   | 级别   | 问题                                                                                                                        | 处置                                                                                                                                       | 落点              |
| --- | ------ | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ----------------- |
| 49  | High   | §2.1 协议会接受本该拒绝的陈旧帧；取消/watch/观察类型/请求路径/健康检查依赖均不可实现                                        | **范围裁定**：整套推送机制移出 5a（七审确认 #1–#5 MOOT）。**注**：#6 观察载荷**未**随之消失，见第 56 行                                    | §2.1              |
| 50  | High   | S2 的注入式请求路径不可实现（`RuntimePaths` 缺 binary/pid/working-dir；`find_binary_path` 调 `dirs::*`；字段名应为 `core`） | 新增 `CoreRequestFactory`（持 `PathResolver` + `RuntimePaths`），resolver 版二进制查找；`snapshot.core` + 显式 `ClashCore → CoreType` 转换 | S2；S7；S8        |
| 51  | High   | 健康检查只穿 `CoreClient`，凑不齐 typed 快照与请求路径                                                                      | 改穿 `CoreModeReconciler{core, application, requests}`，四个调用者共用                                                                     | S8(3)             |
| 52  | High   | facade 伪码里快照/guard/SetBackend/请求构造仍用 `?`，会顶替控制结果                                                         | 整个对账块捕获成一个 `Result`，块内**一律不 `?`**；`control?` 最后抛；新增可注入 `ServiceControlOps` seam 供 T-FA 制造控制失败             | S8(4)；T-FA-01…05 |
| 53  | Medium | S9 仍让命令层直接调 `CoreClient`，与 S8 的私有边界及 T-FA-04 矛盾                                                           | S9 表全部改为 facade 目标；facade 领域方法总计 **7 个**                                                                                    | S9                |
| 54  | Medium | `NoBackend { last_error }` 无法从 actor 状态复现                                                                            | 槽位显式建模为 `BackendSlot::{Ready, Failed{error}}`                                                                                       | S5；D2            |
| 55  | Low    | 清单陈旧：`fake-core/src/lib.rs` 未进回滚/commit 1；"三个 service 命令"                                                     | 两处清单补齐；计数改为四个                                                                                                                 | §6；§7；S8        |

**第七轮（v7 → v8）：模型获认可，补齐实现契约。** 七审确认"no-push + mailbox 串行"模型可行，round-6 #1–#5 判 MOOT；剩余为完成度缺口。

| #   | 级别   | 问题                                                                                                    | 处置                                                                                                                                         | 落点                |
| --- | ------ | ------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| 56  | High   | `RefreshStatus` 无 `OperationId`，§2.1 的结构论证只是文字                                               | **裁定 A**：`RefreshStatus { operation: Option<OperationId> }` + 两条准入规则（`Some` 须等于 active；`None` 仅 gate 空闲时执行，否则回缓存） | §2.1.2；T-RV-06     |
| 57  | High   | D5 的"下次 UI 读取即观察到耗尽"没有实现机制                                                             | `core_status()` 走 `None` 路径，gate 空闲时真查 backend——权衡变成**已实现机制**，弱化措辞删除                                                | §2.1.3；D5；T-RV-09 |
| 58  | High   | `FaithfulLifecycle` 无定义；`run/stop/recover/status` 签名与 `BackendObservation` 矛盾                  | 定义归一化枚举 + 两侧映射表（含 `detail` 缺失的降级）；四个方法统一返回 `BackendObservation`，`observe_status()` 改 async                    | §2.1.4；S3          |
| 59  | High   | `CoreRequestFactory` 仍不可完全注入（`PathResolver::app_install_dir` 走 `dirs::*`）；Updater 路径不存在 | 抽 `CoreBinaryResolver` 可注入策略；工厂 `derive(Clone)` 并存入 `Inner`；Updater 由 facade 递交**已构造的** `CoreRequest`                    | S2；S8；S9.1        |
| 60  | High   | `ServiceControlOps` 声明了但没接进对象图                                                                | 四步注入链：`ClientSetupArgs` → `Inner` → `setup.rs` 生产适配器 → facade 调注入 ops                                                          | S8(4)               |
| 61  | Medium | 六行穿线表仍写裸 `CoreClient`                                                                           | 四行改为 `CoreModeReconciler`；`spawn_health_check` 签名同步                                                                                 | S8(3)               |
| 62  | Medium | `BackendSlot` 无法表达换槽瞬态；错误不可克隆                                                            | **裁定 B**：`Option<BackendSlot>`，`Failed { error: Arc<CoreBackendError> }`                                                                 | S5；D2              |
| 63  | Medium | v6 残留：三来源措辞、`/ws/events`、旧测试范围、RQ-04 的"不走 mailbox"                                   | 定点清扫；测试清单固定为 **T-RV-01/02/03a/03b/05/06/07/08 + 09**                                                                             | RQ-02；S3；S12；§5  |

**第八轮（v8 → v9）：watch 投影回归 + 收尾。**

> **裁定 A-v2**：v8 让 `status()` 走 mailbox RPC，八审指出这在慢操作（apply 30–80 s）期间会让所有读排队超时——**这正是 design.md §6 当初指定 watch 读的原因**。第七轮砍掉的是 **backend→actor 推送流**（订阅/连接/栅栏，仍归 C1）；**actor→client watch 投影**是单写入者、无并发危害、约 30 行，从来不是有争议的部分，因此接回 5a。

| #   | 级别   | 问题                                                                     | 处置                                                                                                                                                                         | 落点                |
| --- | ------ | ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| 64  | High   | mailbox 串行下慢操作期间一切读排队超时（活性缺陷）                       | **裁定 A-v2**：actor 持 `watch::Sender`，每次提交发布；`status()` = 同步 watch 克隆，零 mailbox；删 `CORE_READ_TIMEOUT`                                                      | §2.1.1；S6；T-RV-10 |
| 65  | High   | `RefreshStatus` 的 reply 类型与调用形态未分化                            | 拆成守卫 `call`（必填 `OperationId`）与 UI `cast RefreshHint`（空闲才处理，否则丢弃）；定义 `CoreActorError` 四变体                                                          | §2.1.3；S5；T-RV-06 |
| 66  | Medium | S5 仍有第三个 `Status` 消息；缓存只存投影后的视图                        | 删 `Status` 消息；缓存改存完整 `BackendObservation`（保留忠实 lifecycle）；加 `status_tx`                                                                                    | S5                  |
| 67  | Medium | T-BK-09 期望 `None`，与"`None` 仅瞬态"矛盾                               | 改为期望 `Some(BackendSlot::Failed { error })`                                                                                                                               | T-BK-09             |
| 68  | Medium | 工厂未单次构造入 `Inner`；updater 仍有两处 legacy 查找；健康检查无注入根 | 工厂在 block 内一次构造后 clone；updater 用 request 的 typed `core_type` 比较（删两处查找）；健康检查根落在 `resolve.rs:152` 的 facade 入口，两处 `Config::verge()` 改为传值 | S8；S9.1            |
| 69  | Low    | RQ-02 来源表重复且写"三条"；Exit 未含 T-RV-09/10                         | 表去重为两条；Exit 与摘要同步                                                                                                                                                | RQ-02；§5           |

**第九轮（v9 → v10）：最窄一轮，全部局部。**

| #   | 级别   | 问题                                                                                                                 | 处置                                                                                                                                                                                                                | 落点              |
| --- | ------ | -------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| 70  | High   | watch 接线不可编译：无通道创建点、`CoreClientInner` 缺 receiver、`CoreActorArgs` 缺 `status_tx`、无 `refresh_status` | 新增 §2.1.7：通道在组合根创建、`Sender` 进启动参数、`Receiver` 进 client；给出 `status()` / `refresh_status()` / `hint_refresh()` 全签名与初值语义；单一 `commit()` helper；`None`/`Failed`/shutdown 三种合成观察表 | §2.1.7            |
| 71  | High   | **round-7 的"预构造请求"裁定制造了陈旧竞态**：`replace_core` 在下载/解压**之后**才跑，调用时刻的快照可能已过期       | **推翻该裁定**：注入窄 `CoreRequestProvider`，请求在**替换 guard 内**解析；Updater 只带目标 core type                                                                                                               | S9.1；T-RV-15     |
| 72  | High   | `BackendObservation` / `FaithfulLifecycle` 未 `Clone`，actor 无法既留存又回传                                        | 三个类型（含 `CoreStatusView`）补 `#[derive(Clone)]`                                                                                                                                                                | §2.1.5；§2.1.7    |
| 73  | Medium | `RefreshHint` 无去重，空闲期会积压 N 次查询                                                                          | 共享 `Arc<AtomicBool>` pending 位：client CAS 后才 `cast`，actor 处理开头清位；登记为唯一跨边界共享量                                                                                                               | §2.1.6；T-RV-14   |
| 74  | Medium | 最终一致性未写明例外                                                                                                 | 明确 D3 的 out-of-actor `put_configs` **不发布**，靠后续 idle hint 恢复；随 B3 消失                                                                                                                                 | §2.1.6.1；T-RV-13 |
| 75  | Medium | `None` 瞬态与 shutdown 的发布无测试钉                                                                                | T-RV-11 / T-RV-12 用 barrier 断言两处都发布                                                                                                                                                                         | T-RV-11/12        |
| 76  | Low    | T-RV-10 写成阻塞 `apply`（D3=A 下 apply 不占 handler）；v8 措辞残留；模块路径 `pub mod client` 有误                  | T-RV-10 改为阻塞 `Run`；措辞与 C1 指针清扫；`actor/mod.rs` 不声明 `pub mod client`（typed client 在 `client/core.rs`）                                                                                              | 多处              |

> 对 #9 的一点澄清：审查说"`ConfigRevision` 比 `RevisionId` 多了 `source_hash` 和 `runtime_path`"——这是拿 `ConfigRevision` 与 `RevisionId` 比。原文 RQ-02 里"多一个 `runtime_path`"是拿 `ConfigRevision` 与 **`ConfigRevisionInfo`** 比，那句本身成立。**但结论一致**：原 T-RV-03 把两个不同的转换混成一句，断言不成立，必须拆开。已按建议拆分。

---

## 0. 本阶段的边界

**做（= task.md A1/A2/A3）：**

1. 封闭 `CoreBackend` enum，`Local` 包装 `nyanpasu_core_manager::CoreManager`，`Service` 持有实例化的 `nyanpasu_ipc::client::Client`；
2. 取消安全的 `OperationId` / `OperationGate` / `CoreOperationGuard`，client 侧预分配 ID；
3. `CoreClient` 通过既有 `CoreLifecyclePort` / `CoreLifecycleLease` seam 接入，组合根注入，start/stop/restart/status 改走 actor；
4. 删除 legacy `CoreManager::lifecycle_lock` 与**两条**裸线程递归 recover；
5. **把 Updater 的核心依赖显式化**（`replace_core` 改用注入的 `CoreClient` + operation guard）——这不是范围扩张，是 S10 删除 legacy lease API 的**编译必要条件**，见 S9.1。

**不做（越界即返工）：**

- 不改 apply 管线语义（`check_and_promote` / `apply_candidate` / `apply_promoted` 的**实现路径**保持现状，见决策点 D3）；
- 不动 `rebuild_gate` / `clash_patch_gate` / `RuntimeLifecycleStore` / `publish_promoted` / `publish_applied` / `restore_promoted`（全部是 B1/B2/B3 的范围）；
- 不改 `change_core` 的编排与回滚（B4）；
- 不删除 `RunningConfigPatchPort` / `LegacyRunningConfigPatchBridge`（B3）；
- **不做 UpdaterActor**，也不加 `attach_core_port` 全局桥——S9.1 只把 core 依赖显式注入，`UpdaterManager::global()` 仍是 PR-6d 的 residual；
- **不迁移 macOS DNS**（`feat.rs:392`）——allowlist 到 PR-5c / C3，见 S9.2；
- 不做日志 ring / watch 投影 / `set_mode` / `reconcile_mode`（C1–C2）；
- 不删除 5 s 健康轮询线程与 `IPC_STATE` static（C2）；
- 不改 `get_core_status` 的 wire 形状（C1 才做 additive 扩展）；本阶段唯一的 wire 变化是`DegradationPhase` 新增 `CoreLifecycle` 变体（D5，backend-only，不动前端）。

---

## 1. 已核验事实

> 全部为本次会话直接读源码所得。`nyanpasu-runtime` submodule 确认在 tag `v2.0.0-rc.1`（`git -C backend/nyanpasu-runtime describe --tags HEAD`）。
> 下表 `RT/` = `backend/nyanpasu-runtime/`，`APP/` = `backend/tauri/src/`。

### 1.1 runtime 侧（`nyanpasu-core-manager` @ v2.0.0-rc.1）

| ID  | 事实                                                                                                                                                                                                                                                                                                                                                                                                       | 锚点                                                                                                               |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| R1  | 构造是 **async** 且 **runtime_dir 必填**：`pub async fn new(options: ManagerOptions) -> Result<Self, Error>`；缺 `runtime_dir` 返回 `Error::InvalidManagerOptions("runtime_dir is required")`                                                                                                                                                                                                              | `RT/crates/nyanpasu-core-manager/src/manager/mod.rs:212`、`:218-221`                                               |
| R2  | **构造会取运行目录独占锁**，失败返回 `Error::RuntimeDirectoryOwned(path)`；同时校验 controller 模板、拒绝零超时、清扫孤儿 epoch、拉起 JSONL log sink                                                                                                                                                                                                                                                       | `manager/mod.rs:223`、`:229-274`                                                                                   |
| R3  | `ManagerOptions::default()` 的 `local_ipc_policy` **已经是 `LocalIpcPolicy::Disable`**（`spec.rs:114`，`spec.rs:154-162` 有单测钉住）。A1 要求的"显式写出"是可审性要求，不是行为修复                                                                                                                                                                                                                       | `RT/crates/nyanpasu-core-manager/src/spec.rs:65-128`                                                               |
| R4  | **`CoreManager` 不是 `Clone`**（`struct CoreManager { inner: Arc<Inner> }`，未派生 Clone）→ app 侧必须 `Arc<CoreManager>`                                                                                                                                                                                                                                                                                  | `manager/mod.rs:87-89`                                                                                             |
| R5  | 生命周期方法全部 `&self` + async，错误类型 `nyanpasu_core_manager::Error`：`start(InstanceSpec)`、`stop()`、`check_config(&InstanceSpec)`、`shutdown()`、`restart() -> SwitchOutcome`、`switch(InstanceSpec) -> SwitchOutcome`                                                                                                                                                                             | `manager/mod.rs:319,438,509,541`；`manager/switching.rs:45,52`                                                     |
| R5b | **`switch(spec)` 同时覆盖"未跑"与"在跑"两种情形**，是唯一 request-bearing 的运行迁移原语：`switch_locked` 先判 running；**不在跑** → 清理 stale epoch 后 `start_locked(ctrl, spec)`，返回 `SwitchOutcome::Hard { reason: NotRunning }`；**在跑** → 按能力选 graceful（零停机）或 hard switch。这与 legacy `run_core_with_lease` 的"在跑就先停再按请求启动"语义**完全对应**，且更优                         | `manager/switching.rs:52,58-103`                                                                                   |
| R5c | 反之 `start(spec)` 在核心已在跑时**直接返回 `Error::AlreadyRunning`**；`restart()` 依赖 `ctrl.last_spec`，全新 backend 上返回 `Error::NotStarted`。二者都**不能**充当"按请求运行"原语                                                                                                                                                                                                                      | `manager/mod.rs:319-328`；`manager/switching.rs:45-49`                                                             |
| R6  | **没有 `recover()`**；实际名字是 `recover_quarantine() -> Result<(), Error>`，语义是"清除 quarantine 闩锁"，不是"重启核心"                                                                                                                                                                                                                                                                                 | `manager/quarantine.rs:18`                                                                                         |
| R5d | **wire `CoreState` 是有损两值投影**：上游 doc 原文"it reports `Starting` and `Restarting` as `Stopped(None)`, so a crash loop is indistinguishable from a real stop"。**忠实视图只在 `CoreStateDetail` 里**（`Stopped` / `Starting` / `Running` / `Restarting` / `Switching` / `Stopping`），经 `CoreInfos.detail` 送达。因此 Service 的"是否在跑"判定**必须读 `detail`**，读 `state` 会把过渡态误判成已停 | `RT/nyanpasu_ipc/src/api/status.rs:107-122,133-141`                                                                |
| R6b | **`shutdown()` 不释放运行目录锁**。锁是 `RuntimeDirectoryLock { _lock: atomic_fs::DirLock }`（文件系统目录锁），作为 `_runtime_lock` 字段挂在 `Inner` 上并**声明在最后**（注释："so ordinary Inner destruction drops instances/tasks before releasing directory ownership"）。因此它只在**最后一个 `Arc<Inner>` 被 drop 时**才释放                                                                         | `manager/mod.rs:118-124`；`config/runtime_store.rs:24-26,338-340`                                                  |
| R7  | `apply_config(&self, input: InstanceSpec, expected_revision: Option<RevisionId>) -> Result<ApplyOutcome, Error>`——参数是**整个 `InstanceSpec` 按值**，不是配置路径；要求核心已在跑，否则 `Error::NotStarted`                                                                                                                                                                                               | `manager/apply.rs:19-23`、`:26-29`                                                                                 |
| R8  | CAS 失败返回 `Error::RevisionConflict { expected: RevisionId, actual: Option<RevisionId> }`，且**不应用任何东西**                                                                                                                                                                                                                                                                                          | `manager/apply.rs:30-38`；`src/error.rs:43-47`                                                                     |
| R9  | `RevisionId { epoch: u64, generation: u64, effective_hash: String }`，**不是 `Copy`**；由 `ConfigRevision::id()` 取得，`ConfigRevision` 挂在 `CoreStatus.revision: Option<ConfigRevision>` 上；**没有 `CoreManager::revision()` 访问器**                                                                                                                                                                   | `src/state.rs:143-167`、`:189`                                                                                     |
| R10 | 状态与日志订阅：`subscribe() -> watch::Receiver<CoreStatus>`、`subscribe_logs() -> broadcast::Receiver<Arc<LogFrame>>`（容量 256，可在首次 `start()` 前调用）、`status() -> CoreStatus`                                                                                                                                                                                                                    | `manager/mod.rs:292,296,308`；`src/log.rs:16`                                                                      |
| R11 | 本地 `ApplyOutcome` 有 7 个分支：`Noop` / `Patched` / `Reloaded` / `Restarted` / `Switched` / `RolledBack{failed_apply}` / `DurabilityUncertain{outcome, warning}`。**`Warning` 是包装器不是 outcome**（印证 RQ-03）                                                                                                                                                                                       | `manager/mod.rs:57-85`                                                                                             |
| R12 | 供抄写的 outcome 映射参考实现：`map_apply_outcome`（把 `DurabilityUncertain` 拆成 `CoreApplyData.warning`，可嵌套两层、用 `"; "` 拼接）                                                                                                                                                                                                                                                                    | `RT/crates/nyanpasu-service-runtime/src/server/manager_bridge.rs:605-639`                                          |
| R13 | **manager 自身已做有界重启 + 指数退避**（委托 `nyanpasu_utils::process::Supervisor`）：`InstanceOptions.restart_policy` 默认 `OnFailure{max_restarts: 5}`、`backoff` 默认 `exponential(1s, 30s).with_jitter()`；另有**不可通过 `InstanceOptions` 配置**的 storm guard（默认 5 次/5 分钟）                                                                                                                  | `src/spec.rs:31-50`；`src/instance.rs:229-237`；`RT/crates/nyanpasu-utils/src/process/supervisor.rs:36-55,431-466` |
| R14 | **恢复耗尽没有 typed 信号**——唯一机器可读线索是 `CoreState::Stopped { reason: Some(StopReason::Error(msg)) }` 且 `msg` 以 `"core kept crashing; restart budget exhausted\n"` 开头；上游自己的测试也在字符串匹配                                                                                                                                                                                            | `src/instance.rs:631-641`；`RT/.../tests/instance_lifecycle.rs:246-252`                                            |
| R15 | 第二个不可恢复闩锁是 **quarantine**：`Error::StopUnconfirmed` → `latch_quarantine`，此后所有受门操作（start/switch/restart/apply）都被拒，直到 `recover_quarantine()` 成功。`stop()`/`shutdown()` 故意绕过该门                                                                                                                                                                                             | `manager/quarantine.rs:8-13`；`manager/mod.rs:434-437,537-540`                                                     |
| R16 | **`Error::kind()` 在本 tag 不存在**（整个 crate 没有 `impl Error` 块）。可用的机器可读分类是：ipc 侧的 `nyanpasu_ipc::api::error_kind` 字符串常量表 + 一个**私有**映射函数 `map_error_kind`（位于 `nyanpasu-service-runtime`，app 不依赖该 crate）                                                                                                                                                         | `RT/nyanpasu_ipc/src/api/mod.rs:38-66`；`manager_bridge.rs:646-665`                                                |
| R17 | 可抄的测试脚手架：`tests/common/mod.rs` 的 `fast_options()`（`max_restarts: 2`、50ms→200ms backoff、50ms 健康间隔）、`wait_for_state()`、`wait_for_health()`、`utf8_tempdir()`。fake core 是**该 crate 自己的 `[[bin]]`**，app 侧**拿不到** `CARGO_BIN_EXE_*`                                                                                                                                              | `RT/crates/nyanpasu-core-manager/tests/common/mod.rs:18-147`；`Cargo.toml:12-15`                                   |

### 1.2 runtime 侧（`nyanpasu-ipc` v2 client）

| ID  | 事实                                                                                                                                                                                                                                            | 锚点                                                       |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| R18 | 可实例化构造：`Client::new(placeholder: &str) -> Result<Self>`；`Client` 是 `Clone + Debug`。`service_default()` 仍在（`OnceLock`），A1 禁止使用它                                                                                              | `RT/nyanpasu_ipc/src/client/mod.rs:67-99`                  |
| R19 | 默认端点常量 `pub const SERVICE_PLACEHOLDER: &str = "nyanpasu_ipc"`；placeholder 映射为 Windows `\\.\pipe\{p}` / Unix `/var/run/{p}.sock`                                                                                                       | `RT/nyanpasu_ipc/src/lib.rs:12`；`client/mod.rs:76-79`     |
| R20 | `apply_config(&CoreApplyReq) -> Result<CoreApplyData>`；`CoreApplyReq { core_type, config_file, expected_revision: Option<RevisionIdInfo> }`，`None` = 无条件                                                                                   | `client/shortcuts.rs:51-62`；`src/api/core/apply.rs:21-31` |
| R21 | `check_config(&CoreCheckReq)`、`start_core(&CoreStartReq)`、`stop_core()`、`restart_core()`、`recover_core()`、`status() -> StatusResBody`、`events() -> EventStream`                                                                           | `client/shortcuts.rs:26-110`                               |
| R22 | `Client::recover_core()` 的语义是"清 quarantine 闩锁，幂等"——与 Local 的 `recover_quarantine()` **对称**，不是"重启核心"                                                                                                                        | `client/shortcuts.rs:69`                                   |
| R23 | `ApplyOutcomeKind` 六个 wire 分支含 **`Noop`**；`Warning` 是 `CoreApplyData.warning: Option<String>` **字段**不是分支                                                                                                                           | `src/api/core/apply.rs:34-60,70-85`                        |
| R24 | `/ws/events` 的 `Event` 三个分支：`CoreStateChanged(CoreState)`（有损两值）、`CoreStatusChanged(CoreInfos)`（**完整快照，连接即推一次、丢事件恢复后再推、每次转换都推**）、`CoreLog(Arc<LogFrame>)`。日志与状态**无顺序保证**；连接时不重放日志 | `src/api/ws/events.rs:26-68`                               |
| R25 | Service 侧 revision 载体是 `ConfigRevisionInfo { epoch, generation, source_hash, effective_hash }`，CAS token 是 `RevisionIdInfo { epoch, generation, effective_hash }`，由 `ConfigRevisionInfo::id()` 取得                                     | `RT/nyanpasu_ipc/src/api/status.rs:69-105`                 |
| R26 | 错误分类在 client 侧以 `error_kind: Option<String>` 到达                                                                                                                                                                                        | `RT/nyanpasu_ipc/src/client/mod.rs:51-53`                  |

### 1.3 app 侧现状

| ID   | 事实                                                                                                                                                                                                                                                                                                                                                                                                          | 锚点                                                                                                                                 |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| A1f  | 既有 seam：`CoreLifecyclePort { begin() -> Box<dyn CoreLifecycleLease>, status() -> CoreStatusSnapshot, on_profile_change() }`；`CoreLifecycleLease: Send`（**不是 Sync**）有 5 个方法 `check_and_promote` / `apply_candidate` / `apply_promoted` / `restart` / `stop`                                                                                                                                        | `APP/client/core_bridge.rs:43-74`                                                                                                    |
| A2f  | 组合根在 `setup.rs:42-55`，`core: Arc::new(LegacyCoreBridge::new(runtime_paths))` 从这里注入；`NyanpasuClient::try_new_with_args` 是 **sync fn**，内部 `tauri::async_runtime::block_on`；四个 typed actor 各自在自己的 `Client::new()` 里 `Actor::spawn`                                                                                                                                                      | `APP/setup.rs:42-61`；`APP/client/mod.rs:255-332,273`                                                                                |
| A3f  | **`begin()` 共 10 个生产调用点**，`rebuild_gate` 在**全部 10 处都先于** `core.begin()` 获取；`patch_running_config` 是最严格的三层顺序 `clash_patch_gate → rebuild_gate → begin()`                                                                                                                                                                                                                            | `APP/client/mod.rs:1231,1276,1351-1353,1413,1429`；`APP/client/rebuild.rs:232,257,268,282,406`                                       |
| A4f  | lease 以 **`&mut dyn`** 跨函数传递（4 处签名），并且 `patch_running_config` 把 lease **move 进 `async move` 闭包**交给 `feat::patch_clash_with_rebuild` → guard 必须 `Send` 且可移动进 boxed future                                                                                                                                                                                                           | `APP/client/mod.rs:1285-1291,1371-1400,1436-1439,1452-1459`；`APP/client/rebuild.rs:239-242`                                         |
| A5f  | `CoreLifecyclePort::status()` **没有生产调用者**；生产状态读取仍直连 `CoreManager::global().status()`                                                                                                                                                                                                                                                                                                         | `APP/ipc.rs:403`；`APP/feat.rs:292,385`；`APP/core/service/ipc.rs:83`                                                                |
| A6f  | `CoreLifecycleLease::stop()` **没有生产调用者**；生产停核走 `CoreManager::global().stop_core()`                                                                                                                                                                                                                                                                                                               | `APP/utils/help.rs:268`；`APP/utils/resolve.rs:288`                                                                                  |
| A7f  | typed actor client 房规：`Clone` via `Arc<…Inner>`；每个 client 一个手写 `call` helper，把 `CallResult::{SenderError, Timeout}` 映射成显式错误；**读用 `Some(5s)`，写用 `None`**；`Drop for …Inner` 调 `actor_ref.stop(None)`                                                                                                                                                                                 | `APP/client/application.rs:17-162`                                                                                                   |
| A8f  | 测试注入钩子已存在：`test_client_args_with_lifecycle(dir, core: Arc<dyn CoreLifecyclePort>) -> ClientSetupArgs`——**5a 直接复用**，无需新建测试图                                                                                                                                                                                                                                                              | `APP/client/mod.rs:2087-2106`                                                                                                        |
| A9f  | 需要更新的 lease 测试替身共 6 个：`MockRunningCoreBridge`/`MockCoreLease`、`TestCorePort`/`TestCoreLease`、`CompensationLease`、`BarrierCompensationLease`、`BarrierCore`/`BarrierLease`，外加 trait 上的 `#[cfg_attr(test, mockall::automock)]`。**另有第 4 个 `CoreLifecyclePort` 生产形态实现** `ProcessCoreLifecycleAdapter`（S09 的进程背书 test-only adapter），seam 若改签名它也必须跟着改             | `APP/client/mod.rs:1589-1919`；`APP/client/rebuild.rs:814-928`；`core_bridge.rs:53`；**`APP/client/process_core_bridge.rs:206,256`** |
| A16f | **Updater 直接依赖 S10 要删的 legacy lease API**：`replace_core()` 里 `CoreManager::global()` → `begin_lifecycle()` → `lifecycle.stop_core()` → `lifecycle.run_core_from(product)`。不迁移它则 5a **无法编译**；且即便能编译，legacy 单例也停不掉 CoreActor 拥有的新 manager 实例                                                                                                                             | `APP/core/updater/instance.rs:201,205,216,279`                                                                                       |
| A16g | **Updater 的真实构造链是四段**：`ipc::update_core(core_type)` → `UpdaterManager::global().write().await.update_core(&core_type)` → `instance::UpdaterBuilder::new().set_*(..).build().await` → `Updater`。**类型名是 `Updater`，不是 `UpdaterInstance`**（v2 计划写错了）；`UpdaterBuilder` 现有 5 个 `Option` 字段：`client` / `core_type` / `mirror` / `artifact` / `tag`                                   | `APP/ipc.rs:639-646`；`APP/core/updater/mod.rs:222-244`；`APP/core/updater/instance.rs:31-38,51-58`                                  |
| A21f | `spawn_health_check()` 的调用者共 **4 处**：`core/service/control.rs:97`（install）、`:225`（start）、`:320`（restart），以及 `core/service/mod.rs:32`（`init_service`）。v2 计划只列了最后一处                                                                                                                                                                                                               | 上述四处                                                                                                                             |
| A22f | `ClientSetupArgs` 的**字面构造点共 5 处**（改字段时全部要动）：`setup.rs:42`（生产）、`bridge/verge.rs:1072`、`client/mod.rs:2093`、`client/mod.rs:2325`、`client/rebuild.rs:955`（后四处为测试）                                                                                                                                                                                                             | 上述五处                                                                                                                             |
| A23f | **`Degradation` 是 mutation 响应体上的字段，不是推送通道**：前端在 `provider/index.tsx:46-53` 拦截 mutation 结果里的 `degradations` 数组再交给 handler。`Message` 枚举只有 `SetConfig(Result<(), String>)` 一个变体（`core/handle.rs:25-27`），也不适合承载它。`DegradationPhase` 的 10 个变体里**没有**核心生命周期相关项；前端 `localizeDegradationPhase` 有 `default` 分支，故新增变体不会打断 TS 类型检查 | `APP/client/runtime.rs:446-470`；`frontend/interface/src/provider/index.tsx:46-53`；`frontend/nyanpasu/src/pages/__root.tsx:123-145` |
| A24f | fake-core 的长驻启动**强制要求 `FAKE_CORE_READY_ADDR`**（TCP ready/release barrier）：doc 原文 "Long-running start requires `FAKE_CORE_READY_ADDR`… Without either `START_EXIT` or a barrier, the process fails fast with exit 2"。HTTP 端口由 `FAKE_CORE_HTTP_PORT` 决定。**只加 `GET /version` 路由不足以让它被 manager 拉起来**                                                                            | `backend/fake-core/src/main.rs:13-18,137-147`；`backend/fake-core/src/lib.rs:144,147`                                                |
| A17f | **组合根注入图不可实现（如原 S8 所写）**：`try_new_with_args` 在**进入 `block_on` 之前**就把 `core` 从 `ClientSetupArgs` 解构出来；而 typed `application` client（`enable_service_mode` 的来源）在 block 内部第 279 行才被创建。因此 `setup.rs` 无法构造一个依赖 typed 快照的 `CoreClient` 再传进来                                                                                                           | `APP/client/mod.rs:255-264,273,279`；`APP/setup.rs:42-55`                                                                            |
| A18f | **`cleanup_processes` 的真实顺序**是 `save_window_state` → **`resolve_reset()`（内部已经停核）** → `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`（第二次停核）。`resolve_reset` 全仓**只有这一个调用者**                                                                                                                                                                            | `APP/utils/help.rs:249-271`；`APP/utils/resolve.rs:286-289`                                                                          |
| A19f | **本仓 fake-core 无法满足 manager 的默认就绪探针**：manager 默认 readiness 是 `ControllerVersionProbe`——"healthy iff `GET /version` succeeds"；而 fake-core 的内置 HTTP 只对**精确** `PUT /configs` / `PATCH /configs` 作答，其余一律 404，且长驻启动强制要求 `READY_ADDR` barrier                                                                                                                            | `RT/crates/nyanpasu-core-manager/src/health/probe.rs:109-125`；`backend/fake-core/src/main.rs:22-23,137-147,305-344`                 |
| A20f | 第二条**裸线程递归恢复**路径在 `Instance` 事件循环内（核心异常退出且 `tx.send` 失败时 `std::thread::spawn` → `CoreManager::global().recover_core()`），与 `recover_core()` 自身的 5 s 重试是**两处独立**的实现                                                                                                                                                                                                | `APP/core/clash/core.rs:228-238`（另一处 `:577-582`）                                                                                |
| A10f | 测试全程**零 sleep**：oneshot barrier / `AtomicUsize` / `mockall::Sequence` / `tokio::time::pause()` / `Notify`。5a 必须延续                                                                                                                                                                                                                                                                                  | `APP/client/rebuild.rs:930-1000`                                                                                                     |
| A11f | `NyanpasuClient::shutdown()` 目前**只**关 rebuild worker；生产顺序是 `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`                                                                                                                                                                                                                                                                  | `APP/client/mod.rs:392-404`；`APP/utils/help.rs:249-272`                                                                             |
| A12f | `RunType::classify(enable_service, ipc_state)` 已由 PR-5-pre 抽出为纯函数；`IpcState` 只由 `health_check()` 翻转，兼容门在 `target_ipc_state()`                                                                                                                                                                                                                                                               | `APP/core/clash/core.rs:50-67`；`APP/core/service/ipc.rs:143`                                                                        |
| A13f | `Instance::try_new` 展示了构造核心进程所需的全部输入：core_type、app_data_dir、binary（`find_binary_path`）、config_path、pid_path                                                                                                                                                                                                                                                                            | `APP/core/clash/core.rs:83-124,711-728`                                                                                              |
| A14f | `ractor = "0.16"`；`nyanpasu-core-manager` / `nyanpasu-core-metadata` **当前不在** workspace 依赖里（PR-5-pre 的 D1=A 推迟到本阶段）                                                                                                                                                                                                                                                                          | `backend/tauri/Cargo.toml:63`；`backend/Cargo.toml:27-41`                                                                            |
| A15f | 本仓 `backend/fake-core` 对 `backend/tauri` 是 **dev-dependency**，因此**既不构建该 binary 也不设置 `CARGO_BIN_EXE_fake-core`**。既有做法是预构建（`cargo build -p fake-core`）+ 运行时定位 `fake_core::require_bin_path()`（current_exe profile/triple 查找 → 非空 `NYANPASU_FAKE_CORE` 覆盖 → target 目录回退）                                                                                             | `backend/tauri/Cargo.toml:273-281`；`backend/fake-core/src/lib.rs:399-418`；消费者示例 `APP/client/process_core_bridge.rs:18-20`     |

### 1.4 与 spec 正文的偏差（必须按事实实现，spec 措辞为准的地方已注明）

| 偏差 | design 措辞                                                                       | 实际                                                                                                                                                                                      | 处理                                                                                                                                             |
| ---- | --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| DV-A | §4 `async fn recover(&self) -> Result<()>`                                        | Local 是 `recover_quarantine()`，Service 是 `recover_core()`，二者语义都是**清 quarantine 闩锁**（R6/R22）                                                                                | `CoreBackend::recover()` 保留此名，doc 注明真实语义是 clear-quarantine，**不是**重启                                                             |
| DV-B | §4.1 "只保留机器可读 kind + 原始 message/source"                                  | 本 tag 无 `Error::kind()`（R16），R0 才加                                                                                                                                                 | 见 **S4**：隔离一个 `error_kind` 模块作为过渡；R0 合并并 bump submodule 后一步替换（去向登记在 §8）                                              |
| DV-C | §5 "Supervisor/daemon 最终放弃后，发布一次 `core_recovery_exhausted` degradation" | 无 typed 信号，只有字符串前缀（R14）                                                                                                                                                      | 见决策点 D5                                                                                                                                      |
| DV-D | §4 `async fn apply(&self, request: &CoreRequest) -> Result<CoreApplyData>`        | Local 返回本地 `ApplyOutcome`（7 分支），需要按 R12 映射成 `CoreApplyData`                                                                                                                | backend 内转换，app 侧不新增 outcome 类型                                                                                                        |
| DV-E | §6 `CoreActorState` 含 `runtime: RuntimeLifecycleState` 与 `logs: VecDeque`       | 那是 **B1/C1** 的字段                                                                                                                                                                     | 5a 的 actor state **不含**这两项，见 §0 不做清单                                                                                                 |
| DV-F | §4 把 `start` 与 `restart` 列为两个平级 backend 方法                              | 二者都**不是** request-bearing 的运行原语：`start` 在跑时报 `AlreadyRunning`，`restart` 在全新 backend 上报 `NotStarted`（R5c）。legacy `run_core` 的真实语义是"在跑就先停，再按请求启动" | backend 暴露**一个** `run(request)` 迁移原语：Local 用 `switch(spec)`（R5b 一次调用覆盖两种情形），Service 用 stop-then-start 组合。详见 S3 / S7 |

---

## 2. RQ 必答（roadmap §6.4）

### RQ-04 — `begin_operation` 的调用侧有限超时

**答：** 超时加在 **client 的 RPC 等待**上，不是 actor 里。actor 的 `AcquireOperation` handler 永远立即返回（design §3.3），真正在等的是调用方手里的 reply future。

```rust
/// 排队等待 operation 的上限。不是单次核心操作的上限——
/// 排在前面的一次 apply/start 本身就可能跑满 runtime 的 startup_timeout(30s)
/// + reconcile_timeout(30s)（见事实 R13），所以这个值必须显著大于单次操作。
/// 它存在的唯一目的是：actor 卡死时调用方能失败返回，而不是永久挂起。
const CORE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(120);
```

取值依据：runtime 默认 `startup_timeout` 30 s、`reconcile_timeout` 30 s、`stop_timeout` 10 s、`control_timeout` 10 s（R1/R13），单次最坏 ≈ 80 s；120 s 留余量且仍然有限。

超时后的行为**复用既有 drop 路径**，不新增机制：

1. `.call(..., Some(CORE_ACQUIRE_TIMEOUT))` 返回 `CallResult::Timeout`；
2. `begin_operation` 返回 `Err(OperationError::AcquireTimeout)`；
3. **pending guard 随栈展开被 drop**，其 `Drop` 用 `cast`（fire-and-forget，可在同步 `Drop` 里安全调用）发出 `ReleaseOperation { id }`；
4. actor 收到后：若该 ID 在 waiters 里 → 删除（等价取消）；若刚好已被提升为 active → 清空 active 并放行下一个 waiter；若未知 → 幂等 no-op + debug 日志。

第 4 步的三分支正是 design §3.3 的规则，**不需要为超时新增任何状态**。

**明确不实现**：TTL、auto-steal、watchdog self-message、续期、心跳。

读操作（status）**走** mailbox（§2.1.3 的 `cast RefreshHint`），沿用房规 `Some(5s)` 读超时（A7f）——它不排队等 guard，gate 非空闲时立即回缓存。watch 化是 C1 的端态。

### RQ-02 — engine revision 的 app 侧处理与 `expected_revision` CAS

**表示法（不新增 app 镜像类型）。** design §4.1 禁止在 app 侧复制 `EngineRevision`。两侧结构完全同构：

- Local：`RevisionId { epoch, generation, effective_hash }`（R9），完整信息在 `ConfigRevision`（比 `ConfigRevisionInfo` 多一个 `runtime_path`）。**`runtime_path` 一律丢弃**，理由是保持与 IPC 侧同构的单一表示、并且不把 runtime 内部路径泄进 app 状态——**不是**因为读不到：Local manager 与 app 同进程同用户，那个路径 app 是能读的。真正 app 不可访问的是 **Service** 侧 daemon 的 0o700 私有目录，所以 IPC 的 `ConfigRevisionInfo` 本来就不带它；
- Service：`RevisionIdInfo { epoch, generation, effective_hash }`（R25），完整信息在 `ConfigRevisionInfo`。

**结论：统一采用 IPC 的 `RevisionIdInfo` / `ConfigRevisionInfo` 作为 app 侧唯一表示**，理由：(a) 它已经是 app 依赖的 wire 类型且已 derive specta，C1 要 additive 暴露时零成本；(b) `LocalBackend` 内做一次 `RevisionId → RevisionIdInfo` 的字段搬运即可，转换点收敛在 backend 内部，符合 §4.1"在 backend 内转换"。

**存储。** actor state 持有 `last_revision: Option<ConfigRevisionInfo>`，作为 `CoreStatusView` 的一个字段。它是**观察到的事实缓存**，不是权威——权威永远在 runtime 那边。

**刷新来源（两条）：**

| 来源            | Local                                                 | Service                                       |
| --------------- | ----------------------------------------------------- | --------------------------------------------- |
| 操作返回值      | `run`/`stop`/`recover` 后的 `observe_status()`（R10） | 同左，经 `/status`（R21）                     |
| `RefreshStatus` | `CoreManager::status().revision`（R10）               | `Client::status().core_infos.revision`（R21） |

**重连/对账：5a 不存在这个问题。** 没有持久连接就没有断线，也没有陈旧帧——Service 侧每次 `run()` / `RefreshStatus` 都从 `/status` 重新学习当前 revision（§2.1）。`Client::events()` 的重连循环、连接身份、乱序栅栏全部移出 5a，由 **C1** 随 watch 投影一并交付。

**竞态：构造上没有。** 需要新鲜度的调用方在**持 `CoreOperationGuard` 时**刷新，而 `OperationGate` 保证同一时刻只有一个 active operation——详见 §2.1.2。

**冲突处理。** `expected_revision` 语义：

- `None` = 无条件应用（R7/R20）。**只允许在一种情况下出现：actor 尚未观察到任何 revision**——而 `apply` 要求核心已在跑（R7 `Error::NotStarted`），核心跑起来必然产生 revision，所以正常路径下 `None` 不会出现。因此规则是：**apply 一律传 `Some(last_revision.id())`；若 `last_revision` 为 `None`，视为内部不变量被破坏，返回错误而不是降级为无条件应用。**
- CAS 失败：Local `Error::RevisionConflict { expected, actual }`（R8），Service `error_kind = "revision_conflict"`（R20）。两侧都**没有应用任何东西**，所以处理方式是：用 `actual`（或重新查一次 status）刷新 `last_revision`，然后把冲突作为可重试错误上报。

**5a / 5b 分工（重要）：**

- **5a 只做"观察与存储"**：表示法、两个观察来源（§2.1.1）、操作返回值的同处理提交，由 **T-RV-01/02/03a/03b/05/06/07/08** 钉住。
- **5b 才做"CAS 应用"**：因为 5a 不改 apply 管线（决策点 D3），`expected_revision` 在 5a 没有生产调用点。上面的冲突规则属于 B1 的实现契约，5a 只负责让 `last_revision` 在那时是可信的。

> **前向引用（不在本阶段作答）：** RQ-01 完整 post-commit 失败矩阵、RQ-03 apply parity 含 `Noop`——均由 **PR-5b 计划**回答。事实 R11/R23 已为其备好素材。

---

## 2.1 观察模型（5a 范围内）

> **范围裁定（2026-08-02，leader）：5a 不做推送式观察。** v3–v6 曾围绕"订阅流 + 陈旧帧栅栏"反复迭代（`ConnectionId`、`subscription_epoch`、握手、重连循环、乱序判定），六轮审查全部集中在这套机制上。依据 task.md：**"backend status/events 投影到 actor watch snapshot；status read 不走 mailbox RPC" 明确写在 C1（PR-5c）卡上**，而 A1–A3 与 A-Exit（operation 语义、Local/Service parity、seam 迁移）**没有一条需要实时推送流**。因此该机制整体移出 5a。
>
> **陈旧帧问题在没有推送流时不存在**——这不是把 bug 藏起来，是把产生 bug 的结构删掉。

### 2.1.1 读路径：actor→client watch 投影（裁定 A-v2，2026-08-02）

**为什么 watch 必须回来。** v8 让 `status()` 走 mailbox RPC，八审指出这会破坏活性：ractor 顺序处理消息，`Run` / `apply` 的 handler 可能 await 30–80 s（runtime 的 `startup_timeout` 30 s + `reconcile_timeout` 30 s，R1/R13），期间**排在它后面的每一个读都要等**，5 s 读超时必然失败。**这正是 design.md §6 当初指定 watch 读的原因。**

区分两件被混为一谈的事：

| 机制                          | 5a 是否有 | 说明                                                                 |
| ----------------------------- | --------- | -------------------------------------------------------------------- |
| backend → actor **推送流**    | **否**    | 订阅任务 / 连接身份 / 重连 / 陈旧帧栅栏，全部归 **C1**（第七轮裁定） |
| actor → client **watch 投影** | **是**    | **单一写入者 = actor**，无并发写、无栅栏需求，约 30 行               |

被砍掉的是前者。后者从来不是有争议的机制。

**规则：**

- actor 持有 `watch::Sender<CoreStatusView>`；**每一次提交都发布投影**——操作返回观察、`RefreshStatus` 结果、槽位转换（含 `Failed` / `None` 瞬态）、`shutdown`；
- `CoreClient` 持 `watch::Receiver`，`status()` 是**同步的 watch 克隆**，**完全不碰 mailbox**；
- 因此"读永不阻塞"成为**结构性事实**，不再需要 5 s 读超时（该常量随之删除）。

> 这提前交付了 task.md C1 卡 "status read 不走 mailbox RPC" 的**读半边**；C1 仍负责 backend→actor 推送与 `LogFrame` ring。已在处置表记账。

### 2.1.2 写路径：两个观察来源

| 来源                | 触发                                               | 竞态处理                                               |
| ------------------- | -------------------------------------------------- | ------------------------------------------------------ |
| **操作返回值**      | `run` / `stop` / `recover` 的 `BackendObservation` | actor 在**同一消息处理内**提交并发布 watch，然后才回复 |
| **`RefreshStatus`** | 守卫调用方按需查询                                 | 必填 `OperationId`，须等于 active（见 2.1.3）          |

`apply` **不是**观察来源：D3=A 下它仍走 lease 内的 `api::put_configs`，不经过 actor。

### 2.1.3 刷新的两种调用形态

```rust
/// 守卫刷新（5b 的事务内刷新）。**必填** operation——调用方必然持有 guard。
RefreshStatus {
    operation: OperationId,
    reply: RpcReplyPort<Result<BackendObservation, CoreActorError>>,
},

/// UI 提示（fire-and-forget，`cast` 而非 `call`）。无 operation、无 reply。
/// 仅在**出队时 gate 空闲**才执行刷新；否则**直接丢弃**——
/// 因为此刻必有一个 active operation，它结束时会提交并发布新状态，
/// 再刷一次既多余又会排在慢操作后面。
RefreshHint,
```

`CoreActorError`（八审要求的明确回复类型）：

```rust
pub enum CoreActorError {
    StaleOperation,
    NoBackend { last_error: Arc<CoreBackendError> },
    Backend(Arc<CoreBackendError>),
    ShuttingDown,
}
```

**为什么 hint 用 `cast`：** 它没有返回值需求（结果通过 watch 到达），也就不该占用调用方的等待。丢弃是安全的——丢弃的前提正是"有操作在飞"，而那个操作的提交马上就会发布。

### 2.1.4 UI 读路径与 D5

`NyanpasuClient::core_status()` = **watch 读**（立即返回）+ **`cast RefreshHint`**（不等待），再投影成既有元组 wire。

D5 的"UI 读取观察到耗尽"因此是**最终一致**的：hint 在 gate 空闲时出队 → 刷新 → 提交并发布 watch → **下一次读取/渲染**看到耗尽并发布一次 degradation。T-RV-09 逐步钉住这个序列。

> 前端 `useCoreStatus` 用 react-query，invalidate/重新挂载都会再读一次，因此"下一次读取"在真实交互中很快到来。

### 2.1.5 观察载荷与忠实生命周期

```rust
/// 观察载荷：**忠实**生命周期 + 投影视图。
/// 只在两条路径上出现：mutation 的返回值、`RefreshStatus` 的结果。
#[derive(Clone)]
pub(crate) struct BackendObservation {
    pub(crate) view: CoreStatusView,
    pub(crate) lifecycle: FaithfulLifecycle,
}

/// 归一化的忠实生命周期。两个 backend 的原生类型都投影到它，
/// **crate-private、不上 wire、不进 `CoreStatusView`**。
/// 存在理由：`CoreStatusView.state` 是两值 `CoreState`，看不见 `Starting`/`Restarting`，
/// 而 D5 的 latch 必须在投影**之前**区分它们（否则"启动→再次耗尽"无法复位）。
#[derive(Clone)]
pub(crate) enum FaithfulLifecycle {
    Stopped { reason: Option<String> },
    Starting,
    Running,
    Restarting,
    Switching,
    Stopping,
}
```

两侧映射（**必须逐条实现**）：

| `FaithfulLifecycle`  | Local（`nyanpasu_core_manager::CoreState`，R10/state.rs:106-131）    | Service（`CoreInfos.detail`，R5d/status.rs:107-122） |
| -------------------- | -------------------------------------------------------------------- | ---------------------------------------------------- |
| `Stopped { reason }` | `Stopped { reason }`（`StopReason` 转字符串，D5 的耗尽前缀在此判定） | `Stopped { reason }`                                 |
| `Starting`           | `Starting { .. }`                                                    | `Starting { .. }`                                    |
| `Running`            | `Running { .. }`                                                     | `Running { .. }`                                     |
| `Restarting`         | `Restarting { .. }`                                                  | `Restarting { .. }`                                  |
| `Switching`          | `Switching { .. }`                                                   | `Switching { .. }`                                   |
| `Stopping`           | `Stopping { .. }`                                                    | `Stopping { .. }`                                    |

**Service 侧 `detail` 缺失时**（老 daemon 或字段未送达）：按两值 `CoreState` 保守映射——`Running → Running`，`Stopped(reason) → Stopped { reason }`。此时看不见过渡态，D5 的 latch 会退化为"只在 `Running` 复位"，**这是可接受的降级**（v1 daemon 本就进不了 Service backend，见 PR-5-pre 兼容门）。

### 2.1.6 `RefreshHint` 的合流（round-9 #3）

UI 每次渲染都可能调 `core_status()`，若每次都 `cast` 一条 hint，空闲期会积压出 N 次 backend 查询。**用一个共享 pending 位合流**：

```rust
// CoreClientInner 增加：hint 是否已在途。
hint_pending: Arc<AtomicBool>,
```

- client 侧 `hint_refresh()`：`compare_exchange(false, true)` 成功才 `cast`；失败说明已有一条在途，**直接返回**；
- actor 侧处理 `RefreshHint` 时**第一件事**就是把该位清回 `false`，然后再判 gate、再决定是否查 backend。

于是**任意时刻至多一条 hint 在途**。清位发生在处理开始处，因此清位之后到达的读会重新排一条，不会丢失"最新一次读也想要新鲜值"的意图。

> 该原子位由 client 与 actor 共享（`Arc<AtomicBool>`），是本设计中**唯一**跨 actor 边界的共享可变量。它不承载任何状态、只做去重，因此不违反"actor 状态不得经 `Arc<Mutex>` 外泄"（CLAUDE.md §8）——在此显式登记为窄用途例外。

### 2.1.6.1 最终一致性的一个例外（round-9 #4）

§2.1.4 说 D5 是最终一致的，但有一个**必须写明的例外**：D3=A 下 `apply_promoted` 仍走 lease 内的 `api::put_configs`，它**不经过 actor，因此不产生观察、也不发布 watch**。所以：

- 一次外部 apply 改变了运行态之后，watch 里的视图**在下一次观察之前是陈旧的**；
- 恢复新鲜的路径就是下一次 idle hint（或下一次经 actor 的 mutation）；
- 这条例外随 **B3** 消失——届时 apply 统一走 `CoreBackend::apply`，本身就是观察来源。

T-RV-13 钉住这条：外部 apply 期间读到旧值，随后 idle hint 到达 → watch 更新。

### 2.1.7 watch 通道的具体接线（编译级，round-9 #1）

**通道在组合根创建，`Sender` 随启动参数交给 actor，`Receiver` 交给 client。** 这样 `CoreClient` 从构造那一刻起就持有可读的接收端，不存在"actor 还没起来但已经有人读"的空窗。

```rust
// APP/core/actor/mod.rs —— actor 启动参数
pub(crate) struct CoreActorArgs {
    pub(crate) mode: RunType,
    pub(crate) requests: CoreRequestFactory,
    pub(crate) degradation: Arc<dyn CoreDegradationSink>,
    /// 组合根创建通道后把发送端交进来（唯一写入者 = actor）。
    pub(crate) status_tx: watch::Sender<CoreStatusView>,
}

// APP/client/core.rs
struct CoreClientInner {
    actor_ref: ActorRef<CoreActorMessage>,
    next_operation: AtomicU64,
    /// 读端。`status()` 直接 borrow-clone，永不发消息（§2.1.1）。
    status_rx: watch::Receiver<CoreStatusView>,
}

impl CoreClient {
    pub(crate) async fn new(args: CoreClientArgs) -> anyhow::Result<Self> {
        // 初值 = "尚未观察到任何后端状态"：state = Stopped(None)、
        // state_changed_at = 0、revision = None、recovery_exhausted = false。
        // 与 legacy `CoreManager::status()` 在无 instance 时的返回值一致，
        // 因此 UI 在 actor 完成首次观察前读到的东西与迁移前完全相同。
        let (status_tx, status_rx) = watch::channel(CoreStatusView::initial());
        let actor_ref = Actor::spawn(None, CoreActor, CoreActorArgs { /* … */ status_tx })
            .await?.0;
        Ok(Self { inner: Arc::new(CoreClientInner { actor_ref, next_operation: AtomicU64::new(1), status_rx }) })
    }

    /// 同步 watch 克隆，零 mailbox（§2.1.1）。
    pub fn status(&self) -> CoreStatusView {
        self.inner.status_rx.borrow().clone()
    }

    /// 守卫刷新（§2.1.3）。只有持 guard 的调用方能用。
    pub async fn refresh_status(
        &self,
        operation: &CoreOperationGuard,
    ) -> Result<BackendObservation, CoreActorError> {
        match self.inner.actor_ref
            .call(|reply| CoreActorMessage::RefreshStatus { operation: operation.id(), reply }, None)
            .await
        {
            Ok(CallResult::Success(result)) => result,
            // actor 已终止 / 回复端被丢弃：都只可能发生在关停之后。
            Ok(CallResult::SenderError) | Err(_) => Err(CoreActorError::ShuttingDown),
            Ok(CallResult::Timeout) => unreachable!("guarded refresh passes no timeout"),
        }
    }

    /// UI 提示，fire-and-forget（§2.1.3）。发送失败＝actor 已关停，忽略即可。
    pub fn hint_refresh(&self) {
        let _ = self.inner.actor_ref.cast(CoreActorMessage::RefreshHint);
    }
}
```

**唯一的提交路径。** actor 内所有状态变更都走同一个 helper，杜绝"改了缓存忘了发布"或"发布了但没过 latch"：

```rust
impl CoreActorState {
    /// 提交一次观察：latch 判定（**投影前**吃 `lifecycle`，D5）→ 更新缓存 → 发布 watch。
    /// 所有调用点：run/stop/recover 的返回值、RefreshStatus/RefreshHint 的结果、
    /// 槽位转换（`None` 瞬态与 `Failed`）、shutdown。
    fn commit(&mut self, observation: BackendObservation) {
        self.evaluate_recovery_latch(&observation.lifecycle);   // D5，投影前
        self.observed = observation;
        let _ = self.status_tx.send_replace(self.observed.view.clone());
    }
}
```

**没有 backend 时的观察合成**（槽位 `None` / `Failed` / shutdown 各有确定值）：

| 场景            | 合成的 `BackendObservation`                                                                 | 何时提交            |
| --------------- | ------------------------------------------------------------------------------------------- | ------------------- |
| 换槽瞬态 `None` | `lifecycle = Stopped { reason: None }`，`view.state = Stopped`，`revision` 保留上一次已知值 | 取出旧 backend 之后 |
| `Failed{error}` | `lifecycle = Stopped { reason: Some(error.to_string()) }`，`view.state = Stopped`           | 构造失败写回槽位时  |
| `shutdown`      | `lifecycle = Stopped { reason: None }`，`view.state = Stopped`                              | 关闭 backend 之后   |

三者都经 `commit()`，因此**都会发布 watch**——T-RV-11 / T-RV-12 分别钉住瞬态与关停两处。

> `BackendObservation` 与 `FaithfulLifecycle` 必须 `#[derive(Clone)]`（round-9 #3）：actor 既要留存缓存又要把它经 `RefreshStatus` 回给调用方。`CoreStatusView` 同理（watch 需要）。
>
> 模块路径修正：`APP/core/actor/mod.rs` **不**声明 `pub mod client`——唯一的 typed client 在 `APP/client/core.rs`，与其它四个 client 同层（A7f 房规）。

### 2.1.8 转出到 C1 的内容（前向指针）

以下**明确不在 5a**，由 **PR-5c / C1** 交付：backend→actor 的 status/events **推送订阅**；100 条 `LogFrame` ring。**注意：watch 投影的读半边（`status()` 不走 mailbox）已在 5a 交付**（§2.1.1，裁定 A-v2），C1 只需把推送源接到 actor 的 `commit()` 上——投影与读路径无需再改。

---

## 3. 需 leader 裁定的决策点

> 未另行裁定则按推荐项执行。
>
> **Leader 裁定（2026-08-02）：D1=A、D2=A、D3=A、D4=A、D5=A、D6=120s。** 全部按推荐项执行。D5 附带建议（上游 `StopReason` 加 `RestartBudgetExhausted` typed 变体）**不并入已收口的 R0 分支**——记入待用户授权的上游事项，授权后作独立小 PR；本阶段按字符串前缀实现。

### D1 — workspace 依赖加哪几个 crate？

PR-5-pre 的 D1=A 把 `nyanpasu-core-manager` / `nyanpasu-core-metadata` 推迟到"首个真实消费者"，那就是本阶段。

- **推荐 A**：只加 `nyanpasu-core-manager` 与 `nyanpasu-core-metadata` 两条。前者是 `LocalBackend` 的直接依赖；后者提供 `LogFrame` / `ClashCoreKind`，且 R0 之后的 `CoreErrorKind` 也在其中——虽然 5a 的日志 ring 是 C1 范围，`ClashCoreKind`（= `CoreKind`）在构造 `CoreSpec` 时就要用到，属真实消费。
- **选项 B**：再加 `clash-api`。**不推荐**——app 侧无直接消费者（`clash_api::Host` 只在 `CoreStatus.controller` 里出现，5a 不读它），加了就是死条目。

### D2 — backend 的构造时机

`CoreManager::new()` 会取运行目录独占锁（R2），所以"两个 backend 都常驻"不可行。

**裁定 A：pre_start 只构造当前模式匹配的那一个；`SetBackend` 时换槽。** `SetBackend` 必须在 operation guard 下执行（design §3.4 已列入必须持 guard 的清单）。

**换槽协议（必须逐字实现——`shutdown()` 并不释放目录锁）。** 事实 R6b：锁是 `atomic_fs::DirLock`，挂在 `Inner` 的最后一个字段上，**只在最后一个 `Arc<Inner>` drop 时才释放**。所以"shutdown 旧的 → 构造新的"如果还有任何一个 `Arc<CoreManager>` 存活（例如订阅任务里那份 clone），新 manager 会拿不到锁并以 `Error::RuntimeDirectoryOwned` 失败。因此：

1. **槽位建模为 `BackendSlot::{Ready, Failed{error}}`**——换槽期间必然存在一个短暂空位，状态里必须能表达它，并且失败诊断要能被后续 mutation 复现；
2. 调 `backend.shutdown()`（优雅停核 + 归档 sink）；
3. **显式 drop 掉 backend 值本身以及所有 `Arc<CoreManager>` 克隆**，让 `Inner` 的引用计数归零、目录锁释放；
4. 再构造替换 backend；
5. 成功则写回 `Ready(new)`，失败则写回 `Failed { error }`。

> **比 v6 简单了一步**：5a 不起订阅任务（§2.1），因此**没有需要 cancel + join 的后台任务**，也就不存在 v6 里那个"`SetBackend` 在 join 任务、任务在等同一个 handler 回复"的死锁风险。持有 `Arc<CoreManager>` 的只有 backend 自身，drop 它即释放。

**无 backend 失败态。** 槽位为 `Failed { error }` 时：所有 mutation 返回 `CoreBackendError::NoBackend { last_error: error.clone() }`（诊断从槽位复现，不需要额外字段）；`Status` 返回最后已知的 `CoreStatusView` 并把 `state` 置为 `Stopped`。后续 `SetBackend` 可以重试装载。**不做**自动重试循环。

配套顺序事实：组合根（`setup.rs`）先于 `init_service()` 运行，此时 `IpcState` 仍是 `Disconnected`，`RunType::classify` 必然给出 `Normal`（A12f）。所以**启动时总是先建 Local backend**，随后健康检查若判定兼容再发 `SetBackend(Service)`。这与 fail-closed 语义一致，是期望行为。

### D3 — 5a 是否把 apply 改道到 `CoreBackend::apply`？（**本计划最关键的范围裁定**）

- **推荐 A：不改道。** `check_and_promote` / `apply_candidate` / `apply_promoted` 在新的 lease 适配器里**保持现有实现**（候选检查 + 原子 promote + `api::put_configs` 推送）。理由：(1) task.md A3 列的是 "start/stop/restart/status 改走 actor"，apply **不在其中**；(2) 统一 full-config apply 是 design §7 白纸黑字的 **5b/B3** 范围；(3) `put_configs` 是对核心 external-controller 的 HTTP 调用，与"谁拉起了进程"无关，因此 CoreActor 接管进程所有权后它照常工作；(4) 让 5a 保持"纯所有权搬移"，diff 可审。
  - 代价：5a 结束时短暂存在两条配置应用路径（legacy PUT 用于生产，`CoreBackend::apply` 仅被测试覆盖）。这正是 B3 要消灭的。
- **选项 B**：5a 就改道。会把 B1（Promoted/Applied 入 actor）和 B3（删补偿层）的一部分提前，且 `expected_revision` 的失败矩阵（RQ-01）尚未定义，风险明显更高。

### D4 — `CoreBackend::apply` 是否在 5a 实现？

与 D3 配套。task.md A1 明确写了 "只实现 check/start/apply/stop/restart/recover"。

- **推荐 A：实现，但只被 Test backend 与 parity 测试消费，生产不接线。** 依据是 A1 卡的显式要求；同时它让 R12 的 outcome 映射（`DurabilityUncertain` 拆 warning、可嵌套两层）在 5a 就被测试钉住，5b 接线时只剩接线。
- **选项 B**：5a 不实现 apply，5b 再加。更严格地遵守 CLAUDE.md §2（不写投机代码），但违反 A1 卡的字面要求，且把 outcome 映射的风险全压到 5b。

### D5 — `core_recovery_exhausted` 怎么判定？

事实 R14：没有 typed 信号，只有 `StopReason::Error` 里的字符串前缀。

**裁定 A：常量 + 纯函数集中一处字符串匹配，并用测试钉住前缀。** Local 从 `CoreStatus.state` 取，Service 从 `CoreState::Stopped(Some(msg))` / `CoreStatusChanged` 的 detail 取（同一字符串，因为 daemon 内跑的是同一个 manager）。

```rust
/// 上游 `instance.rs:631-641` 在重启预算耗尽时写入的前缀。
/// 这是目前唯一的机器可读线索——上游没有 typed 信号（见计划 §1.1 R14）。
/// 若上游后续加了 typed 变体，这里连同 `is_recovery_exhausted` 一并删除。
const RECOVERY_EXHAUSTED_PREFIX: &str = "core kept crashing; restart budget exhausted";
```

**但"算出一个 flag"不等于满足 design §5。** design.md:227 的原文要求是"Supervisor/daemon 最终放弃后，**发布一次** `core_recovery_exhausted` degradation"——这是一个**恰好一次的发布行为**，不是一个可反复读取的布尔位。而状态推送是幂等重放的（R24 明确说 `CoreStatusChanged` 可能因重连/丢事件恢复而重复），朴素实现会对同一次耗尽事件重复发布。因此必须补两样东西：

**（a）注入式 degradation sink + 具体生产实现。** 既有 `UiEventSink`（`APP/client/event_sink.rs:11-31`）只有 `state_changed` / `notice_message` / `update_systray*`，**没有** degradation 通道，不要往里塞。事实 A23f 还确认了：`Degradation` 是挂在 **mutation 响应体**上的字段（前端在 `provider/index.tsx:46-53` 从 mutation 结果里取 `degradations` 数组），**没有现成的推送通道可继承**——而 `core_recovery_exhausted` 是无人调用时自发产生的事件。

```rust
/// CoreActor 向应用层发布的核心降级事件。
/// 刻意只有一个方法：5a 只需要 `core_recovery_exhausted` 一种。
#[cfg_attr(test, mockall::automock)]
pub trait CoreDegradationSink: Send + Sync + 'static {
    fn publish(&self, degradation: crate::client::runtime::Degradation);
}
```

**完整 DTO 取值**（复用既有 `Degradation`，`APP/client/runtime.rs:448-470`，四个字段全部定死）：

| 字段        | 值                                                                                                                                                                                                                                                                             | 理由                                                                                                                                                                                                                                                                                                                 |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `phase`     | **`DegradationPhase::CoreLifecycle`（本阶段新增的变体）**                                                                                                                                                                                                                      | **leader 裁定（2026-08-02）**：现有 10 个变体没有核心生命周期项，用 `RuntimeApply` 承载"监督重启预算耗尽"是**语义谎言**，且会直接显示成 UI 上的相位标签。零 diff 闸门的目的是拦**非预期**的 wire 变化，而一个**声明在案、精确、纯新增**的期望同样达成该目的。C1 无论如何都要这个变体，推迟只会先发一段时间的错标事件 |
| `code`      | `"core_recovery_exhausted"`                                                                                                                                                                                                                                                    | design §5 指定的稳定 snake_case 码                                                                                                                                                                                                                                                                                   |
| `message`   | `format!("core restart budget exhausted; the core is not running ({reason})")`，`reason` 取自 `StopReason::Error` 原文。**整条最终 `message`（前缀 + reason + 后缀）上限 512 字节**——超限时**只截断 reason 段**，且必须按 UTF-8 字符边界截断（不得切出半个码点），截断处补 `…` | 面向支持排查，保留 runtime 原始诊断；512 与上游 `CoreHealthInfo.last_error` 的上限一致                                                                                                                                                                                                                               |
| `retryable` | `true`                                                                                                                                                                                                                                                                         | 用户可显式 `restart` / `recover` 重试                                                                                                                                                                                                                                                                                |

**生产实现（`TauriCoreDegradationSink`）：** 放在 `APP/client/event_sink.rs` 旁，与 `TauriUiEventSink` 同层（都是 Tauri 边界适配器）。它持有 `Arc<dyn UiEventSink>`，`publish` 时做两件事：

1. `tracing::warn!(code = %d.code, phase = ?d.phase, message = %d.message, "core degradation")` —— 结构化日志，进 `collect_logs` 的支持包；
2. `ui_sink.notice_message(&Message::SetConfig(Err(d.message.clone())))` —— 复用**既有**的用户可见通知通道（`core/handle.rs:25-27` 只有这一个变体），让用户当场看到"核心反复崩溃已放弃重启"。

> 为什么不新开一条推送 wire：那需要新 event URI + 新 wire 类型 + 前端订阅，属于 **C1**（status/event 投影）的范围。当前实现让**协议义务**（恰好一次、可注入、可 mock）在 5a 完整成立，**传输**在 C1 升级——sink trait 不变，只换实现。

**新增 `DegradationPhase::CoreLifecycle` 的具体改动（backend-only，leader 裁定）：**

- `APP/client/runtime.rs:459-470` 的 `DegradationPhase` 加一个变体 `CoreLifecycle`（serde/specta 为 `snake_case`，即 `'core_lifecycle'`）；
- **不动前端**：`localizeDegradationPhase`（`frontend/nyanpasu/src/pages/__root.tsx:123-145`）有 `default` 分支兜底显示，本阶段**不加**本地化条目——它随 C1 一并补；
- roadmap §9 的 `DegradationPhase` 列表是"至少覆盖"，新增变体不违反它；
- bindings 因此**有且仅有这一处**新增，S13 已把期望从"零变化"改成"恰好这一处"。

**注入路径：** `ClientSetupArgs` 新增 `degradation: Arc<dyn CoreDegradationSink>` 字段（**5 个字面构造点全部要改**，事实 A22f）；生产在 `setup.rs` 传 `Arc::new(TauriCoreDegradationSink::new(ui_sink.clone()))`，测试传 `Arc::new(MockCoreDegradationSink::new())`。

**（b）per-episode latch。** actor state 持 `recovery_exhausted_published: bool`：

- 观察到耗尽且 latch 为 `false` → `publish()` 一次并置 `true`；
- latch 为 `true` 时再观察到同一状态 → **不发布**；
- **latch 复位条件（只有两条）**：观察到核心重新进入非耗尽的**活跃**状态，或 backend 身份变化（`SetBackend` 换槽）。**`recover` 成功本身不复位**——它只清 quarantine、不拉起核心，若复位则重放的同一耗尽快照会二次发布同一 episode。

**观察时机（裁定 A 之后已有确定机制）。** 没有推送流，耗尽状态在**有人观察时**被发现，来源是 §2.1 的两条：任一 mutation 的返回值，或一次 `RefreshStatus`。**UI 的 `core_status()` 走 `cast RefreshHint`（§2.1.3），gate 空闲时会真正查询 backend**（§2.1.2 规则 (d)），因此：

- Local 核心在最后一次操作之后耗尽重启预算 → **下一次 UI status 读取**即观察到并发布一次（T-RV-09 钉住）；
- 无需周期性自消息，也无需依赖 service 健康轮询（那查的是 daemon 状态，不是核心状态）；
- latch 保证同一 episode 只发一次，复位条件见上。

**（c）latch 必须在投影之前判定（round-3 #5）。** `CoreStatusView.state` 是**有损两值** `CoreState`，看不见 `Starting`（事实 R5d）。若在投影后判定，一次"启动→再次崩溃耗尽"的失败重试会因为从没观察到 `Running` 而无法复位 latch，第二次耗尽就不会发布。因此：

- backend 的观察结构里携带一个 **crate-private 的忠实生命周期视图**（Local 用 `nyanpasu_core_manager::CoreState`，Service 用 `CoreInfos.detail`），**不进** `CoreStatusView`、不上 wire；
- latch 的判定与复位**读这个忠实视图**：`Starting` / `Running` / `Restarting` / `Switching` 均视为"活跃"，复位 latch；
- 投影成两值 `CoreStatusView` 是判定**之后**的最后一步。

测试见 T-BK-06 / T-BK-07 / T-BK-12。

**附带建议（需用户授权，不在本阶段执行）**：给上游提一个小改动，在 `StopReason` 上加 `RestartBudgetExhausted` 变体。**Leader 已裁定不并入已收口的 R0 分支**，授权后作独立小 PR。

### D6 — `CORE_ACQUIRE_TIMEOUT` 取值

推荐 **120 s**，依据见 RQ-04。若 leader 认为应更激进（例如 60 s），需同时接受"前序 apply 跑满 runtime 超时时后续操作可能误超时"的代价。

---

## 4. 实施步骤

> 每步给出编辑内容 → 验证命令 → 通过判据。全程**不要**跑 `cargo clippy -- -D warnings`（仓库本就红）。
> 记忆事项：本仓共享 target 曾出现 kache 污染导致本地 clippy 假红；若遇到，用独立 target 目录复验再判定。

### S1 — workspace 依赖（按 D1=A）

`backend/Cargo.toml` 的 `[workspace.dependencies]` `# --- nyanpasu ---` 段追加两条，沿用既有注释风格：

```toml
nyanpasu-core-manager = { path = "nyanpasu-runtime/crates/nyanpasu-core-manager" }
nyanpasu-core-metadata = { path = "nyanpasu-runtime/crates/nyanpasu-core-metadata" }
```

`backend/tauri/Cargo.toml` 的 `# Local Dependencies` 段追加：

```toml
nyanpasu-core-manager = { workspace = true }
nyanpasu-core-metadata = { workspace = true }
```

**验证：**

```powershell
cargo metadata --manifest-path .\backend\Cargo.toml --format-version 1 | Out-Null; $LASTEXITCODE
Select-String -Path .\backend\Cargo.lock -Pattern '^name = "nyanpasu-core-manager"' -Context 0,2
cargo tree --manifest-path .\backend\Cargo.toml --duplicates --edges normal | Out-String
```

**通过判据：** exit 0；两个 crate 以 path 依赖入 lock（无 `source =` 行）；`--duplicates` **没有新增重复组**（出现新重复组 → 停止并上报，不要自行改 feature）。

### S2 — 请求与状态投影类型（纯类型，零行为）

新文件 `APP/core/actor/types.rs`（模块树见 S5）。

```rust
/// 一次核心操作的完整输入。两个 backend 各自把它翻译成自己的形状：
/// Local → `InstanceSpec`（事实 R7），Service → `CoreStartReq` / `CoreApplyReq`（R20）。
#[derive(Debug, Clone)]
pub struct CoreRequest {
    pub core_type: nyanpasu_utils::core::CoreType,
    pub binary_path: camino::Utf8PathBuf,
    pub config_path: camino::Utf8PathBuf,
    pub working_dir: camino::Utf8PathBuf,
    pub pid_path: Option<camino::Utf8PathBuf>,
}

/// 给 UI 的最小状态投影。刻意**不**镜像 runtime 的 `CoreStatus`：
/// 只保留 5a 的调用点真正需要的字段（见事实 A5f 的四个读取点）。
#[derive(Debug, Clone)]
pub struct CoreStatusView {
    pub state: nyanpasu_ipc::api::status::CoreState,
    pub state_changed_at: i64,
    pub run_type: crate::core::RunType,
    /// 观察到的运行配置 revision（RQ-02）。权威在 runtime，这里只是缓存。
    pub revision: Option<nyanpasu_ipc::api::status::ConfigRevisionInfo>,
    /// runtime 侧重启预算耗尽（事实 R14）。
    pub recovery_exhausted: bool,
}
```

**`CoreRequest` 的构造需要一个显式的工厂（round-6 #7 / round-7 #4）。** 注入 `RuntimePaths` **不够**——它只有 `product` 与 `candidate_dir`（`APP/client/runtime.rs:193-221`），而 `CoreRequest` 还需要 `binary_path` / `working_dir` / `pid_path`。**注入 `PathResolver` 也不够**：它的 `app_install_dir()` 直接委托 `dirs::app_install_dir()`（`APP/utils/path.rs:82-84`），测试无法重定向。因此二进制查找必须是**独立可注入的策略**：

```rust
/// 核心二进制的查找策略。生产实现复刻既有"data dir → install dir"顺序；
/// 测试（含 S12 parity）注入固定路径，指向 `require_probe_bin_path()` 的探针核。
/// 这一层单独存在的唯一理由：`PathResolver::app_install_dir()` 走 `dirs::*`，不可注入。
pub(crate) trait CoreBinaryResolver: Send + Sync + 'static {
    fn resolve(&self, core_type: &CoreType) -> anyhow::Result<Utf8PathBuf>;
}

/// 把"当前意图"物化成 `CoreRequest`。**所有路径都来自注入依赖，无 `dirs::*()`**。
/// `Clone` 是必需的：`CoreModeReconciler` derive 了 `Clone` 且以它为字段（round-7 #6）。
#[derive(Clone)]
pub(crate) struct CoreRequestFactory {
    data_dir: Utf8PathBuf,                     // 从 PathResolver 取值后固化，非按需调用
    pid_path: Utf8PathBuf,
    runtime_paths: RuntimePaths,               // product 路径
    binary: Arc<dyn CoreBinaryResolver>,       // 可注入的二进制查找
}

impl CoreRequestFactory {
    /// `core` 来自 typed 快照，字段名是 **`core`**（`clash_core` 只是 serde alias，
    /// `nyanpasu-config/src/application/mod.rs:97-99`）。
    pub(crate) fn for_product(&self, core: ClashCore) -> anyhow::Result<CoreRequest> {
        let core_type: nyanpasu_utils::core::CoreType = (&core).into();  // 显式转换
        Ok(CoreRequest {
            binary_path: self.binary.resolve(&core_type)?,
            core_type,
            config_path: self.runtime_paths.product().to_owned(),
            working_dir: self.data_dir.clone(),
            pid_path: Some(self.pid_path.clone()),
        })
    }
}
```

**归属：** 工厂存进 `NyanpasuClientInner`（`core_mode_reconciler()`、`restart_core()`、facade 的 service 方法、以及 S9.1 的 Updater 都要用它）。`ClientSetupArgs` 增加 `binary_resolver: Arc<dyn CoreBinaryResolver>`，生产在 `setup.rs` 传 OS 实现，测试传固定路径实现。

**Updater 的路径（round-9 #2 修正，推翻 round-7 的预构造裁定）。** v8/v9 让 facade 在 `update_core()` **调用时**构造 `CoreRequest` 再交给 Updater——**那是一个陈旧竞态**：`replace_core` 发生在**下载与解压之后**（`UpdaterState` 序列 Downloading → Decompressing → Replacing，`APP/core/updater/instance.rs:139,200,291`），大文件下载可能持续数分钟，期间用户完全可能换核。用调用时刻的快照去比较/重启，会**重启一个用户已经切走的核**，或**漏停一个下载期间才变成活跃的核**。

**正确做法：请求在替换 guard 内、替换时刻才解析。** Updater 只携带**目标核类型**，另注入一个窄 provider：

```rust
/// 在替换时刻解析"当前意图"的窄接口。实现由 facade 提供，内部读 typed 快照 + 路径工厂；
/// Updater 因此既不认识 `ApplicationClient` 也不认识路径细节。
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub(crate) trait CoreRequestProvider: Send + Sync + 'static {
    /// 返回**此刻**的运行请求（当前 typed 快照决定 core type）。
    async fn current(&self) -> anyhow::Result<CoreRequest>;
}
```

**可见性约定（round-8 #4）：** 5a 新增的实现面统一 `pub(crate)`——`CoreRequest` / `CoreStatusView` / `BackendObservation` / `FaithfulLifecycle` / `CoreBackend` / `BackendSlot` / `CoreActorError` / `CoreRequestFactory` / `CoreBinaryResolver` / `CoreModeReconciler`。只有 `CoreClient`、`CoreOperationGuard`、`CoreDegradationSink`、`ServiceControlOps` 需要 `pub`（前二者被 client 层引用，后二者是注入点）。

**验证：** `cargo check -p nyanpasu --all-features`（或 `pnpm lint:clippy`）。

### S3 — `CoreBackend` 封闭 enum

新文件 `APP/core/actor/backend.rs`。

```rust
pub enum CoreBackend {
    Local(LocalBackend),
    Service(ServiceBackend),
    #[cfg(test)]
    Test(TestBackend),
}

impl CoreBackend {
    pub async fn check(&self, request: &CoreRequest) -> Result<(), CoreBackendError>;

    /// **唯一的 request-bearing 运行迁移原语**（DV-F）。语义 = legacy `run_core_from`：
    /// 核心未跑就按 `request` 启动；已在跑就迁移到 `request` 指定的 spec。
    /// 刻意**不**暴露 `start` / `restart` 两个平级方法——它们各自只在半边状态空间有效
    /// （R5c），把选择权交给调用方就是把 bug 交给调用方。
    pub async fn run(&self, request: &CoreRequest) -> Result<BackendObservation, CoreBackendError>;
    pub async fn stop(&self) -> Result<BackendObservation, CoreBackendError>;
    /// 清除 runtime 的 quarantine 闩锁。**不是**"重启核心"——
    /// Local 走 `recover_quarantine()`、Service 走 `recover_core()`（R6/R22/DV-A）。
    pub async fn recover(&self) -> Result<BackendObservation, CoreBackendError>;

    /// **async**：Service 需要一次 IPC roundtrip（Local 读 `CoreManager::status()` 是同步的，
    /// 但签名统一为 async 才能装进同一个封闭 enum）。返回**忠实**观察，不是投影后的视图。
    pub async fn observe_status(&self) -> Result<BackendObservation, CoreBackendError>;

    /// D4=A：实现但**生产不接线**（B3 才改道）。因此它**不是** §2.1 的观察来源——
    /// 5a 里只有测试调用它，actor 不会用它的返回值提交状态。
    pub async fn apply(&self, request: &CoreRequest, expected: Option<RevisionIdInfo>)
        -> Result<CoreApplyData, CoreBackendError>;

    /// 优雅停核 + 归档 sink。调用后此值必须被立即 drop（D2 换槽协议）——5a 没有订阅任务需要 join。
    pub async fn shutdown(self) -> Result<(), CoreBackendError>;
}
```

**`run()` 的两侧实现（不对称，且必须如此）：**

| backend   | 实现                                                                                                                                                                       | 依据                                                                                           |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `Local`   | **一次** `CoreManager::switch(spec)` 调用即可 —— `switch_locked` 自己判 running：未跑则清理 stale epoch 后 `start_locked`；在跑则按能力做 graceful（零停机）或 hard switch | R5b（`switching.rs:52,58-103`）                                                                |
| `Service` | IPC **没有** `/core/switch`，只能组合：判定"是否需要先停" → 需要则 `stop_core()` → `start_core(CoreStartReq)`                                                              | R21；`api/core/apply.rs` 的文档说明 switch 语义只存在于 `/core/apply`，而 apply 要求核心已在跑 |

**Service 的"是否在跑"判定必须读 `detail`，不能读 `state`（事实 R5d）。** wire 上的 `CoreState` 是有损两值投影，把 `Starting` 和 `Restarting` 都压成 `Stopped(None)`——若按它判定，一个正在启动/重启中的核心会被误判为"已停"，于是跳过 `stop_core()` 直接 `start_core()`，而 daemon 侧仍会返回 already-running，`run()` 就失败了。正确判定：

```rust
/// 只有 `Stopped` 是终止态；其余五个 detail 变体都意味着"有东西在跑或正在动"，
/// 必须先 stop。`detail` 缺失（老 daemon 或字段未送达）时**保守当作在跑**——
/// 多发一次 stop 是幂等的，漏发会导致 start 失败。
fn service_needs_stop(infos: &CoreInfos) -> bool {
    match infos.detail.as_ref() {
        Some(CoreStateDetail::Stopped { .. }) => false,
        Some(_) => true,
        None => true,   // fail-safe
    }
}
```

**兜底：`stop_core()` 的 not-started 错误必须被吞掉。** 即便判定正确，daemon 侧仍可能在两次 RPC 之间自行停掉核心。因此 `stop_core()` 返回 typed `not_started`（`error_kind` 常量表，R16）时视为成功继续；**只抑制这一种 kind**，其余错误照常上报。

两侧 `run()` 成功后都必须**同步刷新一次状态**——`switch` 返回 `SwitchOutcome`、`start_core` 返回 `()`，都不带 revision。`run()` 因此**返回刷新后的 `CoreStatusView`**（提交规则见 RQ-02 的 post-run 段），而不是 `Result<()>`。

`LocalBackend`：

- 持 `Arc<nyanpasu_core_manager::CoreManager>`（事实 R4：非 Clone）+ `watch::Receiver<CoreStatus>`（R10）；
- 构造时 `ManagerOptions` **显式写出** `local_ipc_policy: LocalIpcPolicy::Disable`，并加注释说明"上游默认值已是 Disable（`spec.rs:114`），显式化是为了让这条安全门在 app 侧可见可审"（A1 卡要求，事实 R3）；
- `runtime_dir` 来自注入的 `RuntimePaths`，**不得**用 `dirs::*` 自行解析（roadmap §1.6 测试禁真实目录）；
- `apply` 把本地 `ApplyOutcome`（7 分支，R11）映射成 `CoreApplyData`，映射规则**照抄** `manager_bridge.rs:605-639`（R12），含 `DurabilityUncertain` 可嵌套两层、warning 用 `"; "` 拼接。

`ServiceBackend`：

- 持 `nyanpasu_ipc::client::Client`（实例，`Client::new(SERVICE_PLACEHOLDER)`，**禁止 `service_default()`**——A1 卡要求，事实 R18/R19）；
- 状态经 `observe_status()` 走 `/status`（R21）；`/ws/events` 订阅归 C1。

**5a 不起订阅任务（§2.1 范围裁定）。** 因此 backend 内**没有** `SubscriptionHandle`、没有可取消重连循环、没有需要 join 的后台任务。两个 backend 都只需提供 `status()`（按需查询）与各 mutation 的返回观察，并产出 §2.1 的 `BackendObservation`（含忠实 `lifecycle`）。runtime 侧已有的 `subscribe()` / `subscribe_logs()` 保持可达但**不消费**——C1 接线时无需改 backend 形状。

- Local：`CoreManager::status()` 按需查询（`subscribe()` 保持可达但 5a 不消费）；
- Service：`Client::status()` 按需查询（`events()` 的订阅/重连整体移出 5a，归 C1）。

`TestBackend`（`#[cfg(test)]`）：脚本化返回值 + 调用计数 + oneshot barrier 钩子，供 A2/A3 的**并发与 gate 语义**测试使用。**注意范围：`TestBackend` 不得用于 A-Exit 要求的 Local/Service parity**（见 S12 的说明）。

**验证：** `cargo check`；`rg 'service_default' backend/tauri/src` 只命中 legacy `core/clash/core.rs`（5a 不动那 5 处，见 §0）。

### S4 — error kind 分类（R0 条件化，隔离在一个模块）

新文件 `APP/core/actor/error_kind.rs`，**整个文件就是 R0 的适配层**。

```rust
//! 机器可读的核心错误分类。
//!
//! 当前 submodule pin（v2.0.0-rc.1）**没有** `Error::kind()`（见计划 §1.1 R16）；
//! 上游的映射函数 `map_error_kind` 是 `nyanpasu-service-runtime` 的私有函数，
//! 本 app 不依赖该 crate。因此这里维护一份等价映射作为过渡。
//!
//! TODO(actor-migration): 过渡实现，等 R0 合并并 bump submodule。
//! Reason: `nyanpasu_core_manager::Error::kind()` 在 v2.0.0-rc.1 不存在。
//! Remove when: submodule 指向含 R0 的 tag —— 届时本文件退化为
//! `err.kind()` 的一次调用，Service 侧的字符串解析也改成解析 typed kind。
```

- Local：`fn local_error_kind(err: &nyanpasu_core_manager::Error) -> Option<&'static str>`，逐条对齐 `nyanpasu_ipc::api::error_kind` 的 12 个常量（R16），并像上游一样递归进 `DurabilityUncertain`；
- Service：直接读 `ClientError` 的 `error_kind: Option<String>`（R26）。

**R0 落地后的替换步骤（一步，独立提交）：** 删除 `local_error_kind` 的 match 体，改为 `err.kind()`；Service 侧把字符串解析成同一个 enum。**本计划不执行该步**（见 §0 与 out-of-scope）。

**验证：** 单测断言 12 个常量的映射与 `nyanpasu_ipc::api::error_kind` 一一对应（用常量本身而非字面量，防止上游改字符串时静默漂移）。

### S5 — `OperationId` / `OperationGate` / actor 骨架

新文件 `APP/core/actor/mod.rs`（`pub(crate) mod backend; pub(crate) mod types; mod error_kind;`——**不含 `client`**：唯一的 typed client 在 `APP/client/core.rs`，与其它四个 client 同层，见 §2.1.7）与 `APP/core/actor/gate.rs`。

按 design §3.1–§3.3 逐字实现：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(NonZeroU64);   // 0 保留为无效值

struct OperationGate { active: Option<ActiveOperation>, waiters: VecDeque<OperationWaiter> }
struct ActiveOperation { id: OperationId, acquired_at: Instant }
struct OperationWaiter { id: OperationId, reply: RpcReplyPort<Result<(), OperationError>> }
```

消息枚举（**5a 只放本阶段用得到的**，design §3.2 里属于 B1 的 `CheckAndPromote` / `ApplyPromoted` / `StartPromoted` **不在 5a**）：

```rust
pub enum CoreActorMessage {
    AcquireOperation { id: OperationId, reply: RpcReplyPort<Result<(), OperationError>> },
    ReleaseOperation { id: OperationId },

    Check   { operation: OperationId, request: CoreRequest, reply: ... },
    /// 唯一的运行迁移消息（DV-F）。**没有** `Start` / `Restart` 两条平级消息：
    /// 它们各自只在半边状态空间有效，拆开就是把状态判断推给调用方。
    Run     { operation: OperationId, request: CoreRequest, reply: ... },
    Stop    { operation: OperationId, reply: ... },
    Recover { operation: OperationId, reply: ... },
    /// 换槽（D2 协议）。**成功后不自动启动核心**——调用方需要核心跑起来时
    /// 必须在**同一个** guard 下紧接着发 `Run { request }`，不能发 `Restart`
    /// （新 backend 没有 `last_spec`，见 R5c）。
    SetBackend { operation: OperationId, mode: RunType, reply: ... },

    Shutdown(RpcReplyPort<()>),

    /// 守卫刷新（§2.1.3）。**必填** `operation`——调用方必然持 guard。
    RefreshStatus {
        operation: OperationId,
        reply: RpcReplyPort<Result<BackendObservation, CoreActorError>>,
    },
    /// UI 提示，`cast` 而非 `call`。出队时 gate 空闲才刷新，否则丢弃（§2.1.3）。
    RefreshHint,

    // 注意：**没有** `Status` 消息——读走 watch 克隆，完全不碰 mailbox（§2.1.1）。
}
```

actor state（对照 DV-E：**不含** runtime lifecycle 与 log ring）：

```rust
struct CoreActorState {
    /// 显式建模，让 `NoBackend { last_error }` 可从状态复现（round-6 #11）。
    /// `None` = 换槽瞬态；`Failed` = 构造失败并保留诊断（leader 裁定 B）。
    backend: Option<BackendSlot>,
    mode: RunType,
    operation: OperationGate,
    /// 缓存**完整**观察（保留忠实 lifecycle，D5 的 latch 需要它）。
    observed: BackendObservation,
    /// 每次提交都向它发布投影；`CoreClient` 持 receiver（§2.1.1）。
    status_tx: watch::Sender<CoreStatusView>,
    /// `core_recovery_exhausted` 的 per-episode 发布闩锁（D5）。
    recovery_exhausted_published: bool,
    /// 注入的降级发布端（D5）。
    degradation: Arc<dyn CoreDegradationSink>,
}

/// backend 槽位（leader 裁定 B）。外层 `Option` 表示**换槽瞬态空位**——
/// 把 `Ready(CoreBackend)` move 出来关停时，状态里必然有一段没有 backend 的区间，
/// 用 `None` 如实表达，而不是伪造一个 `Failed`。
/// `Failed` 里用 `Arc<CoreBackendError>`：上游 `nyanpasu_core_manager::Error` 与
/// `nyanpasu_ipc::ClientError` 都**不是 `Clone`**（含 `io::Error` / `reqwest::Error` 等），
/// 用 `Arc` 才能在每次 `NoBackend` 回包时廉价复制诊断。
enum BackendSlot {
    Ready(CoreBackend),
    Failed { error: Arc<CoreBackendError> },
}
```

gate 规则（design §3.3，逐条）：无 active → 立即登记并回复成功；有 active → 入 FIFO，**handler 立即返回，绝不等待**；`ReleaseOperation(active_id)` → 清 active 并放行下一个仍有接收方的 waiter；`ReleaseOperation(waiting_id)` → 从 waiters 删除；未知 ID → 幂等 no-op + debug 日志；mutation 的 ID 与 active 不符 → `StaleOperation`；shutdown → 拒绝全部 waiters、清 active、关 backend。

**验证：** S12 的 T-OP-01…07。

### S6 — `CoreClient` + `CoreOperationGuard`

新文件 `APP/client/core.rs`（与 `application.rs` / `profiles.rs` 同级，遵循 A7f 房规）。

```rust
#[derive(Clone)]
pub struct CoreClient { inner: Arc<CoreClientInner> }

struct CoreClientInner {
    actor_ref: ActorRef<CoreActorMessage>,
    next_operation: AtomicU64,   // clones 共享（design §3.1）
}

pub struct CoreOperationGuard { id: OperationId, client: CoreClient, acquired: bool }
```

关键实现点：

- `begin_operation()` **先分配 ID、先构造 pending guard、再发 `AcquireOperation`**（design §3.1 的取消窗口论证）：
  ```rust
  async fn begin_operation(&self) -> Result<CoreOperationGuard> {
      let mut operation = CoreOperationGuard::pending(self.clone(), self.allocate_operation_id()?);
      operation.acquire().await?;   // 内部用 Some(CORE_ACQUIRE_TIMEOUT)
      Ok(operation)
  }
  ```
- `Drop for CoreOperationGuard` 用 **`cast`**（fire-and-forget）发 `ReleaseOperation { id }`——`Drop` 是同步的，不能 await；发送失败只意味着 actor 已终止，此时不存在需要让位的 active operation（design §3.3 原文）；
- 正常路径允许显式 `release().await`，`Drop` 只作兜底；
- `allocate_operation_id` 用 `fetch_add`，`0` 视为无效；溢出 → 进程生命周期内不可恢复的内部错误（design §3.1），**不持久化、不跨进程**；
- `status()` 是**同步 watch 克隆**（`watch::Receiver::borrow().clone()`），不发消息、无超时——读的活性因此是结构性的（§2.1.1）；原计划的 `CORE_READ_TIMEOUT` 常量**删除**；
- `core_status()`（facade）= watch 读 + `cast RefreshHint`，两步都不等待；
- `Drop for CoreClientInner` 调 `actor_ref.stop(None)`（房规）。

### S7 — A3 兼容 seam 适配

在 `APP/client/core.rs` 里定义 **`CoreLifecycleAdapter`** 并由它实现 `CoreLifecyclePort`（**不是**直接给 `CoreClient` 实现——适配器还需要 `ApplicationClient` 与 `RuntimePaths` 两个依赖，见下），再为一个包住 `CoreOperationGuard` 的 lease 适配器实现 `CoreLifecycleLease`。

约束（来自 A1f/A4f，**编译期硬约束，不可妥协**）：

- `CoreLifecycleLease: Send`（不要求 Sync）→ `CoreOperationGuard` 必须 `Send`；
- lease 以 `&mut dyn` 跨 4 处函数签名传递，并被 **move 进 `async move` 闭包**（`patch_running_config`）→ 适配器必须是 `Box<dyn CoreLifecycleLease>` 且可移动进 boxed future；
- `begin()` 返回的 `Box<dyn CoreLifecycleLease>` 拥有 guard，作用域结束即 drop → 自动 `ReleaseOperation`。这正好把"借用式 lease"与"RAII guard"对上，**不需要改任何调用点**。

**适配器的依赖不止 `CoreClient`（round-3 #3b，leader 裁定）。** `CoreLifecycleLease::restart(&mut self)` **不带任何参数**（`core_bridge.rs:71`），而 `Run` 需要一个完整 `CoreRequest`——其中的 core type 在 legacy 实现里是从 `Config::verge().latest().clash_core` 取的（`core/clash/core.rs:97-105`）。照抄那段取值会给新代码引入一个隐藏全局依赖，直接违反本计划自己的"新代码禁止 `Config::*()`"闸门。

**裁定：适配器显式持有 `ApplicationClient` + `RuntimePaths`，每次调用现读 typed 快照。**

```rust
/// `CoreLifecyclePort` 的内部适配器。三个依赖都是显式注入的：
/// - `core`：发 actor 消息；
/// - `application`：**每次**调用时现读 typed 快照拿当前 core type（不缓存——
///   两次 restart 之间用户可能改过核；也不读 `Config::verge()`）；
/// - `runtime_paths`：product 路径，注入而非 `dirs::*` 解析。
struct CoreLifecycleAdapter {
    core: CoreClient,
    application: ApplicationClient,
    requests: CoreRequestFactory,   // 取代裸 RuntimePaths（round-6 #7）
}

impl CoreLifecycleAdapter {
    /// seam 的无参方法靠它把"当前意图"物化成一个 `CoreRequest`。
    async fn current_request(&self) -> anyhow::Result<CoreRequest> {
        let snapshot = self.application.get().await?.state;   // typed，非 legacy
        self.requests.for_product(snapshot.core)              // 字段名是 `core`
    }
}
```

`CoreRequestFactory::for_product` 的取值全部来自注入依赖（data dir / pid path 固化自 `PathResolver`，二进制经 `CoreBinaryResolver`），**不调用 `find_binary_path` 也不调 `dirs::*()`**；core type 来自 typed 快照的 `core` 字段。

> 备选（**不推荐**）：保留一个 legacy bridge 专门供 seam 取 core type 并显式 allowlist。它会让 `config_calls` 指标不降反留，且把一个本可立即消除的全局依赖延寿到 B2/B3，收益为负。

五个 lease 方法在 5a 的路由（按 D3=A）：

| 方法                | 5a 路由                                                                                                                                                                                                                                                                | 去向 |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---- |
| `check_and_promote` | 维持现有实现（候选校验 + 原子 promote），核心检查改调 `CoreBackend::check`                                                                                                                                                                                             | B1   |
| `apply_candidate`   | 维持现有实现                                                                                                                                                                                                                                                           | B3   |
| `apply_promoted`    | 维持现有实现（`api::put_configs`）                                                                                                                                                                                                                                     | B3   |
| `restart`           | **改走 `Run { request }`**（**不是** `CoreBackend::start`）。legacy `run_core_from(product)` 的语义就是"在跑先停、再按 product 启动"，恰好等于 `run()`（DV-F / R5b）。`request` 由 lease 适配器按 A13f 的既有取值方式就地构造，`config_path = runtime_paths.product()` | —    |
| `stop`              | **改走** `CoreBackend::stop`                                                                                                                                                                                                                                           | —    |

> **注意**：`CoreLifecycleLease::restart` 这个**名字**保持不变（A3 要求兼容 seam 不动），但它背后是 `Run` 而非任何 "restart" 原语。适配器上必须写明这一点，否则后续读者会误以为可以直接映射到 `CoreManager::restart()`。

`CoreLifecyclePort::status()` 在 5a **第一次有了生产实现**（A5f 此前无生产调用者），返回值由 `CoreStatusView` 投影成既有 `CoreStatusSnapshot`，**wire 与结构都不变**。

`on_profile_change()` 维持现有实现（连接中断服务，PR-6 范围）。

**顺序不变式（必须在代码注释里写明）：** A3f 已确认 `rebuild_gate` 在全部 10 处都先于 `core.begin()`，`patch_running_config` 是 `clash_patch_gate → rebuild_gate → begin()`。5a 引入 `OperationGate` 后变成**三层嵌套且全局顺序一致**，因此不产生死锁。B2 会把前两层吸收掉。

### S8 — 组合根接线

**不能**沿用"在 `setup.rs` 里构造好再从 `ClientSetupArgs.core` 传进去"的写法：事实 A17f——`try_new_with_args` 在进入 `block_on` **之前**就解构了 `core`（`client/mod.rs:255-264`），而 `CoreClient` 需要的 `enable_service_mode` 来自 typed `application` client，后者到 block 内第 279 行才存在。

**正确的所有权图（三步）：**

**（1）`CoreClient` 在 block 内、typed 快照之后构造。** 位置紧跟 `new_typed_config_clients(...)`（`client/mod.rs:279`）之后：

```rust
let application_snapshot = application.get().await?.state;
let mode = crate::core::RunType::classify(
    application_snapshot.enable_service_mode,
    crate::core::service::ipc::get_ipc_state(),
);
let core_client = CoreClient::new(CoreActorArgs {
    mode,
    runtime_paths: runtime_paths_for_setup.clone(),
    degradation: degradation_sink.clone(),   // D5，由 ClientSetupArgs 注入
}).await?;
```

按 D2 的顺序事实此刻必然是 `Normal`（`init_service()` 尚未跑），先建 Local backend；健康检查随后再发 `SetBackend(Service)`。

**（2）`CoreClient` 成为 `NyanpasuClientInner` 的 typed 字段；legacy trait 适配器内部产出。**

```rust
struct NyanpasuClientInner {
    // ...
    core_client: CoreClient,              // 新增 typed 依赖
    core: Arc<dyn CoreLifecyclePort>,     // 保留，但生产路径由 core_client 内部产出
}
```

`ClientSetupArgs.core` 由 `Arc<dyn CoreLifecyclePort>` 改为 **`Option<Arc<dyn CoreLifecyclePort>>`**：

- 生产（`setup.rs`）传 `None` → block 内构造完整适配器并作为生产 port：
  ```rust
  // 工厂**只构造一次**（round-8 #3），存进 `Inner`，再 clone 给 adapter / reconciler / facade。
  let requests = CoreRequestFactory::new(
      &paths,                                   // data dir / pid path 在此固化
      runtime_paths_for_setup.clone(),
      args_binary_resolver,                     // Arc<dyn CoreBinaryResolver>，测试可换
  )?;
  let core = args_core.unwrap_or_else(|| Arc::new(CoreLifecycleAdapter::new(
      core_client.clone(),
      application.clone(),                 // typed 快照来源（S7）
      requests.clone(),                    // 见下：工厂在 block 内**只构造一次**
  )) as Arc<dyn CoreLifecyclePort>);
  ```
  三个参数**缺一不可**：少 `application` 就回到"无参 `restart()` 拿不到 core type"的死结；少 `CoreRequestFactory` 就凑不齐 `binary_path` / `working_dir` / `pid_path`（`RuntimePaths` 只有 product 与 candidate_dir），只能退回会调 `dirs::*()` 的 `find_binary_path`——那正是被禁止的；
- 测试传 `Some(mock)` → 沿用既有 `test_client_args_with_lifecycle`（A8f），**测试图零改动**。

这**完全复刻**同文件既有的 `clash_patch: Option<...>` 分阶段注入模式（`client/mod.rs:265-268`），是本仓已确立的房规而非新发明。`setup.rs` 相应删掉 `core: Arc::new(LegacyCoreBridge::new(runtime_paths))` 一行，并新增 degradation sink 的注入。

**（3）无注入点的消费者——显式 clone 穿线。** 这些 `pub fn` 既没有参数也拿不到 Tauri state。**共 6 个入口**（v2/v3 只列了 2 个，遗漏了 `spawn_health_check` 的三个直接调用者，事实 A21f）：

| #   | 消费者                                                                 | 现状                                                                                  | 5a 穿线方式                                                                                                                                    |
| --- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `APP/feat.rs:56 restart_clash_core()`                                  | 无参 `pub fn`，`spawn` 后调 `CoreManager::global().run_core()`；调用方是托盘/菜单动作 | 改签名为 `restart_clash_core(client: NyanpasuClient)`，由托盘调用点传入已 `manage` 的 clone（`NyanpasuClient` 是 `Arc` newtype，clone 零开销） |
| 2   | `APP/core/service/ipc.rs:74 on_ipc_state_changed(state)`               | 健康检查线程内调用                                                                    | 由 `spawn_health_check` 往下传（见 3–6）                                                                                                       |
| 3   | `APP/core/service/mod.rs:32` `init_service()` → `spawn_health_check()` | 启动路径                                                                              | `init_service(reconciler: CoreModeReconciler)`；上游 `APP/utils/init/mod.rs::init_service` 同步加参数                                          |
| 4   | `APP/core/service/control.rs:97`（`install_service()` 内）             | 安装成功后拉起健康检查                                                                | `OsServiceControlOps::install(reconciler)` 内部持有并下传；IPC 入口 `APP/ipc.rs:937` 改调 facade                                               |
| 5   | `APP/core/service/control.rs:225`（`start_service()` 内）              | 同上                                                                                  | `OsServiceControlOps::start(reconciler)`；IPC 入口 `APP/ipc.rs:951` 改调 facade                                                                |
| 6   | `APP/core/service/control.rs:320`（`restart_service()` 内）            | 同上                                                                                  | `OsServiceControlOps::restart(reconciler)`；IPC 入口 `APP/ipc.rs:985` 改调 facade                                                              |

**穿的不是裸 `CoreClient`，而是 `CoreModeReconciler`（round-6 #8）。** `on_ipc_state_changed` 要做的是完整事务——读 typed 快照、构造 `CoreRequest`、在 guard 下 `SetBackend` + `Run`——只给 `CoreClient` 三样依赖缺两样。因此定义：

```rust
/// IpcState 翻转后的运行模式对账。自带全部依赖，**不查全局**。
#[derive(Clone)]
pub(crate) struct CoreModeReconciler {
    core: CoreClient,
    application: ApplicationClient,
    requests: CoreRequestFactory,
}

impl CoreModeReconciler {
    /// 读 typed 快照 → 取 guard → SetBackend → Run。取代 `on_ipc_state_changed`
    /// 里原来的 `Config::verge()` + `CoreManager::global()`。
    pub(crate) async fn reconcile(&self, ipc_state: IpcState) -> anyhow::Result<()>;
}
```

`spawn_health_check(reconciler: CoreModeReconciler)`，四个调用者各自把手里的 reconciler 传进去。

**健康检查的注入根在 `resolve.rs:152`（round-8 #3）。** 现状是 `resolve.rs:152` 调 `init::init_service()`，后者（`APP/utils/init/mod.rs:231-269`）与 `APP/core/service/mod.rs:17-36` **各自**读一次 `Config::verge().enable_service_mode`，再决定是否 `spawn_health_check()`。5a 改为：

1. facade 提供入口 `NyanpasuClient::init_service_health()`——它**自己**从 `ApplicationClient` 读一次 typed 快照， 并构造 `CoreModeReconciler`；
2. `resolve.rs:152` 改调该 facade 方法（此处 `NyanpasuClient` 已 `manage`，拿得到）；
3. 下游两处 `Config::verge()` 读取**改为接收传入的值**——`init::init_service(enable_service: bool, reconciler: CoreModeReconciler)` 与 `service::init_service(enable_service: bool, reconciler: CoreModeReconciler)`，它们不再自己查配置；
4. 初次 `spawn_health_check(reconciler)` 由第 3 步传下去的 reconciler 供给。

这样**启动路径上的两处 `Config::verge()` 一并消除**（ledger `config_calls` 应下降 2），且健康检查线程从诞生起就持有完整依赖。

**四个** service IPC 命令（install / start / stop / restart）改为转调 facade 领域方法（见下 (4)），另有 `get_core_status` / `restart_sidecar` / `update_core` 三个也改走 facade（S9 表）。新增的 `tauri::State` 参数**不进 TS 签名**（managed state 在 specta 导出时被跳过）；验证判据是 **bindings 除已声明的 `core_lifecycle` 联合成员外无其它 diff**（S13）。

**（4）所有需要 `CoreClient` 的 Tauri 命令都必须走 facade 领域方法（round-4 #4 / round-5 #3、#4）。** 它们拿到的是 `State<NyanpasuClient>`，而 `NyanpasuClient.inner` 是**私有**字段（`client/mod.rs:93-96`）——命令层既够不到 `CoreClient`，计划又（正确地）禁止加 `core_client()` 访问器。按 `update_core` 的先例（S9.1），facade 补齐**六个**领域方法：

| facade 方法         | 承接的命令                     | 内容                                       |
| ------------------- | ------------------------------ | ------------------------------------------ |
| `install_service()` | `ipc.rs:937`                   | 控制操作 + 健康检查穿线                    |
| `start_service()`   | `ipc.rs:951`                   | 完整 typed 事务（见下）                    |
| `stop_service()`    | `ipc.rs:968`                   | 完整 typed 事务（**v5 遗漏了这一个**）     |
| `restart_service()` | `ipc.rs:985`                   | 完整 typed 事务                            |
| `core_status()`     | `ipc.rs:399` `get_core_status` | 读缓存快照（§2.1 R5），投影成既有元组 wire |
| `restart_core()`    | `ipc.rs:502` `restart_sidecar` | guard + `Run { request }`                  |

**三个 service 方法的完整 typed 事务**（不是 `control::*` 直通）。**关键是错误次序（round-6 #9）**：整个对账块必须作为**一个被捕获的 `Result`** 执行，块内**任何**失败（快照读取、取 guard、`SetBackend`、构造请求、`Run`）都只记录日志，**绝不 `?`**——否则会用对账错误顶替掉原本要返回的控制结果，把"控制成功但对账失败"变成 `Err`，或把"控制失败"的原因替换掉。

```rust
impl NyanpasuClient {
    /// `start_service` / `stop_service` / `restart_service` 同形，只有 `control::*` 那一步不同。
    pub async fn start_service(&self) -> Result<()> {
        // 1) 控制操作。结果**存起来不 `?`**。
        let control = self.inner.service_control.start(self.core_mode_reconciler()).await;

        // 2) 整个对账块捕获成一个 Result —— 内部一律不 `?` 逃逸出去。
        let reconcile: anyhow::Result<()> = async {
            let app = self.inner.application.get().await?.state;
            if app.enable_service_mode {
                self.core_mode_reconciler()
                    .reconcile(crate::core::service::ipc::get_ipc_state())
                    .await?;
            }
            Ok(())
        }
        .await;
        if let Err(e) = reconcile {
            log::error!(target: "app", "{e}");   // 与既有实现一致：只记录
        }

        // 3) 控制操作的错误在**最后**才抛
        Ok(control?)
    }
}
```

> **保留的既有语义（round-5 #3，leader 裁定：不改行为）。** 现有三个命令都是 `let res = control::X().await;` … 中间无条件尝试 `run_core()` … 最后才 `Ok(res?)`（`ipc.rs:951-997`）。也就是说**控制操作失败时仍然会尝试拉起核心**。这不是笔误，而是 **fail-open-to-Local**：控制操作失败 → daemon 没起来 → 健康检查不会把 `IpcState` 置为 `Connected` → `RunType::classify` 给出 `Normal` → 于是拉起的是**本地核心**。用户至少有一个能用的核心，与 PR-5-pre 兼容门"失败一律退回 Local"的哲学一致。
>
> **因此 `control?` 必须留在最后**，对账块的任何错误都只记录不上抛。实施时**不得**"顺手改成先判 `control?` 再对账"——那会静默改变行为：控制失败时用户从"有本地核心"变成"什么都没有"。T-FA-01…05 逐点钉住。

**可注入的控制 seam `ServiceControlOps`（round-6 #9 / round-7 #5）。** 不能靠真的执行 OS service 命令来制造控制失败，因此把四个控制函数抽成窄 trait 并**接入对象图**：

```rust
/// service 控制操作的可测边界。生产实现就是现有的 runas / sudo 调用。
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ServiceControlOps: Send + Sync + 'static {
    async fn install(&self, reconciler: CoreModeReconciler) -> anyhow::Result<()>;
    async fn start(&self, reconciler: CoreModeReconciler) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    async fn restart(&self, reconciler: CoreModeReconciler) -> anyhow::Result<()>;
}
```

注入链（**四处都要改**）：

1. `ClientSetupArgs` 增加 `service_control: Arc<dyn ServiceControlOps>`（**5 个字面构造点全部要动**，A22f）；
2. `NyanpasuClientInner` 增加同名字段；
3. `setup.rs` 构造生产适配器 `Arc::new(OsServiceControlOps)`——它内部就是现有的 `control::{install,start,stop,restart}_service`；
4. facade 方法**调注入的 ops**（`self.inner.service_control.start(...)`），**不再**直接 `control::start_service(...)`。

这才是 T-FA-01/05 能注入失败的前提——测试传 `MockServiceControlOps`，不碰任何 OS 命令。

> **这是 5a 唯一新增的 port。** design §9 说的"不引入完整 `ServiceControlPort`"针对的是把 service 管理**迁进 CoreActor**；这里只给既有函数加一层可测边界，所有权仍在 `core::service::control`，不迁移。

> `uninstall_service`（`ipc.rs:944`）**不加** facade 方法——它只置 `KILL_FLAG`，不需要 `CoreClient`。

> `uninstall_service`（`ipc.rs:944`）**不需要**改——它只置 `KILL_FLAG`，不调 `spawn_health_check`。
> 这两处都**不是**"再加一个全局"——是把已有的实例显式传下去。若某个调用点确实拿不到 client（例如极早期启动路径），**停下来上报**，不要退回全局查找。

**（4）`LegacyCoreBridge` 删除**（`core_bridge.rs:107-153`，含其 `CoreManager::global()` 两处与那条 TODO 标记）。

**（5）IpcState 翻转的处理**（`core/service/ipc.rs:83-91`）改为在**同一个** `CoreOperationGuard` 下顺序执行 `SetBackend(mode)` 然后 `Run { request }`——**不是** `Restart`（新 backend 没有 `last_spec`，R5c）。

### S9 — 迁移直接调用点

按 A3 卡"start/stop/restart/status 改走 actor"逐点替换。**命令层一律只调 facade 领域方法（round-6 #10）**——
`ipc.rs` / `feat.rs` 拿不到私有的 `NyanpasuClientInner`，T-FA-04 也断言 `ipc.rs` 里不得出现 `CoreClient` 类型名。

| 调用点                                         | 现状                             | 5a 改为（**全部经 facade**）                                                                     |
| ---------------------------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------ |
| `APP/ipc.rs:399` `get_core_status`             | `CoreManager::global().status()` | `client.core_status()` —— facade 内部 `CoreClient::status()` 后投影成既有元组 wire，**形状不变** |
| `APP/ipc.rs:502` `restart_sidecar`             | `run_core()`                     | `client.restart_core()` —— facade 内部 guard + `Run`                                             |
| `APP/ipc.rs:937/951/968/985` 四个 service 命令 | `control::*` + `run_core()`      | `client.{install,start,stop,restart}_service()`（S8(4)）                                         |
| `APP/ipc.rs:639` `update_core`                 | `UpdaterManager::global()`       | `client.update_core(core_type)`（S9.1）                                                          |
| `APP/feat.rs:56` `restart_clash_core()`        | `run_core()`                     | 改签名收 `NyanpasuClient`，调 `client.restart_core()`                                            |
| `APP/feat.rs:292,385`                          | `status()`                       | `client.core_status()`                                                                           |
| `APP/core/service/ipc.rs:83,88`                | `status()` + `run_core()`        | `CoreModeReconciler::reconcile()`（S8(3)）                                                       |
| `APP/utils/help.rs:268`                        | `stop_core()`                    | `client.shutdown()` 已覆盖（S11），**删除该行**                                                  |
| `APP/utils/resolve.rs:288`                     | `stop_core()`                    | **删除该行停核**（S11：`resolve_reset` 只留 `reset_sysproxy()`）                                 |
| `APP/feat.rs:392` macOS DNS                    | `change_default_network_dns`     | **不动，但必须 allowlist**（S9.2）                                                               |
| `APP/core/updater/instance.rs:201,205,216,279` | `begin_lifecycle` + lease 方法   | **必须迁移**（S9.1）                                                                             |

> facade 领域方法总表见 S8(4)：`core_status` / `restart_core` / `install_service` / `start_service` / `stop_service` / `restart_service` / `update_core`，共 **7 个**。

#### S9.1 — Updater 必须在 5a 迁移（原计划此处有错）

事实 A16f：`replace_core()` 依赖的是 **S10 要删掉的那一整套 API**——`CoreManager::global().begin_lifecycle()`、`lifecycle.stop_core()`、`lifecycle.run_core_from(product)`。原计划"Updater 不动 + S10 删 lease"两条**互相矛盾，会直接编译失败**。而且即便设法保住编译，legacy 单例也停不掉 CoreActor 拥有的新 manager 实例——那是**两个不同的进程所有者**，会出现"更新器以为停了核、实际没停"的静默损坏。

**Leader 裁定：在 5a 用显式注入的 `CoreClient` + operation guard 改造 `replace_core`，不加 `attach_core_port` 全局桥，UpdaterActor 的完整迁移仍归 PR-6d。**

**注入必须穿透真实的四段构造链**（事实 A16g；v2 计划把类型名写成了不存在的 `UpdaterInstance`，且只说"构造点会接受它"，不够可执行）：

```text
ipc::update_core(core_type)                                   ← Tauri 命令，注入起点
  → NyanpasuClient::update_core(core_type)                    ← facade 领域方法
    → UpdaterManager::update_core(&core_type, core, provider) ← 两个参数，**都不存入 manager**
      → UpdaterBuilder::set_core(core).set_request_provider(provider).build()
        → Updater { core, requests: Arc<dyn CoreRequestProvider>, .. }
```

**逐点签名（四处，全部要改）：**

```rust
// 1) APP/ipc.rs:639 — 薄适配器，只转调 facade 的领域方法
#[tauri::command]
#[specta::specta]
pub async fn update_core(
    client: tauri::State<'_, crate::client::NyanpasuClient>,
    core_type: nyanpasu::ClashCore,
) -> Result<usize> {
    Ok(client.update_core(core_type).await?)
}

// 1b) APP/client/mod.rs — facade 上的**领域方法**（leader 裁定，round-3 #10）。
//     不提供 `core_client()` 这类内部 client 访问器：那会把 typed actor 边界泄给命令层，
//     即使不暴露裸 `ActorRef` 也违反 CLAUDE.md §7 的 facade 约束。
//     依赖穿线全部发生在**这一个方法体内**，命令层看不到 CoreClient。
impl NyanpasuClient {
    pub async fn update_core(&self, core_type: ClashCore) -> Result<usize> {
        let core = self.inner.core_client.clone();          // 私有字段，不外泄
        // facade 传的是 **provider**，不是预构造的 request（round-9 #2）。
        let provider = self.core_request_provider();   // 内部持 ApplicationClient + CoreRequestFactory
        Ok(crate::core::updater::UpdaterManager::global()
            .write().await
            .update_core(&core_type, core, provider).await?)
    }
}

// 2) APP/core/updater/mod.rs:222
pub async fn update_core(&mut self, core_type: &ClashCore, core: CoreClient, provider: Arc<dyn CoreRequestProvider>) -> Result<usize>

// 3) APP/core/updater/instance.rs — builder 新增字段与 setter
pub(super) struct UpdaterBuilder {
    client: Option<reqwest::Client>,
    core_type: Option<ClashCore>,
    mirror: Option<String>,
    artifact: Option<String>,
    tag: Option<CoreTypeMeta>,
    core: Option<CoreClient>,        // 新增
    requests: Option<Arc<dyn CoreRequestProvider>>,   // round-9 #2：provider 而非预构造 request
}
impl UpdaterBuilder {
    pub fn set_core(mut self, core: CoreClient) -> Self { self.core = Some(core); self }
    pub fn set_request_provider(mut self, p: Arc<dyn CoreRequestProvider>) -> Self { self.requests = Some(p); self }
    // build() 里 `core` 缺失时按既有风格 bail
}

// 4) APP/core/updater/instance.rs:31 — Updater 持有它
pub(super) struct Updater {
    // ...既有字段不动...
    core: CoreClient,
    /// 在**替换时刻**解析请求（round-9 #2）——不是 `update_core()` 调用时刻。
    requests: Arc<dyn CoreRequestProvider>,
}

async fn replace_core(&self) -> anyhow::Result<()> {
    self.dispatch_state(UpdaterState::Replacing);
    // 整个 stop → swap binary → run 事务持有同一个 guard，
    // 与 rebuild/change-core 互斥（这正是 legacy begin_lifecycle 原本提供的保证）。
    let operation = self.core.begin_operation().await?;

    // **在 guard 内解析当前意图**（round-9 #2）——下载可能已经跑了几分钟，
    // 期间用户可能换核，因此绝不能用 `update_core()` 调用时刻的快照。
    let request = self.requests.current().await?;
    let replacing_running_core = request.core_type == (&self.core_type).into();

    if replacing_running_core {
        tracing::debug!("stopping core to replace");
        self.core.stop(&operation).await?;             // 取代 lifecycle.stop_core()
    }

    /* 现有的下载 / 校验 / 复制二进制逻辑逐字不动 */

    if replacing_running_core {
        self.dispatch_state(UpdaterState::Restarting);
        self.core.run(&operation, &request).await?;    // 取代 lifecycle.run_core_from()
    }
    Ok(())
}
```

**注入原则（三条，逐条约束）：**

1. **`CoreClient` 只随调用传递，绝不存进 `UpdaterManager`。** 把它挂到全局 manager 上等同于新建一个全局服务定位器（CLAUDE.md §7 禁止），也正是 design §9 拒绝的 `attach_core_port` 半迁移桥。它只从 `update_core` 的参数流向 builder 再流向 `Updater` 实例。
2. **领域 facade 方法，不是内部 client 访问器**（leader 裁定，round-3 #10）：`NyanpasuClient` 新增 `pub async fn update_core(&self, core_type) -> Result<usize>`，**不**新增 `core_client()` 访问器。`CoreClient` 的 clone 只在该方法体内发生，命令层完全看不到它——既不暴露裸 `ActorRef`，也不暴露 typed client（CLAUDE.md §7）。
3. `UpdaterManager::global()` 本身仍是 PR-6d 的 residual——**本阶段只把 core 依赖显式化，不动 updater 自身的全局性**。

原来那条 `TODO(actor-migration): temporary bridge to the legacy global core manager`（instance.rs:202-204）**删除**。

> **`update_core` 的 Tauri 命令签名变化不应影响 TS**：新增的 `client: tauri::State<'_, NyanpasuClient>` 参数**不进入** TS 签名（Tauri 的 managed state 参数在 specta 导出时被跳过，与既有 `patch_verge_config` 等命令同理）。因此 `updateCore` 的 TS 形状不变——但**实施时必须核对**：bindings 的 diff **除已声明的 `core_lifecycle` 联合成员外应为空**（S13），不要假设。

#### S9.2 — macOS DNS residual allowlist（leader 裁定）

`feat.rs:392` 的 `CoreManager::global().change_default_network_dns(...)` 是**第二个** core residual（原计划 §5 的"仅剩 Updater residual"断言因此是错的）。**Leader 裁定：不在 5a 迁移，allowlist 到 PR-5c / C3**，补标记：

```rust
// TODO(actor-migration): macOS DNS 仍走 legacy CoreManager。
// Reason: DNS 归位（MacosDnsGuard，与 start/stop 保序）是 PR-5c / C3 的范围；
//         在 5a 迁移它会把平台副作用编排提前带进本阶段。
// Remove when: PR-5c 把 MacosDnsGuard 移入 CoreActor。
```

于是 5a 结束后 `CoreManager::global()` 的**生产代码**残留恰好 **1 处**（`feat.rs:392`，且带 `#[cfg(target_os = "macos")]`），另有 1 处出现在 `process_core_bridge.rs:4` 的**文档注释**里（内容是"禁止调用"的告诫，不是调用）。S13 的 grep 判据按此写。

### S10 — 删除 legacy 生命周期

- 删除 `CoreManager::lifecycle_lock` 字段、`begin_lifecycle()`、`CoreLifecycleLease<'a>`（legacy 的那个）以及所有 `*_with_lease` 变体（`APP/core/clash/core.rs:386-422,428,444-449,486-651`）；
- **删除两条裸线程递归恢复路径**（design §5 禁止的第二层恢复，缺一不可）：
  1. `core.rs:577-582`——`recover_core()` 失败后 `sleep(5s)` + `std::thread::spawn` + 自调用；
  2. **`core.rs:228-238`**——`Instance` 事件循环里核心异常退出且 `tx.send` 失败时 `std::thread::spawn` → `CoreManager::global().recover_core()`（事实 A20f。原计划**遗漏**了这一条）。

  两条删完后 `recover_core()` 本身若失去调用者也一并删除；恢复完全交给 runtime 的 Supervisor（R13）。

- **保留**：`Instance`、`RunType`、`find_binary_path`、`change_default_network_dns`、`status()`（S9.2 的 DNS allowlist 仍需要）。

> 判定原则：只删"被 CoreActor 取代的排他与恢复机制"，不删"尚有 owner 的功能代码"。

### S11 — shutdown 接线（原计划的顺序描述有错）

事实 A18f：`cleanup_processes` 的**真实**顺序是 `save_window_state` → **`resolve_reset()`** → `client.shutdown()` → widget stop → `CoreManager::global().stop_core()`。而 `resolve_reset()` 内部**已经停了一次核**（`resolve.rs:288`）。所以现状是**核被停两次，且第一次发生在 `client.shutdown()` 之前**——原计划"顺序保持 client.shutdown() → widget stop → 停核"的描述是错的。

**改法（reviewer 建议，已核实可行）：**

1. **从 `resolve_reset()` 里移除停核**，只留 `reset_sysproxy()`。可行性已验证：`resolve_reset` 全仓**只有一个调用者**（`help.rs:251`），移除后不影响任何其它路径；
2. `NyanpasuClient::shutdown()` 追加：rebuild worker 关闭**之后**调 `CoreClient::shutdown()`（`Shutdown` 消息 → 拒绝全部 waiters → 关 backend；5a 无订阅任务需要 join）；
3. 删除 `help.rs:268` 的 `CoreManager::global().stop_core()`——停核已由第 2 步完成；
4. 最终顺序：`save_window_state` → `resolve_reset()`（现在只重置系统代理）→ `client.shutdown()`（rebuild worker + CoreActor + backend）→ widget stop；
5. 更新 `shutdown()` 的契约 doc comment（`client/mod.rs:392-401` 现在明写"不停 CoreManager globals"，5a 后不再成立）。

> 顺序理由：系统代理必须在核心停止**之前**恢复，否则会留下一段"代理指向已死核心"的窗口——这正是现状把 `resolve_reset` 放在最前的原因，保留该位置。

### S12 — 测试

全部 TempDir + barrier/RPC 同步，**零 sleep 断言**（A10f）。

#### A2 gate 测试（对应 A-Exit 六项）

| ID      | 名称                                                 | 断言                                                                                                                                                              | A-Exit 项         |
| ------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| T-OP-01 | `waiters_are_granted_in_fifo_order`                  | 三个 waiter 依次入队；释放 active 后按入队顺序放行（用 oneshot 记录放行次序）                                                                                     | FIFO              |
| T-OP-02 | `dropping_a_waiting_guard_cancels_it`                | waiter guard drop → 从 waiters 移除；后续 release 不会把它误认为 active                                                                                           | 等待取消          |
| T-OP-03 | `guard_dropped_right_after_grant_releases_active`    | 用 barrier 让 release 与 grant 竞争：guard 在刚被提升为 active 后 drop → active 清空且下一个 waiter 被放行                                                        | 刚获批取消        |
| T-OP-04 | `stale_release_is_idempotent_noop`                   | 对已完成/未知 ID 发 `ReleaseOperation` → 不影响当前 active，不 panic                                                                                              | stale release     |
| T-OP-05 | `mutation_with_wrong_id_returns_stale`               | 持 A 的 id 时用 B 的 id 发 `Run` → `StaleOperation`，且 backend **零调用**（TestBackend 计数）                                                                    | wrong-id mutation |
| T-OP-06 | `shutdown_drains_all_waiters`                        | 一个 active + 两个 waiter 时 shutdown → 两个 waiter 都收到错误，backend 收到 stop                                                                                 | shutdown drain    |
| T-OP-07 | `acquire_times_out_and_releases_the_waiter`（RQ-04） | 用 `tokio::time::pause()` 推进到 `CORE_ACQUIRE_TIMEOUT` → `begin_operation` 返回 `AcquireTimeout`，且 waiter 已从队列移除（后续 release active 时直接放行第三个） | RQ-04             |

#### A1 backend parity 测试

| ID      | 断言                                                                                                                                                                                                                                                                                                            |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-BK-01 | **真实 parity**：`CoreBackend::{Local, Service}` 对 check / run / stop / recover 的成功路径产生一致的 `CoreStatusView` 转换。Local = TempDir + manager-compatible 探针核（见下 §S12.1）；Service = 真实 IPC roundtrip harness（见下 §S12.2）。**不得用 `TestBackend` 顶替**（A-Exit 明写 Local/Service parity） |
| T-BK-02 | `LocalBackend` 构造出的 `ManagerOptions.local_ipc_policy == LocalIpcPolicy::Disable`（显式化的回归钉）                                                                                                                                                                                                          |
| T-BK-03 | apply outcome 映射：7 个本地 `ApplyOutcome` 分支 → `CoreApplyData`，含 `DurabilityUncertain` 单层与**双层嵌套**（warning 以 `"; "` 拼接），`Noop` 不丢失                                                                                                                                                        |
| T-BK-04 | `local_error_kind` 对 12 个 `nyanpasu_ipc::api::error_kind` 常量的映射（断言用常量而非字面量）                                                                                                                                                                                                                  |
| T-BK-05 | `is_recovery_exhausted` 对上游前缀命中/不命中（纯函数层）                                                                                                                                                                                                                                                       |
| T-BK-06 | **`core_recovery_exhausted` 恰好发布一次**（D5 / design §5）：注入 `MockCoreDegradationSink`，重复投递同一个耗尽状态 → `publish` 调用次数 `== 1`。**并含 recover-重放分支**：耗尽 → 发布 1 次 → `recover` 成功（核心**未**拉起）→ 重放同一耗尽快照 → `publish` 仍 `== 1`（`recover` 不复位 latch）              |
| T-BK-07 | **latch 复位后可再次发布**：耗尽 → 发布 1 次 → 核心重新 `Running`（或 `SetBackend` 换槽）→ 再次耗尽 → 累计 `publish` 次数 `== 2`                                                                                                                                                                                |
| T-BK-12 | **latch 在投影前判定**（round-3 #5 回归钉）：构造 `Starting → 耗尽 → Starting → 再耗尽` 序列（**全程从未到达 `Running`**）→ `publish` 次数 `== 2`。若实现改成读投影后的两值 `CoreStatusView`，`Starting` 不可见、latch 无法复位，第二次不会发布，本用例即刻失败                                                 |
| T-BK-13 | **`Degradation` DTO 取值**：`phase == DegradationPhase::CoreLifecycle`、`code == "core_recovery_exhausted"`、`retryable == true`、`message` 含 runtime 原始 reason 且**长度 ≤ 512 字节**（超长 reason 被截断）                                                                                                  |

#### facade 编排测试

| ID      | 断言                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-FA-01 | **控制失败仍尝试拉核**（S8(4) 保留的既有语义）：让 `control::start_service` 返回 `Err` → facade **仍然**走 `SetBackend` + `Run`（fail-open-to-Local），最终返回的是控制操作的 `Err`。若实现被"优化"成先 `res?` 再 run，本用例即失败。故障通过注入的 `ServiceControlOps` mock 制造，**不执行任何 OS service 命令**                                                                                                                                                                                                |
| T-FA-02 | **`enable_service_mode == false` 时不碰核心**：typed 快照为 false → 不取 guard、不发 `SetBackend`/`Run`（backend 零调用），只返回控制操作结果                                                                                                                                                                                                                                                                                                                                                                    |
| T-FA-03 | **对账块任一环节失败都不改变返回值**：分别注入"快照读取失败"/"取 guard 超时"/"`SetBackend` 失败"/"构造请求失败"/"`Run` 失败"五种故障 → 每种都只记录日志，facade 仍返回**控制操作**的结果（成功则 `Ok`，失败则原始 `Err`）                                                                                                                                                                                                                                                                                        |
| T-FA-04 | **四个 service 方法 + `core_status` / `restart_core` 都不需要命令层接触 `CoreClient`**：编译期保证——`ipc.rs` 内不出现 `CoreClient` 类型名（用 `rg` 断言）                                                                                                                                                                                                                                                                                                                                                        |
| T-FA-05 | **控制失败的错误不被顶替**：控制返回 `Err(A)` 且对账也失败 `Err(B)` → facade 返回的是 **A**，B 只进日志                                                                                                                                                                                                                                                                                                                                                                                                          |
| T-BK-08 | **换槽后目录锁可重新获取**（D2 协议回归钉）。**必须走 Local → Service → Local**，不能用同模式换槽：同模式 `SetBackend` 的行为未定义，合理实现可以直接 no-op，那样测试会在什么都没释放的情况下通过（假阳性）。断言两点：(a) 第二次 Local 的 `CoreManager` 构造**成功**（不出现 `Error::RuntimeDirectoryOwned`）；(b) backend **身份确实变了**——用 `LocalBackend` 里递增的构造计数器（或 `Arc::ptr_eq` 判否）证明是新实例而非复用。等效替代：直接 consume/drop 第一个 `LocalBackend` 再在同一 `runtime_dir` 上重建 |
| T-BK-09 | **无 backend 失败态**：让替换构造失败 → 槽位为 **`Some(BackendSlot::Failed { error })`**（不是 `None`——`None` 只在换槽**瞬态**出现）→ mutation 返回 `CoreActorError::NoBackend { last_error }`（`Arc` 克隆自槽位）；`status()` 经 watch 仍可读且 `state == Stopped`；随后一次成功的 `SetBackend` 能恢复                                                                                                                                                                                                          |
| T-BK-10 | **`service_needs_stop` 表驱动**（对应 R5d）：遍历 `CoreStateDetail` **全部 6 个变体** —— `Stopped` → `false`；`Starting` / `Running` / `Restarting` / `Switching` / `Stopping` → `true`；外加 `detail == None` → `true`（fail-safe）。共 7 个用例，用表驱动写法保证新增变体时会因不穷尽而被发现                                                                                                                                                                                                                  |
| T-BK-11 | **stop 竞态到 not_started**：`status` 判定需要先停，但 `stop_core()` 返回 `error_kind = "not_started"` → `run()` 仍**成功**并继续 `start_core`；换成任意**其它** `error_kind`（例如 `quarantined`）则 `run()` **失败**且不调 `start_core`（断言 harness 的调用序列）                                                                                                                                                                                                                                             |

#### RQ-02 revision 测试

| ID       | 断言                                                                                                                                                                                                                                                                                                                                                      |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-RV-01  | **两个观察来源都能更新 `last_revision`**（§2.1.1）：操作返回值、`RefreshStatus` 结果                                                                                                                                                                                                                                                                      |
| T-RV-02  | **Service 每次操作/刷新重新学习 revision**：无持久连接，`run()` 与 `RefreshStatus` 各自从 `/status` 取回当前 revision（取代 v6 的重连对账用例）                                                                                                                                                                                                           |
| T-RV-03a | `ConfigRevision → ConfigRevisionInfo`：`epoch` / `generation` / `source_hash` / `effective_hash` 保真，**`runtime_path` 被丢弃**                                                                                                                                                                                                                          |
| T-RV-03b | `RevisionId → RevisionIdInfo`：三字段（`epoch` / `generation` / `effective_hash`）直拷。**这是两个不同的转换**——`RevisionId` 本来就没有 `runtime_path`，原 T-RV-03 把两者混为一谈，语句不成立                                                                                                                                                             |
| T-RV-05  | **start 后同步回填**：Service `run()` 成功（`start_core` 返回 `()`，不带 revision）后 `last_revision` 已非空；Local `switch()` 同理                                                                                                                                                                                                                       |
| T-RV-06  | **两种刷新形态**（§2.1.3）：(a) `RefreshStatus { operation: active_id }` → 查 backend 并提交、发布 watch；(b) `RefreshStatus { operation: wrong_id }` → `Err(CoreActorError::StaleOperation)` 且 **backend 零调用**；(c) `cast RefreshHint` 且 gate **有** active → 出队时**丢弃**，backend 零调用；(d) `RefreshHint` 且 gate 空闲 → 执行刷新并发布 watch |
| T-RV-07  | **操作返回值在回复前已提交**：`run()` 的 RPC 一返回，`CoreClient::status()` 立即可见新 revision，**无需**任何后续刷新或推送                                                                                                                                                                                                                               |
| T-RV-08  | **`RefreshStatus` 失败不污染缓存**：backend 查询返回 `Err` → actor 保留上一次成功的 `CoreStatusView`，把错误回给调用方                                                                                                                                                                                                                                    |
| T-RV-09  | **UI 读路径最终观察到耗尽并只发布一次**（§2.1.4 / D5）：最后一次操作后注入耗尽 → `core_status()`（watch 读 + `cast RefreshHint`）→ hint 出队（gate 空闲）→ 刷新 → 提交并发布 watch → **下一次** `core_status()` 读到耗尽，`publish` 次数 **== 1**；再读一次仍 **== 1**（latch）                                                                           |
| T-RV-10  | **读的活性**（八审发现，watch 回归的核心理由）：让一次 **`Run`** 阻塞在 backend barrier 上（**不是 `apply`**——D3=A 下 apply 不占 actor handler）→ 期间调 `CoreClient::status()` → **立即返回**（watch 克隆，零 mailbox）。若实现回退成 mailbox RPC 读，本用例会等到超时而失败                                                                             |
| T-RV-11  | **换槽瞬态也发布**（§2.1.7）：`SetBackend` 期间取出旧 backend 后、构造新 backend 前，用 barrier 卡住 → 此刻 `status()` 读到 `state == Stopped`（合成观察已 `commit()` 并发布），而不是旧的 `Running`                                                                                                                                                      |
| T-RV-12  | **shutdown 发布终态**：`CoreClient::shutdown()` 后 `status()` 读到 `state == Stopped`（关停合成观察已发布）                                                                                                                                                                                                                                               |
| T-RV-13  | **外部 apply 的最终一致例外**（§2.1.6.1）：lease 内 `api::put_configs` 改变运行态 → watch **不更新**（该路径不经 actor）→ 随后一次 idle `RefreshHint` 到达 → watch 更新到新状态                                                                                                                                                                           |
| T-RV-14  | **hint 合流**（§2.1.6）：gate 空闲时连发 N=10 次 `core_status()` → backend 的 `observe_status` 调用**至多 1 次在途**，总调用数 ≪ N（断言 ≤ 2：一次在途 + 一次清位后重排）                                                                                                                                                                                 |
| T-RV-15  | **Updater 换核竞态**（round-9 #2）：开始 update（目标核 A）→ **下载期间**把 typed 快照切到核 B → `replace_core` 在 guard 内解析 → 用的是 **B**（当前快照），因此不会停/重启用户已切走的核                                                                                                                                                                 |

#### seam 回归

| ID      | 断言                                                                                                                                                                                         |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-SM-01 | 既有 `client/mod.rs` 与 `rebuild.rs` 的全部 lease 测试在 6 个替身更新后**继续通过**（A9f 列的替身逐个适配，不改测试语义）                                                                    |
| T-SM-02 | `rebuild.rs:930-1000` 的 `s04_concurrent_restart_waits_until_change_core_rollback_completes` 在三层嵌套（clash_patch_gate → rebuild_gate → OperationGate）下仍然通过——这是顺序不变式的回归钉 |

**Local backend 测试的 fake core 问题（事实 R17）。** 两侧的 fake core 都**不能**靠 `CARGO_BIN_EXE_*` 拿到：

- `nyanpasu-core-manager` 的 fake core 是**它自己 package 的 `[[bin]]`**，`CARGO_BIN_EXE_*` 只在同 package 内可见，app 侧取不到（R17）；
- 本仓的 `backend/fake-core` 对 `backend/tauri` 是 **dev-dependency**，而 dev-dependency **既不构建该 binary 也不设置 `CARGO_BIN_EXE_fake-core`**——这一点在 `backend/tauri/Cargo.toml:275-280` 有明确注释。既有做法是**预构建 + 运行时定位**：`cargo build -p fake-core`（或 `cargo test -p fake-core`），然后 `fake_core::require_bin_path()` 按 `current_exe` 的 profile/triple 查找，支持非空 `NYANPASU_FAKE_CORE` 覆盖，最后回退 target 目录（`backend/fake-core/src/lib.rs:399-418`）。现成消费者示例：`APP/client/process_core_bridge.rs:18-20`。

**因此：** 用真实 `LocalBackend` 驱动真实 `CoreManager` 时必须预构建 + 运行时定位。**但既有的 `fake_core::require_bin_path()` 不能用于探针核**——它解析的是 `BIN_NAME = "fake-core"`（`backend/fake-core/src/lib.rs:48,384-418`），找不到 `manager-probe-core`。因此 `backend/fake-core/src/lib.rs` 需要**新增一个同构的具名 resolver**：

```rust
pub const PROBE_BIN_NAME: &str = "manager-probe-core";
pub const PROBE_PATH_ENV: &str = "NYANPASU_MANAGER_PROBE_CORE";
/// 与 `resolve_bin_path` 同样的三段发现顺序，只是目标 bin 名与覆盖环境变量不同。
pub fn resolve_probe_bin_path() -> PathBuf { /* env 覆盖 → current_exe 同目录 → target 回退 */ }
pub fn require_probe_bin_path() -> io::Result<PathBuf> { /* 缺失时给出可操作的错误 */ }
```

解析出的路径直接喂给 `CoreRequest.binary_path`（**不经过** `find_binary_path`——那会去找真实核心）。

#### S12.1 — Local parity 的探针核（**不允许降级为 TestBackend**）

两个**互相独立**的门槛，缺一不可：

1. **manager 的默认 readiness 是 `ControllerVersionProbe`**——"healthy iff `GET /version` succeeds"（`RT/.../health/probe.rs:109`）。本仓 fake-core 的内置 HTTP 只对精确 `PUT /configs` / `PATCH /configs` 作答，其余 404（`backend/fake-core/src/main.rs:22-23,305-344`）。
2. **fake-core 自己要求父进程 barrier**：长驻启动必须有 `FAKE_CORE_READY_ADDR`，否则 exit 2（事实 A24f，`main.rs:13-18,137-147`）。

第 2 条是关键麻烦：**spawn 子进程的是 manager，不是测试**，所以测试无法逐次给子进程注入环境变量，只能设**父进程全局环境**让 manager 的子进程继承——这会强制所有并发测试串行化。

**推荐路径：新增第二个 `[[bin]]`——一个不需要父进程 barrier 的 manager 兼容探针核。**

```toml
# backend/fake-core/Cargo.toml
[[bin]]
name = "manager-probe-core"
path = "src/bin/manager_probe_core.rs"
```

契约（刻意做到最小）：

- argv 与真实核心一致（复用既有 `fake_core::parse_args`：`-t -d <dir> -f <config>` 干跑；`-d <dir> -f <config>` 长驻）；
- `-t` 干跑：读 config、退出码 0（失败注入用**自己的**环境键，与 `FAKE_CORE_*` 不共享命名空间）；
- 长驻：**不需要任何 barrier**，直接在 config 的 `external-controller` 端口上起 HTTP，`GET /version` 返回 `200 {"version":"manager-probe"}`，其余路径 404；
- 端口来源**只从 config 文件读**，不读环境变量——于是"端口对齐"变成结构性保证而非约定。

**为什么端口能对齐：** `LocalIpcPolicy::Disable` 下 manager **不重写** source config 的 `external-controller`（R3 / `spec.rs:65-77`）。所以测试写入 config 的端口 = 探针核监听的端口 = manager 探针访问的端口，三者天然一致。测试用 `tests/common` 风格的 `free_port()` 先取一个空闲端口再写进 config。

> **为什么不提供"复用 fake-core"的备选路径（leader 裁定，round-4 #6）：那条路径是错的。** fake-core 的 barrier 语义不是"放行后继续跑"，而是"收到 RELEASE 即收摊退出"——`signal_ready_and_wait_release` 返回后它立刻 `stop.store(true)`、join HTTP 线程并 `return ExitCode`（`backend/fake-core/src/main.rs:233-249`；release 实现 `lib.rs:490-493`）。所以"accept → release → 让 manager 去探针"根本不成立：release 的那一刻 HTTP 服务就没了。要让它活到断言结束就必须**全程不 release**，那又与"barrier 是同步点"的设计相悖，还得把 ready 连接的所有权一路带到 teardown。**一条好路径胜过两条半路径**——`manager-probe-core` 是唯一 parity 路径。

**S13 必须加对应的预构建命令**（dev-dependency 不会构建 bin，事实 A15f）。

#### S12.2 — Service parity 需要真实 IPC roundtrip harness

`ServiceBackend` 走命名管道 / Unix socket（端点形态见事实 R19）。parity 测试**不能**打真实 daemon（端点是全局固定路径，PR-5-pre 已确认），因此：

- 测试内起一个**进程内最小 IPC 服务端**，绑到**测试专用 placeholder**（如 `nyanpasu_ipc_test_{pid}_{n}`，避开生产的 `nyanpasu_ipc`），实现 parity 所需端点：`/status`、`/core/start`、`/core/stop`、`/core/recover`；
- `ServiceBackend` 用 `Client::new(<测试 placeholder>)` 构造——这正是 A1 卡"禁止 `service_default()`、必须实例化 client"带来的**可测性收益**；
- Windows 命名管道与 Unix socket 绑定方式不同，harness 用 `#[cfg]` 分支。

**禁止裸 `#[ignore]`（round-3 #6）。** 允许某平台跳过就等于允许 parity 从常规验证里静默消失。规则：

- **CI 支持的平台上 parity 必须作为常规测试运行**，不加任何 `#[ignore]`；
- 某目标确实无法实现 harness 时，该测试标 `#[ignore = "<具体原因> — 由 <命令> 显式运行"]`，并且**必须**在 `package.json` 增加一条显式门禁脚本（例如 `test:backend:parity` = `cargo test --manifest-path ./backend/Cargo.toml --all-features -- --ignored parity`）；
- 该脚本**必须进 CI**（与 `pnpm test` 并列的一个 job），否则视为未实现——"有个命令可以跑"不等于"有人在跑"。

### S13 — 门禁

```powershell
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
cargo build --manifest-path .\backend\Cargo.toml -p fake-core --bin manager-probe-core   # T-BK-01 前置：仓库根**没有** Cargo.toml，必须带 --manifest-path（A15f / round-5 #6）
pnpm test:backend
git diff frontend/interface/src/ipc/bindings.ts   # 期望：恰好一处新增（见下），其余零变化
pnpm lint:ts
pnpm architecture-ledger
pnpm lint:architecture-ledger
```

**bindings 预期：差异恰好等于 `DegradationPhase` 新增 `CoreLifecycle` 变体**（Rust 侧一个 enum 变体 + TS 侧联合类型多一个成员 `'core_lifecycle'`），**其余零变化**。`get_core_status` 保持元组形状（S9）；四个 service 命令与 `update_core` 新增的 `tauri::State` 参数不进 TS 签名（S8 / S9.1）。若 diff 超出这一处 → 说明范围溢出，停下核查。

**ledger 基线（已于 2026-08-02 实测，`--mode=gate` 当前通过）：**

```
config_calls 116 · service_globals 74 · migration_markers 19 · legacy_dto_refs 300 · test_real_dirs 0
```

> 注意：这是 **PR-5-pre 落地后**的实测值，与本计划初稿引用的 120/80/18 不同（初稿误用了 5-pre 之前的快照）。以本节为准。

**ledger 预期变化（必须逐条核对后再 `--write-snapshot`）：**

| 指标                                       | 方向                                    | 原因                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------------------------------------ | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `service_globals["CoreManager::global()"]` | **显著下降**（基线 16 条计数）          | S9 迁移 + S9.1 Updater 迁移 + S10 删两条恢复路径 + S8 删 `LegacyCoreBridge`。**5a 后生产代码仅剩 `feat.rs:392` 一处**（macOS DNS，allowlist 到 5c），另有 `process_core_bridge.rs:4` 的文档注释文本命中                                                                                                                                                                                                                |
| `migration_markers`                        | 基线 19 → **预期 17**                   | **+2**：S4 的 R0 过渡标记、S9.2 的 macOS DNS allowlist 标记。**−4**：`core_bridge.rs` 的 `CoreManager::global()` 桥（随 `LegacyCoreBridge` 删）、`core/clash/core.rs` 的 legacy lifecycle 桥、`updater/instance.rs` 的 legacy core manager 桥（S9.1 迁移）、`ipc.rs` 的 core manager status 桥（S9 迁移）。**±0（相对迁移）**：`core_bridge.rs` 的 connection-interruption 标记随 `on_profile_change` 一起搬到新实现处 |
| `config_calls`                             | 基线 116，**预期下降 ≥ 2**              | 新代码禁调 `Config::*()`；S8(3) 的健康检查注入根把 `init/mod.rs:235` 与 `service/mod.rs:19` 两处 `Config::verge().enable_service_mode` 改为传值 → 至少 −2。实际降幅可能更大（S9 迁移的调用点），照实记录                                                                                                                                                                                                               |
| `test_real_dirs`                           | **必须仍为 0**                          | 新测试只用 TempDir + 注入的 `RuntimePaths`；S12.2 的 IPC harness 用测试专用 placeholder，不碰生产端点                                                                                                                                                                                                                                                                                                                  |
| `bridgeFiles`                              | 8 → 7（若 `core_bridge.rs` 整文件删除） | 该文件同时还装着 `RunningConfigPatchPort` / `LegacyRunningConfigPatchBridge`（B3 才删）与 `restore_product`，**大概率保留**。以实际删除结果为准，不要为了凑数字而搬移代码                                                                                                                                                                                                                                              |

> **两条硬规则**：(1) `config_calls` 或 `test_real_dirs` 变差 → **回头改代码，不要靠改 snapshot 掩盖**；(2) `migration_markers` 若不等于 17，**逐条比对上表列出的加减项**找出差异来源再决定，**不要盲目 `--write-snapshot`**。

---

## 5. Exit 判据映射

task.md A-Exit 三条：

| Exit                                                                                              | 交付步骤    | 验证                                                                                                                                                                               |
| ------------------------------------------------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| operation 测试：FIFO / 等待取消 / 刚获批取消 / stale release / wrong-id mutation / shutdown drain | S5、S6、S12 | T-OP-01…06 全绿                                                                                                                                                                    |
| Local/Service 基本生命周期 parity 测试                                                            | S3、S12     | T-BK-01…13 全绿，其中 **T-BK-01 必须是真实 Local/Service 双端**（S12.1 探针核 + S12.2 IPC harness），不接受 TestBackend 顶替                                                       |
| legacy core 生命周期不再被新调用点使用                                                            | S9、S10     | `rg 'begin_lifecycle\|lifecycle_lock\|_with_lease' backend/tauri/src` 为 0；`CoreManager::global()` 的**生产代码**命中仅剩 `feat.rs:392`（macOS DNS，带 allowlist 标记，owner=5c） |
| 既有 lease 调用点与并发契约不回归                                                                 | S7、S12     | **T-SM-01**（6 个测试替身适配后既有 lease 测试全绿）、**T-SM-02**（三层嵌套顺序不变式回归钉）                                                                                      |

roadmap §6.1 附加项：

| §6.1 判据                                                    | 对应                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 封闭 enum，不定义 `CoreEngine` trait/factory                 | S3；`rg 'CoreEngine\|EngineFactory' backend/tauri/src` 为 0                                                                                                                                                                                           |
| `LocalIpcPolicy::Disable` 显式写出                           | S3 + T-BK-02                                                                                                                                                                                                                                          |
| 禁用 `service_default()`                                     | S3；ServiceBackend 用 `Client::new`                                                                                                                                                                                                                   |
| client 预分配 `OperationId` + pending guard                  | S6 + T-OP-03/07                                                                                                                                                                                                                                       |
| 不实现 TTL / auto-steal / watchdog                           | S5；`rg 'ttl\|auto_steal\|watchdog' backend/tauri/src/core/actor` 为 0                                                                                                                                                                                |
| actor 无第二层恢复，只**发布一次** `core_recovery_exhausted` | S3(D5) + S10（删两条裸线程路径） + T-BK-05/06/07                                                                                                                                                                                                      |
| A3 兼容 seam 保留，旧 trait 名不扩散                         | S7；新代码里 `CoreLifecycle*` 只出现在适配 impl 中                                                                                                                                                                                                    |
| RQ-02 / RQ-04 已作答                                         | 本计划 §2 + **§2.1（读/写模型，含 2.1.6 合流与 2.1.7 接线）**；测试 **T-RV-01…15**（06 两种刷新形态；09 UI 最终一致 + 只发一次；10 读的活性；11/12 瞬态与关停发布；13 外部 apply 例外；14 hint 合流；15 Updater 换核竞态），RQ-04 由 **T-OP-07** 覆盖 |

---

## 6. 风险与回滚

| 风险                                                       | 概率 | 影响                                  | 缓解                                                                                                                                                            |
| ---------------------------------------------------------- | ---- | ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CoreManager::new()` 的运行目录独占锁与 daemon 冲突        | 中   | Service 模式下建 Local backend 会失败 | D2=A 只建当前模式匹配的那个；且 app 与 daemon 的 runtime_dir 本就不同（daemon 用 `service_data_dir`）。**S3 开工时先写一个断言两路径不相等的测试**，不要假设    |
| lease 被 move 进 `async move` 闭包导致 guard 不满足 `Send` | 中   | 编译失败                              | A4f 已定位为编译期硬约束；`CoreOperationGuard` 的字段（`OperationId` + `CoreClient` + `bool`）全部 `Send`，`ActorRef` 亦然                                      |
| 三层嵌套锁引入死锁                                         | 低   | 挂起                                  | A3f 证明全局顺序一致（10/10 处 `rebuild_gate` 先于 `begin`）；T-SM-02 作回归钉                                                                                  |
| 6 个 lease 测试替身适配引发大面积测试改动                  | 高   | diff 变大、review 困难                | 适配只改**构造**不改**语义**；`MockRunningCoreBridge` 的 4 方法 mockall 面保持不变，只在 lease 侧多包一层 guard                                                 |
| 上游"重启预算耗尽"字符串前缀漂移                           | 中   | `recovery_exhausted` 静默失效         | D5 集中在一处 + T-BK-05 钉住；typed 变体作**独立上游小 PR**（leader 已裁定不并入已收口的 R0 分支）                                                              |
| 本地 clippy 假红（共享 target kache 污染）                 | 中   | 误判                                  | 已知问题：用独立 `--target-dir` 复验再下结论                                                                                                                    |
| `apply` 实现但不接线被审查判为投机代码                     | 中   | review 争议                           | D4=A 已裁定：实现并由 T-BK-03 钉住 outcome 映射，生产接线留给 B3                                                                                                |
| **换槽时目录锁未释放**（漏 drop `Arc`）                    | 中   | `SetBackend` 后新 manager 构造失败    | R6b 明确锁只在最后一个 `Arc<Inner>` drop 时释放；D2 换槽协议逐步写死；**T-BK-08 是专门的回归钉**。5a 无订阅任务，持有 `Arc` 的只有 backend 自身，风险面比 v6 小 |
| 观察值陈旧（无推送流）                                     | 中   | 状态显示滞后于实际                    | **已知的范围取舍**：5a 的观察是"操作驱动 + 按需刷新"，不是实时。需要新鲜度的路径持 guard 刷新（§2.1.2）；实时性由 C1 的 watch 投影交付                          |
| 新增 `manager-probe-core` bin 影响既有 fake-core 测试      | 低   | 既有 S09 进程矩阵测试红               | 它是**独立的第二个 `[[bin]]`**，与 `fake-core` 不共享源文件、不共享 `FAKE_CORE_*` 环境命名空间，对既有测试是纯新增；commit 1 单独打头即为验证该前提             |
| Updater 迁移触及下载/替换事务                              | 中   | 更新流程回归                          | S9.1 只替换 3 个生命周期调用点，下载/校验/复制逻辑**逐字不动**；guard 覆盖范围与原 `begin_lifecycle` 完全一致                                                   |

**回滚：** 改动集中在——新增 `APP/core/actor/`（目录）、`APP/client/core.rs`；新增 `backend/fake-core/src/bin/manager_probe_core.rs` 与 `backend/fake-core/Cargo.toml` 的 `[[bin]]` 条目、**`backend/fake-core/src/lib.rs`**（具名 probe resolver，round-6 #12）（S12.1）；定点修改 `setup.rs` / `ipc.rs` / `feat.rs` / `utils/{help,resolve,init}.rs` / `core/clash/core.rs` / `core/service/{ipc,mod,control}.rs` / `core/updater/{mod,instance}.rs` / `client/{mod,core_bridge,runtime,event_sink}.rs`；以及 `backend/Cargo.toml` + `backend/tauri/Cargo.toml`（S1）。第一个 commit 单独回滚不影响生产路径。

---

## 7. 提交切分建议

1. `test(fake-core): add manager-compatible probe core binary` —— S12.1 的 `manager-probe-core`（新 `[[bin]]` + `Cargo.toml` 条目 + `src/lib.rs` 的 `resolve_probe_bin_path` / `require_probe_bin_path`）。**单独打头**：它是 T-BK-01 的前置，且对既有 S09 进程矩阵测试是纯新增，先独立验证不污染既有测试；
2. `feat(core): add CoreBackend enum and cancellation-safe OperationId protocol` —— S1–S6 + T-OP / T-BK / T-RV（纯新增，生产路径未变）；
3. `refactor(core): own the core lifecycle in CoreActor` —— S7–S11 + T-SM 回归 + S13。其中 S9.1（Updater）与 S10（删 legacy lease）**必须在同一个 commit 内**——拆开任一半都无法编译。

---

## 8. 明确 out-of-scope（登记去向）

| 项                                                              | 去向                                                                                                |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| typed `CoreErrorKind` 消费（替换 S4 的过渡映射）                | R0 合并 + submodule bump 之后的**独立一步**；bump 本身待用户授权                                    |
| `StopReason::RestartBudgetExhausted` typed 变体                 | **独立上游小 PR**，待用户授权推送（leader 已裁定不并入已收口的 R0 分支）                            |
| apply 管线统一到 `CoreBackend::apply`                           | **PR-5b / B3**（D3=A）                                                                              |
| Promoted / Applied 入 actor、删 `RuntimeLifecycleStore`         | **PR-5b / B1**                                                                                      |
| 删 `rebuild_gate` / `clash_patch_gate`                          | **PR-5b / B2**                                                                                      |
| `change_core` 简化为 commit-first                               | **PR-5b / B4**                                                                                      |
| post-commit 失败矩阵（RQ-01）、apply parity 含 `Noop`（RQ-03）  | **PR-5b 计划**                                                                                      |
| watch snapshot 投影、100 条 `LogFrame` ring、删 `Logger` global | **PR-5c / C1**                                                                                      |
| `set_mode` / `reconcile_mode`、删 5 s 轮询线程与 statics        | **PR-5c / C2**                                                                                      |
| macOS DNS 归入 actor（`MacosDnsGuard`）                         | **PR-5c / C3**                                                                                      |
| UpdaterActor 完整迁移、`UpdaterManager::global()`               | **PR-6d**。5a 只把 Updater 的 **core 依赖**显式注入（S9.1，编译必要条件），updater 自身的全局性不动 |
| `clash-api` workspace 条目                                      | 无 app 侧消费者，暂不加（D1=A）                                                                     |
