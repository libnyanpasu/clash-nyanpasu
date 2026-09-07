# Jobs P0：实施契约与验证结论

状态：P0 技术收敛完成，供 P1–P7 实施和评审；不是生产功能已完成。

日期：2026-09-07。应用基线：`f30d52211890f4e2c77aca6ccd9ee2689684b850`（#5220）；已从远端核对并快进文档 worktree。该提交的 runtime gitlink 为 `9dbe16c`。本轮未修改应用或 runtime 生产代码。

本文优先于 [初步设计](jobs-subsystem.md)中的“暂定”内容。阶段进度见 [实施计划](../plan/2026-09-07-jobs-subsystem-plan.md)；可运行验证见 [P0 probe](../probes/jobs-p0/README.md)。

## 1. 决策摘要

| 编号 | P0 决定                                                                                  | 理由及实施阶段                                                                                                                            |
| ---- | ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| D01  | 通用 crate 放 runtime 的 `crates/nyanpasu-jobs`；默认不依赖 Tauri、Profiles 或服务端 IPC | actor/纯类型/存储端口可独立测试。redb adapter 用 `redb-store` feature；应用业务绑定留在应用仓库。P1/P2。                                  |
| D02  | 使用单独的 `jobs.redb`，由 composition root 解析路径并创建唯一数据库所有者               | 现有 Storage 暴露的是借用数据库且初始化较晚；避免改造 WebStorage 或让日志事务争用它的写入路径。仍复用 redb 技术，不增加第二种数据库。P2。 |
| D03  | 分离业务结果、结果编码状态和 journal 持久化状态                                          | 不把终态保存失败、输出编码失败或 runtime 应用未知误报为订阅刷新失败。P1/P2/P4。                                                           |
| D04  | Run 的默认取消能力为 Unsupported；显式声明并实现协作协议才可设 Cooperative               | 当前 Profiles fetch 没有取消入口，运行中的 blocking 工作也不能靠 abort 停止。P1/P4。                                                      |
| D05  | 手动刷新既等待 Profiles 提交，也保留一次原有 post-commit 协调；结果分字段表达            | 调用链不丢失 after_commit，不承诺“刷新成功 = 核心应用成功”。P4。                                                                          |
| D06  | 使用固定 `croner = 4.0.0`、`chrono = 0.4.45`、`chrono-tz = 0.10.4`                       | probe 验证五/六字段、非法输入、时区、DST 和无下次日期；不继承旧 timer 方言。P3。                                                          |
| D07  | 分页排序使用持久化 admission_sequence，而不是墙钟时间                                    | 校时回退时，新 Run 不会落到已翻过的页；时间只用于展示和年龄清理。P2/P5。                                                                  |
| D08  | 当前版本的 IPC 采用有限 DTO；ID、序号和 revision 以字符串传输                            | 避免递归 JSON 导出及 JS 大整数精度问题。Rust 内部仍保留强类型输入/输出。P1/P5。                                                           |
| D09  | 独立 journal tracing layer 和有界采集通道，不受文件日志的全局级别过滤                    | 现有全局 EnvFilter 会提前丢弃事件；只在后面加 layer 不够。P5。                                                                            |
| D10  | 不导入、不自动删除旧任务历史；不启用未注册的 ClearLogsJob                                | P7 停止旧任务读写，旧 WebStorage 数据保持原样；显式清理或日志文件删除属于另一个变更。                                                     |

## 2. 源码核对与必要改动

以下是现状，不是新接口：

