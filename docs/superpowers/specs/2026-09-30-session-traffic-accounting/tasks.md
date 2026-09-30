# 内核会话级连接历史与流量统计实施计划

**日期：** 2026-09-30

**状态：** 待实施；T0–T10 复选框表示实施交付，不表示文档已经讨论过。

**设计依据：** [design.md](./design.md)。

**范围：** 后端库、宿主接线、服务 IPC、现有实时消费链的必要迁移；不实现历史页面和新统计页面。

## 1. 实施原则与依赖顺序

先完成领域契约和确定性计算，再完成存储与 actor，最后迁移两种宿主和现有消费链。不得先给 `StreamsActor` 增加一套历史数据库，然后再另建 traffic owner。

```text
T0 仓库/协议边界核对
  → T1 领域类型与 ports
  → T2 纯计算与归因
  → T3 redb 与共同契约测试
  → T4 TrafficActor/采样 adapter
  → T5 内核生命周期接线
  → T6 历史查询与拓扑
  → T7 服务 IPC 与宿主组合
  → T8 应用 facade / 旧流迁移
  → T9 故障、性能和平台验收
  → T10 发布集成准备
```

每阶段须完成其测试后再进入依赖它的阶段。按最终职责组织可构建的原子提交；不把已知失败的中间状态提交到开发分支，不追加用于补完前一个提交的 fix-up commit。任务不自动授权发布、推送或合并。

## 2. T0：工作环境与集成边界

- [ ] 在主仓库外创建独立实施 worktree，记录主仓库和 runtime 子模块基线；保留用户未跟踪文件。
- [ ] 初始化该 worktree 的 runtime 子模块，并在子模块自己的隔离分支上工作，不修改其他 worktree 的子模块。
- [ ] 遵循根 AGENTS 复用策略：只链接 sidecar/resources；node_modules 独立安装；target 和 frontend dist 独立。仅文档阶段不需要下载/构建资源。
- [ ] 核对当前 `StreamsActor`、`ApiClient`、本地 `CoreControl`、服务 bridge、widget 和 frontend provider 调用链。
- [ ] 核对进程启动/退出的实际通知点、graceful switch/drain、快速启停；明确如何向 traffic 交付有序的真实 instance 生命周期，而非仅观察合并后的 watch。
- [ ] 核对 runtime/service 版本发布机制、旧服务 capability 和主仓库 sidecar 下载逻辑。

**验收：** 在任务记录中列出实际改动入口与必须迁移的消费者；没有 unresolved 的循环依赖；实现路径遵循 design §3、§11。不把未发布 runtime revision 当成已发布兼容版本。

## 3. T1：领域类型、查询模型与可替换 ports

主要位置：新 `crates/nyanpasu-traffic`，runtime workspace manifest。

- [ ] 定义 SessionId、HostId、instance 身份、source generation、observation sequence 和 committed revision。
- [ ] 定义连接状态/关闭原因、累计字节、可选速率、freshness、coverage、归因区段和质量事件。
- [ ] 定义 session/分钟窗口、有限过滤与 group-by、分页 cursor、usage/topology 结果。
- [ ] 定义 object-safe async `TrafficStore`、`TrafficSource`、Clock；错误类型明确区分未提交和结果不确定。
- [ ] 定义幂等 ObservationCommit/CommitReceipt 与恢复状态，文档写明原子边界。
- [ ] 定义无数据库依赖的 fake store/source/clock 测试支持，放在 cfg(test) 或 test-support 内。
- [ ] redb-store 为可选 feature，领域无 Tauri、nyanpasu-ipc 和 core-manager 反向依赖。
- [ ] 统一 wire 长整数策略，验证序列化不会经过 JS number 损失精度。

**验收：** 无 redb feature 能编译测试；trait 可被注入和 mock；模型能表达缺口/歧义/未知速率，不用零或空集合代替失败。

## 4. T2：纯流量计算与历史投影

主要位置：`accounting.rs`、`topology.rs`，迁入 `connection_rates.rs` 的必要逻辑。

- [ ] 输入显式包含前次已提交 baseline、完整采样、单调时间和可用上下文，输出待提交领域变化。
- [ ] 首次发现连接入账累计值一次，连续采样仅记增量；重复采样不重复累计。
- [ ] 负值/溢出/回退有明确行为；reset 开新区段并标记质量。
- [ ] 仅完整有效快照推导关闭；关闭后累计值不减少，重现 ID 恢复持久化基线。
- [ ] 断线重连补累计并使速率重新建立基线；未知时间分布不制造瞬时峰值。
- [ ] 分开 core-reported 和 attributed 统计，保留差额方向而非 clamp 掩盖。
- [ ] 规范化规则、进程、目标、逻辑路径；保留歧义与原始证据，metadata 更新不重写旧归因。
- [ ] 实现分钟桶及 time-unallocated 规则，明确连接时间过滤与字节时间过滤不同。
- [ ] 实现四层逻辑拓扑投影，去重与字节守恒按 design §6.4，不能累加全部边作为总量。

