# Tauri/core 分离：剩余修正清单（待人工审核）

> 整体目标与阶段定义见[迁移计划](tauri-core-extraction-plan.md)。本文只回答：**现在还要改哪里、为什么、完成到哪一层、怎样才算验收。**
> 本文是基于 P08、已发布 P09-C1（`16fca2eda5c0b0a3c2485aaba45c923cf5f9e0e2`）及后续未提交候选的审阅快照，不代表默认分支状态，也不是实时 CI 面板。
> 审核这两份文档不等于批准执行构建、清理、继续源码实现、整合候选或创建提交。

下文源码路径以 `backend/` 为根；候选中的目标路径不代表已经整合到上述提交。

## 1. 先给结论

1. **P01–P08 的共享领域能力主体保留，不整体回退，也不再搬一遍。** CLI 直接使用 `NyanpasuClient`，不需要新增独立 HTTP/RPC 服务。
2. **HTTP hosting、frontend telemetry、icon cache 三项已经有源码候选。** 当前缺的是针对各候选的实际编译、测试、真实导出和批准后的整合，不是继续设计第三套框架。
3. **还有三个明确的代码切口未实施：** GUI session state 的完整迁出闭包、proxy 的 fresh/raw 查询与前端投影分离、少量共享启动准备。它们分别属于 P09 后续与 P10，不混入前三个候选。
4. **还有独立 Rust consumer 验收待建立。** 这是 P11 的工作，不是实现 CLI 产品，更不是重新提取 HTTP 服务。
5. **内核更新目的地、无人值守宿主要求、生命周期问题须单列。** 不把尚未决定的产品政策写成已经确定的迁移，也不把已发现的关闭风险隐藏到 P10/P11。

## 2. 当前状态与下一步的区别

| 编号 | 项目                                       | 已有状态                                                | 真正还缺什么                                                                 |
| ---- | ------------------------------------------ | ------------------------------------------------------- | ---------------------------------------------------------------------------- |
| R0   | 未提交 `IpcError` 候选与 `syn full` 的处置 | 两者混在原工作区未提交改动中；不在下面三项候选内        | 停止交付 core wire-error 搬迁；另行批准其处置，独立保留宏的真实 feature 需求 |
| R1   | HTTP hosting 回宿主                        | 源码/原测试搬迁、静态复审、Cargo 锁协调完成             | host 编译、原 HTTP/RPC 回归、实际 exporter、人工审查及整合                   |
| R2   | Frontend telemetry 回宿主                  | 源码/原测试/调用者接线及静态补审完成                    | core/host/macro 编译与回归、实际 exporter、人工审查及整合                    |
| R3   | Icon cache 回 presentation                 | 源码/调用者搬迁、静态复审、Cargo 锁协调完成             | 编译、真实 RPC/导出路径验证、人工审查及整合                                  |
| R4   | GUI session state 迁出 core                | 归属与版本耦合已识别；未实施                            | 先审 query/event/version/退出保存闭包，再一起改动                            |
| R5   | Proxy raw/fresh API 与前端投影分离         | 缓存与 projection 混合已识别；未实施                    | 增补必要的 typed 查询闭包、迁移投影及其测试，更新真实 consumers              |
| R6   | 共享启动准备                               | 已定位 `init_config` 与受保护 traffic opener；未实施    | 两个窄能力按实际 consumer 提取并保留失败/保护行为                            |
| R7   | 独立 Rust consumer 验收                    | 公共 async 构造与 facade 已存在；新目标的外部验收未完成 | 真实外部使用证据及必要持续验证接线                                           |

**R1–R3 不是三份已整合提交。** 它们是从相同基线分别准备的独立候选，不能把某一份 worktree 的通过结果算成另外两份也通过，更不能据此宣称合并后的组合已验证。

