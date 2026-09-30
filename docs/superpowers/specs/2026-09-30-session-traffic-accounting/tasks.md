# 内核会话级连接历史与流量统计实施计划

**日期：** 2026-09-30

**状态：** 后端实施和 leader review 已完成；未执行的平台/故障注入与发布集成保留为未勾选项。T0–T10 复选框表示已验证的实施交付，不表示文档已经讨论过。

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

- [x] 在主仓库外创建独立实施 worktree，记录主仓库和 runtime 子模块基线；保留用户未跟踪文件。
- [x] 初始化该 worktree 的 runtime 子模块，并在子模块自己的隔离分支上工作，不修改其他 worktree 的子模块。
- [x] 遵循根 AGENTS 复用策略：只链接 sidecar/resources；node_modules 独立安装；target 和 frontend dist 独立。仅文档阶段不需要下载/构建资源。
- [x] 核对当前 `StreamsActor`、`ApiClient`、本地 `CoreControl`、服务 bridge、widget 和 frontend provider 调用链。
- [x] 核对进程启动/退出的实际通知点、graceful switch/drain、快速启停；明确如何向 traffic 交付有序的真实 instance 生命周期，而非仅观察合并后的 watch。
- [x] 核对 runtime/service 版本发布机制、旧服务 capability 和主仓库 sidecar 下载逻辑。

**验收：** 在任务记录中列出实际改动入口与必须迁移的消费者；没有 unresolved 的循环依赖；实现路径遵循 design §3、§11。不把未发布 runtime revision 当成已发布兼容版本。

## 3. T1：领域类型、查询模型与可替换 ports

主要位置：新 `crates/nyanpasu-traffic`，runtime workspace manifest。

- [x] 定义 SessionId、HostId、instance 身份、source generation、observation sequence 和 committed revision。
- [x] 定义连接状态/关闭原因、累计字节、可选速率、freshness、归因区段和质量事件；coverage 由 session 采样时间与质量标记表达。
- [x] 定义 session/分钟窗口、有限过滤与 group-by、分页 cursor、usage/topology 结果。
- [x] 定义 object-safe async `TrafficStore`、`TrafficSource`、Clock；错误类型明确区分未提交和结果不确定。
- [x] 定义幂等 ObservationCommit/CommitReceipt 与恢复状态，文档写明原子边界。
- [x] 定义无数据库依赖的 fake store/source/clock 测试支持，放在 cfg(test) 或 test-support 内。
- [x] redb-store 为可选 feature，领域无 Tauri、nyanpasu-ipc 和 core-manager 反向依赖。
- [x] 统一 wire 长整数策略，验证序列化不会经过 JS number 损失精度。

**验收：** 无 redb feature 能编译测试；trait 可被注入和 mock；模型能表达缺口/歧义/未知速率，不用零或空集合代替失败。

## 4. T2：纯流量计算与历史投影

主要位置：`accounting.rs`、`topology.rs`，迁入 `connection_rates.rs` 的必要逻辑。

- [x] 输入显式包含前次已提交 baseline、完整采样、单调时间和可用上下文，输出待提交领域变化。
- [x] 首次发现连接入账累计值一次，连续采样仅记增量；重复采样不重复累计。
- [x] 负值/溢出/回退有明确行为；reset 开新区段并标记质量。
- [x] 仅完整有效快照推导关闭；关闭后累计值不减少，重现 ID 恢复持久化基线。
- [x] 断线重连补累计并使速率重新建立基线；未知时间分布不制造瞬时峰值。
- [x] 分开 core-reported 和 attributed 统计，保留差额方向而非 clamp 掩盖。
- [x] 规范化规则、进程、目标、逻辑路径；保留歧义与原始证据，metadata 更新不重写旧归因。
- [x] 实现分钟桶及 time-unallocated 规则，明确连接时间过滤与字节时间过滤不同。
- [x] 实现四层逻辑拓扑投影，去重与字节守恒按 design §6.4，不能累加全部边作为总量。

