# 第二阶段：连接明细按需推送与 proxies 负载精简 设计

**日期：** 2026-09-29
**基线：** `main @ 218fd4b9d`（已含第一阶段 #5423）
**来源：** UI 卡顿审计 `.claude/reviews/2026-09-29-ui-jank-performance-audit.md` §8 第二阶段（N2、N4、N9）
**范围：** 轨道 A（Clash 连接流：汇总常驻、明细按需）；轨道 B（`get_proxies` 负载归一化）。两条轨道互不依赖，各自成 PR，A 先行。
**不在范围：** 日志流批量合并（N9 的日志部分，见 §6）；`ProxiesActor` 的 3 s 缓存、10 s 强制刷新与指纹计算（只耗 Rust 侧 CPU，不经过 WebView）；前端 `useFlushSync` 调整。
**权威顺序：** `AGENTS.md` > `docs/design/actor-migration-roadmap.md` > 本设计 > 实施计划

---

## 0. 结论摘要

1. **连接流拆成两路。** 汇总（总流量、总速率、内存、连接数、按链路成员聚合的速率）随每个样本推给所有订阅方，大小与连接数无关。明细（逐条连接）只在有界面订阅时才构建、只推给订阅它的 webview，并且只保留最新一帧。
2. **速率在 Rust 里算。** 逐连接速率和按链路成员（分组、节点）聚合的速率，由一个纯服务从相邻两个原始样本推导。前端不再保留两份样本做差，现有的两套做差代码（connections 页、proxies 分组页头）删除。
3. **任何一端都不再保留明细历史。** Rust 只保留上一个原始样本用于算速率；`Reset` 快照和 `get_clash_ws_snapshot` 不含明细，大小由日志上限决定，与连接数无关。
4. **明细订阅的生命周期由适配层按 webview label 显式管理。** Tauri `Channel::send` 在页面重载后、甚至窗口关闭后仍返回 `Ok`（§1.3），不能靠发送失败发现订阅方消失。订阅在"显式退订""该 webview 开始加载新页面""该 webview 销毁"三种情况下结束。
5. **StreamsActor 不接触 Tauri。** 明细经 `tokio::sync::watch` 发布；actor 用 `receiver_count()` 判断是否有人需要明细，没人订阅时不做逐条 JSON 转换。Tauri `Channel` 转发与订阅登记全部在适配层。
6. **Proxies（轨道 B）：** 节点记录只出现一次（`nodes` 表），分组的 `all` 改为成员名列表。负载从 O(Σ分组成员数 × 节点记录大小) 降到 O(节点数 × 记录大小 + Σ成员数 × 名字长度)，一次测速结果只替换一个节点对象。

---

## 1. 现状事实（已核对源码）

### 1.1 连接流（Rust → WebView）

| 事实                                                                                                                                           | 位置                                                                                                         |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| 连接流按 mihomo 默认 1 s 采样；`ConnectionStreamQuery` 只有 `interval` 一个参数，没有只要总量的模式，Rust 必然收到完整连接列表（经 localhost） | `nyanpasu-runtime/crates/clash-api/src/api/connections.rs:204-230`；`tauri/src/core/actor_v2/api.rs:119-125` |
| 每个样本都把每条连接 `serde_json::to_value` 成 JSON 值，克隆一份进历史，再把完整快照作为事件推出，与是否有人在看无关                           | `core/clash/ws.rs:352-376`                                                                                   |
| `recording` 默认全开，且只控制是否保留历史，不控制是否推送                                                                                     | `ws.rs:80-89`、`ws.rs:369-376`                                                                               |
| Rust 与前端各保留 32 个样本，每个样本含完整连接列表                                                                                            | `ws.rs:17`；`frontend/interface/src/provider/clash-ws-state.ts:3`                                            |
| `snapshot()` 深拷贝全部历史；bridge 收到 `Lagged` 时取快照并以 `Reset` 整体推送                                                                | `ws.rs:173-188`；`core/clash/mod.rs:103-124`                                                                 |
| 四类数据共用一个序号和一个容量 64 的 broadcast，每条日志单独一个事件                                                                           | `ws.rs:272-277`、`ws.rs:378-387`、`ws.rs:689-690`                                                            |
| 另有一个小的总量事件 `ClashConnectionsEvent` 被转发到 WebView，但**前端没有监听方**；它在 Rust 内部的唯一消费者是网速小组件                    | `core/clash/mod.rs:84-100`；`widget.rs:118-160`、`setup.rs:163-170`                                          |

