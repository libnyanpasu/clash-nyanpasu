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

| 字段                                   | 来源                                                                               | 未知时    |
| -------------------------------------- | ---------------------------------------------------------------------------------- | --------- |
| `process`                              | `process_path`，否则 `process`；`\` 统一换成 `/`                                   | `unknown` |
| `source`                               | `source_ip`（界面上称"设备"）                                                      | `unknown` |
| `inbound`                              | `inbound_user`，为空时取 `inbound_name`                                            | `unknown` |
| `target`                               | `host`，否则 `destination_ip`（界面上称"主机"）                                    | `unknown` |
| `protocol`                             | `network`                                                                          | `unknown` |
| `rule`                                 | `RuleKey { kind, payload }`                                                        | —         |
| `chains`                               | clash 原始顺序：出口在前，最外层组在后                                             | 空        |
| `profile`                              | 连接**首次出现时**的当前订阅 uid，此后不再改变                                     | `None`    |
| `source_region` / `destination_region` | GeoIP 代码去重并转大写；恰好一个时取该值，否则未知（逻辑从 `geography.ts` 移过来） | `unknown` |

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
| 地图      | 沿用 `GeographyView` 的绘制；数据来自报表的 `topology`，请求为 `layers = [SourceRegion, DestinationRegion]`、`limit_per_layer = None`。目标地区的合计取第二层节点                                                                                                                                                                                                                                                    |
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
