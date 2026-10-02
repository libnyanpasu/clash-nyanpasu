# 流量用量分析与拓扑改造 设计

**日期：** 2026-10-01
**基线：** `main @ 70a5853ce`（含 #5477–#5479 流量记录、#5486 已结束连接标签页、#5487 规则流量）
**来源：** 拓扑页需求讨论（2026-10-01）；调研见 [`2026-09-16-traffic-topology-research.md`](../../reports/2026-09-16-traffic-topology-research.md)
**范围：** `nyanpasu-traffic` 存储与查询模型重做；`TrafficActor` / `TrafficClient` / `NyanpasuClient` / IPC 接口调整；新增保留期限设置；拓扑页改为基于后端数据的"流量"分析页；规则页与连接页"已结束"标签页随接口迁移。
**不在范围：** 见 §10。
**权威顺序：** `AGENTS.md` > `docs/design/actor-migration-roadmap.md` > 本设计 > `task.md`

---

## 0. 结论摘要

1. **用一套带时间桶的"用量统计表"取代 `TOTALS` 与 `TOPOLOGY`。** key 为（时间桶, 维度组合），value 为（上传, 下载, 连接数）。按任意维度排名、筛选、拓扑展开、地图，都由同一个纯函数从这张表算出。`CLOSED` 只保留为连接明细日志。
2. **两级时间粒度，写入时同时写两份。** 分钟级只保留 6 小时，供 1 小时和 6 小时两档时间范围使用；小时级按用户设置的保留期限保留，供其余档位使用。两级各自清理，不做合并或压缩。
3. **连接关闭时才写入统计表。** 活跃连接在内存里按分钟、按小时累计自己的流量；关闭时按最终维度一次写入。所以"已结束"直接读统计表，"活跃"读内存，"全部"等于两者之和。三种范围在任何时间范围、任何筛选下都满足 **全部 = 活跃 + 已结束**，不需要做减法。
4. **订阅成为一个维度，切换订阅不再清空统计。** `Session::switch_profile` 及与之配套的清空逻辑删除。规则页按"当前订阅 + 保留期内全部"查询。
5. **新增维度：入站、来源地区、目标地区。** 入站取 `inbound_user`，为空时取 `inbound_name`。地区规范化从前端移到后端，地图视图因此也能按时间范围、范围和筛选条件统计。
6. **保留期限在设置中配置。** 可选 1 天、7 天（默认）、30 天、90 天、永久。连接明细日志和小时表一同按此清理。由 actor 每次落盘时读取当前设置，不需要接入 effect 计划。
7. **前端：拓扑页改为"流量"分析页，沿用连接页的页面骨架**（内容区滚动，底部工具栏 `h-16 bg-mixed-background`），按 MDY（Material Design You / M3）规范使用现有的 `Card`、`SegmentedButton`、`Select`、`Modal` 组件和配色令牌。时间范围用 Select 选择，默认最近 1 小时。

---

## 1. 现状事实（已核对源码）

### 1.1 后端

| 事实                                                                                                                                                                                                            | 位置                                                                                    |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| 统计以"订阅会话"为单位：切换订阅时 `switch_profile` 清零，下一次落盘带 `reset` 清空全部表                                                                                                                       | `nyanpasu-traffic/src/accounting.rs:169-181`；`redb.rs` `flush` 中的 `batch.reset` 分支 |
| 每帧把增量按 7 个维度各自累加进 `pending_totals`，同时累加进固定四层的 `pending_topology`                                                                                                                       | `accounting.rs:259-274`                                                                 |
| 活跃连接的维度每帧都被覆盖为最新值，而增量按"当帧维度"记账；连接存活期间维度变化时，按维度做"全部 − 活跃"会算错                                                                                                 | `accounting.rs:115-121`                                                                 |
| 表结构：`SESSION`、`ACTIVE`（每次落盘整体重写）、`CLOSED`（(closed_at, id) → JSON）、`TOTALS`（(维度, 取值) → 字节）、`TOPOLOGY`（JSON `TopologyKey` → 字节）；`SCHEMA_VERSION = 1`，其他版本的文件直接删掉重建 | `redb.rs:7-22`、`redb.rs:29-73`                                                         |
| `CLOSED` 无上限，有 TODO                                                                                                                                                                                        | `redb.rs:152`                                                                           |
| 没有时间维度，`TOPOLOGY` 不含目标主机，`TOTALS` 各维度互相独立，无法做交叉筛选                                                                                                                                  | `model.rs`、`redb.rs`                                                                   |
| actor 每 30 秒落盘；查询在 actor 内串行执行；帧经 `watch` 投递，actor 忙时只保留最新一帧                                                                                                                        | `core/traffic/actor.rs:23`、`actor.rs:419-450`                                          |
| 维度从 `clash_api::Connection` 映射；元数据里有 `inbound_name`、`inbound_user`、`source_geo_ip`、`destination_geo_ip`，目前未使用                                                                               | `core/traffic/source.rs:43-71`；`clash-api/src/api/connections.rs:159-230`              |
| 订阅由 `ProfileSelection` 端口提供，实现读取已提交的 profiles 快照                                                                                                                                              | `core/traffic/ports.rs`；`client/traffic.rs:10-27`                                      |
| 存储打开失败时记录被禁用，所有查询返回 "traffic recording is unavailable"                                                                                                                                       | `setup.rs:225-243`；`client/traffic.rs:30-35`                                           |

### 1.2 前端

| 事实                                                                                                                                                                      | 位置                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| 拓扑页完全不用后端统计：读 WebSocket 连接快照，在前端用 `buildTopology` 聚合，每列最多 7 个节点                                                                           | `pages/(main)/main/topology/route.tsx:27-48`；`_modules/topology.ts:36-122`                           |
| 拓扑页的"流量"只是当前存活连接的累计值，说明文字自己也承认"不代表历史用量"                                                                                                | `messages/zh-cn.json` `topology_caption`                                                              |
| 地图视图在前端规范化 GeoIP 地区代码（多个代码判为未知）                                                                                                                   | `_modules/geography.ts:15-30`                                                                         |
| 拓扑页有页内大标题（eyebrow + h1 + 描述），连接页、规则页没有；连接页工具栏在底部，`h-16 bg-mixed-background`，内含 `SegmentedButton size="sm"`                           | `topology-view.tsx`（header 段）；`connections/index.tsx:96`                                          |
| 后端统计的消费方只有两个：规则页 `useTrafficUsageByKeys('rule', labels)`；连接页"已结束"标签页（`useTrafficClosedConnections`、`useTrafficSummary().closed_connections`） | `rules/_modules/use-rule-stats.ts`；`connections/_modules/closed-viewer.tsx`、`status-tabs.tsx:85-99` |
| `query_traffic_topology` 与分页的 `query_traffic_usage` 前端没有调用                                                                                                      | `rg queryTraffic frontend`                                                                            |
| 设置项的写法：`useSetting(key)` + `SettingsCard` + `DropdownMenu`                                                                                                         | `settings/nyanpasu/_modules/log-level-selector.tsx`                                                   |
| 应用配置的每个字段在 `impact.rs` 的 `AppCase` 表中登记影响类别                                                                                                            | `client/application_workflow/impact.rs:740-830`                                                       |

---

## 2. 目标与成功判据

| #   | 判据                                                                                                         | 验证方式                                       |
| --- | ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------- |
| G1  | 可以按设备、入站、主机、出口、进程（以及规则、策略链、协议、订阅、地区）排名，每组同时给出上传、下载、连接数 | 纯函数单元测试；页面浏览器测试                 |
| G2  | 任意筛选组合下都可以查询（AND），点击排名项或拓扑节点即可添加筛选                                            | 聚合单元测试；浏览器测试                       |
| G3  | 时间范围可选 1 小时（默认）/6 小时/24 小时/7 天/30 天/全部                                                   | 时间桶单元测试；浏览器测试                     |
| G4  | 范围可选全部/活跃/已结束，并且对每个分组都满足 全部 = 活跃 + 已结束                                          | 用属性式测试遍历多组帧序列断言该等式           |
| G5  | 切换订阅不丢数据；按订阅筛选能区分各订阅的流量                                                               | actor 测试                                     |
| G6  | 保留期限可配置，默认 7 天，支持永久；缩短期限后，下一次落盘就删除超期数据                                    | 存储测试；actor 测试（注入假时钟与假保留策略） |
| G7  | 拓扑和地图使用后端数据，并与排名共用同一组筛选条件                                                           | 浏览器测试                                     |
| G8  | 页面结构和组件与连接页、规则页一致，不再有自造的输入框和页内大标题                                           | 代码审查；冒烟截图                             |
| G9  | 查询开销可接受：模拟 50 万条小时表记录时，一次报表查询的耗时被测出并记录在案                                 | `#[ignore]` 基准测试，结果写入 PR 描述         |

