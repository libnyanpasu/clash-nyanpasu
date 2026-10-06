# Clash-rs Meow Mihomo 代理组接口核查报告

> 本报告保留调查基线的接口与问题描述；文中的“当前”指调查时的代码。后续实现、能力边界及验收结果见 [代理组发现 spec](../spec/2026-10-05-proxy-group-discovery/design.md)。

本报告依据项目 **clash-api 1.0.0-rc.9 的现有方法和 DTO**，对照三个上游的实现及 `C:\Users\a6320\Downloads\REPORT.md` 复核兼容性。已确认：Mihomo 的组列表可由现有 `Client::groups()` 的数组契约表达；Meow 的组列表是 map；Clash-rs 在核查版本中没有组列表 GET。`GLOBAL.all` 是成员关系，不应作为组全集。

三个实测二进制的无尾斜杠 `/proxies` 与 `/providers/proxies` 均可访问，可作为修正客户端后的共同读取候选。**这不等于当前 crate 已能直接读取三核，也不是全接口兼容性结论。** 当前方法带尾斜杠，Meow 与 Clash-rs 的对应 GET 返回 404；这与 Mihomo 上有效的既有 API 定义应分开描述。

核查日期：2026-10-05。算法建议另见 [issue 5112 代理组算法重构报告](2026-10-05-issue-5112-proxy-algorithm-report.md)。原始 HTTP 响应、测试配置、二进制 SHA-256、源码提交及模型检查结果见 [核查证据](2026-10-05-proxy-core-api-evidence.json)。

## 仓库克隆和更新结果

按主要实现语言存放，已有 Meow 工作树更新前是干净状态，使用 fetch 与 fast-forward 更新，没有覆盖本地修改。