**验收测试：** 首样本、空列表、双向计数、多个连接同规则、相同 chain 名、时间回退/零间隔、counter reset、断流、新 ID/重现 ID、规则歧义、路径变化、跨分钟边界、超 JS 安全整数。测试纯值，无 actor/sleep。

## 5. T3：redb adapter 与共同存储契约测试

主要位置：`adapters/redb.rs`、store contract tests。

- [x] 在注入路径打开独立数据库，设置 schema version 和显式页缓存预算。
- [x] 建立 session、连接、查询索引、归因组合汇总、分钟桶、拓扑及已提交位置表。
- [x] 一次 ObservationCommit 的所有表变化在同一事务完成；begin/finish 幂等。
- [x] 实现最新序号同内容重复成功、不同内容冲突、过旧序号/前序不匹配拒绝；回执存储不随空闲采样次数增长。
- [x] 实现 committed position 与恢复所需 baseline 查询；异常结果可核实，不盲目重放增量。
- [x] 阻塞 IO 放在 adapter 边界；panic JoinError 继续传播；不创建无界写入队列。
- [x] 实现 session 清理、flush 和重开；未结束实例与当前 session 不被清理。
- [x] 明确序列化读写格式，不以 Clash wire Deserialize 意外解释已序列化的 `_extra`。
- [x] fake 和 redb 共用存储契约测试；redb 增加临时目录 reopen/故障测试。

**验收测试：** 原子性、幂等重试、冲突、部分失败不推进位置、重开后累计不变、session 隔离、索引一致、未知 schema/损坏不删库、重复封存不重复加账、retention 不删活跃 session。

## 6. T4：TrafficActor 与唯一 connections 采样链

主要位置：`actor.rs`、`client.rs`、`adapters/clash.rs`。

- [x] 使用 ractor typed messages 和 typed client，构造参数显式注入全部依赖。
- [x] 实现 worker 完整快照 → actor → 原子提交 → baseline 更新 → 发布 → 确认。
- [x] worker 同时最多一份未确认采样；内部没有额外 scheduler、写队列或优先级。
- [x] 实现实例/source generation 校验、重连与过期状态；网络 deadline 仅在数据源 adapter。
- [x] 帧接收时记录时钟，速率不受数据库提交延迟伪造；collector 停止与内核退出状态分别处理。
- [x] 实现存储失败/不确定结果的有界恢复状态；不能无限占据单个 handler 重试。
- [x] 实时摘要与详情携带相同 session/revision；详情 DTO 只在有 UI 订阅时构造。
- [x] 历史记录不依赖 UI watch/broadcast，不受 recording 展示选项影响。
- [x] query/drop caller 不取消已开始入账；根 token 取消后拒绝新消息，post_stop 清理自身 worker 和存储。
- [x] 测试多存活实例的 drain 采样归属，避免把“当前 controller 改变”当作旧进程已退出。

**验收测试：** 无 UI 消费仍记录、慢/消失订阅不丢入账、旧 worker 消息被拒、提交失败不发布已入账版本、结果不确定后不重复计数、取消等待者不取消提交、正常/异常退出清理、panic 不降级为普通错误。全部用显式 ack/fake clock 驱动。

## 7. T5：宿主生命周期与配置上下文接线

主要位置：core-manager 实际实例事件出口、本地 composition root、service manager bridge；adapter 留在宿主层。

- [x] 在实际生命周期点下发实例开始/退出，保留 instance ID；短命且无采样实例也可见。
- [x] 在启动采集前准备 owner，启动记录失败报告 degraded，不阻止核心控制流程无限等待。
- [x] domain 支持同实例 controller 重绑，WS 断线仅标 stale；当前 core-manager 将 controller/secret 变化分类为 Switch，因此宿主实际建立新实例/session，不伪装成同实例热更新。
- [x] 快速重启、崩溃重启、切核和 drain 正确建立/结束各 session。
- [x] 从配置拥有者提供去敏归因上下文，只把 watch 视为最新上下文，不伪造完整 revision 时间线。
- [x] 支持中途附着与宿主恢复，记录 late attach/gap；实例身份未知时不误封存。
- [x] 本地与服务各自注入目录/权限/host identity；数据库不跨进程同时打开。

