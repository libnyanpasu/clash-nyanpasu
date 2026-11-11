# sing-box 内核接入 设计

**日期：** 2026-11-11
**状态：** 设计稿，待确认。§8 的假设由 S0 验证后才能定稿。
**基线：** `main @ 93ce63265`；runtime 子模块主仓 pin `3a79dab`，本设计读取的是工作区签出的 `3127f5a`。
**上游基线：** sing-box `v1.14.4`（2026-10-24 发布，仓库默认分支 `testing`）；1.15 处于 alpha。
**来源：** 现有多内核只覆盖 Clash 系。需求四条：① sing-box 首推 gRPC，现有 Clash API 客户端要二次封装以抹平两套 API，不支持的功能在设计阶段探明并给出处理办法，前端 hooks 同步更新；② 评估用信号重载 sing-box（参考 `keiko233/blossom` 的 `server-agent`）；③ profiles 及其他位置加上种类区分，防止混用，短期不做内置转换；④ 内核更新带来的 schema 变动从最近版本开始兼容，并进入 manifest v2，防止旧版应用更新到新内核后无法使用。
**范围：** 把 sing-box 作为第二个内核家族接入所需的全部设计：控制面抽象、种类隔离、进程管理与重载、配置生成、schema 世代与 manifest v2、前端适配。S0–S3 写到 PR 粒度（见 [`tasks.md`](tasks.md)），S4 之后写到里程碑粒度（见 [roadmap](../../roadmap/2026-11-11-sing-box-core.md)）。
**不在范围：** 见 §9。
**权威顺序：** `AGENTS.md` > `docs/development/*` > 本设计 > 子设计 > `tasks.md` > roadmap。本设计与现有设计文档的偏差集中列在 §6。

子设计：

| 文档                                   | 内容                                                         |
| -------------------------------------- | ------------------------------------------------------------ |
| [`core-api.md`](core-api.md)           | 内核控制面抽象：操作矩阵、能力模型、驱动、RPC 与 hooks 迁移  |
| [`config-family.md`](config-family.md) | 种类隔离：内核家族、Profile、转换链、选中关系、受管设定      |
| [`core-manager.md`](core-manager.md)   | runtime 侧：启动、校验、控制器、重载策略、日志、native store |
| [`schema-compat.md`](schema-compat.md) | schema 世代、版本解析、manifest v2、升级预检                 |
| [`tasks.md`](tasks.md)                 | S0–S3 的任务卡                                               |

---

## 0. 结论摘要

1. **sing-box 是第二个内核家族，不是第五个 Clash 变体。** 现有四种内核（Mihomo、clash-rs、Clash Premium、Meow）共用 Clash YAML 与 Clash REST API；sing-box 的配置格式、控制面、重载方式、发布节奏都不同。引入 `CoreFamily { Clash, SingBox }` 作为贯穿配置、Profile、控制面、manifest 的唯一区分轴（§4 D1）。
2. **控制面以 gRPC 为主，不依赖 Clash API。** sing-box 1.14 的 API 服务暴露 `daemon.StartedService`（`SB/daemon/started_service.proto`）。只配置 API 服务就会创建流量管理器（`SB/box.go:243`），连接与流量统计不需要 `experimental.clash_api`。Nyanpasu 自己的控制走 gRPC；Clash API 只作为给外部面板用的可选项保留（§4 D2）。
3. **现有 `ApiClient` 就是二次封装的落点。** 它已经把协议客户端关在适配器内（`C/actor_v2/api.rs:1-2`），对外是 20 个左右的操作。本设计把它改成“一个句柄、两个驱动、一份能力表”，逐项列出两套 API 的差异与处理办法（[`core-api.md`](core-api.md) §4）。gRPC 缺三项：规则列表、自定义测速 URL、解除固定选择；两套 API 都没有 provider。
4. **能力由后端回答，前端不保存内核知识。** 新增 `get_core_api_features` 查询，前端按能力隐藏或禁用功能，不再用内核名判断。这延续 [`core-specific-config.md`](../../design/core-specific-config.md) 结论 6。
5. **首版一律重启，不用 SIGHUP。** sing-box 的 SIGHUP 重载是进程内完整重建（`SB/cmd/sing-box/cmd_run.go:198-222`），连接同样中断，只在 Unix 上可用，且信号送达不证明新配置生效。`blossom/server-agent` 出于同样理由选择替换进程（`server-agent/src/main.rs:352-355`）。SIGHUP 作为 S7 的可选优化保留，前提是用 `GetStartedAt` 做生效验证（[`core-manager.md`](core-manager.md) §6）。
6. **种类隔离靠类型与校验，不靠约定。** Profile 项带 `family` 字段，默认 `clash`，创建后不可变；Config 与它引用的 transform、组合成员必须同家族；选中的 Profile 与选中的内核必须同家族，两者由一个工作流一起切换（[`config-family.md`](config-family.md)）。不做 Clash 与 sing-box 之间的转换。
7. **pipeline 前段共用，尾段按家族分开。** 前段跑在格式无关的 `ConfigValue` 树上。尾段的字段白名单、内置脚本、`include-all` 展开都是 Clash schema，对 sing-box 不执行；sing-box 的受管设定需要“按 tag 增改数组元素”这个新原语。
8. **sing-box 的 schema 按世代管理。** 世代取 `主.次` 版本号。应用声明自己会写的世代集合，受管设定按世代选择写法。起点是 1.14：API 服务从 1.14.0 才有，这是硬下限。这是对 `core-specific-config.md` 结论 1“能力表不看版本”的有意偏离，只对 sing-box 家族生效（§6）。
9. **manifest v2 给每个发行版标世代。** 应用只安装自己支持的世代里最新的版本；上游出了新的次版本，旧应用继续停在旧世代的最新补丁版。v1 清单保持原样继续生成，不加入 sing-box。Clash 系内核也迁到 v2，世代恒为 `1`，为将来的破坏性变更留一个开关（[`schema-compat.md`](schema-compat.md)）。
10. **分八个阶段，前三个互相独立。** S0 验证假设；S1 控制面中性化（只动 Clash，行为不变）；S2 种类隔离的领域模型；S3 runtime 仓的 core-manager。S4 起接线、分发、前端，S7 是 TUN、SIGHUP 等后续项。阶段依赖与完成判据见 [roadmap](../../roadmap/2026-11-11-sing-box-core.md)。