---

## 3. 数据模型

### 3.1 维度

`Dimensions` 是一条连接的全部维度，在连接首次出现时生成，此后每帧更新（`profile` 除外）：

| 字段                                   | 来源                                                                                                   | 未知时    |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------ | --------- |
| `process`                              | `process_path`，否则 `process`；`\` 统一换成 `/`                                                       | `unknown` |
| `source`                               | `source_ip`（界面上称"设备"）                                                                          | `unknown` |
| `inbound`                              | `inbound_user`，为空时取 `inbound_name`                                                                | `unknown` |
| `target`                               | `host`，否则 `destination_ip`（界面上称"主机"）                                                        | `unknown` |
| `protocol`                             | `network`                                                                                              | `unknown` |
| `rule`                                 | `RuleKey { kind, payload }`                                                                            | —         |
| `chains`                               | clash 原始顺序：出口在前，最外层组在后                                                                 | 空        |
| `profile`                              | 连接**首次出现时**的当前订阅 uid，此后不再改变                                                         | `None`    |
| `source_region` / `destination_region` | GeoIP 代码去重并转大写；恰好一个时取该值，否则未知（逻辑从 `geography.ts` 移过来）；后续定位方式见 §11 | `unknown` |

`profile` 固定为首次出现时的值，是因为连接的规则与策略链属于建立它时的订阅配置。

查询时用 `Dimension` 枚举指定维度。现有的 `GroupBy` 改名为 `Dimension`，因为它同时用于分组和筛选。

| `Dimension`                                              | 分组键                                          | 说明                                                                        |
| -------------------------------------------------------- | ----------------------------------------------- | --------------------------------------------------------------------------- |
| `Origin`                                                 | 进程已知时取 `process`，否则取 `source`         | 派生维度，不存储；拓扑默认第一层                                            |
| `Process` / `Source` / `Inbound` / `Target` / `Protocol` | 对应字段                                        |                                                                             |
| `Rule`                                                   | `RuleKey::label()`                              | 与规则页的 `ruleLabel` 一致                                                 |
| `Chain`                                                  | 去掉出口后的策略组路径，最外层在前，用 `→` 连接 | **语义变更**：现在的 `Chain` 包含出口（前端没有使用）。没有策略组时键为空串 |
| `Exit`                                                   | `chains[0]`                                     |                                                                             |
| `Profile`                                                | 订阅 uid；`None` 时为空串                       |                                                                             |
| `SourceRegion` / `DestinationRegion`                     | 对应字段                                        |                                                                             |

### 3.2 用量与时间桶

```rust
pub struct Usage { pub bytes: Bytes, pub connections: u64 }

/// 分钟级：自 Unix 纪元起的分钟数；小时级：自纪元起的小时数（均按 UTC 对齐）。
pub enum Tier { Minute, Hour }

pub enum TrafficRange { LastHour, Last6Hours, Last24Hours, Last7Days, Last30Days, All }
pub enum TrafficScope { All, Active, Closed }
pub struct TrafficFilter { pub dimension: Dimension, pub value: String } // 不叫 Filter：specta 导出时与日志模块的同名类型冲突
pub struct TrafficQuery { pub range: TrafficRange, pub scope: TrafficScope, pub filters: Vec<TrafficFilter> }
```

- **增量记在哪个桶：** 每帧算出的增量，记入该帧墙钟时间所在的分钟桶和小时桶。首次看到一条连接时，它的计数器全部记入首次出现的桶，沿用现有语义。
- **时间范围对应哪一级：** ≤ 6 小时用分钟级，其余用小时级。范围起点向下对齐到所在的桶，因此"最近 1 小时"实际覆盖 60–61 分钟，"最近 7 天"实际最多多出 1 小时。`All` 用小时级，不设起点。
- **连接数的含义：** 已结束的连接，记在**关闭时刻**所在的分钟桶和小时桶，每条记 1；活跃连接按当前存活数计（只要符合筛选条件）。因为时间范围总是截止到"现在"，在范围内有流量的已结束连接，一定是在范围内关闭的。反过来，在范围内关闭、但流量全部发生在范围开始之前的连接，会以 0 字节计入连接数。这一点在界面说明中写明。
- **墙钟回拨：** 增量记入回拨后的桶，不做纠正。这些是统计数据，不是计费账本。

### 3.3 活跃连接

```rust
pub struct ActiveConnection {
    pub id: String,
    pub started_at: i64,
    pub first_seen_at: i64,
    pub counters: Bytes,
    pub dimensions: Dimensions,
    /// 本连接在各分钟桶的流量；早于分钟级保留窗口的桶直接丢弃（小时桶里已经有这部分）。
    pub minutes: BTreeMap<u32, Bytes>,
    /// 本连接在各小时桶的流量。
    pub hours: BTreeMap<u32, Bytes>,
}
```

- 每个增量同时加到 `minutes` 和 `hours` 上，两者都只记录非零的桶。
- 每次落盘时，丢弃早于 `now − 6h` 的分钟桶。一条长期存活的连接最多保留约 361 个分钟桶，再加上它存活期间的全部小时桶。小时桶在连接存活期间不按保留期限裁剪，关闭写入后才随小时表一起过期。
- `ActiveConnection.bytes` 删除，改为由 `hours` 求和得出（`ClosedConnection.bytes` 保留，连接明细要用）。

### 3.4 关闭时写入

连接关闭时（包括内核实例切换导致的批量关闭），按它的最终 `dimensions`：

- 每个 `minutes[m]` 加到分钟表的 (m, 维度组合) 上，每个 `hours[h]` 加到小时表的 (h, 维度组合) 上；
- 关闭时刻所在的分钟桶和小时桶，各自 `connections += 1`；
- 同时向 `CLOSED` 追加一条明细（现有行为）。

写入先累积在 `Session` 的待落盘缓冲里，下一次落盘时写入数据库。查询时把缓冲合并进结果（与现在 `pending_totals` 的处理方式相同）。

### 3.5 已删除的概念

- `Session::switch_profile`、`reset_pending`、`require_reset`、`FlushBatch.reset`，以及"存储里还是另一个会话"的查询分支；
- `SessionMeta.profile`、`started_at`、`core_bytes`（`instance_id`、`global_counters`、`last_sample_at` 保留，用于计算增量）；
- `TOTALS`、`TOPOLOGY` 两张表，`TopologyKey`、`pending_totals`、`pending_topology`、`totals_of`；
- `query_traffic_topology` 命令（并入报表查询）。

---

## 4. 存储（`RedbTrafficStore`，schema v2）

| 表             | key → value                                         | 说明                                                                         |
| -------------- | --------------------------------------------------- | ---------------------------------------------------------------------------- |
| `schema`       | `"version"` → `2`                                   | 遇到 v1 文件，按现有"可丢弃"策略删除重建                                     |
| `session`      | `"meta"` → JSON `SessionMeta`                       |                                                                              |
| `active`       | 连接 id → JSON `ActiveConnection`                   | 改为只写入有变化的连接、删除已关闭的连接，不再整表重写（长连接的桶数据较大） |
| `closed`       | (closed_at, id) → JSON `ClosedConnection`           | 按保留期限清理                                                               |
| `tuples`       | `u64` → JSON `Dimensions`                           | 维度组合的编号表                                                             |
| `minute_usage` | (分钟 `u32`, 组合编号 `u64`) → (上传, 下载, 连接数) | 保留 6 小时                                                                  |
| `hour_usage`   | (小时 `u32`, 组合编号 `u64`) → (上传, 下载, 连接数) | 按保留期限清理                                                               |

- **为什么要给维度组合编号：** 同一个组合会在每个时间桶里出现一次。7 天范围的查询，同一组合最多出现 168 次。先按编号把各桶的用量加总（只是整数相加），每个组合只需要做一次筛选和分组。如果直接用 JSON 当 key，每一行都要解码一次，而且永久保留时文件会膨胀好几倍。
- **编号缓存：** 打开数据库时把 `tuples` 整张表读进内存（编号 → `Arc<Dimensions>`，以及反向索引）。缓存放在适配器内部的 `Mutex` 里，加注释说明这是窄范围的实现细节。actor 是唯一调用方，所以实际不会有锁竞争。
- **查询端口**返回的是领域数据，不暴露编号：

```rust
pub trait TrafficStore: Send + Sync + 'static {
    fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>>;
    /// 一个事务内：写 meta、增删活跃连接、写入关闭产生的用量与明细，再按 `batch.prune` 清理。
    fn flush(&self, batch: &FlushBatch) -> TrafficResult<()>;
    /// `from` 起（含）每个维度组合在该级别上的用量合计，每个组合一行。
    fn usage(&self, tier: Tier, from: Option<u32>) -> TrafficResult<Vec<(Arc<Dimensions>, Usage)>>;
    fn closed_connections(&self, before: Option<&ClosedCursor>, limit: usize) -> TrafficResult<ClosedPage>;
    fn closed_count(&self) -> TrafficResult<u64>;
    /// 删除两张用量表都不再引用、也不在 `keep` 中的维度组合。
    fn collect_tuples(&self, keep: &[Arc<Dimensions>]) -> TrafficResult<u64>;
}