**验收：** 生命周期事件无同步环；实例身份不由 PID/epoch/凭据推断；配置热更新不重建 session；UI 生命周期不影响服务端 owner。

## 8. T6：明细、规则、拓扑与统计查询

主要位置：领域查询模型、store 实现、纯拓扑投影、TrafficClient。

- [x] 连接按状态/规则/进程/目标/出口/路径/协议/开始时间过滤，默认 100、最大 500 的 keyset 分页。
- [x] cursor 绑定 session、查询形状和首屏高水位；文档和结果明确跨页非冻结快照。
- [x] usage 返回总量、完整匹配集合累计、group-by/排行和数据质量；截断输出不截断声称的总量。
- [x] 提供规则 reported key 合计及可用上下文细分，关联连接可下钻，歧义可见。
- [x] 历史拓扑使用存储的原路径；返回节点/边统计和过滤条件，不返回全部历史 ID 集合。
- [x] 实现 Live/Session/MinuteWindow 范围，在 actor 内组合同 revision 的实时速率和持久化累计值。
- [x] 区分活跃数、观测过的唯一连接数、路径成员数，避免不同路径重复计为全局连接数。
- [x] 分钟趋势单独返回 time-unallocated，不把未知分布算成零流量。
- [x] 查询携带 session 质量/采样时间/revision；固定整 session 保留语义见 design §9，过宽查询显式报错，不隐藏不完整合计。
- [x] 大查询使用受限内存和索引/流式扫描，不把所有历史记录加载到 actor 内存。

**验收：** 固定混合数据集可从总览下钻到规则/进程/出口/连接；三种投影与同一账目核对；关闭后、配置变化后结果不丢失；拓扑 top-N 的 other 保持字节量；分页不串 session，retention 后 cursor 可检测失效。

## 9. T7：服务 IPC 与本地/服务查询一致性

主要位置：`nyanpasu_ipc/src/api`、`client`、service-runtime `server/routing` 和 `server/mod.rs`。

- [x] 添加 traffic protocol capability 和版本化查询/订阅路由，复用既有 IPC 授权边界。
- [x] service composition root 构造 source/store/actor，root token 管理其生命周期。
- [x] queries 经 TrafficClient，订阅只传领域结果；credentials/config/database path 不进入 wire。
- [x] IPC 客户端支持 typed domain 查询，deadline 由该层负责，in-process actor RPC 无 timeout。
- [x] capability 缺失返回 Unsupported，不回退为第二个桌面采集 owner。
- [x] 测试桌面退出再附着后仍能查询其离线期间观测到的已关闭连接。
- [x] 用共同领域 fixture 和真实 named pipe roundtrip 验证本地/服务字段与大整数；分页和质量细节由领域/store 测试覆盖，尚未把全部组合重复跑过 IPC。

**验收：** 断开 IPC 不停止采集；慢订阅可重取最新版本；旧服务行为明确；服务数据库仅服务进程访问；不新增通用远程 SQL 或存储访问接口。

## 10. T8：NyanpasuClient 与旧连接流迁移

主要位置：`backend/tauri/src/client`、`core/clash`、commands、widget/setup、必要的 `frontend/interface` provider/bindings。