---

## 1. 现状事实

路径缩写：`C/` = `backend/nyanpasu-core/src/`，`Q/` = `backend/nyanpasu-config/src/`，`T/` = `backend/tauri/src/`，`R/` = `backend/nyanpasu-runtime/crates/`，`F/` = `frontend/`，`SB/` = sing-box `v1.14.4` 源码，`BL/` = `keiko233/blossom` `main` 分支的 `server-agent/`。

### 1.1 已有的预留

| 事实                                                                                                             | 位置                                                                                  |
| ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| 线格式 `CoreType::SingBox` 已存在，序列化为 `"singbox"`，有 golden 测试；`get_supported_cores` 里被注释掉        | `R/nyanpasu-utils/src/core/definition.rs:96-97,141`                                   |
| 旧的实例启动路径对 `SingBox` 是 `unimplemented!`                                                                 | `R/nyanpasu-utils/src/core/instance.rs:230`                                           |
| 服务侧 `core_kind()` 对 `SingBox` 直接 `bail!`                                                                   | `R/nyanpasu-service-runtime/src/server/manager_bridge.rs:736`                         |
| `nyanpasu-core-metadata` 有 `CoreKind { Clash(ClashCoreKind), SingBox }`，但 core-manager 用的是 `ClashCoreKind` | `R/nyanpasu-core-metadata/src/kind.rs:6-11`；`R/nyanpasu-core-manager/src/kind.rs:13` |
| core-manager 的配置模块已写明不讲 Clash API 的内核需要自己的模块                                                 | `R/nyanpasu-core-manager/src/config/clash.rs:5-8`                                     |
| 应用侧 `wire_core_type_to_kind` 对 `SingBox` 返回 `None`                                                         | `C/actor_v2/endpoint.rs:775`                                                          |
| `ClashCore` 上有 TODO：支持 sing-box 时移除这个结构                                                              | `Q/application/clash_core.rs:6`                                                       |
| Profile 组合设计把 ConfigTarget、sing-box 支持、跨 target 转换列为非目标，留给后续设计                           | `docs/design/profile-composition-clean-design.md` §3                                  |

### 1.2 Clash 假设所在的位置

