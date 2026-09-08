# 内核控制通道（IPC / HTTP）：审计与最终实施计划

日期：2026-09-07（实施记录更新于 2026-09-08）

状态：源码已实现并提交审核；服务发布和跨平台/跨用户实测待完成，详见 §10。

审计基线：主仓库 `7e24d8e4`，runtime 子模块 `82092b0`

## 1. 目标与范围

保留原稿的产品决策：

- 设置项「控制通道 / Control Channel」：`PreferIpc`（默认）与 `HttpOnly`。
- 独立开关「IPC 可用时关闭 HTTP 外部控制器」：默认关闭，即默认保留 HTTP。
- 首页显示实际主控制通道：`HTTP`、`IPC · Named Pipe`、`IPC · Unix Socket`。
- Local 与 Service 使用相同应用设置；修改设置不要求重装 daemon。
- UI 不暴露 `Force`。保留 HTTP 指保留源配置中的 HTTP 地址及 secret，不额外创造监听地址。

本次仅审计和修改计划，不执行功能开发、服务安装或版本发布。以下设计以现有 actor/DI 规则为约束；平台相关行为作为实施验收项，不把代码阅读等同于实机验证。

## 2. 审计结论

原稿不能直接执行，主要问题如下。路径均相对仓库根目录。

| 级别 | 原稿问题                                                                                            | 当前实现依据                                                                                                                                           | 最终处理                                                                                  |
| ---- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| P0   | 队列外 setter 与下一次 restart 分离，策略可能在 prepare 中途变化，操作身份也不包含策略              | `backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/control/mod.rs` 的 `ReconcileRequest`、`CoreCommand::payload_digest`；`control/executor.rs` | 设置随同一个 Reconcile 不可变请求传递，进入现有队列、幂等注册和事务；删除独立 setter 方案 |
| P0   | 保留 HTTP 破坏原先“每个 epoch 只有独立 IPC controller”的切换前提                                    | core-manager `config/clash.rs::rewrite_managed_controller`；`manager/switching.rs::graceful_degrade_reason` 目前只检查主通道是否 IPC 等条件            | 有 HTTP controller 监听的切换保守使用 hard switch；不能凭主通道是 IPC 就允许重叠启动      |
| P0   | 服务 runtime 目录是私有目录，只 chmod socket 不解决父目录不可遍历；放宽整个目录会暴露配置等私有文件 | core-manager `config/mod.rs::managed_unix_endpoint` 强制 socket 位于 runtime 内；IPC `api/status.rs::ConfigRevisionInfo` 说明 runtime 为 `0700`        | 独立控制端点目录和窄权限适配器；保留私有 runtime 的权限与验证器                           |
| P0   | daemon 的管道 ACL 工具并不能直接控制第三方核心创建的管道                                            | `backend/nyanpasu-runtime/nyanpasu_ipc/src/server/mod.rs::create_server` 把安全描述符传给自己创建的 listener                                           | Windows 单独验证核心管道及重建权限；不把“生成 SDDL”当作已解决访问问题                     |
| P1   | 新动词不在现有单一 Reconcile 协议内，新增一套操作路径；仅捕获未知方法不足以保证旧服务兼容           | IPC `api/core/v2.rs`；`backend/tauri/src/core/service/compat.rs` 已有显式最低版本门                                                                    | 扩展现有 Reconcile DTO，复用版本门；禁止旧 daemon 静默忽略新设置                          |
| P1   | 安装参数不是持续生效的应用设置来源；daemon 重启、宿主切换和后续普通 reconcile 都可能丢设置          | service-runtime `cmds/install.rs`、`server/manager_bridge.rs`；tauri `client/core_lifecycle/workflow.rs::reconcile`                                    | 每次应用 Reconcile 都携带完整设置；daemon 不另存应用偏好                                  |
| P1   | 只检查 manager 自身探针不能证明普通 GUI 可连接 root/SYSTEM 创建的端点                               | tauri `core/actor_v2/api.rs` 和 `endpoint.rs::api_connection`：GUI 后端直接访问核心 API                                                                | 服务验收必须包含非提权 GUI 的 REST 和 WebSocket；区分核心能力与宿主可达性                 |
| P1   | 未覆盖子模块、服务版本和实际 sidecar 的发布一致性                                                   | `.gitmodules`；`backend/Cargo.toml` 关于发布 tag 与服务下载版本的约束                                                                                  | runtime 先完整实现及发布，主仓库更新 gitlink、兼容门和打包版本                            |
| P2   | Local 状态不必另取 credential-bearing API binding，否则可能拼出跨实例快照                           | core-manager `state.rs`、`manager/publish.rs` 已发布无 secret 的 controller                                                                            | 从同一 `CoreStatus` 映射 controller；Service 从 `CoreInfos` 映射                          |
| P2   | “仅 HTTP”可能仅改变客户端选择，源配置残留 pipe/unix 仍被核心监听                                    | core-manager `config/mod.rs::prepare` 在不改写 IPC 时只用 `inspect_http`，没有删除源 IPC 字段                                                          | 显式应用通道设置时统一规范化三个 controller 键                                            |

