# Issue 5112 代理组发现与快照算法重构建议

> 本报告保留调查基线的接口与问题描述；文中的“当前”指调查时的代码。后续实现、能力边界及验收结果见 [代理组发现 spec](../spec/2026-10-05-proxy-group-discovery/design.md)。

建议保留现有 ProxiesActor 作为快照所有者，把组发现、排序、成员解析和确定化处理集中到纯转换逻辑：**group 列表可用时由内核返回的组集合决定有哪些组，不可用时才从 proxies 推断。GLOBAL 只提供排序提示，provider 响应补成员记录。** 复用现有 Feature/EnumSet 控制能力；支持时增加一次 `/group` 读取，不新增 actor。

本文汇总 [issue 5112](https://github.com/libnyanpasu/clash-nyanpasu/issues/5112) 的背景、三内核接口核查、Mihomo 内部实现和重构建议，作为后续实施的主报告；建议尚未实现。[原接口核查报告](2026-10-05-proxy-core-api-report.md) 保留完整路径状态码与历史调查记录，原始响应、测试配置、二进制 SHA-256 和模型检查保存在 [证据文件](2026-10-05-proxy-core-api-evidence.json)。

按用户最终要求，本文已将原先“全部从 proxies 发现”的建议修订为 group 优先。具体能力传递、错误降级与验收标准见 [代理组发现 spec](../spec/2026-10-05-proxy-group-discovery/design.md)。provider 冲突治理、可选 GLOBAL 等超出该 spec 首次实施范围的建议，保留为独立后续工作。

## 以项目 clash-api 定义为接口基准

本次涉及的内核客户端 API 定义以当前子模块中的 **`clash-api` 1.0.0-rc.9** 为准，具体见 [src/api/proxies.rs](../../backend/nyanpasu-runtime/crates/clash-api/src/api/proxies.rs) 与 [crate 公共导出](../../backend/nyanpasu-runtime/crates/clash-api/src/lib.rs)。它与 Nyanpasu 面向前端的 Unified RPC 是不同边界。后文的内核源码用于解释服务端实现；跨内核实测用于发现已有客户端的兼容性差异，不另外猜测或设计同义 API，也不把客户端已定义方法视为所有内核都实现了同一契约。

下表摘录现有 async 方法的参数、返回类型和实际路径；省略共同的 `&self`。`Result` 是 `clash_api::Result`，`IndexMap` 也由 crate 导出，动态名称均通过现有 path-segment 构造器编码。

| 现有 Client 方法                                    | 返回类型                                        | 当前请求                                                    |
| --------------------------------------------------- | ----------------------------------------------- | ----------------------------------------------------------- |
| `groups()`                                          | `Result<Vec<Proxy>>`                            | `GET /group/`，内部解码 `ProxyList { proxies: Vec<Proxy> }` |
| `group(name: &ProxyName)`                           | `Result<Proxy>`                                 | `GET /group/{name}/`                                        |
| `group_delay(name: &ProxyName, query: &DelayQuery)` | `Result<IndexMap<ProxyName, u16>>`              | `GET /group/{name}/delay`                                   |
| `proxies()`                                         | `Result<IndexMap<ProxyName, Proxy>>`            | `GET /proxies/`，内部解码 ProxyMap                          |
| `proxy(name: &ProxyName)`                           | `Result<Proxy>`                                 | `GET /proxies/{name}/`                                      |
| `proxy_providers()`                                 | `Result<IndexMap<ProviderName, ProxyProvider>>` | `GET /providers/proxies/`，内部解码 ProviderMap             |
| `proxy_provider(provider: &ProviderName)`           | `Result<ProxyProvider>`                         | `GET /providers/proxies/{provider}/`                        |
| `select_proxy(selection: ProxySelection<'_>)`       | `Result<()>`                                    | `PUT /proxies/{group}/`，JSON 为 `{"name": target}`         |
| `clear_proxy_selection(group: &ProxyName)`          | `Result<()>`                                    | `DELETE /proxies/{group}/`                                  |
| `update_proxy_provider(provider: &ProviderName)`    | `Result<()>`                                    | `PUT /providers/proxies/{provider}/`                        |

crate 已用 **同一个 `Proxy` 表达节点和组**，没有公开的独立 Group 响应类型。`ProxyList`、`ProxyMap`、`ProviderMap` 只是私有 envelope。应用的 `ProxyGroupItem` 是展示投影，不能作为内核协议 DTO 重新定义一套 HTTP 接口。

与算法直接有关的已有类型为：

- 标识使用 `ProxyName` 与 `ProviderName`，都是透明字符串 newtype。复用其公共构造和 `as_str()`，不新增重复标识类型。
- `Proxy.all` 是 `Option<Vec<ProxyName>>`，`Proxy.now`、`fixed`、`empty_fallback` 是 `Option<ProxyName>`；`hidden` 是 `Option<bool>`，`proxy_type` 是从 JSON `type` 映射的 String。空列表与缺失字段不同，now 可缺失已经由类型表达。
- `Proxy.name/proxy_type/history/udp` 必填，core 扩展与组元数据按现有 Option/serde 属性解码。可选不等于接受错误类型，不能把解码失败改成空对象。
- `ProxyProvider.proxies` 已是 `Vec<Proxy>`；`ProviderType` 与 `VehicleType` 已保留 Unknown 字符串，VehicleType 也已有 Inline。provider 补全直接消费这些已有类型。
- `DelayQuery` 已定义 `url: Url`、`timeout: Duration`、`expected: Option<ExpectedStatus>`，构造器验证参数，传输时将 timeout 转为毫秒；`group_delay` 返回名称到 u16 延迟的 map，不另拟返回模型。`ProxySelection` 已持有 group/target 的 `&ProxyName`。

`RequestMetadata` 已把 groups/group/proxies 等读取标为 retry-safe，把 group_delay 和选择等操作标为非 retry-safe；默认 RetryPolicy 是 NoRetry。这是现有操作语义，不能仅根据 GET 方法自行推断可重试。HTTP 错误和 JSON 解码错误分别由已有 `Error::HttpStatus`、`Error::Decode` 表达。[请求元数据](../../backend/nyanpasu-runtime/crates/clash-api/src/retry.rs)、[错误类型](../../backend/nyanpasu-runtime/crates/clash-api/src/error.rs)

现有测试也提供明确依据：[tests/mihomo.rs](../../backend/nyanpasu-runtime/crates/clash-api/tests/mihomo.rs) 的 `assert_proxy_and_rule_apis` 已调用 groups、group、proxies、选择、测速及 provider 方法；[tests/rest.rs](../../backend/nyanpasu-runtime/crates/clash-api/tests/rest.rs) 已覆盖最小节点字段、部分订阅信息和错误类型拒绝，但 proxy 列表 mock 注册的是带尾斜杠路径；[client.rs](../../backend/nyanpasu-runtime/crates/clash-api/src/client.rs) 的 `dynamic_path_segments_are_percent_encoded` 还显式断言了动态详情的末尾斜杠。因此尾斜杠是当前实现及测试固定的行为，跨内核路径修正必须同步更新这些契约测试，而非绕开 crate 重写请求。

## 核查范围与接口结论汇总

核查日期为 2026-10-05。三个上游仓库已按语言克隆或更新；这里固定源码提交，不将移动分支或下载标签当成不可变版本。

| 内核     | 本地目录与源码基准                                                                                                                                 | 本轮实际运行的现有 sidecar |
| -------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- |
| Mihomo   | `G:\Programs\Go\mihomo`，Meta / v1.19.32 `88dcbf7f1614a67c3b36b848ee3592dfa92ada36`；另核对 Alpha `9f053c49075de076d83d3e5918241e410f7e9adf`       | **v1.19.30**               |
| Meow     | `G:\Programs\Rust\meow-rs`，main `3c27aca92d64c7194c6b590e529da46465fbb7eb`；v0.22.0 `4b26a0b5d0cd893e11af71b07e3c6c145c66ce88`                    | **0.21.2**                 |
| Clash-rs | `G:\Programs\Rust\clash-rs`，master / v0.10.9 / 当次 latest `0cf7ed5f6a99a6ea6d62afd9188963657f796d4c`；另核对 v0.10.8 与 manifest Alpha `39d06a4` | **0.10.8**                 |

Mihomo 默认 main 不含内核实现，实际核查使用 Meta。Meow 从干净的 `7b20bef` 快进更新，其余两个仓库为新克隆。最新源码没有在本轮编译，源码结论与现有二进制实测分别标记。当前应用 manifest 的 Clash-rs stable/Alpha 仍是 v0.10.8 / `39d06a4`，不能与移动 latest `0cf7ed5` 混为一谈。下表记录服务端 wire 行为；项目 Rust API 仍以上一节的 clash-api 定义为准。

| 接口                      | Mihomo                      | Meow                                             | Clash-rs       | 对本次算法的意义                     |
| ------------------------- | --------------------------- | ------------------------------------------------ | -------------- | ------------------------------------ |
| `GET /proxies`            | 运行时名称索引              | 运行时名称索引                                   | 运行时名称索引 | 三核共同的组发现底座                 |
| `GET /providers/proxies`  | provider 名称索引与节点记录 | 同左                                             | 同左           | 补齐注册表缺失的成员元数据           |
| `GET /group`              | `{"proxies": [Proxy, ...]}` | `{"proxies": {name: ProxyInfo}}`，v0.18.0 起提供 | 没有列表路由   | URL 相似不等于响应契约相同           |
| `GET /group/{name}`       | 组详情                      | 组详情                                           | 没有详情路由   | 非三核共同接口                       |
| `GET /group/{name}/delay` | 已注册                      | 已注册                                           | 已注册         | 测速能力不能证明列举能力             |
| `GET /api/proxy-groups`   | 未注册                      | 配置声明顺序的管理数组，并补运行时成员           | 未注册         | 不能替代运行时组全集或完整 Proxy DTO |
| `GET /proxy/group`        | 未找到注册                  | 未找到注册                                       | 未找到注册     | 不使用猜测出的统一组接口             |

这里的“已注册”只表示服务端存在该路由，不是通过 clash-api 的兼容性结论。当前 `groups()` 的 `/group/` + Vec 契约与 Mihomo 相符；Meow 首先会在该路径返回 404，改为 `/group` 后仍有 map/Vec 的解码差异；Clash-rs 则没有组列表。不能将这三种情况统称为“group API 不支持”，也不能把 Mihomo 的正确客户端定义说成需要修复的服务端接口。

组测速也不能只按 URL 归为同一能力。现有 `group_delay` 返回 `IndexMap<ProxyName, u16>`；Mihomo 汇总成功成员的延迟，Meow 按成员名返回详细探测结果，而 Clash-rs 的处理函数只返回**请求的组名对应的单个延迟**，取测试前活跃成员的结果，无法定位时退回组结果。相同 JSON map 外形不代表相同键语义。[Clash-rs 组处理](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/app/api/handlers/group.rs#L78)、[测速辅助函数](https://github.com/ibigbug/clash-rs/blob/0cf7ed5f6a99a6ea6d62afd9188963657f796d4c/clash-lib/src/app/api/handlers/utils.rs#L22)

参数也有差异：crate 的 DelayQuery 按 Mihomo 组接口允许 `1..=i32::MAX` 毫秒，Meow 与 Clash-rs 处理函数解析为 u16；Clash-rs 的 DelayRequest 只有 url/timeout，不消费 expected。Meow 与 Mihomo 的失败结果汇总也并非完全一致。详细源码对照见 [接口调查中的组测速一节](2026-10-05-proxy-core-api-report.md)。本轮没有执行测速，不把这部分计作已通过的二进制功能测试；本 issue 的发现算法不依赖它。

Mihomo `/group` 早在 [2022-05-30 的提交](https://github.com/MetaCubeX/mihomo/commit/4092a7c84b623d2e567112d494bdd22c048b882e) 引入；标签检查界限为 v1.11.1 未挂载、v1.11.2 已挂载。Meow 的兼容 `/group` 由 [2026-07-16 的提交](https://github.com/madeye/meow-rs/commit/fbe01cc47e2bdd6e2ed3a3d16b802bc72ef0fd06) 引入；v0.17.0 只有组测速，v0.18.0 才有列表与详情。其管理扩展更早由 [2026-03-17 的提交](https://github.com/madeye/meow-rs/commit/d1121d5790f7f4127aaabd3a6851dc84a87c838f) 引入，当前可追溯版本标签始于 v0.2.0；不是同一套接口。

Meow 管理扩展按 `raw_config.proxy_groups` 列举，优先取运行时 `members()`，缺失时退回声明成员。它输出配置类型 `select`、成员字段 `proxies` 和有限管理字段，不是运行时 `Selector` / `all` 契约；也不自动包含未声明的合成 GLOBAL。该运行时补全可追溯到 [2026-06-19 的提交](https://github.com/madeye/meow-rs/commit/890783728c07c0537e9fbe271f95e6cef25ceba6)。扩展的创建、修改和删除会涉及 core 配置所有权，不能绕过应用 profiles/runtime 提交流程直接接入。

本轮三个二进制的无尾斜杠 `/proxies` 与 `/providers/proxies` 均为 200；带尾斜杠时仅 Mihomo 仍为 200，Meow 与 Clash-rs 均为 404。这些是**原始 HTTP GET 的路径与响应证据**，未通过现有 crate 执行三核 typed-client 全套测试，不能据此声称当前 `proxies()` / `proxy_providers()` 已兼容三核。`/group/` 在 Meow 为 404，管理扩展详情 GET `/api/proxy-groups/Foo` 为 405。`/proxies/group` 实际是名称为 group 的单资源，不能把参数匹配误认为组列表。

以上结论限于表中版本。输入 `C:\Users\a6320\Downloads\REPORT.md` 的其他二进制结果属于此前证据；Premium 未做本轮源码核查，不据此扩大兼容性承诺。

## 问题背景与当前状态

issue 描述的是自定义 GLOBAL 只引用部分组时，代理页与托盘遗漏其他运行时组。其原始代码基准为 `46d1aaf8`，不能直接套用全部旧行号和旧字段。当前工作区基准为 `3b942c760c74df0a7fa832dc455c3a15b676cb3b`，运行时子模块为 `889901cb57b1b1753354c2b7b0a442569601e417`。

| 问题或旧实现                              | 当前核查结果                                             | 重构处理                                          |
| ----------------------------------------- | -------------------------------------------------------- | ------------------------------------------------- |
| 根据 `GLOBAL.all` 发现普通组              | 仍存在于 `Proxies::from_responses` 的主分支              | 改为扫描完整运行时注册表                          |
| GLOBAL 缺失时筛选 `name == "GLOBAL"`      | 仍存在；缺失时得到空组列表                               | 合并为一个与 GLOBAL 是否存在无关的发现算法        |
| 旧 `Proxies.proxies` 反向筛选和内置项重复 | 该字段已移除，改为 `nodes` 名称索引                      | 保持节点去重设计，不恢复旧向量                    |
| 旧托盘 `proxies.is_empty()` 兜底          | 已移除，当前仅 Global 模式额外展示 global 项             | 明确处理缺失 GLOBAL，不按过时条件重写             |
| Adler32 变化检测                          | 已改为序列化 `(&proxies, &providers)` 得到字节向量并比较 | 仍需消除无语义 map 顺序变化；无需“升级 hash 算法” |
| 旧全局 ProxiesGuard                       | 现为 ProxiesActor 与 typed client                        | 沿用现有 actor 所有权                             |
| clash-api 所有 Mihomo 扩展字段必填        | 多数扩展字段已可选；history、udp 仍必填                  | 用实际协议样本验证，避免重复修复已完成项          |

定位：[纯转换入口](../../backend/tauri/src/core/clash/proxies.rs)、[ProxiesActor 与变化检测](../../backend/tauri/src/core/proxies.rs)、[托盘投影与 diff](../../backend/tauri/src/core/tray/proxies.rs)、[当前协议 DTO](../../backend/nyanpasu-runtime/crates/clash-api/src/api/proxies.rs)。

## 证据支持的根因

本轮配置创建 `GLOBAL=[A,Foo]`，同时存在 Foo、Bar 和名称为 group 的普通组。Mihomo v1.19.30 与 Meow 0.21.2 均在 `/proxies` 返回四个组，但按当前 Rust 代码的发现条件投影，只能得到 Foo。Bar 和 group 没有被内核删除，丢失发生在应用组列表的构造条件。

Clash-rs 0.10.8 则重新合成 GLOBAL，使其成员包含所有普通组以及 GLOBAL 本身；将捕获响应代入当前纯转换条件，会产生 `GLOBAL,Foo,Bar,group`。这是算法模型结果，不代表现有客户端已越过尾斜杠 404 成功读取快照。它说明同一转换条件既可能漏组，也可能把 GLOBAL 重复作为普通组。

三个二进制都将 `provider-node` 放在 provider 响应，未放在 `/proxies` 注册表。当前代码只收集已发现组和 GLOBAL 的成员，因此漏掉 Bar 还会阻止其独占 provider 节点进入 `nodes`。单纯在 UI 上补一个组名不能完整修复数据。

以上“当前算法输出”由源码条件和捕获数据的模型投影确认，不是一次 Nyanpasu UI 运行。证据文件中明确标记了模型检查与实际 HTTP 探测的区别。

## Mihomo 内部如何提供代理组 API

以下调用链按 Meta / v1.19.32 `88dcbf7…` 追踪。对已拉取的 Alpha `9f053c4…` 比较后，组路由、ProxyGroup 接口、代理包装序列化、Selector 序列化与 GroupBase 成员处理文件均无差异。

### 配置构建与运行时注册

[`config.parseProxies`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/config/config.go#L879) 先建立普通代理和内置项的 `map[string]C.Proxy`，记录配置声明的组顺序，再对组依赖执行 DAG 排序并构建各组。每个组由 `outboundgroup.ParseProxyGroup` 创建，通过 `adapter.NewProxy(group)` 包装后放入同一张 proxies map。

该版本支持构建 Selector、URLTest、Fallback、LoadBalance，relay 配置已明确拒绝。[`ParseProxyGroup`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/outboundgroup/parser.go#L48) 把显式 `proxies` 引用包装为 Compatible provider，并连接 `use` 指定的 provider；组对象因此拥有运行时成员来源，而非只保留原始 YAML 名称数组。

`parseProxies` 仅在未声明 GLOBAL 时合成一个 Selector。显式 GLOBAL 与其他组一样构建，其成员没有“自动扩充为所有组”的保证。配置的顺序辅助列表用于构建兼容 provider 等逻辑，但不会成为 `/group` 的返回排序依据。

[`executor.ApplyConfig`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/executor/executor.go#L84) 经 `updateProxies` 把配置中的 proxies/providers 发布给 [`tunnel.UpdateProxies`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/tunnel/tunnel.go#L230)。API 读取的是这张运行时注册表，不在每次请求时重新解析配置或沿 GLOBAL 遍历。

### 路由与组识别

[`server.router`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/route/server.go#L119) 将 `groupRouter()` 挂到 `/group`，位于同一 controller 的鉴权路由组；配置 secret 时使用该鉴权中间件。[`groupRouter`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/route/groups.go#L19) 再注册列表、详情和测速处理函数：

```text
GET /group
  → getGroups
  → tunnel.Proxies() 的运行时 map
  → 对每个 p.Adapter() 做 outboundgroup.ProxyGroup 接口断言
  → 收集匹配的 C.Proxy，返回 {"proxies": [...]}

GET /group/{name}
  → parseProxyName → findProxyByName
  → 从同一运行时 map 按名称取对象
  → getGroup 再检查 ProxyGroup 接口，成功返回单对象，否则 404
```

[`ProxyGroup`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/outboundgroup/util.go#L11) 是 Go 接口，扩展 `C.ProxyAdapter` 并要求 Providers、Proxies、Now、Touch、Hidden、Icon、URLTest 等方法；文件用编译期断言确认四种组实现它。`adapter.Proxy` 是外层包装，`Adapter()` 返回内部的具体 adapter，因此列表筛选检查的是组能力，不是名称、`type` 字符串白名单、`now` 是否存在，也不是 JSON `all` 字段。

`getGroups` 不排除 GLOBAL 或 hidden 组，不要求组被规则或其他组引用，也不先检查成员数。它只是同一注册表的按组类型投影。普通节点按名称访问 `/group/{name}` 也会被二次组检查拒绝。

### 响应序列化与成员来源

处理函数收集的是 `C.Proxy` 对象，未另外构造一套 Group DTO。JSON 编码调用 [`adapter.Proxy.MarshalJSON`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/adapter.go#L136)：先让具体 adapter 序列化组字段，再补入 name、history、extra、alive、udp 和其他运行时属性。`/proxies` 与 `/group` 使用相同对象及序列化逻辑，外层分别是名称 map 与组数组。

以 [`Selector.MarshalJSON`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/outboundgroup/selector.go#L56) 为例，`all` 来自 `GetProxies(false)` 的成员名称，`now` 来自 `Now()`；另带 type、testUrl、hidden、icon、emptyFallback。URLTest/Fallback 同样输出 all 和 now，并有 fixed、expectedStatus 等字段。**LoadBalance 也输出 all，但其 JSON 不包含 now**，因此前端不能因 now 缺失而丢弃它。[LoadBalance 序列化](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/outboundgroup/loadbalance.go#L260)

[`GroupBase.GetProxies`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/adapter/outboundgroup/groupbase.go#L122) 按 provider 版本缓存成员；版本变化时展开 provider 节点，处理 filter、exclude-filter 与 exclude-type，以及过滤条件产生的顺序。没有可用成员时返回 emptyFallback。因而 API 的 all 是已展开、已过滤、可能含兜底节点的运行时列表，不是原始 `proxies` 声明，也不能由客户端重新拼 provider 全集代替。

客户端应保留这个成员顺序，只补每个名字的节点元数据。不能把 provider 内不属于该组或已被过滤掉的节点重新加回组。`GetProxies(false)` 本身不对 provider 调用 Touch；列表处理函数也不发起 `group.URLTest`，但仍可能读取或更新内部成员缓存，不能把它理解成冻结的深拷贝快照。

### 顺序与测速接口的边界

`getGroups` 直接遍历 Go map，没有排序，所以组数组顺序不稳定。组内 all 的顺序则来自运行时成员解析，具有意义。这两层顺序必须分别对待：规范化外层组顺序，保留内层成员顺序。正常配置会有显式或合成 GLOBAL；若底层注册表完全没有组，当前处理函数的 nil slice 编码为 `proxies: null`。**现有 clash-api 的 ProxyList 定义为 Vec，无 null 转空数组逻辑，此响应会返回 Error::Decode。** 这只是源码边界说明，不在本 issue 中擅自改变现有解码契约，也不混同于正常配置只有 GLOBAL。

[`getGroupDelay`](https://github.com/MetaCubeX/mihomo/blob/88dcbf7f1614a67c3b36b848ee3592dfa92ada36/hub/route/groups.go#L53) 是另一种操作：对于实现 SelectAble 的非 Selector 组，先清除强制选择与持久化选择，再校验 timeout、expected 并调用组 URLTest 返回延迟 map。即使后续参数校验失败，也可能已经改变选择状态。因此虽然 HTTP 方法是 GET，它不能用作无副作用的组发现或能力探测。本轮未调用该测速接口。

## 采用 group 优先的组发现策略

Mihomo `/group` 的权威性来自内部 ProxyGroup 接口判断，Meow 来自运行时 members 标记。按用户要求，在列表能力可用时优先使用这份返回；Clash-rs、旧版本或能力不明时从 `/proxies` 推断。

成功的 group 列表决定组集合，不能再与 proxies 推断结果取并集。列表为空也是有效结果；不能因为空而切换到推断。仍需 `/proxies` 提供普通节点记录及 `/providers/proxies` 补充 provider 成员，因此能力启用时为三个 GET，未启用时为两个 GET。这里提高的是组分类可靠性，不是跨请求事务一致性。

复用 [nyanpasu-core-metadata 的 FeatureSupport](../../backend/nyanpasu-runtime/crates/nyanpasu-core-metadata/src/feature/clash.rs)：已有版本解析和 `EnumSet<Feature>`，无需新建通用 FeatureSet 框架。增加组列表能力，与 delay、详情、写操作和 RuntimeFeature::LocalIpc 分开。能力及格式随 applied instance 的 API binding 传递，不能从全局“选中的内核”或独立 status 快照拼出。

Mihomo >= v1.11.2 使用数组；Meow >= v0.18.0 使用 map，均先由 clash-api 规范化为现有 `Vec<Proxy>`。未知版本遵循 metadata 的保守策略，不自动启用；nightly 遵循现有版本策略并仍接受实际响应校验。运行时列表失败的分类与回退见 spec：不支持、暂时失败、协议异常必须可区分，不能把鉴权或实例失效伪装成成功。

增加稳定夹具契约验证：暂停 provider 更新时，比较 Mihomo `/group` 与同版本 `/proxies` 带 all 的名称集合，覆盖四种组、hidden、自定义 GLOBAL 子集；不比较数组位置。生产环境两个非原子请求有差异时以 group 为准，不因此合并额外组。

## 先解决生产路径的协议前提

当前 `ApiClient::proxy_snapshot()` 并行调用 `proxies()` 与 `proxy_providers()`，后两者使用 `/proxies/` 和 `/providers/proxies/`。本轮 Meow、Clash-rs 实测两条带尾斜杠路径都为 404，无尾斜杠路径为 200。URL 构造器不会删除末尾斜杠，因此跨内核兼容性问题发生在纯算法之前。

建议首先在 `clash-api` 修正这些端点的跨核路径，同时核对代理详情、选择、provider 详情与更新所用的空末尾路径段。保留动态名称的分段百分号编码；不要用字符串拼接名称或全局重写所有 URL。写请求的正确路径由路由与精确 mock 测试验证，不用对真实配置发写请求来验证。路径修正也不自动增加服务端缺少的 HTTP 方法：例如当前 Clash-rs 的代理单资源路由注册 GET/PUT，没有注册现有 crate `clear_proxy_selection()` 使用的 DELETE；这类能力差异不能靠去掉斜杠解决，也不应在本 issue 中承诺全 API 兼容。

普通组发现保留 `proxies()` 与 `proxy_providers()` 补节点，并在列表能力启用时调用 `groups()`。`groups()` 已有明确的 Mihomo 数组契约；这个方法在 Mihomo 上有效，但直接复用于 Meow 时既有尾斜杠又有 map/array 差异，Clash-rs 则没有组列表路由。只改其 URL 无法完成三核适配，不能将当前契约称为已经支持所有内核的统一组 API。

## 数据模型与算法不变量

设 `R` 对应现有 `Client::proxies()` 返回的名称索引，`P` 对应 `proxy_providers()` 返回的 provider 索引，`L` 是成功解码并验证的 `groups()` 列表。复用现有 Proxy/ProxyProvider/名称类型与纯转换入口。定义本次组记录索引 S：

```text
S = L 按 name 建索引                         # 权威分支，空列表也是成功
  或 { key: R[key] | R[key].all is Some }    # 列表不可用时的推断分支
G = keys(S) 去掉精确名称 "GLOBAL"
```

这里比较精确名称，不进行大小写折叠；名称为 `global` 的用户组与 `GLOBAL` 不等价。DTO 层的 `Some([])` 仍按空成员组处理，不能用成员非空、`now` 存在或一份组类型白名单替代结构识别。这不表示当前 Mihomo 会把空 provider 原样序列化为空 all：其 GroupBase 会补 emptyFallback。这个判断依据本次核对的三核序列化契约；未知协议应在适配层验证，不是宣称适用于所有代理内核。

成功的 L 缺失某组时，不用 R 补回；L 与 R 同名组记录冲突时采用 L 的完整记录，并诊断跨请求不一致。L 中缺失 all、重复 name 或 map key/name 不一致属于列表协议错误，不静默过滤。GLOBAL 也从 S 提取，不能从另一来源偷偷补回。

建议确立以下不变量：

1. 每个 G 中的运行时组恰好出现一次，GLOBAL 不出现在普通 groups。
2. 普通组是否存在，不取决于 GLOBAL、规则可达性、provider 来源或 hidden 展示标志。
3. 保留每个组的 `all` 原顺序及 `now`，不递归展开嵌套组，不用遍历图计算全集。
4. `nodes` 保存全部 R 记录，以 S 的组记录覆盖同名项，并补齐组与 GLOBAL 引用的缺失成员；共享节点只保存一次。nodes 中存在记录不意味着它必须进入 groups。
5. JSON 对象键换序不引起展示或指纹变化；成员顺序、选择、节点元数据等有语义的变化仍能被观察。
6. 缺失资源和冲突必须可诊断，不能静默删除组或把任意同名 provider 节点当成已正确解析。

输入 map 的 key 作为引用标识。正常协议要求 key 与记录中的 name 一致；不一致时建议返回结构化协议错误或诊断，禁止一边按 key 关联、一边按另一个 name 展示和提交选择。

## 确定化排序方案

推荐第一步采用保留现有顺序的最小方案：从 G 建立待输出集合；按 GLOBAL.all 顺序取出其中的组，首次出现时输出并从集合移除；最后将未输出组按 Rust `String::cmp` 的确定性字典序排序追加。

```text
remaining = 所有普通组名称的集合
ordered = []

for name in GLOBAL.all（若不存在则为空）:
    if remaining.remove(name):
        ordered.push(name)

ordered.extend(sort_by_exact_name(remaining))
groups = ordered.map(name => convert_group(S[name]))
```

这不是用 GLOBAL 定义成员集：更换 GLOBAL 的成员不会让其他组消失，仅可能调整普通组的展示顺序。GLOBAL 中重复组名只输出一次；普通节点、缺失名字和 GLOBAL 自引用不会进入 groups。缺失名字作为成员解析问题另外报告。

此前仅从 R 推断的模型检查中，三核样本均得到 `Foo,Bar,group`，Clash-rs 去掉了多余 GLOBAL；移除 GLOBAL 的合成样本得到 `Bar,Foo,group`。这些结果验证兜底与排序模型，尚未执行新增 group 优先分支。

不推荐按 `/proxies` 接收顺序追加遗漏组：Meow 和 Clash-rs 直接序列化 HashMap，跨请求或跨进程没有顺序保证。也不推荐全量按 lowercase 排序，大小写碰撞时还需要额外规则，且会改变原有排列。

未来若产品要求严格遵循最终生成配置的组声明顺序，再由该配置的 serial owner 下发带版本的顺序数据，优先匹配运行时存在的组，然后追加未列出的运行时组。不要为本次修复读取 profiles 全局变量、扫描原始订阅，或让 actor 同步读取 sibling snapshot。原始 profile 的顺序也未必等于经过 merge/script 后的有效配置顺序。

## 成员解析与 provider 补全

继续沿用 `nodes + groups[].all` 的共享节点模型。先建立 provider 候选索引，再对所有已发现组和 GLOBAL 收集成员；先以 S 覆盖 R 的同名组记录形成运行时节点索引；每个成员优先使用该索引，其次使用可唯一解析的 provider 记录，最后保留 Unknown 占位和诊断。

当前 `from_responses` 只纳入 HTTP/File provider，而协议枚举已经支持 Inline 与 Unknown vehicle。vehicle 类型描述加载方式，不应天然决定一份有效节点记录是否可以解析。建议消费所有可用的 proxy-provider 节点记录，包括 Inline；Compatible 中已有的普通节点由 R 优先去重。规则 provider 或不能解析的协议记录则独立处理。

当前 `provider_proxy_map` 将同名节点后写覆盖，结果受 provider map 顺序影响。建议只合并语义相同的候选；有差异时优先利用运行时明确提供的 provider 标识或适配器已确认的归属。仅凭组成员字符串无法确定来源时，保留占位并记录歧义，禁止通过 provider 名字排序后任意选一个来伪装正确性。

这一步不要求立即把所有节点 ID 升级为 `(provider,name)`：组成员协议本身只给名字时，新增复合 ID 也无法凭空补齐归属。只有产品确实要支持 provider 内同名节点的独立操作时，才另行设计完整 ID、RPC 与 UI 迁移。

诊断建议至少区分 unresolved member、ambiguous provider member、key/name mismatch。纯服务返回诊断值，由 ProxiesActor 按实例和快照变化去重输出摘要；不要在纯函数中读取全局 logger，也不要每个轮询周期重复打印同一批节点。

两个或三个 GET 请求不是一个原子快照。现有实例撤销检查能防止旧进程数据发布，但不能证明同一进程配置重载期间的各份响应同代。暂时无法解析的成员不能导致整个组被丢弃；继续遵循现有失效与刷新机制。若要加强同代保证，应由运行时提交版本提供证据，不能把实例 ID 当成配置版本，也不应增加无限重试或第二套 actor 调度队列。

## GLOBAL 缺失的表达

当前代码把缺失 GLOBAL 替换为 `ProxyGroupItem::default()`，会产生空名称的伪组。建议将 GLOBAL 表达为 `Option<ProxyGroupItem>`，同时迁移 RPC DTO、生成的 TypeScript 类型、query hook、代理页、dashboard 和托盘消费者。

GLOBAL 缺失时仍显示所有普通组；Global 模式的专用选择项显示不可用状态或不生成菜单，不能伪造一个可选择的 GLOBAL 并向内核提交。普通组名叫 `global` 时，托盘内部 ID 也不能与真实组名称混用。

这是可迁移的契约变更，应在一个完整调用路径内更新消费者并重新生成绑定，不增加旧字段兼容包装。如果首个修复只修改组发现函数，可以先保留现有 DTO，但必须明确 GLOBAL 缺失的展示仍是后续工作，不能将该阶段宣称为所有边界均完成。

## 快照比较与通知

现有 ProxiesActor 将 `Proxies` 与 provider 响应一起序列化比较。即便 groups 已稳定，`nodes`、provider map，以及未来保留的嵌套 map 顺序仍可能制造伪变化。因此确定化必须覆盖参与比较的整个值，而不是只排序 `/group` 的数组。

建议在纯转换边界为无语义的名称索引建立固定顺序：R/nodes、provider 外层 map，以及参与输出的扩展对象键。保留 `groups` 展示顺序、组成员列表、provider 节点声明顺序和 history 样本顺序；不要递归排序所有 JSON 数组。对 JSON 对象的 canonical 编码可只用于比较，但展示数据也应采用稳定的 map 顺序。

第一阶段继续使用现有字节相等比较即可，先保证其输入语义正确。不要把 Adler32 换成另一个 hash 当作本 issue 的修复，也不要通过删除 history、alive 或更新时间字段来强行让指纹不变；它们的变化可能是真实状态变化。

若后续性能数据表明确有必要，再分别维护拓扑、选择与遥测投影，让托盘仅关注菜单相关变化。当前托盘已有自己的组顺序、成员与选中状态 diff，应先复用并测试这条路径。ProxiesActor 只发布自己拥有的域数据，UI 和托盘读取同一个规范化快照，不各自发现组。

## 架构落点与复杂度

| 组件                                     | 分类           | 职责                                                       |
| ---------------------------------------- | -------------- | ---------------------------------------------------------- |
| 现有 clash-api / ApiClient               | Adapter / port | 精确 URL、方法、协议 DTO、HTTP 错误与 IO deadline          |
| 现有 ProxiesActor / ProxiesClient        | Actor service  | 实例生命周期、读取与选择串行化、缓存、失效、通知、诊断去重 |
| `Proxies::from_responses` 及小型私有函数 | Pure service   | 分类、排序、成员解析、确定化，输入全部显式传入             |
| NyanpasuClient                           | 应用 facade    | 保持普通 async 应用 API                                    |
| RPC、query、页面、托盘                   | 边界与展示     | 消费同一快照，分别执行已有展示策略                         |

优先改进现有纯函数，不为单次调用创建新的 service trait、actor 或泛型策略框架。handler 等待完整操作，保持当前 actor mailbox 为唯一串行化机制；不引入进程内 RPC timeout。网络 deadline 保留在适配器，调用者离开不取消 owner 已开始的选择操作。

在哈希索引平均 O(1) 的条件下，设 R 有 N 条记录、GLOBAL 有 H 个成员、遗漏排序组数为 U、provider 节点记录数为 P、全部待解析成员数为 E，另设 group 列表长度为 Q，发现与补全约为 `O(N + Q + H + U log U + P + E)`，另加字符串比较与输出字节成本。内存约为节点索引、provider 候选和成员引用所需空间。保留名称引用，不为每个组重复复制完整节点对象。

前端当前会过滤 hidden 组，托盘有自己的模式投影。这是展示策略，与发现全集分开；本次先保留既有行为。若要统一 hidden 策略，应明确产品决策和测试，不能靠“修复漏组”顺便改变它。

## 分阶段实施建议

1. **协议前提**：在运行时子模块修正本调用路径的尾斜杠，增加三种协议的 exact-path/shape fixture 测试，完成子模块提交后更新应用指针。验证读取、单资源、选择与 provider 相关 URL，确认结构化错误没有被吞成空列表。
2. **能力与发现算法**：扩展 metadata Feature，贯通实例绑定、clash-api array/map 适配和 ProxiesActor；纯函数优先消费 group 返回，仅在不可用时遍历 R。GLOBAL 仅排序提示，遗漏组显式排序，保留空组、成员顺序和 provider 补全。补齐本 issue 的纯值回归测试，检查页面与托盘的普通组输出。
3. **完善快照契约**：完成 provider 类型与冲突处理、全值确定化、诊断、可选 GLOBAL 的完整消费链迁移。若这一步独立提交，清楚列出前一阶段仍未解决的边界。

三步均采用显式依赖和现有 facade。RPC 签名或 DTO 变化通过现有 export workflow 生成绑定；不手改生成文件，不新增原始 invoke 或 HTTP fetch。正式实现前按开发规范评估并选择 worktree；本次仅报告，已依用户指定写在当前工作区。

## 验收用例

| 场景                                         | 应断言的结果                                                                  | 测试边界           |
| -------------------------------------------- | ----------------------------------------------------------------------------- | ------------------ |
| 自定义 GLOBAL 只包含 Foo                     | Foo、Bar 等所有普通组保留                                                     | 纯函数             |
| 完全没有 GLOBAL / GLOBAL.all 缺失            | 仍发现全部普通组；可选 GLOBAL 契约无伪组                                      | 纯函数与消费者     |
| `all: []`，未知组类型                        | 空组保留，不依赖类型白名单                                                    | 纯函数             |
| GLOBAL 重复成员、自引用、普通节点混入        | 普通组恰好一次，GLOBAL 独立，节点不被当成组                                   | 纯函数             |
| JSON 对象键和 provider map 换序              | 输出展示顺序与比较值不变                                                      | 纯函数与 actor     |
| 实际组成员顺序改变、now 改变                 | 有效变化可见；选择变化使用既有 tray diff                                      | 纯函数与托盘       |
| provider-only、Inline、共享节点              | 元数据补全，节点只存一份，所有引用可解析                                      | 纯函数             |
| provider 同名异值、缺失成员                  | 不随机挑选、不丢组，诊断稳定且不过量                                          | 纯函数与 actor     |
| 嵌套组、组循环引用                           | 不递归发现全集，不栈溢出，保留原成员关系                                      | 纯函数             |
| 名称含空格、斜杠、百分号、中文               | path segment 编码正确，无意外尾斜杠                                           | 协议 adapter       |
| Mihomo array、Meow map、Clash-rs 无 `/group` | Mihomo/Meow 优先组列表，Clash-rs 推断；不合并多余组                           | adapter            |
| Mihomo 四种组、hidden、自定义 GLOBAL         | 稳定夹具中 `/group` 与 `/proxies` 的组名称集合一致；LoadBalance 无 now 仍保留 | 协议契约           |
| provider 过滤、成员更新与 emptyFallback      | 保留 API 给出的成员及顺序，不把已排除节点加回组                               | adapter 与纯函数   |
| Mihomo 空注册表的 `/group`                   | 按当前 Vec 契约返回 Error::Decode，不静默归一化为成功空列表                   | adapter            |
| HTTP 401、404、错误 JSON                     | 鉴权与实例错误传播；组列表 404/协议异常按 spec 诊断并推断，不伪装成功空列表   | adapter 与 actor   |
| 读取过程中 core 撤销                         | 不发布旧实例数据                                                              | actor fake adapter |
| caller 在选择后离开                          | owner 继续到终态，结果/退化遵循既有约定                                       | actor              |
| 缺 GLOBAL、hidden、Rule/Global 模式          | 页面和托盘按明确策略展示，无意外重复项                                        | query、组件和托盘  |
| IPC 与 HTTP 的 `get_proxies`                 | 同一 domain 快照、结构化错误与事件重同步                                      | 两种应用 transport |

actor 测试用 fake adapter 和显式应答，不靠 sleep；接口测试 mock 必须严格区分 `/proxies` 与 `/proxies/`，否则无法捕捉这次发现的问题。保留现有 provider 元数据和共享节点测试。

## 本轮已验证与尚未验证

此前接口核查完成三核 51 条隔离 GET 响应，并用捕获注册表做了 **10 项组发现模型检查**：每个内核各有捕获注册表投影、移除 GLOBAL、对象键反序三项，另有一个组合边界用例覆盖空组、重复、缺失、节点与自引用，全部通过。本次汇总沿用这些证据，没有将它们计为新增探测。

本次先按项目 clash-api 的公开方法、DTO、URL 构造和现有测试核对 API 定义，再沿 Mihomo 的配置构建、运行时发布、路由、接口断言、组成员解析和两层 JSON 序列化确认服务端实现，并比较指定 Alpha 文件差异；同时补查三核组测速参数、结果键语义及 Clash-rs 单资源方法注册。四种组的集合一致性、LoadBalance 实际响应、过滤与空注册表边界目前是源码确认及新增验收要求，未追加二进制实验；本次读取了既有 crate 测试，未将它们记为本轮执行通过。

模型只验证 proxies 兜底的发现与排序推理，不验证新增 group 优先和 Feature 控制，使用 PowerShell 对捕获值投影；它不是编译执行改造后的 Rust，未验证完整指纹、provider 冲突、actor 通知或 UI。上表是实施后的验收要求，本轮没有把它们标成已通过。

本次没有修改生产代码、运行完整 Cargo/前端测试或发布 GitHub 评论。后续实现应先补能复现当前漏组的 Rust 回归测试，再完成相关协议、actor 与消费者测试；涉及 DTO 时同时验证 IPC 和 HTTP，并按仓库开发规范完成相关检查。