| 事实                                                                                                                        | 位置                                                                                                   |
| --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| 应用的内核身份是 `ClashCore`（7 个变体，`#[bitflags] repr(u8)`），持久化在应用配置的 `core` 键，跨 8 个 crate 约 370 处引用 | `Q/application/clash_core.rs:7-39`；`Q/application/mod.rs:143`                                         |
| core-manager 启动参数、`-t` 校验、`SAFE_PATHS` 环境变量按 Clash 系编写                                                      | `R/nyanpasu-core-manager/src/kind.rs:38-80,132-173`                                                    |
| 版本探测固定用 `-v`                                                                                                         | `R/nyanpasu-core-manager/src/capability.rs:95-99`                                                      |
| 配置读入用 YAML 解析成 `Mapping`，输出固定 YAML，运行期文件名写死 `.yaml`                                                   | `R/nyanpasu-core-manager/src/config/mod.rs:72-76`；`config/runtime_store.rs:144`                       |
| 控制器由顶层 `external-controller*` 键解析，类型是 `clash_api::Host`                                                        | `R/nyanpasu-core-manager/src/config/clash.rs:16-55`；`spec.rs:72-76`                                   |
| 健康探测调 Clash `/version`；热重载走 `PUT /configs`                                                                        | `R/nyanpasu-core-manager/src/health/probe.rs:130`；`manager/apply.rs:224-229`                          |
| 优雅切换只对 Mihomo 开放                                                                                                    | `R/nyanpasu-core-manager/src/manager/switching.rs:31`                                                  |
| 受管设定写 Clash 顶层键；字段白名单是 45 个 Clash 键                                                                        | `Q/runtime/executor/plan.rs:33-66`；`Q/runtime/executor/builtin.rs:12-71`                              |
| pipeline 尾段固定执行白名单、受管设定、默认值、内置脚本、`include-all` 展开                                                 | `Q/runtime/executor/mod.rs:319-394`                                                                    |
| Profile 文本用 YAML 解析器读入；最终产物序列化为 YAML                                                                       | `Q/runtime/executor/mod.rs:143-144`；`backend/nyanpasu-profile-transform/src/runtime_builder.rs:57-60` |
| 导入时只认 `proxies` 与 `proxy-providers`                                                                                   | `C/profile_import.rs:142-144`                                                                          |
| `ApiClient` 是实例绑定的 Clash API 能力句柄，返回 `clash_api` 类型                                                          | `C/actor_v2/api.rs:1-2,40-57`                                                                          |
| 节点记录直接使用 `clash_api::Proxy`，一路到前端                                                                             | `docs/spec/2026-10-05-proxies-api-unification/design.md` §1                                            |
| 前端按内核名判断功能：透明代理、代理模式里的 script                                                                         | `F/nyanpasu/src/hooks/use-proxy-settings.ts:57`；`F/query/src/ipc/use-proxy-mode.ts:34-36,55`          |
| 内核更新清单 v1 只有每个内核的一个 `latest` 与文件名模板，结构体字段是写死的七个内核                                        | `manifest/version.json`；`backend/nyanpasu-updater/src/updater/mod.rs:29-60`                           |
| 代码里已有 TODO：manifest v2 应为 kebab-case                                                                                | `backend/nyanpasu-updater/src/updater/mod.rs:37`                                                       |

### 1.3 sing-box v1.14.4