pub struct Prune {
    pub minutes_before: u32,
    pub hours_before: Option<u32>,      // None = 永久
    pub closed_before: Option<i64>,     // None = 永久
}
```

- **清理：** 每次落盘时在同一个事务里执行。按 key 的时间前缀做范围删除，没有过期数据时开销很小。
- **维度组合回收**最多每小时做一次。`keep` 传入活跃连接和待落盘缓冲里用到的维度组合。永久保留时小时表不会删除任何东西，回收直接跳过。

---

## 5. 查询（纯服务 `nyanpasu_traffic::query`）

### 5.1 统一聚合器

```rust
pub struct ReportRequest {
    pub query: TrafficQuery,
    pub rankings: Vec<Dimension>,          // 每个维度一张排名卡片
    pub ranking_limit: usize,              // 每张卡片的条数，上限与现在的 MAX_LIMIT 相同
    pub topology: Option<TopologyRequest>,
}
pub struct TopologyRequest {
    pub layers: Vec<Dimension>,            // 2..=5 层，不能重复
    pub metric: Metric,                    // Bytes | Connections
    pub limit_per_layer: Option<usize>,    // None = 不合并为"其他"（地图使用）
}
pub struct TrafficReport {
    pub total: Usage,
    pub current_rate: Option<Rate>,
    pub rankings: Vec<Ranking>,
    pub topology: Option<Topology>,
}
pub struct Ranking { pub dimension: Dimension, pub distinct: u64, pub groups: Vec<UsageGroup>, pub other: Usage }
pub struct UsageGroup { pub key: String, pub usage: Usage, pub current_rate: Option<Rate> }
```

- 输入是一组 `(Dimensions, Usage)` 行，可能来自存储、待落盘缓冲或活跃连接。聚合器对每一行判断是否通过筛选，再按请求的各维度累加。纯函数，只用普通数据做测试。
- **筛选：** 不同维度之间是 AND；同一维度只能有一个值（前端添加同维度的新筛选时替换旧值）。键为空串（例如没有策略组的 `Chain`）也可以作为筛选值。
- **排序：** 按 `bytes.total()` 从大到小，相同时按键排序（与现有分页游标规则一致）。`distinct` 是筛选后该维度不同取值的个数，用于数字卡片。
- **拓扑：** 把现在后端 `topology::project` 和前端 `buildTopology` 合并成一个纯函数：按层生成节点和边，节点 id 由（层序号，键）做 JSON 编码，避免不同层同名节点冲突（沿用现有做法）；每层按 `metric` 取前 N 个，其余合并成该层的"其他"节点，相关的边一并重定向到"其他"。`Chain` 层的键为空时，该路径跳过这一层，与现在"无策略组时省略组层"的行为一致。
- **速率：** 只有活跃连接有速率，按分组相加。范围为"已结束"时速率为 `None`。

### 5.2 范围与数据来源

| 范围     | 输入的行                                                           |
| -------- | ------------------------------------------------------------------ |
| `Closed` | `store.usage(tier, from)` + 待落盘缓冲（按同一级别、同一起点过滤） |
| `Active` | 内存中每条活跃连接：在对应级别上、起点之后的桶求和，连接数记 1     |
| `All`    | 以上两者的行一起输入聚合器                                         |

因为关闭时写入的就是该连接各个桶的流量，"已结束"和"活跃"永远不会重复统计同一个字节。G4 的等式据此成立，并用测试锁定。

### 5.3 对外接口

| `NyanpasuClient` 方法 / IPC 命令                             | 说明                                                                                                                       |
| ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------- |
| `traffic_summary()` / `get_traffic_summary`                  | 返回 `{ active_connections, closed_connections, current_rate, last_sample_at }`；`closed_connections` 为保留期内的明细条数 |
| `query_traffic_report(ReportRequest)`                        | 新增；流量页每次轮询只调这一个                                                                                             |
| `query_traffic_usage(TrafficQuery, Dimension, after, limit)` | 加上 `TrafficQuery` 参数，其余不变；"查看全部"弹窗分页使用                                                                 |
| `query_traffic_usage_by_keys(TrafficQuery, Dimension, keys)` | 加上 `TrafficQuery` 参数；规则页传 `{ range: All, scope: All, filters: [profile = 当前订阅] }`                             |
| `query_traffic_closed_connections(before, limit)`            | 不变；返回保留期内的连接，不区分订阅                                                                                       |
| `query_traffic_topology`                                     | 删除                                                                                                                       |

所有查询仍在 actor 内串行执行，先在阻塞线程上调用 `store.usage(..)`，再在 actor 里合并内存数据。查询耗时较长时，帧会在 `watch` 中合并，只是速率的采样间隔变长；计数器是累计值，增量不会丢。§9 记录了这一点。

---

## 6. actor 与依赖注入

- `TrafficArgs` 新增 `retention: Arc<dyn RetentionPolicy>` 和 `clock: Arc<dyn Clock>`（墙钟改为可注入，供测试控制分钟、小时边界）。`ProfileSelection` 保留，只用于给新连接标记订阅。
- `RetentionPolicy` 是消费方定义的端口，`fn retention(&self) -> Option<Duration>`。它的实现放在 `client/traffic.rs`，读取 `ApplicationClient::snapshot_handle()`，写法与 `SelectedProfile` 相同。
- 每次落盘时，actor 计算 `Prune`：分钟级 = 当前分钟 − 360；小时级、明细 = 当前时间 − 保留期限，永久时为 `None`。每次计算都重新读取设置，所以修改设置不需要 effect 计划，`impact.rs` 中按 `RuntimeImpact::None`、`owners: &[]` 登记即可。缩短保留期限后，超期数据在下一次落盘（≤ 30 秒）时删除。
- 维度组合回收：`flush` 返回是否删除过小时表数据；删除过即记为"待回收"，回收成功后才清除。待回收且距上次回收超过 1 小时时执行。"待回收"在启动时为真，因此每次启动后的第一次成功落盘会做一次对账回收，避免上次退出前未完成的回收被遗忘。
- 存储读取失败（`load()` 出错）时 actor 启动失败，组合根按"记录不可用"处理，与存储打不开相同，避免空会话覆盖已存的活跃连接。
- 落盘失败时，批次退回 `Session`，并且会话在下一次落盘成功前忽略新帧（速率清空）。内存只保留存活连接；计数器是累计值，恢复后的第一帧会补上期间的增量；在故障期间开始并结束的连接不计入。
- `post_stop` 落盘不变；启动时不再需要"订阅不同就清空"的分支，`load()` 读到的数据直接恢复。

---

## 7. 设置项

- `nyanpasu-config` 的 `NyanpasuAppConfig` 新增 `traffic_retention: TrafficRetention`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
pub enum TrafficRetention {
    #[serde(rename = "1d")] OneDay,
    #[serde(rename = "7d")] #[default] SevenDays,
    #[serde(rename = "30d")] ThirtyDays,
    #[serde(rename = "90d")] NinetyDays,
    #[serde(rename = "forever")] Forever,
}
```

- 旧配置文件里没有这个字段，按容器上已有的 `serde(default)` 取默认值，不需要迁移。
- 设置页"Nyanpasu"分区新增 `traffic-retention-selector.tsx`，结构照抄 `log-level-selector.tsx`。描述行显示当前值，并提示"缩短后，超出期限的流量记录会被删除，无法恢复；记录中包含访问过的主机"。

---

## 8. 前端：流量页（MDY）

### 8.1 页面骨架（与连接页一致）

```text
┌ ScrollArea  内容区 max-w-7xl mx-auto p-4 space-y-4 ─────────────────────┐
│  数字卡片  总流量(↑↓) · 连接 · 设备 · 入站 · 主机 · 出口                │
│  拓扑卡片  [流向 | 地图]  [流量 | 连接数]  [层级模板 ▾]                 │
│  排名卡片  热门设备 · 热门入站 · 热门主机 · 热门出口 · 热门进程        │
└──────────────────────────────────────────────────────────────────────┘
┌ 底部工具栏 h-16 bg-mixed-background px-4 gap-3 ─────────────────────────┐
│ [全部|活跃|已结束]  [筛选条件 chip … 横向滚动]  [最近 1 小时 ▾]  [⏸] │
└──────────────────────────────────────────────────────────────────────┘
```

