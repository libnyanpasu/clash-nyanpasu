# UI 状态变更通知：typed event 与按数据性质缓存 设计

**日期：** 2026-10-26
**基线：** `main @ 09c6a9d00`
**来源：** 页面切换卡顿调查。页面挂载时，所有过期查询会在同一个任务里各自发一次 IPC（WebView2 上每次 1–2.5 ms 主线程），切换一次 proxy group 就有 5 次。查询不能放心地设成不过期，因为后端的变更通知不全。
**范围：** 后端补全配置、profile、proxies 的变更通知，统一为一个 typed event，经 `UiEventSink` 发出；前端按事件让查询失效，事件覆盖的查询不再挂载即刷新。
**不在范围：** `useKvStorage` 共享缓存（#5996）；图标资源协议与 RPC 合批（§6，#5997）；proxies 的 10 s 轮询间隔本身（§1.3）；`configuration status` 事件的语义。

---

## 0. 结论摘要

1. **一个 typed event 取代字符串事件。** `nyanpasu://mutation` 和它的 `StateChanged` 载荷改为 `tauri_specta::Event` 的 `StateChangedEvent`，进 `EVENT_NAMES` 与 specta 导出，前端用生成的 `events.stateChangedEvent`。Profiles 变体带上变更的 profile id 列表。
2. **通知从提交点发出，经 `UiEventSink`。** 三个配置 owner 提交后都会调用 `CommitNotifications`（`state/application.rs:183`、`state/clash_config.rs:147`、`state/profiles/actor.rs:503,1347`），所有来源（RPC、托盘、热键、同步任务、HTTP 客户端）都经过这里。组合根把 `CommitNotifications` 接成"effects + UI"的扇出，UI 一侧只是 `UiEventSink` 的适配。Effects actor 里提前发的 `refresh_clash`（`client/effects/actor.rs:312-314`）随之删除。
3. **Proxies 事件挪出托盘模块。** 现在由 `core/tray/proxies.rs:192-206` 在托盘模块里转发，改到组合根的事件转发里经 `UiEventSink` 发出；托盘刷新留在托盘。轮询保留：mihomo 对选中节点、`now`、延迟没有推送，轮询已经是"指纹变了才发"。前端测速后让后端刷新快照，托盘的延迟随之同步。
4. **前端以一张表维护"事件 → 查询"，只由这些事件改变的查询默认不过期。** 事件到来时按表失效；数据只经事件覆盖的路径变化的查询用 `setQueryDefaults(key, { staleTime: Infinity })`，其余（外部也能改的）保持挂载即刷新。

## 1. 现状事实（已核对源码）

### 1.1 通知

| 事实                                                                                                                            | 位置                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `StateChanged` 有 `NyanpasuConfig/ClashConfig/Profiles/Proxies` 四个变体，经字符串事件 `nyanpasu://mutation` 发出，只发给主窗口 | `client/event_sink.rs:7-58`                                                                                                                             |
| `refresh_verge` / `refresh_profiles` 没有生产调用方；`NyanpasuConfig`、`Profiles` 两种载荷从不发出                              | `event_sink.rs:28-34`；全仓 grep                                                                                                                        |
| `ClashConfig` 只在 effects 第 2 组（Locale/Logger/Widget/WidgetLogo/Tray）执行**前**发出                                        | `client/effects/actor.rs:309-314`                                                                                                                       |
| `Proxies` 由托盘模块里的监听在 proxies watch 变化时发出，监听本身由 `utils/resolve.rs:74` 无条件启动，与托盘是否存在无关        | `core/tray/proxies.rs:192-206,250-256`                                                                                                                  |
| 三个配置 owner 提交成功后各自调用一次 `CommitNotifications`；托盘、热键改配置也走这些 owner                                     | `state/application.rs:183`、`state/clash_config.rs:147`、`state/profiles/actor.rs:503,1347`；`client/hotkey/mod.rs:146-200`、`core/tray/mod.rs:521-538` |
| `CommitNotifications` 由 `MutationCoordinator` 在启动时接到 effects owner                                                       | `state/mutation.rs:30-68`、`client/effects/ports.rs:94-118`                                                                                             |
| profiles 的提交已经知道受影响的 profile：`AffectsRule::Touched(uid)` 等，保存文件内容也走同一提交（`commit_file_first`）        | `state/profiles/actor.rs:393-399,557-572,1709-1740`                                                                                                     |
| `ConfigurationStatusChanged` 在 mutation journal、effects、sources 任一变化时发出                                               | `setup.rs:398-418`                                                                                                                                      |