能力探测及 HTTP/UnixSocket/NamedPipe 客户端抽象可以复用。版本下限以 `nyanpasu-core-metadata/src/feature/clash.rs` 的实现与测试为准；它证明声明的核心能力，不证明双监听、GUI 权限或实际二进制行为。

## 3. 服务分类与依赖方向

```text
Tauri command / UI
  -> NyanpasuClient
    -> CoreLifecycleActor（串行工作流，读取已提交状态）
      -> RuntimeIntentBuilder（纯服务）
      -> CoreFacade / CoreClient（现有路由与宿主交接）
        -> LocalEndpoint -> CoreControl
        -> ServiceEndpoint -> daemon IPC Reconcile -> CoreControl
          -> 现有 core-manager 执行器 / manager 生命周期
            -> 注入的控制端点权限适配器
```

| 组件                                   | 分类与职责                                                                                    |
| -------------------------------------- | --------------------------------------------------------------------------------------------- |
| 应用状态 actor                         | 持久化期望设置；schema、默认值、patch 使用纯类型与函数                                        |
| CoreLifecycleActor                     | actor service；串行构建并提交运行意图，协调提交后的副作用                                     |
| CoreClient / 路由 actor                | actor service 的 typed client；沿用宿主交接与操作归属，不在此读取 globals                     |
| ServiceHostActor                       | actor service；仅负责 daemon 安装、启动、退出、兼容探测与 endpoint 供给，不持有另一份通道偏好 |
| RuntimeIntentBuilder / controller 改写 | pure service；显式输入设置、能力与配置，输出意图或有效配置                                    |
| CoreManagerService / ServiceEndpoint   | adapter；做 wire/domain 转换并调用现有控制执行器，不另开策略锁或后台循环                      |
| 控制端点权限适配器                     | adapter/port；宿主注入授权身份、端点根目录、命名空间与 OS 操作，生命周期由现有 manager 管理   |

本功能不新增 `ControlChannelActor`，也不顺带重写成熟的 CoreControl 执行器为 ractor。复用已有状态所有者，新增依赖由 Local composition root 和 daemon `server::run` 显式构造。核心域和 manager 不引入 Tauri 类型。

## 4. 设置模型及运行语义

### 4.1 持久化与不可变请求

应用 schema 新增：

- `clash_control_channel: ClashControlChannel`，serde `prefer_ipc` / `http_only`，缺失默认 `prefer_ipc`。
- `clash_ipc_disable_http_controller: bool`，缺失默认 `false`。
- `IVerge` 按既有双 schema bridge 使用对应 `Option<T>`，补全正反映射与默认投影。

core-manager 定义值类型 `LocalIpcSettings { policy, keep_http_controller }`。应用到 domain 的映射为纯函数：`PreferIpc -> Prefer`，`HttpOnly -> Disable`，`keep_http_controller = !disable_http`。HTTP only 下开关保留用户值但不生效。

将设置放入 `RuntimeIntent`、`ReconcileRequest`，并传入 `InstanceSpec` / epoch plan；请求进入队列后不再读取可变偏好。`ManagerOptions` 的现有策略仅作为旧调用者/CLI 的构造默认，不增加运行期 setter；无显式设置的调用在入队前解析默认值。新的应用调用始终显式传完整设置。

设置必须参与 `CoreCommand::payload_digest`，不要仅塞入当前不参与摘要的 `InstanceOptions`。配置文本摘要仍只校验配置字节，不能与操作身份摘要混淆。源 YAML 不变但设置变了仍要经过有效配置比较，不能被 source hash 提前判为 Noop。

### 4.2 行为矩阵

