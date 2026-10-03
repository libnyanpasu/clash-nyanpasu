# 规则 / 连接 / 流量页 跨页互查 设计

**日期：** 2026-10-03
**基线：** `main @ cc21cbd31`
**来源：** 跨页互查需求讨论（2026-10-03）。已定：跳转式互查；返回采用"返程票"；connections 与 traffic 共用查询词汇（含 `All`）；筛选结果与来源数字精确一致（需后端改动）。
**范围：** 三页之间的跳转入口、跳转后的返回、connections 页按流量维度筛选与"全部"视图、后端按维度列出已结束连接与活跃连接 id。
**不在范围：** traffic 页滚动位置恢复；connections 页时间范围选择器；代理侧栏（`proxy`）与流量维度合并；Alt+← / 鼠标侧键的自定义处理。

---

## 1. 目标与成功标准

- 从任一页的相关数字跳到另一页，看到的集合与来源数字一致：
  - rules 行的活跃连接数 = 跳到 connections（活跃）后的行数；
  - rules 行的累计流量 = 跳到 traffic 后的总量（同口径：当前 profile、全部时间）；
  - traffic 的 `{scope, range, filters}` = 跳到 connections 后对应 tab 的计数。
- 跳转后能一步回到来源页的原状态：URL 条件、排序、搜索，并定位、高亮来源行。
- 来源页之间可以链式跳转、逐级返回。

## 2. 返回机制："返程票"

- 跳转时在**目标**历史记录的 `history.state` 写入 `returnTo: { page, href, index }`，`index` 为来源记录的 `__TSR_index`。
- 若来源有需要定位的条目，先以 `replace` 把 `focus` 写入**来源**记录的 state，再 push 目标。
- 返回：`delta = ticket.index − 当前 __TSR_index`，`delta < 0` 时 `history.go(delta)`，一次弹回来源记录（目标页内的 tab / 筛选 push 一并弹出）；否则回退到 `navigate({ href })`。
- 目标页内的导航（切 tab、改筛选、改排序、侧栏）用 `state: keepReturn` 携带返程票，丢弃 `focus`。
- 页面挂载时读取一次本记录的 `focus`：滚动到该条目、居中并高亮约 2 秒。
- 页面状态进 URL：rules 的 `q`、`sort`，connections 的 `q`（输入防抖，`replace`）。
- 路由动画：`__TSR_index` 变小视为后退，向后滑。

**界面：** 三页底部工具栏最左侧放 `ReturnButton`（"‹ 返回规则"），无返程票时不渲染；窄宽度只显示图标 + tooltip。跨页带来的条件以 FilterChip 展示，可逐个移除，不影响返回。

## 3. connections 与 traffic 对齐

- search 参数：`scope: all | active | closed`（默认 `active`，替代 `status`）、`range`、`filters: [{d, v}]`、`q`、`proxy`（原侧栏，不变）。
- tab 顺序与 traffic 一致：全部 / 活跃 / 已关闭。
- **全部**：活跃行在上、已关闭行按 `closed_at` 倒序在下（"最近活动优先"，与已关闭分页顺序一致，翻页只追加）；列为两者并集，不适用的单元格留空；同一 id 同时出现时以活跃为准。
- 有 `filters` 时：活跃行 = 实时 Clash 流 ∩ 后端返回的匹配 id；已关闭行 = 后端按 `range` + `filters` 筛选的分页。`range` 只作用于已关闭（与 traffic 报表一致：活跃连接不论时间范围都计 1）。
- tab 计数：无条件时沿用现状；有条件时 活跃 = 匹配 id 数，已关闭 = traffic 报表（`scope: closed`、同 range / filters）的 `total.connections`，全部 = 二者之和。
- `range` 仅以 chip 出现（由 traffic 带来），connections 不新增时间选择器。

## 4. 后端

- `nyanpasu-traffic`：
  - `ClosedSelection { since_ms, filters }`，`matches()` 用 `group_key`，与报表同一匹配器。
  - `TrafficStore::closed_connections(before, limit, selection)`：倒序扫描，跳过不匹配项；早于 `since_ms` 即停止；单次扫描上限 `MAX_CLOSED_SCAN`，未扫完时游标停在最后扫描位置（页可以为空但带 `next`）。
  - `merge_closed_page` 对未落盘的已结束连接同样筛选；未截断时 `next` 沿用存储页的游标。
  - `Session::active_ids(filters)`。
  - `TrafficRange::start_ms(now)`：报表起始桶的毫秒值；已结束连接按关闭时刻所在桶计入报表，因此 `closed_at >= start_ms` 与报表口径一致。
- `TrafficActor` / `TrafficClient` / `NyanpasuClient` / IPC：
  - `query_traffic_closed_connections(range, filters, before, limit)`（破坏性修改，唯一调用方随之迁移）；
  - 新增 `query_traffic_active_connection_ids(filters) -> Vec<String>`，查询类，`rpc(http)`，与其他 traffic 查询一致。

## 5. 入口

| 来源                        | 入口           | 目标                                                                |
| --------------------------- | -------------- | ------------------------------------------------------------------- |
| rules 行（同 label 的首条） | 活跃连接数徽标 | connections `scope=active, filters=[rule]`，来源 focus = 规则 label |
| rules 行（同 label 的首条） | 累计流量       | traffic `range=all, scope=all, filters=[profile=当前, rule]`        |
| traffic 工具栏              | 「查看连接」   | connections `{scope, range, filters}`                               |
| connections 详情弹窗        | 「定位规则」   | rules，目标 focus = 规则 label                                      |
| connections 详情弹窗        | 「查看用量」   | traffic `filters=[rule]`                                            |

## 6. 已知限制

- traffic 页返回后滚动回到顶部（工具栏入口不需要定位）。
- 报表与已结束连接列表的清理粒度不同（小时桶 vs 毫秒），保留期边缘可能相差个位数。
- 活跃连接 id 每秒轮询一次，新连接可能晚一个周期出现在筛选结果中。
