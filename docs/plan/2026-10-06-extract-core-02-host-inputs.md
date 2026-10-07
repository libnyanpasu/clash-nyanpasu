# Phase 02 子计划：resources／version／channel／build／device 输入边界

## 1. 状态、前提与目标

**状态：任务 1 实现与本地验证完成，用户已审阅并另行授权提交／推送；源码及配套文档随本次任务 1 提交交付。任务 2—7 尚未实施，不声明 CI 或合并完成。** 本计划只细化[完整边界审计](../design/2026-10-06-tauri-core-boundary-audit.md)中 Phase 02 的这组输入，不替代完整 roadmap，也不代表 Phase 02 的 errors/contracts、notification seams 等其余工作已经完成。

最初源码核对的 checkout 为 `refactor/extract-core-02-boundaries`，HEAD 为 `8f344a358404f43fe118cc9145f1e9eeba24a5f5`。当时含历史 Phase 01 实现，但不含独立 paths PR；已丢弃的旧 02-A 不是实施基础。最新 main 接入覆盖见第 10 节，最初执行与验证记录保留历史基线。

独立 [PR #5645](https://github.com/libnyanpasu/clash-nyanpasu/pull/5645) 在最初查询时仍为 OPEN、`mergedAt: null`，核对 head 为 `65dd5c8be7094630d925ef93318a3e59b51ec3b9`。涉及路径 API 的设计以该 head 的 [README](https://github.com/libnyanpasu/clash-nyanpasu/blob/65dd5c8be7094630d925ef93318a3e59b51ec3b9/backend/nyanpasu-paths/README.md)及调用者实现为准；不能把 PR 内实现写成当前 checkout 或 main 已集成。

### 目标

1. 共享 DTO、纯规则和非 GUI 基础设施不再读取 GUI crate 的编译环境、`BundleMetadata`、版本 helper 或全局设备实例。
2. GUI 只供应真实的发行包／构建输入；可复用的 OS 设备识别不要求 CLI 反向调用 GUI。
3. 保留现有版本来源、channel 规则、订阅 headers、设备采集时序、错误及 wire 契约。
4. 每个迁移单元同时更新全部调用者并删除被替代实现，不留下 re-export shim 或后续清理清单。

这不是完整 headless bootstrap 的验收：服务执行、诊断进程查询、资源初始化、profiles 整项能力及 Tauri 自更新与 facade 的最终分离仍有各自阶段。

## 2. 范围与非目标

| 输入／职责          | 本次处理                                                                                                        | 不在本次扩大范围                                                                                 |
| ------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| resources 位置      | 复用 #5645 的 `ResolvedPaths` 可选资源根目录，明确供应者与缺失语义                                              | 不建立第二套 resources locator，不重新提取 paths；资源复制初始化归 Phase 03，UWP 执行归 Phase 04 |
| application version | 从 GUI 构建元数据显式供应实际消费者，保留不同 UA 的来源及格式                                                   | 不重写发布版本生成、不统一原本不同的版本语义                                                     |
| channel             | application state／typed client 使用 `nyanpasu_config::application::ReleaseChannel`；宿主供应 installed channel | 不把 Tauri updater 的 feeds、发行包选择、安装支持判定搬进 core                                   |
| build               | `BuildInfo` 与诊断数据契约归共享层；编译输入装配继续属于各宿主                                                  | 不让 core build script 读取 GUI 的 package.json、Tauri config、git-info 或前端 assets            |
| device              | 订阅设备 DTO、HWID 规则与 OS 采集适配迁入共享 device 能力，替换全局实例                                         | 不迁整个 subscription/materialization 服务，不增加 HWID 策略、隐私开关或移动平台专用实现         |
| diagnostics         | 分离数据契约与 GUI 内现有采集／进程实现，显式传入 build 输入                                                    | 不统一两套代理内核版本查询，不修复既有诊断 binary 查找／panic／输出语义                          |

资源根目录已经是 #5645 的能力，因此本子计划对 resources 的新增工作主要是边界核对，不人为制造一个待实现模块。若需要同时迁移 `init_resources`，应作为 Phase 03 的明确扩展重新评估，不默默并入“输入提取”。

## 3. 当前调用链与必须保留的语义

### 3.1 资源位置与资源初始化不是同一职责

#5645 中，GUI setup 发现真实 Tauri resource 位置，再由 `PathResolver::resolve_paths(resources)` 形成 `ResolvedPaths`。共享实例已持有可选资源目录；不需要 `ResourceContext`、虚构目录或调用回 GUI 的 resource resolver。

现有 `utils/init::init_resources(&ResolvedPaths)` 处理从 bundle resources 到 data root 的文件复制：一般平台为 `Country.mmdb`、`geoip.dat`、`geosite.dat`，Windows 另含 `wintun.dll`。它包含目录准备、mtime 比较、复制失败日志等 IO 规则；setup 用 best-effort 日志边界调用它。`core/win_uwp.rs` 另有可执行资源消费者。

本次固定以下契约：

- 资源目录发现与共享实例构造不触发资源复制，不创建假资源目录。
- 没有 bundled resources 的宿主可构造路径／输入；不能因为存在 data root 就伪造 resource root。
- 当操作真实要求资源根目录时，继续返回 `app_resources_dir()` 的实际错误；不把缺失资源改成静默成功。
- 已在 data root 中的运行数据与 bundle source 是不同概念；不要求 CLI 必须模拟 GUI 包布局。
- 原初始化顺序、best-effort 边界和 UWP 行为留给各自 capability 迁移，不在本次顺手重写。

### 3.2 四类 version 含义必须分别保留

| 当前数据／消费者                                   | 现有来源                                           | 本次迁移约束                                                              |
| -------------------------------------------------- | -------------------------------------------------- | ------------------------------------------------------------------------- |
| `BuildInfo.app_name`／`app_version`                | GUI crate 的 `CARGO_PKG_NAME`／`CARGO_PKG_VERSION` | DTO 不在 core crate 内重新执行这些 `env!`；值由实际宿主提供               |
| `BuildInfo.pkg_version`、`utils/dirs::APP_VERSION` | GUI build.rs 输出的 `NYANPASU_VERSION`             | 保留应用发行版本含义；不因为字段名称而交换 `app_version` 与 `pkg_version` |
| 订阅默认 UA                                        | `clash-nyanpasu/v{NYANPASU_VERSION}`               | 保留 `v` 前缀及用户自定义 UA 优先级                                       |
| 通用 HTTP helper UA                                | `clash-nyanpasu/{NYANPASU_VERSION}`                | 保留无 `v` 前缀；消费者包括 icon cache、URL delay、IP/ASN 查询            |
| 代理内核 updater UA                                | `clash-nyanpasu/{CARGO_PKG_VERSION}`               | 显式注入原来源，不能随迁入 core 变成 core library 的版本                  |
| backup manifest／migration 当前应用版本            | `BUILD_INFO.pkg_version`                           | 备份显式接收真实发行版本；迁移目标版本与运行应用版本分别传入，不混用      |
| runtime snapshot `crate_version`                   | `nyanpasu-config` 自己的 `CARGO_PKG_VERSION`       | library/schema 元数据，不是应用版本；本次不改                             |
| 正在运行／磁盘上的代理内核版本                     | 执行 Clash/Mihomo 等 binary 得到的输出             | 不是 application build metadata，不用 application version 替代            |

`BuildInfo` 还包含 commit hash/author/date、build date/profile/platform、rustc/LLVM version。所有 11 个现有字段、序列化名称和意义保留；不增减 DTO 字段，不在本次重命名 wire API。

GUI `build.rs` 继续保留 package.json→Tauri config 的版本发现及 `tmp/git-info.json`→git 查询等真实构建输入。共享层只消费值，未来 CLI 也应供应自身真实构建元数据，而非借用 GUI build pipeline。

### 3.3 Channel 是共享状态输入，包策略仍是 GUI 职责

现有 `BundleMetadata` 同时含 `is_portable`、`is_fixed_webview`、`release_channel`。`ApplicationClient` 只需要 channel 值，但类型路径仍经过 `crate::bundle::Channel`；`ApplicationActorArgs` 与 actor state 已直接使用 config 的 `ReleaseChannel`，不重复修改；facade 的 installed/effective channel 查询则读取整个 bundle metadata。

边界应为：

- `ReleaseChannel` 继续由 `nyanpasu-config` 所有，不定义第二个 shared channel enum。
- GUI 的 `compiled_channel(cfg!(feature = "nightly"), NYANPASU_VERSION)` 负责把真实构建特征／包版本转换为 installed channel；该包策略及原测试仍留 GUI。
- application state／actor 只接收 installed channel。保留现有 `ReleaseChannel::resolve`、初始化／加载后的 channel 持久化及 Nightly 限制。
- installed channel、有效用户 preference 与 Tauri updater 使用的 channel/configuration 是不同观察点；保留当前各调用点的 `resolve`／`unwrap_or` 差异，不借提取统一策略。
- `is_fixed_webview`、WebView runtime 检查、AppImage／portable 更新支持判定、updater endpoints、SourceForge 元数据、签名／包选择／安装执行继续属于 GUI。

本次不把 app updater 重新分类成共享能力。现有 GUI facade 内仍有 app updater 的装配和方法，它们在 Phase 10 随 GUI facade 分离处理；本次共享数据／actor 输入不能再经过 `BundleMetadata`。GUI 包元数据可以继续被 GUI updater 使用，但不能充当未来 core 的输入总包或兼容层。

### 3.4 两种 DeviceInfo 不能合并

| 当前类型                        | 实际用途                                                            | 归属                                            |
| ------------------------------- | ------------------------------------------------------------------- | ----------------------------------------------- |
| `utils/hwid::DeviceInfo`        | `hwid`、`device_os`、`os_version`、`device_model`；用于订阅 headers | 共享 device capability 的 DTO／规则和 OS 适配   |
| `utils/collect::DeviceInfo<'a>` | CPU 品牌／频率／数量及格式化内存大小                                | 共享 diagnostics 数据契约；现有采集实现独立处理 |

订阅设备识别的保留项：

- Windows MachineGuid、macOS IOPlatformUUID、Linux machine-id 的查找规则及其他平台 fallback 不变。
- 固定 `clash-nyanpasu:` salt、SHA-256 前 16 字节、32 位 hex 编码及固定 fallback seed 不变；不随 CLI 包名或路径 app identity 改变设备身份。
- 先生成真实 OS/model 和 fallback HWID，再尝试平台 HWID；失败保留 fallback 的顺序与日志边界不变。
- `sanitize_for_header` 按现有过滤规则处理字符，不增加额外 normalization，不改变 `x-hwid`、`x-device-os`、`x-ver-os`、`x-device-model`。
- 目前首次采集发生在订阅 HTTP client 成功构建后、第一次请求前；一次 fetch 的重试共用同一 snapshot。
- 不在 application startup 或 DTO `Default` 中提前进行平台采集；实例构造本身不触发 OS IO。

## 4. 建议的输入与所有权模型

采用几个有真实职责的边界，不建立装下 paths/build/device/bundle 的万能 `HostContext`。

```text
GUI composition root
  ├─ GUI build.rs／consts 的真实编译输入
  │    ├─ shared BuildInfo → diagnostics／显示消费者
  │    └─ 按现有来源生成 UA → 对应 HTTP adapter
  ├─ GUI BundleMetadata／compiled_channel
  │    ├─ ReleaseChannel 值 → application owner／共享查询
  │    └─ WebView／包策略／app updater → GUI 内部
  ├─ Tauri resource discovery → nyanpasu-paths::ResolvedPaths
  └─ 一个 OsDeviceInfoSource 实例 → subscription fetcher

其他宿主
  ├─ 自己的真实 build／channel 输入
  ├─ 自己的路径与可选资源来源
  └─ 同一个共享 OS device adapter 或显式替代适配
```

### 4.1 Build 与 version

- 将 `BuildInfo` 定义归到 `nyanpasu-core::diagnostics`；`EnvInfo`、diagnostics `DeviceInfo`、`CoreInfo` 数据契约同归该 capability，避免共享 DTO 依赖 GUI。
- GUI `BUILD_INFO` 可以继续作为现有的不可变编译元数据装配结果；它不是新服务单例。共享实现不得直接读取它，也不得在 GUI 保留 `pub use` 的旧类型入口。
- Rust lifetime／ownership 仅按真实输入生命周期作必要调整；保持 Serde/Specta 输出，不为这次提取扩展模型。
- `collect_envs` 的现有 OS／进程采集仍是边界实现，显式接收 build 值与 #5645 的 resolver；它不是把旧共享实现包起来的兼容 wrapper。
- 只需要 UA 的 HTTP adapter 接收实际 UA，不注入整个 BuildInfo，不创建每个函数一个 version context。
- 当前 GUI facade 可保存确有 diagnostics／网络消费者的不可变输入，并通过对应应用操作使用它；不把 facade 变成任意元数据／服务查询器。
- backup manifest 显式接收真实应用发行版本；migration `Runner` 显式接收实际应用版本与迁移 target，不把 `--version` 指定的 target 当成备份的 `app_version`。这组修改只移除版本发现依赖，不迁移整个 backup／migration owner。

### 4.2 Channel 与 bundle

- application client／actor／状态校验直接使用 `ReleaseChannel`；installed channel 由 composition root 显式传入。
- shared-facing 查询使用同一 installed-channel 输入，不通过 `inner.bundle_metadata.release_channel` 反查 GUI 包对象。
- 不让 core-facing constructor 接收 `BundleMetadata` 或固定 WebView／发行包参数。GUI updater 必需的 metadata 保留在 GUI updater 装配侧。
- 不机械地把 GUI 包对象的所有字段复制进新 shared struct。portable 以 GUI 既有的宿主检测结果为同一来源，路径用途继续由 #5645 的 host 输入消费；其他真实消费者需要时显式供应同一值，不重复探测，不假定 resolver 已有额外查询 API，也不产生两个独立可写的 portable 来源。
- 现有 `is_portable` 对外结果不变；查询来源调整与原测试一起更新。不能以这个查询为由新增只传 metadata 的 `tauri::State`。

### 4.3 Device

建议在 `nyanpasu-core::device` 中明确区分数据／规则、窄设备来源接口和 OS 适配：`DeviceInfo` 是值，`DeviceInfoSource` 只提供设备 snapshot，`OsDeviceInfoSource` 执行平台采集。具体文件拆分按代码量决定，不创建无消费者的空 modules。

- 窄接口由共享能力所有，`ProfileFileService` 显式持有 `Arc<dyn DeviceInfoSource>`；生产与测试使用相同字段与同一 fetch 实现。
- OS adapter 实例内部可用 `OnceLock<DeviceInfo>` 缓存一次不可变结果。这是实例拥有的缓存，不是 `static`，不需要新的 actor、周期刷新或 shutdown 任务。
- composition root 每个应用图建立一个 OS source，并把同一实例注入该图的订阅消费者；首次 snapshot 调用才采集，后续请求 clone 同一结果。
- 测试注入固定 snapshot 的 fake source；实际 OS 采集原测试仍随 adapter 移动。不使用 `cfg(test)` 去掉生产依赖字段或让同一个方法在测试时跳过依赖。
- 原 `DEVICE_INFO`、无参数 `get_device_info()`、隐藏 OS IO 的 DTO `Default` 与旧 `utils/hwid` 模块一起删除或替换为对应真实角色的实现；不在旧路径留转发 API。

缓存生命周期从隐式进程全局变为显式应用图实例。当前 GUI 图内仍是首次请求采集一次、全图复用；不同图可以独立构造与注入，这是本次必要的所有权变化，不宣称跨图仍共用一个进程全局缓存。

## 5. 七个原子提交任务

每个任务对应一个完整、可构建、可验证、可单独审阅的提交；任务之间是不同输入边界，不是同一实现的修补版本。以下 subject 只是建议，不授权提交／推送。实际需要修改的 setup、facade、测试构造点与依赖文件跟随本任务一起完成，不推给后续任务。

### 5.1 公共前置检查：不作为实现提交

- 确认用户选择的工作位置、分支与 #5645 的实际集成基线；不复制第二套 paths 实现，不自动 rebase 已推送分支。
- 枚举所有定义、调用者、原 tests／fixtures，先记录受影响 focused tests 的基线。
- 记录三种 UA、channel 查询／状态规则、设备首次采集时序，以及 backup 应用版本与 migration target 的区别。
- 每项从可构建的前一个提交开始；通过自身验收并完成用户审阅后，才进入该项获授权的提交步骤。之后发现属于该项的遗漏，折回该项，不另建 fix-up 提交；已推送历史的改写须再次授权。
- 在每项中删除自己形成的旧定义、孤儿、独占依赖及精确 static 豁免；不另设“最后删除旧代码”的提交。该项状态文档也在该项一起更新，历史 inventory 保持不变。

### 任务 1：共享 build／diagnostics 数据契约

**建议 subject：** `refactor(core): share build and diagnostic data contracts`

**状态：实现已通过用户审阅，随本次任务 1 提交交付；验证记录见第 8 节。**

**独立结果：** build／diagnostics DTO 已由 shared core 所有，GUI 供应真实 build 输入；无需等待 device、channel 或 UA 改造即可构建与导出。

**迁出定义：** `consts.rs` 的 `BuildInfo`；`utils/collect.rs` 的 `EnvInfo`、diagnostics `DeviceInfo`、`CoreInfo`。建议集中在 `backend/nyanpasu-core/src/diagnostics/mod.rs`，不为每个 DTO 建独立文件。

**改动文件：**

- `backend/nyanpasu-core/src/{diagnostics/mod.rs,lib.rs}`；仅在实际需要时修改 core manifest／lockfile。
- `backend/tauri/src/consts.rs`、`utils/collect.rs`、`cmds/mod.rs`、`ipc.rs`。
- `backend/tauri/src/client/mod.rs`、`setup.rs`：仅限该 diagnostics 操作的真实输入／调用链需要；命令只调用 facade，不塞入采集编排。
- Specta export 引用及通过原 export 生成的 bindings，仅保留真实生成变化。

**同时删除：** 原 DTO 定义／旧引用，不留 `consts::BuildInfo` 或 `utils::collect::EnvInfo` re-export。实际实现还在同一 diagnostics 模块声明窄 `EnvironmentCollector` 契约，GUI 的 `OsEnvironmentCollector` 显式持有 BuildInfo，facade 调用注入的 collector。该契约让 RPC 保持薄适配，并未将 OS／进程实现迁入 core；CLI 直接构造同一 adapter。原无参数采集函数已被真实 adapter 实现替代，不留转发 wrapper。

**明确不做：** 不迁 GUI build.rs、`BUILD_INFO` 编译装配、诊断 OS／进程采集，不统一 `collect_envs` 与 Tauri shell 的 core-version 行为。

**依赖：** 公共基线；其余任务不必已完成。

**验收：** 11 个 BuildInfo 字段及现有 EnvInfo wire 契约保持；collect 显式接收 build 数据；原 Specta export、CLI／RPC 路径及 core／GUI 编译通过；共享层不读取 GUI 编译变量。

### 任务 2：application channel 输入脱离 GUI bundle 类型

**建议 subject：** `refactor(application): inject the installed release channel`

**独立结果：** application channel 类型直接来自 config，installed／effective 查询使用显式输入；不依赖后续 device／UA／backup 修改。

**改动文件：**

- `backend/tauri/src/client/application.rs`、`client/mod.rs`、`setup.rs`、`ipc.rs`、`bundle.rs`。
- 原 channel 引用／测试所在的 `client/effects/tests.rs`、`client/application_workflow/tests/mutations.rs`、`client/application_workflow/impact.rs`，以及实际需要更新的 `bundle/tests.rs`。

**同时删除：** application 对 `crate::bundle::Channel` 和 channel 查询对整个 bundle metadata 的依赖。已被替代的 GUI 公共 alias 不保留为兼容入口；GUI 内部 shorthand 若需要只是普通 import。

**明确不做：** 不新增 channel enum。`state/application.rs` 已直接使用 `ReleaseChannel`，不重复改其规则；Tauri self-update、compiled-channel、feeds、WebView／AppImage／portable 包策略仍留 GUI。现有 portable 查询结果不变，不重新检测。

**依赖：** 公共基线；语义上独立于任务 1，建议按编号执行以减少同一 facade 文件的并行编辑。

**验收：** 原 load/seed/patch/Nightly、installed/effective channel tests 通过；保留各调用点 `resolve`／`unwrap_or` 差异；bundle updater 原 tests 通过，包行为及 RPC 契约不变。

### 任务 3：迁出订阅 device capability，替换全局缓存

**建议 subject：** `refactor(core): extract instance-owned device identity`

**独立结果：** 真实 OS device adapter 可由非 GUI 宿主构造；订阅请求已使用注入的实例，旧实现全部删除。本任务不同时改变默认 UA。

**迁出实现：** `backend/tauri/src/utils/hwid.rs` 的 DTO、HWID／fallback／sanitization、OS 采集及原六个 tests。建议去向为 `backend/nyanpasu-core/src/device/{mod.rs,os.rs}`。

**改动文件：**

- shared device modules、core `lib.rs`、core／GUI manifests 及 lockfile：依平台和真实消费者迁移依赖。
- `backend/tauri/src/service/profile_file.rs`、`client/mod.rs`、`client/profiles.rs`、实际装配的 `setup.rs`。
- `backend/tauri/src/utils/mod.rs`、`ipc.rs` 中仍指向被删除 HWID API 的历史残留引用。
- `scripts/src/architecture-ledger/policy.ts` 中已被删除 `DEVICE_INFO` 的精确豁免项。

**同时删除：** GUI `utils/hwid.rs`／module declaration、`DEVICE_INFO`、无参数 `get_device_info()` 及隐藏 IO 的 DTO Default；不用转发 wrapper 保留它们。

**明确不做：** 不迁整个 ProfileFileService，不改 UA、网络 retry／proxy／timeout，不新增 actor 或新平台实现。

**依赖：** 公共基线；语义上不依赖任务 1／2，按编号执行。接口迁移时更新全部生产／测试 constructors，同一字段与同一 fetch 方法使用不同注入 source。

**验收：** 原六个 device tests 和订阅／profiles 原 tests；平台／fallback／headers 不变；实例构造无 OS IO，首次 snapshot 仍在 HTTP client 成功构建后；相同实例结果缓存，fetch 重试共用 snapshot。硬件 baseline 失败独立记录，不能修改断言掩盖。

### 任务 4：订阅默认 User-Agent 显式注入

**建议 subject：** `refactor(profiles): inject the subscription user agent`

**独立结果：** subscription fetcher 不再读取 GUI 版本常量，只使用显式默认 UA；用户自定义 UA 仍优先。

**改动文件：** `backend/tauri/src/service/profile_file.rs`、`client/mod.rs`、`client/profiles.rs` 及实际 composition root／原测试构造点。只有需要供应真实 build 输入时才改 `setup.rs`。

**同时删除：** profile fetch 实现及其测试对 `utils::dirs::APP_VERSION` 的读取。默认 UA 由真实宿主装配为 `clash-nyanpasu/v{NYANPASU_VERSION}`。

**明确不做：** 不改任务 3 的 device 机制，不改其他两类 UA。此时通用 HTTP helper 仍有真实版本消费者，因此暂不删除 `APP_VERSION`／`get_app_version()`；这不是 profile 的兼容 API。

**依赖：** 建议在任务 1／3 后执行。再次调整同一 constructor 是添加不同的真实 HTTP 输入，不是修补任务 3 的设备提取。

**验收：** 原 default-UA/HWID、headers、proxy、retry／auth、timeout 及 profiles tests；所有原 constructor 均已更新；自定义 UA／`v` 前缀／HTTP builder 时序不变。

### 任务 5：通用 HTTP UA 输入与旧 version helper 删除

**建议 subject：** `refactor(network): inject the default HTTP user agent`

**独立结果：** icon、URL delay、IP/ASN 的通用 HTTP 路径不再隐藏发现 GUI 版本；旧 dirs version API 已无消费者并在本提交删除。

**改动文件：**

- `backend/tauri/src/utils/candy.rs`、`utils/net.rs`、`service/icon.rs`、`utils/dirs.rs`。
- 对应 `client/mod.rs`、`setup.rs`／原 fake graphs 及 `ipc.rs`，仅限实际网络应用操作的输入链；命令保持薄适配，不在 handler 构建 client 或编排查询。

**同时删除：** 在确认全部真实消费者已经替代后，删除 `APP_VERSION`／`get_app_version()`。保留前一应用版本 constants、日志归档等其他真实职责。

**明确不做：** 不迁整个 icon cache、网络诊断或 candy 模块，不统一 UA，不提前执行 proxy discovery／HTTP builder，不改变 warm-up、timeout、缓存或失败语义。

**依赖：** 任务 4 消除最后的订阅版本常量消费者；使用任务 1 的真实构建输入。不能先删常量再让后续提交恢复编译。

**验收：** `clash-nyanpasu/{NYANPASU_VERSION}` 原格式不变；相关原 client／RPC／network 测试；全量搜索旧 helper 无消费者；原 HTTP capability classification 不变。

### 任务 6：代理内核 updater UA 显式注入

**建议 subject：** `refactor(updater): inject the proxy-core user agent`

**独立结果：** HttpUpdaterBackend 不在自身内部执行 GUI `CARGO_PKG_VERSION` 宏；移到任何 crate 后仍使用宿主供应的原 UA。

**改动文件：** `backend/tauri/src/core/updater/instance.rs`、`client/mod.rs` 及实际 composition root；包括该 backend 的原 test constructors。

**同时删除：** adapter 内的 `concat!("clash-nyanpasu/", env!("CARGO_PKG_VERSION"))`；真实宿主仍按 GUI crate version 装配同一 UA。

**明确不做：** 不迁 updater actor／download session，不改 URL、progress、staging／cleanup、120 秒 HTTP timeout 或 shutdown；不触及 Tauri app updater。

**依赖：** 公共基线；可直接由真实 GUI 编译输入装配，不依赖任务 3／4／5。按编号执行只是推荐顺序。

**验收：** 原 updater／backend tests 与生产构造均通过；UA 不误用 `NYANPASU_VERSION` 或 core library version；取消／清理语义不变，不通过真实下载／安装测试本次提取。

### 任务 7：backup／migration 显式应用版本输入

**建议 subject：** `refactor(migration): inject application and target versions`

**独立结果：** backup manifest 与 migration Runner 不再发现 GUI build metadata；实际应用版本与迁移 target 分别供应。两者同一个提交，因为 migration 创建 backup 时必须完整传递这两个不同输入。

**改动文件：**

- `backend/tauri/src/core/backup.rs`：BackupRequest 供应实际应用发行版本，manifest 消费该输入。
- `backend/tauri/src/core/migration/{mod.rs,runner.rs}`：Runner 不再内部调用无参数 current_version；更新相关 constructors／Default，移除被替代的隐式构造。
- `backend/tauri/src/cmds/migrate.rs`：命令／宿主边界解析真实应用版本或用户提供的 target，再显式供应。
- `backend/tauri/src/client/mod.rs` 及完整调用者枚举得到的 runner／backup 原测试和构造点。

**同时删除：** migration 的无参数 `current_version()`、backup 内对 `BUILD_INFO.pkg_version` 的读取；没有回查 GUI metadata 的兼容构造入口。

**明确不做：** 不迁整个 backup／migration 能力，不改 schema revisions、advice、store／registry、backup 内容／命名／原子提交／prune，不把 migration target 当作 manifest 的 app_version。GUI 错误／exit code 边界仍保留。

**依赖：** 公共基线；用任务 1 的宿主 build 值装配，独立于 device／channel／网络 UA。不要拆成“先改 BackupRequest、下次再修 runner”的两个不完整提交。

**验收：** 原 backup／migration／CLI migration tests，所有 BackupRequest／Runner constructors 已迁移；manual／migration manifest 仍记录实际应用版本；用户指定 target 不改变该值；原错误及 backup-before-migration 边界不变。

### 5.2 resources 核对与整体验收：不制造第八个实现提交

- 复核 #5645 的 optional resources 在装配／消费者间传递正确，无新增 locator／fake source，data root 不冒充 bundle source。
- `init_resources`／UWP／GUI artwork 的完整迁移不在七项之中，仍按对应后续阶段执行；发现确有独立 input 缺陷时，先说明并获准，再另立原子任务，不能自动塞入“收尾”。
- 每个提交已各自完成旧代码／孤儿／独占依赖删除及文档状态更新；最终只做 full suite、bindings、dependency tree、architecture gate、平台限制和语义 review，不以新的 fix-up 提交补齐前项。
- 七个提交不等于七个 PR。提交到一个已确认的 Phase 02 子 PR，或分别交付，须由用户另行决定；不自动改变现有 stack 或远端。
- 推荐顺序为 1→2→3→4→5→6→7；真实依赖重点是 4→5 的旧 helper 消费者清空，以及每项的调用链完整。core 正常依赖不引入 Tauri／egui。
- 后续说明／实现／审阅按任务逐项进行，不将“已批准某一项”视作余下六项或提交／push 的授权。

## 6. 原测试与验收矩阵

不新增迁移专用 tests；保留已有 tests／fixtures，随真实能力归属移动。不能为让本次通过而重写硬件／端口／代理行为或删除失败断言。

| 验证面                        | 优先使用的已有验证                                                                               | 必须单独报告                                                           |
| ----------------------------- | ------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| Build／diagnostics DTO        | `specta_export::tests::export_typescript_bindings`；现有 CLI／RPC 路径；对比原 JSON 字段与构建值 | export 通过不等于实际设备／core version 采集通过                       |
| Channel                       | bundle 的 compiled-channel 原 tests；application／facade 原 channel/Nightly tests                | bundle 发行包 tests 留 GUI，不为移动一个值而搬整套 updater             |
| Device                        | 原 hwid 六 tests；原 `fetch_sends_default_user_agent_and_hwid`                                   | container 空设备 model 的 baseline 失败仍需记录，不能声称全 suite 通过 |
| Subscription                  | 原 fetch header、proxy fallback、transient retry/auth、timeout tests；profiles 原 tests          | 构造位置／依赖变更覆盖全部原 fake graphs，不只编译单个 module          |
| 通用 HTTP／proxy-core updater | 相关现有 client／updater／RPC tests，核对其 UA 与 timeout                                        | 不通过真实下载／应用安装来验证提取，不新增 live external side effects  |
| backup／migration             | 原 backup、migration、CLI migration tests；manifest 与 target 输入对比                           | 版本输入已解耦不等于整个 persistence／migration capability 已迁出      |
| Resources                     | #5645 的原 paths tests、依赖与调用者核对；现有 setup 顺序                                        | 初始化 IO 未迁移、UWP 未迁移，与本次资源位置边界分开报告               |
| 平台                          | Linux 本地 tests／Clippy，Windows/macOS 实际可获得的 compile checks 与 CI                        | cross compile、平台 runtime、CI 和 merge 分别列出，不能互相替代        |

实施后按实际模块名称选择 focused tests，再运行受影响 core／GUI 原 suite。基本检查：

```sh
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib specta_export::tests::export_typescript_bindings
cargo tree --manifest-path backend/Cargo.toml -p nyanpasu-core -e normal
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
deno task lint:architecture-ledger
deno task test:architecture-ledger
deno task lint:deno
git diff --check
```

若 GUI 原 tests 确实需要容器环境规避，先记录该实施基线的原失败，再使用进程局部 `GSETTINGS_BACKEND=memory`／串行执行；不得修改系统设置。仅对确认的原硬件-model 失败做明确过滤，并报告其随迁移改变后的准确路径。生成 bindings 若有真实变化，还需相关 frontend typecheck／tests，不手改生成文件。

### 完成条件

- [x] shared BuildInfo／diagnostics 数据定义不依赖 GUI 编译输入或 GUI 类型入口。
- [ ] 应用 version、三种 UA、config crate version、代理内核 version 的语义分别保留。
- [ ] backup manifest 与 migration Runner 版本输入显式，实际应用版本与用户提供的 target 不混用。
- [ ] application channel 输入／查询直接使用 shared config 类型，不借 GUI 包对象做输入定位。
- [ ] Tauri self-update 的装配／发行／下载／安装行为没有进入 shared core。
- [ ] subscription device 来源显式、lazy、实例拥有；旧全局和旧模块已删除，全部 constructor 已迁移。
- [ ] resources 使用 #5645 的现有模型；optional source 不成为 fake GUI resource 前提。
- [ ] 原 tests／bindings／错误契约、HTTP capability classification 和 actor lifecycle 未意外改变。
- [ ] 每个移动定义的调用者、module declaration 和独占依赖一起迁移，没有兼容 re-export 或重复实现。
- [ ] 成功验证、未运行验证、平台阻塞、CI／merge 状态分别记录；审计不把本次输入提取写成完整 capability 完成。

## 7. 执行选择与后续交接

正式实施前，应先由用户确认子计划与工作位置。建议使用独立 worktree：本任务横跨 shared DTO、application channel、HTTP adapter 和多处 profile fake graphs，当前 checkout 还有未提交的审计文档，隔离更安全。成本是独立 Cargo target、pnpm 安装及 GUI 编译前置资源准备；只允许共享 `backend/tauri/sidecar/` 与 `resources/`，不共享 target 或 tmp/dist。文档规划本身不创建 worktree、不安装依赖或调整分支。

建议在 #5645 合入并按用户授权衔接 core stack 后，以 Phase 02 的独立子 PR 实施；若用户选择合入前开发，也必须明确采用哪个实际 paths head 与最终 base，不能自动 rebase 已推送分支。本计划不授权提交、push、创建 PR、修改旧 PR 或重写历史。

后续仍需处理：Phase 03 的资源初始化／持久化与 FS；Phase 04 的代理内核版本进程／permissions／service/UWP；Phase 08 的整个 profiles/materialization/fetch；Phase 10 的独立 facade/bootstrap，以及把现有 Tauri app updater 的 facade/装配整体留在 GUI。device 单元提前拆出不等于提前完成这些能力，也不允许留旧模块到最终终审再删除。

## 8. 任务 1 本地执行记录

用户确认在当前 checkout 实施；实施基线为 `8f344a358404f43fe118cc9145f1e9eeba24a5f5`，没有切换分支、集成 #5645 或重写历史。验证后用户审阅通过并另行授权提交／推送；源码及两份配套计划／审计文档纳入本次任务 1 原子提交，不创建 PR 或修改其他分支。

### 实际范围

- `backend/nyanpasu-core/src/diagnostics/mod.rs` 包含原四个数据契约和窄 `EnvironmentCollector` trait；core `lib.rs` 注册模块，无新增依赖。
- GUI `consts.rs` 使用共享 BuildInfo，保留原编译装配与全部字段值来源。
- GUI `utils/collect.rs` 保留真实 OS／core-version IO，改为显式持有 BuildInfo 的 `OsEnvironmentCollector`；构造不执行采集，snapshot 使用 owned BuildInfo 返回，不借用 adapter。
- setup 注入真实 collector；CLI collect 直接构造同一 adapter；HTTP／desktop 共用的 RPC 命令只调用 `NyanpasuClient::collect_envs`。
- 原测试构造点注入明确未配置、调用即失败的 test collector，避免无关测试意外访问机器或执行 sidecar。生产／测试 facade 依赖字段与调用方法一致，没有新增测试函数。
- 原 OS／进程采集 body 除显式 build 输入、ownership 与缩进外逐项保持；原三个 DTO 的字段定义／注释及 CoreInfo alias 保持。没有修改 channel、device/HWID、UA、backup／migration 或 resource 初始化。

### 验证结果

| 检查                                    | 基线／实现后结果                                                                 |
| --------------------------------------- | -------------------------------------------------------------------------------- |
| core 原 tests                           | 前后均 140 passed                                                                |
| GUI client 原 tests                     | 前后均 576 passed，串行、进程局部 `GSETTINGS_BACKEND=memory`                     |
| unified RPC 原 tests                    | 前后均 5 passed／1 原 ignored                                                    |
| 原 Specta export                        | 前后均 1 passed；两份生成 bindings 与改动前逐字节一致                            |
| macro 原 source／dispatcher tests       | 与 GUI 一起选择包测试，前后均 7 passed                                           |
| 完整 GUI suite，不过滤                  | 实现后 1162 passed／6 原 ignored／1 failed：设备 model 为空                      |
| 完整 GUI suite，仅过滤原硬件 model test | 实现后 1162 passed／6 原 ignored／1 明确 filtered，串行                          |
| Clippy／Rustfmt                         | workspace all-targets／all-features Clippy、Rustfmt check 通过；无本任务新增诊断 |
| architecture gate／ledger tests／Deno   | gate、49 项原 ledger tests、Deno fmt/typecheck 通过                              |
| core 正常依赖树／旧入口／whitespace     | 无 Tauri／egui；被替代 DTO 与无参函数无旧消费者；diff check 通过                 |

完整 suite 的唯一失败是未改动的 `utils::hwid::tests::test_device_model_not_empty`。保留的 starting-main 基线二进制 `backend/target/debug/build/clash-nyanpasu/2cc72b6a5d016d72/out/clash_nyanpasu_lib-2cc72b6a5d016d72` 再次复现同一断言失败，exit 101；当前 HWID 源码与本任务 HEAD 基线 SHA-256 相同。没有修复或删除这个测试，不宣称未过滤 suite 全通过。

单独选择 `nyanpasu-macro` 的原基线命令因缺少 `syn/full` feature 编译失败；与 GUI 一起选择包时原七项检查通过。未修改 macro manifest／实现来处理此无关问题。

本轮没有执行真实 CLI collect 的端到端采集、Windows／macOS runtime 或 CI／merge 验证；CLI 路径已编译，原 version 显示逻辑未改。这里的 Linux 本地／类型契约验证不能替代平台运行验证。

本地日志：`/tmp/core02-task1-{baseline,macro-baseline,focused,gates,full-gui,serial-gui,hwid-baseline}.log`；改动前 bindings 在 `/tmp/core02-task1-baseline-bindings/`，正常依赖树在 `/tmp/core02-task1-core-tree.txt`。

## 10. 最新 main 接入与 rebase 适配记录

用户授权按 00→01→02 更新分支栈，遇到与 main 的迁移重叠后又确认以 main 归属为准继续。共同 main 基线为 `8ac8ba84c`，00 为 `2e03a81a4`，更新后的 01 为 `9656d6021`。#5645 已在该 main 提交合并；本分支直接消费其实际 `nyanpasu-paths` API，不重建旧 paths 实现。

- RuntimeBuilder／builtin 保留在 main 的 `nyanpasu-application`；script／FS adapters 保留在 `nyanpasu-platform`，widget enum 保留中立 `nyanpasu-helper` 的唯一类型。
- 01 首项保留原错误分层和全部原测试，不覆盖 #5662／#5663 的快照图与单次序列化行为。main 已删除 preparation 阶段重复 render，因此不恢复无消费者的 preparation `SerializeRuntimeConfig`；实际 runtime inspection render 错误保持原状。
- 02 任务 1 的 `OsEnvironmentCollector` 注入真实 BuildInfo 与 PathResolver；构造无 IO，采集继续使用 main 的 `data_or_sidecar_path`，CLI 先准备 data dir。RPC 仍只调用 facade，不丢失 main 新增的 provider healthcheck 操作。
- 01 错误分层适配后 application 原 7、widget 原 2、GUI client 584、golden 5、CLI widget 2、Specta 1、前端相关 27 项通过，完整 typecheck 通过；Clippy、架构与 backend boundaries gates 通过。
- 连接速率自身提交 core 118 项通过；service compatibility 自身提交 core 134 项通过，保留 main `2.0.0-rc.10` 门槛。
- 02 任务 1 适配后 core 134、GUI client 584、Specta 1、macro 8、platform 49 单元与 1 集成测试通过。两份生成 bindings 相对新 01 字节不变；workspace Clippy／Rustfmt、architecture gate／49 ledger tests、backend boundaries 与 Deno checks 通过。
- 新基线未过滤 GUI suite 为 1163 passed／5 ignored／1 failed，仅原 `test_device_model_not_empty` 设备 model 为空；HWID 源码与 main SHA-256 相同。只过滤该已知项后的串行 suite 为 1163 passed／5 ignored／1 filtered。未改断言，不把新基线验证与最初 task 1／2 结果混用。
- 本次日志为 `/tmp/core-stack-01-*.log` 与 `/tmp/core-stack-02-*.log`；历史 inventory 的 275 条记录及原顺序逐条一致。

任务 2 的原实现保留于 stash `28d02a8818a443c7f50144c1bec3b1771daccca6`，不混入任务 1 提交；任务 3—7 未实施。子模块实际 HEAD `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7` 未变，沿用 main 的 gitlink。未宣称本次 Windows／macOS runtime、真实 CLI diagnostics、CI 或 01／02 合并通过。