本轮离线准备已经补齐 runtime 与嵌套依赖。HTTP/icon 各一次获准的离线 Cargo metadata 已成功并完成锁比较：外部 package records 未变，仅对应能力的 workspace 直接依赖 owner 改变；telemetry 锁字节未变。这些只证明依赖解析/锁协调，不是构建、features、运行或 wire 验收。

**R1–R3 当前候选尚未执行新的 build/test/export/format/boundary 验证。** 过去阶段的通过结果不能填入这里。此前准备失败的记录也不会因为后续限定恢复成功而变成首次全绿。

## 3. 已确定归属：收口现有候选，不再扩大设计

### R0 — 不继续交付 core `IpcError`；独立处理宏 feature

**为什么：** `IpcError` 的 `kind/message/detail` 属于前端协议，CLI 直接调用 Rust API 不需要它。`ClientError`、`RuntimeError`、`ProfilesError` 等领域错误则继续留 core。

**涉及位置：** 未提交的 `nyanpasu-core/src/transport/rpc/{mod,error}.rs`、`transport/mod.rs`、宿主 `ipc.rs` / `unified_rpc.rs`，以及独立的 `nyanpasu-macro/Cargo.toml` 修改。

**还要做：**

- 不把 core error 搬迁叠入 R1，不在 HTTP 移出后为了兼容旧候选再重建 core transport 层。
- 对原未提交候选的撤回/保留先取得明确授权；不能借更新文档或整合其他候选覆盖它。
- 原宏确实使用 `ItemFn` / `FnArg` / `Pat` 与 `File` / `parse_file`；`syn full` 按真实依赖声明独立保留，不因为取消 RPC 重设计一起丢弃。
- 宿主 wire/error 测试保留；foreign error 转换若确需改变，逐个核对原 Display/Debug、source 与分类，不顺手统一错误框架。

**完成条件：** 候选基线与处置范围明确，core 不导出前端 error 协议，宏 feature 有独立编译/测试证据，宿主错误 payload 不变。这里是两项处置问题，不要求捆成一个代码提交。

### R1 — HTTP hosting 回到现有宿主

**为什么：** 这是 GUI/browser/debug 入口，不是 GUI 与 CLI 共用的业务用例。普通 Rust/Axum 实现也不因此成为 core 职责。

**候选已改：** `nyanpasu-core/src/transport/http/mod.rs` 回到 `tauri/src/server/debug_http.rs`；更新宿主 callers、模块和直接依赖。原五个测试随真实 SUT 搬迁，没有换成模拟 Router。

**仍需验收：**

- host 默认及 dev-proxy 相关 feature 下的实际消费者能编译；core 不再负责该 listener。
- embedded assets/SPA/404、start/stop 与 foreign Origin、打开的事件流关闭、缺失前端保持禁用、dev proxy query/status/WebSocket 协议等原测试保持。
- 原 host RPC 权限、owner、错误映射和 HTTP opt-in 回归；默认关闭、loopback、Host/Origin、token/cookie、weak routes、generation fencing 和 drain 不变。
- 运行真实 bindings exporter，确认命令清单/顺序与三份 bindings 没有非预期差异。

**不做：** 中立 Router 工厂、新的 HTTP registry、CLI HTTP 模式或新的 HTTP event-bus。已发布的旧 P09-C1 不自动改写历史。

### R2 — Frontend telemetry 回到宿主并保留实际 sink 注入

**为什么：** 前端错误批量上报、字段净化及前端 owner 标记是宿主协议行为；不是 core 的 app/core 日志能力。

**候选已改：** `nyanpasu-core/src/logs/frontend.rs` 回到 `tauri/src/desktop/frontend_events.rs`；移除 core 的 frontend sink/forwarder 和 `LoggingSetup.frontend`；宿主用最小 `FrontendTelemetry(Arc<dyn FrontendLogSink>)` 接线。

同一个实例用于 setup/manage 与 `RpcDependencies`，原宏只增加所需 State mapping/allowlist 及回归。实际 RPC fixtures 继续注入 `RecordingSink`，不是为了迁移而把 browser 路径换成固定 tracing sink。frontend-error-reporting 文档中的测试 owner 已在候选中修正。