| 基线位置                                          | 已核对事实                                                                                       | P0 对实施的约束                                                                                          |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------- |
| `core/tasks/storage.rs`、`task.rs`                | 旧索引误删/错键、按列表长度分配身份、注册后写盘、单 Running 状态仍存在                           | 不修补旧框架作为新能力的兼容层；P1–P3 新内核验证，P7 删除。                                              |
| `client/profiles.rs::refresh`                     | `origin = Manual`，写 RPC 没有等待超时                                                           | Jobs 应持有业务等待；用户 wait 的超时不能传播成取消。                                                    |
| `state/profiles/actor.rs::RefreshRemote`          | origin 被忽略；pending_refresh 按 uid 拒绝重叠；options patch 先提交；下载 task 没有被保存为句柄 | P4 必须保留 patch 的真实提交语义、来源和旧结果校验；即使首版不支持取消，也要补齐下载任务的归属和 drain。 |
| `CommitRefreshed`                                 | 指纹/URL fence 后才物化和提交；无 reply 的定时刷新只 request_rebuild，有 reply 则由 facade 接续  | 迁移为 Job reply 后必须显式接续 rebuild；不能依赖旧的无 reply 分支，也不能重复触发。                     |
| `client/mod.rs::refresh_profile/after_commit`     | 提交后收集 degradation；影响当前配置时等待 rebuild；现有映射将重建错误压成普通 degradation       | P4 抽出窄 post-commit 协调，保留 CoreError 的 operation ID 和未知结果语义。                              |
| `client/core_lifecycle/mod.rs::call_with_timeout` | 等待上限 180 秒；超时可能仍排队/运行，错误携带 operation ID；status 留有限近期结果               | 超时不等于核心失败，不自动重发；不能承诺无限期查询历史 operation。                                       |
| `utils/init/logging.rs`、`lib.rs`                 | 全局 subscriber 在 init_config 中先于应用 facade 安装；全局过滤器在日志层外                      | channel 必须在早期初始化显式创建并传递，不通过新增 OnceCell 或 Logger::global 注入 journal。             |
| `core/storage.rs::setup`、`utils/resolve.rs`      | WebStorage 在 resolve_setup 较晚才创建，并向 Tauri managed state 注册                            | jobs.redb 提前由专用 adapter 打开，不重复打开共享文件。                                                  |
| `utils/help.rs::cleanup_processes`                | 当前关闭入口主要 drain core lifecycle；Profiles post_stop 只关 timer/watch/reconcile task        | P4/P7 必须把 Jobs/Profiles drain 放到核心关闭之前，不能依赖 Drop 时 detached task 自行结束。             |

## 3. Run 状态与操作语义

### 3.1 接纳与身份

JobKey 是稳定业务键；RunId 是每次独立执行的 UUID。客户端在提交前生成 RunId，IPC 请求也携带该 ID，因此即使回复丢失，调用者仍能查它。同一 ID 的重复提交返回 AlreadySubmitted(RunId)，不重新执行，也不以新输入覆盖已有提交；调用者随后按 ID 查询。

JobsActor 为同 JobKey 和全局容量预留名额，再交给受管理的 admission 写入任务。写入待定期间也占名额，但不阻塞 actor handle 等待数据库；cancel、inspect、shutdown 仍可被处理。只有接纳事务确认后才启动 handler。过期 registration_generation、关闭或非法输入在 admission 前拒绝。

| 情况                                    | 对调用方的结果                                   | 是否运行 handler                               |
| --------------------------------------- | ------------------------------------------------ | ---------------------------------------------- |
| 输入非法、无此 Job、手动 Busy、服务关闭 | 明确拒绝，不创建 Run 历史                        | 否                                             |
| 已知接纳事务中止                        | AdmissionFailed                                  | 否                                             |
| 接纳写入/回复仍待定或结果不确定         | AdmissionUnknown，携带 RunId；只查询，不自动重试 | 只有负责此次 admission 的 owner 能在确认后启动 |
| 接纳事务已确认                          | Accepted(RunId)                                  | 启动一次                                       |
| 定时 Busy                               | 写终态 Skipped(Busy) 记录                        | 否                                             |

重复提交检查覆盖正在接纳、执行中及仍在 journal 留存的 Run。去重记录与 Run 的留存期一致。历史清理后不允许重用该 RunId 来表达“重试”；新的主动执行必须生成新 ID。P1 的客户端契约禁止自动重试不确定的提交。