- 去掉现在的页内大标题（eyebrow、h1、描述段落）。连接页和规则页都没有页内大标题；原来的说明文字改放到对应卡片的 Tooltip 里。
- 导航名称和页面标题改为"流量"，路由路径 `/main/topology` 不变。
- **范围**（全部/活跃/已结束）放在工具栏，对整页生效，包括数字卡片、排名和拓扑。控件与连接页的 `StatusTabs` 相同：`SegmentedButton size="sm"`。
- **时间范围**使用 `Select`，选项按 §3.2 的六档，默认"最近 1 小时"。超过当前保留期限的档位（"全部"除外）置灰并附说明。
- **暂停**：图标按钮加 Tooltip。暂停后停止轮询，保留当前数据（替代现在的"冻结快照"）。
- 页面状态全部放在 URL search 中（zod 校验）：`range`、`scope`、`filters`（`[{ d, v }]`）、`view`（`flow` / `map`）、`layers`（模板名）、`metric`、`limit`（每层节点数：5 / 7（默认）/ 10 / 20 / `all`，`all` 对应 `limit_per_layer = None`）。删除旧的 `proxy` 参数；当前没有其他页面链接到它。
- **轮询间隔**：时间范围 ≤ 24 小时时 2 秒，更长时 10 秒；规则页的累计流量查询改为 10 秒（实时数据仍来自 WebSocket）。

### 8.2 组件与令牌

| 元素      | 规范                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 数字卡片  | `Card`（basic），`p-4 flex gap-4`；图标容器 `size-12 rounded-2xl bg-secondary-container text-on-secondary-container`；标签 `text-xs text-on-surface-variant`；数值 `text-2xl font-medium tabular-nums`；总流量卡片第二行显示 ↑/↓。网格 `grid-cols-2 sm:grid-cols-3 xl:grid-cols-6`                                                                                                                                   |
| 排名卡片  | `Card`（basic），标题行：图标 + `text-base font-medium` 标题 + 不同取值个数徽标（与 `CountBadge` 写法相同）+ 文字按钮"查看全部"。网格 `grid-cols-1 md:grid-cols-2 xl:grid-cols-3`                                                                                                                                                                                                                                    |
| 排名行    | `<button>`，`rounded-2xl px-4 py-3`，悬停时 `hover:bg-on-surface/8`；第一名 `bg-secondary-container/60`，总量用 `text-primary`；已作为筛选条件的行用 `bg-secondary-container` 并设 `aria-pressed`。第一行：标签（截断，完整值放在 Tooltip）+ 总量；第二行 `text-xs text-on-surface-variant tabular-nums`：↑ 上传、↓ 下载、N 条连接（↑↓ 的写法沿用 `rule-row.tsx` 的 `text-outline`）。卡片底部一行"其他 N 项 · 流量" |
| 标签显示  | 进程显示文件名，完整路径放 Tooltip；设备（IP）用 `font-mono`；订阅显示订阅名称，已删除的订阅显示 uid 加"（已删除）"；`unknown` 和空串分别显示"未知""无策略组"                                                                                                                                                                                                                                                        |
| 筛选 chip | 页面内模块 `filter-chip.tsx`（只此一处使用，不放进 `components/ui`）。`h-8 rounded-lg border border-outline-variant px-3 text-sm`，内容为"维度名：值"，末尾是带 `aria-label` 的移除按钮；有筛选时末尾加一个"清除全部"文字按钮                                                                                                                                                                                        |
| 拓扑卡片  | 与其他卡片相同的 basic `Card`，沿用现有的 SVG 桑基图绘制（节点、连线、`motion` 动画、`prefers-reduced-motion`）。数据改为报表里的 `topology`。点击节点即添加该层维度的筛选，"其他"节点不可点击；悬停时只高亮与该节点相连的边（后端不再返回每条连接 id）。连线上不标数值。节点或连线超过 200 时不做动画、列顶部对齐，绘图区限高并在内部滚动，列标题吸顶                                                               |
| 层级模板  | `Select`：应用/来源 → 规则 → 策略链 → 出口（默认）；设备 → 主机 → 出口；入站 → 规则 → 出口；进程 → 主机 → 出口                                                                                                                                                                                                                                                                                                       |
| 地图      | 沿用 `GeographyView` 的绘制；数据来自报表的 `topology`，请求为 `layers = [SourceRegion, DestinationRegion, DestinationBasis]`、`limit_per_layer = None`。目标地区的合计取第二层节点；着色、占比列表和定位依据见 §11.6.9                                                                                                                                                                                              |
| 查看全部  | `Modal`，用 `query_traffic_usage` 分页（每页 50 条，滚动到底加载下一页），行组件与排名行相同，点击行添加筛选并关闭弹窗                                                                                                                                                                                                                                                                                               |
| 状态      | 记录不可用：`bg-error-container text-on-error-container rounded-2xl p-4`；当前范围没有数据：`text-on-surface-variant` 提示                                                                                                                                                                                                                                                                                           |

### 8.3 删除的前端代码

- `topology/_modules/topology.ts` 的 `buildTopology`，`geography.ts` 的 `buildGeography` 与 `connectionRegion`（只保留地区名称、坐标等展示用数据），以及 `tests/connection-topology.test.ts`；
- 拓扑页的全文搜索框、"连接详情"列表（前 8 条连接），由筛选条件加排名取代；
- 不再使用的 i18n key（`topology_eyebrow`、`topology_description`、`topology_detail_count` 等），新增 key 后运行 paraglide 编译。

---

## 9. 风险与已知限制

| 风险                                             | 处理                                                                                               |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------------- |
| 永久保留时，长时间范围的查询需要扫描整个小时表   | 先按编号合计再聚合（§4）；按 G9 测量并记录。超出可接受范围时，在后续 PR 增加天级粒度，不在本次范围 |
| 查询在 actor 内串行执行，会推迟处理帧            | `watch` 只保留最新帧，计数器是累计值，增量不会丢；只影响速率的采样间隔                             |
| 升级时 schema 从 v1 变为 v2，现有统计被清空      | 统计数据本来就按可丢弃处理（`redb.rs:6`），在 PR 描述中写明                                        |
| 采样缺口：两帧之间开始又结束的连接无法统计       | 现有限制，界面说明里保留"统计值，不是精确计费"的措辞                                               |
| 隐私：访问过的主机会保留到期限结束，或者永久保留 | 设置项描述中明示；本次不做"清空统计"按钮（§10）                                                    |
| `Chain` 维度的语义变更                           | 前端此前没有使用；变更在 IPC 类型注释中写明                                                        |

---

## 10. 不在范围

- 趋势折线图、自定义起止时间；
- 设备命名（IP → 名称映射）、按主域名合并主机；
- 全文搜索、导出、"清空统计"按钮；
- 天级粒度与历史数据压缩；
- 记录开关（存储打不开时仍按现在的方式禁用）。

---

## 11. 地理定位：接入 nyanpasu-geodata、定位依据与 DIRECT 出口

**日期：** 2026-10-02（本节为后续补充）
**状态：** §11.8 的探测接口已实现但未接入。其余部分是设计，按 §11.9 分成多个叠加 PR 实施。`nyanpasu-geodata`（#5537，`904993c89`）已经合入 main，但应用目前还没有依赖它。
**核对依据：** mihomo Alpha `9f053c49`。§11.1 表中的路径相对 mihomo 仓库，其余路径相对本仓库。

### 11.1 内核字段的实际语义

