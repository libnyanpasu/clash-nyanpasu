# 托盘代理菜单项：自描述 id 与 Linux 托盘后端评估

**日期：** 2026-10-05

**状态：** 设计已确认，按本 spec 直接实施。

**基线：** `c8c86ff80`（`refactor/proxies-clash-api-records`，PR #5605 的头）；Tauri 2.12.1，muda 0.20.0，tray-icon 0.25.1。

**分支：** `refactor/tray-menu-item-ids`（当前 checkout），叠在 PR #5605 之上。

**需求：** 去掉托盘按文本查找代理菜单项的 workaround 和 `bimap` 依赖；评估 Linux 专有的菜单重填 workaround 是否已有替代方案。

**依据：** Tauri 2.12.1 `src/menu/menu.rs:363-373` 与 `src/menu/submenu.rs:330-340`、muda 0.20.0 `src/menu_id.rs`、tray-icon 0.25.1 `src/platform_impl/{mod.rs,gtk/mod.rs,ksni/mod.rs,ksni/menu.rs}`、[代理 API 结构统一 spec](../2026-10-05-proxies-api-unification/design.md)，以及 §5 列出的上游 PR 与 issue。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

## 1. 决策与范围

1. 节点菜单项的 id 由组名和节点名编码而成：`proxy_node:` 后接 JSON 数组 `["组名","节点名"]`。组子菜单的 id 为 `proxy_group:` 后接组名。
2. 点击节点时直接解码 id 得到组与节点，不再经过 display 记录解析。删除 `bimap` 依赖、`ProxyItemIds`、`ProxySection.item_ids`、`ProxyItem` 与 `TrayDisplay::proxy_item`。
3. 切换勾选时按 id 逐级用 Tauri 的浅层 `get` 查找。删除按文本匹配的 `find_check_item`，以及 `update_selected_proxies` 里注释掉的按 id 实现。
4. 菜单处于 Unknown 状态时的点击照常执行。id 就是用户点击的那一项，不会被解析成别的节点；组或节点在内核中已不存在时，由 `select_proxy` 返回错误。
5. Linux 的 `refill_menu` workaround 本次保留。改用 ksni 后端的评估与切换条件见 §5。
6. 不变：`TrayProxies`、`diff_proxies`、重建与重绘的状态机（除去点击解析部分）、菜单结构与文案、其他固定菜单项 id、WebView 托盘、RPC。

## 2. 现状与问题

- Tauri 2.12.1 的 `Menu::get` 与 `Submenu::get` 只在直接子项中查找，不递归。2024-09 迁移到 Tauri v2 时（`eea903b34`），按 id 取不到嵌套在组子菜单里的节点项，于是改成按文本匹配（`proxies.rs` 的 `find_check_item`），按 id 的实现被注释保留至今。
- 节点项 id 是每次重建都从 0 计数的 `proxy_node_{n}`，本身不含语义，所以要用 `BiMap<(组, 节点), usize>` 反查。又因为序号在每次重建后复用，display 引入了 `ProxyItem::{Node, NotInMenu, Unknown}`：菜单状态未知时丢弃点击，避免按过期的映射把点击解析成别的节点。
- 按文本匹配的问题：
  - 组子菜单按标题匹配，若某个组名恰好等于其他顶层子菜单的标题，会找错子菜单；
  - Phase B 计划在节点文字上显示固定标记，届时按文本匹配会失效；
  - 顶层查找把 `select_proxy` 的特判混在同一个谓词里。
- `BiMap::insert` 遇到重复的左键会覆盖旧映射。组成员重名时，第一个重名项的 id 失去映射，点击它只会记录 "node id not found"。

## 3. 设计

全部改动在 `backend/tauri/src/core/tray/{proxies.rs,display.rs}` 与 `backend/tauri/Cargo.toml`（及 `backend/Cargo.lock`）。

### 3.1 id 编码（`proxies.rs`，模块级纯函数）

```rust
const NODE_ITEM_ID_PREFIX: &str = "proxy_node:";
const GROUP_MENU_ID_PREFIX: &str = "proxy_group:";

/// A node item's id names its group and node, so a click needs no lookup.
fn node_item_id(group: &str, node: &str) -> String {
    format!("{NODE_ITEM_ID_PREFIX}{}", serde_json::json!([group, node]))
}

fn group_menu_id(group: &str) -> String {
    format!("{GROUP_MENU_ID_PREFIX}{group}")
}

/// The group and node an id names; `None` for any other menu item.
fn parse_node_item_id(id: &str) -> Option<(String, String)> {
    serde_json::from_str(id.strip_prefix(NODE_ITEM_ID_PREFIX)?).ok()
}
```