| 事实                                                                                                                                                                    | 位置                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| API 服务是 gRPC 服务端，注册 `StartedService`、gRPC 健康检查与反射；不注册 `ManagedService`，所以经它不能 `ReloadService` 或 `StopService`                              | `SB/daemon/server.go:16-27`；`SB/daemon/attached_service.go:10`                  |
| API 服务只监听 TCP，以 h2c 提供 gRPC，同时接受 gRPC-Web；鉴权是 `authorization: Bearer <secret>` 元数据                                                                 | `SB/service/api/server.go:46,87,105`；`SB/daemon/server.go:49-66`                |
| `GetVersion` 返回版本字符串与 `apiVersion`；1.14.4 的 `APIVersion` 是 4                                                                                                 | `SB/daemon/started_service.proto:57-60`；`SB/daemon/started_service.go:37`       |
| `StartedService` 提供：服务状态、日志、状态（内存、上下行速率与总量、连接数）、分组、全部出站、Clash 模式读写与订阅、测速、选择出站、连接事件与关闭、弃用警告、启动时间 | `SB/daemon/started_service.proto:8-31`                                           |
| 没有规则列表、provider、DNS 查询的 RPC                                                                                                                                  | 同上                                                                             |
| `URLTest` 只收出站 tag，立即返回空；结果写入测速历史，失败时删除历史项；不能指定 URL 与超时                                                                             | `SB/daemon/started_service.go:717-755`                                           |
| 只要配置了 Clash API 或 API 服务，就创建流量管理器；`SubscribeConnections` 在没有流量管理器时返回 `Unimplemented`                                                       | `SB/box.go:152-164,243-244`；`SB/daemon/started_service.go:803-806`              |
| Clash API 只监听 TCP                                                                                                                                                    | `SB/experimental/clashapi/server.go:162`                                         |
| Clash API 的 `PUT /configs` 是空操作，`PATCH /configs` 只处理 `mode`                                                                                                    | `SB/experimental/clashapi/configs.go:53-71`                                      |
| Clash API 的 `/version` 返回 `{"version":"sing-box <ver>","premium":true,"meta":true}`                                                                                  | `SB/experimental/clashapi/server.go:364`                                         |
| Clash API 有 `/proxies`、`/group`、`/rules`、`/connections`、`/logs`、`/traffic`、`/memory`、`/dns`、`/cache`；`/providers/*` 是空桩                                    | `SB/experimental/clashapi/server.go:116-129`；`api_meta.go:31-33`；`provider.go` |
| 官方默认构建标签包含 `with_clash_api`；API 服务没有对应的构建标签桩文件                                                                                                 | `SB/release/DEFAULT_BUILD_TAGS`；`SB/include/`                                   |
| `run` 监听 SIGHUP：先 `check()`，失败只记日志并保留旧实例；通过则关闭实例、重读配置、重新创建；重新创建失败则进程退出                                                   | `SB/cmd/sing-box/cmd_run.go:186-222`                                             |
| 发行包命名 `sing-box-<ver>-<os>-<arch>.tar.gz`（Windows 为 `.zip`）；Linux amd64 另有 `-glibc`、`-musl` 变体                                                            | GitHub release `v1.14.4` 资产列表                                                |
| 弃用表显示每个次版本都有移除项：1.13 移除旧 inbound 字段，1.14 移除旧 DNS 服务器格式，1.15 弃用 TUN `stack`（计划 1.17 移除）                                           | <https://sing-box.sagernet.org/deprecated/>                                      |

### 1.4 blossom server-agent 的先例

| 事实                                                                                                         | 位置                        |
| ------------------------------------------------------------------------------------------------------------ | --------------------------- |
| 启动用 `run -c <config>`，校验用 `check -c <config>`，15 秒超时，错误输出去 ANSI 后截断到 4096 字符          | `BL/src/process.rs:141-177` |
| 版本取 `sing-box version` 输出里以 `sing-box version ` 开头的那一行                                          | `BL/src/process.rs:179-194` |
| 控制台日志带 ANSI 颜色，形如 `+0800 19:49:27 INFO v2ray-api: grpc server started`                            | `BL/src/process.rs:393-400` |
| 应用新配置时替换进程而不发 SIGHUP，理由写在注释里：信号成功只证明送达，sing-box 可能拒绝新文件并继续跑旧配置 | `BL/src/main.rs:352-355`    |
| 应用流程是 预检 → 提升候选 → 替换进程 → 观察期 → 提交；任何一步失败回滚到上一份已知可用配置                  | `BL/src/main.rs:313-420`    |
| gRPC 客户端用 tonic 的底层 `Grpc` 加手写 prost 消息，不用 `.proto` 与 tonic-build                            | `BL/src/stats.rs:1-4`       |

---

## 2. 术语

| 术语        | 含义                                                                                                             |
| ----------- | ---------------------------------------------------------------------------------------------------------------- |
| 内核家族    | `CoreFamily`：`Clash` 或 `SingBox`。决定配置格式、控制协议、重载方式。持久化与线格式为 `clash`、`sing-box`       |
| 内核种类    | core-manager 的 `CoreKind`：Mihomo、ClashRust、ClashPremium、Meow、SingBox。决定启动参数、日志布局、配置变更分类 |
| 内核身份    | 应用配置里选中的内核，现名 `ClashCore`，含通道（如 `mihomo-alpha`）。S2 改名，工作名 `CoreId`                    |
| 控制面      | 应用用来读状态、发指令的内核 API：Clash REST/WebSocket 或 sing-box gRPC                                          |
| 驱动        | `ApiClient` 内部针对一种控制面的实现                                                                             |
| schema 世代 | 一段互相兼容的内核配置 schema。sing-box 取 `主.次`（如 `1.14`）；Clash 家族恒为 `1`                              |
| 受管设定    | Nyanpasu 在 pipeline 尾段强制写入、移除或补默认值的运行配置键                                                    |

类型名是工作名，以创建它的 PR 为准，与 `backend-packages.md` 对计划包名的处理一致。

