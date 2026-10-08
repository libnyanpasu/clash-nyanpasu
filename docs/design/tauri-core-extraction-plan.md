# Tauri → nyanpasu-core：按可合并 PR 重新设计迁移

> 本文仅维护整体迁移计划：目标架构、阶段依赖、能力归属、原子提交范围与验收要求，不记录实施进度、提交状态或已执行的验证结果。
> 本文重新编号，不继承旧设计的 Phase 00—11、A/B/C 或完成百分比。
> 一个 Phase 对应一个 PR；一个 PR 可以包含多个独立可构建的原子 commit。

## 1. 目标、基线与范围

**目标不是移动一个 facade 文件，而是让非 GUI 宿主直接使用同一套应用实现。**

最终 `backend/tauri` 只拥有 GUI 产品装配、Tauri IPC/插件适配、窗口/webview/托盘/widget/主线程及原生交互。共享业务、状态机、持久化、网络、进程、OS 能力，以及不依赖 GUI 的 HTTP/RPC 实现，移入 `nyanpasu-core` 的具体能力模块。已在独立库中的实现继续复用，不物理吞并 `nyanpasu-config`、`nyanpasu-paths`、runtime 子模块、traffic/geodata 等库。

### 1.1 复用边界与基线选择

| 复用的既有能力                                                    | 对应迁移责任                                                   |
| ----------------------------------------------------------------- | -------------------------------------------------------------- |
| core 的 state transaction/format                                  | application/clash/profiles/session 的具体 owner 与事务接线     |
| `runtime::config` 的 builder、builtin、FS/JS/Lua adapters及原测试 | runtime snapshot/inspection、准备、TCC、恢复、facade           |
| core 的 connections rates、service compatibility                  | streams/proxy cache、service actor/control、完整生命周期       |
| core 的 diagnostics DTO/port、device source                       | diagnostics OS/process adapter与剩余身份输入                   |
| `nyanpasu-paths::PathResolver`                                    | shared init、资源复制、migration process、单实例及 OS shutdown |
| backend dependency gate                                           | 独立 headless 构造、观察、关闭和无 GUI 的完整 HTTP 服务        |

优先复用已有实现及其行为和测试保障，按真实消费关系调整模块归属、必要的可见性与调用路径。不为迁移新增抽象或重写业务逻辑；独立行为变更须另行明确审批。内部breaking change应一次迁完调用者，不加旧路径shim。

**启动实施前的 B0：** 盘点当前工作区、分支提交和共同目标分支，选定重整所依据的可复现源码状态及PR base。**不要求现有修正先按旧计划提交/合并，也不要求先丢弃它们。** 未提交成果可直接重组到下表对应PR；未合并的旧stack按能力重新归组；已在目标分支的实现只提交必要增量，不反复搬出再搬入。B0只是实施准备，不是额外PR或旧设计审批关卡。

多worktree开始写代码之前，其共同前置仍须成为一致的、可构建的base；不能从dirty checkout只取HEAD却假定拥有全部工作区变更。实际快照/提交/历史操作在实施时确认，不擅自stash、reset或覆盖当前改动。允许重整代码不等于授权改写已推送历史；后者仍须单独确认。

| 需保留的能力与约束                                               | 新设计的吸收/重整责任                                | 不应丢失的行为                                                                              |
| ---------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| runtime config、builtin、FS/script收敛及application/platform删除 | P01建立唯一owner；P07/P08可为真实消费边界继续调整API | 单次序列化、snapshot graph、build/preparation错误分层、JS/Lua顺序及private blocking runtime |
| BuildInfo/channel/订阅device与剩余host inputs                    | P01按实际消费者重整，不延续旧“任务1/2/3/4”提交分组   | 真实宿主版本/channel、设备采集时机、实例缓存、原header语义                                  |
| paths/resolver与host_paths接线                                   | P01复核接口，P03/P10按执行能力调整消费者             | 一次发现、显式roots、可失败install路径、现有binary查找语义；不重建第二套resolver            |
| connection rates、service compatibility                          | P04/P03随能力调整归属和调用                          | 原算法、最低版本规则、fixtures                                                              |
| workspace/lock/gates/musl harness/说明                           | 随所属能力的原子commit重组，P11做最终矩阵            | 唯一实现、无传递GUI依赖；不把原局部musl覆盖宣称为完整headless覆盖                           |

如果重整表明某个现有抽象应被合并、拆开或删除，按其新消费者完整调整，而不是因为它“已经实施”就排除在规划之外。没有实际边界收益的改名或重写仍不做。

### 1.2 明确排除

不新建 CLI/OpenWrt/mobile 产品，不修改 profile schema、代理算法或发布渠道语义，不升级依赖，不重写 ractor 框架，不新增万能 service locator、全局状态或 migration compatibility shim。迁移造成的孤儿必须当场删除；预先存在的死代码不借机清理。

现有文档不作为本次架构划界的结论。与本方案冲突的文档在对应实施 PR 同步；本次只记录需要同步的地方。显式 DI、串行 owner、窄端口、真实结果等待和原子提交等工程约束仍适用。

## 2. 迁移闭包与边界风险

### 2.1 旧任务拆分为什么不能直接执行

1. **Phase 不是 PR 单元。** 同一编号内有多条不同依赖的 A/B/C，后来又有任务级前置；很难判断该 Phase 的 PR 应基于谁、何时可合并。
2. **将文件清单当作迁移闭包。** “先搬 runtime DTO”没有证明其 errors/ports/impl/test helpers 已经闭合；DAG 在文档里无环不等于 Rust crate 能编译。
3. **并行只分了目录，没有分接线。** `client/mod.rs`、`setup.rs`、manifest、宏、bindings 和 `client::tests` 是实际冲突中心。
4. **独立宿主验收太晚。** GUI-free trait 不等于能构造 core；必填假 window/widget、Tauri 测试 runtime、从 Tauri 转发出来的 HTTP events 都可能让纸面迁移通过。
5. **历史状态和未来计划相互覆盖。** 已删除的路径、旧 PR head 的 API、当前未提交实现混在任务表里，容易重复迁移。
6. **机械迁移与行为边界修改没有分开评审。** effects、通知、shutdown、版本进程和宏协议都需要具体契约，不能在最终“集成/清理”中补做。

### 2.2 关键依赖与迁移责任