- JSON 数组可以无歧义地还原任意组名与节点名，包括 `:`、`"`、`\`、`,`、空格、emoji 和空字符串。
- 两个前缀都不与现有固定 id 冲突（`rule_mode`、`select_proxy`、`no_proxies`、`quit` 等），旧格式 `proxy_node_3` 也不会被解析。
- 组 id 只用于查找、从不解析，组名在 `TrayProxies` 中唯一，因此不编码。

### 3.2 构建菜单

- `generate_group_selector` 用 `SubmenuBuilder::with_id(app_handle, group_menu_id(group_name), group_name)` 建组子菜单，节点项用 `.id(node_item_id(group_name, item))`，不再接收 `item_ids` 参数。
- `generate_selectors` 只返回 `Vec<MenuItemKind<R>>`。
- `setup_tray` 返回的 `ProxySection` 只含 `proxies`。

### 3.3 切换勾选

`update_selected_proxies` 用下面的查找替换 `find_check_item`，其余逻辑（未找到或 `set_checked` 出错时 `warn!` 并记为 `Shown::Partly`）不变：

```rust
fn find_node_item<R: Runtime>(menu: &Menu<R>, group: &str, node: &str) -> Option<CheckMenuItem<R>> {
    let group_id = group_menu_id(group);
    // Normal mode lists the groups at the top; Submenu mode nests them in `select_proxy`.
    let group_menu = menu.get(group_id.as_str()).or_else(|| {
        menu.get("select_proxy")?
            .as_submenu()?
            .get(group_id.as_str())
    })?;
    group_menu
        .as_submenu()?
        .get(node_item_id(group, node).as_str())?
        .as_check_menuitem()
        .cloned()
}
```

### 3.4 点击

`on_system_tray_event` 直接解码 id：

```rust
let Some((group, name)) = parse_node_item_id(event) else {
    if event.starts_with(NODE_ITEM_ID_PREFIX) {
        error!("malformed proxy item id: {event}");
    }
    return; // not a proxy item
};
```

随后照旧调用 `client.select_proxy(group, name)`，不再读取 `TrayState` 的 display 记录。

### 3.5 display

- `ProxySection` 只保留 `proxies`，文档改为"菜单中代理部分显示的组与选择"。
- 删除 `ProxyItem` 与 `proxy_item`。
- `TrayDisplay::Unknown` 的文档去掉"不对其解析点击"。
- 测试删去 `proxy_item` 断言、`node` 辅助函数与 `a_click_while_unknown_is_not_resolved`。`section` 改为只接收选中项，其余状态机断言保持不变。

### 3.6 依赖

从 `backend/tauri/Cargo.toml` 删除 `bimap`。`clash-nyanpasu` 是 `bimap` 在工作区内唯一的依赖方，`backend/Cargo.lock` 随之去掉该包，与本次改动一起提交。

## 4. 测试与验收

`proxies.rs` 新增单元测试：

- `node_item_ids_round_trip`：组名与节点名包含 `:`、`"`、`\`、`,`、`[`、空格、emoji、空字符串时，`parse_node_item_id(&node_item_id(g, n)) == Some((g, n))`。
- `node_item_ids_are_distinct`：`("a:b", "c")` 与 `("a", "b:c")`、`("a\",\"b", "c")` 与 `("a", "b\",\"c")` 的 id 不同。
- `other_menu_ids_are_not_node_items`：`rule_mode`、`select_proxy`、`no_proxies`、`quit`、`group_menu_id("Proxy")`、`proxy_node_3`、`proxy_node:not json` 都解析为 `None`。

检查命令：`cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray`、`pnpm lint:clippy`、`pnpm lint:rustfmt`。`test:backend` 中已知的 `connection_policy::profile_policy_and_noop_gates_do_not_acquire_a_source` 在 main 上同样失败，不在本次范围。

验收标准：

- 源码、`Cargo.toml` 与 `Cargo.lock` 中不再有 `bimap`。
- 不再有按菜单文本查找代理项的代码，也不再有注释掉的旧实现。
- `Tray::on_menu_item_event` 的固定 id 分支不变，非代理 id 仍落到 `proxies::on_system_tray_event` 并被忽略。