| 事实                                                                                                                             | 位置                                                                                                                          | 对地图的影响                                                         |
| -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `DstGeoIP` 只在 `GEOIP` 规则执行时写入。mmdb 模式写入完整查表结果；geodata 模式只追加命中的那个代码，未命中时保持 nil            | `rules/common/geoip.go:35-94`                                                                                                 | geodata 模式下它只说明命中了哪条规则，不能当作定位结果               |
| IP-CIDR、IP-SUFFIX、IP-ASN、GEOIP 规则以及 ipcidr 规则集都会触发 `ResolveIP`（使用 DefaultResolver）                             | `tunnel/tunnel.go:337-350`                                                                                                    | `destinationIP` 有值的连接比 `destinationGeoIP` 有值的多             |
| fake-ip 模式会清空 `DstIP`                                                                                                       | `tunnel/tunnel.go:296-299`                                                                                                    | 命中纯域名规则的连接，两个字段都为空                                 |
| TCP 只有命中 hosts 时才调用 `Pure()`，其余情况交给出站的是域名                                                                   | `tunnel/tunnel.go:561-567`                                                                                                    | 走代理的域名连接，`destinationIP` 是本地解析的结果，代理端会重新解析 |
| DIRECT 的 TCP 按 Host 拨号，使用 `DirectHostResolver`，不回写 `DstIP`；DIRECT 的 UDP 在 `ResolveUDP` 中回写 `DstIP`              | `adapter/outbound/direct.go:24-30`、`52-58`                                                                                   | DIRECT TCP 的实际对端以 `remoteDestination` 为准                     |
| `RemoteDst` 在 TCP 下取 socket 对端；在 UDP 下只取出站配置的地址（DIRECT 为空，代理可能是域名）                                  | `tunnel/statistic/tracker.go:120,212`；`adapter/outbound/base.go:232-243,312-315`                                             | 走代理时 `remoteDestination` 是节点入口，不是目标                    |
| Meta-geoip0 数据库可能返回多个标签；mihomo 默认 `geox-url` 下载的就是这种格式的 `geoip.metadb`，并写入已有的 `Country.mmdb` 路径 | `component/mmdb/reader.go:55-68`；`config/config.go:584`；`component/updater/update_geo.go:47-48`；`constant/path.go:126-145` | 现有的 `normalize_region` 遇到多个标签就判为未知（§11.6.7）          |

### 11.2 候选链

选出用于定位的 IP 后，统一用应用自己的国家索引（§11.5）查表。降级只在描述同一对象的证据之间进行。

**目标地区：**

| 顺序 | 条件                                                           | 定位用的 IP         | 依据       |
| ---- | -------------------------------------------------------------- | ------------------- | ---------- |
| 1    | 出口 `chains[0] == "DIRECT"`、TCP，`remoteDestination` 是 IP   | `remoteDestination` | `Dialed`   |
| 2    | 出口为 DIRECT、UDP                                             | `destinationIP`     | `Dialed`   |
| 3    | `host` 为空（客户端直接连接 IP），或 TCP 且 `dnsMode == hosts` | `destinationIP`     | `Dialed`   |
| 4    | 其余：出站拿到的是域名，`destinationIP` 有值                   | `destinationIP`     | `Resolved` |
| 5    | 以上都不满足                                                   | —                   | 未知       |

**源地区：** 用 `sourceIP` 查表。网关场景下的公网客户端可以由此定位；回环和内网地址查不到，结果为未知（本机位置见 §11.7）。

- 索引不可用时（内核尚未就绪、所在目录没有对应数据库、加载失败），退回内核给出的 `destinationGeoIP` / `sourceGeoIP`。内核的代码描述的是 `destinationIP`，依据按第 3、4 行判定。
- 用出口名判断 DIRECT 是近似做法：自定义的 `type: direct` 节点会落到第 4 行。结果偏保守，但不会标错。
- 以下证据描述的是别的对象，不用于目标地区：
  - 走代理时的 `remoteDestination`：它是节点入口；
  - 应用自行解析域名：TUN 加 fake-ip 时拿到的是 fake IP，其余情况得到的也只是客户端视角；
  - GeoSite：它是分类，不是位置；
  - 出口探测：只能按代理链缓存，不是逐连接的事实。

### 11.3 依据

- `Dimensions` 新增 `destination_basis: Option<GeoBasis>`，取值 `Dialed`（出站实际拨的就是这个 IP）或 `Resolved`（出站拿到的是域名，这个 IP 只是内核在规则匹配时本地解析的结果）。地区未知时为 `None`。
- 依据不包含字段名、查表方或 IP 本身：字段名是内核的实现细节；查表方不影响可信度；IP 会让维度组合数量暴涨。
- 依据由出口、协议、host 是否为空以及 `dnsMode` 决定，前三项已经在维度中，所以新增的组合很少。
- 新增 `Dimension::DestinationBasis`。地图请求三层拓扑 `[SourceRegion, DestinationRegion, DestinationBasis]`：地区节点仍按地区代码区分，第二层到第三层的边给出每个地区按依据拆分的用量（§11.6.9）。
- schema 从 2 升到 3，统计数据按可丢弃处理，直接清库。

### 11.4 服务拓扑

```text
组合根 setup.rs / NyanpasuClient::with_parts
│
├─ FsCountryIndexSource（适配器，core/geo/adapters.rs）
│    读 <app_data_dir> 里内核的国家库；镜像缓存在 <app_data_dir>/cache/geodata；监听国家库文件
│        ▲ load(mode, current) / watch(changed)
│        │
├─ GeoIndexActor + GeoIndexClient（actor，core/geo/）
│    独占：当前 geodata-mode、当前索引的内容键、watch::Sender<Option<Arc<IpIndex>>>
│    输入：follow_core 任务 ── CoreClientV2::api_client() → ApiClient::configs().geodata_mode
│          文件监听回调 ── FilesChanged
│        │ watch::Receiver<Option<Arc<IpIndex>>>（只读快照，单向往下游）
│        ▼
├─ TrafficActor 的 pump 任务（现有，core/traffic/actor.rs）
│    StreamsClient 的连接帧 ──▶ frame_from_snapshot(raw, wall, mono, index)
│        │                          └─ core/traffic/geo.rs（纯函数）locate_source / locate_destination
│        ▼
│    Session（nyanpasu-traffic）按含 destination_basis 的 Dimensions 记账
│
└─ DirectEgressProbe（已实现，§11.8）┄┄ 后续接入 GeoIndexActor，发布本机位置（§11.7）
```

| 组件                                      | 类别     | 位置                            | 职责                                                                                          |
| ----------------------------------------- | -------- | ------------------------------- | --------------------------------------------------------------------------------------------- |
| `GeoIndexActor` / `GeoIndexClient`        | actor    | `core/geo/actor.rs`、`mod.rs`   | 跟随内核实例读取 `geodata-mode`；收到模式或文件变化时在阻塞线程加载；只在内容变化时发布新索引 |
| `CountryIndexSource`                      | 端口     | `core/geo/ports.rs`             | `load`：按模式读取、哈希，打开或构建索引；`watch`：国家库文件被写入时回调                     |
| `FsCountryIndexSource`                    | 适配器   | `core/geo/adapters.rs`          | 发现文件、`read_source`、SHA-256、镜像缓存（mmap）、notify 监听                               |
| `locate_source` / `locate_destination`    | 纯服务   | `core/traffic/geo.rs`           | 候选链、查表、退回内核代码                                                                    |
| `CountryLookup`                           | 纯接口   | `core/traffic/geo.rs`           | `IpAddr → Option<String>`（大写 ISO 代码）；为 `IpIndex` 实现，测试用固定表实现               |
| `GeoBasis`、`Dimension::DestinationBasis` | 领域类型 | `nyanpasu-traffic/src/model.rs` | 依据的取值与分组键                                                                            |

- **为什么单独一个 actor：** 索引有自己的生命周期（文件监听、跟随内核实例、阻塞构建），和记账无关；记账存储打不开时 `TrafficClient` 为 `None`，索引仍应可用。把构建放进 `TrafficActor` 的处理器还会推迟帧处理。
- **为什么用 watch 发布快照，而不是每帧查询 actor：** 每帧有几百条连接，单次查表约 40 ns。`IpIndex` 是不可变的 `Send + Sync` 值，发布的是只读快照，不是共享的可变状态；依赖方向保持为单向的树。
- **不加载 ASN 与 GeoSite：** 地图只需要国家代码（§11.2 已排除 GeoSite）。

### 11.5 接入 nyanpasu-geodata

**读哪个文件。** 内核以 `-d <app_data_dir>` 启动（`backend/tauri/src/core/actor_v2/local_host.rs:22` 的 `working_dir`；服务模式使用同一个 `working_dir`），启动前 `init_resources` 会把内置的 `Country.mmdb`、`geoip.dat` 复制到这里（`utils/init/mod.rs:164-219`）。`MihomoGeoFiles::discover(home)` 按内核的规则解析文件名。

| `geodata-mode` | 文件                                                      | 构建                      |
| -------------- | --------------------------------------------------------- | ------------------------- |
| `false` 或缺省 | `ip_mmdb`（`Country.mmdb` > `geoip.db` > `geoip.metadb`） | `IpIndex::from_mmdb`      |
| `true`         | `geoip_dat`（`GeoIP.dat`）                                | `IpIndex::from_geoip_dat` |