### 1.2 谁在消费什么（前端）

| 消费方                    | 读取的样本                          | 读取的字段                                                             |
| ------------------------- | ----------------------------------- | ---------------------------------------------------------------------- |
| Dashboard 上/下行流量卡片 | 最新 1 个                           | `downloadTotal` / `uploadTotal`                                        |
| Dashboard 连接数卡片      | **全部 32 个**（折线） + 最新 1 个  | 仅 `connections.length`                                                |
| Topology                  | 最新 1 个                           | 明细：`id`、`upload`、`download`、`chains`、`metadata.*`，以及全文搜索 |
| Connections 页            | 最新 **2** 个（按 `id` 做差算速率） | 明细全部字段；详情弹窗展示所有非空字段（含未知字段）                   |
| Proxies 分组页头速率      | 最新 **2** 个（同样做差）           | 明细：`id`、`chains`、`download`、`upload`                             |

位置：`dashboard/_modules/widget-sparkline.tsx:118-200`、`topology/route.tsx:27-41`、`connections/index.tsx:88-115`、`proxies/_modules/hooks.ts:12-55`。

结论：**只有 Topology 和 Connections 两个页面需要明细**；其余只需要标量、连接数，以及按分组聚合的速率。

### 1.3 Tauri 投递语义（已读 `tauri 2.12.0` 源码）

| 事实                                                                                                                                                                                              | 位置                                                                         |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `emit` 只投给为该事件注册了 JS 监听器的 webview                                                                                                                                                   | `tauri-2.12.0/src/event/listener.rs:309-333`                                 |
| 主窗口和托盘菜单窗口共用 `pages/__root.tsx`，两者都挂载 `ClashWSProvider` 并监听 `clash-ws-event`                                                                                                 | `frontend/interface/src/provider/index.tsx:63-71`                            |
| 托盘菜单默认失焦后**隐藏**而不是关闭，所以打开过一次之后，这个隐藏的 webview 一直在接收、解析完整连接列表；而托盘页面本身不读任何流数据                                                           | `nyanpasu-config/src/application/mod.rs:334-335`；`utils/resolve.rs:495-512` |
| `Channel::send` 只是 `webview.eval(...)`。页面重载后 webview 仍在，`eval` 成功，`send` 返回 `Ok`，但 JS 回调已不存在；窗口关闭后，大负载走 `insert_if(.., is_registered())` 分支直接返回 `Ok(())` | `tauri-2.12.0/src/ipc/channel.rs:409-445`                                    |
| 超过阈值的负载，`Channel` 走 fetch 取数，不用 eval；源码注释给出了 eval 与 fetch 的阈值依据                                                                                                       | `channel.rs:33-40`、`channel.rs:429-445`                                     |

### 1.4 Proxies