**验收测试：** 首样本、空列表、双向计数、多个连接同规则、相同 chain 名、时间回退/零间隔、counter reset、断流、新 ID/重现 ID、规则歧义、路径变化、跨分钟边界、超 JS 安全整数。测试纯值，无 actor/sleep。

## 5. T3：redb adapter 与共同存储契约测试

主要位置：`adapters/redb.rs`、store contract tests。

- [ ] 在注入路径打开独立数据库，设置 schema version 和显式页缓存预算。
- [ ] 建立 session、连接、查询索引、归因组合汇总、分钟桶、拓扑及已提交位置表。
- [ ] 一次 ObservationCommit 的所有表变化在同一事务完成；begin/finish 幂等。
- [ ] 实现同序号同摘要重复成功、不同摘要冲突、前序不匹配拒绝。
- [ ] 实现 committed position 与恢复所需 baseline 查询；异常结果可核实，不盲目重放增量。
- [ ] 阻塞 IO 放在 adapter 边界；panic JoinError 继续传播；不创建无界写入队列。
- [ ] 实现 session 清理、flush 和重开；未结束实例与当前 session 不被清理。
- [ ] 明确序列化读写格式，不以 Clash wire Deserialize 意外解释已序列化的 `_extra`。
- [ ] fake 和 redb 共用存储契约测试；redb 增加临时目录 reopen/故障测试。

**验收测试：** 原子性、幂等重试、冲突、部分失败不推进位置、重开后累计不变、session 隔离、索引一致、未知 schema/损坏不删库、重复封存不重复加账、retention 不删活跃 session。

## 6. T4：TrafficActor 与唯一 connections 采样链

主要位置：`actor.rs`、`client.rs`、`adapters/clash.rs`。

- [ ] 使用 ractor typed messages 和 typed client，构造参数显式注入全部依赖。
- [ ] 实现 worker 完整快照 → actor → 原子提交 → baseline 更新 → 发布 → 确认。
- [ ] worker 同时最多一份未确认采样；内部没有额外 scheduler、写队列或优先级。
- [ ] 实现实例/source generation 校验、重连与过期状态；网络 deadline 仅在数据源 adapter。
- [ ] 帧接收时记录时钟，速率不受数据库提交延迟伪造；collector 停止与内核退出状态分别处理。
- [ ] 实现存储失败/不确定结果的有界恢复状态；不能无限占据单个 handler 重试。
- [ ] 实时摘要与详情携带相同 session/revision；详情 DTO 只在有 UI 订阅时构造。
- [ ] 历史记录不依赖 UI watch/broadcast，不受 recording 展示选项影响。
- [ ] query/drop caller 不取消已开始入账；根 token 取消后拒绝新消息，post_stop 清理自身 worker 和存储。
- [ ] 测试多存活实例的 drain 采样归属，避免把“当前 controller 改变”当作旧进程已退出。

**验收测试：** 无 UI 消费仍记录、慢/消失订阅不丢入账、旧 worker 消息被拒、提交失败不发布已入账版本、结果不确定后不重复计数、取消等待者不取消提交、正常/异常退出清理、panic 不降级为普通错误。全部用显式 ack/fake clock 驱动。

## 7. T5：宿主生命周期与配置上下文接线

主要位置：core-manager 实际实例事件出口、本地 composition root、service manager bridge；adapter 留在宿主层。

- [ ] 在实际生命周期点下发实例开始/退出，保留 instance ID；短命且无采样实例也可见。
- [ ] 在启动采集前准备 owner，启动记录失败报告 degraded，不阻止核心控制流程无限等待。
- [ ] controller/secret 更新同 session 重绑，WS 断线仅标 stale。
- [ ] 快速重启、崩溃重启、切核和 drain 正确建立/结束各 session。
- [ ] 从配置拥有者提供去敏归因上下文，只把 watch 视为最新上下文，不伪造完整 revision 时间线。
- [ ] 支持中途附着与宿主恢复，记录 late attach/gap；实例身份未知时不误封存。
- [ ] 本地与服务各自注入目录/权限/host identity；数据库不跨进程同时打开。

