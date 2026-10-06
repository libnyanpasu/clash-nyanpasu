# Clash GUI 流量拓扑与归因调研

调研日期：2026-09-16。通过三个 subagent 分别检查桌面 GUI、Web dashboard、原生客户端与流量账本；主线程补查 Singboard 和本仓库。依据公开源码及官方资料，没有运行这些第三方 GUI 做交互或性能测试。源码快照结论不保证对应功能已进入正式发行版。

## 范围与术语

- **连接拓扑**：把来源、命中规则、策略组和出口的逻辑关系画出来。不是 traceroute，也不代表物理网络逐跳路径。
- **流量归因**：把上传／下载字节归入进程、设备、目标、规则或出口，可以是排行、趋势图或桑基图。
- **统计口径**：连接数、连接累计字节、时间窗口增量和瞬时速率不同；图的线宽不一定代表字节占比。

## 已确认的实现

### Zashboard：Vue + ECharts 桑基图

检查 commit：`50c873c8d75da33222391e265c311657be3c8091`。

- Vue 3、Vite、Tailwind/daisyUI；图形使用 ECharts 6 的 `SankeyChart` 与 `CanvasRenderer`。
- 将连接聚合为「来源 → 规则 → 代理入口 → 代理出口」。入口取 `chains.at(-1)`，出口取 `chains[0]`；两者相同则省略入口。
- 节点按层与名称区分；边按连接数累计，显示权重为 `log10(count + 1) * 10`，提示保留原始计数。
- 因此是连接分布桑基图，不是按字节比例绘制的流量图；只取链首尾也不等于完整嵌套策略链。

