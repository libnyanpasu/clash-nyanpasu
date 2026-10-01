# 流量用量分析与拓扑改造 任务

关联设计：`design.md`（同目录）

## 0. 全局约束

1. 遵守 `AGENTS.md`：不新增全局单例；墙钟、保留策略、订阅选择都以端口形式注入 `TrafficActor`；聚合、拓扑投影、时间桶计算都是纯函数。
2. 不保留旧接口的兼容层：`GroupBy` → `Dimension`、删除 `query_traffic_topology`、给 usage 查询加 `TrafficQuery` 参数，这些改动在同一个提交里把调用方一起改完。
3. 每个提交都必须能编译、测试通过；每个提交只做一件事（`AGENTS.md` §18）。
4. actor 测试不用 sleep：时钟通过注入控制，帧通过现有的 `observe` / `flush` 测试钩子投递。
5. 开始实施前，先把 `feat/topology-traffic-filter` rebase 到 `main`（当前落后 `70a5853ce`）。

## 1. 提交与 PR 划分

| PR                        | 提交                                                               | 内容   |
| ------------------------- | ------------------------------------------------------------------ | ------ |
| PR-1 后端                 | C1 `feat(traffic): aggregate usage by dimension, scope and range`  | T1、T2 |
|                           | C2 `feat(traffic): record usage per time bucket with retention`    | T3–T6  |
| PR-2 前端（叠在 PR-1 上） | C3 `feat(settings): choose how long traffic history is kept`       | T7     |
|                           | C4 `feat(traffic): rebuild the topology page as traffic analytics` | T8、T9 |
|                           | C5 `feat(traffic): draw topology and map from recorded usage`      | T10    |

C2 改变了存储端口和 IPC 签名，所以存储、actor、facade、IPC、bindings 和现有前端调用方必须放在同一个提交里，否则中间状态无法编译。

---

## T1 — 模型：维度、用量、时间桶（纯）

- [ ] `model.rs`：`Dimensions` 增加 `inbound`、`profile`、`source_region`、`destination_region`；`GroupBy` 改名为 `Dimension`，增加 `Origin`、`Inbound`、`Profile`、`SourceRegion`、`DestinationRegion`；`Chain` 改为不含出口的策略组路径（design §3.1）。
- [ ] 新增 `Usage { bytes, connections }`、`Tier`、`TrafficRange`、`TrafficScope`、`Filter`、`TrafficQuery`、`Metric`（全部带 `specta` 派生，与现有类型一致）。
- [ ] 新增 `bucket.rs`：`minute_of(wall_ms)`、`hour_of(wall_ms)`、`TrafficRange::tier()`、`TrafficRange::start(now_ms) -> Option<u32>`（向下对齐到桶）。
- [ ] 地区规范化函数：去重、转大写，恰好一个代码时取该值，否则为 `unknown`。

**验证**

- `cargo test -p nyanpasu-traffic`：覆盖每个 `Dimension` 的分组键（含 `Origin` 的回退、`Chain` 为空、`Profile` 为 `None`）；桶边界（整点前后各 1 ms）；6 档范围对应的级别与起点；地区规范化（0 个、1 个、大小写重复、多个代码）。

## T2 — 聚合器与拓扑投影（纯）

- [ ] 新增 `query.rs`：`ReportRequest`、`TopologyRequest`、`TrafficReport`、`Ranking`、`UsageGroup`（含 `usage` 与 `current_rate`）；聚合器接受 `(&Dimensions, Usage, Option<Rate>)` 行，按 design §5.1 执行筛选、排名、`distinct`、`other`。
- [ ] 分页：`usage_page(rows, dimension, after, limit)` 沿用现有游标语义（按字节总量从大到小，相同时按键排序；游标为不含）；`usage_by_keys` 按请求顺序返回，没有流量的键不返回。
- [ ] 拓扑：把 `topology::project` 改为按任意层列表投影，每层按 `metric` 取前 N、其余合并为"其他"（连同边一起重定向），`limit_per_layer = None` 时不合并；`Chain` 层键为空时该路径跳过这一层；节点 id 仍为（层序号，键）的 JSON 编码。
- [ ] 本提交与旧代码并存（旧的 `rank_usage` 等暂不删），保证 C1 可以独立编译。