| 证据位置                                                                  | 实际问题                                                                                               | 本方案处理                                                                    |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| `client/mod.rs::try_new_with_args/with_parts`                             | 同时 block_on、创建 domain owners、HTTP server、app updater、streams/geo/traffic；依赖 AppImage/bundle | P05/P06 先拆 capability；P08 async bootstrap与真实 facade一起切换；P09迁 HTTP |
| `state/mutation.rs`                                                       | `MutationCoordinator` 持有具体 `ApplicationWorkflowClient`；source actor依赖工作流参与者               | 留在 P08 同一事务闭包，不造临时总线/回调注册表                                |
| `client/runtime.rs`、`state/profiles/error.rs`、`client/runtime_error.rs` | `DegradationReason/ProfilesError/CommitAborted/RuntimeError/preparation` 递归引用                      | P07只抽独立 IO 错误；其余 P08一起迁，不先公开整套内部错误构造器               |
| `core/proxies.rs`                                                         | proxy selection返回共享 `MutationOutcome`，不是纯 cache 叶子                                           | 整个 proxy owner随 P08；不拆出第二份 outcome                                  |
| `client/core_lifecycle/ports.rs`                                          | binary artifact/install契约与 runtime preparation契约混在同文件                                        | P01只抽 binary契约；runtime部分随 P08                                         |
| `service/profile_file.rs`                                                 | 依赖 profiles ports/errors、UA、device；测试依赖 `client::tests`                                       | P07完整迁移端口+IO错误+实现+原测试；不依赖 facade测试模块                     |
| `client/ui_effects/ports.rs`                                              | `WidgetIpcError/StatisticWidgetVariant` 来自 egui                                                      | GUI widget实现/错误留宿主；消费契约只接收中立数据与映射后的结果               |
| `client/event_sink.rs`、`unified_rpc.rs::bridge_tauri_events`             | main webview存在才 emit；HTTP再监听Tauri事件                                                           | P06提供真实 owner观察；P09让两种transport直接消费同一观察源                   |
| `unified_rpc.rs::CommandEntry`、宏 `unified_command.rs`                   | 一个表混合 HTTP handler和 `AppHandle/Window/Webview` handler；宏硬编码host路径                         | P09拆中立表与desktop binding，宏与全部消费者同提交更新                        |
| `core/*/tests`、`client/*/tests`                                          | Tauri block_on和host `client::tests`反向依赖                                                           | 测试跟owner迁移；fake下沉到使用它的最小能力测试模块                           |
| `utils/core_version.rs`                                                   | 版本查询仍用Tauri shell sidecar，不能照搬为core adapter                                                | P03只迁契约/错误/parser；Tauri reader留host，保持shell执行语义                |
| `shutdown_hook.rs`                                                        | 非GUI OS会话检测使用两个全局mutable statics                                                            | P10改实例owner/回调上下文后迁移，不能把allowlist整条平移                      |

上表路径均相对 `backend/tauri/src/`，宏文件位于 `backend/nyanpasu-macro/src/`。

边界约束：保留CoreClient/ServiceClient 的现有 caller budgets。它们限制调用者等待；mailbox residence 消耗预算，但预算不保证任意队列延迟或外部动作完成时间。C1 保留它们及原有 timeout/unknown/recovery 契约。ActorRef 首跳不等于整条能力纯进程内；ServiceEndpoint 使用真实 IPC，ServiceClient 的 adapter 执行 OS/提权命令。通用进程内调用规则不授权删除这些既有边界预算。部分旧测试使用 sleep；logger reload线程结束接收后永久 park，P10负责收敛涉及的资源所有权，使用独立行为 commit。若需要触碰 upstream IPC，必须先确认依赖交付方式，不擅自改 runtime 子模块。无关 sleep清理不扩张到整个测试库。

## 3. 最终架构与边界决策

### 3.1 目录与依赖方向

建议能力归属如下；只在迁移真实实现时创建模块，不先建空目录。现有 core 模块直接扩展，不创建 `core/core`、第二套 state machinery 或 application/platform crate。

```text
nyanpasu-core
  client/                 NyanpasuClient及其领域操作
  bootstrap/              async组装、启动、后台源、关闭
  state/                  现有事务基础 + app/clash/session owner
  profiles/               profiles owner、ports、FS/fetch/materialization
  runtime/                config（已有）、工作流、准备、snapshot/recovery
  control/                core endpoint/router、API lease、local host
  service/                compatibility（已有）、daemon actor/OS adapter
  effects/                desired/revision/health、平台收敛、可选宿主效果契约
  system_proxy/           OS proxy/PAC/autostart
  system_dns/             DNS能力
  connections/ proxies/   中立流、连接规则、proxy owner
  logs/ traffic/ geo/     日志、流量与地理索引
  storage/ backup/ migration/
  updates/                kernel/ 与 application/，不混淆安装对象
  download/ network/ icons/ diagnostics/
  process/                实际共用的执行/提权/实例锁/OS shutdown机制
  transport/              rpc/、http/；不进入domain依赖方向

Tauri host
  GUI builder/插件/managed state/事件循环
  原生窗口、托盘、widget、clipboard/dialog、快捷键插件adapter
  Tauri RPC dispatcher / Channel / Event wrappers / Specta导出
  bundle/WebView2/APPIMAGE/资源发现/安装器adapter
  消费core bootstrap、capability clients和观察，不实现共享业务
```

**运行方向：** `transport → NyanpasuClient → domain owners → ports`。
**构造方向：** host选择能力和具体adapter，调用core bootstrap。core内部transport可消费facade；facade不持有Router、assets、HTTP鉴权或Tauri handler。

不把所有adapter都强制放一个 `platform` 桶；FS、HTTP、process具体实现归消费能力，只有真正共用的实现才提升到明确的共享模块。

### 3.2 不沿用旧白名单的三个决定

- **HTTP在范围内。** `debug_http.rs` 的server actor、loopback监听、token/Host/Origin校验、SSE、静态asset接口、dev代理都可以不依赖Tauri。放core的transport adapter模块，而不是业务层。Tauri AssetResolver和dev URL读取留host，向其传普通值/asset port。HTTP默认仍关闭，迁移绝不扩大 `rpc(http)` allowlist。
- **应用更新按状态机与安装后端拆。** `AppUpdateClient` 的检查/下载/取消/进度/状态规则可以通过现有backend契约运行，迁入 `updates::application`；feed/mirror/版本比较的纯规则也迁入并接收显式产品输入。Tauri plugin的包上下文、签名验证实现、平台installer、关GUI/重启以及WebView2 target选择留adapter。headless不配置该能力时明确 unavailable；不假设未来CLI与GUI使用同一feed，更不新增通用自更新框架。
- **“UI会调用”不是保留理由。** icon下载缓存、proxy env文本、frontend error batch净化/日志写入、会话几何数据持久化均可由中立能力拥有；实际展示、剪贴板写入、JS捕获、窗口尺寸应用仍属于GUI。反之tray绘制队列、窗口URL构造、widget展示生命周期，即使只操作普通值，也仍是GUI。

上述边界按实际消费关系确定，不以历史目录或白名单替代。

### 3.3 Bootstrap必须可以真正不用GUI

- 共享构造是async，调用者提供Tokio runtime；只有Tauri同步setup边界允许block_on。不得在core内另建全局runtime来替换Tauri runtime。
- `PathResolver`仍由host一次发现后传入；resources source与target显式传入，不给resolver增添bundle/GUI字段。
- identity仅传实际消费的version/channel/app name/UA等值。尤其不能把原GUI `env!("CARGO_PKG_VERSION")` 原样移入core，导致请求头变成库版本；保留各调用链当前不同的UA字节格式。
- platform能力和presentation能力分别可选。缺失能力不启动actor、不注册计时器、不报告“已成功应用”。持久化GUI偏好不要求window/widget存在。
- `NyanpasuClient`暴露领域操作/typed subscriptions，不暴露任意服务查询、ActorRef注册表或Router。GUI-only action pump在GUI分派dashboard等动作，其余调用普通core领域方法。
- 一条root CancellationToken及显式资源owner；拒绝新工作、已开始owner工作结算、post_stop清理与等待语义不变。新增构造失败回收边界，不能启动一半actor后返回Err并遗留资源。

