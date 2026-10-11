# Tauri → nyanpasu-core：GUI 与 CLI 共享 Rust 应用核心

> 本文定义目标架构、职责归属、阶段范围与验收要求，不记录实施流水账。
> 当前阶段状态、实际验证与剩余修正，见[阶段状态与剩余修正](tauri-core-remaining-corrections.md)。
> 保留 P01–P11 编号作为能力与交付索引；修订计划不等于重做已完成迁移，也不授权改写已发布提交。

## 1. 唯一主目标

**Tauri GUI 与未来 CLI 直接调用同一个 `NyanpasuClient` 和共享 Rust 能力。CLI 不通过新增的 HTTP/RPC 服务使用 core。**

```text
Tauri GUI                         未来 CLI
  Tauri IPC / 现有浏览器 HTTP       参数、终端输出、进程入口
  窗口、托盘、widget、原生交互       适合该宿主的具体 adapters
             \                   /
              直接调用 typed Rust API
                        |
               nyanpasu-core
          NyanpasuClient、领域操作、typed clients
          actor-owned state、事务、工作流、消费端口
                        |
       config / paths / runtime / traffic / geodata 等既有库
```

### 1.1 本次要做到什么

- 非 GUI Rust consumer 能通过公开 API 构造、调用、观察和正常关闭真实 core graph，不依赖 `AppHandle`、窗口、托盘、webview 或必填的 GUI 替身。
- GUI 与非 GUI 宿主复用同一套 profiles/config、runtime、代理内核、日志、流量和领域状态实现，不各写一套业务逻辑。
- GUI 产品功能留在宿主；core 不承担前端协议、显示投影或进程入口政策。
- 保留现有 desktop/browser/debug HTTP 功能及其协议、安全与关闭行为。取消独立 HTTP 服务目标，不是删除现有 HTTP 功能。

### 1.2 明确不做什么

- 不提取可独立部署的中立 HTTP/RPC 服务，不为此把命令注册表、`RpcOwner`、`IpcError`、Specta 导出或事件总线迁入 core。
- 不恢复旧 catalog/schema DSL、跨 crate 命令目录、signature alias 树或中立 registry/宏重设计。
- 不以当前任务交付 CLI、OpenWrt 或 mobile 产品；独立 Rust consumer 是验证载体，不是新产品。
- 不升级依赖，不改 profile 持久化格式、代理算法、应用更新渠道或已有前端 wire。
- 不重建 application/platform crate、第二套 resolver、通用 service locator、全局服务状态或兼容 re-export shim。
- 不因取消独立 HTTP 目标而整体回退 P01–P08，也不自动撤销或改写已经发布的 P09 提交。

## 2. 职责归属：按实际能力划界，不按是否使用 Tauri 划界

**没有 Tauri import、只使用普通 Rust 值，只能说明代码可以脱离 Tauri 编译，不能单独证明它应属于 core。** 同样，当前只有 GUI consumer，也不足以把真正共享的领域基础设施移回 GUI。