删除/重建 JobKey 不释放旧 Run 的名额：Forbid 按稳定 JobKey 计算，不按注册代次计算。运行持有定义和 handler 快照，后续 reconcile 不改写它的参数或终态。

### 3.2 状态转换

| 当前状态           | 事件                       | 后续状态及结果                                                         |
| ------------------ | -------------------------- | ---------------------------------------------------------------------- |
| Admitting（内部）  | 确认持久化                 | Admitted；若已经关闭 admission，取消尚未启动的执行并保存终态           |
| Admitted           | worker 接管                | Running                                                                |
| Admitted           | 取消/关闭先赢得串行判定    | Finalizing(Cancelled)，不进入 handler                                  |
| Running            | 支持取消且接纳请求         | Cancelling；仅发协作信号，不 abort 业务等待                            |
| Running/Cancelling | handler 确认结束           | Finalizing(outcome)                                                    |
| Finalizing         | 日志 flush 与终态事务确认  | 对应业务终态，journal = Durable；通知等待者                            |
| Finalizing         | 持久化失败/等待预算耗尽    | 同一业务终态，journal = Degraded；保留内存结果、通知等待者并关闭新接纳 |
| 旧进程非终态记录   | 新进程独占数据库并完成恢复 | Interrupted（结果未知）；不重跑                                        |

业务终态：Succeeded、Failed、Cancelled、Interrupted、Skipped。取消与成功竞争时以 worker 的真实结果为准；收到取消请求后仍提交成功，应是 Succeeded。普通 panic 记录 Failed(Panicked)，但不能因此推断已委托业务 actor 的工作也停止；若存在未完成委托，先 drain/确认；确认前保持非终态 Running，并在 inspect 标记 execution_uncertain，关闭该 Job 的新接纳。不得先发布 Failed 再释放名额。

正常 wait 在结果和已接收日志可查询后返回。多个 waiter 共用结果；完成后再 wait 从内存或 journal 读取。wait 超时仅返回 WaitTimedOut(RunId)。日志被丢弃和输出不可编码分别以 dropped_log_count、OutputUnavailable 标示，不改变已知业务成功。

## 4. 取消和关闭

| 任务种类                                         | 首版取消能力     | 条件                                                                                               |
| ------------------------------------------------ | ---------------- | -------------------------------------------------------------------------------------------------- |
| Profiles 刷新，含已提交 options patch 和后续物化 | Unsupported      | P4 没有完整下载/提交取消协议前，不向 UI 承诺可停止；删除 Profile 由既有提交 fence 防止旧下载覆盖。 |
| 声明了协作式停止的纯异步 Job                     | Cooperative      | handler 必须确认不再产生副作用后返回 Cancelled；子任务也受 owner 管理。                            |
| 已开始的 blocking 操作                           | 默认 Unsupported | 只有业务自身提供停止协议才可 Cooperative；abort 无法强制结束它。                                   |
| future 已委托给其他 actor 的工作                 | 默认 Unsupported | 不能把丢弃 RPC future 当取消；只有底层业务取消/完成确认后才能释放对应资源。                        |

cancel 对 Unsupported 返回 Unsupported，不进入 Cancelling；对终态返回 AlreadyFinished；重复的有效取消请求幂等。Admitting 未确认时返回 AdmissionPending，调用者查询接纳结果后再操作，不伪报取消已接受。

启动顺序：早期创建日志通道并安装 layer → 解析 jobs.redb 路径/启动 journal/恢复旧记录 → 启动 Profiles/Core 等业务 actor → 构造 JobsActor 与 handler → 启动版本化绑定 → 初次 reconcile 完成后开放生产执行和定时触发。数据库恢复失败则 Jobs 不 Ready，不能退回旧 scheduler 悄悄执行。