| 期望设置             | 核心及宿主具备可用 IPC | 有效配置与主控制通道                                     |
| -------------------- | ---------------------- | -------------------------------------------------------- |
| PreferIpc，保留 HTTP | 是                     | 写受管 IPC，保留源 HTTP，主通道 IPC                      |
| PreferIpc，关闭 HTTP | 是                     | 写受管 IPC，移除 HTTP，主通道 IPC                        |
| PreferIpc，任意开关  | 否                     | 清理源 pipe/unix，保留源 HTTP，主通道 HTTP，返回回退原因 |
| HttpOnly，任意开关   | 任意                   | 清理源 pipe/unix，保留源 HTTP，主通道 HTTP               |

需要 HTTP 时若源配置没有合法 HTTP controller，则显式报错，不猜端口或 secret。开关只管理本计划所列三个 controller 键；实现前核对所支持核心是否还有其他 HTTP/TLS 控制监听，若有则一并定义关闭规则，不能对仍可访问的 HTTP 入口声称“已关闭”。

停止状态修改设置只持久化，不启动核心、不启动 daemon。运行中修改后提交一次包含配置与设置的 Reconcile，由 manager 决定 Noop/Restarted/Switched 等结果，不在 UI 或 endpoint 再叠加一次 restart。

已提交配置不因运行失败回滚；按现有部分提交/降级结果报告“已保存但未应用”。旧运行实例保留或事务回退时，状态展示其真实通道。快速连续修改最终收敛到最新已提交快照；TUN 与通道一起修改合并为一次提交后的重建。

### 4.3 核心改写必须反映到配置 snapshot（2026-09-08 补充）

这里的 snapshot 包含配置检查页使用的运行配置快照图，不仅是 `CoreStatusSnapshot.controller`。当前 `client/core_lifecycle/workflow.rs::reconcile` 在 host 应用前发布 `RuntimeSnapshot`；`client/runtime_inspection.rs` 只读取 promoted pipeline graph。manager 的 `EpochPlan` 已保留 `source_document` 与 `effective_document`，但尚未接回应用快照。因此只扩展状态 controller 不能满足“核心改写在 snapshot 中可见”。

审计发现的 manager 配置转换：

| 转换                      | 当前行为                                                                                                                                                        | snapshot 语义                                                                                |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| 控制通道改写              | 删除/重写 `external-controller`、`external-controller-pipe`、`external-controller-unix`；本计划增加保留 HTTP 与 fallback 规范化                                 | 稳态有效配置的一部分，必须反映真实 endpoint、HTTP 保留/移除及 diff                           |
| Mihomo bootstrap 入站处理 | `config/mihomo.rs::zero_inbounds` 将已存在且非零的 `port`、`socks-port`、`redir-port`、`tproxy-port`、`mixed-port` 置零；将原本为 true 的 `tun.enable` 置 false | 仅 graceful switch 过渡配置；不得当作最终已应用配置。诊断中展示时明确标记 bootstrap/候选实例 |
| canonicalize / serialize  | 映射键排序及 YAML 序列化规范化                                                                                                                                  | 不视为业务字段改写；diff 按配置值比较，避免排序噪声                                          |

当前检查的 manager prepare/apply/switch 调用链中，未发现除此之外自主修改稳态业务配置的逻辑。PATCH 字段筛选是提交核心 API 的请求构造，DNS adapter 是 OS 副作用，均不应伪装成额外的最终 YAML 改写；应用 enhance pipeline 的其他转换已有自己的上游 snapshot 节点。

实施要求：

