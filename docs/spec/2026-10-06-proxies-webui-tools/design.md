# 代理页 WebUI 功能（Phase C）

**日期：** 2026-10-06

**状态：** 设计已确认，待实施计划。

**基线：** `feat/proxies-pinned-selection`（#5631，叠放在 #5627 上）。

**分支与 PR：** 四个叠放的 draft PR，均在当前 checkout 实施。

| 阶段 | 分支                            | 基础分支                        |
| ---- | ------------------------------- | ------------------------------- |
| C1   | `feat/proxies-latency-test-url` | `feat/proxies-pinned-selection` |
| C2   | `feat/proxies-node-list-tools`  | `feat/proxies-latency-test-url` |
| C3   | `perf/proxies-trim-extra`       | `feat/proxies-node-list-tools`  |
| C4   | `feat/proxies-group-header-p2`  | `perf/proxies-trim-extra`       |

**需求：** 完成代理页组类型横向调研（2026-10-05，P1、P2 两级建议）中的主窗口代理页功能（多内核差异见[代理组内核接口核查报告](../../audit/2026-10-05-proxy-core-api-report.md)），以及 [Phase B spec](../2026-10-06-proxies-pinned-selection/design.md) 留给 Phase C 的事项：

- C1：测速 URL、超时与 `expected` 参数化，按 `extra[url]` 读取延迟；
- C2：组页面的搜索、排序、隐藏不可用节点，新增 MD3 风格的搜索栏和筛选 chip；
- C3：裁剪 `extra` 载荷；
- C4：组头显示可用数与当前链路，嵌套组成员显示最终节点；断开连接设置改为三档；provider 健康检查。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 实施计划。

## 1. 决策

1. **数据由后端保存。** 本项目之后会同时提供 WebUI 与 Tauri GUI，所有数据（包括只在前端实现的功能的偏好）都必须由后端保存：设置进应用配置，界面偏好进 `useKvStorage`（后端 redb，`get/set_storage_item` 为 `rpc(http)`）。URL 中的搜索词属于临时视图状态，不持久化。
2. **测速 URL 由前端解析（方案 A）。** 组的有效测速 URL 为：组的 `testUrl`（非空）→ 应用配置 `default_latency_test`。前端用同一个纯函数决定测速请求的 URL 和读取 `extra[url]` 的键。后端在请求未带 URL 时回退到 `default_latency_test`，不再写死 gstatic。
3. **超时默认 5 秒**，设置页可改，范围 1–30 秒（mihomo 单节点测速的 timeout 按 int16 毫秒解析，上限 32767 ms；API 适配器对每次调用另有总时限），后端读取应用配置，不作为 RPC 参数。
4. **搜索仅作用于当前组**，搜索词放在 `/main/proxies` 的 URL 参数 `q`，切换组时保留；侧栏不变。
5. **排序与「隐藏不可用」是持久偏好，所有组共用一份**，存于 `useKvStorage`。
6. **工具栏在组头下方第二行**，吸顶。
7. **组头显示可用数和当前链路，嵌套组成员卡片显示最终节点**。
8. 界面的实现与 browser 调试由 Opus medium 或 Sonnet medium 的子代理负责。
9. 在当前 checkout 实施，复用 `backend/target` 构建缓存。

不在本次范围：每组单独覆盖测速 URL、订阅到期时间、Smart 组权重、WebView 托盘与原生托盘的改动。

## 2. 共用概念

本节的函数都是纯函数，放在 `@nyanpasu/query`，供 C1、C2、C4 共用。

| 函数                                     | 含义                                                                                                                                         |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `groupTestUrl(group, defaultUrl)`        | 组的有效测速 URL：`group.testUrl` 非空时取它，否则取 `defaultUrl`                                                                            |
| `nodeDelayHistory(node, url)`            | `node.extra?.[url]?.history`，不存在时退回 `node.history`                                                                                    |
| `latestDelay(node, url)`                 | `nodeDelayHistory` 的最后一条的 `delay`；没有记录时为 `undefined`；`0` 表示测速失败                                                          |
| `resolveChain(group, proxies)`           | 从 `group.now` 逐层沿成员组的 `now` 走到叶子节点，返回经过的名字和叶子节点；记录已访问的组，遇到环或缺失节点即停                             |
| `memberDelay(name, group, proxies, url)` | 成员本身是组时，取 `resolveChain` 得到的叶子节点在直接包含它的那一层组的有效 URL 下的 `latestDelay`；否则取该成员在 `url` 下的 `latestDelay` |

节点是否是组按 `proxies.nodes[name].all` 是否存在判断，与现有 `group-delay.ts` 一致。`resolveChain` 取代 `getGroupSelectedDelay` 中的沿链逻辑。

## 3. C1：测速语义

### 3.1 后端