### 1.2 前端

| 事实                                                                                                 | 位置                                                           |
| ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `QueryClient` 用默认 `staleTime: 0`，挂载即刷新                                                      | `frontend/nyanpasu/src/services/rpc.ts:10`                     |
| 失效表的键是 `nyanpasuConfig` / `clashConfig`，后端载荷却是 snake_case，这两种事件从未让任何查询失效 | `mutation-provider.tsx:11,17`                                  |
| mutation 事件 → 失效表；`nyanpasuConfig`、`profiles` 两项永远收不到                                  | `frontend/query/src/provider/mutation-provider.tsx:9-35,66-72` |
| `getProfiles` 用 30 s staleTime 兜底"同步非活动 profile 不发事件"                                    | `frontend/query/src/ipc/use-profile.ts:99-103`                 |
| Tauri 下 `listenResync` 是空实现                                                                     | `frontend/rpc/src/tauri-event-transport.ts:56`                 |
| 每个 `listen` 也是一次 IPC（`plugin:event\|listen`），`useKvStorage` 每次挂载各发一次                | `frontend/query/src/hooks/use-kv-storage.ts:184-228`           |

### 1.3 proxies 的 10 s

`#417`（2024-02）引入托盘节点菜单时写的 `sleep(10s) // TODO: add a config`，此后 `#5215`、`#5458/#5465`、`#5640` 只搬迁、加指纹，没有改间隔，也没有给出理由。mihomo 的 REST 只有 `/connections`、`/logs`、`/memory`、`/traffic` 四个流，选中节点、url-test 的 `now`、后台测速都不推送（外部知识，未在仓库内验证）。应用内的选择、取消固定、provider 操作、布局、模式变更都已立即刷新；只有前端自己做的测速不刷新后端快照（`client/clash_api.rs:154-186`）。

## 2. 事件

```rust
/// What changed in the state the frontend reads. The frontend refetches what
/// depends on it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChangedEvent(pub StateChanged);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    /// The profiles document. `files` names the profiles whose file this
    /// commit wrote or removed, so a reader of a profile's content refetches
    /// only those.
    Profiles { files: Vec<ProfileId> },
    Proxies,
}
```

- 保留 `StateChanged` 这个名字和四个变体，只把 `Profiles` 变成带字段的变体，使其成为 internally tagged 枚举。线格式从字符串变为对象，前后端同一次提交更新。`docs/architecture/legacy-iverge-call-network.md` 里"载荷保持四个字符串"的门禁属于已结束的 actor 迁移，该文档已是历史记录，不再约束。
- `TauriUiEventSink` 改为 `StateChangedEvent(state).emit(app_handle)`，发给所有窗口：托盘菜单窗口、编辑器窗口也读这些查询；HTTP 桥照常由 `listen_any` 接住。
- 删掉 `UiEventSink` 上无调用方的 `refresh_verge` / `refresh_profiles` / `mutate_proxies` 默认方法，只留 `state_changed`。

## 3. 发出点

### 3.1 配置提交

`CommitNotifications` 加一个扇出实现，组合根连接时传入 `(effects, ui)`：

```rust
/// Tells both owners about every commit, the effects first.
impl<A: CommitNotifications, B: CommitNotifications> CommitNotifications for (A, B) { ... }

/// The UI side: every commit becomes a state change for the frontend.
pub(crate) struct UiCommitNotifications(pub Arc<dyn UiEventSink>);
```

- `application_committed` → `NyanpasuConfig`；`clash_committed` → `ClashConfig`；`profiles_committed(files)` → `Profiles { files }`；`runtime_bound` / `publish_full` → `ClashConfig`。Workflow 在每次 Confirm/Cancel、生命周期命令（reconcile、启动）与恢复之后调用 `runtime_bound`，运行时的配置与端口随之可能变化；原先这条通知由 effects 第 2 组顺带发出（`a_core_reconcile_notifies_the_ui` 等测试覆盖）。因此扇出同时交给 workflow 的 `notifications` 与 `MutationCoordinator`。
- `profiles_committed` 加参数 `files: Vec<ProfileId>`：本次提交的主体（`commit_subject`：`Touched(uid)`、新建的 profile，或切换后的 current）加上 `next` 中已不存在的 id。保存文件、导入、远程同步、外部文件变化都以 `Touched` 或新建提交；切换 current 时多失效一个文件查询，无害。前端只有 `readProfileFile` 按 uid 读内容，条目元数据都在 `getProfiles` 里，后者每次 Profiles 事件都失效，所以不需要比较条目（`ProfileItem` 也没有 `PartialEq`）。
- 删掉 effects actor 在第 2 组执行前的 `ui.refresh_clash()`。它比提交通知晚、且只覆盖部分字段；effects actor 不再需要 `ui` 依赖，`EffectsArgs.ui`（`client/effects/actor.rs:72-74`）一并删除。
- 不新建 actor、不新增调度：通知在 owner 已有的提交路径上同步调用，`emit` 本身不阻塞。