源码：[聚合函数](https://github.com/Zephyruso/zashboard/blob/50c873c8d75da33222391e265c311657be3c8091/src/components/overview/topology.ts)、[图组件](https://github.com/Zephyruso/zashboard/blob/50c873c8d75da33222391e265c311657be3c8091/src/components/overview/TopologyCharts.vue)、[图形注册](https://github.com/Zephyruso/zashboard/blob/50c873c8d75da33222391e265c311657be3c8091/src/composables/useEChart.ts)。

### Singboard：Tauri + Vue + ECharts 桑基图

这是 sing-box 桌面 dashboard，通过 Clash API 获取数据，作为兼容生态参考。检查 commit：`5728251b5db5b83c5900701a5d9f6603fe374740`。

- Tauri 2、Vue 3、Vite；拓扑组件直接使用 ECharts 5.6 的 `SankeyChart` 与 `CanvasRenderer`。
- `/connections` WebSocket → Vue connections store → computed 聚合 → `setOption()`。
- 「来源 IP → rule + rulePayload → chains 最后一项 → chains 第一项」；入口等于出口时省略入口。
- 边按连接数累计，并以 `log10(count + 1) * 10` 显示；支持路径高亮、渐变连线、暂停与容器 resize。
- 当前实现以名称作为节点去重键；如果借鉴，应注意跨层同名节点碰撞。该风险是源码分析推断，未做运行复现。

源码：[TopologyChart.vue](https://github.com/Yuu518/singboard/blob/5728251b5db5b83c5900701a5d9f6603fe374740/src/components/overview/TopologyChart.vue)、[connections store](https://github.com/Yuu518/singboard/blob/5728251b5db5b83c5900701a5d9f6603fe374740/src/stores/connections.ts)、[依赖](https://github.com/Yuu518/singboard/blob/5728251b5db5b83c5900701a5d9f6603fe374740/package.json)。

### Clash Party（原 Mihomo Party）：拓扑与历史归因分开实现

检查 `smart_core` 分支 commit：`5ace1da9c87ad04d2f4ab1f77697b24c6e1f5e1c`。

- Electron + React；拓扑使用 D3 7.9 + SVG 自定义分层绘制。
- 拓扑按「代理组 → 代理节点 → 规则 → 来源 IP → 来源端口」展示当前活跃连接；取 `chains[0]` / `chains[1]`，不保留完整多层链。
- 累计 `upload + download` 用于统计信息，但线宽按连接数，不能把视觉宽度解释成字节比例。
- 历史归因：`/connections` → `recordTrafficUsage` → `TrafficUsageAccumulator` 按连接 ID 差分 → 来源 IP、host、outbound、process 四维分钟桶 → `node:sqlite` worker 持久化。
- 历史统计页使用排行／下钻表格和 Chart.js 趋势图，不是另一张桑基图。

源码：[D3 拓扑](https://github.com/mihomo-party-org/clash-party/blob/5ace1da9c87ad04d2f4ab1f77697b24c6e1f5e1c/src/renderer/src/components/network/network-topology.tsx)、[差分与归因](https://github.com/mihomo-party-org/clash-party/blob/5ace1da9c87ad04d2f4ab1f77697b24c6e1f5e1c/src/shared/trafficUsage.ts)、[SQLite worker](https://github.com/mihomo-party-org/clash-party/blob/5ace1da9c87ad04d2f4ab1f77697b24c6e1f5e1c/src/main/traffic/database-worker.ts)、[趋势图](https://github.com/mihomo-party-org/clash-party/blob/5ace1da9c87ad04d2f4ab1f77697b24c6e1f5e1c/src/renderer/src/components/traffic/traffic-trend-chart.tsx)。

### MetaCubeXD：当前已迁移 Nuxt/Vue，拓扑使用 D3

检查 commit：`8bbc8f58fef71148a94fb5c0ff808f79b057337d`。当前框架是 Nuxt 4 + Vue 3 + Pinia，不能沿用旧版 SolidJS 的结论。

- `d3.hierarchy` + `d3.tree` + SVG 实现可折叠树；普通时间曲线另用 Highcharts 13。
- 层次为「代理组 → 出口节点 → 规则 → 来源 IP → 源端口」。组取 `chains[1]`，缺失时回退 `[0]`；出口取 `[0]`。
- 每层累计连接数和 `upload + download`，没有重建完整代理链。
- 历史归因按连接计数增量记录，IndexedDB 保存时间、来源 IP、host、outbound、process、inboundUser、上传和下载，再按维度汇总和钻取。

源码：[拓扑](https://github.com/MetaCubeX/metacubexd/blob/8bbc8f58fef71148a94fb5c0ff808f79b057337d/packages/ui/components/NetworkTopology.vue)、[差分](https://github.com/MetaCubeX/metacubexd/blob/8bbc8f58fef71148a94fb5c0ff808f79b057337d/packages/ui/stores/connections.ts)、[IndexedDB](https://github.com/MetaCubeX/metacubexd/blob/8bbc8f58fef71148a94fb5c0ff808f79b057337d/packages/ui/utils/db.ts)、[归因查询](https://github.com/MetaCubeX/metacubexd/blob/8bbc8f58fef71148a94fb5c0ff808f79b057337d/packages/ui/composables/useDataUsage.ts)。

### ClashMac：SwiftUI 原生，具体图形库不可确认

- 官方明确使用 SwiftUI，客户端闭源。
- 当前官方手册描述「本地 → 代理国家 → 目标国家」飞行地图，以流量改变航线宽度，并提供实时和历史视图。
- 官方公告也提到连接拓扑；旧 `26.3-beta.9` release 地址已不可访问，不用转载证明其具体实现。
- 第三方清单说明独立 Mihomo 进程与 HTTP API 通信。清单包含 Zashboard 和 GRDB.swift 等，不能据此认定原生拓扑的渲染或存储实现。

来源：[官方仓库](https://github.com/666OS/ClashMac)、[地图手册](https://clashmac.app/guide/dashboard/radar_map)、[作者公告](https://github.com/666OS/ClashMac/issues/197)、[第三方清单](https://github.com/666OS/ClashMac/blob/main/THIRD_PARTY_LICENSES.txt)。

### Mihomo Traffic Ledger：Go + SQLite 的历史归因账本

- 配合 Mihomo / Clash Verge Rev 的独立工具，提供应用、设备、目标和出口排行及下钻，不是节点拓扑 GUI。
- Go `net/http` + `embed`，Windows 托盘及命名管道，`modernc.org/sqlite`。
- 前端原生 HTML/CSS/JavaScript；趋势曲线使用手写 Canvas 2D，排行和链路详情使用 DOM。
- 默认每秒读取 `/connections`，将累计计数的差值归入进程、目标、规则、代理链和出口。
- 会话键包含连接 ID 和启动时间；区分启动前已有连接与新连接，处理计数器重置，并持久化会话和汇总。

源码：[项目说明](https://github.com/severin-ye/mihomo-traffic-ledger)、[采集与服务](https://github.com/severin-ye/mihomo-traffic-ledger/blob/main/main.go)、[账本](https://github.com/severin-ye/mihomo-traffic-ledger/blob/main/connection_ledger.go)、[Canvas 图表](https://github.com/severin-ye/mihomo-traffic-ledger/blob/main/web/app.js)。此项目引用默认分支，后续复用前应固定 commit。

## 未找到拓扑的已检查项目

- **Clash Verge Rev**：检查 `f58e0a0af573cfce0aedfb5297694869cd46d462`，确认 Tauri + React + MUI、连接表格和手写 Canvas 速度曲线。当前前端未找到拓扑或 Sankey。[连接页](https://github.com/clash-verge-rev/clash-verge-rev/blob/f58e0a0af573cfce0aedfb5297694869cd46d462/src/pages/connections.tsx)、[速度图](https://github.com/clash-verge-rev/clash-verge-rev/blob/f58e0a0af573cfce0aedfb5297694869cd46d462/src/components/layout/traffic-graph.tsx)。
- **Clash Nyanpasu 上游**：检查 `d8547a9fb65b571767ba9481bd1f14a09bfdfee3`，确认连接表格及 D3/SVG sparkline，未找到拓扑。[sparkline](https://github.com/libnyanpasu/clash-nyanpasu/blob/d8547a9fb65b571767ba9481bd1f14a09bfdfee3/frontend/nyanpasu/src/components/ui/sparkline.tsx)。

上述“未找到”仅针对当前检查的前端源码、依赖和相关页面，未覆盖历史分支、未合并 PR 或插件。Yacd/Yacd-meta 未完成源码核实，不作功能判断。

## 数据处理结论

Mihomo 的 `/traffic` 提供总体流量；归因需要 `/connections` 的 `id`、metadata、`chains`、`rule`、`rulePayload`、`upload` 和 `download`。见 [官方 API](https://wiki.metacubex.one/api/)。

以下是对已查源码的归纳及实现建议：

1. 实时连接拓扑可从最新快照纯计算得到；历史归因还需要采集生命周期、差分和持久化。
2. 不能把每帧的累计字节重复相加。窗口增量取相邻样本差值，速率再除以真实采样间隔。
3. `/connections` 是采样快照；两次采样之间产生并结束的连接、断线期间的连接及最后一段字节可能无法完整归因。不要宣称它是精确计费账本。
4. `process` 等 metadata 可能缺失，需要明确的未知分类；不能从来源 IP 自动推断真实应用。
5. 代理组选择关系、实际连接使用的 chains 和物理网络跳数应分开；不能用当前策略组选择反推历史连接出口。
6. 同一字节会沿多条逻辑边显示，不能把所有层的边权再求和当作总流量。

## 对 Nyanpasu 的初步建议

本 worktree 基于 `bbd00932e`；前端已有 React 19、D3 7.9、TanStack Table/Virtual。当前连接页面是虚拟表格，使用 `useClashConnections()` 的采样数据；已有 `chains`、规则、process、来源和目标字段。D3 当前用于 sparkline，尚未看到本分支的拓扑页面。

- 若重点是流向总览，参考 Zashboard 的纯聚合函数与 ECharts 桑基图。
- 若重点是适配现有 React/D3 技术栈及自定义节点交互，参考 Party 的 D3 + SVG；是否采用它需要后续原型验证。
- 若重点是历史归因，优先参考 Party / Ledger 的采集和差分设计，图形库是独立选择。
- 如进入实现阶段：长生命周期采集／历史状态归 actor；快照到图的聚合归 pure service；存储归 adapter；经 `NyanpasuClient` 暴露应用 API。复用现有注入的连接流能力。

本次仅建立 worktree 与调研文档，未安装依赖、构建应用或实现功能。`sidecar` 与 `resources` 已按仓库规则链接回主 checkout。

## 实现进展与实时／历史口径（续）

上述初步建议之后，worktree 已实现实时连接拓扑，并补充以下功能：

- 使用现有 `motion/react` 平滑更新节点位置、连线形状／粗细和进出场；媒体查询变化立即停用位置动画。
- 地理视图使用内核 `sourceGeoIP` / `destinationGeoIP`，只绘制可定位的地区和已知两端的示意连线。未知、歧义或不支持的地区保留在未知统计中。
- 底图与 239 个地区标记位置离线打包；不查询外部定位服务，不从节点名称／国旗推测出口位置。代理出口仍未定位，标记也不代表服务器精确经纬度。
- 搜索或代理筛选变化时清除旧快照和选择，保留地图／路径视图选择。

当前实现仍然只统计活跃连接累计字节，没有把历史快照重复累加，也没有新增历史账本。

| 面板        | 实时拓扑／地图                                             | 历史连接与流量                                                 |
| ----------- | ---------------------------------------------------------- | -------------------------------------------------------------- |
| Zashboard   | 明确读取 `activeConnections` / `filteredActiveConnections` | 不能从其他历史列表推断拓扑包含历史；当前检查的拓扑只用活跃连接 |
| Clash Party | 活跃连接分组树                                             | 独立的差分归因、SQLite 历史统计                                |
| MetaCubeXD  | `connectionsStore.activeConnections`                       | 有关闭连接列表；另用 IndexedDB 保存流量增量供归因统计          |
| ClashMac    | 实时地图读取活跃连接                                       | 官方手册描述今天／本周／本月的历史流量地图                     |

补查依据：[Zashboard 数据源](https://github.com/Zephyruso/zashboard/blob/50c873c8d75da33222391e265c311657be3c8091/src/components/overview/TopologyCharts.vue)、[MetaCubeXD 拓扑数据源](https://github.com/MetaCubeX/metacubexd/blob/8bbc8f58fef71148a94fb5c0ff808f79b057337d/packages/ui/components/NetworkTopology.vue)、[MetaCubeXD 连接与增量记录](https://github.com/MetaCubeX/metacubexd/blob/8bbc8f58fef71148a94fb5c0ff808f79b057337d/packages/ui/stores/connections.ts)、[ClashMac 地图时间范围](https://clashmac.app/guide/dashboard/radar_map)。

下一步若实现历史，应区分：

1. 实时连接视图：当前还存活的连接及它们的累计字节。
2. 历史连接列表：曾经观察到的连接及最后一次计数，不等于按时间窗口的流量。
3. 历史流量归因：按会话 ID 差分、按时间桶累计与持久化；需要处理启动前连接、重启、计数器重置和采样缺口。由 actor 持续采集和持有历史状态，经存储 adapter 持久化。