- `ProxyGroup` 新增 `test_url: Option<String>` 与 `expected_status: Option<String>`，取自内核记录；空串记为 `None`。
- `nyanpasu-config` 的应用配置新增 `default_latency_timeout_ms: u64`，`#[serde(default = ...)]` 默认 5000。缺少 serde 默认值时，已有安装在迁移检查中解析配置会失败。校验范围 1000–30000（理由见决策 3），超出时拒绝写入。
- `client/clash_api.rs`：
  - `delay_query(url, expected)` 改为由调用方传入应用配置：`url` 为空时取 `default_latency_test`，该值也为空时才用 `http://www.gstatic.com/generate_204`；超时取 `default_latency_timeout_ms`。
  - `expected` 非空时经 `ExpectedStatus::new` 校验后加入查询，不合法时返回错误。
  - `proxy_delay(name, provider, url, expected)` 与 `group_delay(group, url, expected)` 增加 `expected` 参数。
- `ipc.rs`：`clash_api_get_proxy_delay` 与 `clash_api_get_group_delay` 增加 `expected: Option<String>`，重新生成 bindings。

### 3.2 前端

- 新增第 2 节的 `groupTestUrl`、`nodeDelayHistory`、`latestDelay`。`defaultUrl` 来自 `useSetting('default_latency_test')`。
- `useClashProxies`：
  - `updateProxiesDelay` 与 `updateGroupDelay` 的选项改为 `{ url, expected }`，由调用方传入组的有效 URL 和 `expectedStatus`；固定组逐节点测速同样使用它们。
  - 乐观更新同时追加到 `history` 和 `extra[url]`（不存在时新建 `{ alive, history }`），与读取顺序一致。
- 组页面：单节点测速与组测速传入当前组的有效 URL 和 `expectedStatus`；`ProxyNodeButton` 的当前延迟与 `DelayHistory` 改读 `nodeDelayHistory(node, url)`。
- 侧栏 `GroupSummary`：改用 `resolveChain` 与 `memberDelay`。
- 设置页（`settings/clash`）新增两项：测速 URL（文本输入，失焦或回车保存，空值恢复默认）与超时（`NumericInput`，单位秒，换算为毫秒保存）。

### 3.3 局限

- mihomo 节点若从未用该 URL 测过，会退回 `history`，其中混有其他 URL 的结果。
- `expected` 只有 mihomo 生效；Clash-rs 与 Meow 忽略它。

## 4. C2：节点列表工具

### 4.1 新组件（`@nyanpasu/ui`）

两者都遵循 Material You，沿用现有语义 token、深色模式、焦点与键盘行为，并带 `data-slot`。

- `SearchField`（`search-field.tsx`）：MD3 搜索栏的紧凑版。高 40px 的圆角胶囊，`surface-variant` 底色（深色为 `surface-variant/30`），与连接页、规则页搜索框一致；左侧搜索图标，有内容时右侧出现清除按钮。清除按钮可用键盘操作，有无障碍名称，清除后焦点回到输入框。
- `FilterChip`（`chip.tsx`）：基于 Radix `Toggle`，暴露 `aria-pressed`。未选中时高 32px、圆角 8px、`outline-variant` 描边；选中时填充 `secondary-container`，左侧 ✓ 以动画出现。与页面 `_modules/filter-chip.tsx`（可移除的输入 chip）用途不同，不合并。

排序菜单用图标按钮加现有 `DropdownMenu` 的 `DropdownMenuRadioGroup`，不新增组件。

### 4.2 组页面

- `GroupHeader` 下方加一行吸顶工具栏：`SearchField`、排序按钮、「隐藏不可用」`FilterChip`。
- 搜索：
  - `/main/proxies` 路由的 `searchQuery` 改为 `q`，用 `useSearchTerm` 读写；侧栏的组链接保留 `q`。
  - 匹配：不区分大小写的子串，空格分隔的多个词须全部命中，匹配节点名和类型。
  - 命中部分用 `HighlightText` 高亮。
- 排序：
  - `default`：配置顺序；
  - `name`：自然排序（`Intl.Collator` 的 `numeric`）；
  - `delay`：按 `memberDelay` 从低到高，未测过的排在已测之后，失败的排最后，同类保持配置顺序。
- 隐藏不可用：隐藏 `memberDelay` 为 0 的成员；未测过的保留；`group.now` 总是保留。
- 偏好：`useKvStorage('proxies-node-view', { sort: 'default', hideUnavailable: false })`，所有组共用。
- 虚拟列表对过滤后的列表计数；「定位当前节点」按过滤后的位置滚动，当前节点不在列表中时按钮置灰；全部被过滤掉时显示"没有匹配的节点"。
- 搜索匹配、排序、过滤写成纯函数，放在页面 `_modules`。

## 5. C3：载荷裁剪

