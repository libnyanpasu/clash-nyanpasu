# 代理组固定选择与统一结构的后续修复

**日期：** 2026-10-06

**状态：** 设计已确认，待实施计划。

**基线：** main `6e30d919a`（#5605、#5623、#5624 已合入）。

**分支与 PR：** 两个叠放的 draft PR，均在当前 checkout 实施。

| 阶段    | 分支                                | 基础分支                            |
| ------- | ----------------------------------- | ----------------------------------- |
| Phase 1 | `fix/proxies-unification-followups` | `main`                              |
| Phase 2 | `feat/proxies-pinned-selection`     | `fix/proxies-unification-followups` |

**需求：**

- Phase 1 修复 [代理 API 结构统一](../2026-10-05-proxies-api-unification/design.md) 审查时报告、但没有修复的小问题。
- Phase 2 让主页面、WebView 托盘菜单与原生托盘读取同一份组能力：
  - 按能力限制节点选择；
  - 新增"取消固定"操作，显示固定状态；
  - 选择失败时给出反馈；
  - 组测速不再静默取消用户的固定选择。

**依据：** [代理 API 结构统一 spec](../2026-10-05-proxies-api-unification/design.md)（组语义层与能力推断）、[托盘菜单项 id spec](../2026-10-05-tray-menu-item-ids/design.md)、[代理组内核接口核查报告](../../audit/2026-10-05-proxy-core-api-report.md)、mihomo `hub/route/proxies.go`（`unfixedProxy`）与 `hub/route/groups.go`（`getGroupDelay` 在测速前对非 Selector 的可选组调用 `ForceSet("")`）。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 实施计划。

## 1. 决策

1. 组有固定选择（`ProxyGroup.fixed` 非空）时，组测速改为逐个成员调用单节点测速，并发 8 路，固定选择保留。未固定的组仍走组测速接口。
2. 取消固定改变了组的实际出口，因此和选择节点一样，按"切换代理时断开连接"的设置（关闭、按组、全部）断开连接。
3. 原生托盘中被固定节点的菜单文字后缀 ` 📌`。可以取消固定的组，在子菜单顶部放一项"恢复自动选择"，下接分隔线；没有固定时这一项置灰。
4. 能力来自后端快照 `ProxyGroup.capabilities`，三个界面都不自行推断。后端不预先校验能力：内核拒绝时，把内核的错误原样返回。
5. 主页面在选择失败、取消固定失败时，用现有的 `message(..., { kind: 'error', error })` 提示。WebView 托盘菜单只写 `console.error`：弹出原生对话框会让托盘窗口失焦而关闭；而前端的 warning/error 在 #5604 之后会写入应用日志。
6. 在当前 checkout 实施，复用 `backend/target` 构建缓存。

不在本次范围（留给 Phase C）：

- 测速 URL、超时与 `expected` 参数化（仍回退到 gstatic 与 10 s）；
- 按 `extra[testUrl]` 读取延迟；
- 排序、搜索、过滤；
- 载荷裁剪；
- 断开连接设置改为三档的界面。

## 2. Phase 1：遗留问题

| #   | 问题                                                                                       | 修复                                                                                                                                 |
| --- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | 能力矩阵测试缺少 `Selector`、`Fallback` 带 `fixed="a"` 的用例                              | `core/clash/proxies.rs` 的 `capabilities_follow_the_type_and_the_fixed_field` 补这两行，期望分别为 `(true, false)` 与 `(true, true)` |
| 2   | 托盘 rule 模式测试只断言小写 `"global"` 不存在                                             | `core/tray/proxies.rs` 的测试对 Rule 与 Script 模式断言没有 `"GLOBAL"`                                                               |
| 3   | provider 订阅数值为负时，providers 页显示负的已用量                                        | `use-proxies-subscription.tsx` 把 `Upload`、`Download`、`Total` 各自截到不小于 0 再计算                                              |
| 4   | `providers-refresh.browser.test.tsx` 的 mock 延迟样本缺 `time`，与 `DelayHistory` 类型不符 | mock 样本补上 ISO 8601 的 `time`                                                                                                     |
| 5   | `provider_proxy_map` 每次刷新都复制全部 provider 节点                                      | 改为按引用建立"节点名 → (provider 名, 节点)"索引，只在组成员缺失时复制该节点并设置 `provider`                                        |