- [x] facade 增加设计规定的 session/connection/usage/topology 查询和实时订阅，并能发现当前或最近保留的 session。
- [x] 本地调用 TrafficClient，服务调用 IPC adapter；调用方不接触 ActorRef 或数据库。
- [x] 从 StreamsActor 删除已迁移 connections/traffic worker、baseline 和流量历史所有权；logs/memory 保留。
- [x] connection detail subscriptions 改订阅 TrafficClient，保留按 webview 显式取消的机制。
- [x] widget、dashboard、proxies 速率与现有 connections/topology 实时数据改接新来源。
- [x] 更新 provider 对领域 sequence/session/revision 的处理，不用兄弟 actor 转发模拟旧合并序号。
- [x] recording 和 clear-history 保持展示缓存语义，不清除 session 账目；必要的旧接口直接迁移调用方。
- [x] 更新导出类型和现有测试；新历史页、规则流量 UI、新统计页面不在本任务中实现。
- [x] 搜索并清理本次迁移造成的孤儿类型/函数，任何不得不保留的迁移桥写明原因与删除条件。

**验收：** 同一实例只有一个 connections 采集 owner；当前 UI 实时功能和订阅清理不回归；无页面订阅仍记录；facade 可独立调用所有历史 API；未新增 Tauri 业务耦合或 globals。

## 11. T9：故障、性能和跨平台验收

- [x] 在领域/actor fake 和 manager 生命周期测试中覆盖采样、关闭、断流、重连、reset、配置更新、快速重启和 drain；实际 controller 的端到端验证范围见 §15。
- [x] fake 注入 capacity exhausted、unknown outcome、未提交错误，并验证宿主恢复；真实 redb 的 reopen/schema/损坏测试与 OS 级 IO 故障注入分开记录。
- [ ] 真实 redb 运行中的磁盘满、写入中断等操作系统级故障注入。
- [x] 长 session 压测：大量已关闭连接和唯一目标，记录 100k/1M 历史下的采样 RSS；本组未表现为随历史条数线性增长，不宣称峰值内存上限。
- [x] 无等待连续提交与分页/排行并行负载下记录 actor 处理延迟，核对每个 live instance 至多一个未确认采样批次；未测真实网络固定频率的端到端吞吐。
- [x] 记录 1k/10k 活跃连接、100k/1M 历史连接的数据规模、RSS、DB 大小、提交和查询延迟；用显式数据生成，不依赖实际网络碰巧产生负载。
- [x] 核对 32 MiB cache 只是页缓存预算，单独报告活跃状态和进程 RSS；操作系统文件缓存不计入此预算。
- [x] Windows 验证 named pipe、redb 关闭后重新打开、真实 Mihomo HTTP 与关闭后清理。
- [ ] Linux/macOS、Unix socket 和跨平台同语义查询验收。
- [x] 真实 mihomo smoke test 验证字段映射、规则/路径归因及关闭后可查，明确无法验证为“最终精确字节”。
- [x] 维护性能结果表；如常用查询压住采集，先优化索引/受限查询，不私自加 actor 内第二队列。

**完成门槛：** design §12 所有场景有可重复验证；缺口、磁盘满、未知归因不会被报告为空或零；本地/服务查询一致。性能数据应给出测量环境，不凭空承诺吞吐或固定 RSS。

## 12. T10：发布集成准备与提交检查

- [ ] 按 runtime 仓库规则更新 service/runtime 版本和 capability，记录最低可用服务版本。
- [x] Draft PR 的主仓库 gitlink 引用 runtime 实现提交；两个仓库独立提交，按用户后续授权推送供审查。
- [ ] runtime 发布产物具备后，验证正式发行的 gitlink/依赖组合；创建 Draft PR 不代表发布授权。
- [ ] 验证 `prepare:check` 下载的 sidecar 包含新 IPC，而非只有源码类型更新。
- [x] 审查 diff：没有新 singleton、raw ActorRef 外泄、共享 actor mutable state、兄弟快照转发或隐藏兼容层。
- [x] 本次没有修改架构规则；AGENTS.md 与 CLAUDE.md 均未改动。
- [x] 仅显式 stage 相关路径；review `git status` 与 `git diff --cached --stat`；分别处理子模块提交与主仓库 gitlink。
- [x] 在本文件记录已执行检查、真实限制和后续 UI 接入点，区分后端实施完成与发布/跨平台验收。

