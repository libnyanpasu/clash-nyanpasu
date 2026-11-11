# 内核控制面抽象

[sing-box 内核接入设计](design.md) 的子设计。回答需求 ①：把现有 Clash API 客户端二次封装，抹平 Clash REST 与 sing-box gRPC 的差异，逐项列出不支持的功能与处理办法，并同步更新前端 hooks。

路径缩写同主设计：`C/`、`T/`、`R/`、`F/`、`SB/`。

---

## 1. 目标与非目标

目标：

- 应用层只面对一个控制面句柄，不知道背后是 REST 还是 gRPC。
- 每个操作在每种内核上的可用性在编译期或绑定时确定，不靠运行时试错。
- 前端从后端读能力表，不按内核名分支。
- 带 `clash` 字样但语义中性的命令、事件、hook 改成中性名。

非目标：

- 不重新定义节点记录类型（主设计 D4）。
- 不暴露 sing-box gRPC 独有的大块能力（Tailscale、OpenVPN、USB/IP 等）。
- 不让前端直接连内核。

---

## 2. 现状

`ApiClient`（`C/actor_v2/api.rs`）是实例绑定、可吊销的能力句柄，内部持有 `clash_api::Client`。它的全部操作：

| 分组   | 操作                                                                                            | 消费者                                          |
| ------ | ----------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| 流     | `connections_ws`、`logs_ws`、`traffic_ws`、`memory_ws`                                          | `C/clash/ws.rs:648-651`                         |
| 代理   | `proxy_snapshot`、`select_proxy`、`clear_proxy_selection`、`proxy_delay`、`group_delay`         | `C/proxies.rs`、`C/client/clash_api.rs`         |
| 提供者 | `update_proxy_provider`、`healthcheck_proxy_provider`、`rule_providers`、`update_rule_provider` | `C/proxies.rs`、`C/client/clash_api.rs`         |
| 规则   | `rules`                                                                                         | `C/client/clash_api.rs`                         |
| 配置   | `configs`                                                                                       | `C/client/clash_api.rs`、`C/client/geo_mode.rs` |
| 连接   | `connections`、`close_connection`、`close_all_connections`                                      | `C/client/clash_api.rs`、`C/connections.rs`     |
| 版本   | `version`                                                                                       | `C/client/clash_api.rs`                         |

core-manager 另有三处直接调用 Clash API：健康探测的 `/version`、热重载的 `PUT /configs`、热补丁的 `PATCH /configs` 与读回（`R/nyanpasu-core-manager/src/health/probe.rs:130`、`manager/apply.rs:224-270`）。这三处属于 [`core-manager.md`](core-manager.md)。

---

## 3. 结构

```text
C/actor_v2/api.rs        ApiClient：绑定、吊销、期限、能力表
C/actor_v2/api/clash.rs  Clash 驱动：clash_api::Client
C/actor_v2/api/sing_box.rs  sing-box 驱动：sing_box_api::Client
R/clash-api/             协议客户端（已有）
R/sing-box-api/          协议客户端（新增）
```

```rust
/// 协议驱动。工作区内枚举，保持穷尽。
enum ApiDriver {
    Clash(ClashDriver),
    SingBox(SingBoxDriver),
}
```

- `ApiClient` 的公开方法签名不带协议类型以外的新概念：返回值仍是现有 DTO。每个方法体是对 `ApiDriver` 的一次 `match`。
- 某驱动不支持的操作返回 `ApiError::Unsupported { operation }`，不返回空数据冒充成功。调用方在调用前应已读过能力表；这个错误是防线，不是常规路径。
- 绑定信息 `CoreApiConnection` 增加 `protocol` 字段（`clash-rest`、`sing-box-grpc`），旧服务不发这个字段时按 `clash-rest` 解。`ApiClient::new` 按它构造驱动。
- 期限、吊销、`same_instance` 检查留在 `ApiClient`，两个驱动共用。

---

## 4. 操作矩阵

三列分别是：Clash 系内核上的 Clash API、sing-box 的 gRPC `StartedService`、sing-box 上的 Clash API（仅作对照，Nyanpasu 不用）。