第 5 项保持现有语义：

- 只收录 vehicle 为 HTTP、File、Inline 的 provider；
- 不同 provider 中有同名节点时，后者覆盖前者；
- `/proxies` 中已有的节点优先。

#5623、#5624 的原生菜单行为仍需人工 GUI 检查（macOS、Linux、Windows），本次无法自动化，在 PR 中注明。

## 3. Phase 2 后端：取消固定

### 3.1 调用链

```text
clear_proxy_fixed (RPC, mutation, rpc(http))
  -> NyanpasuClient::clear_proxy_fixed(group)        // 读取 break_connection.on_proxy_change
    -> ProxiesClient::clear_fixed(group, strategy)
      -> ProxiesActor: Message::ClearFixed { group, strategy, reply }
        -> ApiClient::clear_proxy_selection(&ProxyName)  // DELETE /proxies/{group}
```

- `ApiClient::clear_proxy_selection` 放在 `core/actor_v2/api.rs`，与 `select_proxy` 相同，通过 `self.execute(self.client.clear_proxy_selection(group))` 调用 crate 已有的方法。
- `Message::ClearFixed` 是一个独立的消息变体，不复用 `Select`。关闭期间的处理与 `Select` 相同：回复 "proxy owner is shutting down"。
- `NyanpasuClient::clear_proxy_fixed` 放在 `client/clash_api.rs`，紧挨 `select_proxy`，用同样的方式读取断开策略。
- RPC 命令 `clear_proxy_fixed(client, group: String) -> Result<MutationOutcome<()>>` 放在 `ipc.rs`，紧挨 `select_proxy`，带 `#[nyanpasu_macro::rpc(http)]`；在 `specta_export.rs` 中紧挨 `ipc::select_proxy` 注册为 mutation。它与 `select_proxy` 效果同类（改变组的出口），所以同样启用 HTTP。
- 通过 `specta_export::tests::export_typescript_bindings` 重新生成 `rpc-bindings.ts` 与 `query-bindings.ts`，不手改生成文件。

### 3.2 actor 内的流程

`select` 与新增的 `clear_fixed` 共用变更之后的流程，提取为 `State` 上的一个方法：

1. 取得 `api`。失败时清空缓存并返回错误（与 #5624 后的 `select` 相同）。
2. 执行内核变更（`select_proxy` 或 `clear_proxy_selection`）。被拒绝时经 `reread_after_rejection` 重新读取快照，并返回内核的错误。
3. 按 `ConnectionScope::for_proxy_change(strategy, group)` 断开连接。
4. 刷新缓存。
5. 返回 `MutationOutcome`。断开连接失败记为 `ProxyInterruptionFailed`，刷新失败记为 `ProxyCacheRefreshFailed`。

降级原因沿用这两个变体：它们描述的是哪一步失败，与触发它的变更无关。只有消息文字区分操作：

- 选择：`proxy selected, but …`（不变）；
- 取消固定：`pinned selection cleared, but …`。

### 3.3 测试（`core/proxies.rs`）

在现有 axum fixture 上增加 `DELETE /proxies/{group}` 路由，记录调用 `"clear"`；`fail_mutation` 置位时返回 503。断言：

- `All` 策略的调用顺序为 `["clear", "close", "read"]`，`Off` 为 `["clear", "read"]`；
- `ProxyGroup` 策略只关闭链路含该组的连接（与现有 `group_policy_closes_only_matching_chains` 相同的数据）；
- 被拒绝时返回错误，调用顺序为 `["clear", "read"]`，快照保留；
- 断开连接失败时结果带一个 `ProxyInterruptionFailed` 降级，快照已刷新。

## 4. Phase 2 前端