**仍需验收：**

- 七个原净化/批处理/structured tracing 测试在新 SUT 上执行，原断言、helpers 与条件保留。
- 原宏回归及新增 State 映射回归、desktop/HTTP 上报与 `RecordingSink` 路径实际通过。
- core 普通日志初始化不再要求前端 sink；host owner 的来源、错误返回和 batch wire 保持。
- 实际 exporter 核对三份 bindings；不把一次 State 支持扩成通用跨 crate RPC 框架。

### R3 — Icon cache 回到 presentation

**为什么：** 该缓存服务于前端 icon 显示，不是代理控制或领域缓存。

**候选已改：** `nyanpasu-core/src/icons.rs` 回到 `tauri/src/desktop/icon_cache.rs`，IPC callee 和 bytes/postcard 直接依赖随 owner 迁移。core 的共享 network 能力及仍有消费者的 sha2/hex 不跟随迁走。

**仍需验收：** host 和实际 IPC 调用编译；URL/hash、七日 TTL、MIME/data URL、postcard 与下载行为保持；运行真实 exporter 确认公开 wire 不变。

**覆盖限制：** 原模块没有单元测试。本候选不能宣称“原 icon 单测全部通过”，也不能把编译/导出等同于已覆盖 TTL 或外网下载。若审阅要求新增行为测试，另列最小测试范围，不伪造既有 coverage。

## 4. 明确还要改代码，但必须按完整调用链实施

### R4 — GUI session state：不能只搬 actor 或删除一个字段

**当前问题：** `nyanpasu-config::state::PersistentState` 当前只有 `window_state`，但窗口 session 的 owner 和 facade 仍在 core。其版本又参加 `ConfigurationStatus.source_versions.session` 和 `event_seq`，不是无人消费的字段。

**主要切口：**

- `nyanpasu-core/src/state/session_state.rs`、`client/session_state.rs`、真实构造/关闭接线；
- `client/configuration_status.rs` 与宿主 status query/event 投影；
- 窗口几何保存、`tauri/src/utils/exit.rs` 的 final geometry drain，以及现有前端消费契约。

**建议修改：** 窗口 session owner/保存操作归宿主；core 返回自身领域状态，宿主在原 query 与事件路径组合真实 session 版本。通用持久化机制不必随窗口状态整体迁回宿主，DTO/schema 的调整限于该闭包。

**实施前必须审定：**

1. 原 query 和事件如何取得同一个真实 session 版本，保持 `event_seq` 更新语义；不能填 `0`、删字段或仅改 query 不改事件。
2. 退出时如何保存最后窗口几何并等待 session owner 完成；不能把 queued save 当已持久化。
3. 持久化文件/旧数据、error payload 与生成 bindings 如何保持兼容，core 构造如何不再要求 GUI session。

**完成条件：** 非 GUI consumer 不构造窗口 owner；GUI 版本与事件仍来自真实变化；最后一次 geometry 保存有实际完成证明。整个闭包单独审查，不塞进 R1–R3。

### R5 — Proxy：保留领域 owner，只迁前端裁剪

**当前问题：** `nyanpasu-core/src/client/clash_api.rs` 的查询会调用 `clash/proxies.rs::trim_for_frontend`。领域缓存与刷新应共享，前端裁剪不应成为所有 Rust consumers 的唯一结果。

**关键区别：** 现有 `proxies_snapshot()` 是 raw **cached** snapshot，不是 fresh、未裁剪查询。用它替换原 async refresh 查询会改变行为。

**建议修改：**

- actor/cache、endpoint lease、generation、刷新/选择/失效及订阅继续由 core 独占。
- facade 提供基于同一 owner、保持原刷新契约的未裁剪 typed 查询；只增加真实消费者需要的入口，不公开任意 actor access。
- `trim_for_frontend` 与其测试归宿主 projection；desktop/HTTP query 和 tray 等真实 consumers 按各自原语义接线。
- 保留 refresh-before-subscribe、默认 latency URL、detail demand、lag/coalescing、错误及失效行为；不能复制第二份 cache，也不能让 GUI 突然收到完整 raw payload。