| 能力                                                                | 目标 owner                 | 边界说明                                                                      |
| ------------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------------------------- |
| `NyanpasuClient`、profiles/config、事务与 runtime 工作流            | core                       | facade 暴露领域操作，不暴露任意 service/ActorRef 查询                         |
| core/daemon 控制、Clash API、出站 HTTP、下载、代理内核更新          | core 及既有中立库          | 这些是领域执行能力，不是此次取消的前端入站 RPC 服务                           |
| storage、backup、配置 migration、日志存储、traffic、geo             | core 及既有中立库          | 复用真实实现与持久化约束；不因当前 consumer 单一而移回 GUI                    |
| system proxy、DNS、PAC 获取/校验/缓存与 OS URL 安装                 | core                       | 不把这些机制误称为本仓提供了本地 PAC server                                   |
| effects 调度、desired/revision/health、平台收敛                     | core                       | 只有一个 scheduler；GUI presentation 是可选端口                               |
| `PathResolver`、能力所消费的 FS/process/OS ports                    | 对应中立库或 core 能力模块 | 显式注入路径和宿主输入，不反查 GUI 构建环境                                   |
| 现有 HTTP listener、鉴权、静态资源、dev proxy、RPC/SSE adapters     | Tauri/浏览器宿主边界       | 可以不依赖 Tauri 类型，但不是共享 Rust 业务核心                               |
| `ipc.rs`、`unified_rpc.rs`、`IpcError`、owner/envelope、Specta 导出 | 宿主传输边界               | `ClientError`、`RuntimeError`、`ProfilesError` 等领域错误仍留 core            |
| frontend telemetry 批处理、净化、前端 owner 日志写入                | 宿主                       | 与 core 的 app/core 日志存储、轮转和查询能力区分                              |
| 前端远程 icon 缓存、data URL/显示相关投影                           | 宿主 presentation          | 共享网络客户端仍可复用，不把整个 network 能力移回 GUI                         |
| 窗口 session state、窗口几何、托盘/widget/主线程                    | 宿主                       | session 迁出必须同时处理真实版本、status wire 投影与退出前保存                |
| proxy cache、刷新/选择/失效与订阅                                   | core                       | frontend trimming 留宿主；不复制 actor/cache，不拿旧 snapshot 冒充 fresh 查询 |
| Tauri 应用自身的 updater、feed/mirror/release、安装/重启            | 宿主                       | 与代理内核 updater 分开，不能因已有 backend trait 就迁 core                   |
| bundle 发现、CLI parse、GUI 单实例、进程退出、全局 subscriber       | 宿主                       | 实际共享的机制按 consumer 单独评估，不整桶迁 `process`                        |

core 内继续按能力组织，不先创建未来无人使用的目录、trait 或 feature。现有 config、paths、runtime 子模块和 traffic/geodata 库继续复用，不物理吞并它们。

### 2.1 构造与依赖规则

- 真实 core 构造是 async，调用者提供 Tokio runtime；同步 `block_on` 仅在宿主边界。保留真实 facade 与事务闭包，不新增 `HeadlessClient` 替身或 assembly wrapper。
- identity、UA、版本/channel、路径、资源位置和支持能力由宿主显式提供，不能把 GUI 的构建常量换成 core crate 的版本。
- `NyanpasuClient` 不持有 Router、HTTP assets、鉴权或 Tauri handler；宿主 transport 调用 facade，不反向进入其构造条件。
- 外部 Rust consumer 可以注入可控的 core/daemon/OS adapters；这不同于为满足构造而伪造 window、tray、widget 或 main-thread 服务。
- 不把默认 OS/提权 adapter 宣称为已满足无人值守 CLI。能被 Rust 调用与适合某种宿主运行模式是两项要求。

### 2.2 Effects、观察与协议必须保留的契约

1. core 保留领域状态、平台 effects、revision/retry/health owner；宿主拥有 TrayView、locale/widget/native shortcut 执行与失败映射。
2. `PresentationEffectsPort` 缺席时保持 `Unsupported` / `PresentationUnsupported`、零 applied revision 和不自动重试；平台及日志效果照常执行，不用“全部 Healthy”的假实现掩盖缺失能力。
3. 不增加第二个 effects scheduler。保留组内顺序、locale-before-tray、同一 system proxy owner 的一次 reconcile、full/part 优先级与 stale revision 防护。
4. `EffectInvalidationSink::before_visual_apply()` 是同步 apply 前失效提示，不是 commit event。保留宿主旧 wire、门控及时序，不改成异步入队或统一 state-changed 事件。
5. 复用真实 owner 的 typed watch/broadcast、mutation receipt 和状态快照。不得从配置 watch 合成 mutation，也不为 CLI 新增中立事件总线。
6. 保留 stream/Channel/SSE owner、refresh-before-subscribe、lag/coalescing、detail demand 与释放语义。CLI 直接使用 Rust 观察接口；不以重做 HTTP 事件出口为其前置。
7. 保留 `rpc(http)` 显式 opt-in、命令名、query/mutation 分类、Result/null/camelCase、错误 payload、事件名和三份生成 bindings。owner 由可信 transport 授予，desktop label 与 `http:<UUID>` 分离，不能来自 body 自报。
8. HTTP 默认关闭、loopback、Host/Origin、token/cookie、weak routes、generation fencing、drain 均不因归属纠正改变。