关闭顺序：停止 binding/timer、拒绝新接纳 → drain admission 与已运行 Job → 确认 Profiles 下载/提交已停止 → drain core lifecycle → flush journal/关闭 store → 卸载日志 sink。首版公共 shutdown 等待预算 30 秒；超时返回仍运行的 RunId 和持久化状态，owner 继续持有资源，不能提前关闭其业务依赖。OS 强制终止无法保证收尾，下一次启动报告 Interrupted。这里是待实现的应用级关闭协议，不改变已有核心关闭 API 的含义。

## 5. Profiles 完成边界与接入契约

P4 新增窄 `ProfileRefreshWorkflow`，注入 ProfilesClient、CoreLifecycleClient 和 UI port；handler 不捕获 NyanpasuClient，也不调用再次触发 Job 的 facade。该 workflow 执行：

1. 调用保留 `origin + patch + RunContext` 的内部刷新入口，并等到 CommitRefreshed 的明确结果。
2. 失败时保留已发生的 options patch revision（如有）；不宣称 patch 和下载整体回滚。
3. 成功时保留 Profiles revision、affects_current 和现有 degradation。
4. 如影响当前配置，执行一次现有 post-commit reconcile 等待；否则标记 NotRequired。手动和定时统一执行，删除旧 notifier fallback，避免双重重建。
5. 返回有限 RefreshReport；通过 Jobs 的终态持久化路径完成 Run。

RefreshReport 的 application 字段分为 NotRequired、Reconciled、Degraded、Unknown(operation_id?)。Succeeded 表示订阅提交成功；不受重建失败/超时改写。Reconciled 只表示此次协调返回成功，不表示核心永久固定在该 revision；Unknown 表示无法确认结果，不标为 Applied，不自动重发。当前 180 秒 lifecycle 等待超时语义保留；这与用户的短 wait 超时是两层不同的等待。

完成 Run 时，Profiles 的写入必须已结束。超时后仍在运行的 core 操作由 CoreLifecycleActor 独立序列化和持有；它不是释放 Profile 名额后可自行重跑的副作用。保留 operation ID 供已有状态 API 检查，近期结果被淘汰后仍显示 Unknown，不凭当前核心状态推断旧操作成功。

这是对初步设计“默认报告提交结果”的细化：成功的判定仍以提交为准，但 Run 的终态发布时间包含一次现有 post-commit 协调返回。现有 `refresh_profile` facade 在触发 Job 后等待并映射这个报告，继续保留原有调用方的完成等待语义。

Profiles 快照广播采用 commit 后的 watch/最新值模型，不用 UI 事件。snapshot 带持久化 revision；binding 校验整份目标集合、拒绝旧版本，并公开 applied_revision/last_error。相同业务声明为 no-op；仅物化时间变化不重置 timer。启动补更新只对启动快照判定，之后的刷新提交不能反复触发 catch-up。

## 6. 数据库、日志与有界失败处理

jobs.redb 仅由 RedbJobStore 持有一个数据库实例；应用读写通过 journal/store port，禁止外部按路径再次打开。writer 由服务拥有，阻塞 redb 操作走专用线程/受限 blocking adapter；句柄和线程必须能 drain/join。关闭等待超时也不能假装已关闭数据库。

表按 schema version 隔离：runs、runs_by_job、run_logs、meta。admission_sequence 在创建 Run 的同一事务中递增并写入索引，排序键为 `(JobKey, admission_sequence, RunId)`；日志按 `(RunId, sequence)` 游标。记录删除与所有索引、日志删除在同一事务内，retention 只清理已确认终态。

日志序号在有界采集入口分配，丢弃也计数；flush 是按 Run 的屏障，不是“队列现在为空”。handler 结束后封闭该 Run 的采集，再将屏障前已接受的日志批次与终态有序提交。晚到/未传播上下文的子任务日志不能偷偷写进已封闭 Run；P1/P4 要先保证任务所有权，P5 再实现采集。