| 事实                                                                                                                                                      | 位置                                                                               |
| --------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `Proxies` 有 `global`、`direct`、`groups`、`records`、`proxies` 五个字段；每个分组的 `all` 是**完整节点记录的克隆**，同一节点在每个包含它的分组里各有一份 | `core/clash/proxies.rs:52-59`、`130-151`、`176-181`                                |
| `records` 是 `/proxies` 的全部条目（含分组）；前端只有 `group-delay.ts` 用它兜底查找隐藏节点                                                              | `proxies.rs:185`；`frontend/nyanpasu/src/components/proxies/group-delay.ts:26`     |
| `direct`、`alive` 前端从未读取；`proxies` 字段前端从未读取，Rust 托盘读了它，但只用来判断是否为空                                                         | `core/tray/proxies.rs:39`                                                          |
| `proxies` 字段初始化时就放入了 DIRECT 与 REJECT，永远不为空，所以托盘的 `raw_proxies.proxies.is_empty()` 恒为假                                           | `proxies.rs:167-171`                                                               |
| 每次 `get_proxies` 都克隆整份 `Proxies`                                                                                                                   | `core/proxies.rs:342-349`                                                          |
| 指纹变化时 Rust 推送 `StateChanged::Proxies`，前端据此让查询失效并重新请求                                                                                | `core/tray/proxies.rs:118-130`                                                     |
| 整组测速期间，前端每 500 ms 重新请求一次 `getProxies`                                                                                                     | `frontend/interface/src/ipc/use-clash-proxies.ts:156-157`                          |
| 前端测速成功后，逐个分组地替换同名节点的副本                                                                                                              | `use-clash-proxies.ts`（`updateProxiesDelay` / `updateGroupDelay` 的 `onSuccess`） |

---

## 2. 目标与成功判据

| #   | 判据                                                                                       | 验证方式                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| G1  | 没有明细订阅方时，每个连接样本推给 WebView 的负载与连接数 N 无关                           | Rust 单测：N = 0 / 1000 时，序列化后的 `ClashWsEvent` 字节数之差只来自成员速率表（该表的规模取决于链路成员名的数量，与 N 无关） |
| G2  | 没有明细订阅方时，actor 不做逐条 JSON 转换                                                 | actor 测试：没有 watch receiver 时，明细 watch 始终是 `None`                                                                    |
| G3  | 明细只发给订阅它的 webview；托盘 webview 收不到明细                                        | 适配层单测，加上真机抓包或日志核对                                                                                              |
| G4  | `Reset` / `get_clash_ws_snapshot` 的大小与 N 无关                                          | Rust 单测：N = 0 / 1000 时，快照序列化字节数相同                                                                                |
| G5  | 页面重载或窗口关闭后，该 webview 的明细订阅全部结束，最后一个订阅结束后 actor 不再构建明细 | 适配层单测，加上 actor 测试                                                                                                     |
| G6  | Dashboard、Topology、Connections、分组页头的显示与改动前一致；速率语义的变化见 §3.2        | 真机冒烟清单                                                                                                                    |
| G7  | Proxies 负载规模为 O(节点数 + Σ成员数 × 名字长度)，测速结果只替换一个节点对象              | Rust 单测：同一节点出现在 k 个分组时，序列化结果里只有一份记录；前端探针                                                        |

---

## 3. 轨道 A：连接流

### 3.1 契约（IPC 类型）

```rust
/// 每个连接样本都推送；大小与连接数无关。
pub struct ClashConnectionsSummary {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub memory: Option<u64>,
    pub connection_count: u32,
    /// 以链路成员名（分组或节点）为键，汇总所有 chains 中包含它的连接的速率。
    pub member_rates: IndexMap<String, TrafficRate>,
}

pub struct TrafficRate {
    pub download: u64, // bytes/s
    pub upload: u64,
}

/// 只推给订阅方；只存在最新一帧。
pub struct ClashConnectionDetails {
    pub sequence: u64, // 与汇总事件同一序号空间，便于前端对齐
    /// 保持可扩展的 JSON：每条是内核的连接对象，另外加入 downloadSpeed / uploadSpeed。
    pub connections: Vec<serde_json::Value>,
}
```