---

## 3. 总体结构

```text
前端页面 / 托盘
    │  按 CoreApiFeatures 与 ConfigFeatures 决定显示什么
    ▼
@nyanpasu/query hooks ── @nyanpasu/rpc
    ▼
RPC 命令（薄适配）
    ▼
NyanpasuClient
    ├─ 状态：应用配置（内核身份） / Profiles（family、current） / 内核设定
    ├─ runtime build：前段共用 ── 尾段 Clash | 尾段 sing-box（按世代）
    │       产物：YAML | JSON
    ├─ CoreClient ──► core-manager（本地）或服务（IPC）
    │       CoreKind 决定：参数、校验、控制器解析、重载、日志
    └─ ApiClient（实例绑定、可吊销）
            ├─ Clash 驱动   ── clash-api crate ── REST / WebSocket
            └─ sing-box 驱动 ── sing-box-api crate ── gRPC（h2c，回环 TCP）

更新：manifest v2（世代标注）──► updater 按应用支持的世代选版本 ──► 升级预检 ──► 替换二进制
```

依赖方向不变：core 拥有端口与用例，实现放在 runtime 仓的协议 crate 或实现 crate 里，Tauri 只做组合与适配。新增的 `sing-box-api` 与现有 `clash-api` 同级，都在 runtime 仓。

---

## 4. 关键决策

### D1. `CoreFamily` 是唯一的区分轴

所有“这是 Clash 还是 sing-box”的判断都落到一个枚举上，由内核身份派生（`core.family()`）。Clash 专属代码对它做无通配的穷尽 `match`，新增家族时在编译期逐处暴露（`rust.md` §5.5）。

否决的做法：

| 方案                                          | 否决理由                                                                        |
| --------------------------------------------- | ------------------------------------------------------------------------------- |
| 把 sing-box 当作 `ClashCore` 的又一个变体处理 | 配置格式与控制面都不同，会让每个按内核名的分支再多一格，且无法表达 Profile 归属 |
| 按配置文件格式（YAML / JSON）区分             | 格式是家族的结果，不是原因；JSON 也是合法 YAML，无法据此拒绝混用                |
| 运行时探测内核类型                            | `/version` 在 sing-box 的 Clash API 上自称 `meta: true`（§1.3），探测会误判     |

### D2. sing-box 的控制面用 gRPC，Clash API 只留给外部面板

- Nyanpasu 写入一个 `type: "api"` 的服务，监听回环地址与应用分配的端口，带密钥，关闭内置面板。
- Nyanpasu 不为自己的功能读取 sing-box 的 Clash API。
- 用户在设定里开启外部控制器时，才写入 `experimental.clash_api`，供 yacd、metacubexd 这类面板使用。这是 S7 的项。
- 代价：gRPC 缺规则列表、自定义测速 URL。处理办法见 [`core-api.md`](core-api.md) §4。

否决“sing-box 也走 Clash API”：它能最快跑通，但 sing-box 官方客户端与新面板都走 gRPC，Clash API 在上游处于 `experimental` 下且 provider 是空桩；把它当主控制面等于把最不受维护的接口当依赖。

### D3. `ApiClient` 一个句柄、两个驱动

`ApiClient` 保持“实例绑定、可吊销、协议客户端不外泄”的现有约定，内部改为 `enum ApiDriver { Clash(..), SingBox(..) }`。每个操作是一次穷尽 `match`。不用 `dyn` trait：实现只有两个且都在工作区内，穷尽匹配能保证新增操作时两边都被处理，也避免给流式返回值套 `async_trait`。

### D4. 记录层沿用 Clash 形状，中性化的是操作、能力与命名

节点记录继续用 `clash_api::Proxy`（`2026-10-05-proxies-api-unification` 决策 1）。sing-box 驱动把 gRPC 的 `Group` / `GroupItem` 映射成这个形状，缺的字段留空。理由：

- 那份 spec 刚删掉应用侧的有损重复定义；再造一套中性记录等于把重复加回来。
- sing-box 自己的 Clash API 服务端做的就是这个映射，说明形状够用。
- 前端代理页的类型不需要动。

需要中性化的是带 `clash` 字样的命令名、hook 名、事件名，以及“某功能是否可用”的判断来源。

### D5. 能力表由后端提供

`CoreApiFeatures` 描述当前运行实例的控制面能力，随内核状态事件刷新。前端的 Providers 页、规则列表的来源说明、解除固定、自定义测速 URL 都读它；模式列表是运行期数据，另由模式状态查询返回。`ConfigFeatures` 继续回答“内核读哪些配置键”，两张表职责不同，不合并。