## 3. P01–P08：保留共享核心迁移，不重复施工

下表是既有迁移的责任索引与需保留的设计，不是新的待执行文件搬迁清单。具体剩余增量以[剩余修正清单](tauri-core-remaining-corrections.md)为准。

| Phase | 保留的共享能力                                                                                          | 不应重新扩大或破坏的边界                                                                         |
| ----- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| P01   | runtime config 唯一 owner、tasks/blocking helpers、network/identity 输入、binary installation contracts | 不恢复 application/platform，不改变不同调用链的 UA，不重建 resolver，不迁无消费者的 trait        |
| P02   | storage/change subscriptions、consistent backup、配置 migration/legacy schema                           | host 负责产品入口与退出；同产品 CLI 不需要另造 strict Storage opener，既有 `web:` namespace 保留 |
| P03   | core/service control、版本契约/错误/parser、OS diagnostics                                              | 原 Tauri version reader 留宿主；不借 move 改路径选择、进程参数、IPC 或 caller budgets            |
| P04   | core/app 日志能力、Clash streams、connections、traffic/geo、direct-egress/network diagnostics           | frontend telemetry 与 icon cache 不因曾被迁入就继续留 core；它们由 P09 纠正                      |
| P05   | 代理内核 download/session/updater/manifest/installer ports                                              | 应用自身 updater 与 release policy 整体留宿主；安装目的地政策另行决定                            |
| P06   | system proxy/DNS、autostart owner/port、effects、可选 presentation                                      | 具体 autostart 配置/注册、tray/widget/native adapters 留宿主；保留同步 invalidation 和既有观察   |
| P07   | profiles FS/fetch/materialization ports、IO errors、content preparation 与原测试                        | 保留 fetch-before-commit、symlink/path validation、frozen contents、顺序和设备采集时机           |
| P08   | async `NyanpasuClient`、typed clients、真实事务/TCC/settlement/startup/recovery、jobs/proxy owners      | 不拆坏递归错误和事务闭包，不引临时 callback registry；GUI-only 残余按真实闭包单独纠正            |

### 3.1 不能在边界纠正中丢失的行为

- 持久化保留 backup-before-mutation、redb 一致快照、原 schema/fixtures、文件保护与失败策略。
- migration Runner 现有 stdout/stderr 有真实宿主 helper 消费；保留双流和退出结果，不为尚未提出的 quiet/machine-readable 需求重写输出。
- 控制面保留 endpoint generation、API lease、timeout 后 unknown/recovery 语义。现有 CoreClient/ServiceClient budgets 涉及真实 IPC/OS 边界，不因看到 ActorRef 首跳就机械删除；新的期限契约另审。
- 领域事务保留 Required vote、commit/degraded、receipt/compensation 与 owner-started work 语义。调用者离开不等于已开始工作取消。
- logger 的可复用 writer/rotation/filter 与宿主全局 subscriber/reload thread/guard 区分，不把机械迁移当成线程生命周期修复。
- 原测试、helpers、fixtures、字典/goldens、cfg 与 ignored 语义随真实 SUT 保留，不用少量 smoke tests 替代原回归。

## 4. P09–P11：按直接 Rust 复用目标收窄

### P09 — 纠正前端职责与 core API 的边界

**交付目标：** core 不再被前端 hosting、telemetry、icon cache 或前端协议适配占据；GUI/browser 原功能保持。

