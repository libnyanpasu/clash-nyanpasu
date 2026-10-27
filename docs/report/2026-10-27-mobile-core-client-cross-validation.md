# 移动代理内核与客户端交叉验证

日期：2026-10-27。配套阅读：[Android / iOS POC 与 MVP 调研报告](2026-10-27-mobile-poc-mvp-research.md)。本文补充固定源码版本的实现路径、证据及对 Nyanpasu 的选型影响；**不是任何候选的真机测试、性能测试或安全审计结果**。

## 1. 如何看待证据

本次使用代码、构建文件、系统官方资料及项目自己的文档互相核对。客户端功能介绍表示项目声称实现，源码说明某个固定提交中存在什么路径，两者均不自动证明在用户设备上可用。项目的 bug 注释反映其开发者描述的问题与处理，不能直接当成我们复现过的缺陷。Apple 的公开 API、推荐用途、签名能力、商店审核是不同问题；其他应用存在或上架不证明我们的实现也符合 Apple 的用途及审核要求。

固定 Git commit 的源码链接优先；发布版本、使用的内核版本、项目 main 和文档日期可能不同。网络库、协议功能会按构建 features 改变，比较时必须记录应用 commit、内核 commit、构建 feature、ABI、系统版本、配置和测试出口。

## 2. 结论和决策

Nyanpasu 当前支持的内核是 Clash Premium、Mihomo、Clash Rust、Meow（及部分 alpha），见 [ClashCore](../../backend/nyanpasu-config/src/application/clash_core.rs)。**第一轮应并行验证 Mihomo Android 桥与 Meow Android 桥**：前者更贴近现有 Mihomo 配置语义，后者是现有可选内核且已有 Rust 移动客户端。把“Rust 同语言”视为降低 FFI 和工具链复杂性的可能性，不视为配置完全兼容、低内存或免维护的证明。先通过同一组配置和流量验收，再决定哪一个作为 MVP 默认；不要同时承诺多个核心。

Meow iOS 当前源码提供值得深入的公开 `packetFlow` + Rust 网络栈路径，但实际还包含本地 mixed/DNS listener；Apple 对 Packet Tunnel provider 托管监听器及转发已接管流量的用途给出限制，这仍是 iOS 产品/分发决策。iOS 应与 Android 分开验收，不能把 Android 接口或 FD 假设套用过去。

跨内核不只是换共享库：Nyanpasu 的 profile 生成、Mihomo 规则处理、API 请求、持久化和控制状态均有现有语义。sing-box 和 Xray 是技术候选，但它们不是已接入的 Nyanpasu 核心，协议名重叠不代表配置、规则、DNS 或控制 API 等价；迁移前必须做明确的转换与兼容矩阵。

## 3. 已检查源码基线