### 4.1 流

| 操作 | Clash 系                   | sing-box gRPC                           | sing-box Clash API | sing-box 驱动的做法                                                                      |
| ---- | -------------------------- | --------------------------------------- | ------------------ | ---------------------------------------------------------------------------------------- |
| 流量 | `WS /traffic`              | `SubscribeStatus`：`uplink`、`downlink` | 有                 | 订阅一次 `SubscribeStatus`，间隔 1 秒，映射成 `ClashWsTraffic`                           |
| 内存 | `WS /memory`               | `SubscribeStatus`：`memory`             | 有                 | 与流量共用同一条 `SubscribeStatus`，拆成两个下游流；`oslimit` 无对应，置 0               |
| 连接 | `WS /connections` 全量快照 | `SubscribeConnections` 增量事件         | 有                 | 驱动内维护连接表，把 NEW / UPDATE / CLOSED 事件折叠成与 Clash 相同的周期性全量快照（§6） |
| 日志 | `WS /logs?level=`          | `SubscribeLog`：分批、带级别            | 有                 | 映射成现有日志事件；级别过滤在驱动侧做（gRPC 订阅不带级别参数）                          |

`SubscribeStatus` 还带上下行总量与出入连接数，Clash 侧没有对应流；首版丢弃，不新增 DTO 字段。

### 4.2 代理

| 操作                  | Clash 系                                       | sing-box gRPC                            | sing-box Clash API | sing-box 驱动的做法                                                                                                 |
| --------------------- | ---------------------------------------------- | ---------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------- |
| 代理快照              | `GET /proxies`、`/providers/proxies`、`/group` | `SubscribeGroups`、`SubscribeOutbounds`  | 有，provider 为空  | 驱动持有两条订阅的最新值，`proxy_snapshot` 返回由它们合成的 `ProxySnapshot`；`providers` 为空（§5）                 |
| 选择节点              | `PUT /proxies/{group}`                         | `SelectOutbound`                         | 有                 | 直接映射；非 selector 组返回的 gRPC 错误映射为现有的“不可选”错误                                                    |
| 解除固定              | `DELETE /proxies/{group}`                      | 无                                       | 无                 | **不支持。** sing-box 的 urltest 组没有“固定”概念。能力表 `unfix_selection = false`，前端不显示入口                 |
| 节点测速              | `GET /proxies/{name}/delay?url&timeout`        | `URLTest(tag)`，立即返回，结果经订阅回传 | 有，支持 url       | 调 `URLTest` 后等待 `SubscribeOutbounds` 中该 tag 的 `urlTestTime` 前进或延迟被清除，期限取请求的 `timeout`（§7）   |
| 组测速                | `GET /group/{name}/delay?url&timeout`          | `URLTest(groupTag)`                      | 有                 | 同上，等待组内全部成员的 `urlTestTime` 前进或期限到                                                                 |
| 自定义测速 URL 与超时 | 支持                                           | 不支持：`URLTest` 只收 tag               | 支持               | **部分支持。** 能力表 `delay_test_url = false`；`timeout` 只作为驱动等待结果的期限。前端在该能力为假时隐藏 URL 输入 |
| 分组展开状态          | 无                                             | `SetGroupExpand`                         | 无                 | 不使用。Nyanpasu 的分组布局存在应用侧                                                                               |

### 4.3 提供者与规则

| 操作                 | Clash 系                  | sing-box gRPC | sing-box Clash API | 处理办法                                                                                                             |
| -------------------- | ------------------------- | ------------- | ------------------ | -------------------------------------------------------------------------------------------------------------------- |
| 代理提供者列表与更新 | `/providers/proxies` 系列 | 无            | 空桩               | **不支持。** sing-box 没有 provider 概念。能力表 `proxy_providers = false`，Providers 页的代理页签隐藏               |
| 规则提供者列表与更新 | `/providers/rules` 系列   | 无            | 空桩               | **不支持。** sing-box 的 rule-set 由内核自己按 `update_interval` 更新，没有控制接口。能力表 `rule_providers = false` |
| 规则列表             | `GET /rules`              | 无            | 有                 | **换来源。** 从已应用的运行配置投影 `route.rules`：Nyanpasu 持有它构建的最终配置。能力表 `rules = static`（§5.2）    |