### 3.4 Effects与观察：选定最小切口，避免另造一套调度

不将现有effects整体留GUI，也不将egui错误和TrayView原样送进core。

1. core保留真实domain slices、平台effects、revision、retry和health owner；GUI保留tray/menu projection、locale/widget/native shortcut执行。
2. 在现有dispatch seam接入**可选、静态类型的presentation port**，只暴露现有需要的配置/触发/结果。它不是任意capability registry。GUI适配层从输入形成TrayView并映射widget/shortcut失败；没有presentation时明确能力未提供。
3. 保留现有分组与顺序约束，尤其locale在tray之前、同一system proxy owner的一次reconcile、group 2原有logger位置、full refresh压过part refresh、stale revision不覆盖新结果。不要为迁移再创建第二个核心effects scheduler。
4. 原group 2 apply前的 `refresh_clash` 是一个**失效提示**，不是commit成功事件。观察必须保留触发来源/意图，不能用“统一state changed”掩盖mutation/lifecycle/retry差异。GUI映射为旧wire；源状态版本/health另由真实owner发布。
5. P06拆分时完成core observations到现有Tauri事件的接线；P09再让HTTP直接订阅，不等最终PR才恢复通知。没有main window、没有tray时core观察依然有效。
6. 现有RPC `EffectKind`/health/error形状默认不变；确需增加unsupported表达时，在P06同一行为commit同步DTO、生成bindings和消费者。不要让CLI靠注入“全部Healthy”的fake通过验收。

## 4. 能力依赖、串行PR与执行边界

### 4.1 一个Phase，一个串行堆叠的PR

下表硬前置说明能力迁移所需的依赖闭包，不代表PR分支关系。实际交付按Phase编号串行堆叠，每个PR以紧邻的前一个Phase为base；即使能力相互独立，也不再创建兄弟PR。

| Phase / PR | 交付结果                                      | 硬前置             | 建议commit数 | 风险                  |
| ---------- | --------------------------------------------- | ------------------ | ------------ | --------------------- |
| P01        | 吸收/重整已有修正，共享输入与叶子依赖         | B0                 | 3–4          | 中：多调用者，小实现  |
| P02        | KV、备份及配置迁移能力                        | P01                | 3            | 高：磁盘一致性        |
| P03        | core/daemon控制面、进程版本与OS执行           | P01                | 3            | 高：资源所有权/跨平台 |
| P04        | 日志、streams、traffic/geo及网络诊断能力      | P03                | 3            | 中高：流生命周期      |
| P05        | kernel与application更新能力                   | P03                | 3            | 高：下载/安装/取消    |
| P06        | 平台effects和GUI presentation边界             | P04                | 3            | 高：顺序/降级/通知    |
| P07        | profiles IO/materialization/fetch             | P01                | 2            | 高：文件事务          |
| P08        | 真实事务闭包、NyanpasuClient与async bootstrap | P02、P05、P06、P07 | 3            | 高：最大机械迁移      |
| P09        | 可独立运行的HTTP/中立RPC及事件出口            | P08                | 3            | 高：宏/权限/wire      |
| P10        | 共享启动准备、实例锁及OS关机资源owner         | P08                | 3            | 高：进程/平台         |
| P11        | 独立宿主与剩余GUI边界的持续验收               | P09、P10           | 1–2          | 中：验证/门禁         |

P08通过P06传递依赖P04/P03，不是遗漏控制面。P05依赖P03交付的core version parser（`updater/instance.rs` 的真实调用），不能仅凭updater backend是trait就与P03完全独立。P04中的core logs可先本地准备，但整个P04 PR以P03为base，**不再产生“P04-A先落地”的第二套合并单位**。

能力依赖图（不作为并行创建PR的分支图）：

```text
B0 → P01 ─┬─ P02 ─────────────────────────────┐
          ├─ P03 ─┬─ P04 → P06 ──────────────┤
          │       └─ P05 ────────────────────┼→ P08 ─┬─ P09 ─┐
          └─ P07 ────────────────────────────┘       └─ P10 ─┴→ P11
```

实际PR stack：

```text
main → P01 → P02 → P03 → P04 → P05 → P06 → P07 → P08 → P09 → P10 → P11
```

### 4.2 串行实施与只读并行

- 按Phase顺序实施，前一阶段的候选完成审核和交付后，下一阶段从其明确的head继续；不从更早的共同祖先创建兄弟实现分支。
- 一个worktree只有一个writer。可复用已选定的隔离worktree及其独立构建资源，不为每个Phase重复创建构建缓存。
- 独立源码核对、调用者盘点和测试范围分析可以并行；它们不修改共享源码，也不提前实施后续Phase。
- Cargo、依赖调整和集成验证串行执行。前置head改变后，先确认受影响范围并验证，再继续下游迁移。
- 原子候选仍分别人工审核；通过后的提交和推送不再增加subagent复审。stack顺序不授权自动提交、改写历史或合并PR。

### 4.3 Worktree/branch与合并策略

- 分支可用 `refactor/core-p01-foundation` 等一Phase一branch；PR描述固定写base SHA、前置PR、commit清单、验证结果和剩余风险。
- 每个PR以紧邻的前一个Phase分支为base；前一个PR已合入main时，使用包含其交付结果的main基线。所有PR最终顺序合入main，不从不同旧head拼接“已迁移类型”。
- 前置PR未合并时可以串行stack，不要求等待整栈全部合并才继续；后续分支必须包含其实际base的完整交付结果，不能只修改GitHub base而忽略代码集成。
- 一次合入一个PR。前置更新或合并后，检查下游base、冲突、lock与bindings并做相关验证；没有“所有PR一起合才绿”的批量合并。保留已发布提交，merge/rebase等集成操作及其提交按人工授权执行。
- 未推送修正折入所属commit；已推送历史的改写需要许可。PR策略应保留经过验证的原子commit；若项目选择squash，则整体PR必须同样可构建、可回退，不能据此容忍中间提交坏掉。
- 只软链接 `backend/tauri/sidecar/` 和 `resources/`；`target/`、`node_modules/`、`tmp/dist/`各自独立。Rust-only Tauri检查可使用各自dist placeholder；独立core检查不准创建placeholder以掩盖GUI依赖。Cargo任务按当前stack串行执行。

### 4.4 文件写入权，而不只是目录分工