**验证**

- 单元测试：AND 筛选；同一维度替换筛选值；`distinct` 只统计筛选后的取值；`other` 等于排名之外各组之和；"其他"节点合并后边的流量守恒（每层节点流量之和等于总量）；跨层同名节点 id 不冲突；`limit_per_layer = None` 时不出现"其他"；`Metric::Connections` 按连接数排序。

## T3 — 统计会话：按桶累计、关闭时写入、按订阅标记

- [ ] `accounting.rs`：`ActiveConnection` 改为 `minutes` / `hours` 两个桶映射（删除 `bytes`）；每个增量同时记入两个映射；`profile` 在连接首次出现时由调用方传入的当前订阅确定，之后不再改写。
- [ ] 关闭时（含实例切换导致的批量关闭）按最终维度把桶数据写入待落盘缓冲，关闭时刻所在桶 `connections += 1`，同时生成 `ClosedConnection`。
- [ ] `take_batch(now_ms, prune)`：丢弃活跃连接中早于 `now − 6h` 的分钟桶；批次里带上发生变化的活跃连接、已关闭连接的 id、两级用量增量、明细以及 `Prune`。
- [ ] 删除 `switch_profile`、`reset_pending`、`require_reset`、`FlushBatch.reset`、`pending_totals`、`pending_topology`、`record`；`SessionMeta` 删除 `profile`、`started_at`、`core_bytes`。
- [ ] 提供查询所需的内存行：`active_rows(tier, from)`、`pending_rows(tier, from)`。

**验证**

- 现有的增量、计数器重置、实例切换、断连、恢复等测试按新结构改写后保持通过。
- 新增测试：跨整点的连接，其流量正确拆分到两个桶；关闭后待落盘缓冲的合计等于该连接各桶之和；**等式测试**：对一组固定帧序列，在每个范围、每个级别、每个维度上都断言 全部 = 活跃 + 已结束；订阅切换后已有连接仍保持原订阅、新连接标记新订阅，且没有任何数据被清空。

## T4 — redb 存储 v2

- [ ] `SCHEMA_VERSION = 2`；表结构按 design §4；遇到 v1 文件按现有逻辑删除重建。
- [ ] 维度组合编号：打开时加载 `tuples` 表到内存（`Mutex` 包裹，注释说明理由）；`flush` 时为新组合分配编号。
- [ ] `flush`：一个事务内写 meta、增删活跃连接（不再整表重写）、累加两级用量、追加明细，再执行 `Prune` 的三项范围删除。
- [ ] `usage(tier, from)`：按时间前缀做范围扫描，先按组合编号合计，再返回 `(Arc<Dimensions>, Usage)`。
- [ ] `collect_tuples(keep)`：删除两张用量表都不引用、且不在 `keep` 中的组合，返回删除个数。
- [ ] 删除 `TOTALS`、`TOPOLOGY` 两张表及 `totals`、`totals_of`、`topology` 方法；处理掉 `redb.rs:152` 的 TODO。

**验证**

- `tempfile` 测试：v1 文件被替换；`flush` 后 `usage` 能读回，并且与重新打开数据库后读到的一致；`Prune` 三项删除的边界（恰好在截止桶上的数据保留）；永久保留（`None`）不删除任何数据；`collect_tuples` 不删除仍被引用或在 `keep` 中的组合；活跃连接只更新有变化的那些，已关闭的从 `active` 表移除。
- `#[ignore]` 基准测试 `usage_scan_500k_hour_rows`：构造 500 个组合 × 1000 个小时桶的数据，测量 `usage(Hour, None)` 加一次报表聚合的耗时（release 模式运行），结果写入 PR 描述（design G9）。

## T5 — actor、端口与 facade