规则列表的“换来源”需要一条新的读取路径：`ApiClient::rules` 在 sing-box 驱动下不可用，由 `NyanpasuClient` 在上一层按能力表选择“问内核”还是“读已应用配置”。投影是纯函数，输入是已应用产物，输出沿用现有 `ClashRule` 形状（`type`、`payload`、`proxy`）。逻辑规则（`type: logical`）展开成一行，`payload` 用其子规则的紧凑文本。命中计数等运行期字段留空。

应用目前不调用规则启停（`ApiClient` 没有对应操作），所以能力表不为它留位置；将来接入 Mihomo 的 `PATCH /rules/disable` 时再加。

### 4.4 配置、模式、连接、版本

| 操作             | Clash 系                         | sing-box gRPC                              | sing-box Clash API | sing-box 驱动的做法                                                                                          |
| ---------------- | -------------------------------- | ------------------------------------------ | ------------------ | ------------------------------------------------------------------------------------------------------------ |
| 读运行配置       | `GET /configs`                   | 无                                         | 仅 mode 等少数字段 | **拆分。** 模式走 `GetClashModeStatus`；其余字段（端口、`allow-lan`、日志级别）从已应用配置读，不问内核      |
| 读当前模式与列表 | `GET /configs` 的 `mode`         | `GetClashModeStatus`、`SubscribeClashMode` | 有，含 `mode-list` | 新操作 `mode_status()`：返回当前模式与可选列表。Clash 驱动返回固定列表（按内核种类）与 `/configs` 的 `mode`  |
| 切换模式         | 经运行期覆盖项写 `mode`          | `SetClashMode`                             | 有                 | 直接映射。模式不进 Nyanpasu 的状态文档，由 sing-box 的 cache file 记住（§9.1 的 `set_core_mode`）            |
| geodata 模式     | `GET /configs` 的 `geodata-mode` | 无                                         | 无                 | sing-box 1.12 起没有 geoip / geosite。`C/client/geo_mode.rs` 对 sing-box 家族固定报告 mmdb，不调内核         |
| 连接快照         | `GET /connections`               | 由 `SubscribeConnections` 维护的表         | 有                 | 返回驱动内连接表的当前快照                                                                                   |
| 关闭一个连接     | `DELETE /connections/{id}`       | `CloseConnection`                          | 有                 | 直接映射                                                                                                     |
| 关闭全部连接     | `DELETE /connections`            | `CloseAllConnections`                      | 有                 | 直接映射                                                                                                     |
| 版本             | `GET /version`                   | `GetVersion`：`version`、`apiVersion`      | 有                 | 映射成现有 `ClashVersion`：`version` 取原值，`meta`、`premium` 置假；`apiVersion` 只在驱动内部用于兼容性核对 |

### 4.5 sing-box 独有、首版接入的能力

| 能力     | gRPC                    | 用途                                                                                                 |
| -------- | ----------------------- | ---------------------------------------------------------------------------------------------------- |
| 弃用警告 | `GetDeprecatedWarnings` | 在 Profile 检视页与内核状态里提示“当前配置使用了将在某版本移除的字段”。能力表 `deprecation_warnings` |
| 启动时间 | `GetStartedAt`          | S7 的 SIGHUP 生效验证；首版不暴露给前端                                                              |

### 4.6 小结：需要在 UI 上处理的差异

| 差异           | Clash 系 | sing-box | UI 行为                                                        |
| -------------- | -------- | -------- | -------------------------------------------------------------- |
| 代理提供者     | 有       | 无       | 隐藏 Providers 的代理页签；代理页不显示 provider 分节          |
| 规则提供者     | 有       | 无       | 隐藏 Providers 的规则页签                                      |
| 规则列表       | 来自内核 | 静态     | 规则页照常显示列表，页眉注明“来自已应用配置”                   |
| 解除固定       | 有       | 无       | 不显示“恢复自动选择”                                           |
| 自定义测速 URL | 有       | 无       | 隐藏 URL 输入                                                  |
| 模式列表       | 固定     | 动态     | 模式切换按后端返回的列表渲染；列表为空时隐藏模式切换并显示说明 |
| 弃用警告       | 无       | 有       | 有警告时在内核状态处显示入口                                   |