## 13. 检查命令与执行范围

以下是复现检查的命令；已执行结果见 §15。先聚焦测试，再在跨模块接线后跑受影响包与仓库检查。

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
- [x] 设计阶段仅检查本目录 Markdown 格式及 diff；该阶段未运行 Rust 构建或修改实现代码。

## 15. 实施记录

- 设计提交：`e509b5a05`（`docs: plan session traffic accounting infrastructure`）。
- 实施 worktree：`G:/Programs/Rust/.ccg/clash-nyanpasu/session-traffic-accounting`；主仓库和 runtime 子模块均使用独立的 `feat/session-traffic-accounting` 分支。
- runtime 基线：`c62514a63b4b665f9800ec7e446fa7207d70d707`；嵌套 utils 基线：`cd6c9d3821a8c943bc249d96d456e2bedffd3ada`。
- 已完成独立依赖安装、interface 构建和 Rust 检查所需的 frontendDist 占位；没有共享 target、node_modules 或前端构建输出。
- 实际接线入口：core-manager `instance.rs` 的真实 `Started/Exited` 事件、local host composition、service runtime composition、typed endpoint/facade、Tauri streams/detail adapters、widget 与 frontend provider。
- 宿主流向：内核生命周期/配置拥有者 → TrafficActor；查询与订阅由应用边界直接调用 typed traffic client。StreamsActor 不成为 traffic 的中转拥有者。
- 发布边界：`scripts/check.ts` 依据 runtime 中 service manifest 的版本下载已发布 sidecar。源码按用户后续指示提交并推送 Draft PR，不把源码实现当成可下载的服务发行版；T10 的版本与正式发行组合仍须在实际发行后验证。
- 提交状态：设计文档提交为 `e509b5a05`；runtime 实现提交为 `4218c60`，见 [runtime Draft PR #433](https://github.com/libnyanpasu/nyanpasu-runtime/pull/433)。主仓库接线、验收记录和对应 gitlink 一并提交供 Draft PR 审查。
- 集成边界：验证基线之后，两个仓库的 `main` 新增 native storage 接线。Draft PR 保留已验证实现；与这些上游修改的冲突协调及集成验证尚未完成，不代表可直接合并。
- 验证环境：Windows x64，Rust `1.101.0-nightly (c1070d693 2026-09-28)`；可用 smoke 内核为 Mihomo Meta `v1.19.30`。最终检查与实测结果如下。

### 已验证的集成边界

- redb 基线（加入后续 Turso 对照前）：`nyanpasu-traffic` 无默认 feature 测试 21 项通过；all-features 31 项通过、性能测试默认忽略 1 项；all-targets/all-features Clippy `-D warnings` 通过。
- 主程序 `cargo test -p clash-nyanpasu --all-features`：994 通过、1 忽略；包含真实 TypeScript bindings 导出及字段断言。interface 与 nyanpasu TypeScript 检查、architecture ledger gate 通过。
- 主程序 all-targets/all-features Clippy 通过，仍报告仓库警告及尚未接 UI 的 `traffic_status` 方法未使用；没有将带警告的主程序检查描述为 `-D warnings` 通过。主 workspace 与 runtime workspace Rust 格式、两份 Markdown 格式、两仓库 diff whitespace 检查通过。
- 前端最终全量测试：19 个文件、108 项通过；包含 summary Channel 浏览器回归，覆盖同 revision 的 stale 更新、旧 revision 拒绝、session 变化、None 清缓存与取消订阅。
- core-manager 单元测试：105 通过、1 忽略。完整并行集成运行中两个 config_apply 时序测试失败，随后该测试文件串行重跑 20/20 通过；不把一次并行失败描述成已证明的既有问题。
- IPC：11 项单元、14 项 Windows transport roundtrip、32 项 wire golden 通过；补充真实 named pipe 的 traffic session/usage/summary 及超过 JS 安全整数的 roundtrip 通过。
- service-runtime：109 通过、1 忽略；自定义 runtime backend 与 lifecycle sink 的不兼容组合显式拒绝测试通过。
- 真实 Mihomo smoke：无桌面订阅时连接关闭后仍可查；根取消后等真实 Exited 入队再 drain，重开数据库保留同 session 的已结束状态与累计值。2 项通过，不宣称关闭连接的最后字节精确可知。
- controller/secret 更新在当前 manager 中属于 Switch（`config/mihomo.rs`），实例的 `ResolvedController` 不原地改变；新真实 Started 绑定新 session，旧 drain 实例继续使用自己的 controller。同实例重绑能力留在 typed domain port。

### Leader review 修正

- 退出时先登记确切的待封存事件，再重试未知提交；避免 IO 故障吞掉真实 Exited。
- 当前错误按实例恢复，一个实例恢复不能掩盖另一个实例故障；历史 quality 证据仍保留。
- 根取消只拒绝新业务工作，已拥有实例的真实 Exited 作为清理交付；宿主等内核终态后 drain traffic mailbox。
- 修正拓扑节点的实时速率初始化，已知速率能相加，任一未知贡献使合计仍未知。
- Live 和 active-only 历史聚合走活跃状态索引，避免扫描大量已关闭连接；4,199 closed + 1 active 的回归通过。
- 服务协议 capability 与数据库健康分离；已支持协议的服务仍返回真实存储错误，不误报为旧服务 Unsupported。
- 摘要和详情按 webview Channel 交付，退休源明确清空展示；widget 不保留失效速率。新增当前/最近 session 命令，修正导出类型断言。
- 性能回归发现规则过滤后按 Rule 分组仍扫描 facts，改为直接读取持久化规则汇总；与强制扫描路径比较 context 过滤、top-N、other、total 和未分配字节，等价测试通过。
- 高基数目标排行改为每个 session、每种分组最多 500 项的持久化索引；完整分组累计仍保留，`other` 精确扣除，更新同事务完成。新增并列排序、未入榜项晋升、reopen、prune 与守恒回归。schema 2 显式拒绝不兼容旧库。

### 性能测量方法

本节记录后端首次验收的 redb 基线：当时 harness 使用单线程 Tokio、系统临时目录（C:）。随后用户追加了独立 Sol agent 实施双 store 对照，当前 harness 已扩展；后续方法和数据见 [store-benchmark.md](./store-benchmark.md)，两轮不跨磁盘或 runtime 配置计算性能提升。

测试入口为 runtime `crates/nyanpasu-traffic/tests/performance.rs` 的 opt-in ignored 测试，使用生产 release profile（`opt-level="s"`、LTO、单 codegen unit）。每组独立进程、独立临时数据库、32 MiB redb 页缓存，计时期间没有本任务的并行编译。硬件为 i9-14900KF、约 63.8 GiB RAM、Windows x64；用户原有桌面开发进程保持运行，测量环境不是独占基准机。

- 活跃组包含 1k/10k 连接，首次入账后连续提交 20 帧，分别报告首帧和稳定帧耗时。
- 历史组以每批 1,000 连接的创建快照和空关闭快照累计到 100k/1M，每个连接有唯一目标；同一逻辑代理链、主要为 Match 规则，每 10k 条有一条稀有 DomainSuffix 规则。fixture 只包含必要 metadata，不代表全部真实连接字段的大小。
- 耗时包含 actor 入队、计算、redb Immediate 事务提交和回执，不包含真实 WS 解析或按秒等待。查询在入账后的同一进程中测量，不是冷启动磁盘读取测试。
- RSS 为进程结束入账时及查询后的采样值，不是连续记录的峰值；数据库大小是文件逻辑长度。操作系统文件缓存和其他进程内存不计入 RSS。
- 单独让每种查询先入 actor mailbox，再提交一份完整观察，记录查询耗时及排在后面的观察总延迟；没有添加第二队列、优先级或超时来掩盖阻塞。
- 最终 schema 2 原始 JSON 保存在 worktree 外的 `G:/Programs/Rust/.ccg/clash-nyanpasu/traffic-perf-release-{case}.json`，优化前同 profile 的结果保存为 `traffic-perf-baseline-release-{case}.json`，仅规则索引优化后的中间结果保存为 `traffic-perf-rule-index-release-{case}.json`。不把早期 debug 数据与 release 比较为性能提升。

当前入口仍可使用以下命令，每次只选一个 case，分别启动新进程；扩展后的完整双 store 复现命令见对照报告：

```powershell
$env:NYANPASU_TRAFFIC_PERF_CASE = '1000000-closed'
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-traffic --release --all-features --test performance measured_session_workloads -- --ignored --nocapture
```

### 原后端验收的 redb 性能基线

schema 2，全部四组断言通过。下表耗时单位为 ms；活跃组的均值/p95 只统计首帧后的 20 帧，历史组统计全部创建/关闭帧。RSS 为结束入账时采样。

| 数据集      |  帧数 | 入账总耗时 s |   首帧 |      均值 / p95 | 最大单帧 |   DB MiB | RSS MiB |
| ----------- | ----: | -----------: | -----: | --------------: | -------: | -------: | ------: |
| 1k 活跃     |    21 |        0.919 |  62.75 |   42.51 / 45.30 |    62.75 |    16.07 |   25.32 |
| 10k 活跃    |    21 |       10.702 | 781.09 | 493.03 / 519.91 |   781.09 |   128.50 |  119.37 |
| 100k 已关闭 |   200 |       12.496 |  66.51 |   62.35 / 75.69 |   331.82 |   514.00 |   66.54 |
| 1M 已关闭   | 2,000 |      138.252 |  65.84 |   69.01 / 88.49 | 4,162.82 | 6,144.64 |   68.58 |

| 数据集      | 连接分页 | 规则累计 | 目标 top-20 | Live 拓扑 | 规则查询排前时的观察延迟 | 目标查询排前时的观察延迟 | Live 查询排前时的观察延迟 |
| ----------- | -------: | -------: | ----------: | --------: | -----------------------: | -----------------------: | ------------------------: |
| 1k 活跃     |    0.418 |    0.822 |       1.058 |     35.18 |                    25.70 |                    23.34 |                     59.56 |
| 10k 活跃    |    0.547 |    6.237 |      11.565 |    404.67 |                   285.32 |                   221.15 |                    624.92 |
| 100k 已关闭 |    1.112 |    0.035 |       0.141 |     0.055 |                    2.052 |                    2.287 |                     2.074 |
| 1M 已关闭   |    1.543 |    0.044 |       0.144 |     0.055 |                    0.702 |                    2.193 |                     2.116 |

本组 1M 历史目标排行从规则索引阶段的约 2.1 s 降为 0.144 ms，查询先入队时后续观察约 2.193 ms 完成。数据库仍是 redb，证据支持索引/扫描路径是这处查询瓶颈；不能据此认定数据库引擎的普遍优劣。各轮写入耗时有变化，未做重复统计，不宣称稳定的写入加速比例。

100k 与 1M 历史的入账结束 RSS 分别为 66.54、68.58 MiB，但 1M 数据库逻辑文件约 6 GiB；有界内存没有解决磁盘增长。10k 活跃完整帧仍约 493 ms，Live 拓扑约 405 ms；1M 历史测试出现一次 4.16 s 单帧长尾。当前计时包含计算、序列化、索引更新与持久化，尚未分解定位，不宣称硬实时或归因于单一引擎。

### 验证范围与限制

- fake/redb 共用基础存储契约，覆盖 begin/commit/finish 幂等与冲突、累计、恢复、分钟与时间偏移、保留策略等；分页、索引、拓扑、高基数与 reopen/schema 另外由 adapter/domain 专项测试覆盖，尚未形成每个 trait 方法的完整共同错误矩阵。
- 容量不足、未知提交结果等由 fake adapter 注入；没有对真实 redb 运行中的磁盘写满或中途 IO 故障做操作系统级注入。
- Windows named pipe 与真实 Mihomo HTTP 已验证；Linux/macOS 和 Unix socket 未在本机执行。
- 当前会话完整保留历史，磁盘仍随历史增长；32 MiB 是 redb 页缓存预算，不是 RSS 或磁盘上限。v1 不自动裁剪当前会话明细。
- 旧发行 service 没有 traffic capability 时明确 Unsupported。源码接线通过不代表下载的旧 sidecar 已具备新协议；未执行发布或发行版本更新。

### 后续页面接入点

- 先经 `get_current_traffic_session` 取得当前或最近保留的 session，再固定该 ID 查询，不能翻页途中换 session。
- 已关闭连接使用 `query_traffic_connections`，`filter.status = Some(false)`；`None` 表示全部，分页游标只用于原查询。
- 规则页面使用 `query_traffic_usage` 的规则过滤：省略规则 context 表示按内核报告的 kind/payload 汇总；带 context 才缩小到可识别的配置归因。累计与 `current_rate` 的未知状态分别展示。
- 拓扑使用 `query_traffic_topology` 的 Live、Session 或 MinuteWindow；节点过滤可直接用于下钻。拓扑全部边的字节之和不是会话总量。
- 统计页面使用 usage 的 Process/Target/Rule/Exit/Path 等 group-by；保留 `other` 和 `time_unallocated`，并展示采样缺口。分钟窗口必须按 UTC 分钟对齐，过宽请求返回明确错误。
- 页面“清空”和“暂停记录”仍是展示层操作，不删除 traffic 账目。新历史页面和统计页面不在本次实现范围内。

## 16. 用户追加：独立 Sol 的双 store 对照

原后端实施和 leader review 完成后，按用户新指示启动独立 Sol agent，实现 `turso-store` 可选 feature 下的本地 `TursoTrafficStore`，与现有 redb 运行同一 `TrafficStore` 契约和 actor 工作负载。默认生产组合仍使用 redb，不提供自动数据格式迁移或应用设置切换。

- [x] 完整 trait 实现、共享基础契约、adapter 专项回归与完整 u64/u128 边界；all-features 44 项、仅启用 Turso 的 33 项测试通过，严格 Clippy 通过。
- [x] 确认实际 Turso 连接的 WAL/FULL/cache 设置；处理失败事务显式回滚、未知提交后的清理与不可用连接，避免缓存语句读取未提交状态。
- [x] 同一 release 可执行文件、G: 新数据库、固定四线程 Tokio；四种规模、两种 store、各三轮，共 24 组通过。单线程试跑保留作诊断，不混入正式结果。
- [x] leader 独立审查源码与 24 份原始 JSON，核对帧/提交次数、活跃数、分页/分组/路径数量及持久化配置；这不是逐记录扫描校验数据库。

百万已关闭连接的三轮入账耗时中位数为 redb 126.735 s、Turso 454.816 s；文件逻辑长度中位数（含附属文件）为 6,144.64 MiB、4,471.33 MiB，结束入账时 RSS 采样中位数为 69.84 MiB、45.85 MiB。当前两个 adapter 表现为写入耗时与文件/内存占用的取舍，不据此宣称引擎本身的性能上限，也不把换库当成当前会话磁盘增长的解决方案。

完整结果、测量身份、复现脚本、SQL schema/写入策略差异和真实限制见 [store-benchmark.md](./store-benchmark.md)。测量完成后按用户后续指示提交实现并创建 Draft PR；未执行发布。