### 3.2 proxies

`setup.rs` 的 `forward_actor_events` 已经转发核心与服务状态，把 proxies watch 的转发也放在这里，经 `ui_sink.state_changed(StateChanged::Proxies)`；`core/tray/proxies.rs` 的监听只保留 `Tray::request(PROXIES)`。

`proxy_delay` / `group_delay` 返回后（成功或失败，超时也记进历史）调用 `request_proxy_refresh()`（`client/clash_api.rs`）。mihomo 把测速结果记进节点历史，强制重读的快照指纹因此变化，托盘延迟与前端 `getProxies` 经同一个 Proxies 事件同步。组测速一次调用只刷新一次。固定分组按节点逐个测速（前端 8 路并发），每次都排一次强制全量重读会让后面的选节点请求排在 N 次重读之后，所以 `ProxiesClient::request_refresh` 合并请求：已有一次排在邮箱里时不再追加，actor 开始处理时清掉标记。

## 4. 前端

`MutationProvider`（`@nyanpasu/query`）里一张表，键是 `StateChanged['kind']`，类型保证与后端载荷一致：

| 事件              | 失效                                                                         |
| ----------------- | ---------------------------------------------------------------------------- |
| `nyanpasu_config` | `getAppConfig`、`getSysProxy`、`getProxies`（按默认测速 URL 裁剪）           |
| `clash_config`    | `clashApiGetVersion`、`getClashConfig`、`getClashInfo`、`clashApiGetConfigs` |
| `profiles`        | `getProfiles`，以及 `files` 中每个 uid 的 `readProfileFile(uid)`             |
| `proxies`         | `getProxies`、`clashApiGetProvidersProxies`                                  |

不过期（`staleTime: Infinity`，在 provider 渲染时设置，先于子组件的查询决定是否挂载即刷新）：`getAppConfig`、`getClashConfig`、`clashApiGetVersion`、`getProfiles`、`getProxies`。它们只经上表的事件变化：配置只经三个 owner 提交，proxies 快照的外部变化由 10 s 指纹轮询推送。

- 保持挂载即刷新：`getClashInfo`（未观察到结果的 apply 进入 `RecoveryRequired` 时作废已确认端口，不经过任何 `runtime_bound`）、`clashApiGetConfigs`（外部面板可直接 PATCH 内核）、`getSysProxy`（系统设置可改）、`clashApiGetProvidersProxies`（订阅流量信息不在指纹里）、`readProfileFile`（只有打开编辑器时读）。
- `getProfiles` 去掉 30 s staleTime：非活动订阅的定时同步也经 profiles owner 提交，现在会发 Profiles 事件。
- 目前托盘、热键对配置的修改靠窗口重新聚焦时刷新过期查询才到达界面。不过期的查询改由 §3.1 的提交通知承担，所以 §3.1 与本节同一 PR 落地。
- 核心状态事件的监听不变（只失效 `getCoreStatus`）；运行时的变化经 `runtime_bound` 的 `ClashConfig` 到达。
- 失效前先 `cancelQueries`（事件与 HTTP 传输的 resync 都是）：没有数据的查询在首次读取进行中时，`invalidateQueries` 会复用这次读取，它若在提交之前发出，旧答案会被当作新鲜数据永久保留。
- 监听注册完成后对不过期的查询补一次失效：注册是异步的，子组件可能已经读过，期间的提交事件没人听到。每个窗口启动时多读一次，只此一次。
- 打开的编辑器在文件被其它窗口、同步或外部修改后会收到失效。草稿（`useDraft`）只在用户未改动时跟随新内容，改过的草稿保留，关闭时照常提示未保存；Monaco 显示的是草稿而非已保存内容（原先显示已保存内容，“重置”也因此不起作用）。
- profile 卡片的 YAML 摘要缓存在 `perf/profiles-load` 分支，合并时按 uid 接入 `files`。