模式列表为空的情况：sing-box 的模式来自路由规则里的 `clash_mode` 项，Profile 没写这类规则时列表为空，切换模式没有意义。

---

## 5. 能力模型

```rust
/// 当前运行实例的控制面能力。随内核状态事件刷新。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct CoreApiFeatures {
    pub proxy_providers: bool,
    pub rule_providers: bool,
    pub rules: RulesSupport,
    pub unfix_selection: bool,
    pub delay_test_url: bool,
    pub deprecation_warnings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RulesSupport {
    /// 内核提供规则列表。
    Core,
    /// 内核不提供；列表来自已应用的运行配置。
    Static,
}
```

- 取值由内核种类决定，写成对 `CoreKind` 的无通配穷尽 `match`，每一格附上游依据，写法同 `core-specific-config.md` §3.2。
- 首版取值：

  | 内核种类     | `proxy_providers` | `rule_providers` | `rules`  | `unfix_selection` | `delay_test_url` | `deprecation_warnings` |
  | ------------ | ----------------- | ---------------- | -------- | ----------------- | ---------------- | ---------------------- |
  | Mihomo       | 真                | 真               | `core`   | 真                | 真               | 假                     |
  | ClashRust    | 待 S1 核对        | 待 S1 核对       | `core`   | 待 S1 核对        | 真               | 假                     |
  | ClashPremium | 待 S1 核对        | 待 S1 核对       | `core`   | 待 S1 核对        | 真               | 假                     |
  | Meow         | 待 S1 核对        | 待 S1 核对       | `core`   | 待 S1 核对        | 真               | 假                     |
  | SingBox      | 假                | 假               | `static` | 假                | 假               | 真                     |

  标“待 S1 核对”的格子在 S1 按 [代理组内核接口核查报告](../../audit/2026-10-05-proxy-core-api-report.md) 与上游源码填实；填不出依据的格子不能进主干。

- 模式列表不放进能力表：它是运行期数据，随 Profile 变化，由 `mode_status()` 返回。
- 能力表与 `ConfigFeatures` 的分工：`ConfigFeatures` 回答“内核读哪些配置键、接受哪些值”，在没有运行实例时也有意义；`CoreApiFeatures` 回答“运行中的实例能做哪些操作”。两者都由内核种类推出，但消费者与刷新时机不同，不合并。

### 5.1 提供给前端

- 新增查询 `get_core_api_features`，薄命令，调 `NyanpasuClient::core_api_features()`。
- 没有运行实例时返回当前选中内核种类的能力（列表与页面仍需决定是否显示）。
- 前端通过应用配置事件与内核状态事件失效这个查询，做法与 `useConfigFeatures` 相同（`F/query/src/ipc/use-config-features.ts`）。
- 是否开启 HTTP：该查询只读且不含敏感信息，按 `rpc.md` 评审后可以 `rpc(http)`。

### 5.2 规则的三种来源

`NyanpasuClient::rules()` 按 `RulesSupport` 分派：

- `Core`：问内核（现有路径）。
- `Static`：读已应用产物并投影。产物来自现有的已应用运行配置检视路径（`C/client/runtime_inspection.rs` 的 `inspect_applied_runtime`）。

---

## 6. 流模型归一

应用现有的下游（`C/clash/ws.rs`、流量记账、连接详情订阅）以 Clash 的模型为准：连接是周期性的全量快照，流量与内存是周期性的采样。sing-box 驱动负责把 gRPC 的模型折叠成这个形状，下游不改。

连接表折叠规则：

