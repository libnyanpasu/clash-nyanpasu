# Android / iOS POC 与 MVP 调研报告

日期：2026-10-27。性质：调研和建议，**不是已批准的实现设计，也不是移动端已可运行的证明**。

后续针对具体客户端与内核的源码交叉验证见[移动代理内核与客户端交叉验证](2026-10-27-mobile-core-client-cross-validation.md)。该报告固定提交并补充 Android Mihomo、Meow、sing-box、Xray 和 iOS 客户端的实际实现、限制及更细的选型依据；有关具体客户端的判断以该补充报告为准。

## 1. 目标与结论

目标是在移动设备上运行 Nyanpasu，接管设备流量并使用代理规则；先 Android，后 iOS。用户已有 Android 和 iPhone 真机，暂无 Apple Developer Program 账号。本报告基于本地 main `9f72efe292975b13b5d9018a9f5641ffbb071c27`、当日开放 PR 状态、代码静态检查及下列资料；没有执行移动端交叉编译、安装、性能测试或 VPN 真机测试。PR、平台政策和外部依赖均可能变化，实施前需重新核实。

**建议：优先做 Android 真机 VPN 数据通路的纵向 POC，同时推动现有 core 解耦 PR 栈整合；之后把真实共享应用和最小移动 UI 接入。** iOS 先完成方案与依赖研究，待 macOS/Xcode、签名和 entitlement 条件具备后再验证真机。两端不要复制一套业务状态与配置处理逻辑，但各自必须实现原生 VPN 生命周期和数据面。

核心判断：

1. P01–P10 为共享业务提供必要基础，尤其 P08 提取真实 `NyanpasuClient`；它们不提供移动 VPN、可嵌入内核或移动宿主。
2. 现有配置、profile、事务和前端组件有复用价值；桌面进程管理、系统代理、托盘、快捷键和窗口行为不能原样移植。
3. 最大不确定性是 **移动系统 VPN ↔ 代理内核数据面 ↔ 共享应用控制面** 的完整闭环，以及网络切换、失败恢复和 iOS 平台适配。
4. 样例 UI、Android 编译成功、VPN 图标出现、单个 HTTP 请求成功都不等于 POC 完成；应从其他应用验证真实 TCP、UDP/DNS 流量、断开恢复和生命周期。
5. 完整设备 VPN 不等于所有流量都能被平台绝对捕获：系统例外、IPv6、应用绑定网络、用户路由、DNS、always-on/lockdown 等均需按平台测试并明确产品语义。

## 2. 项目当前基线

### 可复用的资产