### D6. 首版重载等于重启

sing-box 的所有配置变更归类为 `Switch`（重启），只有模式切换走 `SetClashMode`。理由与证据见 [`core-manager.md`](core-manager.md) §6。

### D7. Profile 带 `family`，默认 `clash`，不可变

见 [`config-family.md`](config-family.md) §3。不可变是因为改家族等于换了一份内容完全不同的 Profile；允许修改只会制造“字段是 sing-box、内容是 Clash”的状态。

### D8. 选中的 Profile 与选中的内核必须同家族

不变量：`current` 为空，或 `family(current) == family(core)`。跨家族切换由一个工作流同时提交内核与 Profile，并记住每个家族上次选的内核与 Profile。见 [`config-family.md`](config-family.md) §5。这一条需要确认，备选方案也列在那里。

### D9. sing-box 的受管设定按世代写

受管设定写入器以 `SingBoxGeneration` 为参数，对（世代, 字段）做穷尽 `match`。世代由调用方在构建前解析，方式与端口解析相同。见 [`schema-compat.md`](schema-compat.md) §3。

### D10. manifest v2 与 v1 并存

v2 是新文件、新结构，v1 文件与生成逻辑保持不变并且永不加入 sing-box。应用在同一个版本里把全部内核的更新来源切到 v2，不做双读回退。见 [`schema-compat.md`](schema-compat.md) §4。

### D11. 换二进制前先预检

替换内核二进制之前，用新二进制对“按新世代重新构建的当前配置”跑一次校验。失败则不替换，保留旧二进制并报告结构化错误。见 [`schema-compat.md`](schema-compat.md) §6。

### D12. 服务与应用的版本错位要显式处理

服务（守护进程）独立安装与升级。旧服务对 `SingBox` 直接报错（§1.1）。应用在以服务模式启动 sing-box 前检查服务是否支持该内核种类，不支持时报告“需要升级服务”，不回退到本地内核。检查依据是服务状态里新增的受支持内核种类列表；缺这个字段的旧服务按“只支持 Clash 家族”处理。

---

## 5. 分阶段概览

完整的依赖、完成判据与风险见 [roadmap](../../roadmap/2026-11-11-sing-box-core.md)。

| 阶段 | 内容                                                         | 用户可见       | 依赖                |
| ---- | ------------------------------------------------------------ | -------------- | ------------------- |
| S0   | 用真实二进制验证 §8 的假设，修订本设计                       | 否             | —                   |
| S1   | 控制面中性化：能力表、命令与 hooks 改名、前端按能力判断      | 否（行为不变） | S0                  |
| S2   | 种类隔离：内核身份改名、`CoreFamily`、Profile `family`、校验 | 否             | S0                  |
| S3   | runtime 仓：core-manager 支持 sing-box、`sing-box-api` crate | 否             | S0                  |
| S4   | 配置生成与生命周期接线、sing-box 驱动                        | 否             | S1、S2、S3          |
| S5   | manifest v2、updater、打包、升级预检                         | 部分           | S5a 无；S5b 依赖 S4 |
| S6   | 前端：内核选择、Profile 家族、导入、编辑器、能力门控         | 是，预发布通道 | S4、S5              |
| S7   | TUN、SIGHUP 重载、外部 Clash 面板、透明代理、下一世代        | 是             | S6                  |

---

## 6. 与现有设计的偏差

| 现有结论                                                                               | 偏差                                                                                                  | 处理                                                            |
| -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `core-specific-config.md` 结论 1：能力表只按内核类型判断，不看版本                     | sing-box 家族的受管设定按 schema 世代选择写法                                                         | 对 Clash 家族不变。落地 S4 时在该文档加一节说明 sing-box 的例外 |
| `core-specific-config.md` §2 否决“在构建配置前确定二进制并探测版本”                    | sing-box 需要在构建前知道世代                                                                         | 世代由调用方解析后作为输入传入，构建器仍是纯函数，不做 IO       |
| `profile-composition-clean-design.md` 第 8 条：`Profiles.current` 只保存单个 ProfileId | 不变。新增的是“每个家族上次选中项”的记忆，不是第二个 current                                          | S2 在该文档补充不变量与记忆字段                                 |
| `profile-composition-clean-design.md` §3 把 ConfigTarget、sing-box 支持列为非目标      | 本设计就是那份“后续设计”                                                                              | —                                                               |
| `2026-10-05-proxies-api-unification` 决策 3：组能力按字段存在性推断，不读内核类型      | 不变。sing-box 驱动产出的记录同样按字段存在性推断；`CoreApiFeatures` 回答的是操作是否存在，不是组能力 | —                                                               |
| `linux-transparent-proxy.md` 的兼容表不含 sing-box                                     | sing-box 列为 Deferred                                                                                | S7 再评估；首版在该表加一行                                     |