- 先量化：Rust 测试构造 1000 个节点、每节点 3 个 URL、每个 URL 10 条历史的 `Proxies`，输出裁剪前后的序列化字节数，数字写入 PR 描述。节省不到 30% 时在 PR 中说明，由用户决定是否合并。
- `Proxies::retain_extra(&mut self, keep: &HashSet<&str>)`：每个节点的 `extra` 只保留键在 `keep` 中的条目，`history` 不变。
- `keep` = 所有组（含 GLOBAL）的 `test_url` ∪ `default_latency_test`。
- 只在 `NyanpasuClient::get_proxies` 与 `refresh_proxies` 的返回值上裁剪（对应 `get_proxies` 与 `mutate_proxies` RPC）。`ProxiesActor` 的快照保持完整，托盘读取完整快照，actor 不依赖应用配置。
- 局限：provider 健康检查写入的 provider URL 条目，若不是某个组的 URL 会被裁掉，界面目前不读取；HTTP 调用方同样拿不到完整 `extra`。

## 6. C4：组头增强与其他 P2

### 6.1 组头与成员卡片

- 组头第二行显示「可用 x/y · A › B › 叶子节点」：
  - `x` 为 `memberDelay` 大于 0 的成员数，`y` 为成员总数；
  - 链路取 `resolveChain(group)`，组没有 `now` 时不显示链路。
- 已有的固定标记行保留。
- 本身是组的成员，卡片第二行在类型 chip 与延迟之间显示「→ 叶子节点」，延迟取 `memberDelay`。卡片保持 64px 高。

### 6.2 断开连接设置三档

- `settings/nyanpasu` 的 `break-when-proxy-change-switch` 改为 `SegmentedButton`：关闭（`Off`）、仅本组（`ProxyGroup`）、全部（`All`），写入 `break_connection.on_proxy_change`。
- 说明文字注明：Meow 的连接链路只有规则目标，"仅本组"会漏断经嵌套组建立的连接。
- 不再使用的 `proxyChangeBreakMode(enabled)` 等转换函数随之删除；组件名与 `data-slot` 改为 `break-when-proxy-change-selector`。

### 6.3 provider 健康检查

- 后端新增 `#[nyanpasu_macro::rpc(http)] clash_api_healthcheck_proxy_provider(name)`，注册为 mutation。`NyanpasuClient::healthcheck_proxy_provider` 调用 `ProxiesClient::healthcheck_provider`：与更新 provider 共用 `ProxiesActor` 的同一路径（调用内核、被拒绝时重读、成功后刷新快照），因为健康检查改变了 actor 缓存的节点延迟。内核返回 202 与 204 都算成功（`send_empty` 按 2xx 判断）。
- 前端在 provider 详情页 `InfoCard` 底部「更新」旁加「健康检查」按钮，成功后刷新 providers 与 proxies 查询，失败时用 `message(..., { kind: 'error', error })` 提示。

## 7. 测试与验收

| 阶段 | 测试                                                                                                                                                                                                                     |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1   | Rust：`ProxyGroup` 带出 `testUrl`/`expectedStatus`（空串为 `None`）；`delay_query` 的回退与超时、`expected` 校验；配置缺字段时取默认值。前端：第 2 节函数的单元测试；测速传参与乐观更新写入 `extra[url]` 的 browser 测试 |
| C2   | 搜索匹配、排序、过滤的单元测试；`SearchField` 与 `FilterChip` 的 browser 测试（键盘、`aria-pressed`、清除按钮）；组页面 browser 测试（过滤后计数、定位按钮置灰、空结果提示）                                             |
| C3   | `retain_extra` 单元测试（保留与删除、`extra` 为空或 `None`）；`trim_for_frontend`（由 `get_proxies`、`refresh_proxies` 调用）的单元测试证明组 URL 与默认 URL 都被保留；体积量化测试                                      |
| C4   | `resolveChain`、可用数、环检测的单元测试；健康检查的 actor 测试（假内核 202 与 204，成功后重读）；三档设置与健康检查按钮的 browser 测试                                                                                  |

每段完成后执行 `/ccg:review`，修复 Critical/High 后重新审查，直到不再出现 Critical/High，然后推送并创建 draft PR。每个提交独立通过 `pnpm typecheck`，后端改动通过 `cargo test`（已知失败的 `connection_policy::profile_policy_and_noop_gates_do_not_acquire_a_source` 除外）与 clippy。

需要人工检查：设置页两项与三档设置在 GUI 中的外观；工具栏在窄窗口下的换行；provider 健康检查在 mihomo、Clash-rs、Meow 上的表现。

## 8. 已知局限

- 见 3.3 与第 5 节。
- 延迟排序在组测速过程中会随结果到达而重排。
- 搜索不匹配嵌套组内部的节点，只匹配当前组的直接成员。
