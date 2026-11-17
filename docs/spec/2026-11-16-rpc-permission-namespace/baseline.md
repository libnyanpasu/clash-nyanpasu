# RPC 权限层 授权基线（P0-1）

[任务卡 P0-1](tasks.md#p0-1-差异工具与经审阅的基线) 的产出。

**基线提交：** 远程访问栈顶 `feat/remote-access-pr7-lan-tls@b1741e0f8`。它比设计稿记录的 `a5b135695` 多出 a8c640a73 及其两个评审修复：这些提交把声明改写成 `permission = "group.name:action"`，但没有改变任何资源的 scope、effect、暴露面或 capability。所以本基线同样适用于退回 legacy `scope = …` 之后的代码，下一张卡用它来验证。

## 机器可读的基线与比较工具

> **P3-3 之后：** 本节描述的 scope 形式的基线已冻结为 `backend/nyanpasu-rpc/tests/fixtures/permission-scope-baseline.txt`，只供 `permission_baseline/mapping.rs` 核对切换的变更清单。`permission-baseline.txt` 改为权限形式（`kind | name | permission | sensitive | effect | targets | capabilities | optional_capabilities`），比较工具的调用者取未解析者加七个可授予资源各取 无/读/写 的全部 2187 种组合，准入模型与 `Authority` 核对，是 P4 各卡“迁移差异为空”的依据。本节以下内容是 P0-1 时的历史描述。

| 项       | 位置                                                          |
| -------- | ------------------------------------------------------------- |
| 基线表   | `backend/nyanpasu-rpc/tests/fixtures/permission-baseline.txt` |
| 比较工具 | `backend/nyanpasu-rpc/src/permission_baseline.rs`（仅测试）   |

基线表一行一个资源：`kind | name | scope | effect | targets | capabilities | optional_capabilities`。命令取自 `COMMAND_CATALOG`，事件取自 `EventName` 的 `scope`、`targets`、`capability`；两条 HTTP 路由与连接详情流的 scope 目前是准入处的字面量（`http/routes.rs`、`local/server.rs`），测试里逐条写出。

比较工具按 `(kind, name)` 对齐两张表，报告三类差异：

- 资源的增删；
- 字段变化，包括 `scope`、`effect`、`targets`、`capabilities`、`optional_capabilities`；
- 准入变化：在 Desktop、HTTP、Local 三种传输上，对未解析调用者和七个 scope 的全部 128 个子集逐一比较，每个资源每种传输输出一行，给出变化的调用者数量和一个例子。

准入模型是：已认证，并且暴露面含该传输，并且持有该 scope，`rpc_describe` 除外。测试 `comparison_admits_like_the_production_policy` 对每条命令、每种传输、每个调用者核对，工具的结论与生产的 `policy::is_command_allowed` 一致。capability 取决于宿主，只作为字段比较。

| 测试                                           | 判据                                                                            |
| ---------------------------------------------- | ------------------------------------------------------------------------------- |
| `authorization_matches_the_baseline`           | 当前代码与基线的差异为空；不应改变授权的每一步都必须保持为空                    |
| `baseline_covers_every_resource_once`          | 资源数为 225；命令名集合等于 `COMMAND_CATALOG`，事件名集合等于 `EventName::ALL` |
| `comparison_reports_a_changed_requirement`     | 改一条命令的 scope，字段差异与准入差异都被报告                                  |
| `comparison_reports_a_removed_exposure`        | 去掉一条命令的 `http` 暴露，字段差异与 HTTP 上 64 个调用者的准入差异都被报告    |
| `comparison_of_identical_tables_is_empty`      | 原样比较为空；渲染后再解析得到同一张表                                          |
| `comparison_admits_like_the_production_policy` | 见上                                                                            |

更新基线（只在评审过的授权变化之后）：

```sh
cargo test -p nyanpasu-rpc --lib permission_baseline::export_permission_baseline -- --ignored
```

与 tasks.md 的差异：P0-1 原计划先写成仓库外的脚本，到 P2-1 并入生成器的测试目标。实际直接写成 `nyanpasu-rpc` 里的测试，并且之后留在那里：事件与路由的现有元数据在 protocol 与 rpc 里，而 `nyanpasu-rpc-macro` 不得依赖会引入 config 的 crate（设计 §9 E-6），生成器的测试目标读不到它们。

## 数量

设计稿记录的是 189 条命令、208 个资源。栈顶新增了网络自动化的三条命令（`get_network_automation_status`、`apply_network_automation`、`request_wifi_name_access`）和 `install_cli`；rebase 到 main 后又多了配置同步的 12 条命令与 1 个事件（#6428，全部仅桌面），所以命令是 205 条，资源是 225 个。

| 种类 | 数量 | 说明                                                                 |
| ---- | ---- | -------------------------------------------------------------------- |
| 命令 | 205  | 对外暴露 128 条：仅 HTTP 81，HTTP 与 Local 39，仅 Local 8；仅桌面 77 |
| 事件 | 17   | `EventName::ALL`                                                     |
| 路由 | 2    | `icon_fetch`（`read`）、`logs_archive`（`sensitive_read`），仅 HTTP  |
| 流   | 1    | `connection_details`（`read`），HTTP 与 Local                        |

命令按 scope：

| scope            | 命令数 |
| ---------------- | ------ |
| `read`           | 54     |
| `sensitive_read` | 37     |
| `proxy`          | 9      |
| `profile`        | 20     |
| `settings`       | 21     |
| `system`         | 55     |
| `remote_admin`   | 9      |

`sensitive_read` 的命令从设计稿的 34 条变为 37 条，多出的是 `get_network_automation_status`、`get_sync_settings` 与 `list_sync_conflicts`。事件中有 8 个是 `sensitive_read`（多出 `sync-status-changed-event`），路由中有 1 条（`logs_archive`），与设计稿一致。

## 现有例外

| 例外                       | 资源                                                                                                                                                                                            |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 任何已解析的调用者都可调用 | `rpc_describe`（`read`）                                                                                                                                                                        |
| query，但 scope 不是读类   | `clash_api_get_proxy_delay`、`clash_api_get_group_delay`、`url_delay_test`（`proxy`）；`get_local_socket_status`、`list_remote_access_tokens`、`list_remote_browser_sessions`（`remote_admin`） |
| mutation，但 scope 是读类  | `open_log_session`、`close_log_session`、`query_logs_summary`、`subscribe_clash_connection_details`、`unsubscribe_clash_connection_details`（`read`）；`query_logs`（`sensitive_read`）         |

设计 §3.3 预计的例外是 3 + 6 = 9 条。这里的六条“读类 mutation”与设计一致。三条 `remote_admin` query 在新模型下自然是 `remote:read`，不需要 `access =`，因而不计入预算。这一点由 P0-2 的映射表确认。