1. 保留不可变的生成产物 `promoted`，增加与之关联的 host 有效配置视图；配置检查图追加“核心有效配置”节点及 source → effective 的 diff。不能覆盖原生成节点，也不能将受管 socket 路径回写成下一次 Reconcile 的源配置。
2. manager 提供原子读取的有效配置快照，携带 host 侧实例 ID、revision（含 source/effective hash）、配置及应用阶段。读取由现有状态所有者串行完成，不让应用直接读取服务私有配置路径或跨锁拼接 controller 与文档。语义是 manager 已确认的有效配置，不承诺包含核心自身未上报的默认值或第三方修改。
3. Local 通过 typed endpoint 读取；Service 增加受现有 IPC 授权保护的只读 effective-config 查询与对应 DTO。该查询纳入本轮服务版本门和发布闭环；不会引入第二条变更通道。完整配置按需查询，状态事件只承载实例/revision 等失效标识。
4. CoreLifecycleActor 将操作 ID、构建 snapshot ID 与返回 revision 关联，并校验查询结果的实例/revision。源码 hash 相同不代表同一次构建；迟到结果、宿主变化或查询时实例已替换，必须丢弃或重新获取，不能把 A 实例的 effective 节点挂到 B 构建上。
5. `Started/Patched/Reloaded/Restarted/Switched/Noop` 的成功终态均关联真实有效快照；失败或 `RolledBack` 保留旧实例的 applied 视图，并把新生成产物标为未应用。有效配置查询失败时显示未知/待刷新，不用 promoted 配置冒充已应用结果。自动重启、恢复及宿主交接通过现有状态订阅重新校验绑定。
6. bootstrap 不是最终节点：只有 full config 提交并确认成功后才能标记 applied。过渡期保留旧已应用视图并标记切换中；如展示候选配置，明确区分实例与 bootstrap 阶段。本轮不扩展为通用的全生命周期快照历史系统。
7. 完整配置只经受控的配置检查 API 按需提供，不加入通用状态、日志或广播事件。公开检查内容中的 secret 等凭据作脱敏投影；revision/hash 仍取原始有效配置的结果，不对脱敏 YAML 重新计算后冒充运行 revision。

验收补充：IPC endpoint 和 HTTP 字段变化可在最终 snapshot/diff 中看到；HTTP fallback 与实际配置一致；bootstrap 置零/关闭 TUN 不污染最终 applied 节点；回退保留旧配置；Noop 可正确关联新生成产物；过期查询和重复 source hash 不发生串线；Local/Service 的有效配置视图语义一致，且不泄漏凭据。

## 5. core-manager 事务与切换

1. 设置沿 prepare、check、apply、switch、bootstrap/full config 全链路显式传递。同一事务内能力解析和改写使用同一份设置。
2. 自动 respawn、手动重启及失败回退使用对应实例/epoch 保存的设置，不能使用后来到达请求的设置。
3. 统一 controller 改写函数支持保留 HTTP，并保证 HTTP fallback/HTTP only 不遗留非受管 IPC。旧无显式设置入口确需保留旧语义时，按仓库要求标注迁移原因和移除条件，不让新应用经过该兼容入口。
4. 判断 graceful switch 时检查旧、新有效配置的 HTTP controller 监听面。任一侧存在 HTTP controller 监听时保守退化为 hard switch，并给出明确原因；本轮不实现临时随机 HTTP 端口或控制器端口迁移。
5. IPC-only 的原有 graceful switch 继续沿用现有入站/DNS/核心能力门槛。关闭 HTTP 并不意味着其他监听没有冲突。
6. 有效配置校验在破坏旧实例前完成；事务失败沿用既有 rollback/quarantine/stop proof，不能通过改全局策略规避。

验证：纯配置矩阵、设置改变但 source hash 不变、重复提交幂等、同 ID 不同设置冲突、并发请求隔离、CAS 失败无副作用、带 HTTP 的切换不并行占端口、失败回退保留旧设置、respawn 重建端点。

## 6. 服务模式设计（关键路径）

### 6.1 协议与生命周期

扩展 `nyanpasu_ipc/src/api/core/v2.rs::CoreCommandInfo::Reconcile` 的结构化通道设置 DTO，复用 `CoreV2Submit` / operation query；不新增 `CoreV2SetLocalIpc` 路由。

- wire DTO 与 manager 类型显式转换；不把 manager 内部对象、调用者路径或授权身份透传为服务端输入。
- 兼容旧客户端时，缺失字段解析为 daemon 构造默认；在服务端 admission 时固定下来，并纳入最终入队操作身份。增加所需的 `TODO(actor-migration)`，移除条件是停止支持缺字段的客户端。
- 新应用通过 `core/service/compat.rs` 的最低版本门确认 daemon 支持此语义后才进入 Service endpoint；版本解析失败或过旧不得试着提交再凭结果猜测。旧 daemon 可能静默忽略未知字段。
- bump 到包含本协议的实际服务发布版本，并同步最低版本；不在计划中编造尚未发布的版本号。复用现有不兼容提示和服务管理流程，不新增自动卸载/重装。
- 保留现有 `--local-ipc-policy` 作为独立 CLI/旧调用者默认，应用不再通过安装参数同步日常偏好，也无需为本功能再加一套安装配置持久化。
- 每次首次启动、普通重建、核心切换、Local↔Service 交接后的目标 Reconcile 都带设置。daemon 重连不应无条件启动已停止核心；复用现有恢复工作流，在下一次需要 Reconcile 时携带最新设置。
- 交接前先验证目标兼容性；旧服务不兼容时不停止 Local 核心。不因为应用设置已保存就停止旧服务或自动换宿主。
- 请求超时属于结果未知：保留原 operation ID 查询结果，不自动换 ID 重做，更不能退回另一条非事务 setter 路径。