---

## 7. 风险

| 风险                                                                 | 影响                                   | 缓解                                                                                                |
| -------------------------------------------------------------------- | -------------------------------------- | --------------------------------------------------------------------------------------------------- |
| sing-box 每个次版本都移除配置项（§1.3）                              | 新内核拒绝 Nyanpasu 写的片段，无法启动 | 世代机制加 manifest v2（D9、D10）；升级预检（D11）                                                  |
| gRPC `apiVersion` 随上游演进                                         | 驱动与内核对不上                       | 驱动在绑定时核对 `apiVersion`，不在支持范围内报 `Unsupported`；世代门控让应用不会自动装到未知次版本 |
| 没有热重载，每次改设定都重启                                         | 连接中断、TUN 闪断，体验不如 Mihomo    | 首版接受；S7 评估 SIGHUP；UI 明示“应用设定会重启内核”                                               |
| 用户的 sing-box Profile 自身用了已移除的字段                         | 升级内核后启动失败                     | 升级预检拦截；`GetDeprecatedWarnings` 在 UI 提示即将移除的字段                                      |
| API 服务只监听 TCP，没有本地套接字                                   | 同机其他用户可以连回环端口             | 每次启动生成密钥；与 Clash Premium、Meow 的 HTTP 回落同等级；在 UI 的控制通道说明里写明             |
| 订阅生态以 Clash YAML 为主                                           | 初期只有自备 sing-box 配置的用户能用   | 明确写进发布说明；不做转换（需求 ③）                                                                |
| 内核身份改名触及约 370 处，并与 core 包拆分的 B–E 阶段争用同一批文件 | 冲突与评审成本                         | 改名是独立的机械 PR，选在两个拆分阶段之间落地；S2 开始前与拆分排期对齐                              |
| 服务与应用版本错位（D12）                                            | 服务模式下 sing-box 起不来             | 显式能力检查与升级提示                                                                              |
| `sing-box-api` 需要 tonic/prost，工作区目前没有这两个依赖            | 构建时间与依赖面增加                   | 只放在 runtime 仓的协议 crate 里；裁剪 proto 到用到的 RPC；提交生成代码，构建不需要 `protoc`        |
| sing-box 许可证为 GPL-3.0-or-later，另有不得暗示关联的命名条款       | 分发与命名合规                         | 与本项目 GPL-3.0 兼容；内核以原名、原样二进制分发，不改名、不暗示官方关联；进入 credits             |

---

## 8. 待验证假设

以下结论来自阅读 `v1.14.4` 源码与文档，没有用二进制跑过。S0 逐条验证并把结果写进 `docs/probe/sing-box/`，与结论不符的在定稿前修订本设计。

| #   | 假设                                                                                                               | 影响的设计                         |
| --- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------- |
| V1  | `sing-box version` 首行是 `sing-box version <semver>`，退出码 0                                                    | 版本解析、世代解析                 |
| V2  | `sing-box check -c <file> -D <dir>` 成功退出 0；失败非 0 且错误在 stderr                                           | core-manager 校验                  |
| V3  | 配置里的未知字段使 `check` 失败                                                                                    | 家族隔离、受管设定                 |
| V4  | sing-box 是否接受带 `//` 注释或尾随逗号的 JSON                                                                     | 导入时是否需要正规化               |
| V5  | tonic 能以 h2c 连接回环上的 API 服务，带 Bearer 密钥调通 `GetVersion` 与 `grpc.health.v1.Health/Check`             | 驱动传输、健康探测                 |
| V6  | `SubscribeGroups` 与 `SubscribeOutbounds` 订阅后先推一份完整状态，之后在选择变化与测速结果变化时推送               | 代理快照、测速结果回传             |
| V7  | `URLTest` 失败时订阅方能观察到该出站的延迟被清除，从而区分“失败”与“未完成”                                         | 测速的完成判定                     |
| V8  | `SubscribeConnections` 首帧 `reset = true` 并带全量；之后是增量事件；`uplink` / `downlink` 是速率，`*Total` 是累计 | 连接流归一、流量记账               |
| V9  | `SubscribeStatus` 的 `uplink` / `downlink` 是每个间隔的速率；只配 API 服务时 `trafficAvailable = true`             | 流量与内存流                       |
| V10 | `SubscribeLog` 的 `reset` 语义、是否带时间戳、是否受配置的 `log.level` 限制                                        | 日志流、级别过滤                   |
| V11 | 控制台日志格式与 `log.disable_color`、`log.timestamp` 的效果                                                       | 日志解析器                         |
| V12 | Windows 上 `Ctrl+Break` / 控制台关闭事件能让 `run` 正常退出                                                        | 停止策略                           |
| V13 | 同一工作目录下 `experimental.cache_file` 的默认路径与锁行为                                                        | native store                       |
| V14 | API 服务在 TUN 与非特权模式下都能启动；`dashboard: false` 时不下载面板                                             | 受管设定                           |
| V15 | 主流订阅服务对 `User-Agent` 含 `sing-box` 的请求返回 sing-box JSON                                                 | 订阅默认 UA                        |
| V16 | 官方发行包里的二进制路径是 `sing-box-<ver>-<os>-<arch>/sing-box`                                                   | manifest v2 的 `binary` 字段、解包 |