初始上限如下，均为工程默认值，不是基于真实用户日志负载的容量结论；P5 压测只能调参，不能删除有界/降级语义。

| 项目                        | 初始值                                                                       |
| --------------------------- | ---------------------------------------------------------------------------- |
| 同 JobKey 活跃 Run          | 1；首版不做手动队列                                                          |
| 全局在途 admission + Run    | 8；后台维护使用独立的受限系统操作，不绕开有界写入                            |
| 日志采集队列                | 1024 条，每条编码后最多 4 KiB；超限截断/丢弃并计数                           |
| 单 Run 日志                 | 1 MiB 或 4096 条，以先到的为准                                               |
| 默认日志级别                | Info；只采集带 RunContext 且经过允许列表的事件                               |
| 输入、序列化输出            | 各 64 KiB；输入超限在接纳前拒绝，输出超限显示 OutputUnavailable              |
| 查询页                      | 默认 50、最大 200；日志按总响应 256 KiB 再截断分页                           |
| 正常历史保留                | 最多 7 天、每 Job 100 次、全局 10000 次、逻辑日志/结果 256 MiB，以先到者为准 |
| 正常完成内存缓存            | 128 次；淘汰后读 journal                                                     |
| writer 批次                 | 最多 128 条或 100 ms；终态屏障立即触发 flush                                 |
| 普通控制 RPC / 终态保存等待 | 各 10 秒；长业务 wait 默认 30 秒，可重复按 RunId 等待                        |

journal 错误或写入超时后关闭新业务接纳，允许 inspect/cancel/drain；定时跳过记录也无法落盘时只增加服务级 dropped_trigger_count，不能无限积攒 Skipped 记录。正在执行的最多 8 次工作继续安全收尾；其失败持久化结果不可在缓存满时静默驱逐。对 journal 做单次在途、幂等的持久化重试，间隔 1/2/4/8/16/30 秒，全部待保存终态确认落盘且 writer 恢复后才可恢复接纳；不重跑 handler。若原 write 仍在阻塞，不能再发第二个阻塞写来堆满线程。

终态写入超过 10 秒也通知 waiter：业务 outcome + journal Degraded；实际 writer 仍由服务拥有。后续写入成功，查询可变为 Durable。原 write 是否成功不明确时，通过 RunId 读回/幂等写入确认，不猜测回滚。服务进程终止前仍未持久化的内存结果无法保证恢复；重启只能报告 journal 中的 Interrupted/Unknown。

retention 限额按逻辑数据计，不承诺 redb 文件物理立即缩小。waiter 在运行期间可持有结果引用；按 ID 的后续查询只保证在保留期内可见，清理后返回 NotFound，不无限保存 tombstone。旧共享库里的任务键不自动迁移或清除，也不作为新 journal 的 fallback。

## 7. Cron/interval 确定语义

固定 croner/chrono/chrono-tz 版本。解析器为 Seconds::Optional、Year::Disallowed；接受五字段或带秒的六字段，时区必须是有效 IANA 名称。禁止通过系统当前时区隐式决定计划；校验必须同时检查 parse 和能否计算下一次日期。未证明与旧 delay_timer 表达式兼容，因此 P7 应重建显式业务计划，而非原样导入旧 cron 字符串。