| 热点                                       | 规则                                                                                     |
| ------------------------------------------ | ---------------------------------------------------------------------------------------- |
| `client/mod.rs`、`setup.rs`、root `mod.rs` | 各能力PR只做自己构造参数/import/字段的最小接线；禁止整段重新格式化/重排；P08独占结构拆分 |
| Cargo manifests / `Cargo.lock`             | 依赖随实际消费者迁移，同commit更新；合并后由Cargo重算，禁止手工拼lockfile或顺手升级      |
| `ipc.rs` / `specta_export.rs`              | 叶子PR只改路径/必要wrapper；P09独占共享命令定义/宏协议重构                               |
| generated bindings                         | 同PR由实际源码导出；base更新后重导出，不用文本合并替代真实生成                           |
| `client::tests`                            | 不作为跨crate生产test-support API；迁移叶子只带走其最小fake，facade大fixture由P08负责    |
| `nyanpasu-core/src/logs/archive.rs`        | P01先把HTTP/mirror拆出并更新所有调用者；P04再搬剩余archive，不同时编辑同一混合块         |
| `client/core_lifecycle/ports.rs`           | P01移binary契约；P05只消费它；P08才处理runtime preparation部分                           |
| `state/profiles/error.rs`                  | P07只分离IO/fetch错误及ErrorPath消费者；P08移事务错误；不让两条线同时改枚举              |
| architecture policy静态allowlist           | 每次move只更新该能力精确条目和测试；不批量扩大豁免或刷新snapshot以隐藏新增残留           |

某条线确实需要另一条线尚未交付的类型时，暂停并修改依赖表；不能复制DTO、增加Tauri re-export、用字符串抹掉错误或让集成者以后补洞。

## 5. 每个Phase的原子commit设计

以下commit标题是建议，不是允许分开提交“不完整搬家”的步骤。每个 `C` 都包含生产/测试调用者、manifest、旧module删除、必要bindings与自己的验证。后续C不得只是修复前一个C漏掉的调用者。

### P01 — 已有修正重整、叶子依赖与显式输入

**PR结果：** 吸收已有实现并形成新计划的共同基础，其余能力不等待facade搬迁。旧GUI facade仍正常工作。以下按目标分支的真实diff交付：已在base且符合设计的部分没有空commit；未合并的实现直接整理成最终原子commit，不先合旧版再追加修正。

- **C1 `Consolidate shared runtime configuration in core`**：吸收当前runtime config/FS/script迁移、原测试及依赖/tooling改动，按本方案形成唯一能力owner。必要时重整已有API/目录，但不恢复application/platform，也不复制新旧builder。若该完整结果已在PR base，无增量则省略此commit。
- **C2 `Extract shared task ownership and blocking helpers`**：迁移 `drain_on_shutdown`、`track_until_shutdown` 及blocking join的实际共用部分；调用者全部改路径。facade-specific lifecycle暂留原处。保持panic恢复传播，测试验证取消前后注册与真实drain。
- **C3 `Inject host identity into shared network consumers`**：把 `utils/config.rs`、`candy` 的client/proxy/mirror/speed能力，以及profile fetch和两种updater共同消费的 `SelfProxyPortSource` 归 `network`；将已有BuildInfo/channel/device修正按实际消费者重整接线，订阅、普通请求、kernel updater分别注入原有UA，backup/migration注入版本，资源路径不隐式发现。只保留GUI真正需要的version常量；旧helper无消费者才删除。
- **C4 `Extract kernel binary installation contracts`**：移 `PreparedCoreBinary/BinaryInstallProgress/BinaryInstaller/InstallCoreBinaryError` 与独立 `ErrorPath` 到消费能力；`FsBinaryInstaller`、workflow、updater全部使用同一类型。**不搬 `RuntimePreparationPort/PreparedRuntime`**，它们引用runtime闭包。Snafu跨crate构造用必要的窄构造入口，不把全部selector公开。

验收：原网络/UA与binary安装相关测试，变更前后请求header字节、安装progress顺序、blocking panic语义；core+GUI检查。没有只声明未来无人使用trait的commit。

### P02 — 持久化、备份、配置迁移

**PR结果：** 离开Tauri也能打开同一KV、制作一致备份、迁移同一配置。

- **C1 `Move storage and change subscriptions into core`**：storage/error/export/change receiver迁core；Tauri `StorageValueChangedEvent`与listener留adapter，生产listener订阅新API。不要只搬数据库而让唯一订阅入口仍依赖AppHandle。
- **C2 `Move consistent configuration backups into core`**：backup请求/保留策略/archive及版本输入整体迁移；manual facade和migration使用同实现。保留redb snapshot而非复制锁住的文件、backup失败退出码、prune失败策略。
- **C3 `Move configuration migrations into core`**：registry/runner/store/legacy schema/formats/fixtures同迁，所有source/client加载格式更新。`current_version`改为显式目标版本；legacy locale默认直接消费config现有规则。CLI参数解析/彩色输出/进程退出留host。

测试：旧迁移fixture、重复运行、部分失败/journal恢复、backup-before-mutation、文件权限、Windows数据库句柄释放。P02不顺便迁移home-dir进程等待/提权，它有P10的明确owner。

### P03 — 控制面与OS执行

**PR结果：** core可以构造local/service端点、执行服务操作，并拥有版本查询契约/错误/parser、OS environment collector与UWP执行。原 `TauriCoreVersionReader` 继续作为host适配器使用Tauri shell，不在move refactor中重写进程执行。

- **C1 `Move core and service control into core`**：`actor_v2/*`、`core/service/control.rs`、local host/OS adapter同迁；status Event wrapper留host。必要的路径/提权实现同调用链迁移；原actor tests不再依赖host `client::tests::test_paths`。保留所有现有期限、错误分类与原测试；跨 crate cfg(test) 消费者先通过私有 host fixture 与既有生产 API 闭合，不公开测试 hook。
- **C2 `Move version contracts and OS utilities`**：迁移现有version契约/错误/parser、OS environment collector与UWP工具执行，更新真实调用者。`utils/core_version.rs` 保留原Tauri reader实现，仅改import；路径选择、参数、进程生命周期和错误行为仍由原shell调用承担。不新增进程adapter、probe binary或对照测试框架。
- **C3 `Review control IPC and operation bounds`**：核对真实 daemon IPC、OS adapter 与 caller budget 的不同覆盖范围；保留现有期限及 timeout 后 unknown/recovery 语义，不把期限当作取消证明，不要求或承诺未来删除 caller budgets，不借此重写控制算法或升级 IPC。任何期限/IPC 契约变更另行明确审批。此项只做复核和文档同步，不为凑commit数量制造代码改动。

验收：保留原handoff、revocable API lease、generation、service compatibility/restart exhaustion、queued call/caller drop及parser、注入reader与updater测试。版本参数 `-v/-V`、sidecar路径、spawn/exit处理使用原host实现；不为迁移新增进程对照框架。区分源码复核、定向回归与真实IPC/提权/打包路径/native进程验证，不把观察超时当作操作终止或完整关闭证明。需要fake-core的原测试须先构建，不假设dev-dependency会产生binary。

### P04 — 日志、流与诊断

**PR结果：** 日志、连接流和流量记录在无窗口状态下能运行、观察和关闭。