- `ClashWsUpdate::ConnectionsUpdated` 的载荷从 `ClashWsConnectionSnapshot` 改为 `ClashConnectionsSummary`；`ClashWsSnapshot.connections` 相应改为 `Vec<ClashConnectionsSummary>`。这是**可迁移的破坏性变更**：重新生成 bindings，调用方一次迁完，不保留旧形状（AGENTS §11）。
- 明细继续用 `serde_json::Value`。原因有两个：`ws.rs:40-43` 的 specta 递归类型限制仍然存在；`Connection` 本身带有 `#[serde(flatten)] extra`，详情弹窗需要展示内核新增的未知字段。
- `member_rates` 的规模取决于活跃链路里出现的分组名和节点名，典型是几十项，与连接数无关。分组页头直接读 `member_rates[groupName]`，不需要明细。

### 3.2 速率推导（纯服务）

新增纯服务 `ConnectionRates`（放在 `core/clash/` 下，零 IO、零 actor）：

```rust
pub struct RawConnectionsSample { /* 由 clash_api::ConnectionsSnapshot 与采样时刻组成 */ }

impl ConnectionRates {
    /// 由上一个样本与当前样本推导汇总；只有 `with_details` 为真时才生成逐条明细。
    pub fn derive(
        previous: Option<&RawConnectionsSample>,
        current: &RawConnectionsSample,
        with_details: bool,
    ) -> (ClashConnectionsSummary, Option<Vec<serde_json::Value>>);
}
```

语义（全部可以用普通值单测）：

- 时间间隔用两次采样之间实际经过的时间，与现有总速率的算法一致（`ws.rs:327-340`）；间隔为 0 时速率记 0。
- 上一个样本里没有的连接，速率记 0，与现在前端的做法一致。计数器变小（内核重置）时用 `saturating_sub`，记 0。
- `member_rates`：对每条连接，把它的速率加到 `chains` 中每个成员名上。同一条连接的 `chains` 里重复出现的名字只算一次。
- **行为变化：** 现在前端显示的是"两个样本之间的字节差"再加上 `/s`，实际上假设了采样间隔恰好是 1 s；改动后是按实际间隔换算的每秒字节数。在 1 s 采样下两者几乎相同，采样抖动时新算法更准确。

### 3.3 所有权与 actor 改动（`StreamsActor`）

| 状态                                                                        | 改动                                                                                                              |
| --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `history.connections: VecDeque<ClashWsConnectionSnapshot>`（32 × 完整列表） | 改为 `VecDeque<ClashConnectionsSummary>`（32 个汇总）                                                             |
| `baseline: Option<(u64, u64, Instant)>`                                     | 改为 `previous: Option<RawConnectionsSample>`，由 `ConnectionRates` 使用；原 `baseline` 的总速率计算并入 `derive` |
| 新增 `details: watch::Sender<Option<Arc<ClashConnectionDetails>>>`          | 放在 actor 启动参数里（依赖显式注入）；`StreamsClient::subscribe_connection_details()` 返回 `watch::Receiver`     |

处理一个连接样本的流程：

1. `with_details = details.receiver_count() > 0`；
2. `ConnectionRates::derive(previous, current, with_details)`；
3. 把汇总放进历史，并 `emit(ConnectionsUpdated(summary))`；
4. 如果有明细，就 `details.send_replace(Some(Arc::new(details)))`；
5. `previous = current`。

- `reset()` / `Invalidated`：清空 `previous` 和汇总历史，`details.send_replace(None)`。这与现在的断线语义一致（`ws.rs:290-304`）。
- `ClashConnectionsConnectorEvent::Update(ClashConnectionsInfo)` 保持不变，继续供网速小组件使用（`widget.rs`）。它的数值由汇总里的四个标量填充。
- 不引入第二个队列或调度层（AGENTS §8）：明细发布是 actor 处理样本时的同步步骤；`watch` 只保留最新值，天然不会积压，也就不需要为明细做滞后重同步。
- 订阅方从 0 变成 1 时，最多要等一个采样周期（1 s）才拿到第一帧明细。因为明细只在有订阅方时构建，订阅建立前 watch 里是 `None`。前端在这段时间显示加载态，见 §3.5。

