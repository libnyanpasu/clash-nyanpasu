# 代理 API 结构统一：clash-api 记录层与组语义层

**日期：** 2026-10-05

**状态：** 设计已确认，待实施计划。

**基线：** `f3461ae8e649a40b0c85033161ed56d5d09e78ad`；运行时子模块 `f523c77c031c8a36ee699c894d6a095c721e2931`，clash-api `1.0.0-rc.10`。

**分支：** `refactor/proxies-clash-api-records`（当前 checkout）。

**需求：** 代理、provider 与测速结果统一使用 clash-api 的类型，删除应用侧的有损重复定义；在后端纯函数中生成组语义层（组类型、规范化后的固定选择、可选与可解除固定能力），使托盘与前端读取同一份语义；顺带修复托盘 Global 模式无法选择节点、托盘显示 hidden 组两个问题。

**依据：** 代理页组类型横向调研（2026-10-05，仓库外文档，含 Clash-rs 0.10.8 与 Meow v0.21.2 的接口实测对照）、[代理组内核接口核查报告](../../audit/2026-10-05-proxy-core-api-report.md)、[代理组发现 spec](../2026-10-05-proxy-group-discovery/design.md)、mihomo `hub/route/proxies.go`（`findProxyByName` 与 `unfixedProxy`）。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 后续实施计划。

## 1. 决策与范围

1. 节点记录直接使用 `clash_api::Proxy`，包括 mihomo 原样的 kebab-case 键（`provider-name`、`dialer-proxy`、`routing-mark`）。不新增 camelCase 镜像类型。
2. 组语义层是应用侧类型 `ProxyGroup`，只放派生或规范化后的字段；原始组记录仍在 `nodes[组名]` 中。
3. 组能力按字段存在性推断，不读取内核类型，不依赖 `/version` 的 `meta`。
4. 统一范围：`getProxies`、`mutateProxies`、provider 列表、节点测速、组测速的返回类型。RPC 入参（`group`、`name`、`provider`、`url` 的 `String`）不变。rules、config、version 等非代理 DTO 不在本次范围。
5. 本次不修改 clash-api crate。组类型枚举与能力推断放在应用侧纯函数。
6. hidden 不影响组的存在性，`Proxies.groups` 继续包含 hidden 组；过滤由托盘投影与前端 hook 两个展示层各自完成（沿用代理组发现 spec 的约束）。
7. 采用可迁移的破坏性变更：同一提交内迁移全部消费方并重新生成 bindings，不保留兼容层。

不在本次范围（留给 Phase B/C）：解除固定 RPC、前端按能力禁用节点、fixed 徽标、测速 URL 与超时参数化、`extra` 按 URL 读取延迟、载荷裁剪、托盘菜单项按句柄查找、托盘显示 fixed 标记或"恢复自动选择"。

## 2. 现状与问题

同一份数据经过四次转换：`clash_api::Proxy` → `core::clash::api::ProxyItem`（丢弃 `fixed`、`testUrl`、`expectedStatus`、`extra`、`mptcp`、`smux`、`dialer-proxy`、`uot` 等，`history.time` 转字符串）→ `core::clash::proxies::{Proxies, ProxyGroupItem}` → 托盘 `TrayProxyItem { current, all, type: String }`。provider 侧另有 `ProxyProviderItem`、`VehicleType`、`ProviderType`、`SubscriptionInfo` 的重复定义，其中订阅数值从 `i64` 转为 `usize`，任一 provider 报告负值时整次刷新失败。

托盘的两个缺陷（`backend/tauri/src/core/tray/proxies.rs`）：

- Global 模式下组键写为小写 `"global"`（`to_tray_proxies`），点击节点会调用 `select_proxy("global", …)`，即 `PUT /proxies/global`。mihomo `findProxyByName` 对 `tunnel.Proxies()` 做区分大小写的 map 查找，GLOBAL 只以 `"GLOBAL"` 登记，因此返回 404。同处把组类型写死为 `"Selector"`。
- 托盘不过滤 hidden 组；前端 `use-clash-proxies.ts` 过滤了，两边行为不一致。