| 客户端 / 核心         | 固定源码                                                                                                                                                                                                                                                                                                                                                | 检查范围                                                                   |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Meow Android          | [`4740012bf0ef`](https://github.com/meow-rs/meow-android/tree/4740012bf0efdb8bae8fbeeefaf4937dfa309f26)                                                                                                                                                                                                                                                 | VPN service、manifest、Rust FFI、TUN/DNS、protect 与 Cargo 配置。          |
| Meow iOS              | [`f8d7601924a7`](https://github.com/meow-rs/meow-ios/tree/f8d7601924a7be339cf8544a51a101f6158fbf60)                                                                                                                                                                                                                                                     | Packet Tunnel provider、packetFlow 读写、启动停止、Rust FFI、内核依赖。    |
| Meow 引擎             | iOS 固定 [`fa7a7d6dc94e`](https://github.com/meow-rs/meow-rs/tree/fa7a7d6dc94e886ae0bc43eeb0227082ae5bf2e6)，Android 固定 `v0.26.0`                                                                                                                                                                                                                     | 以各客户端的构建文件为准；不是同一个快照。                                 |
| Clash Rust            | [`36eec77463d1`](https://github.com/Watfaq-legacy/clash-rs/tree/36eec77463d1243ede287bf6b7197e38237948d5)                                                                                                                                                                                                                                               | `clash-ffi` C ABI、构建类型和 TUN 配置入口；没有证明完整移动 VPN 桥。      |
| Mihomo Android 系列   | CMFA [`1a4f9492`](https://github.com/MetaCubeX/ClashMetaForAndroid/tree/1a4f949213fcbb7f0c9e2a4a21138642d84b8f22)、FlClash [`7fd417fb`](https://github.com/chen08209/FlClash/tree/7fd417fb9a9a277c6a9339a8ac87f907df7c25f7)、libmihomo [`bd66e480`](https://github.com/oviron/libmihomo-android/tree/bd66e4800290791fd5653b482c6270356003b29d)          | 完整客户端与独立 AAR；具体内核和 FD 交接见下文。                           |
| ProjectClash iOS      | [Client `3b8ea95e`](https://github.com/ProjectClash/Clash-Client/tree/3b8ea95eff7cdfd817608fdba8079f4b03013148)、[Adapter `821b6e40`](https://github.com/ProjectClash/Clash-Adapter/tree/821b6e40f13b423f6e5d3017611851ef6955ca94)、[Kernel `a5cb38da`](https://github.com/ProjectClash/Clash/tree/a5cb38dab6123ff1dded608b0d7fdceca9027421)            | Client 锁文件中相互关联的源码；upstream Mihomo v1.19.32。                  |
| sing-box / 官方客户端 | [core `7054cac5` / v1.14.3](https://github.com/SagerNet/sing-box/tree/7054cac5124465ffdb145870d7c01885566711b1)、[Android `ae93152f`](https://github.com/SagerNet/sing-box-for-android/tree/ae93152f21457723af77d144f28c1f7be7362aec)、[Apple `e232aaf2`](https://github.com/SagerNet/sing-box-for-apple/tree/e232aaf22d6d68773196ffc8e6e79684a6be717c) | 两个客户端是所查 core 提交关联的子模块；未证明发布二进制对应。             |
| Xray / v2rayNG        | [v2rayNG `0f961c54`](https://github.com/2dust/v2rayNG/tree/0f961c54c8c9f85c5271778653340025d33772cc)、[AndroidLibXrayLite `76a6e217`](https://github.com/2dust/AndroidLibXrayLite/tree/76a6e2172210613fcb4aa32b6de55bcd9452ce43)、[Xray `1b1647cd`](https://github.com/XTLS/Xray-core/tree/1b1647cd9ddf0abc34b981d4611fc03bcac1ca74)                    | 前两者是子模块关系，绑定模块指向所列 Xray；未核实 CI 下载 AAR 与源码一致。 |

这些 commit 是源码快照，未核对它们分别对应的 App Store / Play Store 二进制，也未执行其测试。其他客户端与核心的固定基线见下文。

## 4. Meow Android：FD → lwIP → Rust 核心

源码路径为 `VpnService` → `Builder.establish()` → `ParcelFileDescriptor.fd` → JNI → lwIP 网络栈 → meow-rs TCP/UDP handler。该版本 [manifest](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/AndroidManifest.xml#L17)把 VPN service 放入独立 `:vpn` 进程，并声明前台服务类型；不能假设 UI 的 Rust 对象或 actor 能直接与它共享。其 [VpnService](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/java/io/github/madeye/meow/bg/VpnService.kt#L151)配置 IPv4 默认路由和 DNS，根据配置决定 IPv6；处理 per-app allow/disallow，建立接口后传入 `conn.fd`，撤销走停止路径。

这与上一个报告中 AAR 的 `detachFd()` 所有权契约不同：这个版本保留 `ParcelFileDescriptor`，并在服务清理时关闭。**不能混用两个项目的 FD 交接规则。** 移植时必须逐一验证 FD 是否借用、复制或转移；停止等待与 FD 关闭的顺序；失败和并发停止是否恰好释放一次。

[Rust protect 实现](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/rust/meow-android-ffi/src/protect.rs#L34)通过 JNI 调 `VpnService.protect(fd)`，把 `false` 转成 I/O 错误；[注册点](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/rust/meow-android-ffi/src/lib.rs#L617)安装 protect 后启动 TUN。源码利用内核进程内 registry/static；这属于上游实现事实，不能直接照搬为 Nyanpasu 应用层全局服务。需验证安装失败是否能阻止启动，以及 TCP、UDP、DNS、订阅/provider 等所有出站调用是否覆盖。

[TUN 数据面](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/rust/meow-android-ffi/src/tun2socks.rs#L138)使用 lwIP 接收 IP 包；TCP 调 meow 的 `handle_tcp`，UDP 调 `handle_udp`，UDP/53 在栈前拦截并调用内置 DNS resolver。不要因此假定任意 DNS-over-TLS、DNS-over-HTTPS、Private DNS 或所有非 TCP/UDP IP 协议都自动具备相同规则语义。停止实现有三秒等待并记录超时，不能把该时间视为 Nyanpasu 要采用的全局关闭期限，更不能将超时当成停止证明。

其 [Cargo 配置](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/rust/meow-android-ffi/Cargo.toml#L21)固定 meow-rs `v0.26.0` 和单独的 lwIP fork revision；注释记录了 lwIP 状态、waker 和 idle 问题及补丁。复用 Rust 内核仍需维护原生 C 网络栈、Rust 依赖、JNI、ABI 和 feature 矩阵。

一个具体平台坑：该版本在 API 29 以下观察并设置 underlying network，API 29 及以上不主动指定，让系统跟随默认网络；代码注释描述显式声明旧 Wi-Fi 导致切换后错误离线的案例。[源码](https://github.com/meow-rs/meow-android/blob/4740012bf0efdb8bae8fbeeefaf4937dfa309f26/core/src/main/java/io/github/madeye/meow/bg/VpnService.kt#L219) 这是此客户端的做法，仍需在目标 Android 版本及 OEM 真机验证。

## 5. Meow iOS：packetFlow → lwIP → 本地内核

[Provider](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/PacketTunnel/Sources/PacketTunnelProvider.m#L60)把 `self.packetFlow` 交给 `MWTunnelEngine`。[Ingress](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/PacketTunnel/Sources/MWTunnelEngine.m#L258)调用 `readPacketsWithCompletionHandler` 并将每个原始 IP 包传给 `meow_tun_ingest`；[writer](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/PacketTunnel/Sources/MWPacketWriter.m#L26)通过 `writePackets` 返回数据。这里查看到的是公开 packetFlow 回调路径，而不是从系统私有对象获取 utun fd；这仅限已检查代码路径，不能证明整个应用所有接口都符合审核要求。

此版本的 [Rust 数据面](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/core/rust/meow-ios-ffi/src/tun2socks.rs#L1)经 lwIP 用户态栈处理 TCP/UDP，再通过本地 mixed listener 的 SOCKS5 和 DNS listener 接入 meow 引擎；[engine](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/core/rust/meow-ios-ffi/src/engine.rs#L488)实际启动 mixed、DNS 和 API 相关任务。旧文档可能描述 in-process 直接通道，源码顶部部分注释仍提到 smoltcp；具体依赖是 Cargo 中固定的 [`meow-lwip`](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/core/rust/meow-ios-ffi/Cargo.toml#L73)。以固定提交的实际调用和依赖为准，不能用早期 PRD 证明当前路径。

这一实现暴露了真实设计权衡：

- **系统政策：** [Apple TN3120](https://developer.apple.com/documentation/technotes/tn3120-expected-use-cases-for-network-extension-packet-tunnel-providers)对 provider 内托管监听器，以及将接管流量转发到其他接口的用途提出限制。项目代码确实使用相关路径；是否适合我们的产品及分发渠道应向 Apple 确认，不能从其他客户端存在推断认可。
- **停止竞态：** [停止代码](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/PacketTunnel/Sources/MWTunnelEngine.m#L216)等待 TUN 任务结束后才释放 writer 上下文，注释记录此前回调仍在运行时释放对象会产生 use-after-free。停止、网络回调和 FFI 上下文所有权需要一个串行 owner 及可证明的终态。
- **重复 read：** ingress generation guard 防止旧回调在停止后重新启动第二条读取链；换配置或网络重连需要区分旧/新 generation。
- **DNS 与 UDP：** 代码含 fake-IP、AAAA、DNS 请求并发上限和 SOCKS5 UDP session 的处理；应用协议、IPv6、QUIC 和大量 UDP session 都要真实测试。不要从旧文档中“UDP 未实现”或项目首页的协议列表推断当前提交的完整行为。
- **资源与配置：** 引擎在启动前移除部分 listener 字段，并重写可能越界的 provider 路径；[配置处理](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/core/rust/meow-ios-ffi/src/engine.rs#L90)明确指出直接序列化已知结构会丢未知 YAML 字段。客户端会改变配置，与桌面原样运行存在差异，应单独记录可见效果。
- **兼容矩阵：** 本版本 [project.yml](https://github.com/meow-rs/meow-ios/blob/f8d7601924a7be339cf8544a51a101f6158fbf60/project.yml#L5)为 iOS 18；旧计划或其他版本的最低版本不能代表此版本。内存阈值应以实机记录，而不是将项目预算写成 Apple 对所有设备的保证。

## 6. Mihomo：三个 Android 客户端的真实差别

三个项目都在 Android 应用内嵌入 Go Mihomo 家族，而不是仅启动桌面程序。CMFA 的 Go module 表面的 `v1.7.0` 被本地子模块 replace，实际为 [Mihomo `3f27ef39`](https://github.com/MetaCubeX/ClashMetaForAndroid/blob/1a4f949213fcbb7f0c9e2a4a21138642d84b8f22/core/src/main/golang/go.mod#L5-L11)；FlClash v0.9.2 使用 [作者的 Clash.Meta fork](https://github.com/chen08209/FlClash/blob/7fd417fb9a9a277c6a9339a8ac87f907df7c25f7/core/go.mod#L1-L11)而非可从占位版本读出的官方 release；libmihomo v0.3.12 则在 [go.mod](https://github.com/oviron/libmihomo-android/blob/bd66e4800290791fd5653b482c6270356003b29d/src/main/jni/core/go.mod#L5-L10)声明上游 v1.19.33。源码 SHA 和已发布 APK/AAR 的对应性仍未复现验证。

| 维度     | CMFA                                                | FlClash v0.9.2                                     | libmihomo-android v0.3.12                                        |
| -------- | --------------------------------------------------- | -------------------------------------------------- | ---------------------------------------------------------------- |
| 形态     | 完整 Android 客户端，Go 共享库和 JNI                | 完整客户端，gomobile AAR                           | 独立 AAR；VPN 授权、服务、路由和网络观察由宿主实现               |
| TUN      | `detachFd()` → Mihomo `sing_tun`，由 Android 设路由 | `detachFd()` → Go 复制 fd 并关闭原始 fd → listener | 按 API 契约接收 detach 后的 fd → listener                        |
| protect  | Kotlin 有 Boolean，但 JNI 路径丢掉结果              | 回调有 Boolean，Go 调用后不检查                    | Kotlin 接口返回 Unit，无法从此接口检查 Android 结果              |
| 启动结果 | Go 有错误码，所查 JNI 丢弃                          | TUN 启动返回 Boolean，宿主检查并清理               | `startTUN` 不返回结果，Go 失败记日志；宿主不能从调用返回推断就绪 |

依据：[CMFA TUN](https://github.com/MetaCubeX/ClashMetaForAndroid/blob/1a4f949213fcbb7f0c9e2a4a21138642d84b8f22/core/src/main/golang/native/tun/tun.go#L45-L76)、[CMFA JNI](https://github.com/MetaCubeX/ClashMetaForAndroid/blob/1a4f949213fcbb7f0c9e2a4a21138642d84b8f22/core/src/main/cpp/main.c#L112-L129)、[FlClash FD](https://github.com/chen08209/FlClash/blob/7fd417fb9a9a277c6a9339a8ac87f907df7c25f7/core/tun/tun.go#L77-L103)、[FlClash protect](https://github.com/chen08209/FlClash/blob/7fd417fb9a9a277c6a9339a8ac87f907df7c25f7/core/lib.go#L85-L96)、[AAR 接口](https://github.com/oviron/libmihomo-android/blob/bd66e4800290791fd5653b482c6270356003b29d/src/main/kotlin/io/github/oviron/libmihomo/TunInterface.kt#L1-L5)。这些源码不证明实际发生流量回环；它们证明失败传播不足以支持我们拟定的严格状态契约。

另外两个具体问题影响直接采用 AAR：

- **潜在 fd 泄漏：** [解析地址失败](https://github.com/oviron/libmihomo-android/blob/bd66e4800290791fd5653b482c6270356003b29d/src/main/jni/core/tun/tun.go#L36-L44)早于 listener 建立，而[调用方清理](https://github.com/oviron/libmihomo-android/blob/bd66e4800290791fd5653b482c6270356003b29d/src/main/jni/core/lib.go#L56-L86)只显示关闭已有 listener；静态检查无法证明该分支关闭传入 fd。需要构造失败实验和审计完整所有权链，尚不能断言已复现。
- **README 与接口不一致：** [示例](https://github.com/oviron/libmihomo-android/blob/bd66e4800290791fd5653b482c6270356003b29d/README.md#L126-L132)使用 `homeDir`、`profile`，实际 [JSON 字段](https://github.com/oviron/libmihomo-android/blob/bd66e4800290791fd5653b482c6270356003b29d/src/main/jni/core/constant.go#L15-L22)为 `home-dir` 等，配置从 home 下固定 `config.yaml` 加载。按源码不能假设示例字段会设置预期目录和 profile；未运行示例验证。

三者的 DNS 也不能一概而论：CMFA 公布虚拟 DNS `172.19.0.2`、FlClash 使用 `198.18.0.2`；它们另外将 DNS 劫持参数传给内核，并分别监听非 VPN 网络 DNS 变化。FlClash 对网络切换进行去抖和连接重置；库只提供 `updateDNS`，不替宿主管理网络观察。[CMFA 网络观察](https://github.com/MetaCubeX/ClashMetaForAndroid/blob/1a4f949213fcbb7f0c9e2a4a21138642d84b8f22/service/src/main/java/com/github/kr328/clash/service/clash/module/NetworkObserveModule.kt#L119-L127)、[FlClash 网络观察](https://github.com/chen08209/FlClash/blob/7fd417fb9a9a277c6a9339a8ac87f907df7c25f7/android/service/src/main/java/com/follow/clash/service/NetworkObserver.kt#L119-L138)。CMFA 还会修改 DNS、关闭配置内 TUN 并过滤桌面 listener；FlClash 与库解析失败时应用默认配置但同时返回错误。[CMFA 配置处理](https://github.com/MetaCubeX/ClashMetaForAndroid/blob/1a4f949213fcbb7f0c9e2a4a21138642d84b8f22/core/src/main/golang/native/config/process.go#L68-L106)、[FlClash 失败行为](https://github.com/chen08209/FlClash/blob/7fd417fb9a9a277c6a9339a8ac87f907df7c25f7/core/config.go#L151-L183)。因此“内核有一个运行配置”不等于“用户选择的配置已成功生效”。

## 7. Mihomo：iOS 客户端的不同包路径

ProjectClash 的 [Client 锁文件](https://github.com/ProjectClash/Clash-Client/blob/3b8ea95eff7cdfd817608fdba8079f4b03013148/Dependencies.lock.json#L1-L12)固定上表的 Adapter 与内核：extension 应用网络设置后，Adapter 调用 `socketpair(AF_UNIX, SOCK_DGRAM)`；`packetFlow.readPackets` 取得系统包，附加地址族头发送到自建 socketpair，另一方向调用 `writePackets`；内核 `dup` 传入 fd 并将其交给 TUN。[创建桥](https://github.com/ProjectClash/Clash-Adapter/blob/821b6e40f13b423f6e5d3017611851ef6955ca94/Sources/ClashAdapter/PacketFlowBridge.swift#L405-L436)、[readPackets](https://github.com/ProjectClash/Clash-Adapter/blob/821b6e40f13b423f6e5d3017611851ef6955ca94/Sources/ClashAdapter/PacketFlowBridge.swift#L573-L603)、[内核接入](https://github.com/ProjectClash/Clash/blob/a5cb38dab6123ff1dded608b0d7fdceca9027421/bind/clash/service.go#L330-L342)。这是**应用自己创建的 fd**，不能与 Apple 系统的 utun fd 混淆。

App 与 extension 另用 `sendProviderMessage` 和 App Group 下 `clash.sock` 进行控制；这个 Unix 控制 socket 与 extension 内部的 packet socketpair 是两条不同通道。[消息](https://github.com/ProjectClash/Clash-Client/blob/3b8ea95eff7cdfd817608fdba8079f4b03013148/apple/ClashClient/Sources/ClashClient.swift#L664-L695)、[控制 socket](https://github.com/ProjectClash/Clash/blob/a5cb38dab6123ff1dded608b0d7fdceca9027421/bind/clash/clash_api.go#L43-L86)。Adapter 的一个排队上限为 1,024 包、4 MiB，超限会丢包；这是局部队列，不是进程内存上限。[队列](https://github.com/ProjectClash/Clash-Adapter/blob/821b6e40f13b423f6e5d3017611851ef6955ca94/Sources/ClashAdapter/PacketFlowBridge.swift#L607-L615)

内核使用 Go `debug.SetMemoryLimit` 的默认 37.5 MiB 软目标作为自身 50 MiB 预算假设的 3/4；该值不是整个 extension 的硬上限或 Apple 保证。[实现](https://github.com/ProjectClash/Clash/blob/a5cb38dab6123ff1dded608b0d7fdceca9027421/bind/clash/memory_ne_pacing.go#L9-L45)、[Go GC 语义](https://go.dev/doc/gc-guide)。已捕获流量命中 `DIRECT` 后由内核建立新的目的地 TCP/UDP socket；它不是系统路由层未进入 tunnel 的 bypass。[DIRECT 实现](https://github.com/ProjectClash/Clash/blob/a5cb38dab6123ff1dded608b0d7fdceca9027421/adapter/outbound/direct.go#L24-L67)

对照 [Clash Mi 固定 Swift 实现](https://github.com/KaringX/clashmi/blob/0813a98f230f68154db7a7553c3ae223e4780e00/bind/apple/LibVpnCore/ExtensionPlatformInterface.swift#L193-L226)：先用 KVC 取 `packetFlow.socket.fileDescriptor`，失败扫描 utun fd；这不是公开 packetFlow 包桥。该仓库引用的 `Libclash.xcframework` 和相邻服务依赖没有包含在所查快照，无法闭合内核 SHA、内存策略和 DIRECT 细节。Karing 的 [项目说明](https://github.com/KaringX/karing/blob/47f9769262b9135aa7a462af7fd2c95b4cd8b8ca/README.md#L9-L31)称其使用修改版 sing-box，不能把它当作 Mihomo，也不能把 Clash Mi 的 FD 实现直接套到 Karing。

## 8. sing-box / libbox 与 Xray 的移动实现

**sing-box。** 所查 [libbox 构建脚本](https://github.com/SagerNet/sing-box/blob/7054cac5124465ffdb145870d7c01885566711b1/cmd/internal/build_libbox/main.go#L60-L70)使用 gomobile 生成 Android AAR 和 Apple XCFramework；Android legacy AAR 的 build tags 与普通版不同。SFA `VpnService` 建立 TUN，libbox 复制 fd；SFA 调 `protect(fd)` 但所查路径没有检查其 Boolean。[libbox FD](https://github.com/SagerNet/sing-box/blob/7054cac5124465ffdb145870d7c01885566711b1/experimental/libbox/service.go#L47-L87)、[SFA](https://github.com/SagerNet/sing-box-for-android/blob/ae93152f21457723af77d144f28c1f7be7362aec/app/src/main/java/io/nekohasekai/sfa/bg/VPNService.kt#L51-L59)。

固定 SFI [Apple 平台接口](https://github.com/SagerNet/sing-box-for-apple/blob/e232aaf22d6d68773196ffc8e6e79684a6be717c/Library/Network/ExtensionPlatformInterface.swift#L208-L228)用 KVC 获取系统 tunnel fd，失败时回退到扫描；Apple DTS 表示这种取 fd 方式不受支持。[Apple DTS](https://developer.apple.com/forums/thread/13503) 这是一项平台兼容及审查风险，并不证明某个既有版本必然被拒。SFA/SFI 是很有价值的系统接入参考，但不应将这条 iOS 路径当作公开 API 保证。

**Xray / v2rayNG。** 固定版本的 v2rayNG 有两条 Android 路径：默认通过 HEV tun2socks 将系统 fd 接到本地 SOCKS，再进入 Xray；关闭 HEV 后将 fd 传给 Xray TUN inbound。[默认值](https://github.com/2dust/v2rayNG/blob/0f961c54c8c9f85c5271778653340025d33772cc/V2rayNG/app/src/main/java/com/v2ray/ang/handler/SettingsManager.kt#L409-L415)、[模式分支](https://github.com/2dust/v2rayNG/blob/0f961c54c8c9f85c5271778653340025d33772cc/V2rayNG/app/src/main/java/com/v2ray/ang/core/CoreServiceManager.kt#L140-L156)。所查代码的主要可证实防回环方式是排除自身应用；虽然存在 `vpnProtect` 方法，未找到该固定调用链对它的调用，不应据方法存在宣称所有出站 socket 已保护。[排除应用](https://github.com/2dust/v2rayNG/blob/0f961c54c8c9f85c5271778653340025d33772cc/V2rayNG/app/src/main/java/com/v2ray/ang/service/CoreVpnService.kt#L256-L294)

独立 [XTLS/libXray](https://github.com/XTLS/libXray/blob/756ef80f41903eaf7bbeeb38a0a1f802ec204464/controller/controller_android.go#L13-L26)有可传播 protect 失败的 Android controller，但它不是上述 v2rayNG 固定的绑定子模块。此轮没有闭合 Xray iOS 客户端从系统包到运行核心的完整公开调用链，不将其视为 iOS 方案已验证。

**兼容与供应链。** sing-box 的 Clash API build tag 不等于支持 Mihomo YAML；sing-box 与 Xray 的 JSON 配置、路由和 DNS 模型也不同。一个可核验的例子：固定 [Xray 有 XHTTP](https://github.com/XTLS/Xray-core/blob/1b1647cd9ddf0abc34b981d4611fc03bcac1ca74/infra/conf/transport_internet.go)，固定 [sing-box 的 transport 枚举](https://github.com/SagerNet/sing-box/blob/7054cac5124465ffdb145870d7c01885566711b1/option/v2ray_transport.go#L14-L20)没有 `xhttp`。其他同名协议也必须按传输、TLS、UDP 和服务端版本测试，不能以协议名称判断等价。

所查 v2rayNG 的 [CI](https://github.com/2dust/v2rayNG/blob/0f961c54c8c9f85c5271778653340025d33772cc/.github/workflows/build.yml#L67-L81)根据子模块 tag 下载预编译 AAR；源码固定不自动证明二进制与其一致。[AndroidLibXrayLite 工作流](https://github.com/2dust/AndroidLibXrayLite/blob/76a6e2172210613fcb4aa32b6de55bcd9452ce43/.github/workflows/main.yml#L29-L55)使用 `gomobile@latest`，还需要锁工具链。libXray 自述 API 针对最新 Xray，且提醒同进程加载多个独立 Go runtime 的风险；若将多个 Go 核心集成在同一宿主必须先验证运行时和进程边界。[libXray README](https://github.com/XTLS/libXray/blob/756ef80f41903eaf7bbeeb38a0a1f802ec204464/README.md)

许可证需要按整个构建组合审查：所查 [sing-box](https://github.com/SagerNet/sing-box/blob/7054cac5124465ffdb145870d7c01885566711b1/LICENSE) 及官方客户端使用 GPL-3.0-or-later；[Xray-core](https://github.com/XTLS/Xray-core/blob/1b1647cd9ddf0abc34b981d4611fc03bcac1ca74/LICENSE) 为 MPL-2.0，[AndroidLibXrayLite](https://github.com/2dust/AndroidLibXrayLite/blob/76a6e2172210613fcb4aa32b6de55bcd9452ce43/LICENSE) 包含 LGPLv3 文本，[HEV](https://github.com/heiher/hev-socks5-tunnel/blob/c5584feca8f6df70ee53eb2879e034b849a3d15a/LICENSE) 为 MIT；Meow 仓库标示 MIT，但其传递依赖也需逐一审查。组合后的源码提供、修改、静态/动态链接与商店分发义务不能只看最宽松的一项，本报告不作法律结论。

## 9. Clash Rust / Clash Premium

当前 [clash-ffi](https://github.com/Watfaq-legacy/clash-rs/blob/36eec77463d1243ede287bf6b7197e38237948d5/clash-ffi/src/lib.rs#L10)有 `clash_start`、`clash_shutdown`、字符串释放 C ABI，且 [Cargo](https://github.com/Watfaq-legacy/clash-rs/blob/36eec77463d1243ede287bf6b7197e38237948d5/clash-ffi/Cargo.toml)构建 staticlib/cdylib。已检查的 FFI 表面没有 TUN fd 注入、Android protect 或 iOS packetFlow 桥；这不是对整个生态不存在这些实现的证明。它可以是后续研究候选，但目前缺少像 Meow 和 Mihomo 客户端这样已定位的端到端移动接入证据。选用前仍需找到具体实现并验证配置、生命周期和网络。

Clash Premium 在 Nyanpasu 的桌面核心列表中，但本次没有找到可供审计并集成的移动 SDK/源码证据；不把它列为移动首轮可行方案，也不从缺失证据推断它一定无法运行。

## 10. 核心比较必须验证什么

| 比较项            | 验证方法                                                                             | 失败时意味着什么                                                   |
| ----------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| YAML / 配置兼容   | 对同一脱敏 profile，分别记录加载、生成结果、诊断和每个规则分支                       | 同名协议不代表同一配置可运行；必须标示不支持项，不能静默删除。     |
| Clash controller  | 比对节点/组、provider、规则、配置应用、连接、流量、日志和 WebSocket/SSE 的请求与响应 | 仅支持 `/version` 或 `/proxies` 不等于满足 Nyanpasu 的控制调用链。 |
| 资源文件和脚本    | 检查 rule/proxy provider、GeoIP/GeoSite、路径限制、JS/Lua 执行及更新                 | 移动沙盒、离线启动和商店政策会改变实际可用性。                     |
| DNS/IPv6          | 可控 DNS、A/AAAA/HTTPS/SVCB、fake-IP、Private DNS、无 IPv6 和 IPv6-only 网络         | 防止看似可用但泄漏、回环、回退或节点不可达。                       |
| TCP/UDP           | 浏览、流式长连接、UDP/QUIC 和并发；切换网络并中断上游                                | 用户态网络栈可能受连接数、NAT、超时、背压或协议支持限制。          |
| 生命周期          | 授权拒绝、重复启停、启动中停止、服务撤销、进程杀死、断网及恢复                       | 必须能解释配置提交、内核、VPN 和 UI 状态为何一致或降级。           |
| CPU / 内存 / 电量 | 同设备、同配置、同协议、同流量负载和时长，记录峰值、内存 footprint、丢包和热/电量    | 语言、README 或桌面基准不能代替跨端同条件测量。                    |

对照仓库已有 [runtime builder](../../backend/nyanpasu-core/src/enhance/runtime_builder.rs)、[include-all 展开](../../backend/nyanpasu-config/src/runtime/executor/include_all.rs)、[clash-api](../../backend/nyanpasu-runtime/crates/clash-api) 和项目 [core manager](../../backend/nyanpasu-runtime/crates/nyanpasu-core-manager/README.md)。测试须区分同一份配置的解析成功、规则实际命中、数据确实走预期出口三个层级。

## 11. 交叉验证实施建议

1. 冻结每个候选的源码、依赖树和许可证；建立相同的 Android ARM64 受控实验，不先改 Nyanpasu 业务层。
2. 同时为 Mihomo AAR 与 Meow FFI 画出 `VpnService → FD → 核心` 的所有权、启动成功/失败和 socket 保护流程；测试不支持的操作、重复停止与撤销。
3. 用同一组最小但有区分度的配置测试：代理/DIRECT、节点切换、域名规则、provider、TCP、UDP、DNS、IPv6、网络切换；真实出口用可控服务端验证，记录不兼容字段。
4. 在解耦栈整合后，用独立外部 consumer 检查真实 `NyanpasuClient` 构造、runtime 提交与状态观察；移动宿主提供显式 port，避免伪造进程和 PID。
5. iOS 在可用签名真机后，分别验证公开包桥、宿主/extension 配置一致性、后台、峰值内存和苹果用途限制；优先向 Apple 确认实际数据路径及监听器的发布适用性。

建议第一轮实验保存一份可重跑的结果表：设备/API/ABI、各候选的源码 SHA 与二进制摘要、构建开关、配置修订、出口和 DNS 服务器、各测试用例预期/实际、失败日志和资源峰值。对 AAR 特别注入无效 CIDR、`protect` 返回 false、配置解析失败、启动中撤销、双重停止和网络从 Wi-Fi 切到蜂窝；记录 TUN fd 是否仍存在、是否有回环、调用者是否收到明确失败。只在获得设备和实验结果后更新“已验证”状态。

风险与下一步应基于实验结果重新排序；本文没有为候选背书，也没有以源码静态检查替代实际手机验证。