### 3.4 Tauri 适配层：明细订阅

新增适配模块（放在 `core/clash/` 的 Tauri 边界，与现有 `StreamEventBridge` 同层）：

```rust
/// 适配层自有状态，不属于任何 actor，按 AGENTS §8 用锁。
struct ConnectionDetailSubscriptions {
    inner: Mutex<HashMap<SubscriptionId, Subscription>>,
}
struct Subscription {
    webview: String,           // webview label
    cancel: CancellationToken, // 转发任务的退出信号
}
```

IPC 命令：

```rust
#[tauri::command]
async fn subscribe_clash_connection_details(
    webview: tauri::Webview,
    client: State<'_, NyanpasuClient>,
    subscriptions: State<'_, ConnectionDetailSubscriptions>,
    on_frame: tauri::ipc::Channel<ClashConnectionDetails>,
) -> Result<SubscriptionId>;

#[tauri::command]
async fn unsubscribe_clash_connection_details(id: SubscriptionId, ..) -> Result<()>;
```

- 订阅时生成一个转发任务：持有 `watch::Receiver`，先发送当前值（如果不是 `None`），然后在每次 `changed()` 后发送最新帧，直到 `cancel` 被触发或 watch 关闭。任务结束时 receiver 被丢弃，actor 在下一个样本就能看到 `receiver_count` 变小。
- 订阅结束的三种情况：
  1. 前端显式退订（组件卸载）；
  2. 该 webview 开始加载新页面（`Builder::on_page_load`，`PageLoadEvent::Started`），用来覆盖刷新和热重载；
  3. 该 webview 销毁（`WindowEvent::Destroyed` 或 webview 销毁事件）。

  第 2、3 种按 label 批量取消。之所以必须这样，见 §1.3：`send` 不会因为订阅方消失而失败。

- 关闭：转发任务用根 `CancellationToken` 的子 token 启动，关闭时随根 token 结束，不设期限（AGENTS §6）。
- 选择 `Channel` 而不是 `emit_to(label)`：两者都能只投递给目标 webview；`Channel` 按订阅区分、天然有序，大负载走 fetch 路径（`channel.rs:429-445`），不需要在全局事件表里登记监听器。生命周期管理两者都得自己做。
- 同一个 webview 内的多个消费者（例如以后同时打开 Connections 和 Topology）由前端合并成一个订阅（§3.5），适配层不需要做引用计数。

### 3.5 前端

- **`ClashWSProvider`** 的 `connections` context 改为汇总历史（32 × 小对象）。`useClashConnections()` 返回汇总历史，字段和用法在类型上分开，避免误把汇总当明细用。
- **新增 `ClashConnectionDetailsProvider`**（挂在根上，和 `ClashWSProvider` 同层），它：
  - 对 `useClashConnectionDetails()` 的使用方计数；从 0 变成 1 时调用 `subscribe_clash_connection_details`，从 1 变成 0 时退订；
  - 把最新帧放进单独的 context，只有明细消费者会随明细更新而重渲染；
  - 在第一帧到达之前返回"加载中"状态，页面沿用现有的空态和加载态。
- **`ClashWSFreezeBoundary`** 同时冻结明细 context，退出中的页面不再随明细更新重渲染（沿用第一阶段的机制）。
- 调用方迁移：

| 调用方               | 改动                                                                                                   |
| -------------------- | ------------------------------------------------------------------------------------------------------ |
| Dashboard 流量卡片   | 读汇总历史的 `downloadTotal` / `uploadTotal`，只有类型变化                                             |
| Dashboard 连接数卡片 | 折线读 `connectionCount` 历史                                                                          |
| Topology             | 改用 `useClashConnectionDetails()` 的最新帧                                                            |
| Connections 页       | 改用明细帧；逐条速率直接读 `downloadSpeed` / `uploadSpeed`，删除 `prevMap` 做差                        |
| Proxies 分组页头     | 读最新汇总的 `memberRates[groupName]`，删除 `sumGroupTrafficSpeed` / `useGroupTrafficSpeed` 的做差实现 |