- **C1 `Move log services and archive adapters into core`**：core logs/model/redb/codec/dictionary、service-log查询端口与IPC adapter、app log文件能力、ZIP/tempfile归logs；frontend batch净化/sink归 `logs::frontend`。facade方法暂留host，调用共享能力；Tauri Event/保存对话框留host。将独立logging writer/rotation/filter helpers与process subscriber安装分开；channel创建、原内联reload线程、具体reload handle操作及FileAppenderGuard生命周期仍留host init，host仍决定全局subscriber及profilers。随迁独立 `LogRotation/LoggerRefresher/LoggerError`，其到effects failure code的映射暂由现有executor完成，不让logs反向等待P06的effects类型。
- **C2 `Move connection streams and traffic recording into core`**：Clash read DTO、streams/history/recording、connection规则、geo与traffic owner同迁，复用现有独立库。Event derive与Channel/webview注册不迁；原stream和geo测试fake在中立测试模块构造。
- **C3 `Move network diagnostics and icon caching into core`**：direct egress、URL/IP/ASN探测、icon URL key/TTL/postcard/下载、proxy env文本迁出。暂依赖facade snapshot的 `LocalSourceCache` 留P08；实际probe实现可先迁。dialog/clipboard/data URL展示留host。

测试：session切换/失联不等于停止、日志cursor/rotation/codec兼容、stream receiver demand/取消、traffic flush/retry、geo失效重载、DIRECT探测不借系统代理、缓存过期。字典bytes不改；原ignored字典再生成测试保持ignored，不为迁移重训。

### P05 — 更新能力（kernel与application分开）

**PR结果：** 更新状态机和网络/文件操作可共享，Tauri只实现应用包插件/安装/重启适配。

- **C1 `Move kernel downloads and updates into core`**：download/session/bolt adapter、kernel updater/manifest/platform selection/HTTP backend与原tests迁出。通过P01的artifact和注入 `CoreUpdateInstaller` 调用仍在host的workflow；不把workflow反向带入core，不复制安装编排。
- **C2 `Extract application release policy from the GUI bundle`**：从bundle提取feed候选、mirror验证与版本比较，参数使用普通version/date/target/project值；plugin `RemoteRelease`在adapter转换，构建env读取留host。WebView2/portable bundle判定不迁。
- **C3 `Move the application update owner into core`**：app update actor/typed client/settings/progress/tests迁入可选能力；插件adapter/event包装留host。host提供支持性和产品输入，core不读取 `IS_APPIMAGE`。`PreparedAppUpdate`插件上下文保持不透明、只供backend使用，不变成服务查询或向facade暴露的downcast接口。

测试：取消与owner终态、已下载但未验证不得安装、discard/retry、source切换、nightly比较、SourceForge URL约束、staging目录存活、core restart失败。GUI installer-before-exit/relaunch在平台上验收；不为测试做真实更新发布或升级本机应用。

### P06 — Effects/presentation切口

**PR结果：** core平台effects可独立运行；GUI效果是可选adapter，不是core启动前提。

- **C1 `Move system proxy and DNS capabilities into core`**：system proxy/PAC/autostart/DNS owner/adapters、其实际消费的desired/revision/health类型一起迁移；GUI注入实际executable/AppImage值。现有effects executor改用它们，不留旧类型alias。
- **C2 `Separate presentation from application effect execution`**：按3.4节一次完成plan/actor/executor/ports拆分及GUI实现。core保留平台effects和协调，并同迁其真实依赖 `convergence.rs` 的health/retry值模型；TrayView、widget runtime错误、window/main-thread/native快捷键接线留host。提交前hotkey验证用独立契约，不要求注册器存在；非GUI动作调用领域操作。迁移涉及的core/GUI原测试随职责拆分，不能让core tests import widget。
- **C3 `Publish application observations without a webview`**：从owner输出独立订阅与现有refresh意图，host事件映射直接消费；proxy变化的frontend转发不再借tray保活。原proxy owner仍在host直到P08，使用其现有watch，不提前复制它。明确提交后的通知、apply前invalidations、configuration status三种语义。

测试：四组并发边界/组内顺序、requested-but-unchanged、full/part、retry budget、stale revision、shutdown restore/unregister、无presentation与显式unsupported；desktop事件名/payload/触发时机保持。旧casing不一致若复现，先记录为独立问题，不借切分悄悄改wire。

### P07 — Profiles IO闭包

**PR结果：** 同一套FS/materialization/fetch实现可在core测试，具体profiles actor仍留待P08。

- **C1 `Move profile filesystem and subscription adapters into core`**：一次迁移 `ProfileFsPort/SubscriptionFetcher/ProfileMaterializationPort`、独立file/fetch错误及 `ProfileFileService` 实现/原测试；复用P01已迁的 `SelfProxyPortSource`；同commit更新所有真实消费者。事务 `ProfilesError`、`CommitAborted`仍留原owner，引用新IO错误。不要整个error.rs原样搬走制造反向依赖。
- **C2 `Isolate profile content preparation from actor ownership`**：将 `FsRuntimeBuildAdapter` 实际调用的 `ProfilesActor::current_closure` 纯规则归profiles能力，原actor和builder直接调用同一实现；只迁这条实际依赖及其测试，不为future builder另造content框架。不搬RuntimeSnapshot/RuntimeError图，不重复现有 `runtime::config`。

测试：import fetch-before-commit前提、header/name/interval解析、symlink/path validation、materialize prepare/commit/recover/cleanup、frozen contents与原顺序。UA/device timing不变，测试用临时路径和固定device；不增加生产fake接口来复用host测试fixture。

### P08 — 事务闭包、facade与async bootstrap

**PR结果：** 第一个真正可用、可构造和可关闭的 `nyanpasu_core::NyanpasuClient`。此PR不是等待后续“frontend接入”的半成品。

- **C1 `Separate GUI-only facade operations and host composition`**：在现有host内把dashboard/window dispatch、main-thread、GUI展示projection从facade移给实际GUI owner；HTTP控制移至host transport owner，不再放 `NyanpasuClientInner`。命令名不变，命令只调用所属能力。补齐headless所需capability选择，而不是增加 `HeadlessClient` 替身。
- **C2 `Make application assembly asynchronous and owned`**：原真实构造改async，domain组装逻辑与facade方法分离；host唯一同步边界适配。全体构造测试改显式Tokio runtime。失败时回收已启动owner；不引入全局shutdown phase/budget，也不把caller丢弃改成取消已启动事务。
- **C3 `Move the application transaction graph and client into core`**：一次迁移剩余source/typed clients、MutationCoordinator、workflow/TCC/settlement/startup/recovery/core lifecycle、runtime snapshot/inspection/errors、剩余enhance projection/goldens、jobs/scheduler/sources、proxies与facade全部领域impl。所有GUI/HTTP/测试调用者同commit改为新路径；共享bootstrap接入P02—P07的真实能力，旧模块删除。

**为何C3允许大diff：** 它移动的是已经切断外部GUI依赖的真实事务闭包。已发现 `MutationCoordinator → ApplicationWorkflowClient → source snapshots/participant` 和递归错误图；为把这次move分成小行数commit而引入临时trait、复制DTO或Tauri re-export，风险更高。原子性按“一个完整能力切换”衡量，不按行数。P08前应重新计算闭包；若出现未知依赖，停止C3并补完前置，不能把破坏性迁移拆成坏commit。