1. 订阅 `SubscribeConnections`，间隔与 Clash 侧一致。
2. 收到 `reset = true` 的帧：清空连接表，再应用帧内事件。
3. `NEW`：插入。`UPDATE`：按 `uplinkDelta` / `downlinkDelta` 累加或直接取 `*Total`（以 V8 的验证结果为准）。`CLOSED`：移除，并把该连接作为“已关闭”交给流量记账。
4. 每处理完一帧，向下游发一份全量快照，形状同 `clash_api::Connections`。

字段映射（gRPC `Connection` → Clash 连接元数据）：

| Clash 字段                                                              | 来源                                 | 备注                                                         |
| ----------------------------------------------------------------------- | ------------------------------------ | ------------------------------------------------------------ |
| `id`                                                                    | `id`                                 |                                                              |
| `metadata.network`                                                      | `network`                            |                                                              |
| `metadata.type`                                                         | `inboundType`，辅以 `inbound`（tag） | 形如 `mixed/mixed-in`，与 sing-box Clash API 一致            |
| `metadata.sourceIP` / `sourcePort`                                      | 解析 `source`                        |                                                              |
| `metadata.destinationIP` / `destinationPort`                            | 解析 `destination`                   | 目标是域名时 IP 留空                                         |
| `metadata.host`                                                         | `domain`                             |                                                              |
| `metadata.processPath`                                                  | `processInfo.processPath`            | 取决于 Profile 是否开启进程查找                              |
| `metadata.sniffHost`、`dnsMode`、`specialProxy`、`remoteDestination` 等 | 无                                   | 留空                                                         |
| `chains`                                                                | `chainList`                          | 顺序以 V8 验证为准，必要时反转以对齐 Clash 的出口在前        |
| `rule` / `rulePayload`                                                  | `rule`                               | sing-box 只给一个字符串；整体放进 `rule`，`rulePayload` 留空 |
| `upload` / `download`                                                   | `uplinkTotal` / `downlinkTotal`      |                                                              |
| `start`                                                                 | `createdAt`                          | 毫秒时间戳转 RFC 3339                                        |

流量记账（`backend/nyanpasu-traffic`）以 `chains`、`rule` 为维度。sing-box 的规则字符串格式与 Clash 不同，同一份统计库里会并存两种风格的规则键；这不影响正确性，拓扑与报表按字符串原样分组。

---

## 7. 测速的完成判定

`URLTest` 是异步的：调用立即返回，结果写入内核的测速历史，再经订阅推送。驱动的做法：

1. 调用前记下目标 tag 当前的 `urlTestTime`。
2. 调 `URLTest(tag)`。`NotFound` 映射为“节点不存在”。
3. 等待 `SubscribeOutbounds` 的下一份数据中该 tag 满足其一：`urlTestTime` 大于记下的值（成功，取 `urlTestDelay`）；延迟被清除（失败）。
4. 到达期限仍未满足：返回 `ApiError::Timeout`。

组测速对组内每个成员应用同一判定，全部落定或期限到即返回已落定的部分。

“失败时延迟被清除且订阅方能看到”是假设 V7。若验证不成立，则失败只能靠超时判定，文档与 UI 文案要相应改为“超时或失败”。

---

## 8. sing-box 驱动与协议 crate

### 8.1 `sing-box-api`（runtime 仓）

- 位置：`R/sing-box-api/`，与 `clash-api` 同级。
- 传输：tonic 通道，h2c，目标是回环地址上的 API 服务端口。每个请求带 `authorization: Bearer <secret>` 元数据。
- proto：从上游 `daemon/started_service.proto` 裁剪出用到的 RPC 与消息，文件头注明上游 tag 与提交。裁剪不改字段号与方法路径，线上兼容。
- 用到的 RPC：`GetVersion`、`SubscribeStatus`、`SubscribeLog`、`GetDefaultLogLevel`、`SubscribeGroups`、`SubscribeOutbounds`、`GetClashModeStatus`、`SubscribeClashMode`、`SetClashMode`、`URLTest`、`SelectOutbound`、`SubscribeConnections`、`CloseConnection`、`CloseAllConnections`、`GetDeprecatedWarnings`、`GetStartedAt`，以及 `grpc.health.v1.Health/Check`。
- 生成代码提交进仓库，构建不需要 `protoc`。提供一个重新生成的脚本与一个测试，测试核对提交的生成代码与 proto 一致。备选是像 `blossom/server-agent` 那样手写 prost 消息；消息有十几个且带嵌套，手写易错，不采用。
- 对外是普通的 async 方法与流，错误类型区分传输错误、鉴权失败、`Unimplemented`、`NotFound`。
- 生成的枚举镜像外部协议，匹配时列出已知变体，通配分支给保守行为（`rust.md` §5.5）。