验证：现有 `server/routing/tests.rs` harness 覆盖 DTO 与映射、终态查询、幂等冲突、默认兼容；actor fake endpoint 覆盖宿主交接、断连重连与旧版本门。

### 6.2 明确两个 IPC 边界

1. **GUI → daemon**：现有 nyanpasu-ipc socket/pipe，用于生命周期请求及读取 API binding。
2. **GUI 后端与 daemon → 核心**：核心自己创建的 socket/pipe，承担 Clash REST / WebSocket。首页“控制通道”展示的是这一条。

daemon 以 root/SYSTEM 探测成功不能证明 GUI 可达；`CoreApiConnection.secret` 只留在私有后端 binding，不进入首页、状态事件或日志。HTTP 关闭时，GUI 的代理查询、选择、连接管理及订阅仍须能工作。

### 6.3 Unix：独立端点目录，保留私有 runtime

- 增加受注入的控制端点根目录，默认 Local 可沿用自身私有目录；Service 使用私有 runtime 之外的独立目录，只存 socket。
- manager 的 `managed_unix_endpoint` 改为校验注入的端点根目录，而非简单删除现有路径限制。根目录由宿主创建并规范化，禁止调用者请求指定绝对路径。
- 服务目录 owner 为服务账户，授权范围复用安装时建立的 `nyanpasu` 用户组；目标模式为目录 `0750`、socket `0660`，父目录也须可遍历。组不能写目录，私有配置/日志/runtime 不扩大权限。
- 权限 adapter 在每次核心创建/重建 socket 后、对外宣告控制端点就绪前执行授权与验证；启动、自动 respawn、切换和失败清理共用同一生命周期钩子。
- adapter 验证路径属于受管根、对象确为预期 socket、owner 合法且不是符号链接替换；仅清理本 manager/epoch 拥有的端点。
- 命名包含宿主/安装实例命名空间与 epoch，避免多个 manager 都从 epoch 1 开始导致碰撞；覆盖 Unix 路径长度限制。

实测必须用属于授权组的非 root GUI 进程完成连接；另验证未授权用户不能访问。安装后组成员身份尚未进入当前 GUI 会话时，明确报告访问问题，不能仅凭磁盘上的组成员表宣称就绪。

### 6.4 Windows：核心 pipe 的创建与重建独立验证

- pipe 名称使用宿主/安装实例命名空间与 epoch，避免 Local 与 Service 冲突。
- 授权目标沿用服务安装的明确 SID 集合及 SYSTEM/管理员；不默认放宽到所有 Authenticated Users。
- daemon listener 的 `generate_windows_security_descriptor` 只能复用描述符构造部分。核心管道能否在创建时设 ACL、是否允许受控修改、每个 pipe instance 重建是否保留授权，必须针对实际 mihomo/clash-rs 二进制验证。
- 权限处理封装在 Windows adapter，由 manager 生命周期调用；没有可靠的创建/重建保证时，该核心/宿主组合标记为不具备可用 IPC。
- 本轮不新增通用 API relay/proxy。如果直接连接方案不可行，PreferIpc 在该组合使用 HTTP 并报告原因；不能交付一个仅 SYSTEM 探针可达的 IPC 状态。

### 6.5 回退边界与发布门槛

分开记录“核心实现了 IPC”与“当前宿主能提供符合权限约束的 IPC”。前者来自 metadata，后者来自注入 adapter 的平台支持及检查结果。