**模式从哪里来。** 读取运行中内核的 `GET /configs` 的 `geodata-mode` 字段（`ApiClient::configs()`，clash-api `configs.rs:323`）。clash-rs 和 Clash Premium 不返回该字段，按 `false` 处理。每当内核绑定新实例时重新读取，写法与 `StreamsClient` 的 worker 相同（`core/clash/ws.rs:441-487`）：`core.api_client()` → `configs()` → `api.cancelled()`。内核回答之前没有模式，也就不加载；这时还没有连接。

**何时重新加载。**

1. 内核绑定新实例：模式可能变了，`init_resources` 也可能更新了文件。
2. 文件被写入：用 `notify-debouncer-full`（已是依赖，`state/profiles/scheduler.rs` 在用）非递归监听 `app_data_dir`，防抖 2 秒，只认四个国家库文件名（忽略大小写）。这覆盖了内核的 `geo-auto-update` 原地改写（mihomo `component/updater/update_geo.go:47-73` 先关闭自己的映射，再经 `component/resource/vehicle.go:38-47` 的 `os.WriteFile` 写回原路径），以及外部面板调用 `/configs/geo` 或 `/upgrade/geo`。应用本身不调用这两个 API（clash-api 的 `update_geo_databases` 目前没有调用方）。
3. 内容没变时不重建：适配器用 `read_source` 把文件复制到匿名内存（绝不映射内核的文件），计算 SHA-256；与当前索引的键相同就返回 `Unchanged`，不碰缓存。

**镜像缓存。**

- 目录：`paths.cache_dir().join("geodata")`，即 `<app_data_dir>/cache/geodata`。`cache_dir` 本来就标注为可清理。
- 文件名：`country-{mmdb|dat}-{sha256 十六进制}.idx`，按内容寻址，所以同名文件的内容永远相同，不会改写一个正在被映射的文件。
- 命中：只读打开（Windows 下共享模式为 `READ | DELETE`，不带 `WRITE`），用 `memmap2::Mmap` 映射，`IpIndex::from_bytes`。`from_bytes` 失败就删除该文件并重建。
- 未命中：在阻塞线程上构建；用 `tempfile::NamedTempFile::new_in(dir)` 写入 `as_bytes()`，再 `persist` 到目标名；然后丢弃构建出的索引，改从镜像重新打开，让页面成为可回收的文件页。
- 清理：切换成功后尽力删除同类的其他镜像，失败就忽略（例如 Windows 上仍被映射），下次切换时再试。

**失败语义。**

- `load` 出错：保留当前索引，记录 warn。
- `Missing`（目录里没有对应的数据库）：发布 `None`，按 §11.2 退回内核代码。
- 创建文件监听失败：记录 warn，只在内核实例变化时重新加载。

**成本。** 重新打开镜像约 3 ms，私有内存约 0.01 MiB；单次查表约 40 ns（均为 `nyanpasu-geodata/DESIGN.md` 的 Windows 实测）。其余数字是 2026-10-02 在 i9-14900KF（Windows 11，release，`opt-level = 's'`）上对内置数据库各测三轮的结果：

| 文件           | 大小     | 读取（冷 / 热） | SHA-256 | 构建   | 镜像     |
| -------------- | -------- | --------------- | ------- | ------ | -------- |
| `Country.mmdb` | 7.5 MiB  | 16.9 / 2.4 ms   | 2.8 ms  | 75 ms  | 4.71 MiB |
| `geoip.dat`    | 16.3 MiB | 20.9 / 5.2 ms   | 6.2 ms  | 240 ms | 5.22 MiB |

因此内容未变时，一次检查耗时在几毫秒到二十几毫秒之间。这颗 CPU 支持 SHA 指令，不支持的 CPU 上哈希会更慢。

**依赖。** `backend/tauri/Cargo.toml` 新增 `nyanpasu-geodata = { path = '../nyanpasu-geodata' }` 和 `memmap2 = '0.9'`（与 nyanpasu-geodata 相同的主版本）。`sha2`、`hex`、`tempfile`、`notify-debouncer-full` 已经是依赖。

### 11.6 关键代码

以下片段定义接口和数据流；内部实现细节以 PR 为准。

**11.6.1 端口**（`backend/tauri/src/core/geo/ports.rs`）

```rust
/// Which country database the core reads: `geodata-mode` in its running config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeodataMode {
    Mmdb,
    Dat,
}

/// What a published index was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexKey {
    pub mode: GeodataMode,
    pub sha256: [u8; 32],
}

pub enum Loaded {
    /// The database still has the content of `current`.
    Unchanged,
    /// The core's home has no database for this mode.
    Missing,
    Index { key: IndexKey, index: Arc<IpIndex> },
}

#[cfg_attr(test, mockall::automock)]
pub trait CountryIndexSource: Send + Sync + 'static {
    /// Blocking: reads and hashes the database, then reopens or builds its index.
    fn load(&self, mode: GeodataMode, current: Option<IndexKey>) -> Result<Loaded, GeoIndexError>;
    /// Calls `changed` after a country database in the core's home is written; watching
    /// ends when the returned guard drops.
    fn watch(&self, changed: OnChange) -> Result<Box<dyn Send>, GeoIndexError>;
}
```

**11.6.2 actor**（`backend/tauri/src/core/geo/actor.rs`）

```rust
pub struct GeoIndexArgs {
    pub source: Arc<dyn CountryIndexSource>,
    pub core: CoreClientV2,
}

pub(super) enum Message {
    /// A core instance answered `GET /configs`.
    Mode(GeodataMode),
    /// A country database in the core's home was written.
    FilesChanged,
}

pub(super) struct State {
    source: Arc<dyn CountryIndexSource>,
    mode: Option<GeodataMode>,
    key: Option<IndexKey>,
    published: watch::Sender<Option<Arc<IpIndex>>>,
    _watch: Option<Box<dyn Send>>,
    follower: JoinHandle<()>,
}

// handle(): every message ends in one reload of the current mode.
match message {
    Message::Mode(mode) => state.mode = Some(mode),
    Message::FilesChanged => {}
}
let Some(mode) = state.mode else { return Ok(()) };
let (source, current) = (state.source.clone(), state.key.clone());
match blocking::join(spawn_blocking(move || source.load(mode, current)).await) {
    Ok(Loaded::Unchanged) => {}
    Ok(Loaded::Missing) => {
        state.key = None;
        state.published.send_replace(None);
    }
    Ok(Loaded::Index { key, index }) => {
        state.key = Some(key);
        state.published.send_replace(Some(index));
    }
    Err(error) => tracing::warn!(%error, "the country index was not reloaded"),
}

/// Reads `geodata-mode` from every core instance as it binds.
async fn follow_core(actor: ActorRef<Message>, core: CoreClientV2) {
    loop {
        let Ok(api) = core.api_client().await else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        loop {
            match api.configs().await {
                Ok(config) => {
                    let mode = match config.geodata_mode {
                        Some(true) => GeodataMode::Dat,
                        _ => GeodataMode::Mmdb,
                    };
                    if actor.cast(Message::Mode(mode)).is_err() {
                        return;
                    }
                    break;
                }
                Err(_) if api.is_revoked() => break,
                Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
            }
        }
        api.cancelled().await;
    }
}
```

`pre_start` 调用 `source.watch(OnChange::new(move || { let _ = myself.cast(Message::FilesChanged); }))`，把返回的守卫保存在状态里。`post_stop` 中止 `follower`，并丢弃守卫。`GeoIndexClient::spawn(args, shutdown, &tasks)` 的签名与 `ProxiesClient::spawn` 相同；`GeoIndexClient::subscribe()` 返回 `watch::Receiver<Option<Arc<IpIndex>>>`。客户端不暴露其他方法。

**11.6.3 适配器**（`backend/tauri/src/core/geo/adapters.rs`）