### 8.2 兼容性核对

驱动在绑定后的第一次调用里取 `GetVersion`：

- `apiVersion` 不在驱动支持的集合内：所有操作返回 `ApiError::Unsupported`，并在内核状态里报告“控制面版本不受支持”。首版集合为 `{4}`。
- 这是给用户自行替换二进制准备的防线。正常路径上，世代门控已经保证安装的版本在支持范围内（[`schema-compat.md`](schema-compat.md)）。

### 8.3 重连

API 服务随内核实例重建。`ApiClient` 是实例绑定的，内核重启后旧句柄被吊销，消费者按现有流程重新取句柄，不需要驱动自己跨实例重连。同一实例内的流断开按现有 `clash/ws.rs` 的重试策略处理。

---

## 9. 命令与事件改名

原则：名字里的 `clash` 只在语义确实是 Clash 专属时保留。改名与行为变更分开提交；改名 PR 同时迁移全部调用方并重新生成绑定，不留别名（`architecture.md` 的兼容策略）。

### 9.1 RPC 命令

| 现名                                   | 新名                             | 备注                                               |
| -------------------------------------- | -------------------------------- | -------------------------------------------------- |
| `clash_api_get_version`                | `get_core_api_version`           |                                                    |
| `clash_api_get_configs`                | `get_core_mode_status`           | 语义收窄为当前模式与模式列表；其余字段读已应用配置 |
| `clash_api_get_rules`                  | `get_core_rules`                 | 按 `RulesSupport` 分派                             |
| `clash_api_get_providers_proxies`      | `get_proxy_providers`            |                                                    |
| `clash_api_get_providers_rules`        | `get_rule_providers`             |                                                    |
| `clash_api_update_providers_rules`     | `update_rule_provider`           | 与已有的 `update_proxy_provider` 对齐              |
| `clash_api_healthcheck_proxy_provider` | `healthcheck_proxy_provider`     |                                                    |
| `clash_api_get_proxy_delay`            | `get_proxy_delay`                | `url` 变为可选                                     |
| `clash_api_get_group_delay`            | `get_group_delay`                | `url` 变为可选                                     |
| `clash_api_delete_connections`         | `close_connections`              |                                                    |
| `get_clash_ws_snapshot`                | `get_core_stream_snapshot`       |                                                    |
| `set_clash_ws_recording`               | `set_core_stream_recording`      |                                                    |
| `clear_clash_ws_history`               | `clear_core_stream_history`      |                                                    |
| `subscribe_clash_connection_details`   | `subscribe_connection_details`   | 仍是桌面专属的变更命令（`rpc.md`）                 |
| `unsubscribe_clash_connection_details` | `unsubscribe_connection_details` |                                                    |
| （新增）                               | `get_core_api_features`          | 查询                                               |
| （新增）                               | `set_core_mode`                  | 变更，见下                                         |
| （新增，S6）                           | `get_core_deprecation_warnings`  | 查询                                               |

`set_core_mode(mode: String)` 是前端切换模式的唯一入口，`NyanpasuClient` 按家族分派：

- Clash 家族：把字符串解析成现有的 `Mode` 枚举，走现有的运行期覆盖项路径（持久化到 `ClashGuardOverrides.mode`，由 core-manager 热补丁）。行为与现在经 `patch_runtime_overrides` 改模式相同。
- sing-box 家族：核对字符串在 `mode_status()` 返回的列表内，调驱动的 `SetClashMode`。模式不写进 Nyanpasu 的状态文档：sing-box 的模式是 Profile 定义的任意字符串，装不进 `Mode` 枚举；开启 `experimental.cache_file` 后由 sing-box 自己记住上次的模式。