- [ ] `core/traffic/ports.rs`：新增 `RetentionPolicy`（`fn retention(&self) -> Option<Duration>`）和 `Clock`（`fn now_ms(&self) -> i64`），两者都 `cfg_attr(test, mockall::automock)`。
- [ ] `TrafficArgs` 增加 `retention`、`clock`；`pump` 与落盘改用注入的时钟；每次落盘时由保留设置计算 `Prune`；维度组合回收最多每小时一次（design §6）。
- [ ] actor 消息：`Report(ReportRequest)`、`Usage(TrafficQuery, Dimension, after, limit)`、`UsageByKeys(TrafficQuery, Dimension, keys)`；删除 `Topology`。查询按 design §5.2 组合存储行、待落盘行和活跃行。
- [ ] `source.rs`：映射 `inbound`（`inbound_user` 优先，然后 `inbound_name`）、`source_region`、`destination_region`。
- [ ] `client/traffic.rs`：`SettingsRetention` 读取 `ApplicationClient::snapshot_handle()`，实现 `RetentionPolicy`；在组合根注入；`SystemClock` 实现 `Clock`。
- [ ] `NyanpasuClient`：新增 `query_traffic_report`；`query_traffic_usage`、`query_traffic_usage_by_keys` 加上 `TrafficQuery` 参数；删除 `query_traffic_topology`；`TrafficSummary` 按 design §5.3 精简。

**验证**

- `core/traffic/tests.rs`：用假时钟推进跨越分钟、小时边界并落盘，断言两级用量；把保留期从 7 天改为 1 天后，下一次落盘就删除超期的小时数据和明细；永久保留不删除；订阅切换不清空数据，按 `Profile` 筛选可以区分；"活跃"范围的速率等于各连接速率之和，"已结束"范围的速率为 `None`；记录不可用时，新命令也返回 "traffic recording is unavailable"。

## T6 — 配置字段、IPC 与现有调用方（与 T3–T5 同一提交）

- [ ] `nyanpasu-config`：新增 `TrafficRetention` 和 `traffic_retention` 字段（design §7）；补充配置的序列化 / patch 测试。
- [ ] `impact.rs`：在 `AppCase` 表中登记 `traffic_retention`（`RuntimeImpact::None`，`owners: &[]`）。
- [ ] `ipc.rs`：新增 `query_traffic_report`，修改两个 usage 命令的参数，删除 `query_traffic_topology`；`specta_export` 注册新类型。
- [ ] 按现有的 specta 导出测试重新生成 `frontend/interface/src/ipc/rpc-bindings.ts`。
- [ ] `interface`：`useTrafficUsageByKeys(query, dimension, keys)`；新增 `useTrafficReport(request, options)`、`useTrafficUsagePages(query, dimension)`（基于 `useInfiniteQuery`）。
- [ ] 规则页 `use-rule-stats.ts`：传 `{ range: 'all', scope: 'all', filters: [{ dimension: 'profile', value: 当前订阅 uid }] }`；轮询间隔改为 10 秒。
- [ ] 连接页 `status-tabs.tsx`：适配精简后的 `TrafficSummary`。

**验证（C2 整体）**

- `pnpm lint:clippy`、`pnpm lint:rustfmt`、`pnpm test:backend`、`pnpm lint:ts`、`pnpm test:frontend`、`pnpm lint:architecture-ledger` 全部通过。
- `rg "GroupBy|query_traffic_topology|switch_profile|TopologyKey" backend frontend/interface/src frontend/nyanpasu/src` 无结果（生成文件中出现的除外，生成文件也应已更新）。

---

## T7 — 设置：流量记录保留期限（C3）

- [ ] `settings/nyanpasu/_modules/traffic-retention-selector.tsx`，结构照抄 `log-level-selector.tsx`：5 个选项，描述行显示当前值和 design §7 中的提示文字。
- [ ] 在设置页"Nyanpasu"分区挂载；为 `en`、`ko`、`ru`、`zh-cn`、`zh-tw` 补充 i18n key，然后运行 paraglide 编译。

**验证**

- 浏览器测试 `traffic-retention-selector.browser.test.tsx`：展示当前值；选择后用对应的值调用 `upsert`。
- `pnpm lint:ts`、`pnpm test:frontend`。

## T8 — 流量页骨架、工具栏与数字卡片（C4）