- Cron 查询下一次时使用 exclusive 起点；UTC instant 用于去重和持久化，墙钟仅用于表达和显示。
- 固定时刻遇春季不存在时间：顺延到跳变后第一个有效 instant；秋季重复时刻只用第一次。已验证的每分钟通配 cron 遍历重复时段的两个 offset，但 UTC instant 严格递增；其他组合的 DST 行为仍须 P3 参数化测试，不能推定所有表达式都等同固定时刻。这是库的已验证日历规则，不是机器休眠补跑。
- Interval 使用单调时钟，注册后完整等待一个周期。手动执行不改变 anchor；变更 schedule 才产生新 generation/anchor。Once 从注册生效时算延迟，额外手动执行不消费它。
- 默认不补跑停机期间的任务。运行中 Due 允许最多 5 秒迟到；超过则跳过，并从当前时间重算下一次未来计划，记录聚合 missed_trigger_count，不为长时间休眠制造无限历史。Once 逾期跳过后标记该计划已消费。
- Tokio Skip 仍会给出一个 overdue tick，因此 P3 必须在 actor admission 再检查 generation、scheduled_at 和迟到窗口。不能只设置 MissedTickBehavior 就认为满足契约。
- Cron 等待至少每 5 秒复核墙钟；时钟回退时不重放已提交 UTC slot，前跳按迟到规则处理。Interval 不受墙钟回拨影响。每次启动重新计算未来计划；Profiles 的“过期最多补一次”是显式 StartupCatchUp 来源。

## 8. 日志与 DTO 的接入约束

文件/终端日志的过滤器改为各自 layer 过滤；journal 使用独立过滤，不能在 registry 外统一过滤掉 Run span。P5 通过早期创建的显式 channel 将 sender 交给 JobJournalLayer、receiver 交给 writer；不加可变静态服务。layer 内只做关联、字段允许列表和快速有界投递，writer 自己的错误不能再次进入自身 Run journal。

自动归属不等于任意原始 tracing message 都安全：首批 Profiles 日志只保留经过审查的目标/字段；URL、认证信息、配置正文以及含这些内容的自由格式错误必须变成安全 code/message，不只靠正则替换。脱敏后的输入摘要与执行用输入分开，默认不保存原始输入。跨 actor/子任务显式传播 RunContext，并在真实处理位置 instrument。

IPC 字段固定为非递归 DTO；通用结果以有类型的领域结果或 `Vec<ResultField { key, value: String }>` 展示，不把任意 serde_json::Value 展开给 Specta。UUID、revision、序号为字符串；时间为 UTC RFC3339 字符串；有限页数/数量可用 u32。Rust 泛型 O 在内核内序列化，typed wait 若无法解码输出，报告 OutputUnavailable/SchemaMismatch，不把成功记录改成 Failed。

## 9. 验证结果和边界

独立 probe 锁定依赖，未接入 backend workspace 或生产构建：

| 验证组         | 已运行的观察                                                                                                                            |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| cron / time    | 五/六字段、非法范围/年字段/时区、无有效日期、纽约 DST gap/overlap、interval 首次延迟、Skip 的一次 overdue tick。                        |
| Tokio 生命周期 | started blocking 工作收到 abort 仍继续；单个 waiter 超时后 owner 继续，第二个和晚到 waiter 均可读结果。                                 |
| redb           | 同文件第二次写打开被拒绝；显式 abort 不留下半条 Run/索引；终态和日志可在一个事务回滚/提交；范围游标按稳定接纳键读取。                   |
| Specta         | rc.25 + serde/typescript 0.0.12 成功导出候选有限 DTO，无 any/bigint；大序号、业务成功 + journal 降级 + application 未知可以序列化往返。 |
| tracing        | registry 的全局 WARN 过滤隐藏 INFO；独立 layer 过滤允许 journal 记录 INFO，同时文件层仍不记录。                                         |

这些验证证明依赖能力和反例，不是新 JobsActor 的正确性测试。尚未模拟真正 ENOSPC、断电、OS suspend、完整 Profiles 刷新或 GUI；事务 abort 不能代替所有磁盘故障。对应故障注入/状态机测试仍是 P1–P7 的验收要求。容量参数尚无生产测量数据，不能把初值当性能结论。

P0 退出条件：上述决策已明确，依赖版本和候选 DTO 可编译，probe 可重复通过，后续阶段的缺口已列明。P1 可以实现核心类型、执行 owner、有限控制协议及 fake store；在 P2/P3/P4 完成前不替换生产调度。
