# nyanpasu-jobs 应用集成

## 调查结果

调查基线：主仓库 `09d9d16f2`，runtime 子模块 `c62514a`。PR 栈在 `b75b20f68`（更新后的 main）上重新验证。现有 jobs crate 已提供持久化、调度、终态恢复、分页查询、日志采集和保留策略；不再沿用 9 月初文档中尚未实现的 P0 状态。

| 位置                               | 现状                                           | 迁移方案                                                                                         |
| ---------------------------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| state/profiles/scheduler.rs        | 每个远程 profile 一个 sleep 循环，启动时补同步 | 每个 profile 一个稳定 JobKey；Interval / Manual；显式 StartupCatchUp                             |
| state/profiles/actor.rs            | 每 5 分钟物化日志恢复                          | profiles scope 的独立 Interval job，等待 actor 实际完成                                          |
| core/tasks                         | delay_timer 只启用每小时事件清理               | 删除旧设施、setup 和依赖；旧 storage.db 表不导入、不自动删除；jobs 内建 retention 替代新历史清理 |
| core/proxies.rs                    | 每 10 秒强制刷新缓存                           | 独立 Interval job，仍由 ProxiesActor 串行读取网络并发布                                          |
| core/updater/mod.rs                | 每 30 秒清理下载记录                           | 独立 Interval job，仍由 UpdaterActor 持有记录并执行清理                                          |
| client/system_proxy/actor.rs       | 动态系统代理守护 interval                      | domain owner 发布守护注册快照；禁用时移除；实际 OS 操作留在 actor                                |
| client/application_workflow/mod.rs | 恢复 interval 和配置收敛 send_after            | 分别 Interval / Once job；状态和 due 判断留在 workflow owner；完成需 RPC 确认                    |
| client/effects/actor.rs            | 250ms Tick 扫描重试 due                        | jobs 提供唤醒；预算、版本和副作用归属仍在 EffectsActor                                           |

保留在边界 owner 内：core/actor_v2 状态 IPC 轮询、WebSocket 重连退避、下载进度采样、文件 watcher debounce、窗口 ready 超时，以及网络/IPC deadline。这些属于连接或资源生命周期，不作为可持久化业务任务。

## 实施与验收

1. composition root 注入共享 JobsClient，独立 jobs.redb。root CancellationToken 触发 owner 关闭，TaskTracker 等待真实完成。
2. profiles 注册由已提交 snapshot 派生；held producer gate 不接纳自动工作。held 状态仍可手动同步；创建、编辑、删除及时 reconcile；不改变手动运行的 interval anchor。
3. 手动与自动同步共用稳定 key。任务完成覆盖下载、验证、提交和现有 runtime 协调；已提交但降级仍标记业务成功，另显示降级结果。调用方离开不取消 owner 的工作。
4. profile 详情显示任务状态、下次调度、分页执行历史和选中 run 的分页日志。只采集审核过的固定阶段文本，不持久化订阅 URL、令牌或配置正文。
5. 默认每个 job 保留最新 10 份终态，沿用 jobs 的 7 天、全局 10000 条、256 MiB 上限；运行中记录不删。应用默认常量可调整，首版不增加设置 UI。
6. 其余业务 timer 逐一改为 jobs 注册与实际完成确认。避免 JobsActor 等待业务 actor：jobs 的 handler 由独立执行 owner 持有。
7. 注入 fake adapters / 临时数据库，验证调度变更、启动补偿、并发 Busy、失败与降级、历史 reopen、保留数量、分页和关闭；执行 Rust 检查、bindings 导出和前端类型检查。

应用设置无限迟到容忍，暂停/休眠后仍执行一次到期唤醒；jobs 合并已错过的 interval，不回放无限历史 ticks。并发碰撞仍由 jobs 的 Busy / Skipped 表达。

上游 jobs 内部控制/持久化 deadline 由 crate 自己管理；应用不增加 RPC deadline。业务等待超时继续等待同一个 RunId，不再次提交或执行。

## 验证记录

- macOS 上 `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features`：981 通过，1 项既有测试忽略。
- Clippy（tauri 全目标、全特性）、Rustfmt、前端类型检查（interface / app / tests）、Oxlint、Prettier 和架构 ledger gate 通过；ledger 的 44 项测试通过。
- 前端 17 个测试文件、101 项测试通过；interface 构建和 `pnpm web:build` 通过。
- 关键覆盖：Interval 调度及增删/类型变化、held gate、启动补偿、同步阶段日志在 WARN 文件过滤下仍持久化、订阅 URL/令牌不进入日志、连续失败保留 10 份、分页及数据库重开恢复、取消调用方不取消下载、同 profile 并发 Busy。
- 浏览器覆盖：实时状态覆盖持久化 admission 状态、超出 JS 安全整数的序号排序、切换历史读取对应日志、失败原因和默认保留数量展示。

OS 副作用通过 fake adapters 验证；尚未进行完整桌面实机运行验证。