**验收：** 生命周期事件无同步环；实例身份不由 PID/epoch/凭据推断；配置热更新不重建 session；UI 生命周期不影响服务端 owner。

## 8. T6：明细、规则、拓扑与统计查询

主要位置：领域查询模型、store 实现、纯拓扑投影、TrafficClient。

- [ ] 连接按状态/规则/进程/目标/出口/路径/协议/开始时间过滤，默认 100、最大 500 的 keyset 分页。
- [ ] cursor 绑定 session、查询形状和首屏高水位；文档和结果明确跨页非冻结快照。
- [ ] usage 返回总量、完整匹配集合累计、group-by/排行和数据质量；截断输出不截断声称的总量。
- [ ] 提供规则 reported key 合计及可用上下文细分，关联连接可下钻，歧义可见。
- [ ] 历史拓扑使用存储的原路径；返回节点/边统计和过滤条件，不返回全部历史 ID 集合。
- [ ] 实现 Live/Session/MinuteWindow 范围，在 actor 内组合同 revision 的实时速率和持久化累计值。
- [ ] 区分活跃数、观测过的唯一连接数、路径成员数，避免不同路径重复计为全局连接数。
- [ ] 分钟趋势单独返回 time-unallocated，不把未知分布算成零流量。
- [ ] 质量/采样时间/revision/retention 随每种查询返回；过宽查询显式报错，不隐藏不完整合计。
- [ ] 大查询使用受限内存和索引/流式扫描，不把所有历史记录加载到 actor 内存。

**验收：** 固定混合数据集可从总览下钻到规则/进程/出口/连接；三种投影与同一账目核对；关闭后、配置变化后结果不丢失；拓扑 top-N 的 other 保持字节量；分页不串 session，retention 后 cursor 可检测失效。

## 9. T7：服务 IPC 与本地/服务查询一致性

主要位置：`nyanpasu_ipc/src/api`、`client`、service-runtime `server/routing` 和 `server/mod.rs`。

- [ ] 添加 traffic protocol capability 和版本化查询/订阅路由，复用既有 IPC 授权边界。
- [ ] service composition root 构造 source/store/actor，root token 管理其生命周期。
- [ ] queries 经 TrafficClient，订阅只传领域结果；credentials/config/database path 不进入 wire。
- [ ] IPC 客户端支持 typed domain 查询，deadline 由该层负责，in-process actor RPC 无 timeout。
- [ ] capability 缺失返回 Unsupported，不回退为第二个桌面采集 owner。
- [ ] 测试桌面退出再附着后仍能查询其离线期间观测到的已关闭连接。
- [ ] 针对同一 fixture 对比本地和服务结果，覆盖字段、质量、分页与大整数序列化。

**验收：** 断开 IPC 不停止采集；慢订阅可重取最新版本；旧服务行为明确；服务数据库仅服务进程访问；不新增通用远程 SQL 或存储访问接口。

## 10. T8：NyanpasuClient 与旧连接流迁移

主要位置：`backend/tauri/src/client`、`core/clash`、commands、widget/setup、必要的 `frontend/interface` provider/bindings。

- [ ] facade 增加设计规定的 session/connection/usage/topology 查询和实时订阅。
- [ ] 本地调用 TrafficClient，服务调用 IPC adapter；调用方不接触 ActorRef 或数据库。
- [ ] 从 StreamsActor 删除已迁移 connections/traffic worker、baseline 和流量历史所有权；logs/memory 保留。
- [ ] connection detail subscriptions 改订阅 TrafficClient，保留按 webview 显式取消的机制。
- [ ] widget、dashboard、proxies 速率与现有 connections/topology 实时数据改接新来源。
- [ ] 更新 provider 对领域 sequence/session/revision 的处理，不用兄弟 actor 转发模拟旧合并序号。
- [ ] recording 和 clear-history 保持展示缓存语义，不清除 session 账目；必要的旧接口直接迁移调用方。
- [ ] 更新导出类型和现有测试；新历史页、规则流量 UI、新统计页面不在本任务中实现。
- [ ] 搜索并清理本次迁移造成的孤儿类型/函数，任何不得不保留的迁移桥写明原因与删除条件。

**验收：** 同一实例只有一个 connections 采集 owner；当前 UI 实时功能和订阅清理不回归；无页面订阅仍记录；facade 可独立调用所有历史 API；未新增 Tauri 业务耦合或 globals。

## 11. T9：故障、性能和跨平台验收