```rust
pub struct FsCountryIndexSource {
    /// The core's home: `-d` of every core the app starts.
    home: PathBuf,
    /// Index images, named by the content they were built from.
    cache: PathBuf,
}

impl CountryIndexSource for FsCountryIndexSource {
    fn load(&self, mode: GeodataMode, current: Option<IndexKey>) -> Result<Loaded, GeoIndexError> {
        let files = MihomoGeoFiles::discover(&self.home)?;
        let Some(path) = (match mode {
            GeodataMode::Mmdb => files.ip_mmdb,
            GeodataMode::Dat => files.geoip_dat,
        }) else {
            return Ok(Loaded::Missing);
        };
        let source = read_source(&path)?;
        let key = IndexKey { mode, sha256: Sha256::digest(&*source).into() };
        if current.as_ref() == Some(&key) {
            return Ok(Loaded::Unchanged);
        }
        let image = self.cache.join(image_name(&key));
        // An unreadable image is rebuilt; `reopen` removes one `from_bytes` rejects.
        let index = match self.reopen(&image).unwrap_or(None) {
            Some(index) => index,
            None => {
                let built = match mode {
                    GeodataMode::Mmdb => IpIndex::from_mmdb(&source)?,
                    GeodataMode::Dat => IpIndex::from_geoip_dat(&source)?,
                };
                drop(source);
                // The image is only a cache: without it the built index serves.
                match self.store(&image, built.as_bytes()).and_then(|()| self.reopen(&image)) {
                    Ok(Some(reopened)) => reopened,
                    _ => built,
                }
            }
        };
        self.remove_other_images(&key);
        Ok(Loaded::Index { key, index: Arc::new(index) })
    }
    // watch(): notify-debouncer-full, NonRecursive on `home`, 2 s; calls `changed` when an
    // event names Country.mmdb, geoip.db, geoip.metadb or GeoIP.dat, ignoring ASCII case.
}
```

**11.6.4 纯函数**（`backend/tauri/src/core/traffic/geo.rs`）

```rust
/// A country for an address: an upper-case ISO code, as the map's markers are keyed.
pub(crate) trait CountryLookup {
    fn country(&self, ip: IpAddr) -> Option<String>;
}

impl CountryLookup for IpIndex {
    fn country(&self, ip: IpAddr) -> Option<String> {
        Some(self.lookup(ip)?.country()?.to_ascii_uppercase())
    }
}

/// What `destinationIP` is to the outbound: rows 2 to 4 of §11.2.
fn destination_ip_basis(meta: &ConnectionMetadataFields, exit: Option<&str>) -> GeoBasis {
    let tcp = matches!(meta.network, Some(ConfigEnum::Known(ConnectionNetwork::Tcp)));
    let hosts = matches!(meta.dns_mode, Some(ConfigEnum::Known(DnsMode::Hosts)));
    let dialed = (exit == Some("DIRECT") && !tcp)
        || meta.host.as_deref().is_none_or(str::is_empty)
        || (tcp && hosts);
    if dialed { GeoBasis::Dialed } else { GeoBasis::Resolved }
}

/// The address that stands for the destination, and its basis (§11.2).
fn destination(meta: &ConnectionMetadataFields, exit: Option<&str>) -> Option<(IpAddr, GeoBasis)> {
    let parse = |value: Option<&String>| value?.parse::<IpAddr>().ok();
    let tcp = matches!(meta.network, Some(ConfigEnum::Known(ConnectionNetwork::Tcp)));
    if exit == Some("DIRECT") && tcp {
        if let Some(peer) = parse(meta.remote_destination.as_ref()) {
            return Some((peer, GeoBasis::Dialed));
        }
    }
    let ip = parse(meta.destination_ip.as_ref())?;
    Some((ip, destination_ip_basis(meta, exit)))
}

pub(crate) fn locate_destination(
    meta: &ConnectionMetadataFields,
    exit: Option<&str>,
    index: Option<&dyn CountryLookup>,
) -> (String, Option<GeoBasis>) {
    let Some(index) = index else {
        // The core's codes describe destinationIP, whatever the outbound dialed.
        let region = normalize_region(meta.destination_geo_ip.iter().flatten());
        let basis = (region != UNKNOWN).then(|| destination_ip_basis(meta, exit));
        return (region, basis);
    };
    match destination(meta, exit).and_then(|(ip, basis)| Some((index.country(ip)?, basis))) {
        Some((region, basis)) => (region, Some(basis)),
        None => (UNKNOWN.to_owned(), None),
    }
}

pub(crate) fn locate_source(meta: &ConnectionMetadataFields, index: Option<&dyn CountryLookup>) -> String {
    match index {
        Some(index) => meta
            .source_ip
            .as_deref()
            .and_then(|ip| ip.parse().ok())
            .and_then(|ip| index.country(ip))
            .unwrap_or_else(|| UNKNOWN.to_owned()),
        None => normalize_region(meta.source_geo_ip.iter().flatten()),
    }
}
```

**11.6.5 帧转换与 pump**（`core/traffic/source.rs`、`core/traffic/actor.rs`）

```rust
pub(crate) fn frame_from_snapshot(
    frame: &ClashConnectionsFrame,
    wall_ms: i64,
    mono: Duration,
    index: Option<&dyn CountryLookup>,
) -> Frame

pub struct TrafficArgs {
    // ...existing fields
    /// The country index the regions are looked up in; `None` until the core's database loads.
    pub geo: watch::Receiver<Option<Arc<IpIndex>>>,
}

// pump(): the latest index at the moment each frame converts.
let index = geo.borrow().clone();
let frame = frame_from_snapshot(&raw, clock.now_ms(), origin.elapsed(), index.as_deref());
```

连接存活期间，如果地区随索引变化（例如索引在连接中途加载完成），关闭时按最终维度入账（`accounting.rs:328-356`），不会拆成两笔。

**11.6.6 组合根**（`setup.rs`、`client/mod.rs`）

```rust
// setup.rs, before ClientSetupArgs takes `paths`
let geo_index = Arc::new(FsCountryIndexSource::new(
    paths.app_data_dir().to_owned(),
    paths.cache_dir().join("geodata"),
));

// NyanpasuClient::with_parts, before TrafficClient::spawn
let geo = crate::core::geo::GeoIndexClient::spawn(
    crate::core::geo::GeoIndexArgs { source: geo_index, core: core_v2.clone() },
    shutdown.child_token(),
    &tasks,
)
.await?;
// TrafficArgs { ..., geo: geo.subscribe() }; NyanpasuClientInner keeps `geo` as
// `_geo_index`, since its actor stops with the last handle.
```

**11.6.7 领域类型**（`nyanpasu-traffic`）

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum GeoBasis {
    /// The outbound dialed this address.
    Dialed,
    /// The outbound was handed the host; the core resolved this address only for its rules.
    Resolved,
}

pub struct Dimensions {
    // ...existing fields
    /// How `destination_region` was located; `None` while it is unknown.
    #[serde(default)]
    pub destination_basis: Option<GeoBasis>,
}

// Dimension gains DestinationBasis; group_key maps it to "dialed", "resolved" or "".
// redb.rs: SCHEMA_VERSION = 3.
```

`normalize_region`（`bucket.rs:63`）改为与 `Tags::country()` 相同的规则：去重后恰好只有一个两字母 ASCII 代码时才采用，转为大写；否则为未知。

**11.6.8 前端：请求**（`topology/_modules/search.ts`）

```ts
// The map's third layer splits every destination region by how it was located.
view === 'map'
  ? {
      layers: ['source_region', 'destination_region', 'destination_basis'],
      metric,
      limit_per_layer: null,
    }
  : ...