- 启动前已知不支持时，Prefer 回退 HTTP，Force 报错。回退不修改已保存偏好；关闭 HTTP 开关在 IPC 不可用时不执行。
- 本轮不把任意启动失败、探针超时或 GUI 断连解释为 IPC 不支持。实际授权失败走有限超时和原事务失败/回退旧实例；输出具体原因，不无限启动重试、不盲目打开 HTTP。
- GUI 对已发布 binding 的真实访问失败不能显示为“已验证连接正常”；通道名称与健康/可达性分开呈现。
- 每个平台的服务 IPC 只有通过授权 GUI 的 REST、WebSocket、核心自动重启和并发实例测试后才能宣称支持；未验证组合按宿主不支持处理，并列入发布限制。

## 7. 应用接线与状态展示

1. 修改 `nyanpasu-config/src/application/mod.rs`、tauri `config/nyanpasu/mod.rs`、`bridge/mapping.rs`、`bridge/verge.rs` 及实际 schema/patch 派生所需调用点。验证旧配置、序列化回写、两个方向映射和默认值一致。
2. `client/core_lifecycle/workflow.rs::reconcile` 使用本轮已读取的 application snapshot 构建通道设置；让 runtime 构建产物与 RuntimeIntent 显式携带该值，避免第二次独立读配置。
3. 更新 `core/actor_v2/intent.rs`、`facade.rs`、`endpoint.rs` 和 Local/Service 提交映射。继续通过 `NyanpasuClient` 的普通 async 应用 API 调用，无需向上暴露设置 setter 或 ActorRef。
4. `bridge/verge.rs` 的提交后重建条件合并 TUN 与两个通道字段；检查实际值变化，停止时只保存。保留现有提交失败与部分成功的区别，前端收到部分成功后重新读取已持久化值，避免乐观回滚显示错误设置。
5. `CoreStatusSnapshot` 与 `CoreStatusInfo` 增加 `controller: Option<CoreControllerInfo>`。Local 从同一份 manager `CoreStatus.controller` 映射，Service 从同一份 `CoreInfos.controller` 映射，HTTP 映射沿用服务端去除凭据的规则。
6. 从 `CoreStatusInfo` 到 `useCoreStatus` 保持状态失效/刷新链路；停机、交接、断连时清除或明确标记过期 controller，不能残留前一宿主的通道。
7. 重新生成 Specta bindings，更新所有相关 mock/status 构造点。Secret 不加入任何公开 DTO。
8. 设置页 PortSettings 后新增选择器和开关；HTTP only 时禁用开关但保留保存值。首页只显示通道种类，不展示内部路径。`None` 显示未知/未运行，不能推断成 HTTP。
9. 补齐 en、zh-cn、zh-tw、ko、ru；开关说明明确 Web UI/第三方 HTTP 客户端将不可用。主通道显示 IPC 不代表 HTTP 已关闭。

保留 `get_clash_info` 的旧返回形状，但审查其调用者：应用内部核心操作必须走现有实例绑定 API，不能继续依赖该 HTTP 地址。纯 Web UI/外部访问链接保留 HTTP 语义，不用它来判断首页主通道。

## 8. 实施顺序与验收

### 阶段 0：隔离环境与服务可达性验证

功能实施在仓库外独立 worktree；本次文档审计不创建功能 worktree。初始化 `backend/nyanpasu-runtime` 子模块并核对版本。按 AGENTS.md §17 仅复用 `sidecar/`、`resources/`，独立安装 node_modules，独立 target 与 dist。Rust-only 可用 frontendDist 占位，UI 验收必须真实构建。

先验证三个平台上目标核心的双监听、非提权服务 IPC、管道重建和端点命名，记录核心版本/运行账户/结果。此阶段决定平台 adapter 的支持范围；不把 macOS 实测替代 Windows/Linux 验收。

→ 出口：端点权限方案及支持矩阵有证据；构建前置条件完整。

### 阶段 1：runtime 内完成完整事务与服务实现

完成 §4–6：不可变设置、幂等身份、配置改写、有效配置快照读取、hard switch 判定、权限 adapter、wire 转换与服务默认兼容。同步 runtime workspace 内所有调用者及测试，不能提交无法构建的 DTO 半迁移。

→ 验证：

```bash
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-core-manager
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-ipc --all-features
cargo test --manifest-path backend/nyanpasu-runtime/Cargo.toml -p nyanpasu-service-runtime
cargo fmt --manifest-path backend/nyanpasu-runtime/Cargo.toml --all -- --check
```

上述命令不能替代权限实测。测试用 fake adapter/typed request/显式事件同步，不用 sleep 猜 readiness。