**完成条件：** fresh/raw 与 cached snapshot 的含义明确，Rust consumer 不被迫接受 UI 裁剪；GUI 返回值保持原 wire 与裁剪语义，原缓存/失效测试与投影测试均保留并执行。

### R6 — P10 的实际共享准备只有窄范围，不是 process 大搬迁

**已确认入口：**

| 入口                                       | 应提取的内容                      | 必须保留                                               |
| ------------------------------------------ | --------------------------------- | ------------------------------------------------------ |
| `tauri/src/utils/init/mod.rs::init_config` | 与 GUI 无关的初始配置文件准备     | profiles stamp、默认内容、已存在文件不覆盖、错误上下文 |
| `tauri/src/setup.rs::open_traffic_store`   | 实际受保护的 store 打开与降级规则 | 现有数据库保护/失败策略，而不是改成盲目直接 open       |

路径和能力输入显式传入；两类宿主使用同一个实现。正式模块位置按现有消费能力确定，不先创建一个包揽所有入口的 bootstrap/process 框架。

**不自动包含：** bundle 发现、当前 GUI executable 的 migration helper 选择、GUI 单实例、产品 home-dir 迁移与提权、全局 subscriber、OS session/exit hooks。资源复制只有真实共享 consumer 需要时才单独评估，输入为显式 source/target。

**完成条件：** 原 GUI 行为不变；外部 Rust consumer 能复用真实准备逻辑；临时路径下验证已有文件、错误和数据库保护，不以真实用户目录做迁移试验。

### R7 — P11 必须增加真实外部 Rust 使用证据

`ClientSetupArgs`、async `try_new_with_args` 与 typed facade 已存在；不再创建另一套“无 GUI client”。需要验证的是公开可用性和真实 graph，不只是 grep 没有 `AppHandle`。

**还要补：** 独立 Rust consumer/公开 API 测试，从路径与支持能力输入到构造、领域 mutation、观察、正常关闭，不能访问 core 私有测试 helper，也不能借 Tauri dev-dependencies/feature unification 才能通过。

同时保留原 GUI/browser transport 验收，使用真实 Specta exporter。平台测试、外部系统 fake 的覆盖边界、ignored browser case 与未运行项分别报告。CI 只接所需门禁，不建设第二套 harness。

**完成条件：** core-only consumer 不需要 GUI 构建前提或入站 HTTP；实际复用领域实现；原 GUI/browser wire 与行为仍通过相应验证。若测试暴露 R4/R5/R6 的漏实现，回到对应修正，不在 P11 隐藏补业务。

## 5. 待决策/独立 OPEN：不先假定解决方案

### D1 — 代理内核更新的目的地政策

`PathResolver.install_dir` 已存在，binary lookup 已有 data → install → development 顺序，不需要再加同义字段。`nyanpasu-core/src/client/mod.rs` 的默认更新 installer 仍有从 `std::env::current_exe()` 推导目录的路径。

**要决定：** CLI 应更新当前选中的 binary、安装目录 binary，还是明确指定目标；当 data override 与 install 同时存在时如何保持查找/更新一致。确定后再复用现有 resolver/installer port 更新真实 callers，不能先把 `current_exe()` 全部替换成另一个猜测目录。

这不等于把 Tauri 应用 updater 迁入 core，也不阻塞先验收 R1–R3。

### D2 — 未来 CLI 是否需要无人值守的 service control

现有 `ServiceHostAdapter` 路径包含 Windows `gui(true)` / `show(true)` 和 macOS osascript 交互。当前“typed Rust 可调用”不证明“无人值守可运行”。

若未来 CLI 要求无交互运行，需先决定提权/失败策略与注入位置，再补窄 adapter 行为及平台测试；没有该需求时不为了抽象纯度重写整个 service/control 层。core/daemon 协议不是本轮取消的前端 RPC。