### 4.1 `useClashProxies`（`frontend/query/src/ipc/use-clash-proxies.ts`）

- 新增 `clearProxyFixed(group)`：
  - 通过 `useMutation` 调用生成的 `api.mutations.clearProxyFixed`，这样降级结果会经 `onDegraded` 统一提示；
  - 完成后与 `selectProxy` 一样，`refetchQueries` 获取最新的代理数据；
  - 失败时抛出，由调用方处理。
- `updateGroupDelay` 的 `mutationFn`：
  - 从查询缓存中按组名找到组（`global` 或 `groups`）。
  - 组有 `fixed` 时，对 `group.all` 中每个成员调用 `clashApiGetProxyDelay(name, nodes[name]?.provider ?? null, url)`：
    - 并发上限为本文件内的常量 `PINNED_GROUP_DELAY_CONCURRENCY = 8`，由一个本地并发函数实现，不新增依赖或共享工具；
    - 单个成员失败时记为 0，即失败样本，与内核写入历史的值一致；不中断其余成员；
    - 返回全部成员的 `{ name: delay }`。
  - 否则仍调用 `clashApiGetGroupDelay`。
  - 两条路径的返回形状相同，`onSuccess` 与 `onSettled` 不变。现有测试要求组测速期间只拉取一次 `get_proxies`，因此不追加刷新。

### 4.2 主页面（`frontend/nyanpasu/src/pages/(main)/main/proxies/`）

- `group/_modules/proxy-node-button.tsx` 新增两个 props：
  - `selectable`：为 false 时点击卡片不调用 `onSelect`，根元素带 `data-selectable="false"` 且不显示指针手势；测速按钮照常可用。
  - `fixed`：为 true 时，节点名旁显示 `~icons/material-symbols/keep-rounded` 图标（`data-slot="proxy-node-fixed-icon"`，`title` 为 `m.proxies_group_fixed_label()`）。
- `group/$name.tsx`：
  - 向节点卡片传入 `selectable={currentGroup.capabilities.select}` 与 `fixed={name === currentGroup.fixed}`；
  - `handleSelectProxy` 捕获错误，用 `message(m.proxies_select_failed_message({ group, name }), { kind: 'error', error })` 提示；
  - 组头在 `currentGroup.fixed` 非空时显示 `data-slot="proxies-group-fixed"`：固定图标、`m.proxies_group_fixed_label()` 与节点名。
  - 若 `capabilities.clearFixed` 为 true，再显示按钮 `data-slot="proxies-group-clear-fixed-button"`：
    - 图标 `keep-off-rounded`，文字 `m.proxies_group_clear_fixed_button()`；
    - 调用 `clearProxyFixed`；
    - 失败时用 `m.proxies_clear_fixed_failed_message({ group })` 提示。
- `frontend/nyanpasu/src/components/proxies/group-summary.tsx`：组有 `fixed` 时，在当前节点前显示固定图标（`data-slot="group-summary-fixed-icon"`，`title` 为固定的节点名）。

### 4.3 WebView 托盘菜单（`frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx`）

- 节点按钮：
  - `selectable` 为 false 时点击不选择，带 `data-selectable="false"`；
  - 被固定的节点显示与主页面相同的固定图标。
- `capabilities.clearFixed` 为 true 且组有 `fixed` 时，顶部吸附栏在测速按钮旁显示"恢复自动"按钮。
- 选择与取消固定失败时写 `console.error`，不弹对话框。

### 4.4 i18n

在 `frontend/nyanpasu/messages/` 的 en、ko、ru、zh-cn、zh-tw 五个文件中，紧挨 `proxies_group_delay_test_*` 这一组插入：

| 键                                   | zh-cn                        | en                                                |
| ------------------------------------ | ---------------------------- | ------------------------------------------------- |
| `proxies_group_fixed_label`          | 已固定                       | Pinned                                            |
| `proxies_group_clear_fixed_button`   | 恢复自动                     | Restore Auto                                      |
| `proxies_select_failed_message`      | 无法在 {group} 中选择 {name} | Failed to select {name} in {group}                |
| `proxies_clear_fixed_failed_message` | 无法恢复 {group} 的自动选择  | Failed to restore automatic selection for {group} |