- [ ] 路由 search schema：`range`、`scope`、`filters`、`view`、`layers`、`metric`（design §8.1），删除 `proxy`。
- [ ] 页面骨架改为"内容 `ScrollArea` + 底部工具栏 `h-16 bg-mixed-background`"；删除页内大标题；导航名称改为"流量"。
- [ ] 工具栏：范围 `SegmentedButton size="sm"`、筛选 chip（`_modules/filter-chip.tsx`）、时间范围 `Select`（超过保留期限的档位置灰）、暂停图标按钮。
- [ ] 数字卡片：总流量（↑/↓）、连接、设备、入站、主机、出口；数据来自 `useTrafficReport` 的 `total` 和各排名的 `distinct`。
- [ ] 轮询间隔：时间范围 ≤ 24 小时为 2 秒，更长为 10 秒；暂停时停止轮询。
- [ ] 本提交中拓扑卡片暂时保留现有的 WebSocket 数据源，T10 再切换。

## T9 — 排名卡片与"查看全部"（C4）

- [ ] 排名卡片 × 5（设备、入站、主机、出口、进程），每张显示前 5 名和"其他"行；行样式与交互按 design §8.2。
- [ ] 点击行切换对应维度的筛选（已是筛选值时移除）。
- [ ] "查看全部"`Modal`：用 `useTrafficUsagePages` 无限分页，点击行添加筛选并关闭弹窗。
- [ ] 标签显示规则：进程文件名加路径 Tooltip，IP 用 `font-mono`，订阅 uid 转为名称（已删除的显示 uid 加"（已删除）"），`unknown` 与空串分别显示"未知""无策略组"。

**验证（C4 整体）**

- 浏览器测试 `traffic-page.browser.test.tsx`（模拟 IPC 命令）：默认请求为 `LastHour` + `All` + 无筛选；切换范围和时间范围后，下一次请求参数随之改变；点击排名行出现 chip，请求带上筛选；移除 chip 和"清除全部"都生效；超过保留期限的档位不可选；记录不可用时显示错误卡片。
- `pnpm lint`、`pnpm test:frontend`。

## T10 — 拓扑与地图改用后端数据（C5）

- [ ] 拓扑卡片：`useTrafficReport` 请求带上 `topology`（按层级模板、指标、每层 7 个节点）；绘制代码改为消费后端返回的 `nodes` / `edges`；点击节点添加筛选，"其他"节点不可点击；悬停时高亮相邻的边。
- [ ] 层级模板 `Select` 提供 4 个模板（design §8.2）；指标切换 [流量 | 连接数]。
- [ ] 地图：请求 `layers = [SourceRegion, DestinationRegion]`、`limit_per_layer = None`；`GeographyView` 改为消费节点和边；点击地区添加 `DestinationRegion` 筛选。
- [ ] 删除 `buildTopology`、`buildGeography`、`connectionRegion`、全文搜索框、"连接详情"列表和 `tests/connection-topology.test.ts`；删除不再使用的 i18n key 并重新运行 paraglide 编译。

**验证**

- 浏览器测试：切换模板后请求的 `layers` 随之改变；点击普通节点添加筛选，点击"其他"节点无反应；地图模式下请求不合并节点。
- `rg "useClashConnectionDetails" frontend/nyanpasu/src/pages/\(main\)/main/topology` 无结果。
- `pnpm lint`、`pnpm test`。

---

## T11 — 冒烟（真机，PR-2 合并前）

- [ ] 浏览若干网站后，流量页默认"最近 1 小时"有数据；6 张数字卡片、5 张排名卡片、拓扑、地图的数字互相一致（例如设备排名各项加上"其他"等于总量）。
- [ ] 在"全部 / 活跃 / 已结束"之间切换：任取一个分组，全部 = 活跃 + 已结束。
- [ ] 切换订阅后，旧数据仍在；按订阅筛选可以区分；规则页只统计当前订阅。
- [ ] 把保留期限改为 1 天，30 秒内超期数据消失；改为永久后重启应用，数据仍在。
- [ ] 浅色、深色主题以及窄窗口（移动端断点）下，布局与连接页一致，没有横向滚动条。
- [ ] 截图附在 PR 描述中。