- [ ] 在 fake core/controller 测试完整场景：采样、关闭、断流、重连、reset、配置更新、快速重启和 drain。
- [ ] 故障注入覆盖 redb IO 失败、capacity exhausted、unknown outcome、宿主异常退出后恢复。
- [ ] 长 session 压测：大量已关闭连接和唯一目标，确认历史增长不线性扩大进程内存。
- [ ] 高频采样与分页/排行并行负载下记录 actor 处理延迟，确认没有无界采样/写入积压。
- [ ] 记录 1k/10k 活跃连接、100k/1M 历史连接的数据规模、RSS、DB 大小、提交和查询延迟；用显式数据生成，不依赖实际网络碰巧产生负载。
- [ ] 核对 32 MiB cache 只是页缓存预算，评估索引、元数据、活跃状态和操作系统缓存的额外成本。
- [ ] Windows 验证 named pipe、文件锁、关闭后清理；支持平台验证 HTTP/Unix socket 和同语义查询。
- [ ] 真实 mihomo smoke test 验证字段映射、规则/路径归因及关闭后可查，明确无法验证为“最终精确字节”。
- [ ] 维护性能结果表；如常用查询压住采集，先优化索引/受限查询，不私自加 actor 内第二队列。

**完成门槛：** design §12 所有场景有可重复验证；缺口、磁盘满、未知归因不会被报告为空或零；本地/服务查询一致。性能数据应给出测量环境，不凭空承诺吞吐或固定 RSS。

## 12. T10：发布集成准备与提交检查

- [ ] 按 runtime 仓库规则更新 service/runtime 版本和 capability，记录最低可用服务版本。
- [ ] runtime 功能及发布产物先具备后，再更新主仓库 gitlink/依赖；发布仍需遵守实际会话授权。
- [ ] 验证 `prepare:check` 下载的 sidecar 包含新 IPC，而非只有源码类型更新。
- [ ] 审查 diff：没有新 singleton、raw ActorRef 外泄、共享 actor mutable state、兄弟快照转发或隐藏兼容层。
- [ ] 如实施需要修改架构规则，同时更新 AGENTS.md 与 CLAUDE.md；普通功能实施不修改规则文件。
- [ ] 仅显式 stage 相关路径；review `git status` 与 `git diff --cached --stat`；分别处理子模块提交与主仓库 gitlink。
- [ ] 在本文件记录已执行检查、真实限制和后续 UI 接入点，再标记任务完成。

## 13. 检查命令与执行范围

以下命令是实施后的检查计划，不表示当前文档阶段已运行或已经通过。新增 crate/feature 就绪后执行对应命令；先聚焦测试，再在跨模块接线后跑受影响包与仓库检查。

```powershell
# runtime workspace：领域与存储
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-traffic --no-default-features
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-traffic --all-features
cargo clippy --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-traffic --all-targets --all-features -- -D warnings

# 生命周期和 IPC 接线后
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-core-manager -p nyanpasu-ipc -p nyanpasu-service-runtime
cargo fmt --manifest-path backend/nyanpasu-runtime/Cargo.toml --all -- --check

# 主应用：先准备 AGENTS 要求的独立 frontendDist
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features
cargo clippy --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-targets --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check

# 仅在 provider/bindings 迁移后需要
pnpm lint:ts:interface
pnpm lint:ts:nyanpasu
pnpm test:frontend
pnpm lint:architecture-ledger
```

需要外部内核二进制、平台权限或特定 feature 的 integration test 按实际测试定义单独运行并记录。环境阻碍要明确报告，不能用单元测试通过替代服务模式/平台验收。

## 14. 本次设计产物验收

| 用户需求                        | 设计位置      | 实施与验收            |
| ------------------------------- | ------------- | --------------------- |
| 已关闭连接可查                  | §5、§7、§9    | T2、T3、T6、T9        |
| 规则累计和实时流量              | §6、§9        | T2、T4、T6            |
| 实时与历史拓扑                  | §6.4、§9      | T2、T6、T8            |
| 流量统计板块后端基础            | §6、§9        | T1、T6、T7            |
| 可替换 redb/Turso 存储          | §3、§7        | T1、T3 的共同契约测试 |
| connections WS actor 协作       | §4、§8、§11   | T4、T8                |
| 内核级 session、本地/服务连续性 | §3、§5、§11   | T5、T7、T9            |
| 内存、磁盘与长期运行            | §7.4、§8、§10 | T3、T9                |

- [x] 两份文档范围一致、互相链接正确，源码引用存在。
- [x] 每个用户需求能映射到设计章节、实施任务和验收场景。
- [x] 默认保留策略、采样误差、服务模式 owner 和可替换存储没有互相矛盾的承诺。
- [x] 仅检查本目录 Markdown 格式及 diff；本次不运行 Rust 构建或修改实现代码。