## 5. `useKvStorage`

改为共享缓存及其后端修复（转发溢出后永久停止、事件乱序、无序号、Tauri 下无 resync）单独在 #5996 跟踪，本轮性能优化完成后处理，不在本设计内。

## 6. 图标资源协议与 RPC 合批（#5997，本设计之后）

调研结论（tauri 2.12.5、wry 0.57.0）：

- 每次 `invoke` 是一次跨源 `fetch` 到 `http://ipc.localhost/call_rpc`，带 4 个 `Tauri-*` 头（`tauri/scripts/ipc-protocol.js:22-41`）。按跨源规则应触发 CORS 预检，`protocol.rs:164-170` 处理 `OPTIONS` 但不设 `Access-Control-Max-Age`（推断，待 trace 确认预检确实发生）。
- `call_rpc` 返回 `serde_json::Value`，走 `InvokeResponseBody::Json`，JS 端 `response.json()`；不是 raw `Response`。raw `Response` 只有在前端改用非 JSON 解码时才有收益。
- 资源协议（`tauri://`、`asset:`）与 IPC 走同一个 `WebResourceRequested` 拦截，并不更便宜；`asset` 协议在本项目未启用。
- 合批能把每次调用的固定开销（fetch、Headers、预检、loader）降到 1/N；结果应一次返回。用 `Channel` 逐条回传时，每条 8 KiB 以下的消息是一次 `ExecuteScript`，会吃掉部分收益。

图标是另一类：`get_cached_icon` / `get_tray_icon` 在后端把图片字节编码成 base64 `data:` URL，作为 JSON 字段返回（`ipc.rs:1359-1386`），前端放进 `<img src>`。每个图标一次 IPC，体积多三分之一，字符串常驻查询缓存。改为一个自定义 URI 协议（`register_asynchronous_uri_scheme_protocol`，按图标 URL 返回缓存的字节和 `Cache-Control`），`<img src>` 直接指向它：JS 主线程不再参与，浏览器的 HTTP 缓存与解码缓存接管；HTTP 传输对应一个 GET 路由。属于传输适配，需要按 RPC 规范单独设计。

§4 落地后，挂载时的 IPC 只剩事件覆盖不到的查询。届时用真机 trace 量一次单页挂载的 IPC 数与耗时，再决定是否做合批。两项都在 #5997 跟踪，作为下一步的优化项。

## 7. 测试

- 后端：扇出对两个接收方各调一次、`runtime_bound` / `publish_full` 映射为 `ClashConfig`（`event_sink.rs`）；经 `NyanpasuClient` 的提交（改应用配置、导入、保存文件、只改排序、删除）发出的事件与 `files`（`commits_tell_the_frontend_what_changed`）；现有 `*_notifies_the_ui` 三个测试改由扇出满足。托盘与热键改配置走同一 owner 提交，不单独测。测速后的刷新请求需要假内核，不加测试。
- 前端：不过期的查询跨挂载只请求一次、事件到来再请求；Profiles 事件只重读 `files` 中的文件。
- 真机：proxies 页、group 切换、设置页切换的 IPC 次数与最长帧，前后对比。

## 8. 已知限制

- 经 runtime 的 clash 配置提交会先后收到 `runtime_bound` 与 `clash_committed`，前端失效两次、多读一次；只发生在提交时，不在页面切换路径上，不另做去重。
- 物化恢复（`ReconcileMaterializations`）在补偿失败后修复文件内容时不经文档提交，不发 Profiles 事件。`readProfileFile` 保持挂载即刷新，影响限于此刻正打开的编辑器。

## 9. 已决

1. 线格式改为对象：`legacy-iverge-call-network.md` 的门禁随 actor 迁移结束失效（§2）。
2. UI 通知挂在 `CommitNotifications` 上扇出。理由：所有来源都经过 owner 的提交点，这是编排简化后仍会保留的不变量。ApplicationWorkflow 只用这个端口的 `runtime_bound` / `publish_full`（UI 一侧不发），owner 的 `*_committed` 在提交（及等待 settlement）之后调用，与 workflow 是否参与无关；以后缩减 workflow、改成 owner 直接驱动 runtime，提交点不变，扇出不受影响。若将来 effects 改为自读 owner 快照而删掉这个端口，UI 适配器只是一个实现，随提交点一起搬，不会散落在各 owner 里。
3. 前端测速后刷新后端快照（§3.2）。