---

## 9. 不在范围

- Clash 配置与 sing-box 配置之间的任何转换，包括节点、分组、规则。
- 跨家族的通用配置 IR。
- sing-box 的 Tailscale、OpenVPN、OpenConnect、USB/IP、网络质量测试、STUN 测试等 gRPC 能力。
- sing-box 的 alpha / beta 通道。
- 移动端与 `libbox`。
- OpenWrt 上的 sing-box。
- 为用户自行替换的内核二进制负责：不在支持世代内的版本直接拒绝构建。
- sing-box 的 Linux 透明代理、macOS DNS 接管（S7 再评估）。

---

## 10. 涉及的规范条款

实现时逐条对照，评审按此检查。

| 条款                                                                                      | 落点                                                                                   |
| ----------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `architecture.md`：不加全局单例；依赖显式注入；`NyanpasuClient` 是 facade                 | 驱动由 `CoreClient` 构造并持有；世代解析走注入的 `CoreVersionReader`                   |
| `architecture.md`：纯服务不做 IO；先提交状态再触发副作用；Required 参与者在提交前投票     | 受管设定写入器是纯函数；家族切换工作流里 runtime 仍是提交前的投票方                    |
| `backend-packages.md`：core 不依赖实现 crate；跨 RPC 类型放在拥有该概念的中性 crate       | `CoreFamily` 放 `nyanpasu-config`；`CoreApiFeatures` 放 core；协议 crate 在 runtime 仓 |
| `rpc.md`：命令是薄适配；查询与变更按副作用分类；重新生成绑定；HTTP 显式开启               | 改名后的命令保持原分类与 HTTP 开关；每个改名命令在两个传输上验证                       |
| `rust.md` §5.2：协议与有限状态用类型化枚举                                                | `CoreFamily`、`SingBoxGeneration`、`RulesSupport`                                      |
| `rust.md` §5.3：端口归消费者所有；只有多个真实实现才定义 trait                            | 驱动用枚举分派，不新增 trait（D3）                                                     |
| `rust.md` §5.5：工作区内枚举保持穷尽；镜像外部协议的类型才用 `#[non_exhaustive]`          | gRPC 生成的枚举按外部协议处理，通配分支给保守行为                                      |
| `rust.md` §6：持久化字段要能解旧文件；序列化名是契约                                      | `family` 用 `#[serde(default)]`；`core` 键的旧取值全部保留                             |
| `rust.md` §7.2：类型化错误，不用字符串桶                                                  | `ApiError::Unsupported { operation }`、`RuntimeBuildError` 的新变体带结构化字段        |
| `testing.md`：纯服务用值测试；actor 经类型化客户端加假适配器；不 sleep                    | 受管设定 golden 测试；驱动对假 gRPC 服务测试；fake-core 增加 sing-box 参数形状         |
| `typescript.md`、`frontend-packages.md`：前端经 `@nyanpasu/rpc` 与 `@nyanpasu/query` 调用 | hooks 改名在 `query` 包内完成，页面不直接读内核名                                      |
| `workflow.md`：一个提交做一件事；改名与行为变更分开                                       | 改名 PR 不带行为变更（[`tasks.md`](tasks.md)）                                         |