保留不改的：`get_clash_config`、`patch_clash_config`、`get_clash_info`、`copy_clash_env`（它们操作的是“Clash 设定”状态文档与给外部工具用的控制器信息，见 [`config-family.md`](config-family.md) §6）；`get_proxies`、`mutate_proxies`、`select_proxy`、`clear_proxy_fixed`、`update_proxy_provider`、`url_delay_test` 已经是中性名。`change_clash_core` 的改名属于 [`config-family.md`](config-family.md) §2。

`url` 变为可选是为了让“不支持自定义 URL”的内核有合法的调用形状：能力为假时前端不传，后端收到非空值返回 `ApiError::Unsupported`。

### 9.2 事件与类型

| 现名                                                                                  | 新名                                                                                                 |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `ClashWsEvent`、`ClashWsKind`、`ClashWsSnapshot`、`ClashWsUpdate`、`ClashWsRecording` | `CoreStreamEvent`、`CoreStreamKind`、`CoreStreamSnapshot`、`CoreStreamUpdate`、`CoreStreamRecording` |
| `ClashWsTraffic`、`ClashWsMemory`                                                     | `CoreTrafficSample`、`CoreMemorySample`                                                              |
| `ClashConnectionsConnectorState`、`ClashConnectionsSummary`                           | `CoreStreamConnectorState`、`ConnectionsSummary`                                                     |
| `ClashConnection`、`ClashConnectionDetails`                                           | `CoreConnection`、`ConnectionDetails`                                                                |
| `ClashVersion`                                                                        | `CoreApiVersion`                                                                                     |
| `ClashRule`                                                                           | `CoreRule`                                                                                           |

序列化后的字段名不变，只改 Rust 类型名与生成的 TypeScript 类型名。`Proxy`、`ProxyGroup`、`Proxies`、`Delay` 等不带 `clash` 前缀的不动。

---

## 10. 前端 hooks 迁移

全部在 `@nyanpasu/query` 内完成，页面改导入名。同一个 PR 里迁完调用方，不留再导出别名。

| 现有 hook（`F/query/src/`）                                     | 新名                                               | 变化                                                                                     |
| --------------------------------------------------------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `ipc/use-clash-proxies.ts` `useClashProxies`                    | `useProxies`                                       | 测速调用按 `delay_test_url` 决定是否带 URL；解除固定按 `unfix_selection` 暴露            |
| `ipc/use-clash-proxies-provider.ts`                             | `useProxyProviders`、`useUpdateProxyProvider`      | `enabled` 受 `proxy_providers` 控制                                                      |
| `ipc/use-clash-rules-provider.ts`                               | `useRuleProviders`、`useUpdateRuleProvider`        | `enabled` 受 `rule_providers` 控制                                                       |
| `ipc/use-clash-rules.ts` `useClashRules`                        | `useCoreRules`                                     | 返回值附带 `RulesSupport`，页面据此决定是否显示启停与来源说明                            |
| `ipc/use-clash-connections.ts`                                  | `useConnections`、`useCloseConnections`            |                                                                                          |
| `ipc/use-clash-traffic.ts`、`use-clash-memory.ts`               | `useCoreTraffic`、`useCoreMemory`                  |                                                                                          |
| `ipc/use-clash-logs.ts` `useClashLogs`                          | `useCoreLogs`                                      |                                                                                          |
| `ipc/use-clash-config.ts` `useClashConfig`                      | 拆成 `useCoreModeStatus` 与现有的设定 hook         | 模式与列表读 `get_core_mode_status`；其余字段读设定                                      |
| `ipc/use-proxy-mode.ts` `useProxyMode`                          | 保留名                                             | 模式集合改为后端返回的列表，切换改调 `set_core_mode`，删除按内核名判断 script 模式的分支 |
| `ipc/use-clash-cores.ts` `useClashCores`、`useClashCoreVersion` | `useCores`、`useCoreVersion`                       | 内核列表带家族；见 [`config-family.md`](config-family.md) §8                             |
| `ipc/use-clash-info.ts` `useClashInfo`                          | 保留名                                             | 语义是“给外部面板用的 Clash 控制器信息”，sing-box 下在未开启外部控制器时为空             |
| `provider/clash-ws-provider.tsx`、`clash-ws-state.ts`           | `core-stream-provider.tsx`、`core-stream-state.ts` | 类型名随 §9.2                                                                            |
| `provider/clash-connection-details-provider.tsx`                | `connection-details-provider.tsx`                  |                                                                                          |
| （新增）`ipc/use-core-api-features.ts`                          | `useCoreApiFeatures`                               | 形状同 `useConfigFeatures`                                                               |
| （新增，S6）`ipc/use-core-deprecation-warnings.ts`              | `useCoreDeprecationWarnings`                       | `enabled` 受 `deprecation_warnings` 控制                                                 |