## 3. 数据模型

全部定义在 `backend/tauri/src/core/clash/proxies.rs`，由 `Proxies::from_responses` 这个纯函数生成。

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    pub global: Option<ProxyGroup>,
    pub groups: Vec<ProxyGroup>,
    pub nodes: IndexMap<ProxyName, clash_api::Proxy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    pub name: ProxyName,
    #[serde(rename = "type")]
    pub kind: ProxyGroupKind,
    pub all: Vec<ProxyName>,
    pub now: Option<ProxyName>,
    pub fixed: Option<ProxyName>,
    pub hidden: bool,
    pub icon: Option<String>,
    pub capabilities: ProxyGroupCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroupCapabilities {
    pub select: bool,
    pub clear_fixed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum ProxyGroupKind {
    Selector,
    #[serde(rename = "URLTest")]
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Smart,
    #[serde(untagged)]
    Unknown(String),
}
```

TS 类型名以生成结果为准（是否带 `_Serialize` 后缀取决于 specta 的阶段拆分）。`ProxyGroupKind`、`ProxyGroupCapabilities` 带 `Proxy` 前缀，避免在共享的 bindings 命名空间中歧义。

### 3.1 组字段的来源

| 字段     | 规则                                                                         |
| -------- | ---------------------------------------------------------------------------- |
| `name`   | 组记录的 `name`                                                              |
| `kind`   | 由记录的 `type` 字符串解析；不认识的值保留在 `Unknown`，序列化后仍是原字符串 |
| `all`    | 记录的 `all`，缺失时为空列表                                                 |
| `now`    | 记录的 `now` 原样保留                                                        |
| `fixed`  | 记录的 `fixed` 为 `Some("")` 或 `None` 时为 `None`，否则为该名称             |
| `hidden` | 记录的 `hidden`，缺失时为 `false`                                            |
| `icon`   | 记录的 `icon` 原样保留                                                       |

### 3.2 能力推断

令 `fixed_field = record.fixed.is_some()`，空串也算存在：

| `kind`                | `select`      | `clear_fixed` |
| --------------------- | ------------- | ------------- |
| `Selector`            | `true`        | `false`       |
| `UrlTest`、`Fallback` | `fixed_field` | `fixed_field` |
| 其余（含 `Unknown`）  | `false`       | `false`       |

依据：mihomo 与 Meow 在 URLTest/Fallback 上恒输出 `fixed`（未固定时为空串），并接受 `PUT` 与 `DELETE /proxies/{name}`；Clash-rs 0.10.8 不输出 `fixed`，对这两类的 `PUT` 返回 404，也不支持 `DELETE`。Clash-rs master 的 URLTest 可锁定但不暴露状态，本规则保守地判为不可选。

### 3.3 节点规范化

- `/proxies`（以及替换它的 `/group` 列表）中的记录：`provider` 取非空的 `provider`，否则取非空的 `provider-name`，否则为 `None`。
- vehicle 为 HTTP、File、Inline 的 provider 自带节点：`provider` 设为 provider 的 key。
- 组成员在 `/proxies` 与 provider 中都找不到时，仍生成占位记录：`type: "Unknown"`、`udp: false`、`history: []`，其余字段为空。
- `nodes` 仍包含全部 `/proxies` 记录（含组记录本身）；同一节点只存一份。
- 组集合、GLOBAL 提取、排序规则不变，沿用代理组发现 spec 第 5 节。

### 3.4 provider 与测速

| RPC                               | 原返回                              | 新返回                                                       |
| --------------------------------- | ----------------------------------- | ------------------------------------------------------------ |
| `clash_api_get_providers_proxies` | `ProvidersProxiesRes { providers }` | `IndexMap<ProviderName, clash_api::ProxyProvider>`，去掉外壳 |
| `clash_api_get_proxy_delay`       | `DelayRes { delay: u64 }`           | `clash_api::Delay { delay: u16 }`                            |
| `clash_api_get_group_delay`       | `IndexMap<String, u32>`             | `IndexMap<ProxyName, u16>`                                   |

`ProxiesClient::providers()` 与 actor 的 `Snapshot.providers` 改为 `IndexMap<ProviderName, clash_api::ProxyProvider>`，provider 原始记录不做规范化。删除 `core/clash/api.rs` 中的 `ProxiesRes`、`ProxyItem`、`ProxyItemHistory`、`ProxyProviderItem`、`ProvidersProxiesRes`、`VehicleType`、`ProviderType`、`SubscriptionInfo`、`DelayRes` 及对应转换函数。快照指纹（序列化 `(proxies, providers)` 的字节）、3 s 缓存、10 s 刷新与 actor 消息不变。

## 4. 托盘

托盘投影改为：

```rust
pub(super) struct TrayGroup {
    pub(super) now: Option<ProxyName>,
    pub(super) all: Vec<ProxyName>,
    pub(super) selectable: bool,
}
pub(super) type TrayProxies = IndexMap<ProxyName, TrayGroup>; // key 为组的真实名称
```

`to_tray_proxies(mode, &Proxies)`：Direct 模式为空；Global 模式为 `global`（存在时）加全部组；Rule/Script 模式为全部组。`groups` 中的 `hidden` 组一律跳过；`global` 不受 hidden 影响，与前端 Global 模式的行为一致。

- 子菜单标题与 key 相同。Global 模式下标题由 "global" 变为 "GLOBAL"，这是局部更新按菜单文字查找子菜单所要求的。
- `diff_proxies` 在组集合、成员或 `selectable` 变化时返回 `Full`，仅 `now` 变化时返回 `Part`，`now` 出现或消失时返回 `Full`。
- 点击节点时以 key（真实组名）调用 `select_proxy`。
- `selectable` 在 C4 中按 `matches!(kind, Selector | Fallback)` 计算以保持原有行为；C5 改为 `capabilities.select`。C5 的可见变化：mihomo/Meow 上 URLTest 组的节点可点，Clash-rs 上 Fallback 组的节点置灰。

## 5. 前端迁移

- `frontend/query/src/ipc/use-clash-proxies.ts`：`ClashProxiesQueryProxyItem`、`ClashProxiesQueryGroupItem`、`ClashProxiesQuery` 改为新生成类型的别名；`withDelaySample` 使用 `DelayHistory`；hidden 过滤保留。
- `proxies.global` 可能为 `null`。主页面与 WebView 托盘菜单的 `currentGroup` 计算需写成 `proxies?.global ?? undefined`；没有 GLOBAL 时 Global 模式显示为空。
- WebView 托盘菜单首页（`(tray-menu)/tray-menu/proxies/index.tsx`）把 `group.history` 改为读 `proxies.nodes[group.name]?.history`，结果与现在等价。
- `frontend/query/src/ipc/use-clash-proxies-provider.ts`：适配去掉外壳的 map。crate 在 `subscriptionInfo`、`updatedAt` 缺失时序列化为 `null`（旧 DTO 省略该键），且订阅字段键名为 PascalCase。`use-proxies-subscription.tsx` 的判断由 `!== undefined` 改为 `!= null`，否则 `null.download` 会抛错。
- `frontend/query/src/ipc/index.ts` 的 `ClashProviderProxies` 重新导出改为 provider 的新类型。
- 同步更新 `frontend/nyanpasu/perf/fixtures/proxies.ts`、`frontend/query/tests/`、`frontend/nyanpasu/tests/` 中的 mock 结构。
- 不新增 UI 行为。

## 6. 提交序列

| #   | 提交                                                                      | 内容                                                                                                           |
| --- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| C1  | `fix(tray): select the GLOBAL group by its real name`                     | 托盘 Global 模式以真实组名为 key，类型取自记录                                                                 |
| C2  | `fix(tray): leave hidden groups out of the tray menu`                     | 托盘投影跳过普通组中的 hidden 组                                                                               |
| C3  | `refactor(proxies): return provider and delay results as clash-api types` | 第 3.4 节；`from_responses` 改为接收 crate provider，provider 节点暂经现有 `proxy_item` 转换                   |
| C4  | `refactor(proxies): build the proxy view from clash-api records`          | 第 3.1–3.3 节、第 4 节（行为不变的 `selectable`）、第 5 节；删除 `ProxyItem`、`ProxyGroupItem` 与 `proxy_item` |
| C5  | `fix(tray): enable node selection only where the core accepts it`         | 托盘 `selectable = capabilities.select`                                                                        |

每个提交独立可构建并通过检查。改变 RPC 契约的提交（C3、C4）在同一提交内重新生成 bindings 并迁移全部前端消费方。C1、C2 不依赖重构，可单独成 PR。

## 7. 测试与验收

### 7.1 Rust

- `core/clash/proxies.rs`：现有组装测试迁移到 crate 记录；新增：
  - 能力矩阵：`Selector`、`UrlTest`、`Fallback` 各配 `fixed` 缺失、`""`、有值三种记录，以及 `LoadBalance`、`Unknown`；
  - `fixed`、`hidden`、`kind` 的规范化；
  - `provider` 的三种来源；
  - `global` 为 `None`。
- `core/proxies.rs`：actor 测试迁移到新类型，provider 元数据断言改为 crate 字段。
- `core/tray/proxies.rs`：Global 模式 key 为 `"GLOBAL"`；hidden 组被跳过；`selectable` 变化返回 `Full`；C5 后按能力门控（URLTest 带 `fixed` 可选、不带不可选）。
- `client/clash_api.rs`：测速返回 crate 类型。
- `specta_export::tests::export_typescript_bindings` 重新生成 bindings，CI 以 `git diff --exit-code` 检查其是否最新。

### 7.2 检查命令

`pnpm lint:clippy`、`pnpm lint:rustfmt`、tauri crate 的相关 `cargo test`、`pnpm typecheck`、`pnpm test:frontend`、`pnpm lint`、`deno task lint:architecture-ledger`。

### 7.3 验收标准

| 场景                     | 期望                                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| 托盘 Global 模式点击节点 | 调用 `select_proxy("GLOBAL", …)`                                                                                                             |
| 托盘任意模式             | 普通组中的 hidden 组不出现，与前端代理页一致                                                                                                 |
| `getProxies` 节点        | 携带 crate 的全部字段，包括 `fixed`、`testUrl`、`expectedStatus`、`extra`                                                                    |
| 组能力                   | 符合第 3.2 节的表                                                                                                                            |
| provider 页与小组件      | 订阅缺失时不抛错，订阅存在时用量与进度不变                                                                                                   |
| 节点与组测速             | 前端行为不变                                                                                                                                 |
| bindings                 | 不再导出 `ProxyItem`、`ProxyGroupItem`、`ProxyProviderItem`、`DelayRes`；`VehicleType`、`ProviderType`、`SubscriptionInfo` 只有 crate 的定义 |
| IPC 与 HTTP 两种传输     | `getProxies`、provider、测速的查询/变更分类与 HTTP 能力不变                                                                                  |

### 7.4 审查

实施完成后执行 `/ccg:review` 循环，修复 Critical/High 级别问题后重新审查，直到不再出现 Critical/High。

## 8. 已知局限

- Clash-rs master 的 URLTest 锁定无法被识别，会被判为不可选。
- `extra` 随节点记录完整下发，千节点订阅下载荷变大；裁剪留给 Phase C。
- 托盘局部更新仍按菜单文字查找菜单项；在托盘中显示 fixed 等元数据之前需先改为按句柄查找（Phase B）。
- Global 模式子菜单标题变为内核真实名称 "GLOBAL"。
