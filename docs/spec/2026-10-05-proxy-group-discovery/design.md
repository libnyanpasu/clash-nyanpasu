# 代理组发现：group 优先与 proxies 兜底

**日期：** 2026-10-05

**状态：** 已按 [审计报告](../../audit/2026-10-05-pr5596-runtime441-audit.md) 修订为实例级运行时探测；runtime PR [#441](https://github.com/libnyanpasu/nyanpasu-runtime/pull/441)，应用 PR 等待 runtime 发布。

**调查基线：** `3b942c760c74df0a7fa832dc455c3a15b676cb3b`；实现基线为 `def0dbf2716ff972255be47924401b60db7d482c`。

**运行时调查基线：** `889901cb57b1b1753354c2b7b0a442569601e417`；实现基线为 `909e7bd093f000be009592df87457c51741c99ca`；clash-api `1.0.0-rc.9`。

**需求：** group 相关接口可用时，使用内核返回的可靠组集合；不可用时从 proxies 推断。能力与运行中的内核绑定，且只探测一次。

**依据：** [接口调查](../../audit/2026-10-05-proxy-core-api-report.md)、[算法重构报告](../../audit/2026-10-05-issue-5112-proxy-algorithm-report.md)、[原始证据](../../audit/2026-10-05-proxy-core-api-evidence.json)、[issue 5112](https://github.com/libnyanpasu/clash-nyanpasu/issues/5112)。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 后续实施计划。调查报告中的后续扩展不自动扩大本 spec 范围。

## 1. 决策与范围

1. 组列表本次读取成功时，`Client::groups()` 的结果是组存在性的权威来源。不得再与 `/proxies` 推断出的额外组取并集。
2. 实例不提供组列表，或本次读取失败时，从本次 `/proxies` 响应中 `all.is_some()` 的记录推断组。
3. GLOBAL 从选定的组来源单独提取，只提供普通组排序提示；GLOBAL.all 不定义组全集。
4. 保留 `/proxies` 和 `/providers/proxies` 读取以构造共享 nodes；实例未被判定为不支持时并发增加一次 `/group` GET。三份响应不是事务快照。
5. 组列表能力由运行时探测决定，结果缓存在与实例一一绑定的 `ApiClient` 中。不新增 metadata Feature、版本下限或 IPC 绑定字段，也不要求升级服务。
6. 保持现有 actor、facade、Unified RPC 与共享节点模型。HTTP 与两种列表外形的解码留在 clash-api；能力缓存归 ApiClient；确定性转换留在现有纯函数。

本次包含组发现、必要的跨核 URL/列表 shape 适配、实例级能力探测、稳定组排序和节点补全。不接入 Meow 管理写接口，不统一组测速语义，不新增 actor、重试调度器或 UI 设置。

保留现有应用 Proxies DTO、hidden 展示策略、刷新周期和字节指纹机制。`global: Option<...>` 的完整消费者迁移、provider 同名冲突治理、全值指纹规范化与遥测分层留作后续工作，不能据本次完成宣称这些问题已解决。

## 2. 已确认事实与现有契约

接口定义以 [clash-api/src/api/proxies.rs](../../../../backend/nyanpasu-runtime/crates/clash-api/src/api/proxies.rs) 为准，调查基线的签名和路径如下；它们不是本 spec 新创的 API。

| 方法                | 返回类型                                        | 当前路径              | 本次处理                                                     |
| ------------------- | ----------------------------------------------- | --------------------- | ------------------------------------------------------------ |
| `groups()`          | `Result<Vec<Proxy>>`                            | `/group/`             | 使用 `/group`；同时接受 array/map，改为按名称索引的 IndexMap |
| `group(&ProxyName)` | `Result<Proxy>`                                 | `/group/{name}/`      | 核对并修正无尾斜杠路径；不用于枚举                           |
| `proxies()`         | `Result<IndexMap<ProxyName, Proxy>>`            | `/proxies/`           | 改为 `/proxies`                                              |
| `proxy_providers()` | `Result<IndexMap<ProviderName, ProxyProvider>>` | `/providers/proxies/` | 改为 `/providers/proxies`                                    |

Proxy 已同时表达节点和组，all 是 `Option<Vec<ProxyName>>`；now 可以缺失，LoadBalance 不能因无 now 被丢弃。继续复用 Proxy、ProxyProvider、ProxyName、ProviderName，不增加平行的 HTTP Group DTO。

| 内核     | 源码确认的组列表                                                                  | 能力边界      | 当前客户端障碍                              |
| -------- | --------------------------------------------------------------------------------- | ------------- | ------------------------------------------- |
| Mihomo   | `/group`，`{"proxies": [Proxy, ...]}`；按内部 ProxyGroup 接口筛选完整运行时注册表 | v1.11.2 起    | 数组可解码；列表无稳定顺序                  |
| Meow     | `/group`，`{"proxies": {name: Proxy, ...}}`；按运行时 members 标记筛选            | v0.18.0 起    | 当前 `/group/` 404；去斜杠后仍需 map 适配   |
| Clash-rs | 已核查版本无列表、无组详情 GET；仅有 group delay                                  | 列表能力为 No | 直接从 proxies 推断；delay 不能证明列表存在 |

版本界限的引入提交与标签比对见接口报告，仅作背景；实现不依赖版本表，以实例的实际应答为准。Premium 未做本轮源码调查，同样由探测决定。

三核无尾斜杠 proxies/provider 路径的 51 条原始 HTTP 探测来自现有 sidecar，未通过修改后的 typed client。Meow 管理扩展 `/api/proxy-groups` 枚举配置声明并补运行时成员，不能代替此处的运行时组权威来源。

## 3. 能力与协议配置

### 3.1 实例级探测

能力只取决于运行中的内核进程，不取决于配置或版本表。`ApiClient` 本身与一个实例一一绑定：binding 改变时 CoreActor 创建新的 ApiClient，旧实例的 ApiClient 被永久撤销，不能重新绑定。因此探测结果存入 ApiClient 共享的 `Arc<OnceLock<bool>>`，天然与该实例的内核类型和版本绑定；换核、升级或重启都会得到新的探测。

| 本次 `/group` 结果        | 本次读取 | 探测结论                     |
| ------------------------- | -------- | ---------------------------- |
| 成功，包括空列表          | 采用列表 | 支持；以后的失败不再改写结论 |
| 404 / 405                 | 推断     | 不支持；该实例以后不再请求   |
| 其他 HTTP、网络或解码失败 | 推断     | 未定；下次读取继续请求       |

只有第一个确定的答案生效，因此每个实例最多完成一次探测。不调用 group delay 或写接口探测，不额外发请求，也不改 IPC、manager 或 service。

### 3.2 clash-api 的最小改动

两种列表外形在 JSON 层面互斥（数组 vs 对象），所以一个 visitor 同时实现 `visit_seq` 与 `visit_map`，不需要按内核选择格式。`groups()` 改为返回 `IndexMap<ProxyName, Proxy>`，与 `proxies()` 一致；两个内核的列表都来自以名称为键的运行时注册表，不会出现重名。

clash-api 不做语义校验：缺少 `all` 的组仍是组，判断记录属于调用方。缺少 envelope、`null` 或字段类型错误仍是 `Error::Decode`。

URL 改动限定在本代理调用链，检查 proxy 详情/选择、provider 详情/更新相同的空尾段构造。保留动态名称分段编码；不全局裁剪任意 URL，不请求真实写操作来验证路径。无尾斜杠也不会给 Clash-rs 补出 DELETE 能力，本次不承诺全 API 兼容。

## 4. 本次读取与失败规则

`ApiClient::proxy_snapshot()` 返回具名的 `ProxySnapshot { proxies, providers, groups: Option<IndexMap> }`。`Some` 为本次列表（空 IndexMap 表示权威的空列表），`None` 表示本次需要推断。不暴露为新的前端 RPC。

三个请求在同一个实例验证与 30 s deadline 边界内并发读取。组读取本身不会令整次读取失败，失败只记录 debug 日志。不开第二轮 HTTP 猜路径，不自动重试。

| 情况                                          | 本次结果                             |
| --------------------------------------------- | ------------------------------------ |
| 实例已判定不支持                              | 不请求 `/group`，从 R 推断           |
| L 成功，包括空列表                            | 只采用 L                             |
| L 任意失败（含 401/403）                      | 从 R 推断；探测结论见 §3.1           |
| R 或 P 失败（含 401/403）                     | 整次刷新失败，不构造残缺的新快照     |
| 整体 30 s deadline 到期、binding 不匹配、撤销 | 整次刷新失败，不发布收集到的部分数据 |

三个请求共用同一个 client 与 secret，鉴权失败必然同时出现在 R 上，因此不对 L 的 401/403 单独特判。不要把 secret 或响应正文写入日志。

网络超时发生在适配器，不给 actor 请求/响应增加 timeout。

## 5. 纯转换算法

### 5.1 决定组集合

```text
if groups is Some(L):
    S = 按精确 name 索引 L 的完整记录
else:
    S = R 中 all 为 Some 的完整记录

global_record = S["GLOBAL"]（如有）
ordinary_names = keys(S) - {"GLOBAL"}
```

成功 L 中不包含而 R 中存在的组，不追加到 ordinary_names；仍可留在 nodes 作为运行时记录。L 存在、R 缺失的组必须保留。两者同名组记录不一致时，整条组记录以 L 为准，不拼接一个来源的 all 和另一个来源的 now。GLOBAL 也从 S 提取，不从另一来源补回。

空列表意味着本次权威组集合为空；与缺少/错误 envelope 不同。`all: []` 是空组，`now` 缺失不排除组，未知 type 不排除组，hidden 不影响存在性。精确名称 `global` 与 `GLOBAL` 不同。

### 5.2 排序

按 global_record.all 的成员顺序，从 ordinary_names 中首次取出对应组；剩余名称按 Rust `String::cmp` 排序追加。不依赖 `/group` 数组或 map 外层迭代顺序。

GLOBAL 中的普通节点、重复名字、缺失名字及 GLOBAL 自引用不进入普通 groups。每个组恰好出现一次；组成员 all 原顺序及重复关系不在此步骤改写。不递归遍历组引用，因此嵌套/循环关系不会影响发现终止。

这里保证组集合与组展示顺序稳定，不承诺现有整体指纹在 provider/map 换序时已经稳定；全值 canonical 比较属于报告后续阶段。

### 5.3 nodes 与成员

从 R 建立共享 nodes，随后用 S 的组记录覆盖同名项并补入 R 缺失的组。对 S 的所有组（含 GLOBAL）按原 all 收集引用，沿用现有 provider 补全与 Unknown 占位，不因缺节点删除整个组。组内 all 继续是名字列表，节点记录只保存一次。

group 的 all 已体现内核 provider 过滤、运行时成员和 emptyFallback；不得把 provider 的所有节点重新加入组，也不使用 raw YAML 替换 API 的成员关系。缺失引用以 Unknown 节点占位，不能递归展开或擅自修复组拓扑。

实现基线已合入 #5592，保留现有 HTTP/File/Inline provider 筛选和重名覆盖政策。因此多个 provider 同名异值仍是已知限制，单独按算法报告后续阶段处理；本 spec 的组可靠性不等于所有节点来源歧义已消除。

### 5.4 GLOBAL 缺失

算法用可缺失的 global_record 排序；缺失时普通组全部按名称排列，不返回空普通 groups。应用现有 `global` 字段的 default 投影暂时保留，继续遵循当前消费者行为。这是既有 DTO 的限制，不是创建一个新的真实内核组；不要在本次加入“合成 GLOBAL 并可选”的行为。

完整消除空名称 default 需要迁移 RPC、query、dashboard、代理页和托盘，另作 `Option<ProxyGroupItem>` 变更。本次验收明确只保证普通组不依赖 GLOBAL 存在，不能声称已完成 Global 模式缺失组的全部展示边界。

## 6. 所有权、状态与一致性

| 位置                                     | 分类          | 本次职责                                                         |
| ---------------------------------------- | ------------- | ---------------------------------------------------------------- |
| clash-api / ApiClient                    | Adapter       | 精确路径、两种列表外形的解码、实例级探测缓存、deadline、实例校验 |
| Proxies::from_responses 或其小型私有函数 | Pure service  | 显式 R/P/组来源输入、集合、排序、节点索引                        |
| ProxiesActor                             | Actor service | 当前快照、失效、通知；handler await 完整刷新                     |
| NyanpasuClient / RPC / UI                | facade 与边界 | 消费同一快照，保持现有接口                                       |

不新增全局 FeatureSet，不让 pure service 访问 HTTP、文件、Tauri 或日志。探测缓存是实例 adapter 内部的一次性事实，不是 actor 状态；不让 ProxiesActor 拉取 sibling 状态拼装能力。无第二队列、后台探测 actor 或 raw ActorRef 暴露。

多 GET 之间可能发生同实例配置/provider 更新。L 优先解决本次集合冲突，缺失节点占位；不承诺 instance_id 是配置版本。仍执行读取前后 binding 检查和刷新发布前撤销检查。旧实例的结果与探测结论均不能覆盖新实例。

组来源不加入 UI 数据指纹；同样的数据从推断切换为列表时不产生虚假的 UI 数据变化。真实组、成员、选择变化继续走现有通知。

## 7. 实施顺序与文件边界

1. **路径修正。** runtime clash-api 去掉代理、组、provider 端点的尾斜杠，并补路径编码测试。
2. **列表解码。** runtime clash-api 的 `groups()` 接受数组与对象并返回 IndexMap，新增 tests/proxy_cores 三核实进程回归。
3. **读取及纯算法。** 应用修改 ApiClient::proxy_snapshot 与实例级探测、ProxiesActor refresh 和 Proxies::from_responses，落地失败表、排序与 nodes 覆盖。子模块先形成可引用提交，再更新应用指针。

正式代码实现按仓库 workflow 选择 worktree 并准备构建前提。用户已选择独立 worktree；实现与测试结果在 PR 中记录。

## 8. 验收标准

| 场景                                        | 必须验证                                                  |
| ------------------------------------------- | --------------------------------------------------------- |
| Mihomo array、Meow map                      | 同一个 `groups()` 无配置地解码两者；精确 URL 无尾斜杠     |
| Clash-rs 仅 delay 存在                      | 首次 404 后该实例不再请求列表，从 proxies 推断            |
| 新实例（换核、重启）                        | 重新探测，不继承旧实例结论                                |
| L={Foo}、R 中有 Foo 和 Bar                  | 普通组只有 Foo；不因 Bar.all 存在补回                     |
| L={Foo,Bar}、R 只有 Foo                     | Bar 仍保留，nodes 补入 Bar，成员缺失有占位                |
| L 与 R 的 Foo.all/now 不同                  | 完整采用 L 的 Foo，不能交叉拼接                           |
| L 为成功空列表                              | groups 为空，不切换到 R 推断；GLOBAL 不从 R 补回          |
| 无 GLOBAL、自定义 GLOBAL 子集               | S 中全部普通组保留；未列出的组稳定追加                    |
| 空组、LoadBalance 无 now、未知 type、hidden | 不因这些条件丢组；hidden 由已有展示层处理                 |
| 列表换序、R map 换序                        | 同一组集合与 GLOBAL 提示产生相同组排序                    |
| GLOBAL 重复、自引用、普通节点；嵌套组循环   | 每组一次，GLOBAL 独立，不递归发现或栈溢出                 |
| group 根 5xx、网络错误、解码失败            | 本次推断；下一刷新继续请求，成功后采用列表                |
| 缺 `all` 的列表记录                         | 仍是组，按空组处理                                        |
| R/P 失败、整体 timeout                      | 整次失败，不伪装空列表，不发布部分新快照                  |
| 读取期间换 core / binding 改变              | 不发布旧数据；新实例使用新的探测缓存                      |
| provider-only 成员、过滤和 emptyFallback    | 保留内核 all，按现有政策补节点，不把 provider 全量加入组  |
| 动态名称包含斜杠、百分号、空格、中文        | 单 path segment 正确编码，不误匹配其他路由                |
| UI 与托盘、IPC 与 HTTP get_proxies          | 共同消费同一组集合，既有错误映射、hidden 与事件重同步不变 |

测试优先使用真实 DTO 纯值、严格区分尾斜杠的 HTTP mock、注入 fake adapter 的 actor 测试；用应答/事件同步，不 sleep。三核实进程夹具只使用隔离 controller 和本地 provider；不使用 delay 或写请求。稳定配置下另比对 Mihomo 四种组的 `/group` 与 `/proxies` 集合，区分端到端证据与 mock。

检查命令遵循主仓库及 runtime 各自现有任务目录；按受影响 crate 执行单测/集成、格式、Clippy 与架构检查。若改变公开 UI DTO 或 RPC，必须重新审视本 spec 范围并通过现有导出流程生成绑定，不手改生成文件。

## 9. 本次交付的验证边界

本 spec 基于固定源码、现有 clash-api 定义及此前 51 条隔离 GET 证据。实现阶段的验证结果记录在 PR 中。