**状态：** R0–R5 已完成实现、组合定向验收、独立审查、人工审核和推送；[P09 PR #5717](https://github.com/libnyanpasu/clash-nyanpasu/pull/5717) 已接入原 stack，base 为 P08 #5707。此状态不等于已合并或全平台通过；验证细节和未运行项见[剩余修正 §2](tauri-core-remaining-corrections.md#2-当前状态与下一步的区别)。P10/R6、P11/R7 仍是后续独立范围。

本阶段按两组职责闭合，各自形成完整的原子变更：

1. **归属已明确的纠正：** HTTP hosting 回宿主；frontend telemetry 回宿主并保留真实 sink 注入；icon cache 回 presentation。生产调用者、原测试、manifest/lock、模块声明与文档随各自能力闭合。
2. **版本与查询接口闭包：** GUI session state 迁出时保持 status query/event 的真实版本与 final geometry drain；proxy 查询提供遵守原缓存新鲜度契约的未裁剪领域数据，宿主负责原 frontend projection。不能只删字段、填零或用 cached snapshot 替代查询。

`IpcError` 保持宿主 wire error；未交付的 core error 搬迁不继续作为 CLI 前置。已经发布的 HTTP 搬迁已通过增量提交纠正，未改写历史。宏实际所需的 `syn full` 是独立依赖声明问题，不与中立 RPC 设计混为一项。

**旧 P09-C2（中立命令/registry/宏重设计）和旧 P09-C3（独立 HTTP 事件总线）取消。** 本节不复用旧 C 编号给相反方向的工作重新命名，也不恢复 catalog/schema DSL。统一 RPC、inventory、`call_rpc` 和 Specta 列表继续属于现有宿主实现。

验收：真实 host consumers 编译；HTTP 原测试、telemetry 原测试/宏映射回归、owner/error/权限及 shutdown 接线通过；实际 exporter 证明 wire 保持。session/proxy 分别证明版本/新鲜度与原 GUI 行为，不把源码搬动或 metadata 成功当行为证明。

### P10 — 仅提取实际共享的启动准备

**交付目标：** GUI 与非 GUI Rust consumer 不必各自复制相同的领域准备规则，而不是把所有 `init/process/exit` 文件搬入 core。

- `utils/init/mod.rs::init_config`：提取真实配置初始化规则，保留 profiles 文件的 stamp、默认值、错误和已有文件不覆盖语义。
- `setup.rs::open_traffic_store`：将现有受保护的 traffic store 打开逻辑归入实际消费能力；保留失败降级和已有数据库保护，不能改成无条件打开/重建。
- 资源复制仅在有真实共享 consumer 时考虑提取；source/target 显式传入，bundle 发现及发布布局由宿主决定。它不是 CLI 使用 core 的一律前置。
- CLI 参数、选择当前 GUI helper executable、migration 子进程启动政策、home-dir 迁移中的产品/提权决策、GUI 单实例、OS session hook、进程退出与全局 subscriber 默认留宿主。另有共享需求时另立窄范围，不整桶迁移。

**P10 不接收 partial-start、constructor-future-drop、native join 或 logger/Windows callback 生命周期的兜底任务。** 这些是独立行为需求，不因取消 HTTP 服务或完成普通构造迁移而解决。

验收：同一初始化实现可由两个宿主消费；临时路径下的原行为与失败语义保持；共享准备不反查 Tauri bundle/dist 或 GUI executable。不得拿真实用户配置进行 home 迁移或系统修改验收。

### P11 — 验证直接 Rust consumer 与原有 GUI/browser

**交付目标：** 证明修正后的边界可用，接入持续验证；不承接遗漏的实现或另建 RPC 服务。

- 用独立 Rust consumer/公开 API 测试构造、调用、观察和正常关闭真实 `NyanpasuClient`。不借同 crate 私有 fixture 绕过公开可见性，也不依赖 Tauri 的 feature unification、源码树、GTK/WebKit 或 dist。
- 覆盖 profiles/config mutation、runtime 工作流、原领域观察与支持能力；外部 core/daemon/OS fake 的边界必须注明，不能据此声称 native 行为通过。
- 保留现有 desktop RPC、browser HTTP/SSE/Channel 的宿主测试与生成 bindings 验证。它们可以依赖 GUI 构建前提，但不属于 core-only 验收。
- 复用现有 backend boundary/architecture 任务，按实际缺口增加独立 consumer 检查与必要 CI 接线。计划中的新任务不能写成已经存在或通过。
- 平台、musl 与真实 OS/进程验证按能力分别报告；`cargo tree --target all` 不是全平台编译，正常 shutdown 测试不是 native join 证明。

发现漏实现，退回具体能力的原子修正；不能在 P11 用“集成”掩盖缺口。独立生命周期问题继续明确列为 OPEN，不写成 P11 已解决。

## 5. 执行与交付边界

### 5.1 依赖顺序

```text
保留 P01–P08 共享核心
         |
P09 前端职责纠正与 session/proxy 接口闭合
         |
P10 实际共享启动准备
         |
P11 外部 Rust consumer + 原 GUI/browser 持续验收

kernel 目的地政策、无人值守 adapter 需求、生命周期问题：独立决策，不自动塞入上述阶段
```

阶段表示责任归属，不要求重复创建历史 PR。正式交付仍使用明确的 base 和串行 stack；已获准隔离准备的多个候选不是已经整合的代码。候选间 `ipc.rs`、`setup.rs`、`client/mod.rs`、manifest 和 lock 的重叠，须在单独获准的整合步骤处理。

### 5.2 一个原子变更的范围

```text
能力实现与必要接口
+ 全部生产/测试调用者
+ 原测试/fixtures及include路径
+ 模块声明、manifest、Cargo生成的lock
+ 必要的宿主adapter与真实bindings导出
- 原实现、孤儿import和独占依赖
= 一个可构建、可测试、可回退的候选
```

不提交“先复制、以后切调用者”的中间态。领域错误与 foreign impl 随真实 owner 处理，不通过公开所有内部 selector、类型 alias 或旧路径 re-export 逃避调用者迁移。

每 cwd 一个 writer。构建与定向验证可串行，透明编译缓存不改变源码/target 的所有权；只释放本轮明确批准的本地构建产物，保留共享缓存和验证证据。资源安排不构成架构迁移任务，也不代表已获执行或清理授权。

按原子范围分别展示 diff 与实际验证，人工批准后才创建提交。提交、amend/rebase/cherry-pick、推送和合并不由“同意计划”自动授权。保护已有 dirty candidate；既有已发布提交默认保留，CI 红灯（包括单平台）不得合并。

## 6. 验证与完成条件

执行方法遵循[测试与审查](../development/testing.md)、[工作流](../development/workflow.md)、[统一 RPC](../development/rpc.md)与[后端包边界](../development/backend-packages.md)。本地运行变更相关的定向测试，全套测试交给 CI；不以旧测试数字代替新候选结果。

| 层次                                                                                       | 必须证明                                                | 不能据此声称                            |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------- | --------------------------------------- |
| 源码/归属复核                                                                              | 唯一 owner、调用闭包、测试保留、无 shim/新 GUI 依赖     | 已编译或实际行为通过                    |
| Cargo 依赖解析/锁比较                                                                      | 完整外部 package records 保持，直接依赖随 consumer 迁移 | 所有 features/targets 可编译            |
| core/host 定向 check、Clippy、format 与测试                                                | 真实消费者编译、相关行为回归和风格检查                  | 全套 CI 或所有平台通过                  |
| 实际 `specta_export::tests::export_typescript_bindings`                                    | 真实生成三份 bindings，核对完整差异                     | 仅靠手工 JSON/类型检查就证明协议等价    |
| `deno task lint:backend-boundaries`、`test:backend-boundaries`、`lint:architecture-ledger` | 依赖方向与现有静态规则成立                              | CLI/native 生命周期已经验收             |
| 独立 Rust consumer                                                                         | 公开构造与领域 API 真正可达，不需要 GUI/入站 RPC        | 所有具体 OS adapters 已适合无人值守 CLI |
| GUI/browser 与平台验证                                                                     | 对应环境中的真实 transport/OS 行为                      | 未运行的环境或独立 OPEN 问题已解决      |

测试 filter 必须定位到真实新 owner，0 tests 不能记为通过。生成文件只由实际 exporter 更新；有非预期差异应停止解释，不手改结果强行匹配。原 ignored/browser/平台测试的未运行状态如实保留。

**本迁移目标完成的条件：** GUI 与非 GUI Rust consumer 使用同一真实应用实现；前端职责不再成为 core 的必填依赖；现有 GUI/browser 功能、wire 和持久化行为保持；相应定向验收与持续门禁有实际结果。独立生命周期和产品政策限制必须列明，不能通过删除计划条目或把工作推给 P11 将其标为完成。
