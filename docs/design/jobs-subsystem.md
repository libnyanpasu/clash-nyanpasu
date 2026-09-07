# Jobs 子系统初步设计

状态：总体设计；P0 已收敛，生产实现尚未开始。日期：2026-09-07。

详细边界以 [P0 实施契约与验证结论](jobs-p0-contract.md) 为准。

依据：[分享对话：Rust 定时任务库比较](https://chatgpt.com/share/6a9e71aa-4344-83e9-b3ff-041d5ee7bca4)中最后一版、结合 Nyanpasu 源码的建议。该版本已将早期的 scheduler + SQLite 候选方案收束为 Tokio + ractor + redb；本设计沿用后者，不把对话中的候选方案都列为实施目标。

初稿核对本地应用 `7e24d8e4`（#5219）；P0 已更新核对至 `f30d5221`（#5220），不是分享中审查的 `cab2330b`。代码路径均相对应用仓库；本文中的新接口名称是建议，不代表已有能力。

实施拆解见 [Jobs 实施计划](../plan/2026-09-07-jobs-subsystem-plan.md)。

## 1. 目标与范围

统一定时和手动执行：配置派生 Job，触发得到唯一 RunId，执行原有业务服务，按这次 Run 等待、取消、查看结果及日志。普通函数/闭包即可定义 Job，移除旧控制流宏，不考虑旧 Rust API 的兼容成本。

首批实际接入订阅刷新及已有维护任务。提供任务列表、执行历史、单次详情的 IPC 和前端入口，避免只做 Rust 框架而继续让用户无法查看日志。

首版不做分布式队列、DAG、插件自动发现、多数据库适配、任意脚本执行、自动重放副作用、所有后台服务的批量迁移。持久化运行记录不意味着进程重启后可以继续执行原 future。Jobs 复用是 crate 级目标，不要求首版新增 daemon 的远程 Jobs API。

## 2. 已核对的问题

| 现有位置                                                          | 当前行为及迁移要求                                                                                                                                |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/src/core/tasks/storage.rs`                         | `remove_task` 删除整个任务索引；`remove_event` 用 event ID 构造 task 索引键；读取悬空事件时 `unwrap`。新记录与索引必须事务更新。                  |
| `backend/tauri/src/core/tasks/task.rs`                            | 按列表长度分配默认 ID；单一 Running 状态不足以表示并发；注册 timer 后才保存索引；`wrap_job!` 混合执行和持久化收尾。替换为稳定 JobKey 和独立 Run。 |
| `backend/tauri/src/core/tasks/events.rs`                          | 主要存运行状态摘要，不是按 Run 关联的日志正文和结构化结果。需要独立日志模型。                                                                     |
| `backend/tauri/src/core/tasks/jobs/mod.rs`                        | 硬编码注册 EventsRotateJob；ClearLogsJob 有实现但未注册。清理运行历史下沉到 journal，不应顺便启用原来未运行的日志删除功能。                       |
| `backend/tauri/src/state/profiles/scheduler.rs`                   | 另有逐 Profile 的 Tokio 定时任务，发送 Scheduled 刷新而不等完成；启动时按物化时间补更新。迁移要替换这条路径并保留补更新语义。                     |
| `backend/tauri/src/client/profiles.rs`、`state/profiles/actor.rs` | 手动刷新等待业务回复，但 origin 固定 Manual；actor 当前接收后将 origin 绑定为 `_origin`。需要真正传播来源和 Run 上下文。                          |
| `backend/tauri/src/client/mod.rs`                                 | `refresh_profile` 在提交后调用 `after_commit`。接入 Jobs 不能丢掉这段副作用协调，也不能让 handler 回调改成触发 Job 的同一 facade，形成递归。      |
| `backend/tauri/src/core/storage.rs`                               | 已使用 redb，但包装层带应用事件和初始化职责。复用存储技术，不把这个 Tauri 包装直接注入通用 crate。                                                |

以上属于静态源码核对，尚未编写复现测试，也未宣称穷尽旧实现缺陷。

## 3. 组件与所有权

P0 确定在 `backend/nyanpasu-runtime/crates/nyanpasu-jobs` 放置可复用 crate；业务绑定留在应用侧，按 runtime PR → 应用固定依赖提交的顺序交付。RedbJobStore 独占 composition root 注入路径的 `jobs.redb`，不复用带 Tauri 初始化职责的 WebStorage 包装。

| 组件                                   | 分类                         | 拥有的职责                                                                        |
| -------------------------------------- | ---------------------------- | --------------------------------------------------------------------------------- |
| JobDefinition、Schedule、派生/校验函数 | Pure service / domain types  | 声明数据、调度计算、配置转 Job 定义，无隐式 I/O。                                 |
| JobsActor / JobsClient                 | Actor service / typed client | 注册表、调度代次、并发接纳、活跃 Run、受管理的 timer 和执行任务。                 |
| JobJournalClient / writer              | Actor service                | 有界写入队列、日志批次、flush 屏障、持久化降级状态和 retention 协调。             |
| RedbJobStore                           | Adapter / port               | 独立表、事务和游标查询；阻塞数据库 I/O 不占用 actor 消息循环。                    |
| ProfilesJobsBinding                    | 受生命周期管理的订阅任务     | 消费提交后的版本化快照，派生并同步 profiles scope；无需为单个派生函数再造 actor。 |
| ProfilesActor、CoreLifecycleClient     | 已有业务服务                 | 继续拥有刷新提交、过期校验和核心生命周期；Jobs 不接管这些状态。                   |
| JobJournalLayer、Tauri commands/UI     | Adapters                     | tracing 归属、薄 IPC 和展示；不提供服务查找或任意闭包执行接口。                   |

Composition root 显式构建 journal、JobsActor、绑定器和业务 handler。handler 只捕获窄依赖与资源 ID，不捕获整个 NyanpasuClient。NyanpasuClient 首选普通领域方法；若保留 `jobs()`，只能返回固定类型的应用 API，不能成为 registry/service locator。

JobsActor 接纳后启动受管理执行任务，不在 handle 中等待整个业务工作流。避免 `ProfilesActor → JobsActor → ProfilesActor` 的同步调用环；配置绑定在业务 actor 外执行。日志 writer 的基础设施错误不得再次写入自身 journal，避免递归。

## 4. 模型及初步执行契约

| 模型             | 首版必要内容                                                                                                           |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------- |
| JobDefinition    | 稳定 JobKey、owner/scope、managed_by、展示名、定义版本、schedule、默认输入、并发策略。handler 单独保留在内存注册项中。 |
| JobSnapshot      | 定义摘要、next_run_at、active_runs、最近完成摘要；不序列化执行器和锁。                                                 |
| RunRecord        | RunId、JobKey、定义版本、触发来源、计划/接纳/开始/完成时间、状态、结果/错误、日志末序号和丢弃量。                      |
| RunCompletion<T> | 业务 outcome 与 journal 持久化状态分开，允许“业务成功但历史保存降级”。                                                 |
| RunLog           | RunId + sequence、时间、级别、target、正文和允许持久化的结构化字段。                                                   |
| ScopeSnapshot    | source_revision、applied_revision、最近同步错误；失败不能显示为已同步。                                                |

Rust 提供 `Job::new(..., Fn(JobContext, I) -> Future<Result<O, JobError>>)`、`JobHandle<I, O>` 和 `RunHandle<O>`；无参数使用 `()`。内部可以类型擦除，外部输入和结果仍保留类型。同步任务通过有并发上限的 blocking 适配器接入，不保留 Sync/Async 双执行器和声明宏。

拟定规则：

- `run_now/run_with` 创建额外一次 Run，不移动原计划；不把当前正在执行的 Run 冒充新执行。
- 同 Job 默认禁止重叠；定时撞车记录 `Skipped(Busy)`，手动撞车返回 Busy。首版不实现手动排队或 `join_current`，有明确需求再加。
- 多个等待者可以等待同一 Run；完成后仍能按 ID 查询。丢弃句柄、IPC 断开、等待超时只结束等待，不取消业务。
- 删除注册项仅停止未来触发；在途 Run 持有定义快照并正常收尾。历史删除是独立命令。
- `cancel` 只表示请求已接纳。保留 Cancelling/不支持取消的可见状态，确认底层工作结束后才释放并发名额。已运行的 `spawn_blocking` 不能靠 abort 强制停止，详见 [Tokio 文档](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)。
- 关闭时先拒绝新 Run、停止 timer，再请求取消/等待在途任务并刷新 journal；超过预算不能伪报已取消。普通 Rust panic 可以统一收尾，进程中止等情况在下一次启动标记 Interrupted/结果未知。

首版调度覆盖 Manual、Once、Interval、Cron。Interval 首次等待完整周期、固定频率；零间隔输入拒绝，业务“禁用自动刷新”转换为 Manual。Cron 显式时区，注册前完整校验；采用已验证的 `croner = 4.0.0` 解析和计算日期；五/六字段、显式 IANA 时区、DST 和迟到规则见 P0 契约。

每个计划拥有可停止的等待任务，Due 携带 JobKey、registration_generation、scheduled_at；删改和同键重建均拒绝旧代次消息。定义内容相同时 reconcile 为 no-op，不比较闭包地址、不重置倒计时。休眠/错过触发默认跳过，订阅的启动补刷新由绑定策略最多补一次；墙钟跳变与单调时钟的边界需要测试。

## 5. Journal 与日志链路

拟建 `runs`、`runs_by_job`、`run_logs` 独立表；按 `(JobKey, admission_sequence, RunId)` 和 `(RunId, sequence)` 范围分页，不按 updated_at 做不稳定分页。不沿用单个 JSON 数组索引；新增、删除主记录及索引在同一事务内完成。

正常顺序为：持久化接纳 → 开始业务 → 得到 outcome → 刷新已接收日志 → 持久化终态 → 唤醒等待者。接纳保存失败时不启动业务；终态保存失败时保留真实业务结果并报告 journal 降级，不自动重跑。失败恢复和重试只针对 journal 写入，不能重放业务副作用；内存保留上限及后续查询行为遵循 P0 契约第 6 节。

tracing layer 只做快速关联、过滤和有界投递；保留 dropped_log_count，关键结果不依赖可丢弃日志。Run span 必须传播到 ProfilesActor 消息、下载任务及 CommitRefreshed 收尾。对 URL token、认证字段、配置正文做允许列表/脱敏，输入输出也要控制体积和敏感信息。

retention 同时限制年龄、条数和字节数，仅清理终态 Run；活跃 Run 的单次日志也有上限。启动时将旧非终态记录标记为 Interrupted，不自动恢复执行。旧表默认保持不读不写，不伪造旧事件的完整 Run 历史；P0 决定不提供自动清理旧历史，不能删除共享数据库。

## 6. Profiles 与应用接入

提交后的快照需要可靠、带版本的订阅，不能拿 Tauri UI 广播充当配置总线。绑定器启动先取最新快照，再持续 reconcile；失败重试最新值，拒绝旧 revision。整份定义先校验，再应用 scope 内差异。

Remote Profile 无论自动刷新是否开启都拥有稳定 JobKey；改名不换键，间隔为零转 Manual，改成 Local/删除则取消注册并保留历史。手动入口要保证目标配置版本已完成绑定，再触发 Run，避免刚创建就刷新时查不到 Job。

新增内部刷新操作，保留 `patch + origin + run context + 完成回复`；现有 pending_refresh、URL/定义指纹及提交过期检查继续生效。参数 patch 当前先提交，随后下载可能失败；迁移不能误报成全部原子回滚。

Run 的业务成功以下载、校验、物化及 Profiles 提交为准；终态发布时间还包括一次现有 `after_commit` 协调返回。窄 workflow 为手动和定时刷新统一执行这段协调，RefreshReport 分别保存提交结果、degradation 和核心协调结果；超时但底层操作可能继续时报告 Unknown 并保留 operation ID。具体边界见 P0 契约第 5 节。

## 7. 查询与前端最小闭环

通过 facade 提供 list/get job、run job、list/get run、游标日志读取、按 RunId 等待及请求取消。run 命令快速返回 RunId；等待命令使用有限超时，重连后可以继续查同一 Run。IPC 只允许执行已注册且允许手动触发的业务任务，并校验输入。

前端路线：任务列表 → 执行历史 → 单次详情。展示来源、调度/next run、状态、结构化结果、错误、日志、截断和持久化降级。配置派生的调度显示 managed_by，并跳转原配置入口修改，不形成第二份配置源。

首版使用游标查询/增量轮询；实时推送后置，查询始终是恢复路径。展示采用专用 DTO，避免直接导出执行器或递归 JSON 导致现有 Specta 导出问题。有限大小的结果字段/文本展示格式在契约阶段确定。