C1/C2是独立可运行的边界改造，C3尽量仅包含路径/可见性/装配迁移；不把行为修复塞进大move。评审分别查看rename-aware diff、删除/新增符号和非机械差异。这里选择闭包迁移不是保护旧结构：若重整已有实现能形成有独立消费者、长期合理的切口，可先交付完整可构建的边界commit，再缩小C3；不能仅为分commit而发明临时抽象。

**可见性计划：** core内部类型维持private/`pub(crate)`；GUI只获启动参数、领域client方法、事件payload和消费端口。`pub(in crate::client)`随新模块树改为最窄合法范围，不全改pub。foreign inherent impl必须随其类型一起迁移；GUI扩展改adapter函数，不能留 `impl NyanpasuClient` 在host。

验收在本PR内完成：无Tauri import、无GUI fake、临时路径、真实配置/脚本/文件实现与可控core endpoint；加载→startup→profile/config mutation→Required vote/commit/compensation→观察→shutdown。保留所有原workflow/facade测试，不以新smoke替代原回归。

### P09 — HTTP、RPC与事件双出口

**PR结果：** 没有Tauri executable也能启动真实HTTP adapter、调用共享操作、接收共享事件。

- **C1 `Move HTTP hosting into a core transport adapter`**：server actor、Frontend/Assets接口、鉴权、dev proxy与测试整体迁移。host直接管理其client，独立于facade；core构造不强制启动HTTP。保持weak route持有与关闭时drain，不能制造 `client → server → router → client` 强引用环。
- **C2 `Separate shared RPC definitions from desktop dispatch`**：中立owner/error/envelope/command metadata/HTTP handlers与共享命令定义迁core；Tauri `CommandEntry`拆成desktop bindings，AppHandle/Window/Webview/Channel解码仅在host生成。宏、`ipc.rs`、registry、Specta导出和源码扫描测试同commit更新。
- **C3 `Feed HTTP and desktop events from core observations`**：共享事件由core观察直接发布到transport bus，两个出口消费；Tauri-only deep link/window events在host按原能力追加。独立SSE/connection-detail demand、session owner与资源关闭一起验收，删除仅为shared events保留的Tauri回流路径。

C2不是另造第二套命令手写列表：共享操作有唯一声明和业务handler，宏生成共享注册及desktop binding所需metadata；GUI-only声明只生成host handler和原有HTTP unsupported行为。宏需要显式区分中立定义与desktop适配生成路径，不能靠core内伪造 `crate::unified_rpc` shim维持硬编码。具体attribute语法在该PR开工时由最小compile fixture确定，不先制造第三个RPC框架。

`IpcError`当前包含中立领域错误和Tauri转换：中立wire部分与映射规则迁core，Tauri错误转换留host；保持JSON结构和Specta名称。`RpcOwner`身份由transport授予，服务端不信任body中的owner；desktop身份与HTTP UUID namespace不能串用。

验收：原query/mutation分类和HTTP opt-in集合一致；未知命令/unsupported/领域错误的HTTP status+payload一致；错误不可序列化成成功。Host/Origin/token/session cookie隔离、owner越权、SSE重连/overflow/resync、Channel reload/destroy/unmount清理、没有main window的更新均覆盖。HTTP事件不再需要先创建窗口才能工作。

### P10 — 启动准备与进程资源所有权

**PR结果：** 不仅能手动拼core graph，非GUI宿主也能复用启动准备和退出资源机制；GUI没有残留共享init算法。

- **C1 `Move application preparation and migration execution into core`**：配置初始化、资源复制、migration子进程stdout/stderr drain、home-dir迁移/下一次启动目录设置等用例迁入实际能力。host只负责CLI parse、对话框、选择当前GUI helper executable/args及退出。保留backup失败处理、双流读完再返回、原child-test入口；P08 bootstrap扩展为调用这些同一实现。
- **C2 `Own instance and OS shutdown hooks explicitly`**：实例锁/SID与Windows会话shutdown机制迁core。锁guard和hook线程拥有显式生命周期；Win32 callback通过窗口实例上下文取得owner，不迁全局sender/state。错误/初始化失败/销毁均回收回调上下文。host收到信号后请求Tauri exit，core不直接退出进程。
- **C3 `Finish shared shutdown and logging resource ownership`**：logger guard/reload线程、shared process child/producer纳入显式所有权与关闭；host最终安装process tracing subscriber和profilers。Tauri panic/main-thread handoff/ExitGate继续在GUI，不给core增加main-thread条件。core关闭先后关系由owner协议保证，不新增全局超时预算。

测试：partial startup失败不遗留owner、migration子进程非零/双流/panic、配置资源缺失的原best-effort策略、实例锁失败/释放、Windows WM_QUERYENDSESSION及callback释放、logger flush/句柄释放、重复shutdown。home迁移和OS关机只在隔离环境/专门平台测试中执行，不能拿真实用户配置验收。

### P11 — 持续验证最终边界

**PR结果：** 分离结果由CI持续证明，而不是再补一轮搬迁。

- **C1 `Enforce standalone core and GUI residual boundaries`**：在已有脚本类别和Deno任务中扩展边界检查，扫描core生产/测试/build/feature依赖、残留旧模块、禁止core读取Tauri源码树/dist；建立剩余GUI职责清单。新增明确命名的standalone任务和CI job，不能把设计中的未来任务写成已经存在。
- **C2（仅当可独立交付）`Exercise headless core and transport lifecycles in CI`**：把P08/P09/P10已经存在并通过的独立宿主/HTTP/关闭验收接入矩阵，补上平台执行与文档；不是到此才写第一个headless测试。若与C1不可独立，合并为一个commit。

本PR不得接收“顺手漏搬”的FS函数、owner或shim删除；发现残留即退回其phase责任，重新评审依赖与变更，不用最终PR掩盖早期PR未完成。

## 6. 原子性与回退契约

### 6.1 每个commit必须完整包含

```text
能力定义/实现迁移或独立边界改造
+ 全部生产/测试消费者切换
+ 原测试/fixture/字典及include路径
+ mod.rs / Cargo manifest / lockfile / 精确policy同步
+ 必要的事件wrapper / 宏生成 / bindings
+ 原实现、孤儿import与独占依赖删除
= 一个可编译、可测试、可回退的commit
```

禁止按“先复制实现 → 再切调用者 → 最后删旧目录”提交；禁止一个commit只是声明unused port，下一个才接入。必要的adapter不是旧路径shim，必须有实际职责与唯一实现。

每个commit至少检查core和GUI消费者，不能只证明被移入的库单独编译。push前对commit序列逐个checkout验证，可在独立验证worktree中执行；不能在dirty用户工作区进行reset式验证。只暂存明确路径。

### 6.2 回退