### D3 — 生命周期与 native 所有权问题仍独立 OPEN

这些不因 HTTP 移回宿主而闭合，也不是把文件搬完就能证明：

- partial-start 失败路径与 constructor future 被 drop 后的 owner 清理；
- shutdown/panic、caller budgets、已开始工作与结果等待之间的契约；
- native thread/process 的实际 join，而非仅 timeout、取消 token、abort 或 Drop；
- `MainThreadHandoff` / `ExitGate` 的接受、完成和主线程边界；
- logger reload/parked thread、Windows callback/thread ownership；
- shutdown driver 不得等待包含自身的 tracker。

相关切口包括 core `client/app_lifecycle.rs`、`tasks/mod.rs`，宿主 `desktop/event_sink.rs`、`desktop/main_thread.rs`、`utils/exit.rs` 与 logger/Windows hooks。先针对真实问题定义可复现契约和所有权，再决定最小修复；不引通用 admission/terminal/watch 框架掩盖问题。

**不作虚假完成声明：** R7 的正常构造/关闭验证不能关闭上述异常与 native 情形。它们的实施优先级和是否阻塞具体产品发布应单独评审。

## 6. 不再列为剩余工作的项目

- 不重做 P01–P08 domain/actor/DI/facade；不把 core 中所有普通 Rust helpers 都迁回宿主。
- 不重启中立 RPC registry、catalog/schema DSL、独立 HTTP host 或 HTTP event-bus 提取。
- 不继续把前端 `IpcError` 当 CLI 前置；也不把领域错误一并迁回宿主。
- 不为同产品 CLI 新建 strict Storage opener；现有宿主 `web:` namespace 不重复建设。
- 不为未提出的 quiet/machine-readable 模式重写 migration Runner 的 stdout/stderr。
- 不重复增加 `install_dir`；不声称现有 PAC 流程是本地 PAC server。
- 不把 Tauri application updater、release/mirror 规则当共享内核 updater。
- 不以 feature flags 包装错误归属，不为 P10/P11 新建通用启动/关闭框架。

## 7. 建议审核顺序与后续门槛

1. **先审核这两份文档。** 确认主目标、R0–R7 的范围以及 D1–D3 的独立状态。此刻暂停源码续行与验证/清理安排。
2. **批准 R0 的具体处置与 R1–R3 的验证清单后，按 HTTP → telemetry → icon 串行验收。** 不再重复实现已完成的源码候选；每项分别报告 diff、实际命令/结果与未运行项。
3. **每个候选人工审查后，另行批准提交与整合。** 重叠调用者/manifest/lock 需要真实组合复核；三个独立通过结果不能替代整合检查，不覆盖原 dirty candidate 或改写已发布历史。
4. **R4、R5 先审接口闭包，再实施；R6 以两个真实准备入口为限。** 不将未决定的 kernel/提权或生命周期问题混入。
5. **R7 验证最终公开 Rust 使用与原 GUI/browser，CI 按相关范围补充。** 发现问题归回对应原子范围，不将“全套 CI 未跑”写成全部通过。

验证/依赖安装/产物清理另列可执行清单并获批。真实 exporter 需要其实际前端工具和构建前提，不能手写 bindings 或用假的 exporter 绕过；透明缓存由环境处理，不调查或修改其配置，不共享 Cargo target。

**尚存的发布限制：** 上次已知 P06 Windows CI 在 `boa_utils::module::http::test_http_module_loader` 因 `YAML content should contain 'packages' field` 失败，本轮未重新查询、修复或重跑；不能猜测响应原因，也不能把 Linux 定向通过当作该红灯解除。全套 CI 与平台结果必须和本地定向结果分开，红灯仍禁止合并。

本文没有宣称任何新候选编译、测试、导出或 CI 已通过。它的用途是先审清“还缺什么”，不是批准下一轮执行。