人工验证（本环境无法运行 GUI，在 PR 中注明）：

- macOS 原生菜单的 Normal 与 Submenu 两种模式下，从托盘选择节点后勾选移动；从页面选择节点后托盘只做局部重绘，勾选移动。
- Linux 重填菜单后两项同样成立。
- 组成员重名时点击任一项都选中该节点。

## 5. Linux 托盘后端（评估，本次不实施）

现状：Linux 上 libappindicator 后端无法可靠地重复 `set_menu`，所以 `Tray` 在 Linux 上不替换菜单，而是用 `refill_menu` 清空旧菜单、把新菜单的项搬进去（`mod.rs:382` 一带）。tray-icon 0.25.1 的 GTK `set_menu`（`gtk/mod.rs:75-80`）没有针对此问题的修复。

上游进展（来源均为 GitHub 与 crate 源码）：

- tray-icon 新增 ksni 后端（StatusNotifierItem D-Bus）：tauri-apps/tray-icon#201，2026-09-08 合入，随 0.25.0 发布。两个后端同时启用时选用 ksni 并给出 Cargo 警告（#364，README）。
- ksni 后端的 `set_menu` 只替换一份菜单快照（`ksni/mod.rs:69-75`），并由后台线程监听 `MenuChangeEvent` 推送更新（`ksni/mod.rs:46-54`、`:124`）。换用它之后可以删除 `refill_menu` 与 Linux 分支，所有平台都走 `set_menu`。
- Tauri 2.12.1 与 dev 分支都依赖 `tray-icon = "0.25"` 并固定启用 `libappindicator`。加 `linux-ksni` 开关的 tauri-apps/tauri#12319 自 2025-02 起停滞，对应的 tauri-apps/tauri#11293 仍未关闭。应用可以在 Linux 目标下直接依赖开启了 `ksni` 的 `tray-icon 0.25`，靠 Cargo 的 feature 合并启用它。#12319 只改了 Tauri 的文档，所以 Tauri 应能编译，但尚未在 Linux 上构建验证。

阻塞点：

1. 0.25.x 的 ksni 后端在 StatusNotifierWatcher 尚未出现在总线上时，`TrayIconBuilder::build()` 直接失败且不重试，整个登录会话都没有托盘（tauri-apps/tray-icon#372）。开机自启早于面板的情况很常见，libappindicator 在同样情况下会等待。修复只在 tray-icon 0.26.0（2026-09-30）中。
2. 0.26 与 Tauri 依赖的 `0.25` 不兼容，要等 Tauri 升级（tauri-apps/tauri#16174，未合并，升级后默认仍是 libappindicator）。
3. 0.25.1 的 ksni 只支持分隔符这一种预定义项（`ksni/menu.rs:58-77`，0.26 由 #374 补全）。本应用托盘只用分隔符，不受影响。沙箱环境下的 D-Bus 名称问题（tauri-apps/tray-icon#379）尚未修复，本应用不发 Flatpak，暂不相关。

切换条件与做法：Tauri 带 tray-icon 0.26 发布后，在 Linux 上启用 ksni（若 #12319 先合入则用 Tauri 的开关），删除 `refill_menu` 与 Linux 分支。切换前须在 Linux 上人工验证：

- KDE、带 AppIndicator 扩展的 GNOME；
- 开机自启早于面板；
- 菜单重建与勾选切换。

顺带可获得 Linux 托盘的左键事件与 tooltip。本节的自描述 id 不依赖托盘后端，切换后无需改动。

## 6. 提交与 PR

- `docs(tray): specify self-describing proxy menu item ids`：本 spec。
- `refactor(tray): identify proxy menu items by group and node`：§3 全部代码与测试，以及 `Cargo.toml`/`Cargo.lock`。

PR 以 `refactor/proxies-clash-api-records` 为基础分支，叠在 #5605 之上。

## 7. 已知局限

- 组成员重名时，各项 id 相同：点击任一项都选中该节点，但切换勾选只更新查到的第一项。用 `bimap` 时第一个重名项根本无法点击，因此这不是退化。
- 每次切换要经过两到三次 `items()` 调用，每次都派发到主线程，与原先按文本遍历的开销相当。
- 菜单查找依赖 Tauri 运行时，项目未启用 Tauri 的 `test` 特性，所以查找本身没有自动化测试，靠人工验证。
