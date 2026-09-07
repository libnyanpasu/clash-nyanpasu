# Jobs 子系统实施拆解（初步）

状态：P0 已完成契约收敛和独立探针验证；P1–P7 尚未实现。日期：2026-09-07。

来源：[分享对话](https://chatgpt.com/share/6a9e71aa-4344-83e9-b3ff-041d5ee7bca4)。基于主线 `f30d5221` 的 P0 复核及 [总体设计](../design/jobs-subsystem.md)，目标是替换旧 TaskManager、统一 Profiles 刷新触发，并提供按 Run 查询的结果和日志入口。

以下人日是单名熟悉代码的开发者的初估，含实现和对应自动化测试，不含评审等待、发布等待及跨平台环境准备；不是排期承诺。未知项主要集中在取消、跨 actor 完成语义和 journal 故障恢复，不应按“替换 timer 的几百行代码”估算。

## 1. 阶段和依赖

| 阶段 | 独立交付内容                                          | 依赖                         | 初估人日 |
| ---- | ----------------------------------------------------- | ---------------------------- | -------- |
| P0   | 运行契约、存储/取消/Profiles 完成边界定稿；针对性验证 | 无                           | 1–2      |
| P1   | 通用 crate、Job/Run 模型、无宏定义、手动执行和等待    | P0                           | 3–5      |
| P2   | redb journal、索引、终态一致性、恢复和 retention      | P1                           | 3–5      |
| P3   | Tokio 调度、cron、scope reconcile、代次隔离           | P1；持久化接纳接 P2          | 3–4      |
| P4   | Profiles 快照绑定、刷新业务接口及日志上下文传播       | P2、P3                       | 4–6      |
| P5   | tracing 日志采集、游标查询和应用 IPC                  | P2；Profiles 集成验收依赖 P4 | 3–5      |
| P6   | 前端任务列表、历史及详情、手动执行/取消               | P5；真实刷新验收依赖 P4      | 2–4      |
| P7   | 维护能力切换、移除旧框架和依赖、完整回归              | P4、P5、P6                   | 2–3      |

合计约 **21–34 人日**。P2 与 P3、P4 与 P5 的部分实现可以在契约稳定后独立推进，但不能因此省略集成验证。每阶段可以拆成多个可构建 PR；不把未完成的整套替换压进一个大 PR。

关键路径：P0 → P1 → P2/P3 → P4/P5 → P6 → P7。第一条真实业务验收链是“手动刷新 Profile → 得到 RunId → 等待准确结果 → 查到业务 actor 的日志”。

## 2. 各阶段任务与验收

### P0：冻结边界（已完成）

交付：[实施契约与验证结论](../design/jobs-p0-contract.md)、[独立验证工程](../probes/jobs-p0/README.md)。13 项测试覆盖 cron/DST、Tokio 生命周期、redb 事务和游标、Specta DTO、日志过滤；这些是依赖能力验证，生产状态机及故障注入仍由后续阶段实现。

- 复核主线的 tasks、Profiles、storage、logger/bootstrap，以及 `refresh_profile → after_commit` 调用链；记录基线提交。
- 确定 crate 放置、表/数据库所有权、composition root 启动及关闭顺序。journal 通过窄存储端口注入，不依赖 Tauri Storage 包装。
- 写出 Run 状态转换表和 admission/completion/cancellation/journal-failure 契约；列出不同业务的取消能力。
- 明确 Run 刷新完成与 runtime 应用完成的区别、旧历史处置、日志/结果限制、可查询期限。
- 验证 croner 的必要语法、时区与 DST；验证 DTO 可由当前 Specta 导出。固定依赖版本，不照搬分享中的示例签名。

验收：后续实现无需自行猜测取消、等待、刷新成功或磁盘失败分别代表什么。选型验证若不满足需求，在这里调整方案。

### P1：先建立执行模型

- 增加 JobKey、RunId、Trigger、RunState、RunCompletion、定义版本与 JobSnapshot。
- 泛型函数/闭包注册，内部类型擦除；提供 `()` 输入和受限 blocking 适配。
- JobsActor 串行接纳；执行在受管理任务中；run_now 返回本次 Run 的句柄。
- wait 支持多等待者及完成后查询；调用者丢弃/超时不取消业务；删除 Job 不妨碍已有 Run 收尾。
- 同 Job 默认 Forbid；定义 panic、取消请求与真实完成的处理，名额不提前释放。

验收：手动并发触发不串结果；多个等待者看到同一结果；运行中删除/重建同 JobKey 不污染旧 Run；panic/取消路径不会泄漏名额。使用 fake handler 和显式通知，不用真实 sleep 猜完成时序。

此阶段只用注入的测试 store 验证执行内核，不切换生产 caller；“内存执行成功”不作为持久化 Jobs 的交付完成。

### P2：持久化和故障边界

- 实现 RedbJobStore、独立表和稳定游标索引；JobJournalClient 管理有界 writer，隔离阻塞 I/O。
- 接纳持久化完成后才运行；结果、日志 flush 与终态持久化完成后正常唤醒 wait。
- 区分业务 outcome 和 journal 降级；磁盘错误不能触发业务重试，也不能让等待者永久挂起。
- 启动时处理遗留非终态 Run；实现只清理终态的 retention 和单 Run 日志上限。

验收：注入接纳/日志/终态事务失败；接纳失败零副作用；业务成功但终态失败仍返回正确 outcome；重启后标记 Interrupted；删记录无悬空索引，分页不重不漏且不清理活跃 Run。

### P3：调度和配置同步

- Manual、Once、Interval、Cron 统一进入与手动执行相同的 admission。
- 每个 timer 由 actor 生命周期管理；Due 带注册代次。修改、移除、同键重建、shutdown 均拒收旧 Due。
- scope reconcile 整份校验、增量更新、相同定义 no-op、拒绝旧 revision；同步失败可 inspect。
- next_run_at、固定频率、错过触发和时区语义按 P0 契约实现；避免追赶形成无限队列。

验收：虚拟时间测试零 interval、首次等待、手动不移动计划、忙碌跳过、改删后旧触发、相同配置不重置计时、休眠补偿及 DST；一个 scope 的同步不能删除另一个 scope 的任务。

### P4：统一订阅刷新

- 添加提交后版本化 Profiles 快照订阅及受管理绑定器，启动同步与失败重试都取最新快照。
- 纯函数派生 Job；覆盖全部 Remote source、零间隔、改名、删改类型及启动过期补一次。
- 新增内部业务刷新入口，传播 origin/RunContext，保留 options patch、pending_refresh 和过期提交检查。
- 整理 facade 的 after_commit 协调，返回小型 RefreshReport 并保留 degradation；handler 不捕获 NyanpasuClient。
- 手动刷新等待绑定到目标 revision；自动和手动都走 Jobs。切换时关闭旧 RemoteUpdateScheduler，保留 ExternalWatchers。

验收：新建后立即刷新、刷新中改 URL/删除/更新 options、间隔变更、自动与手动同时触发、失败重试最新配置、应用启动补刷新；确认只有一条自动刷新路径，结果仍对应实际业务提交。

### P5：让日志和查询可用

- 安装 JobJournalLayer；propagate RunContext 到 ProfilesActor、下载子任务、提交处理和必要的副作用边界。
- 有界采集、批量写入、丢弃计数、脱敏及 flush；journal 内部日志避免再次进入自身采集。
- facade/薄 Tauri commands 暴露任务 inspect、运行触发、历史、单次结果、日志游标、有限等待和取消。
- 查询 DTO 与运行时结构分离，生成 TypeScript；限制手动可执行任务和输入形状。

验收：从 RunId 查到下载、校验、提交日志；日志洪峰可见截断；令牌/正文不会意外落盘；wait 成功后查询可见结果和已接收日志；终态写入失败可见降级；无 UI 也能用 Rust 客户端完整调用。

### P6：前端入口

- 任务列表显示来源、managed_by、schedule、next run、active runs 和同步错误。
- 历史列表用稳定游标；详情展示业务 outcome、journal 状态、日志和截断标记。
- 手动执行拿 RunId 后展示该次运行，支持重载后继续查看；取消按钮依据实际能力和状态呈现。
- 首版增量轮询；切换 Run、取消组件订阅和迟到请求不得覆盖当前详情。派生调度跳转原 Profile 配置修改。

验收：成功、失败、忙碌、取消请求未完成、Interrupted、journal 降级和日志丢失均有可理解展示；分页/切换/重载测试通过，生成绑定、类型检查和生产构建通过。

### P7：切换与删除

- 将旧运行历史清理交给 journal，移除 EventsRotateJob 的私有存储访问；不默认开启未注册的 ClearLogsJob。
- 若选一个维护 Job 验证其他模块接入，先明确原有产品行为和注入依赖，不以接入框架为由扩大删除文件的范围。
- 清点并删除已无 caller 的 TaskManager、JobsManager、旧 Executor、wrap_job/check_task_input/params_validated_failed 控制流宏及 delay_timer 依赖。
- 移除旧 Tauri managed state 与初始化；按 P0 决策处置旧任务表，不能误删 WebStorage 共享数据。
- 补齐架构 ledger、文档和三平台验证；关闭时不遗留 timer、worker、waiter 或 writer。

验收：静态搜索确认旧执行/调度入口及依赖清零；全量回归通过；至少 Profiles 与 journal 自维护边界完成；不会同时运行新旧订阅调度器。

## 3. PR 交付约束

- Runtime 和应用改动分仓库交付，应用 pin 到对应提交；可复用 crate 的运行测试不能依赖 Tauri bootstrap。
- P1–P3 未达到生产切换条件前不接管已有任务。P4 用一个完整 caller 切换完成 Profiles 的调度与手动刷新，不留下隐蔽 fallback。
- 若分阶段发布迫使新旧子系统暂存，明确不同业务的唯一 owner；同一 Profile 不允许双调度。任何桥接都按 AGENTS.md 标注原因、移除条件和对应阶段。
- 各 PR 只提交本阶段相关文件；feature 实现在独立 worktree；验证不使用应用真实数据目录。架构规则若变化，同步 AGENTS.md 和 CLAUDE.md。

## 4. P0 决策与后续验证

| 问题              | P0 决策                                                                        | 后续验收             |
| ----------------- | ------------------------------------------------------------------------------ | -------------------- |
| 通用 crate 归属   | runtime 的 `nyanpasu-jobs`，应用保留业务绑定                                   | P1                   |
| redb 文件所有权   | RedbJobStore 独占 `jobs.redb`；composition root 注入路径                       | P2                   |
| 取消与关闭预算    | 默认 Unsupported；协作取消需业务确认；关闭等待 30 秒后报告未完成，不释放其依赖 | P1/P4/P7             |
| Profiles 完成边界 | 成功以提交为准；终态包括一次 post-commit 协调等待，核心状态未知单独报告        | P4                   |
| 终态持久化失败    | 保留 outcome，报告 journal 降级、关闭新接纳，仅重试持久化                      | P2                   |
| 旧任务历史        | 保留旧表，不导入或自动删除；不启用 ClearLogsJob                                | P7                   |
| 限额和日志过滤    | 初始上限已在契约列出；journal 独立 layer 过滤，允许列表和有界采集              | P2/P5 压测及故障注入 |
| 首版实时推送      | 后置；先保证游标查询、日志可见和重载恢复                                       | P5/P6                |

下一步进入 P1：实现核心类型、执行 owner、有限控制协议及 fake store；在 P2–P4 达到切换条件前，不接管现有生产任务。P0 未覆盖真实 ENOSPC、断电、OS suspend、完整 Profiles 刷新和 GUI，这些不能以探针通过代替后续验收。