页面侧需要改掉的按内核名判断：

| 位置                                            | 现状                                      | 改为                                                                   |
| ----------------------------------------------- | ----------------------------------------- | ---------------------------------------------------------------------- |
| `F/query/src/ipc/use-proxy-mode.ts:34-36,55`    | 按 `clashCore === 'clash'` 判断 script    | 读模式列表                                                             |
| `F/nyanpasu/src/hooks/use-proxy-settings.ts:57` | 按 `mihomo` / `mihomo-alpha` 判断透明代理 | 读 `ConfigFeatures` 新增的透明代理支持项（属配置能力，不属控制面能力） |
| `T/core/tray/mod.rs:93,260,461`                 | 按内核枚举判断托盘项                      | 读同一份能力                                                           |

托盘与前端读同一个后端来源，避免再出现两边行为不一致（`2026-10-05-proxies-api-unification` §2 记录过这类缺陷）。

---

## 11. 错误模型

`ApiError` 新增一个变体，其余不变：

```rust
#[error("the running core does not support {operation}")]
Unsupported { operation: ApiOperation },
```

- `ApiOperation` 是工作区内枚举，列出 §4 里可能不被支持的操作。不用字符串（`rust.md` §7.2）。
- `ApiError::Protocol` 目前包 `clash_api::Error`。改为包一个驱动无关的协议错误枚举，两个变体分别带 `clash_api::Error` 与 `sing_box_api::Error` 作为 source，保留完整错误链。
- 经 RPC 出去时保留结构化信息，前端能区分“不支持”与“失败”（`rpc.md`）。

---

## 12. 测试

| 对象             | 方法                                                                                                     |
| ---------------- | -------------------------------------------------------------------------------------------------------- |
| 能力表           | 每个内核种类断言一次完整的 `CoreApiFeatures`，表格化，作为回归基线                                       |
| 连接表折叠       | 纯函数测试：给定事件序列（含 `reset`、乱序关闭），断言每帧后的快照                                       |
| 字段映射         | 纯函数测试：gRPC `Connection`、`Group` → Clash 形状，覆盖域名目标、IPv6、空进程信息                      |
| 规则投影         | 纯函数测试：`route.rules` 样例 → `CoreRule` 列表，覆盖逻辑规则、rule-set 引用、默认出站                  |
| 测速完成判定     | 对假 gRPC 服务：成功、失败（延迟被清除）、超时三条路径；不用 sleep，用服务端可控的推送时机               |
| `sing-box-api`   | 进程内起一个 tonic 假服务实现裁剪后的 `StartedService`；另有一组标记为需要真实二进制的集成测试，默认不跑 |
| `ApiClient` 分派 | 两个驱动各走一遍公开方法，断言不支持的操作返回 `Unsupported` 而不是空结果                                |
| 改名             | 绑定重新生成后 `pnpm typecheck`；两个传输上各调一次改名后的命令，核对查询 / 变更分类与 HTTP 开关没有变化 |
| 前端             | hook 的单元测试覆盖“能力为假时不发请求”；页面测试覆盖两种 `RulesSupport` 的渲染与空模式列表              |