- **托盘窗口：** 托盘页面不读任何流数据。改动后它只会收到小的汇总事件。托盘窗口仍挂载 `ClashWSProvider`（D3）。

### 3.6 重同步与背压

- `Reset` 与 `get_clash_ws_snapshot` 只含：汇总历史（32 个小对象）、日志（≤ 1024 条）、流量和内存历史（各 32 个）。大小与连接数无关（G4）。
- 明细走 `watch`，没有序号缺口的问题，订阅方总是拿到最新值。明细帧带 `sequence`，前端可以把它和汇总事件对齐，但不依赖它做缺口检测。
- 日志突发仍可能让 broadcast 滞后并触发 `Reset`。改动后 `Reset` 的代价从"32 份完整连接列表"降到"≤ 1024 条日志"。日志批量合并不在本设计范围（§6）。

### 3.7 顺带清理

- `ClashConnectionsEvent` 转发到 WebView 的那个任务，前端没有监听方（§1.1）。Rust 侧的 broadcast 保留给小组件。这个转发任务、`clash-connections-event` 事件和 `get_clash_ws_connections_state` 命令已裁定删除（D4），在 §7 A5 执行。

### 3.8 测试

| 层                          | 测试                                                                                                                                                             |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ConnectionRates`（纯服务） | 首个样本；新增、关闭的连接；计数器回退；间隔为 0；`chains` 里有重复名字；`with_details = false` 时不生成明细；按成员名聚合的求和                                 |
| `StreamsActor`              | 没有 receiver 时明细始终为 `None`（G2）；订阅后收到明细帧，而且与汇总的序号一致；`reset` 清空明细和 `previous`；N = 0 / 1000 时事件与快照的字节数（G1、G4）      |
| 适配层                      | 显式退订、按 label 批量取消、根 token 取消后转发任务结束，`receiver_count` 回到 0（G5）。用假的发送端替代 `Channel`                                              |
| 前端                        | 用浏览器探针（`.probe/` + vite + playwright，见 frontend-adhoc-test-harness）验证 `ClashConnectionDetailsProvider` 的计数订阅和退订、冻结边界；bindings 重新生成 |
| 真机冒烟                    | Dashboard 四张卡片、Topology、Connections（速率、详情弹窗、排序、搜索）、分组页头速率、托盘菜单打开并隐藏后的 CPU 占用对比                                       |

---

## 4. 轨道 B：Proxies 负载归一化

### 4.1 契约

```text
Proxies {
  global: ProxyGroup,
  groups: ProxyGroup[],          // 分组字段不变，唯一变化：all 为成员名
  nodes:  Record<name, ProxyItem> // 取代 records；含 /proxies 全部条目与 provider 节点
}
ProxyGroup.all: string[]
```

- 删除 `records`（并入 `nodes`）、`direct`、`proxies`。
- 托盘改为直接判断 `mode == Mode::Global`，与现在的实际行为完全一致（§1.4：`proxies.is_empty()` 恒为假）。
- `alive` 暂时保留（没有人读，但它属于内核数据），不在本轨道处理。

### 4.2 改动面

| 位置                                                                         | 改动                                                                                                                  |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `core/clash/proxies.rs::from_responses`                                      | 一次性构建 `nodes`（`/proxies` 与 provider 节点合并，保持现有 `resolve_proxy` 的优先级和 Unknown 兜底）；分组只放名字 |
| `core/tray/proxies.rs`                                                       | `to_tray_proxies` 从 `group.all` 名字构建（现在本来就只取 `name`）；删除 `proxies.is_empty()` 判断                    |
| 前端 `use-clash-proxies.ts`                                                  | 测速成功后只替换 `nodes[name]` 一个对象；删除逐分组替换的代码                                                         |
| 前端分组页、托盘分组页、`GroupSummary` / `group-delay.ts`、`ProxyNodeButton` | 按名字从 `nodes` 取节点；`group-delay.ts` 的兜底改为读 `nodes`                                                        |

### 4.3 效果与风险

- 节点对象跨分组共享：测速结果只换一个对象，所有分组里的 memo 节点都能正确更新，未变化的节点保持引用不变。
- 风险：前端的分组渲染需要多一次按名字查找。按名字取节点是 O(1) 的 map 查找，规模由虚拟列表的可见项数决定，可以忽略。
- 测试：Rust 单测保证同一节点出现在 k 个分组时序列化结果里只有一份（G7）；托盘在 Global、Rule 模式下构建的菜单与改动前一致；前端测速后分组内的节点更新（浏览器探针）。

---

## 5. 决策（2026-09-29 已裁定）

| #   | 问题                                                                         | 裁定                                         | 理由                                                          |
| --- | ---------------------------------------------------------------------------- | -------------------------------------------- | ------------------------------------------------------------- |
| D1  | 明细的投递方式                                                               | `Channel`，加按 webview label 的生命周期管理 | §3.4；`emit_to(label)` 同样需要自己管理生命周期，没有额外好处 |
| D2  | 分组页头的速率从哪里来                                                       | 汇总里的 `member_rates`                      | 让分组页订阅明细，会重新引入与连接数成正比的负载              |
| D3  | 托盘窗口是否挂载 `ClashWSProvider`                                           | 本设计不动                                   | 改动后托盘只收小汇总；是否不挂载另起小 PR 评估                |
| D4  | 前端没有监听方的 `clash-connections-event`、`get_clash_ws_connections_state` | 随本次契约改动删除（§7 A5）                  | 本次改的就是这份契约；Rust 侧 broadcast 保留给网速小组件      |
| D5  | 两条轨道的顺序和 PR 切分                                                     | 轨道 A 先，轨道 B 后，各自成 PR              | 两者互不依赖，A 的收益更大                                    |

---

## 6. 不在范围（记录）

- **日志流：** 每条日志一个事件，与其他类型共享 64 容量的 broadcast，突发时会触发 `Reset`。可以在推送边界按时间窗批量合并。它与本设计相互独立，本设计完成后 `Reset` 的代价已经大幅下降，因此另行评估。
- **`ProxiesActor` 每 10 s 强制刷新并序列化整份数据计算指纹：** 只消耗 Rust 侧 CPU，不经过 WebView。
- **整组测速期间 500 ms 轮询：** 轨道 B 之后每次轮询的负载大幅下降。是否改为由 Rust 推送进度，另行评估。

---

## 7. 实施顺序

原则是**先建后删**：先加上新通路，调用方迁过去以后，再从旧契约里删掉明细。这样每个中间提交都能工作，不需要兼容层。

1. A1：`ConnectionRates` 纯服务及其单测。
2. A2：`StreamsActor` 增加明细 `watch` 和 `member_rates`（汇总事件暂时仍带完整列表）；适配层订阅命令，以及按 label 的生命周期管理；重新生成 bindings。
3. A3：前端 `ClashConnectionDetailsProvider`；Topology、Connections 迁到明细订阅，分组页头迁到 `memberRates`。
4. A4：汇总契约定型：`ConnectionsUpdated` 与快照改为 `ClashConnectionsSummary`，历史只保留汇总；Dashboard 迁到 `connectionCount`；删除前端两处做差实现。
5. A5（D4）：删除没有监听方的转发任务和命令。
6. B1：`nodes` 契约、托盘、前端调用方，一个 PR 完成。

每一步各自可构建、可测试，按 AGENTS §18 一件事一个提交。A2 到 A4 之间，汇总事件仍带完整列表。这只是同一个 PR 里的中间状态，不对外发布，因此不算兼容层。