- 未合并PR：停止该分支，不影响已合并前置；修正折入对应未推送commit。
- 已合并PR：先看依赖图；有下游时按逆拓扑回退，不能单独撤销下游仍消费的类型。涉及已推送历史时用正常revert或先获授权，不强推公共分支。
- 迁移默认不改变持久格式、wire或feature默认行为，所以回退不要求反向数据迁移。若实施发现必须改磁盘格式，该项移出本迁移范围单独评审，不能声称天然可回退。
- CI红灯停止后续合并，包括仅Windows/macOS失败；不把跨平台编译问题留到P11。

## 7. 验证计划与完成标准

### 7.1 每个PR的共同检查

以下是当前已有命令。Rust测试filter随实现搬迁更新到真实新模块；不沿用旧filter跑出0 tests后记为通过。

```sh
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
cargo check --manifest-path backend/Cargo.toml -p nyanpasu-core --all-targets --all-features
cargo check --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-targets --all-features
cargo clippy --manifest-path backend/Cargo.toml -p nyanpasu-core -p clash-nyanpasu --all-targets --all-features
deno task test:backend-boundaries
deno task lint:backend-boundaries
deno task test:architecture-ledger
deno task lint:architecture-ledger
```

按影响运行原能力tests；迁移前后保存测试名称/fixture对应关系，PR正文直接记录运行命令、用例数、结果、未运行原因。Rust宏变化加 `cargo test --manifest-path backend/Cargo.toml -p nyanpasu-macro`；涉及导出时运行：

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib specta_export::tests::export_typescript_bindings
```

然后检查generated diff；纯类型move预期wire不变，有变化必须解释。必要时跑 `pnpm typecheck`、受影响frontend tests、`deno task test:http-ui <server-url>`。Linux GUI测试按环境需要设置 `GSETTINGS_BACKEND=memory`，不把它当独立headless证明。

### 7.2 关键验收矩阵

| 边界                    | 首次强制验收     | 不可退让的证据                                                                         |
| ----------------------- | ---------------- | -------------------------------------------------------------------------------------- |
| core生产/build依赖无GUI | 每个迁移PR       | all-target/all-feature dependency tree，无tauri/egui/eframe/GUI toolkit传递依赖        |
| 测试无Tauri runtime     | 各能力PR         | 原actor tests在core运行；无反向host dev-dependency                                     |
| 持久化/事务             | P02/P07/P08      | vote拒绝不commit、commit后失败为degraded、receipt/compensation与cancel语义保留         |
| 无GUI facade            | P08              | public API的集成test在core测试target构造真实graph；不是同crate私有fixture专用构造      |
| 失败启动/关闭           | P08/P10          | 已启动owner被回收、caller drop不偷取消事务、句柄/任务真正结束                          |
| 纯HTTP host             | P09              | 只依赖core的测试宿主，无AppHandle、窗口、Tauri assets或stub GUI服务                    |
| 安全/协议               | P09              | 双transport相同共享handler；原allowlist/errors/owner隔离；overflow重同步               |
| Windows/macOS/Linux     | 触及对应能力的PR | 原生CI编译；OS proxy/service/elevation/installer/shutdown等分别runtime验证             |
| musl/移动端边界         | P11及相关PR      | musl现有harness范围如实记录；移动端未实现能力显式cfg/unsupported，不宣称已有mobile产品 |

core可用一个按需启用的 `http` feature隔离server依赖，默认不启动监听；**禁GUI门禁在all-features下也必须通过**。不要给每个小模块造feature。Windows/macOS/Linux的OS依赖按实际target声明，编译矩阵不能由Linux的 `cargo tree --target all` 替代。

P11独立构建环境不安装GTK/WebKit、不准备Tauri dist，仅有Rust/native shared-library所需工具、注入资源与fake-core。必须跑过应用生命周期和HTTP行为，不只跑 `cargo check -p nyanpasu-core`。

### 7.3 最终完成定义

1. 同一个NyanpasuClient由GUI与非GUI测试宿主构造，非GUI宿主没有fake window/tray/widget/main-thread。
2. profile/config修改、runtime构建/校验/应用、恢复、核心更新、日志/traffic/geo观察和关闭使用同一份实现。
3. 不支持的系统/呈现/更新能力有真实结果；缺席不伪装Healthy，支持的能力不在另一宿主重新实现。
4. HTTP可独立部署，facade不反向依赖transport；Tauri与HTTP都不依赖main window来接收core变化。
5. 所有源模块和fixture都有最终owner；Tauri只剩可解释的GUI/desktop适配，不残留旧路径re-export和第二套业务实现。
6. 最终GUI白名单、原tests与新增边界测试、bindings、平台矩阵和已知限制都可在PR/仓库中复现。

## 8. 模块迁移归属清单

以下按迁移前的模块路径标识职责与目标owner，不表示当前工作区文件是否已迁移。目录行递归包括其tests和module入口。更具体的行优先于目录/剩余项。`G`表示确实服务GUI而保留，不意味着可以继续承载共享业务。测试按实现owner迁移，混合测试按职责拆开。

| 迁移前路径（相对 `backend/tauri/src`）                                                                              | 最终归属 / phase                                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `main.rs`                                                                                                           | G：GUI executable入口                                                                                                                                                                                                                      |
| `lib.rs`、`setup.rs`                                                                                                | G装配；共享graph P08、startup P10、transport P09                                                                                                                                                                                           |
| `host_paths.rs`、`consts.rs`                                                                                        | G产品/构建输入；消费方不反查这些模块，P01/P08                                                                                                                                                                                              |
| `bundle.rs`、`bundle/tests.rs`                                                                                      | G bundle/WebView2；release纯策略及相应tests P05                                                                                                                                                                                            |
| `shutdown_hook.rs`                                                                                                  | core process Windows adapter P10；Tauri exit接线留G                                                                                                                                                                                        |
| `cmds/`                                                                                                             | G parse/print/exit/widget dispatch；迁移/诊断执行 P02/P03/P10                                                                                                                                                                              |
| `ipc.rs`、`unified_rpc.rs`、`specta_export.rs`                                                                      | P09拆中立RPC/共享操作；G保留native commands/desktop dispatcher/导出                                                                                                                                                                        |
| `server/`                                                                                                           | core transport P09；assets resolver实现留G                                                                                                                                                                                                 |
| `window/`、`widget.rs`、`event_handler/`                                                                            | G窗口/展示/widget IPC；调用共享core，不能存业务owner                                                                                                                                                                                       |
| `core/tray/`                                                                                                        | G菜单/绘制/队列/icon；独立proxy通知接线P06                                                                                                                                                                                                 |
| `core/storage.rs`                                                                                                   | core storage P02；G Event/listener                                                                                                                                                                                                         |
| `core/backup.rs`、`core/migration/`                                                                                 | core backup/migration P02                                                                                                                                                                                                                  |
| `core/actor_v2/`、`core/service/`                                                                                   | core control/service P03；G status Event包装                                                                                                                                                                                               |
| `core/manager.rs`                                                                                                   | 当前仅escape helper；P03保留，不为迁移清理无关helper，不按旧文档误记为完整manager                                                                                                                                                          |
| `core/win_uwp.rs`                                                                                                   | core Windows能力P03；资源位置和native命令权限留边界                                                                                                                                                                                        |
| `nyanpasu-core/src/logs/`                                                                                           | core logs P04；G Event包装留host `core/status_events.rs`                                                                                                                                                                                   |
| `nyanpasu-core/src/geo/`、`nyanpasu-core/src/traffic/`、`nyanpasu-core/src/connections/interrupt.rs`                | core geo/traffic/connections P04                                                                                                                                                                                                           |
| `nyanpasu-core/src/clash/api.rs`、`nyanpasu-core/src/clash/proxies.rs`、`nyanpasu-core/src/clash/ws.rs`             | core DTO/streams/proxy view P04；G Event包装                                                                                                                                                                                               |
| `core/clash/mod.rs`、`core/clash/connection_details.rs`                                                             | G stream bridge/Channel/webview订阅生命周期；共享producer P04，HTTP出口P09                                                                                                                                                                 |
| `core/proxies.rs`                                                                                                   | core proxies P08（依赖事务outcome）                                                                                                                                                                                                        |
| `core/download/`、`core/updater/`                                                                                   | core download/updates::kernel P05                                                                                                                                                                                                          |
| `core/mod.rs`                                                                                                       | 各能力迁完删除其旧声明，G tray等按宿主路径保留，不再聚合共享core                                                                                                                                                                           |
| `service/profile_file.rs`                                                                                           | core profiles IO P07                                                                                                                                                                                                                       |
| `service/icon.rs`、`service/mod.rs`                                                                                 | core icons P04；旧聚合入口随最后消费者删除                                                                                                                                                                                                 |
| `enhance/`                                                                                                          | 剩余artifact/log/snapshot projection/goldens P08；不是重搬已在core的builder                                                                                                                                                                |
| `state/`                                                                                                            | core source/transaction P08；P07先分独立IO ports/errors                                                                                                                                                                                    |
| `client/app_update/`                                                                                                | core application updater P05；G plugin/install/Event adapter                                                                                                                                                                               |
| `client/application_workflow/`、`client/core_lifecycle/`                                                            | core runtime/workflow P08；P01先抽binary契约                                                                                                                                                                                               |
| `client/effects/`                                                                                                   | core effects P06；facade impl随P08，G presentation projection拆出                                                                                                                                                                          |
| `client/system_proxy/`、`client/system_dns.rs`                                                                      | core platform effects P06                                                                                                                                                                                                                  |
| `client/hotkey/`                                                                                                    | P06中立配置验证/业务动作契约；G插件注册owner/dashboard/action pump；facade业务方法P08                                                                                                                                                      |
| `client/ui_effects/`                                                                                                | G tray/widget/locale实现；logger基础P04，presentation消费切口P06                                                                                                                                                                           |
| `client/event_sink.rs`、`client/main_thread.rs`                                                                     | G Tauri emitter/main-thread handoff；owner观察P06，不把整个UI trait搬core                                                                                                                                                                  |
| `client/logs.rs`、`client/frontend_events.rs`                                                                       | logs能力/净化/sink P04，facade impl P08；webview身份映射P09                                                                                                                                                                                |
| `client/core_version.rs`、`client/direct_egress.rs`                                                                 | core version P03 / probe P04；facade impl如有随P08                                                                                                                                                                                         |
| `client/convergence.rs`                                                                                             | core effects共享health/retry模型 P06，runtime消费者同步改路径                                                                                                                                                                              |
| `client/app_lifecycle.rs`                                                                                           | 共用task helper P01；facade lifecycle P08                                                                                                                                                                                                  |
| `client/`其余文件                                                                                                   | `mod/application/clash_api/clash_config/clash_info/clash_streams/configuration_status/error/jobs/ports/profiles/runtime/runtime_error/runtime_inspection/runtime_recovery/session_state/traffic` 全归P08；按领域分布，不搬泛用client工具桶 |
| `utils/blocking.rs`、`utils/config.rs`                                                                              | core task/IO与network P01                                                                                                                                                                                                                  |
| `nyanpasu-core/src/logs/archive.rs`                                                                                 | network部分P01；archive部分P04                                                                                                                                                                                                             |
| `utils/collect.rs`、`utils/sudo.rs`                                                                                 | core diagnostics/process P03                                                                                                                                                                                                               |
| `utils/core_version.rs`                                                                                             | G原Tauri版本进程适配器；P03仅将其消费的契约/错误/parser迁入core                                                                                                                                                                            |
| `utils/net.rs`、`utils/proxy_env.rs`                                                                                | core diagnostics/文本P04；clipboard留G                                                                                                                                                                                                     |
| `utils/init/mod.rs`、`utils/init/tests.rs`                                                                          | shared init/process P10；bundle resources发现留G                                                                                                                                                                                           |
| `utils/init/logging.rs`                                                                                             | 独立logs writer/filter helpers P04；内联reload线程/guard及process subscriber/profiler安装留host，生命周期P10                                                                                                                               |
| `utils/help.rs`                                                                                                     | YAML/merge/UID/header解析按实际消费者P02/P07；locale默认调用config规则；open/DPI/tray image/quit/restart/dialog宏留G                                                                                                                       |
| `utils/dirs.rs`                                                                                                     | 当前只有version helper/常量；P01消除共享消费者对它的依赖，GUI有真实用途可留                                                                                                                                                                |
| `utils/exit.rs`、`utils/resolve.rs`                                                                                 | G ExitGate/window setup；shared lifecycle调用P08，进程执行机制P10                                                                                                                                                                          |
| `utils/color.rs`、`utils/dialog.rs`、`utils/dock.rs`、`utils/open.rs`、`utils/main_thread.rs`、`utils/profiling.rs` | G样式/交互/UI线程/宿主profiling；不因纯函数就迁移                                                                                                                                                                                          |
| `utils/winhelp.rs`                                                                                                  | 当前未挂载、用于Windows版本/UI判断的孤立文件；保留记录，不擅自删除或启用；无独立业务调用链证据，不强造core API                                                                                                                             |
| `utils/mod.rs`                                                                                                      | 随各phase删共享module声明，仅留GUI helpers                                                                                                                                                                                                 |

### 8.1 非Rust资源与build.rs

- `Cargo.toml`：依赖跟消费能力逐PR迁移，不整体复制到core；`.gitignore`继续属于GUI构建目录。
- `build.rs`：保留tauri-build、bundle/git metadata/GUI manifest工作；共享BuildInfo作为参数，不让core编译运行Tauri build script。
- `Info.plist`、`capabilities/main.json`、两个Tauri配置、两个overrides、三个installer templates、Windows app manifest：G。
- 五个locale JSON和全部22个icons资产：G。不把GUI locales搬成core错误文案依赖。
- `nyanpasu-core/src/logs/preset.zdict`：P04；四个migration fixture：P02；四个enhance golden fixture：P08。移动不改变内容。
- `tests/sample_clash_config.yaml`：P08复核实际引用后随runtime测试资产归core；不趁机删除、也不为它制造新业务测试。
- gitignored sidecar/resources是host提供的运行输入及分发资产，不要求core从Tauri目录发现它们；`tmp/dist`、`tmp/git-info.json`、Tauri生成schema不成为core构建前提。