| 范围                 | 当前证据                                                                                                                                                                   | 移动端判断                                                                                    |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 领域模型与运行时配置 | [`nyanpasu-config`](../../backend/nyanpasu-config/src/runtime/executor/plan.rs)、[RuntimeBuilder](../../backend/nyanpasu-core/src/enhance/runtime_builder.rs)              | 可以复用配置处理；现有 TUN 配置生成并不创建移动系统 VPN。                                     |
| 应用 facade 和事务   | [架构规范](../development/architecture.md#core-and-frontend-separation)、[当前 client](../../backend/tauri/src/client/mod.rs)                                              | 当前 main 的完整 facade 仍在 Tauri crate；P08 迁移后才能按目标边界复用完整调用链。            |
| 内核控制             | [RuntimeBackend](../../backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/runtime/mod.rs)、[ControlEndpoint](../../backend/tauri/src/core/actor_v2/endpoint.rs)     | 均是待评估的接入点，不保证直接适配嵌入式内核；现有类型、校验和切换流程仍包含进程/二进制假设。 |
| 前端与 RPC           | [主布局](<../../frontend/nyanpasu/src/pages/(main)/route.tsx>)、[RPC provider](../../frontend/query/src/provider/rpc-provider.tsx)、[统一 RPC 规范](../development/rpc.md) | 组件、主题、查询和类型可复用；已有窄屏布局不证明触屏、安全区或移动 WebView 已完成适配。       |
| 流量存储             | [mobile profile](../../backend/nyanpasu-tsdb/src/profile.rs)、[benchmarks](../../backend/nyanpasu-tsdb/BENCHMARKS.md)                                                      | 已有移动预算配置，不代表真实设备或 iOS extension 内存表现已验证。                             |

现有 [Tauri 打包配置](../../backend/tauri/tauri.conf.json)包含多个代理内核和服务 sidecar；[桌面组合根](../../backend/tauri/src/setup.rs)创建进程控制、系统代理及展示能力。[Tauri Shell](https://v2.tauri.app/plugin/shell/) 对移动端仅支持打开 URL，不能假设现有 Shell sidecar 调用在移动端成立。存在 `mobile_entry_point` 并不代表整个应用已可移动构建，插件注册、目标依赖、capability 和资源准备仍需逐项检查。

## 3. 未合并解耦 PR 与依赖

这是一条按 base 分支串联的堆叠链，而不是十个互相独立、可任意排序合并的 PR。下表 head 是调研时的快照；实际合并必须重新核对 head、base、冲突和 CI。

| 阶段 | PR / head（缩写）                                                           | 移动端相关范围                                                 |
| ---- | --------------------------------------------------------------------------- | -------------------------------------------------------------- |
| P01  | [#5675](https://github.com/libnyanpasu/clash-nyanpasu/pull/5675) `2f35b116` | 配置、连接速率、服务兼容性、设备身份与任务/网络等基础能力。    |
| P02  | [#5680](https://github.com/libnyanpasu/clash-nyanpasu/pull/5680) `7187b78f` | 存储、备份和迁移。                                             |
| P03  | [#5683](https://github.com/libnyanpasu/clash-nyanpasu/pull/5683) `8a366e33` | 现有控制服务及 OS 工具；不是移动内核实现。                     |
| P04  | [#5686](https://github.com/libnyanpasu/clash-nyanpasu/pull/5686) `388ff272` | 日志、流、诊断与流量能力。                                     |
| P05  | [#5688](https://github.com/libnyanpasu/clash-nyanpasu/pull/5688) `02b56484` | 代理内核下载与更新抽取；移动端不应默认采用桌面二进制更新机制。 |
| P06  | [#5690](https://github.com/libnyanpasu/clash-nyanpasu/pull/5690) `98e742f8` | 平台效果和展示分界；缺失能力应明确返回不支持。                 |
| P07  | [#5691](https://github.com/libnyanpasu/clash-nyanpasu/pull/5691) `a77f252e` | profile 文件、订阅和内容处理。                                 |
| P08  | [#5707](https://github.com/libnyanpasu/clash-nyanpasu/pull/5707) `b6e85b84` | 真实 facade、profiles actor、事务协调和恢复迁入 core。         |
| P09  | [#5717](https://github.com/libnyanpasu/clash-nyanpasu/pull/5717) `5ddb7a15` | 前端所有权和直接 Rust API 边界。                               |
| P10  | [#5718](https://github.com/libnyanpasu/clash-nyanpasu/pull/5718) `0237f187` | profile 初始化准备和可选流量库打开。                           |

调研时 P01 与最新 main 显示冲突，P02–P10 对各自 base 显示可合并；后者不代表它们与最新 main 可直接合并。各 head 显示 Linux、macOS、Windows 的 lint、Tauri build 和单元测试成功，但这是各自历史 base 的桌面 CI，并非 Android/iOS 证明。PR 正文部分 SHA/历史 CI 记录已过时；以当前 head 和检查为准。P08 明确保留启动失败清理及构造 future 被丢弃时的资源安全问题。P10 将独立外部 Rust 消费者验证留待后续；当前开放列表没有相应 P11。

P10 的 [公开构造参数](https://github.com/libnyanpasu/clash-nyanpasu/blob/0237f187a9795824ca8ab2667ef6d6abad27a2aa/backend/nyanpasu-core/src/client/mod.rs#L100)仍涉及 jobs、storage、logging、core/service clients、OS proxy、DNS、effects、installer 和版本读取等依赖。是否能由移动宿主以真实实现或明确的不支持能力完整装配，尚无独立 consumer 证明；[P11 计划](https://github.com/libnyanpasu/clash-nyanpasu/blob/0237f187a9795824ca8ab2667ef6d6abad27a2aa/docs/design/tauri-core-extraction-plan.md#L155)是重要验收，而非现成成果。P10 的流量库初始化对最小 VPN POC 不一定必要。

**整合建议：**先审查和解决 P01 基底冲突，按链逐层更新并重新验证，保留每层语义和桌面回归；不要为移动端把整栈视为已合并，也不要在 P08 前复制 facade。Android VPN/内核实验可与 PR 整合并行，但产品集成应重新基于已整合的共享边界。

## 4. 建议的边界

```text
移动 UI（Tauri/React，最小单窗口）
   → 现有统一 RPC → 移动宿主组合根 → nyanpasu-core / NyanpasuClient
                                     → 消费方拥有的窄 port
                         ┌────────────────────┴───────────────────┐
                    配置/存储/事件适配                    移动内核控制适配
                                                           ↓
Android: VpnService + 嵌入内核               iOS: Packet Tunnel extension + 嵌入内核
         系统 TUN / protect                            系统 packet flow / 网络配置
```

这是目标责任分界，不是已经存在的类图。Android 可以尝试在同一应用进程中让服务拥有内核生命周期；如果选择独立进程，必须增加明确的 IPC 和恢复协议。iOS 的 extension 与 UI 是独立进程；内核、隧道和其生命周期必须能在 UI 不运行时由系统管理。不要假设两平台内核都与 UI 或 Rust facade 共进程。

共享应用负责配置语义、事务、状态与恢复；宿主负责 OS 授权、实际 VPN 状态、路径、事件、平台能力及内核执行。移动端需要明确区分**用户期望连接、配置已提交、系统隧道状态、实际内核已应用**，并避免把提交配置或发出启动请求当成已成功联网。调用者消失不意味着 owner 已开始的操作被取消。不可用的桌面能力不能以成功的空操作冒充。

iOS 的控制路径、App Group 共享数据、provider 消息及配置提交/版本/并发协议须在选定内核后设计并验证；不要直接让两个进程无协调地写同一份 actor 状态。[Apple App Groups](https://developer.apple.com/documentation/xcode/configuring-app-groups) 支持共享容器与部分 IPC，但不自动提供事务、一致性或进程生命周期保证。不要为 POC 暴露未授权的公网/局域网控制端口。仓库 HTTP RPC 现有安全约束见[统一 RPC](../development/rpc.md)。

## 5. 内核方案比较

| 方案                                 | 优点                                       | 主要风险及决策                                                                                                 |
| ------------------------------------ | ------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| 固定上游 Mihomo 与自行维护薄移动桥接 | 配置语义与版本可控，能按项目需要设计小接口 | 需自行维护 Go/FFI、Android protect、iOS packet flow、生命周期、构建、上游升级；先做原型验证技术成本。          |
| 复用已有移动桥接                     | 可以更早验证数据通路                       | 第三方 API、修改、版本、发布物、许可证和维护状态都需审计；不能以 README 代替真机验证。                         |
| 继续桌面 sidecar 子进程              | 桌面路径现成                               | 不适合作为跨平台默认假设；iOS extension 模型与桌面进程生命周期不同，Tauri 移动 Shell 不提供桌面 sidecar 路径。 |
| 仅做远程控制                         | 可先验证 UI/RPC                            | 不能满足已选定的设备本机 VPN 目标，仅可用于临时 UI 实验。                                                      |

Android 候选参考：[libmihomo-android](https://github.com/oviron/libmihomo-android) 提供 Kotlin/JNI、TUN FD 和 protect 回调；[FlClash Android bridge](https://github.com/chen08209/FlClash/blob/main/core/lib.go) 是另一实现参考。iOS 候选参考：[ProjectClash/Clash](https://github.com/ProjectClash/Clash) 声称提供 Apple 平台 XCFramework。以上均不是已选定生产依赖，也不能证明两个平台的内核版本或语义一致。实施前记录上游 commit、构建指令、依赖树、接口、补丁、升级责任、许可证和安全响应；验证实际接收/发送包接口及是否依赖非公开 API。

首轮 Android 应同时比较 [libmihomo-android v0.3.12](https://github.com/oviron/libmihomo-android/releases/tag/v0.3.12) 和已有 Rust 内核的 [Meow Android](https://github.com/meow-rs/meow-android)：前者的 Mihomo 配置兼容性值得验证，后者的真实 JNI/TUN/protect 路径值得评估。项目声明 libmihomo 对应 Mihomo v1.19.33、bridge ABI 3、minSdk 21，支持 arm64-v8a、armeabi-v7a 和 x86_64；其 1.0 前 API 可能变化。这些均不代表本项目已验证构建或兼容性。[ClashMetaForAndroid](https://github.com/MetaCubeX/ClashMetaForAndroid) 的应用内 JNI 和 VPN 服务可作为生命周期、网络变化的源码参考，但不是已验证的独立稳定 SDK。

候选 AAR 的 `quickSetup` 通过异步回调报告结果，`startTUN` 接收被移交的 fd，`stopTun` 按其文档关闭该 fd。其接口文档没有同样明确的 TUN 启动完成回调；就绪证据、失败路径和 fd 所有权需要检查固定版本代码并实测。其 `TunInterface.protect` 返回 `Unit`，Android `protect` 则返回成功与否：是否能将失败传回内核是选型阻塞问题，不能丢弃失败后宣称防回环可靠。[候选接口说明](https://github.com/oviron/libmihomo-android#api-reference)

仓库自身为 GPL-3.0；第三方桥接、内核特定版本及传递依赖的许可证和分发义务应按**实际选定源码版本**审查，尤其静态/动态链接、App Store 分发和对应源码。不要仅依据仓库默认分支的许可证推断其他分支或 release。法律及商店适用性需要由发布方确认。

## 6. Android 技术路线

1. 系统授权：调用 VPN 准备流程，处理用户拒绝、重复请求和 `onRevoke`。服务必须符合目标 Android 版本的前台服务与通知要求；连接状态以系统/服务观察为准。[Android VPN](https://developer.android.com/develop/connectivity/vpn)
2. VPN 接口：通过 `VpnService.Builder` 设置地址、路由、DNS、MTU、IPv4/IPv6 策略并建立 TUN；明确哪一侧拥有 FD、重复关闭及失败清理。初始化任何一步失败时回滚并报告原因。
3. 出站防回环：接入核心提供的 socket protect 回调，在连接前保护适当 socket，检查失败并中止或报告；DNS、订阅更新、核心上游以及网络变化都要验证。[protect 语义](<https://developer.android.com/reference/android/net/VpnService#protect(int)>)
4. 生命周期：连接、断开、撤销、服务被杀、核心异常、网络切换要可恢复；持久化最后一次**已提交**配置，避免 UI 活着才维持 VPN。POC 若尚不支持系统 always-on，应按系统文档显式退出支持，而不是让设置页面错误承诺支持。
5. 构建：先以测试机 ABI 为 POC 目标，MVP 确定实际 ABI/minSdk 矩阵；固定 Rust/Go/JDK/SDK/NDK/Gradle 和桥接源码版本。检查所有 native 库的 16 KB ELF 与 APK/AAB 对齐，并在 16 KB 环境运行；仅升级工具链不足以证明兼容。[Android 16 KB 指南](https://developer.android.com/guide/practices/page-sizes)
6. 安全与产品语义：配置、订阅凭据和日志按敏感数据处理；说明连接失败/系统断开时是恢复直连还是依赖系统 lockdown。除明确支持的系统机制外，不承诺全局 kill switch。

当前机器可发现 Java、ADB、Cargo、Deno 和 Node；`ANDROID_HOME`、`ANDROID_SDK_ROOT` 未设置，已安装 Rust target 中没有 Android。未探测 SDK/NDK 其他安装位置，也未检查手机授权、版本或 ABI，因此尚不能判断构建环境可用。

DNS 需要分别设计系统 VPN 的 `Builder.addDnsServer`、内核 TUN 入口的 DNS 处理和底层网络 DNS fallback；这三个配置不等价。网络变化时更新底层 DNS，并验证 Private DNS、地址族和无网络恢复；`setUnderlyingNetworks` 不等于自动 protect socket 或迁移已有连接。[Android Builder](https://developer.android.com/reference/android/net/VpnService.Builder)、[Android VpnService](https://developer.android.com/reference/android/net/VpnService)

现有 manager 的嵌入 seam 仍需具体改造：[CoreSpec](../../backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/spec.rs) 包含 binary path；版本/能力探测和 recovery 仍有进程假设，[fake backend 测试](../../backend/nyanpasu-runtime/crates/nyanpasu-core-manager/tests/fake_backend.rs)也记录了 PID 证明的局限。候选接线是 manager 的 embedded backend → `CoreControl` → `LocalEndpoint` → `CoreClient` → facade；这是代码推导，尚非已验证的完整 Android 外部宿主。若无法证明内核停止，不得用虚假 PID 或伪 controller 报告正常关闭。

## 7. iOS 技术路线与前置条件

采用系统 [Packet Tunnel Provider](https://developer.apple.com/documentation/NetworkExtension/packet-tunnel-provider) 是待验证的主路线：系统启动 extension、配置网络并通过 `NEPacketTunnelFlow` 读写包；extension 承担内核和隧道生命周期。宿主 app 负责管理 VPN 配置、交付用户操作、展示观察到的状态。内核是否支持公开 packet flow 数据路径、以及接口与性能是否足够，必须用选定版本验证；不要默认可依赖未公开的 TUN FD 获取方法。

Apple DTS 明确表示没有受支持的方法获取系统 VPN TUN fd，不建议通过 KVC 获取内部 `socket.fileDescriptor`。[Apple DTS](https://developer.apple.com/forums/thread/13503) [ProjectClash/Clash-Adapter](https://github.com/ProjectClash/Clash-Adapter) 描述另一条值得验证的路径：使用公开 packetFlow 接口与**应用自行创建的 socket pair**桥接，该 fd 并非系统私有 utun fd。需审计固定版本实现，测试地址族前缀、背压、丢包和关闭语义；公开包桥本身不证明整个产品的数据路径符合 Apple 用途要求。

路由和 DNS 由系统配置与内核共同决定。[Apple 路由说明](https://developer.apple.com/documentation/networkextension/routing-your-vpn-network-traffic)描述了 include/exclude 与系统例外；[TN3120](https://developer.apple.com/documentation/technotes/tn3120-expected-use-cases-for-network-extension-packet-tunnel-providers) 对用 packet tunnel 将被接管流量代理到其他接口等用途有明确限制。Mihomo 常见 DIRECT、分流和代理监听行为与这些规定如何对应，是**上架与架构的开放问题**，需具体实现和发行用途评估，不能宣称必然合规或必然不可能。

系统 `includedRoutes`/`excludedRoutes` 可让目标 IP 在进入隧道前被排除，但不等价于基于域名、DNS 或嗅探的动态 Clash 规则。被隧道接管后在本地建立 DIRECT 连接，与 Apple 对已接管流量应经过隧道的用途说明存在实质冲突；将出站原包重新注入系统、以此实现过滤也不属于其推荐用途。对于具体代理协议及混合规则是否被允许，应向 Apple 描述实际数据路径并确认，不能以其他产品已发布为合规证据。消费者 iOS 的 per-app VPN 也不能假定与 Android 分应用列表等价；Apple 的部署条件涉及受管理设备。[Apple 部署说明](https://developer.apple.com/documentation/technotes/tn3134-network-extension-provider-deployment)

iOS 编译需 macOS、完整 Xcode 和 Rust/Go 等匹配的目标工具链；真机运行 Network Extension 需要正确的 capability、签名及 provisioning profile。当前没有 ADP 账号，且本调研未确认可用 Mac，因此这些是 iOS 真机阶段的前置条件。Apple 的 [Network Extension entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.networking.networkextension) 与 [DTS 说明](https://developer.apple.com/forums/thread/814047)指出付费团队可启用对应 capability，具体账号及 profile 仍应实测。构建成功、模拟器结果与签名真机运行是不同验收层级。

需要真机测量 extension 峰值内存、启动时间、长时间后台、锁屏、网络切换、系统停止/重启、崩溃恢复和 IPv6；不预设固定内存上限。应用与 extension 同时读取或更新配置时要定义单一所有权、原子发布和版本检查，并验证 UI 终止时隧道仍按系统策略工作。

## 8. POC / MVP 范围与验收

### POC A：Android VPN 数据面（可与 PR 整合并行）

范围：一个固定内核版本、一个测试机 ABI、一个受控配置、一个可观测的代理出口、最小原生控制界面。重点验证平台/内核连接，不将实验中的硬编码作为最终架构。

通过条件：

- 真机安装、授权、连接、断开；另一应用的 TCP 连接经过指定出口，断开后网络行为符合明确策略。
- 使用可控 DNS 与 UDP 目标验证请求路径；分别检查 IPv4、IPv6 的预期行为及 DNS 泄漏。若某协议尚不支持，明确禁用或标示，不能声称完整支持。
- 外发代理连接不回环；授权撤销和内核启动失败释放 TUN、通知和资源，用户能看到失败原因。
- Wi-Fi/蜂窝切换、UI 进入后台/退出、服务终止和重新启动均有可观察的状态；重复启停无泄漏或残留。
- 记录设备型号、Android/API、ABI、构建版本、配置摘要、测试方法与实际结果；不记录订阅秘密。

### POC B：共享业务集成

在整合后的 PR 栈上，构造独立移动宿主，接入共享 profile、配置生成及真实 facade 的最小调用路径。复用原有事务和恢复语义；将系统 VPN 和内核差异放在适配器，验证配置提交、实际应用、失败和 UI 状态之间的对应关系。至少有真实配置导入、启停、节点选择、状态/错误显示；使用 fake adapter 的测试不能替代真机数据面验证。

### Android MVP

在 POC 通过后纳入：本地/订阅 profile 导入与更新、节点选择、连接状态、常见错误与恢复、基本日志、移动布局和持久化；确定 IPv4/IPv6、DNS、网络切换、后台、支持的 Android/API 与 ABI 矩阵。补充可重复的 CI、设备回归、16 KB、资源/电量和安全检查。每项功能以真实设备行为验收，明确不支持的平台特性。多内核、桌面托盘/快捷键、复杂编辑器、核心在线替换、流量历史和每应用分流可另行评估，不应默认进入首版。

### iOS POC / MVP

获取构建及签名条件后，先独立验证 provider 与选定内核的包路径、配置交接、启停、后台和 extension 内存；再接入共享业务及移动 UI。iOS MVP 的功能与 Android 可以共享产品目标，但以 iOS 平台允许的路由、DNS、后台和分发能力为准，不承诺 Android 功能逐项等价。

## 9. 测试与质量门槛

| 类型           | 应覆盖的关键证据                                                                                        |
| -------------- | ------------------------------------------------------------------------------------------------------- |
| 核心单元/actor | 配置构建、失败回滚、状态提交、真实返回值、可用/不支持能力、owner 取消/恢复；使用显式依赖和 fake ports。 |
| 桥接集成       | FFI 所有权与线程回调、FD 单一关闭、重复启停、错误传播、生命周期竞态；覆盖核心崩溃和部分启动失败。       |
| Android 真机   | 其他 App 的 TCP、UDP、DNS、IPv4/IPv6；网络切换、授权撤销、后台、进程死亡、重启和系统 always-on 语义。   |
| iOS 真机       | provider 启停、宿主退出、配置版本一致性、后台/锁屏、网络变化、系统终止和恢复、资源占用。                |
| UI             | 窄屏与横屏、安全区、键盘、系统返回、触摸、连接中/失败/重连状态，无桌面窗口控件误调用。                  |
| 构建/发布      | 锁定依赖可重建、原生库与包对齐、目标 ABI、签名、许可证和商店声明。                                      |

对照现有 [测试与审查标准](../development/testing.md)和[后端包边界](../development/backend-packages.md)；涉及包边界变更时运行规定的架构/依赖 gate。不得把静态分析、模拟数据或桌面 CI 说成移动端通过。

## 10. 发布、隐私及开放决策

Google Play 的 [VpnService 政策](https://support.google.com/googleplay/android-developer/answer/12564964)要求声明用途，规定相关披露、同意和到 VPN endpoint 的加密等条件；是否满足取决于最终功能和流量路径。Apple [App Review 5.4](https://developer.apple.com/app-store/review/guidelines/)对 VPN app 的组织开发者身份、数据声明、隐私及地区许可证有要求。政策是动态的，发布前按实际分发渠道重新审查。App Store 规则关于下载执行代码也需要针对项目脚本和内核更新能力评估，不应假设桌面更新/脚本能力可原样上线。

建议优先确认的决策：

1. Android 首台测试机型号、系统版本与 ABI，以及最低支持版本；目前只有“有真机”的信息。
2. POC 测试出口、可用于分享的脱敏配置及明确的直连/代理/DNS 预期。
3. 候选移动内核的固定源码版本、构建责任、许可证、公开接口及双平台一致性；是否允许为 Android/iOS 使用不同桥接。
4. 移动端的失败策略、IPv6/DNS 行为、always-on 和 per-app 分流是否属于首版。
5. iOS 可用 Mac、付费签名与后续分发方式；是否以 App Store 为目标。

## 11. 推荐推进顺序

1. **PR 栈整合**：确定 P01 冲突范围，按顺序整合并运行对应桌面和架构检查；复核 P08 未解决的资源安全事项。
2. **Android 技术尖峰**：固定内核候选，建立最小 `VpnService`/内核桥接，在真机证明数据通路和生命周期；记录不能支持的配置行为。
3. **共享核心接入**：基于已整合 core 与实际移动适配器建立最小 host，并检验完整事务和恢复链路。
4. **Android MVP**：加入必要 UI/配置体验、可靠性、设备矩阵与 CI，依据真机证据确定范围。
5. **iOS 技术尖峰**：签名环境具备后验证公开数据通路、内存和 Apple 平台约束，再接共享应用及 UI。

当前没有足够的移动编译、桥接及设备数据来给出可信工期或承诺两个平台的发布日期。每一阶段应以记录的验收结果决定下一阶段投入。