### 阶段 2：应用全链路及 UI

完成 §4.3 与 §7 的应用接线，补充 snapshot 图及已应用视图、停止修改、并发修改、部分成功、宿主切换、旧 daemon 拒绝以及状态无 secret 的测试。把设置始终随构建快照传递作为集成验收重点。

→ 验证：

```bash
cargo test --manifest-path backend/Cargo.toml export_typescript_bindings --all-features
cargo test --manifest-path backend/Cargo.toml --all-features
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
pnpm -F interface build
pnpm typecheck
pnpm lint:oxlint
pnpm lint:architecture-ledger
pnpm web:build
```

对本次文档、TS/TSX/i18n 运行定向 Prettier 检查。根 package.json 没有 `pnpm check`，不使用原稿的占位命令。环境缺失或既有失败单独记录，不能声称验证通过。

### 阶段 3：端到端与发布闭环

| 场景                                 | 必须验证                                                            |
| ------------------------------------ | ------------------------------------------------------------------- |
| Local 与 Service，Prefer + 保留 HTTP | GUI 实际走 IPC；REST/WS 均通；HTTP 同时可用                         |
| Prefer + 关闭 HTTP                   | GUI 功能保持；HTTP 实际不再监听；外部客户端行为符合说明             |
| Premium/低版本/宿主不支持 IPC        | HTTP fallback、可见原因、期望设置不变                               |
| HttpOnly，源 YAML 带自定义 pipe/unix | 实际只保留受管规则允许的 HTTP 控制通道                              |
| 保留 HTTP 时重启/切核心              | 无新旧实例 HTTP 端口争用；正确 hard switch 与失败恢复               |
| Service 核心自动重启                 | 新 socket/pipe 权限有效，旧绑定失效，WS 能重新订阅                  |
| Local↔Service、daemon 重启、应用重启 | 每次所需 Reconcile 设置一致；无停止意图被意外启动；无跨宿主状态残留 |
| 旧 daemon、RPC 超时、权限失败        | 不静默忽略、不重复执行、不误报应用成功                              |
| 多实例、未授权账户                   | 命名不冲突，清理不越界，未授权访问失败                              |

runtime 是独立仓库：先完成源码、版本 bump、测试和对应服务制品发布；主仓库正式 gitlink 指向含该功能的已发布 tag。同步 `REQUIRED_SERVICE_MIN` 并验证 `scripts/check.ts` 获取的服务版本就是协议实现版本，不能只对照本机旧 sidecar。

开发调试使用新构建的 daemon 时明确实际路径；不要覆盖 symlink 指向主 checkout 的共享 sidecar。发布、安装实机服务是后续实施动作，本次审计不执行。

## 9. 提交边界与完成标准

- runtime 子模块：按可独立构建的依赖边界拆分；不可变请求与所有受影响调用者在同一原子提交完成，配置语义/权限实现可在不留半成品的前提下独立提交。协议服务能力与发布版本必须一致。
- 主仓库：gitlink、必要的 Rust 调用者迁移、schema、兼容门及 bindings 放在一个可构建提交；UI 与 i18n 可独立提交。
- 不采用原稿“core-manager setter → 新路由 → app setter”的提交顺序；那会保留第二条生命周期变更路径。
- 提交前只显式 stage 相关文件，检查 status 与 cached diff；不处理现有无关文件。

完成标准：设置保存、事务应用、实例恢复、Service 普通用户访问、状态展示和实际发布二进制六者一致。未完成某个平台实测时，交付说明必须列明其 HTTP 回退限制，不能笼统声称服务 IPC 已全面支持。

## 10. 实施记录（2026-09-08）

实现位于独立 worktree `/private/tmp/clash-nyanpasu-control-channel`，分支 `feat/core-control-channel`；runtime 子模块的源码修改也在该 worktree 内。范围为控制通道、服务权限和对应有效配置 snapshot，不扩展其他服务迁移。