```

`DIMENSIONS` 加入 `'destination_basis'`，使它可以作为筛选条件出现在 URL 中。

**11.6.9 前端：地图**（`topology/_modules/geography-view.tsx`）

- 地图数据：`deno task generate:topology-map` 改为输出 `sphere`（海洋）、`graticule`、`countries`（每个国家一条路径，按 `ISO_A2_EH` 取键，与 `centers` 相同）、`unassigned`（没有两字母代码的轮廓）和 `centers`。1:110m 轮廓里没有 HK、SG 这类小地区，它们仍用标记点。
- 着色：地区按当前指标的占比分五档（< 2%、≥ 2%、≥ 5%、≥ 10%、≥ 25%），在陆地色 `surface-variant` 上叠加主色 `primary`，不透明度为 0.2–1.0。由 `motion` 动画过渡 `fillOpacity`，地区出现和消失时淡入淡出。图例和列表里的色块用 `color-mix(in srgb, primary, surface-variant)` 算出同一种颜色。全部取自主题令牌，亮色和暗色模式都随主题变化。
- 国界线用 `surface` 色描边，在两种模式下都表现为一道缝隙。原来的 `stroke-outline-variant` 在暗色模式下与 `surface-variant` 同色（都是 `#43474e`），所以看不见国界。
- 依据：累加第 1 层到第 2 层的边，没有 `dialed` 用量的地区叠加斜线图案（同时用于小地区标记），列表中显示"本地解析"标签。
- 占比列表：卡片宽度 ≥ 1024px 时（容器查询 `@5xl`），列表放在地图右侧，高度与地图相同并可滚动；卡片较窄时，列表放在地图下方，完整列出全部地区。每行包括色块、名称、"本地解析"标签、用量、占比，以及占比条。点击某行即添加筛选；悬停时在地图上勾出对应国家，反之亦然。新行淡入，次序变化时行滑到新位置，占比条宽度也有过渡；离开的行直接移除：换一个筛选时列表几乎整个替换，逐行淡出看不出什么，却会让页面卡顿。行只在位次变化时测量布局（`layoutDependency`），次序不变的刷新不读取布局。
- 弧线：只取第 0 层到第 1 层的边，用三重方式表示方向：渐变从来源端的淡色过渡到目标端的实色；目标端有箭头，并画实心点，来源端画空心环；一个小点沿弧线从来源移向目标（开启减少动态效果时不显示）。小点不在 SVG 里：它们放在地图上方的 HTML 层，用 Web Animations 的 `transform` 关键帧沿同一条二次曲线匀速移动，由合成线程驱动，既不逐帧重绘整张地图，也不会在刷新占用主线程时停顿；地图移出视口时不渲染小点。悬停某个地区时，与它相关的弧线保持清晰，其余弧线变淡。选中某个目标时，它的来源地区以 `tertiary` 描边并标出名称。
- 不随数据变化的底图（海洋、经纬网、国界）只渲染一次。弧线粗细和小地区标记的半径按 0.5 取整，刷新时占比的微小变化不会触发动画和重绘。
- 没有弧线时（来源都是本机或局域网），地图下方用一行文字说明原因；着色和占比列表本身已经承载了主要信息。
- 覆盖率：第 2 层节点按键求和，文案为"实测 {dialed} · 本地解析 {resolved} · 共 {total} 条连接"。
- "只看实测"：标题栏中的 M3 筛选条（filter chip，选中时带对勾），添加或移除筛选 `{ d: 'destination_basis', v: 'dialed' }`，筛选 chip 显示"定位依据：实测"。
- `parseTraffic` 遇到 999.5–1000 之间的数值，原本会用 `toPrecision(3)` 输出 `1.00e+3`，现在改为输出整数。
- i18n：`traffic_dimension_destination_basis` 加在全部五种语言中；`topology_geo_basis_dialed`、`topology_geo_basis_resolved`、`topology_geo_dialed_only`、`topology_geo_region_count`、`topology_geo_share_legend`、`topology_geo_resolved_legend`、`topology_geo_no_routes` 以及修改后的 `topology_geo_coverage`、`topology_geo_caption`，只加在 en、zh-cn、zh-tw 中（ko、ru 原本就没有 `topology_geo_*` 这一组，会退回英文）。改完后运行 paraglide 编译。

### 11.7 本机位置（后续，不在本次叠加 PR 内）

来源是回环或内网地址的连接就是本机发起的，可以用 DIRECT 出口地址定位。接入时：

- `GeoIndexActor` 增加依赖 `Arc<dyn DirectEgressProbe>` 和 `StateSnapshot<ClashConfig>`（读取 TUN 设置），发布的内容从 `Option<Arc<IpIndex>>` 改为 `GeoView { country: Option<Arc<IpIndex>>, local: Option<String> }`。
- `locate_source` 对回环或内网的 `sourceIP` 返回 `local`；探测结果为 `TunEnabled` 或没有地址时，源地区保持未知。
- 待定：何时重新探测（应用目前没有网络变化的信号），以及 TUN 开启时是否保留上一次的结果。接入点在 `core/traffic/source.rs` 的 `TODO(traffic-geo)`。

### 11.8 DIRECT 出口探测接口（已实现，未接入）

| 项目     | 内容                                                                                                                                                     |
| -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 接口     | `NyanpasuClient::probe_direct_egress() -> DirectEgress`；RPC query `probe_direct_egress`，仅桌面端                                                       |
| 结果     | `TunEnabled`，或 `Probed { ipv4, ipv6 }`；某个地址族不可达、超时，或回显的内容不是该地址族的地址时，对应字段为 `None`                                    |
| 做法     | 端口 `DirectEgressProbe`，适配器 `HttpDirectEgressProbe`。应用自己发请求，并清空 reqwest 的全部代理设置，既不走系统代理也不走内核端口。每个请求限时 5 秒 |
| 回显服务 | DNSPod：`https://ipv4.ddnspod.com`、`https://ipv6.ddnspod.com`。两个主机名分别只有 A 记录、只有 AAAA 记录（2026-10-02 用 DoH 核实），返回纯文本地址      |
| TUN      | `enable_tun_mode` 打开时直接返回 `TunEnabled`，不发请求：TUN 会接管应用自己的请求并按规则分流，回显的可能是代理出口                                      |

没有采用的做法：

- 在运行配置中注入 `proxy: DIRECT` 的 `listeners`：mihomo 只能从 `listeners`、`tunnels` 或 provider 的 `proxy` 字段得到 `SpecialProxy`，不能按单个请求指定出站；而配置里只要出现非空的 `listeners`，runtime 就会判定 `InboundSurface`，禁用无缝切换（`nyanpasu-core-manager/src/config/mihomo.rs:437-443`）。
- 绑定物理网卡：reqwest 的 `interface()` 不支持 Windows。
- ipw.cn：截至 2026-10-02，整个区已经没有 A/AAAA 记录。

已知限制：

- 结果是"最近一次探测到的 DIRECT 出口"。多出口、策略路由或校园网环境下，访问不同目的地的出口可能不同；国内回显服务测到的是国内路径的出口。
- 接口不做缓存，由调用方决定。
- 判断 TUN 依据的是设置项，不是实际运行状态：TUN 启动失败时探测也会被拒绝。

### 11.9 交付拆分（叠加 PR）

全部完成后统一创建，每个 PR 以前一个为 base：

| #   | 分支                             | 内容                                                                                                    |
| --- | -------------------------------- | ------------------------------------------------------------------------------------------------------- |
| 1   | `feat/direct-egress-probe`       | §11.8                                                                                                   |
| 2   | `docs/traffic-geolocation`       | 本节                                                                                                    |
| 3   | `fix/traffic-region-country-tag` | `normalize_region` 的两字母规则及测试                                                                   |
| 4   | `feat/geo-country-index`         | `core/geo`（端口、适配器、actor）、依赖、组合根；流量按索引查 `sourceIP`、`destinationIP`，退回内核代码 |
| 5   | `feat/traffic-destination-basis` | 目标地区候选链、`GeoBasis`、`Dimension::DestinationBasis`、schema 3                                     |
| 6   | `feat/traffic-map-basis`         | 前端：地图第三层、按占比着色与占比列表、覆盖率拆分、斜线标记、方向弧线、"只看实测"、i18n                |
| 7   | `feat/traffic-mock-report`       | 开发构建的调试开关：流量页改用前端生成的报表，与连接页的 mock 开关相同                                  |

base 不是 main 的叠加 PR 默认不触发 CI，需要加入 GitHub 原生 stack，或在合入前改 base 后补跑。

### 11.10 判据

| 输入                                                         | 期望                                                | 所在 PR   |
| ------------------------------------------------------------ | --------------------------------------------------- | --------- |
| `normalize_region(["google", "us"])`                         | `US`                                                | 3         |
| `normalize_region(["private"])`、`["cn", "us"]`              | 未知                                                | 3         |
| 适配器：同一内容第二次加载                                   | `Unchanged`，不访问缓存目录                         | 4         |
| 适配器：新实例、同一缓存目录                                 | 从镜像重新打开，不新建镜像文件                      | 4         |
| 适配器：镜像损坏                                             | 删除后重建，结果与首次构建一致                      | 4         |
| 适配器：模式由 mmdb 切到 dat                                 | 键不同，索引来自 `GeoIP.dat`                        | 4         |
| actor：尚未收到模式时收到 `FilesChanged`                     | 不调用 `load`                                       | 4         |
| actor：`load` 返回 `Unchanged`                               | 不重复发布                                          | 4         |
| pump：发布新索引之后的帧                                     | 按新索引定位                                        | 4         |
| DIRECT、TCP，`destinationIP` 为空，`remoteDestination` 是 IP | 按 `remoteDestination` 定位，`Dialed`               | 5         |
| 走代理，有 host，有 `destinationIP`                          | `Resolved`                                          | 5         |
| 走代理，只有 `remoteDestination`                             | 未知（不使用节点入口）                              | 5         |
| 走代理，host 为空，有 `destinationIP`                        | `Dialed`                                            | 5         |
| 没有索引，内核给出 `["us"]`，走代理且有 host                 | `US`，`Resolved`                                    | 5         |
| 地图：某地区只有 `resolved` 用量                             | 斜线填充，列表标"本地解析"；覆盖率分别计数          | 6         |
| 地图：打开"只看实测"                                         | 添加 `destination_basis = dialed` 筛选，chip 可移除 | 6         |
| 探测：`enable_tun_mode` 为真                                 | `TunEnabled`，适配器未被调用                        | 2（已有） |