| 内核     | 上游                                                    | 本地目录                    | 本次操作与最终提交                                                                                     |
| -------- | ------------------------------------------------------- | --------------------------- | ------------------------------------------------------------------------------------------------------ |
| Clash-rs | [ibigbug/clash-rs](https://github.com/ibigbug/clash-rs) | `G:\Programs\Rust\clash-rs` | 新克隆，`master`，`0cf7ed5f6a99a6ea6d62afd9188963657f796d4c`                                           |
| Meow     | [madeye/meow-rs](https://github.com/madeye/meow-rs)     | `G:\Programs\Rust\meow-rs`  | `main` 从 `7b20befaf6fe578ddc55ea774071c423ea18ba80` 快进至 `3c27aca92d64c7194c6b590e529da46465fbb7eb` |
| Mihomo   | [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) | `G:\Programs\Go\mihomo`     | 新克隆，切换至跟踪 `origin/Meta` 的 `Meta`，`88dcbf7f1614a67c3b36b848ee3592dfa92ada36`                 |

Mihomo 默认 `main` 当前只含仓库说明等文件，不能将它误认为内核源码分支。`origin/Alpha` 为 `9f053c49075de076d83d3e5918241e410f7e9adf`，已另外核查其组和代理路由与 Meta 的差异；这两个路由文件没有差异。最终三个工作树均干净。

应用工作区基准是 `3b942c760c74df0a7fa832dc455c3a15b676cb3b`，运行时子模块是 `889901cb57b1b1753354c2b7b0a442569601e417`。本次只新增报告和证据文件。

## 版本和证据边界

| 项目     | 本轮核查的源码                                                                                                                                                                       | 本轮独立运行的现有 sidecar               |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------- |
| Mihomo   | `v1.19.32` 与 Meta 均指向 `88dcbf7…`；Alpha `9f053c4…`                                                                                                                               | **v1.19.30**                             |
| Clash-rs | `v0.10.8` 为 `3ed728b9d54fd29049bb09c715a76ff9d0c163ff`；manifest Alpha 为 `39d06a49ccb5c812ed7cd70b3028f3efcebeae6b`；本轮 `master`、`v0.10.9`、移动标签 `latest` 均指向 `0cf7ed5…` | **0.10.8**                               |
| Meow     | `v0.22.0` 为 `4b26a0b5d0cd893e11af71b07e3c6c145c66ce88`；main 为 `3c27aca…`，相关组路由和处理函数未变                                                                                | **0.21.2**，HTTP `/version` 为 `v0.21.2` |

以上二进制来自本工作区现有 sidecar，未替换或下载覆盖。它们不是本轮最新源码构建产物。输入报告的六个二进制结果属于先前证据，本轮不将其计作新实测；Premium 不在本轮三个源码仓库核查范围内。

当前 [manifest/version.json](../../manifest/version.json) 仍指定 Clash-rs stable `v0.10.8`、Alpha `0.10.8-alpha+sha.39d06a4`，与移动 `latest` 已不相同。版本标签、下载通道和实际执行文件版本必须分别记录。

## 项目 API 定义及逐核对应

客户端定义来自 [clash-api/src/api/proxies.rs](../../backend/nyanpasu-runtime/crates/clash-api/src/api/proxies.rs)，基准子模块提交为 `889901cb57b1b1753354c2b7b0a442569601e417`。公共 DTO 为 Proxy、ProxyProvider、ProxyName、ProviderName 等；不存在另一份公开 Group DTO。Proxy 的 all 是 `Option<Vec<ProxyName>>`，now 是 `Option<ProxyName>`，节点与组共用 Proxy。

| 现有客户端调用                         | 当前 URL 与返回类型                                                 | Mihomo 对应行为          | Meow 对应行为                                                     | Clash-rs 对应行为                                    |
| -------------------------------------- | ------------------------------------------------------------------- | ------------------------ | ----------------------------------------------------------------- | ---------------------------------------------------- |
| `groups()`                             | `GET /group/` → `Vec<Proxy>`，从 proxies envelope 解包              | 与数组契约相符           | 带斜杠路径 404；改成 `/group` 后仍是 map，不能直接用现有 Vec 解码 | 无列表路由                                           |
| `group(&ProxyName)`                    | `GET /group/{name}/` → `Proxy`                                      | 有组详情路由             | 无尾斜杠详情路由存在；已测 Foo 带斜杠为 404                       | 无组详情路由                                         |
| `proxies()`                            | `GET /proxies/` → `IndexMap<ProxyName, Proxy>`                      | 已测 200                 | 已测 404；无尾斜杠为 200、名称 map                                | 已测 404；无尾斜杠为 200、名称 map                   |
| `proxy_providers()`                    | `GET /providers/proxies/` → `IndexMap<ProviderName, ProxyProvider>` | 已测 200                 | 已测 404；无尾斜杠为 200、名称 map                                | 已测 404；无尾斜杠为 200、名称 map                   |
| `group_delay(&ProxyName, &DelayQuery)` | `GET /group/{name}/delay` → `IndexMap<ProxyName, u16>`              | 成功响应按成员名返回延迟 | 按成员名返回延迟，但参数与失败处理并不完全相同                    | 成功响应只有组名对应的一个延迟，不能当成成员测速结果 |

表中状态码来自下文列出的现有二进制；最新固定源码用于核对注册和处理逻辑。组测速仅作源码核查，未发送请求。相同 map envelope 不证明每个字段、错误、写操作或测速语义兼容。本轮没有用 crate 实际执行全套三核集成测试。

`Client::endpoint` 保留末尾斜杠，`endpoint_with_segments` 中的空末尾段会主动追加斜杠。[client.rs 的 URL 测试](../../backend/nyanpasu-runtime/crates/clash-api/src/client.rs) 与 [REST mock 测试](../../backend/nyanpasu-runtime/crates/clash-api/tests/rest.rs) 也固定了这种路径；[Mihomo 集成测试](../../backend/nyanpasu-runtime/crates/clash-api/tests/mihomo.rs) 已使用 groups/group/proxies/provider 等公共方法。这些定义不是猜测出的路由。跨内核修正应在现有 crate 内连同测试一起完成，不在应用另写一套请求。

## 固定源码确认的服务端路由

以下表格描述本轮固定提交的服务端注册和响应外形，使用无尾斜杠 URL 展示。它不是当前 crate 实际发送路径的列表，也不代表已通过 typed client 验证；精确状态码见二进制实测一节。表中的 Proxy/Provider 表示服务端记录，不意味着三者字段完全相同。

| 路径和方法                     | Mihomo                            | Meow                              | Clash-rs                   |
| ------------------------------ | --------------------------------- | --------------------------------- | -------------------------- |
| `GET /proxies`                 | `{"proxies": {name: Proxy}}`      | 同样是名称索引对象                | 同样是名称索引对象         |
| `GET /proxies/{name}`          | 单个运行时代理或组                | 单个运行时代理或组                | 单个运行时代理或组         |
| `GET /providers/proxies`       | `{"providers": {name: Provider}}` | 同样是名称索引对象                | 同样是名称索引对象         |
| `GET /group`                   | `{"proxies": [Proxy, ...]}`       | `{"proxies": {name: ProxyInfo}}`  | 没有 root 列举路由         |
| `GET /group/{name}`            | 组详情，普通节点拒绝              | 组详情，普通节点拒绝              | 没有详情 GET               |
| `GET /group/{name}/delay`      | 已注册                            | 已注册                            | 已注册；并不代表另有组列表 |
| `GET /api/proxy-groups`        | 未注册                            | 直接 `ProxyGroupInfo[]`           | 未注册                     |
| `GET /api/proxy-groups/{name}` | 未注册                            | 该资源注册 PUT/DELETE，未注册 GET | 未注册                     |
| `GET /proxy/group`             | 未找到注册                        | 未找到注册                        | 未找到注册                 |

`/proxies/group` 和 `/proxies/groups` 都可匹配按名称查找的参数路由。前者是名称为 `group` 的单个资源，后者仅在存在名称为 `groups` 的资源时才可能成功；均不是专用组列表路由。

### Mihomo 的运行时组列表

[`groupRouter` 与 `getGroups`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/route/groups.go#L20) 遍历 `tunnel.Proxies()`，通过 `outboundgroup.ProxyGroup` 接口判断组，再返回数组。它不依据 `GLOBAL.all`，因此能列出 GLOBAL 子集之外的组，也包含运行时 GLOBAL。

数组来自 Go map 迭代，代码没有按配置排序。不要把返回数组位置当成稳定顺序或直接送进变化指纹。`/proxies` 的 JSON 对象键即使在特定编码器下有序，也不是跨内核的展示排序契约。

[`parseProxies`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/config/config.go#L887) 记录是否显式声明 GLOBAL，仅在没有声明时合成它。自定义 GLOBAL 的成员不会自动代表其他所有组。

组接口不承诺附带 provider 节点的完整记录。[providers 路由](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/route/provider.go#L18) 仍是补充 provider 节点元数据的必要来源。

### Meow 的两种组列表

[`get_groups`](https://github.com/madeye/meow-rs/blob/3c27aca92d64c7194c6b590e529da46465fbb7eb/crates/meow-api/src/routes.rs#L457) 从运行时 route snapshot 取代理，筛选 `members().is_some()`，复用 `ProxyInfo` 返回 **HashMap**。`all: []` 仍表示一个组。相同 URL 名称不代表与 Mihomo 的数组解码兼容；HashMap 的输出顺序也不能依赖。

[`get_proxy_groups`](https://github.com/madeye/meow-rs/blob/3c27aca92d64c7194c6b590e529da46465fbb7eb/crates/meow-api/src/routes.rs#L1971) 遍历 `raw_config.proxy_groups`，保持该声明列表顺序；成员优先取运行时 `members()`，失败才退回声明的 `proxies`。因此它既不是纯静态 YAML，也不是运行时组全集的严格证明。

管理扩展只输出 `name/type/proxies/now/url/interval/tolerance`，组类型为配置字符串如 `select`，成员字段叫 `proxies`。兼容接口输出运行时类型如 `Selector` 和 `all/history/udp/alive` 等字段。不能直接将管理扩展反序列化成运行时 Proxy。

未显式声明的合成 GLOBAL 不会被 raw 配置枚举出来；声明存在而运行时对象缺失时，扩展仍可能返回声明记录。本轮未实测无效声明退回分支，结论仅由源码支持。

[`create_router`](https://github.com/madeye/meow-rs/blob/3c27aca92d64c7194c6b590e529da46465fbb7eb/crates/meow-api/src/routes.rs#L188) 注册 GET/POST `/api/proxy-groups`、PUT/DELETE `/api/proxy-groups/{name}` 和 PUT `/api/proxy-groups/{name}/select`。这些写接口会涉及内核配置与运行时状态，接入应用必须另行解决 profiles/runtime 配置所有权，不能因为发现路由就绕过应用状态提交。

### Clash-rs 的组信息来源

[`group::routes`](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/app/api/handlers/group.rs#L42) 只挂载 `/{name}/delay`。stable、manifest Alpha 与当前 master 的该结论相同。

组全集应从 [`get_proxies`](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/app/outbound/manager.rs#L183) 的运行时注册表识别。[组序列化 trait](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/proxy/group/mod.rs#L28) 为组填充 `all`；该注册表不包含 provider 节点全集，需另读 provider 响应。

这里还有一个跨内核差异：当前构建逻辑在加载组之后重新插入 GLOBAL。本轮 0.10.8 对显式 GLOBAL 配置返回了重新合成的 GLOBAL，成员甚至包含 `GLOBAL` 自身。将捕获响应代入现有纯转换条件，会把 GLOBAL 再放进普通 `groups`；这是转换模型结果，不代表应用已越过前述 404 成功取得快照。不能把 Mihomo 的自定义 GLOBAL 行为推广到所有内核，也不能依赖任何一种 GLOBAL 合成策略发现组。

### 组测速路由存在但语义并不统一

现有 crate 的 `group_delay` 接收 DelayQuery，timeout 按毫秒编码，构造器允许 `1..=i32::MAX` 毫秒，返回 `IndexMap<ProxyName, u16>`。它被标记为非 retry-safe，不能因 HTTP 方法为 GET 就用于无副作用的能力探测。

| 固定源码中的行为 | Mihomo                                    | Meow                                           | Clash-rs                                                                                 |
| ---------------- | ----------------------------------------- | ---------------------------------------------- | ---------------------------------------------------------------------------------------- |
| timeout 解析     | `strconv.ParseInt(..., 32)`               | u16，拒绝 0                                    | u16                                                                                      |
| expected 参数    | 解析期望状态范围并用于 URLTest            | 读取并传递给 prober                            | DelayRequest 只有 url/timeout，不消费 expected                                           |
| 成功响应键       | 成功测得的成员名                          | 成员名                                         | 请求的组名，只有一个结果                                                                 |
| 结果与失败处理   | 组 URLTest 汇总成功成员；无成功结果时错误 | 汇总详细结果；超时返回 504，非超时失败项可为 0 | 测试组及成员，返回当前活跃成员的延迟，无法定位活跃成员时使用组结果；选中结果失败返回 400 |

依据：[Mihomo 组处理](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/route/groups.go#L53) 与 [GroupBase.URLTest](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/outboundgroup/groupbase.go#L237)、[Meow get_group_delay](https://github.com/madeye/meow-rs/blob/3c27aca92d64c7194c6b590e529da46465fbb7eb/crates/meow-api/src/routes.rs#L2313)、[Clash-rs 组响应](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/app/api/handlers/group.rs#L78) 与 [group_url_test](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/app/api/handlers/utils.rs#L22)。这些是源码结论，不是本轮测速结果。

因此，先前表格的“已注册”仅能说明路由存在，不能说明可按相同成员测速语义复用。组发现算法无需调用这条接口，其兼容性应与列表发现分开处理。

## 本轮二进制 GET 实测

三个进程分别使用独立目录与 `127.0.0.1:28761`、`:28762`、`:28763`，启动前确认端口空闲。关闭 DNS、TUN 和 provider health check，测试节点为 `127.0.0.1:1`，仅使用本地 provider 文件。配置显式声明 GLOBAL、Foo、Bar、group，其中 GLOBAL 只写 `[A,Foo]`，Bar 使用 provider 节点。

只请求了以下 GET 路径，没有测速、切换节点、更新 provider 或修改系统代理。Clash-rs 第一次因 health-check 缺少必填 `url` 拒绝配置；补充回环 URL 与 `interval: 0` 后通过配置检查并完成探测。所有本轮启动的测试进程均已停止。

| 精确路径                | Mihomo v1.19.30 | Meow 0.21.2   | Clash-rs 0.10.8 |
| ----------------------- | --------------- | ------------- | --------------- |
| `/version`              | 200             | 200           | 200             |
| `/proxy/group`          | 404             | 404           | 404             |
| `/group`                | 200，数组       | 200，map      | 404             |
| `/group/`               | 200             | 404           | 404             |
| `/group/Foo`            | 200             | 200           | 404             |
| `/group/Foo/`           | 200             | 404           | 404             |
| `/api/proxy-groups`     | 404             | 200，直接数组 | 404             |
| `/api/proxy-groups/`    | 404             | 404           | 404             |
| `/api/proxy-groups/Foo` | 404             | 405           | 404             |
| `/proxies`              | 200             | 200           | 200             |
| `/proxies/`             | 200             | 404           | 404             |
| `/proxies/Foo`          | 200             | 200           | 200             |
| `/proxies/Foo/`         | 200             | 404           | 404             |
| `/proxies/group`        | 200，单资源     | 200，单资源   | 200，单资源     |
| `/proxies/groups`       | 404             | 404           | 404             |
| `/providers/proxies`    | 200             | 200           | 200             |
| `/providers/proxies/`   | 200             | 404           | 404             |

最终保存 **51 条响应**。三个内核的 `/proxies` 都存在四个组 `GLOBAL/Foo/Bar/group`，都未包含 `provider-node` 的独立记录，而 `/providers/proxies` 包含该节点。Mihomo 与 Meow 的 GLOBAL 成员仍为 `[A,Foo]`；Meow 管理扩展的 Bar 成员包含 `provider-node`，与当前源码的运行时补全逻辑一致。

这些结果补充了输入报告；它们不替代最新二进制的发布验证，也不构成整个 API 功能矩阵、命名管道或 UI 端到端测试。

## 历史功能边界复核

| 功能                     | 可追溯引入证据                                                                                                                                           | 本轮标签检查                                                                      |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Meow 管理扩展            | [`d1121d5`](https://github.com/madeye/meow-rs/commit/d1121d5790f7f4127aaabd3a6851dc84a87c838f)，2026-03-17，当时路径为 `crates/mihomo-api/src/routes.rs` | 按版本排序的现有版本标签中，最早包含该提交的是 `v0.2.0`；不推断未保存的外部发行包 |
| Meow 扩展补运行时成员    | [`8907837`](https://github.com/madeye/meow-rs/commit/890783728c07c0537e9fbe271f95e6cef25ceba6)，2026-06-19                                               | 当前 main 和 v0.22.0 保留该处理方式；本轮 0.21.2 实测 provider 成员               |
| Meow `/group` 列举和详情 | [`fbe01cc`](https://github.com/madeye/meow-rs/commit/fbe01cc47e2bdd6e2ed3a3d16b802bc72ef0fd06)，2026-07-16                                               | `v0.17.0` 只有 delay；`v0.18.0` 已注册 root 与详情 GET                            |
| Mihomo `/group`          | [`4092a7c`](https://github.com/MetaCubeX/mihomo/commit/4092a7c84b623d2e567112d494bdd22c048b882e)，2022-05-30                                             | `v1.11.1` server 未挂载；`v1.11.2` 已挂载                                         |

未在本轮核查的路由源码中找到专用 `/proxy/group` 或 `/proxies/group` 注册。该结论限于这里列出的上游与提交，不覆盖所有 fork 和未来版本。

## 当前应用集成中的新增发现

生产读取链路为 `ProxiesActor::refresh` → `ApiClient::proxy_snapshot` → `clash_api::Client::proxies` 与 `proxy_providers` → `Proxies::from_responses`。可定位到 [actor](../../backend/tauri/src/core/proxies.rs)、[API 适配器](../../backend/tauri/src/core/actor_v2/api.rs) 和 [clash-api 端点实现](../../backend/nyanpasu-runtime/crates/clash-api/src/api/proxies.rs)。

当前 `proxies()` 请求 `/proxies/`，`proxy_providers()` 请求 `/providers/proxies/`。[`Client::endpoint`](../../backend/nyanpasu-runtime/crates/clash-api/src/client.rs) 只移除路径开头的斜杠，保留末尾斜杠；动态详情和选择操作还通过空路径段主动追加末尾斜杠。本轮原始 HTTP 请求证实，现有 Meow 与 Clash-rs 二进制拒绝这种路径。

因此，在直接连接这些 controller 的现有生产路径上，快照读取存在请求失败风险，发生在组发现之前。这是源码调用链与 HTTP 实测结合得到的结论；本轮没有启动 Nyanpasu UI，不声称已实测整个页面的报错表现。建议优先为这组端点采用已验证的无尾斜杠形式，并覆盖详情、选择与 provider 操作的路径构造测试，避免只修 `groups()`。

`groups()` 请求 `/group/` 并解码 `Vec<Proxy>`，`group(name)` 同样追加尾斜杠；**这是与已核查 Mihomo 相符的客户端定义，本身不是 Mihomo 接口错误。** 直接跨核使用才存在上述差异。本轮搜索未发现应用代理发现生产路径调用 `groups()`；调用位于 clash-api 的 Mihomo 测试中，不应将其跨核限制写成当前代理页因调用 groups 而漏组。

issue 中提到的 DTO 风险已有变化：当前 `Proxy` 的 `extra/alive/uot/xudp/tfo/mptcp/smux/interface/routing-mark/provider-name/dialer-proxy` 已是可选字段；`history` 和 `udp` 仍必填。本轮三份实际代理响应提供它们，不能继续声称这些字段全都是未修复的必填项，也不能据此宣布所有未知内核响应兼容。

## 建议采用的读取策略

按用户要求采用 **group 列表优先，proxies 推断兜底**。Mihomo 的 array 与 Meow 的 map 在现有 clash-api 内适配为 `groups()` 的 `Vec<Proxy>`；成功列表决定组全集，不与 `/proxies` 的额外候选取并集。Clash-rs 没有列表路由，旧版本或能力不明时也从 `/proxies` 中 all 为 Some 的记录推断。空成员组仍保留，GLOBAL 单独处理。

复用 nyanpasu-core-metadata 的 Feature/EnumSet 和版本策略控制组列表能力。能力与格式绑定实际运行实例；接口 404、临时故障和协议异常不能混为永久不支持。仍读取 proxies 和 providers 补节点，支持列表时会增加第三个 GET；无尾斜杠适配是三核读取的前提。

Meow 管理扩展不能替代运行时组集合，测速和写操作也不用于能力探测。实施契约与失败规则见 [代理组发现 spec](../spec/2026-10-05-proxy-group-discovery/design.md)，完整动机和 Mihomo 内部分析见 [算法重构报告](2026-10-05-issue-5112-proxy-algorithm-report.md)。

本轮完成源码与版本核查、隔离 HTTP 探测、调用链核对及证据保存。未修改应用算法、未编译最新内核、未运行上游完整测试套件。已有 10 项模型检查针对 proxies 推断与排序，不能作为新 group 优先策略已经实现或通过测试的证明。