- 请求级 `LocalIpcSettings` 随 Reconcile 冻结，纳入 payload digest；未提供设置的旧调用者在提交入口解析一次默认值。应用始终显式传入同一次构建所用设置，没有独立 setter。
- `ConfigSnapshotSink` 是 core-manager 的可注入上报 port；`RuntimeSnapshotSink` 是应用生成/应用快照存储 port。有效配置查询携带实例身份和完整 revision，应用通过宿主 generation 与 revision 校验关联。新增 `CoreController` 节点，以及最近成功应用快照视图；生成产物不被覆盖，bootstrap 不作为最终 applied 节点。
- HTTP 控制字段还包含 `external-controller-tls`，已纳入关闭 HTTP 和 hard switch 判断。除此以外，manager 的 bootstrap 入站端口置零与临时关闭 TUN 仍仅是过渡配置，不改变本轮范围。
- Unix Service 使用独立 `/var/run/nyanpasu-core`，目录 `0750`、socket `0660`，授权给既有 nyanpasu 组；每次 readiness（包含自动重启）重新授权。路径/文件类型/所有者检查失败则报告错误。宿主无法建立授权目录时 Prefer 回退 HTTP；Windows Service 暂不声明 pipe 支持，明确回退 HTTP。
- 独立 socket 目录按 epoch 清理；崩溃恢复仅清理有 runtime 记录、且进程已确认退出的 endpoint。socket 名称的目录哈希采用 16 位十六进制，避免 macOS 路径长度限制；同时校验实际 socket 路径长度。
- daemon 源码版本与应用最低兼容版本同步为 `2.0.0-rc.4`。该版本尚未发布，主仓库 gitlink 暂指向 runtime PR 的审核提交，合并前需更新到正式发布版本；现有 rc.3 服务将被兼容门拒绝。未覆盖共享 sidecar、未安装或发布 daemon。

验证记录：应用后端 workspace library 回归通过（应用 500 项通过、1 项忽略），随后新增 rc.3 拒绝用例单独通过；bindings 导出、interface 构建、前端构建、TypeScript、Oxlint、架构门禁和 Clippy 通过（保留既有警告）。本机四类真实核心原有 20 项测试通过；新增 mihomo/clash-rs 三种通道设置切换的 2 项实机测试通过，包含 HTTP 监听和有效 snapshot 一致性断言。

runtime 最终定向回归：core-manager library 106 项通过、1 项忽略，控制通道集成 2 项通过（另 2 项真实核心用例已显式运行通过），service-runtime 87 项通过。完整 runtime workspace 回归仍有未改动的 `version_probe_is_cached_by_path_and_mtime_and_supplied_versions_skip_it` 失败：macOS 临时复制二进制在修改 mtime 后版本探测以 `process exited with code None` 退出。服务测试此前发现的 socket 长度误拦截已修复并全部复验通过；不将完整 workspace 标记为全绿。相关日志在 `/private/tmp/control-runtime-complete.log`、`/private/tmp/control-service-final.log` 和 `/private/tmp/control-manager-final.log`。

发布前仍需：Linux/Windows 平台验证、Unix root daemon → 普通 GUI 用户的 REST/WS 与拒绝未授权用户实测、正式服务制品发布及 gitlink 更新。这些不能由本机同用户 socket 权限测试或 Local 实机测试替代。

## 11. Snapshot 审计修补（2026-09-09）

后续架构安排见 [配置追踪与 Snapshot Store Roadmap](../roadmap/2026-09-09-tracked-config-and-snapshot-store.md)。本节更新 §10 中的接口状态，原测试记录保留为当时结果。

- 删除 manager 锁内执行的 `ConfigSnapshotSink` 回调，改为 `subscribe_config_commits()` 返回 owned snapshot 的合并式订阅。不会将慢消费者、panic 或存储失败传播到核心事务；订阅只表示配置提交，当前运行状态仍需查询。
- 应用 `RuntimeSnapshotSink` 写侧 trait 收敛为具体 `RuntimeSnapshotStore`，通过生命周期启动参数注入，同一个实例承担读写。完整 reader/writer 能力拆分在后续 P1。
- 成功 reconcile 先记录 revision 与宿主 generation 绑定；effective 内容暂不可用时保留 pending。重新打开 inspection 只读补齐，不重跑 reconcile；较新绑定排除过期结果，失败候选不覆盖历史。
- CoreController 通过统一 `append_transition` 追加，复用 JSON Patch、changed fields 和图校验；空 diff 为 `None`。
- 进程差异改用显式字段比较；IPC 偏好解析后没有运行差异时返回 Noop，同时保留最新策略供 restart 使用。utils 的 Backoff 增加值相等比较。

本轮不实现完整 ConfigWorkspace 或配置批次 ChangeSet；也不改变已有服务发布与跨用户验证门槛。
