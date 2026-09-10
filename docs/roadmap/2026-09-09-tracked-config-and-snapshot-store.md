# 配置追踪与 Snapshot Store Roadmap

日期：2026-09-09

状态：P0 为当前审计修补；P1–P3 为后续交付，不能据此声称已经完成统一配置 wrapper。

审计来源：[Snapshot Store 架构审计](https://chatgpt.com/share/6aa03799-76a0-83ea-8de6-3c3168e55ebf)。审计基线为应用 PR [#5227](https://github.com/libnyanpasu/clash-nyanpasu/pull/5227) 的 `cb3b161`、runtime PR [#414](https://github.com/libnyanpasu/nyanpasu-runtime/pull/414) 的 `3a003e0`。以下安排结合代码核对结果；外部审计的测试描述不作为本地测试证据。

关联：[控制通道实施计划](../plan/2026-09-07-core-control-channel-ipc.md)。本 roadmap 接续该计划的快照部分；服务发布门槛继续有效。

## 1. 目标与责任边界

最终目标是让派生配置的修改、记录和冻结不可分离，同时让应用配置的持久化提交与副作用具有清楚的边界。

| 组件                                      | 权威内容                                                 | 类型与边界                                      |
| ----------------------------------------- | -------------------------------------------------------- | ----------------------------------------------- |
| Application / Profiles / Clash 配置 actor | 已提交的期望配置及版本                                   | Actor service；持久化由注入的 port 完成         |
| ConfigWorkspace（待实现）                 | 一次 pipeline 执行的当前值及转换记录                     | Pure service；不拥有文件、网络或后台任务        |
| core-manager / EpochPlan                  | 已提交给受管核心的有效配置、revision、实例及事务状态     | 生命周期服务；实际通道决策仍在这里              |
| RuntimeSnapshotStore                      | 应用观察视图、成功应用关联、尚未获取的内容、最近成功快照 | 有界的具体读写 wrapper；本轮内部使用私有 watch  |
| ConfigCommitSubscription                  | 最近一次配置提交通知                                     | 观察 port；不是完整状态存储，也不是可靠事件日志 |
| Tauri / UI                                | inspection DTO 和交互                                    | Adapter；不直接改写上述权威状态                 |

IPC 职责保持不变：应用冻结用户偏好；宿主 adapter 提供目录和授权能力；manager 结合核心能力、epoch 与当前实例选择实际通道及切换路径。GUI → daemon 的控制协议兼容性由服务层处理。应用 pipeline 不提前分配受管 socket 路径。

不在此路线中引入全局 store、通用 service locator、持久化 actor → core-manager 的同步调用环，或两仓库共享的庞大 graph 框架。

## 2. 必须保持的不变量

1. 期望配置先提交，再规划副作用；副作用失败报告降级，不静默撤销成功提交。
2. generated、成功应用关联、effective 内容、当前运行实例是不同事实，不用单个 `applied` 布尔替代所有状态。
3. 应用成功但 effective 查询失败，只改变 inspection 可用性；恢复过程只能读核心，不能再次 reconcile。
4. 内容必须与成功应用的完整 revision、宿主及宿主 generation 匹配。相同 source hash 不能证明属于同一运行请求。
5. 新生成失败候选不清除最近成功应用历史；旧查询完成不能覆盖较新的成功关联。
6. manager 的提交通知不能执行注入代码或向消费者泄漏锁 guard；消费者阻塞或 panic 不影响核心事务。
7. runtime 改写通过统一 transition API 附加到 inspection；不把受管端点回写为下一次生成的源配置。
8. 空变化、嵌套字段、数组、独立分支的 diff 语义只有一套实现。
9. bootstrap 仅是过渡配置；rollback 保留旧提交；stop 后历史记录不代表当前仍在运行。
10. 原始配置可能含凭据。日志和普通状态事件不携带配置正文；inspection 在生成 YAML 与 diff 前脱敏。持久化 raw snapshot 必须先另行设计访问权限及保留策略。

## 3. 审计问题与阶段映射

| 审计问题                                         | 当前处理                                                                          | 后续责任                                                         |
| ------------------------------------------------ | --------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| 首次 effective 查询失败后无法恢复                | P0 保存成功绑定与 pending 内容；打开 inspection 时只读重试，身份校验后补齐        | P1 完善 unavailable / superseded / 当前实例的 UI 状态            |
| sink 在 manager 事务锁内执行                     | P0 删除回调式 `ConfigSnapshotSink`，改为私有提交槽和返回 owned snapshot 的订阅    | 只有确有落盘需求时才增加外部持久化消费者及其生命周期             |
| 应用只有写侧 trait，读仍固定在另一通道           | P0 使用具体 `RuntimeSnapshotStore`，由 `CoreLifecycleArgs` 注入，读写来自同一实例 | P1 分离 reader / writer 能力；存在第二种真实实现时才抽完整 trait |
| CoreController 手工改图、顶层 diff、空集合不一致 | P0 `append_transition` 复用 JSON Patch、changed-fields 与图校验                   | P2 将 pipeline 当前值和 recorder 合为 owner                      |
| 策略变化导致相同 HTTP 配置也 Switch              | P0 显式比较进程选项；策略由 effective 配置差异判定；Noop 保存新期望策略           | P2 抽纯 ControllerPlan，继续覆盖 bootstrap / rollback / restart  |
| 同一 YAML 没有冻结对应控制策略                   | 当前构建仍从同次 app 读取传递策略；未声称类型已完全封闭                           | P1 将策略收进冻结请求身份                                        |
| actor 提交没有统一 ChangeSet                     | 维持现有提交协调，避免改变批量提交语义                                            | P3 在批次提交边界统一生成变更集与副作用计划                      |

## 4. P0：本阶段修补

### P0.1 成功应用关联与只读恢复

`ReconcileReport` 保留已应用 revision 和提交时宿主归属，即使后续内容查询失败也不丢失事实。workflow 将带绑定的构建写入 pending；有效内容到达后才追加 `CoreController` 节点并更新最近成功视图。

恢复入口是现有 generated / last-applied inspection 查询。每次读取至多尝试一次 pending 内容查询，沿用 CoreClient 的有限超时；不新增周期任务或无限重试。返回前校验宿主 generation、完整 revision，写回时再次校验 pending 构建身份。

当新候选 C 已生成但未成功应用时，允许补齐之前成功的 B；只更新 B 的 applied 视图，不覆盖 C 的 generated 视图。较新的成功应用 D 则使 B 的晚到结果失效。持续不可用时保留 pending 与上次可读历史。

### P0.2 提交通知隔离

manager 内部保存最近提交的完整配置。`subscribe_config_commits()` 返回专用 reader，仅提供 owned 值与异步变更等待，不暴露 watch guard。消费者自行拥有执行与存储生命周期；manager 不调用消费者，不等待消费确认。

慢消费允许合并中间提交；新订阅者可读取最近值；manager drop 关闭通知。stop 后最近提交仍是历史值。需要判断当前是否运行时必须调用 `effective_config()`，不能凭通知中的最后一条推断。此通知不是 respawn / stop 的完整事件流。

本阶段删除尚未发布的回调式 API，并迁移当前全部调用者，不保留假装等价的 compatibility sink。应用侧具体 store 的注入不因此取消。

### P0.3 图追加统一

`ConfigSnapshotsGraph::append_transition(parent, tag, value)` 负责生成 JSON Patch、推导 changed fields、连接节点和验证候选图；错误不修改原图。materialized 与 stored graph 复用拓扑验证，原 archive 语义保持。

测试必须覆盖与 builder 产生的图一致、空变化为 `None`、嵌套与数组路径、无效 parent / 图结构的原子失败。已有 archive、独立分支及深度限制测试继续执行。

### P0.4 策略与进程差异分离

显式比较核心类型、二进制、版本、features、工作目录及 startup / health / restart / backoff 等进程选项；`local_ipc` 不再因为出现在整个 options 的 Debug 文本中而触发 Switch。Backoff 需要上游 utils 提供值相等比较，这是一项最小基础类型变更，不增加运行时依赖。

实际端点或监听面变化仍由最终配置分类决定。Noop 保持 live revision 与实例，更新 `source_spec` 和 `last_spec`，保证未来手动 restart 使用新策略。自动 respawn 继续使用当前 epoch 的已解析配置，不在后台重新协商出另一通道。

### P0 出口

- 上述四项的回归测试通过；记录完整 workspace 的失败与忽略项，不用定向通过冒充全绿。
- 原 IPC 三种组合、HTTP fallback、服务授权测试无回归。
- DTO / bindings / UI 文案同步，pending 不被展示为已确认当前运行。
- 原始配置继续通过现有脱敏投影，不扩大一般状态事件载荷。
- 代码可在隔离 worktree 审核；发布和跨用户验证不由本阶段自动豁免。

## 5. P1：封闭观察 store 与冻结请求身份

依赖：P0 完成。交付单位以应用仓库为主，避免同时改动所有 pipeline 步骤。

1. 从同一 `RuntimeSnapshotStore` 派生 writer 和 reader 能力。workflow 只获得 generated / bind / resolve 写权限；inspection 获得 read 和窄化的恢复协调入口。禁止替换 writer 后仍读取旧 store。
2. 定义 generated、pending、available、unavailable、superseded 的合法转换及展示规则。unavailable 记录安全的分类错误与最近尝试信息；不保存凭据或无界错误日志。失去宿主联系时不假装已经 stop。
3. 明确两种判断：完整 revision 相同表示配置一致；instance_id 相同才表示同一进程。respawn 可以维持配置关联，但 UI 不得把新进程说成旧实例。stop 后保留历史，当前状态由核心查询决定。
4. 引入冻结构建/请求类型，将生成配置身份、LocalIpcSettings 和必要的输入版本关联。序列化 bytes/hash 与完整请求 digest 分开命名，不能只凭 YAML hash 重放另一套策略。
5. 仅在需求明确后选择自动恢复机制：复用已有状态刷新触发一次有界只读查询并合并并发请求。不要新增无主定时器；定义关闭、宿主切换和 superseded 时的取消行为。

验收：注入两个独立 store 无串线；clone 的 reader/writer 共享同一状态；查询超时不触发 mutation；较新绑定排除旧结果；相同 YAML 不同策略的请求身份不同；stop、respawn、host-generation 变化各有明确断言。保留对没有第二种存储实现时使用具体类型的选择，不为测试引入生产 service locator。

## 6. P2：配置修改与记录合为一个 owner

依赖：P1 冻结身份；P0 统一 transition API。分为可独立评审的应用 pipeline 和 runtime 两步。

### P2-A 应用 ConfigWorkspace

- workspace 拥有当前不可变 ConfigValue 和 recorder，取消 executor 外部的另一份 `working`。
- `step(tag, transform)` 内部完成读取当前 view、计算候选、校验、记录及推进当前位置。不得要求调用方再补一次 `record()`。
- 对既有异步脚本，可采用带父节点身份的候选结果再原子提交；不能把可变权威引用交给脚本。日志与失败 passthrough 的现有策略保持不变。
- 独立 composition 分支携带自己的 baseline，并通过明确的分支 API 接入；不破坏已有节点 key 与 inspection 选择稳定性。
- `finish()` 消费 workspace，从同一最终节点导出 config、bytes、hash 和最终节点引用；外部不能分别填写不一致的 artifact 字段。若序列化失败，不产生部分可发布产物。
- read view 带版本/节点身份且只读；纯函数可自由读取，导出 YAML 或脚本输入是合法边界，但不能由导出副本绕过 workspace 回写权威状态。

迁移顺序：内建确定性步骤 → profile merge / composition → 脚本边界 → finalizing 和 artifact 生成。每条迁移调用链同时移除旧的双变量写法，不留下默认兼容旁路。若公开 API 暂时无法迁移，必须注明 blocker 和删除条件。

验收：transform 返回错误后当前位置、值、图和产物均保持一致；失败脚本按原语义记录而不污染配置；空变化、删除、数组、嵌套和独立分支可回放；最终节点导出的内容与提交 bytes/hash 一致；外部无法取得可写当前值。

### P2-B Runtime 的纯 ControllerPlan / PreparedEffectiveConfig

- 在 manager 内部把策略、已解析能力、宿主目录、epoch 转为纯 ControllerPlan；没有应用的 ProfileId 或 OperatorTag 依赖。
- 用受控入口构造 PreparedEffectiveConfig，区分 full 和 bootstrap；校验、hash、staging 读取同一冻结结果。
- manager 输出通用的输入/输出身份与有效内容；应用 adapter 通过统一 transition API 映射到 CoreController。
- runtime 文件存储、授权和进程仍在 adapters；pure plan 不隐式访问文件系统或分配进程。

验收：真实通道切换、保持 HTTP 的 hard switch、fallback、bootstrap 端口/TUN 恢复、失败 rollback、自动 respawn、人工 restart 均不产生身份错配。任何共用库抽取须由已证明重复的算法驱动，不能让 runtime 反向依赖应用 inspection graph。

## 7. P3：期望配置 ChangeSet 与副作用规划

依赖：P1/P2 的清晰配置身份。涉及 Application / Profiles / Clash 配置 actor 和现有 bridge 提交协调；不合并成一个生命周期巨型 actor。

1. 在统一 mutation 入口根据 before / after 计算类型化 ChangeSet；同值 patch 不触发副作用。
2. 持久化完成后生成 CommittedChange，绑定配置版本及批次身份；提交失败不发布成功通知。
3. 多配置域提交由已有 coordinator 形成完整批次结果。禁止每个底层 actor 提交一部分就分别触发 rebuild，从而读到半批次状态。
4. 应用工作流将一批变更合并为副作用计划。TUN 与控制通道同时变化只安排一次 reconcile；系统代理、热键等仍调用相应 typed clients。
5. 先落地已触及的控制通道/TUN 路径，再逐条迁移 legacy bridge；不借此清理无关服务。需要临时桥接时写明原因及删除条件。
6. 决定 side-effect 失败的重试粒度与幂等键；仅重试未完成效果，不重复提交配置或重跑已成功 mutation。

验收：混合配置批次只观察最终版本；持久化失败无核心调用；提交成功后的副作用失败保留状态并返回降级；相同 patch 为零效果；并发批次结果不会串版本；持久化 actor 不依赖 Tauri 或 core-manager。

## 8. 验证矩阵与执行入口

| 场景                                                           | 观察点                                          | 所属阶段 |
| -------------------------------------------------------------- | ----------------------------------------------- | -------- |
| 应用成功、第一次内容查询失败、再次打开恢复                     | 提交计数不变；pending 清除；CoreController 出现 | P0       |
| B 已成功但内容待取，C 生成失败                                 | C 仍为 generated，恢复 B 成为 last-applied      | P0       |
| B 查询晚到而 D 已成功；不同宿主 generation / revision          | 拒绝旧结果，不覆盖 D                            | P0/P1    |
| 消费者不读、持有 owned snapshot、消费者 panic                  | mutation 仍完成；最新提交可读取；无用户锁 guard | P0       |
| IPC 不可用时 Prefer → Disable，然后宿主恢复 IPC 能力再 restart | 首次切换 Noop，restart 使用 Disable             | P0       |
| 相同内容、嵌套修改、数组修改、非法图                           | diff 与 recorder 一致，失败无半状态             | P0/P2    |
| 注入 store 与独立应用图                                        | workflow 与 inspection 共用实例，两个图互不污染 | P0/P1    |
| stop / rollback / respawn / host handoff                       | full、历史、当前配置、当前进程身份不混淆        | P1/P2    |
| 脚本失败与 finalizing 序列化失败                               | 配置记录原子，容错不变，产物与最终节点绑定      | P2       |
| 跨域 patch、提交失败、后置效果失败                             | 批次与 ChangeSet 语义成立                       | P3       |

本地验证命令（从主仓库 worktree 执行）：

```sh
cargo test --manifest-path backend/Cargo.toml --workspace --all-features --lib
cargo test --manifest-path backend/Cargo.toml export_typescript_bindings --all-features
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-core-manager --lib --test control_channel
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-service-runtime --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
cargo fmt --manifest-path backend/nyanpasu-runtime/Cargo.toml --all -- --check
pnpm -F interface build
pnpm web:build
pnpm typecheck
pnpm lint:oxlint
pnpm lint:architecture-ledger
```

对修改的 TS/TSX/JSON/Markdown 定向运行 Prettier。actor 测试用 acknowledgement、watch 条件或 typed RPC 同步，不用 sleep 推测完成。真实核心测试通过 `MIHOMO_BIN` / `CLASH_RS_BIN` 指向现有二进制并显式运行 ignored 用例，不覆盖共享 sidecar。

## 9. 合并、发布和完成标准

代码依赖顺序：utils 的 Backoff 值相等能力 → runtime 分类器及通知 API → 应用关联恢复与图追加。每层先审核并提供可获取提交；正式 gitlink 必须对应已发布的协议实现。P0 不修改既有远端提交历史，不自动发布服务或安装系统 daemon。

服务发布仍需：

- `rc.4` 制品、版本检测、下载脚本获取的二进制与新协议一致，rc.3 被明确拒绝。
- Unix root daemon 到普通 GUI 用户的 REST、WS、重启后 socket 授权、未授权用户拒绝与孤儿清理实测。
- Linux/macOS/Windows 的实际权限和路径矩阵；Windows 未证明跨用户 pipe ACL 前维持 HTTP fallback。
- 应用、daemon、核心重启和 Local↔Service 切换后的关联恢复；不由同用户 Local 实测替代。

P0 完成只表示修复当前行为与隔离风险。整个 roadmap 的完成要求是：派生配置不存在独立可变 working/recorder 双入口，artifact 只能由同一最终节点冻结，期望配置变更经提交结果统一规划效果，snapshot 观察故障不影响核心事务，并且上述服务发布矩阵有实际证据。

## 10. 本轮验证记录（2026-09-09）

| 验证                                                        | 结果                                                                          |
| ----------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 应用 workspace library 回归                                 | 应用 503 项通过、1 项忽略；配置库 143 项通过，其余 workspace library 测试通过 |
| core-manager library / 通道集成                             | 107 项通过、1 项忽略；通道集成 2 项通过                                       |
| 新增真实通道切换                                            | mihomo、clash-rs 共 2 项通过，实际运行 ignored 用例                           |
| service-runtime                                             | 87 项通过                                                                     |
| bindings / interface / web / TypeScript                     | 通过                                                                          |
| Oxlint / Clippy / architecture ledger / fmt / 定向 Prettier | 通过；保留既有警告                                                            |

完整 core-manager 回归首次在 `dropping_live_manager_releases_runtime_ownership` 出现 `No such file or directory`，对应 manager_orchestration 目标 19 通过、1 失败；该目标串行复跑 20 项全部通过。首次整套运行不能标记为全绿，也未将这个未改动测试的失败诊断为确定根因。本轮旧的 macOS version-probe 测试通过。

新增回归已证明：首次 effective 查询失败可由 inspection 恢复且提交数不增加；晚到结果不覆盖较新绑定；失败候选保留 generated 视图；统一图追加与 recorder 一致；注入 store 被 workflow 和 reader 共用；无实际通道变化时 Noop，restart 保留新策略；消费 owned snapshot 或消费者 panic 不改变核心事务结果。

本轮源码和文档通过应用 PR #5227、runtime PR #414 与 [utils PR #185](https://github.com/libnyanpasu/nyanpasu-utils/pull/185) 关联审核。gitlink 引用相应审核提交；发布前仍需按 §9 完成依赖合并和已发布版本接线，不能只提交父仓库而遗漏嵌套子模块修改。