ko、ru、zh-tw 使用对应语言的译文。通过现有工作流重新生成 Paraglide 输出，不手改生成的消息模块。

## 5. Phase 2 原生托盘（`backend/tauri/src/core/tray/proxies.rs`）

- `TrayGroup` 增加 `fixed: Option<String>` 与 `clear_fixed: bool`，由 `TrayGroup::of` 从 `ProxyGroup` 复制。
- `diff_proxies` 在 `fixed` 或 `clear_fixed` 变化时返回 `Full`，与 `selectable` 变化的处理相同。固定状态很少变化，整菜单重建的代价可以接受。
- `generate_group_selector`：
  - 节点名等于 `fixed` 时，菜单文字为 `format!("{name} 📌")`。勾选与查找都按 id 进行（#5623），改动菜单文字不影响局部更新。
  - `clear_fixed` 为 true 且组有成员时，子菜单开头先放一项"恢复自动选择"，下接一条分隔线：
    - 文案 `t!("tray.restore_auto_selection")`；
    - id 为 `proxy_unfix:` 后接组名；
    - `fixed` 为 `None` 时置灰。
- id 函数（模块级纯函数）：

  ```rust
  const UNFIX_ITEM_ID_PREFIX: &str = "proxy_unfix:";
  fn unfix_item_id(group: &str) -> String { format!("{UNFIX_ITEM_ID_PREFIX}{group}") }
  fn parse_unfix_item_id(id: &str) -> Option<&str> { id.strip_prefix(UNFIX_ITEM_ID_PREFIX) }
  ```

  这个 id 只携带组名这一个字段，前缀之后的全部内容即为组名，不需要转义。

- 点击：`on_system_tray_event` 先尝试解析节点 id，再尝试解析取消固定 id。后者在异步任务中调用 `client.clear_proxy_fixed(group)`，按与选择相同的方式记录错误与降级，然后 `Tray::request(&app_handle, TrayWork::PROXIES)`。普通菜单项不会像勾选项那样被平台翻转，所以不调用 `display.clicked`。
- 文案：在 `backend/tauri/locales/{en,ko,ru,zh-cn,zh-tw}.json` 的 `tray` 下增加 `restore_auto_selection`：

  | 语言  | 文案                              |
  | ----- | --------------------------------- |
  | zh-cn | 恢复自动选择                      |
  | zh-tw | 恢復自動選擇                      |
  | en    | Restore Automatic Selection       |
  | ko    | 자동 선택 복원                    |
  | ru    | Восстановить автоматический выбор |

## 6. 提交序列

### Phase 1（`fix/proxies-unification-followups`）

| #   | 提交                                                                                | 内容                 |
| --- | ----------------------------------------------------------------------------------- | -------------------- |
| P0  | `docs(proxies): specify pinned selection and unification follow-ups`                | 本 spec 与实施计划   |
| P1  | `test(proxies): cover pinned Selector and Fallback, and GLOBAL outside Global mode` | 第 2 节第 1、2 项    |
| P2  | `fix(providers): never show negative subscription usage`                            | 第 2 节第 3 项与单测 |
| P3  | `test(providers): give mocked delay samples a time`                                 | 第 2 节第 4 项       |
| P4  | `perf(proxies): copy only the provider nodes a group references`                    | 第 2 节第 5 项       |

### Phase 2（`feat/proxies-pinned-selection`）

| #   | 提交                                                                       | 内容                                                                                      |
| --- | -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| B1  | `feat(proxies): clear a group's pinned selection`                          | 第 3 节全部，含 bindings                                                                  |
| B2  | `feat(proxies): select only where the core accepts it and report failures` | 第 4.2、4.3 节中的选择限制与失败反馈，以及所需的 i18n 键                                  |
| B3  | `feat(proxies): show pinned members and restore automatic selection`       | 第 4.1 节的 `clearProxyFixed`，第 4.2、4.3 节中的固定显示与"恢复自动"，以及所需的 i18n 键 |
| B4  | `fix(proxies): keep a group's pin through a latency test`                  | 第 4.1 节的 `updateGroupDelay`                                                            |
| B5  | `feat(tray): mark pinned nodes and restore automatic selection`            | 第 5 节                                                                                   |

B3 与 B5 依赖 B1 的 RPC 与客户端方法；B2、B4 不依赖 B1。每个提交独立可构建并通过检查。

## 7. 测试与验收

### 7.1 测试

- Rust：
  - Phase 1 第 1、2、5 项（第 5 项要求现有组装测试全部通过）；
  - 第 3.3 节；
  - 托盘：`unfix_item_id` 与 `parse_unfix_item_id` 往返（组名含 `:`、空格、emoji、空字符串）；节点 id 不会被解析为取消固定 id，反之亦然；`TrayGroup::of` 复制 `fixed` 与 `clear_fixed`；`fixed` 或 `clear_fixed` 变化时 `diff_proxies` 返回 `Full`。
- 前端：
  - `frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx` 增加负值用例；
  - `frontend/query/tests/` 增加：
    - 固定组测速逐个调用单节点测速，并带上 provider；
    - 未固定组仍调用组测速；
    - 单个成员失败记为 0 样本，不影响其余成员；
    - `clearProxyFixed` 调用对应 mutation 并重新获取数据。

### 7.2 检查命令

- `pnpm lint:clippy`、`pnpm lint:rustfmt`；
- tauri crate 相关的 `cargo test`（`core::clash::proxies`、`core::proxies`、`core::tray`、`specta_export`）；
- `pnpm typecheck`、`pnpm test:frontend`、`pnpm lint`；
- `deno task lint:architecture-ledger`。

main 上已有的 `connection_policy::profile_policy_and_noop_gates_do_not_acquire_a_source` 在全量运行时失败，与本次无关。

### 7.3 验收标准

| 场景                                           | 期望                                                                 |
| ---------------------------------------------- | -------------------------------------------------------------------- |
| provider 订阅数值为负                          | 已用量与总量不小于 0                                                 |
| 主页面或 WebView 托盘中 `select` 为 false 的组 | 点击节点不发出 `select_proxy`；测速按钮可用                          |
| 主页面选择失败                                 | 弹出带内核消息的错误对话框                                           |
| URLTest/Fallback 组被固定                      | 主页面节点卡片与组头、侧栏摘要、WebView 托盘、原生托盘都显示固定标记 |
| 点击"恢复自动"（三处任一）                     | 调用 `clear_proxy_fixed`，按设置断开连接，固定标记消失               |
| 固定组测速                                     | 不调用组测速接口；测速后 `fixed` 不变                                |
| 未固定组测速                                   | 行为不变                                                             |
| Clash-rs                                       | URLTest/Fallback 没有固定能力，不显示"恢复自动"                      |
| `clear_proxy_fixed` 两种传输                   | IPC 与 HTTP 都可调用，归为 mutation，错误保留结构化信息              |

### 7.4 审查

每个 Phase 实施完成后执行 `/ccg:review`；修复 Critical/High 级别的问题后重新审查，直到不再出现 Critical/High。然后推送分支并创建 draft PR：Phase 2 的 PR 以 Phase 1 的分支为基础。

## 8. 已知局限

- 固定组成员很多且大量超时时，逐个测速的最坏耗时约为成员数 ÷ 8 × 10 s；组测速接口只需一个超时周期。
- 测速 URL 仍回退到 gstatic；mihomo 的 URLTest 组测速使用组的 `testUrl`，逐个测速则使用请求中的 URL，两者结果可能不同，留给 Phase C 统一。
- 原生菜单的构建、文字与点击没有自动化测试，需要人工 GUI 检查。
- Windows 原生菜单可能以单色显示 📌。
