# RPC 权限层 完整映射（P0-2）

[任务卡 P0-2](tasks.md#p0-2-资源表角色表完整映射与变更清单) 的产出：225 个受保护对象在新模型下的要求、命名空间、敏感标记、例外与暴露面，资源表与角色表的定稿，变更清单，以及搬迁卡的顺序。基线见 [baseline.md](baseline.md)。

**状态：** 已确认（2026-11-18）。资源名与角色名会进入持久化数据（token 记录、`anonymous_permissions`），之后改名即迁移。

## 机器可读的映射与核对

| 项               | 位置                                                                        |
| ---------------- | --------------------------------------------------------------------------- |
| 映射表           | `backend/nyanpasu-rpc/tests/fixtures/permission-mapping.txt`                |
| 变更清单与触达表 | `backend/nyanpasu-rpc/tests/fixtures/permission-mapping-report.txt`（生成） |
| 核对             | `backend/nyanpasu-rpc/src/permission_baseline/mapping.rs`（仅测试）         |

映射表一行一个对象：`kind | name | permission | namespace | sensitive | override | targets | origin`。`origin` 是声明现在所在的文件，供搬迁卡使用。

| 测试                                                 | 判据                                                                                                                                                     |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mapping_covers_the_baseline_with_its_exposure`      | 标识集合等于基线；每个对象的暴露面与基线相同                                                                                                             |
| `requirements_follow_the_reference_derivation`       | 指定的要求等于按设计 §3.2 从命名空间推导的结果（资源 = 顶层，级别 = effect 或例外，事件、路由、流为 `read`）；例外只出现在命令上且确实改变级别，数量 ≤ 9 |
| `sensitive_markings_carry_over_every_sensitive_read` | 带敏感标记的集合恰好是现在要求 `sensitive_read` 的对象（35 条命令、7 个事件、日志归档路由），其新要求都是 `read`                                         |
| `meta_and_remote_keep_their_boundaries`              | `meta` 下只有 `rpc_describe`；`remote` 的对象无一对 HTTP 暴露                                                                                            |
| `every_admission_difference_has_a_category`          | 跨模型比较（见下）的每一格差异都属于设计 §7 的某一类                                                                                                     |
| `registered_top_modules_hold_no_legacy_declarations` | 按本文的搬迁顺序，每张卡登记顶层模块时，同名的现有源文件里没有属于尚未登记资源的声明                                                                     |
| `roles_expand_like_the_design`                       | 五个角色的展开结果等于手写期望                                                                                                                           |
| `mapping_report_is_fresh`                            | 提交的变更清单与触达表等于重新计算的结果                                                                                                                 |

**跨模型比较。** 对每个对象、每种传输、未解析调用者与旧 scope 的全部 128 个子集，比较两种准入：旧的是基线（持有该 scope，`rpc_describe` 除外）；新的是映射表，调用者的 scope 按设计 §7 换算成权限，并且已解析的调用者都持有 `meta:read`，`write` 满足 `read`。暴露面不变，所以差异只来自要求。

另外，映射表生成时还做了一次独立核对：要求分别从“旧 scope 按 §7 换算、取命名空间所在的领域”和“命名空间按 §3.2 推导”两条路算出，二者对 225 个对象全部一致。也就是说，每个对象被放进的领域都是它的旧 scope 换算后允许的领域。

更新（只在评审过映射之后）：

```sh
cargo test -p nyanpasu-rpc --lib permission_baseline::mapping::export_permission_mapping_report -- --ignored
```

## 资源表（定稿草案）

与设计 §2.1 相同，八项，序列化名是 snake_case：`meta`、`proxy`、`profile`、`settings`、`system`、`logs`、`traffic`、`remote`。每一项的覆盖范围见下文“按命名空间”，与设计 §2.1 的表一致；以下归属是落实时做的取舍：

| 对象                                                                          | 归属              | 理由                                                                                                                      |
| ----------------------------------------------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------- |
| 规则与规则提供者的读取                                                        | `proxy.rules`     | 运行中路由的状态；而更新规则提供者是旧 `profile` scope，归 `profile.providers`                                            |
| 运行时配置的读取与检视（`get_runtime_*`、`inspect_*runtime*`、后处理输出）    | `profile.runtime` | 内容是配置文件组合出的结果                                                                                                |
| Clash 的 `/configs` 与控制器信息（`clash_api_get_configs`、`get_clash_info`） | `settings.clash`  | 与 `get_clash_config`、`patch_clash_config` 同属 Clash 偏好                                                               |
| 内核版本、内核目录、内核更新检视、agent skill                                 | `system.core`     | 内核状态与启停属于 `system`                                                                                               |
| `get_http_server_status`                                                      | `system.status`   | 它对 HTTP 暴露，不能进 `remote`（`remote` 不对 HTTP 暴露）                                                                |
| 热键（含 `set_hotkeys`）                                                      | `system.hotkeys`  | 旧 scope 是 `system`，属于 OS 集成                                                                                        |
| `view_profile`、`collect_logs`                                                | `system.os`       | 旧 scope 是 `system`：它们打开外部编辑器、弹出保存对话框，是 OS 集成而不是配置或日志操作                                  |
| `icon_fetch` 路由与托盘、小组件图标                                           | `system.icons`    | 图标文件属于 OS 集成                                                                                                      |
| `state-changed-event`                                                         | `settings.app`    | 只是失效通知，不含内容                                                                                                    |
| IP 查询与网络自动化                                                           | `system.network`  | 网络自动化切换的是系统级的捕获；`probe_direct_egress`、`test_stun_server` 的旧 scope 是 `settings`，归 `settings.network` |

## 角色表（定稿草案）

与设计 §2.2 相同。展开结果（`R` 读，`W` 写，空即无）：

| 角色        | `meta` | `proxy` | `profile` | `settings` | `system` | `logs` | `traffic` | `remote` | 可触达的敏感对象 |
| ----------- | ------ | ------- | --------- | ---------- | -------- | ------ | --------- | -------- | ---------------- |
| `anonymous` | R      |         |           |            |          |        |           |          | 0                |
| `viewer`    | R      | R       | R         | R          | R        | R      | R         |          | 46（全部）       |
| `operator`  | R      | W       | W         | W          | R        | R      | W         |          | 46               |
| `admin`     | R      | W       | W         | W          | W        | W      | W         |          | 46               |
| `owner`     | R      | W       | W         | W          | W        | W      | W         | W        | 46               |

敏感对象全部要求某个领域的 `read`，而 `viewer` 已持有全部六个领域的 `read`，所以从 `viewer` 起每个角色都能触达全部 46 个；名单在 `permission-mapping-report.txt`。这正是设计 §7 第一类的后果：真正起区分作用的是自定义映射，与预留的数据共享层。

`anonymous` 默认触达 0 个。给它附加某个领域的 `read` 后可以触达的敏感对象：

| 附加            | 可触达的敏感对象                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `proxy:read`    | `get_proxy_group_icons`                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `profile:read`  | `get_profiles`、`read_profile_file`、`probe_profile_url`、`get_profile_sync_logs`、`get_profile_sync_runs`、`get_profile_sync_status`、`get_runtime_config`、`get_runtime_yaml`、`inspect_runtime`、`inspect_applied_runtime`、`inspect_runtime_node`、`get_postprocessing_output`                                                                                                                                                                        |
| `settings:read` | `get_app_config`、`get_clash_config`、`get_clash_info`、`clash_api_get_configs`、`get_all_storage_items`、`get_storage_item`、`storage-value-changed-event`、`get_sync_settings`、`list_sync_conflicts`、`sync-status-changed-event`                                                                                                                                                                                                                      |
| `system:read`   | `get_core_status`、`get_core_dir`、`get_core_agent_skills`、`inspect_updater`、`status_service`、`get_service_install_prompt`、`get_configuration_status`、`get_app_update_state`、`get_transparent_proxy_status`、`get_network_automation_status`、`get_custom_app_dir`、`read_clipboard_text`，以及事件 `core-status-changed-event`、`service-status-changed-event`、`configuration-status-changed`、`app-update-state-changed`、`window-message-event` |
| `logs:read`     | `get_core_log`、`get_core_log_status`、`query_core_logs`、`query_logs`、`core-logs-changed`、`logs_archive` 路由                                                                                                                                                                                                                                                                                                                                          |
| `traffic:read`  | 无                                                                                                                                                                                                                                                                                                                                                                                                                                                        |

## 例外清单

共 9 个，等于设计 §3.3 的初始预算 `MAX_OVERRIDES = 9`，没有余量：

| 命令                                   | effect   | 例外             | 原因                                          |
| -------------------------------------- | -------- | ---------------- | --------------------------------------------- |
| `clash_api_get_group_delay`            | query    | `access = write` | 延迟测试触发代理流量（旧 scope `proxy`）      |
| `clash_api_get_proxy_delay`            | query    | `access = write` | 同上                                          |
| `url_delay_test`                       | query    | `access = write` | 同上                                          |
| `open_log_session`                     | mutation | `access = read`  | 只分配调用者自己的日志游标（旧 scope `read`） |
| `close_log_session`                    | mutation | `access = read`  | 只释放调用者自己的日志游标                    |
| `query_logs`                           | mutation | `access = read`  | 推进调用者自己的游标（旧 `sensitive_read`）   |
| `query_logs_summary`                   | mutation | `access = read`  | 同上，摘要视图                                |
| `subscribe_clash_connection_details`   | mutation | `access = read`  | 只订阅调用者自己的连接详情流                  |
| `unsubscribe_clash_connection_details` | mutation | `access = read`  | 只退订                                        |

设计 §3.3 的表把 `query_logs_summary` 计在“日志游标的推进”里，数量 6 与此一致。

## 无投影的凭证与敏感值

设计 §2.2 要求列出“返回值是凭证或敏感值且没有投影”的操作及其保护方式。

**返回凭证的操作。** `create_remote_access_token` 返回 token 原文，`create_http_launch_url` 返回带 launch code 的地址。二者都要求 `remote:write`，只有 `owner`（桌面与本地 socket）持有；不对 HTTP 暴露，自定义映射不能含 `remote`。保护方式与现在相同。

**返回敏感值的操作。** 现在要求 `sensitive_read` 的 35 条命令都没有声明 `privacy` 投影：它们的全部保护就是这个 scope，旁边通常另有一个不敏感的 `*_summary` 或 `*_listing` 视图。7 个敏感事件与日志归档路由同样如此。新模型下它们要求所在领域的 `read`，保护方式是：

1. 带敏感标记，`rpc_describe` 与授权界面据此列出（设计 §2.3）；
2. `anonymous` 默认不持有任何领域权限（C-1）；
3. 交出前的确认由预留的数据共享层负责（设计 §4.5，本稿不实现）。

声明了 `privacy` 的命令（错误详情与结果类型的投影）不在此列，它们的过渡规则是 C-2。

## 映射表

### 按命名空间

记号：`*` 敏感标记；`↑` 例外 `access = write`；`↓` 例外 `access = read`；`[d]` 只对桌面暴露，`[d,h]` 桌面与 HTTP，`[d,l]` 桌面与本地 socket，`[h]`、`[h,l]` 不含桌面（路由与流）；不写即三种传输都暴露。事件、路由、流分别以 `event:`、`route:`、`stream:` 开头。

**`meta`**（1）

| 命名空间 | 读             | 写  |
| -------- | -------------- | --- |
| `meta`   | `rpc_describe` |     |

**`proxy`**（14）

| 命名空间            | 读                                                               | 写                                                                               |
| ------------------- | ---------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `proxy.connections` |                                                                  | `clash_api_delete_connections`                                                   |
| `proxy.delay`       |                                                                  | `clash_api_get_group_delay`↑、`clash_api_get_proxy_delay`↑、`url_delay_test`↑[d] |
| `proxy.groups`      | `get_proxies`、`get_proxy_group_icons`\*[d,h]                    | `clear_proxy_fixed`[d,h]、`mutate_proxies`[d,h]、`select_proxy`                  |
| `proxy.mode`        |                                                                  | `set_proxy_mode`                                                                 |
| `proxy.providers`   | `clash_api_get_providers_proxies`[d,h]                           | `clash_api_healthcheck_proxy_provider`[d,h]                                      |
| `proxy.rules`       | `clash_api_get_providers_rules`[d,h]、`clash_api_get_rules`[d,h] |                                                                                  |

**`profile`**（36）

| 命名空间            | 读                                                                                                                                                                                                               | 写                                                                                                                                                                                                                                                                                                                                                 |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `profile.content`   | `probe_profile_url`\*[d]、`read_profile_file`\*                                                                                                                                                                  | `save_profile_file`[d,h]                                                                                                                                                                                                                                                                                                                           |
| `profile.layout`    |                                                                                                                                                                                                                  | `create_profile_folder`[d,h]、`create_proxy_group_folder`[d,h]、`set_profile_layout`[d,h]、`set_proxy_group_layout`[d,h]                                                                                                                                                                                                                           |
| `profile.providers` |                                                                                                                                                                                                                  | `clash_api_update_providers_rules`[d,h]、`update_proxy_provider`[d,h]                                                                                                                                                                                                                                                                              |
| `profile.runtime`   | `get_postprocessing_output`\*[d,h]、`get_runtime_config`\*[d,h]、`get_runtime_exists`[d,h]、`get_runtime_yaml`\*[d,h]、`inspect_applied_runtime`\*[d,h]、`inspect_runtime`\*[d,h]、`inspect_runtime_node`\*[d,h] |                                                                                                                                                                                                                                                                                                                                                    |
| `profile.store`     | `get_profiles`\*、`get_profiles_listing`                                                                                                                                                                         | `activate_profile`、`create_profile`、`delete_profile`、`enhance_profiles`[d,h]、`import_profile`、`patch_profile_metadata`[d,h]、`patch_remote_profile_options`[d,h]、`reorder_profile`[d,h]、`reorder_profiles_by_list`[d,h]、`replace_profile_definition`[d,h]、`set_global_transforms`[d,h]、`set_profile_valid_fields`[d,h]、`update_profile` |
| `profile.sync`      | `get_profile_sync_logs`\*[d,h]、`get_profile_sync_runs`\*[d,h]、`get_profile_sync_runs_summary`[d,h]、`get_profile_sync_status`\*[d,h]、`get_profile_sync_status_summary`[d,h]                                   |                                                                                                                                                                                                                                                                                                                                                    |

**`settings`**（36）

| 命名空间           | 读                                                                                                                                                                         | 写                                                                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings.app`     | `get_app_config`\*[d,h]、`get_app_settings`、`get_release_channel`[d]、`event:state-changed-event`[d,h]                                                                    | `patch_app_config`、`set_release_channel`[d]                                                                                                               |
| `settings.backup`  |                                                                                                                                                                            | `create_config_backup`[d]                                                                                                                                  |
| `settings.clash`   | `clash_api_get_configs`\*[d,h]、`clash_api_get_settings`[d,h]、`get_clash_config`\*[d,h]、`get_clash_info`\*[d,h]、`get_clash_settings`                                    | `patch_clash_config`[d,h]、`patch_runtime_overrides`[d,h]                                                                                                  |
| `settings.network` |                                                                                                                                                                            | `probe_direct_egress`[d,h]、`test_stun_server`[d]                                                                                                          |
| `settings.storage` | `get_all_storage_items`\*[d,h]、`get_storage_item`\*、`event:storage-resync-event`[d,h]、`event:storage-value-changed-event`\*[d,h]                                        | `clear_storage`[d,h]、`remove_storage_item`、`set_storage_item`                                                                                            |
| `settings.sync`    | `get_sync_provider_kinds`[d]、`get_sync_settings`\*[d]、`get_sync_status`[d]、`list_sync_conflicts`\*[d]、`list_sync_snapshots`[d]、`event:sync-status-changed-event`\*[d] | `configure_sync`[d]、`disable_sync`[d]、`enable_sync`[d]、`resolve_sync_conflict`[d]、`restore_sync_snapshot`[d]、`sync_now`[d]、`test_sync_connection`[d] |

**`system`**（101）

| 命名空间         | 读                                                                                                                                                                                                                                                                                                                                                                                                        | 写                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `system.core`    | `clash_api_get_version`、`fetch_latest_core_versions`[d,h]、`get_config_features`[d,h]、`get_core_agent_skills`\*[d,h]、`get_core_agent_skills_summary`[d,h]、`get_core_dir`\*[d,h]、`get_core_status`\*、`get_core_status_summary`、`get_core_version`[d]、`inspect_updater`\*[d,h]、`inspect_updater_summary`[d,h]、`event:core-status-changed-event`\*、`event:core-status-summary-changed-event`[d,h] | `change_clash_core`、`open_core_dir`[d]、`restart_sidecar`、`update_core`[d]                                                                                                                                                                                                                                                                                                                                                                                                              |
| `system.hotkeys` | `get_hotkey_functions`[d,h]、`get_hotkeys`[d,h]                                                                                                                                                                                                                                                                                                                                                           | `set_hotkeys`[d]                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `system.icons`   | `is_tray_icon_set`[d,h]、`is_widget_icon_set`[d,h]、`route:icon_fetch`[h]                                                                                                                                                                                                                                                                                                                                 | `set_tray_icon`[d]、`set_tray_icon_from_bytes`[d,h]、`set_widget_icon`[d]、`set_widget_icon_from_bytes`[d,h]                                                                                                                                                                                                                                                                                                                                                                              |
| `system.network` | `get_ip_lookup_providers`[d,h]、`get_ipsb_asn`[d]、`get_network_automation_status`\*[d]                                                                                                                                                                                                                                                                                                                   | `apply_network_automation`[d]、`request_wifi_name_access`[d]                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `system.os`      | `get_custom_app_dir`\*[d]、`get_system_accent_color`[d,h]、`is_appimage`[d,h]、`is_portable`[d,h]、`read_clipboard_text`\*[d]、`event:scheme-request-received-event`[d]                                                                                                                                                                                                                                   | `ask_native_dialog`[d]、`collect_logs`[d]、`copy_clash_env`[d]、`flush_system_dns_cache`[d]、`install_cli`[d]、`invoke_uwp_tool`[d]、`open_app_config_dir`[d]、`open_app_data_dir`[d]、`open_backups_dir`[d]、`open_logs_dir`[d]、`open_native_file_dialog`[d]、`open_that`[d]、`open_web_url`[d]、`report_issue`[d]、`set_custom_app_dir`[d]、`show_native_message_dialog`[d]、`show_native_notification`[d]、`take_pending_deep_links`[d]、`view_profile`[d]、`write_clipboard_text`[d] |
| `system.proxy`   | `get_sys_proxy`、`get_transparent_proxy_status`\*[d]、`get_transparent_proxy_status_summary`[d]                                                                                                                                                                                                                                                                                                           | `patch_clash_system_config`、`patch_runtime_access`[d,h]、`patch_system_settings`                                                                                                                                                                                                                                                                                                                                                                                                         |
| `system.service` | `get_service_install_prompt`\*[d,h]、`get_service_status_summary`[d,h]、`status_service`\*[d,h]、`event:service-status-changed-event`\*、`event:service-status-summary-changed-event`[d,h]                                                                                                                                                                                                                | `install_service`[d]、`restart_service`[d]、`set_service_mode`[d]、`start_service`[d]、`stop_service`[d]、`uninstall_service`[d]                                                                                                                                                                                                                                                                                                                                                          |
| `system.status`  | `get_configuration_status`\*、`get_configuration_status_summary`、`get_http_server_status`、`event:configuration-status-changed`\*、`event:configuration-status-summary-changed`[d,h]                                                                                                                                                                                                                     | `retry_configuration_effect`[d,h]、`retry_configuration_runtime`[d,h]                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `system.update`  | `get_app_update_changelog`[d]、`get_app_update_state`\*[d]、`get_app_update_summary`[d]、`event:app-update-state-changed`\*、`event:app-update-summary-changed`[d,h]                                                                                                                                                                                                                                      | `cancel_app_update_download`[d]、`check_app_update`[d]、`discard_app_update_package`[d]、`download_app_update`[d]、`install_app_update`[d]                                                                                                                                                                                                                                                                                                                                                |
| `system.window`  | `event:window-message-event`\*[d]                                                                                                                                                                                                                                                                                                                                                                         | `create_debug_tray_menu_window`[d]、`create_editor_window`[d]、`create_main_window`[d]、`quit_application`[d]、`report_window_ready`[d]、`restart_application`[d]、`save_window_size_state`[d]、`simulate_crash`[d]                                                                                                                                                                                                                                                                       |

**`logs`**（15）

| 命名空间    | 读                                                                                                                                                                                                                | 写                            |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| `logs.app`  | `close_log_session`↓、`list_log_files`、`open_log_session`↓、`query_logs`\*↓、`query_logs_summary`↓[d,h]、`route:logs_archive`\*[h]                                                                               | `report_frontend_events`[d,h] |
| `logs.core` | `get_core_log`\*[d,h]、`get_core_log_status`\*[d,h]、`get_core_log_status_summary`[d,h]、`query_core_logs`\*、`query_core_logs_summary`[d,h]、`event:core-logs-changed`\*、`event:core-logs-summary-changed`[d,h] | `clear_core_logs`[d,h]        |

**`traffic`**（13）

| 命名空间              | 读                                                                                                                                                                                                                                                           | 写                                                           |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| `traffic.connections` | `get_clash_ws_snapshot`[d,h]、`query_traffic_active_connection_ids`、`query_traffic_closed_connections`[d,h]、`subscribe_clash_connection_details`↓[d]、`unsubscribe_clash_connection_details`↓[d]、`event:clash-ws-event`、`stream:connection_details`[h,l] | `clear_clash_ws_history`[d,h]、`set_clash_ws_recording`[d,h] |
| `traffic.usage`       | `get_traffic_summary`、`query_traffic_report`[d,h]、`query_traffic_usage`[d,h]、`query_traffic_usage_by_keys`[d,h]                                                                                                                                           |                                                              |

**`remote`**（9）

| 命名空间          | 读                                  | 写                                                                   |
| ----------------- | ----------------------------------- | -------------------------------------------------------------------- |
| `remote.sessions` | `list_remote_browser_sessions`[d,l] | `create_http_launch_url`[d]、`revoke_remote_browser_session`[d,l]    |
| `remote.settings` | `get_local_socket_status`[d,l]      | `reset_remote_access_tls`[d,l]、`set_remote_access`[d,l]             |
| `remote.tokens`   | `list_remote_access_tokens`[d,l]    | `create_remote_access_token`[d,l]、`revoke_remote_access_token`[d,l] |

### 要求的分布

| 资源       | `read` | `write` | 敏感 | 对 HTTP 暴露 | 只对桌面 |
| ---------- | -----: | ------: | ---: | -----------: | -------: |
| `meta`     |      1 |       0 |    0 |            1 |        0 |
| `proxy`    |      5 |       9 |    1 |           13 |        1 |
| `profile`  |     16 |      20 |   12 |           35 |        1 |
| `settings` |     19 |      17 |   10 |           19 |       17 |
| `system`   |     46 |      55 |   17 |           43 |       58 |
| `logs`     |     13 |       2 |    6 |           15 |        0 |
| `traffic`  |     11 |       2 |    0 |           11 |        2 |
| `remote`   |      3 |       6 |    0 |            0 |        1 |
| 合计       |    114 |     111 |   46 |          137 |       80 |

## 变更清单

跨模型比较的结果：110 个对象的准入有变化，每一格都属于设计 §7 的第一类或第二类，没有无法归类的差异。其余 115 个对象在所有传输与调用者上的准入不变。

| 类别                 | 对象数 | 比较里的表现                                                                                                                                                                                               |
| -------------------- | -----: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1 敏感读不再单独设防 |     46 | 持有领域 `read`（旧 `read` 或该领域的写 scope）而没有 `sensitive_read` 的调用者，现在被放行；只持有 `sensitive_read` 而没有 `read` 的调用者，现在被拒绝，因为 `sensitive_read` 换算后不对应任何权限        |
| 2 写包含读           |     64 | 只持有某个写 scope（`proxy`、`profile`、`settings`、`system`、`remote_admin`）而没有 `read` 的调用者，现在可以读该领域；`settings` 换算为 `settings`、`logs`、`traffic` 三个领域的写，所以也能读日志与流量 |
| 3 写回到所属资源     |      0 | 换算后不变，比较里不出现。涉及 4 条命令：`clear_core_logs`、`report_frontend_events` → `logs:write`；`clear_clash_ws_history`、`set_clash_ws_recording` → `traffic:write`。它们的旧 scope 都是 `settings`  |
| 4 读可以按资源授予   |      0 | 新能力，旧 scope 换算不出部分领域的读，比较里不出现                                                                                                                                                        |
| 5 匿名的默认值       |      — | 不在比较的调用者里：它改的是 `local_open` 下无凭证请求得到的授权（旧：`local_open_scopes`，默认 `read`；新：`anonymous` 只有能力发现），由 P3-3 的调用者构造点测试覆盖                                     |

第一类中“只持有 `sensitive_read` 的调用者被拒绝”是设计 §7 没有写明的一面，归入第一类：它同样源于 `sensitive_read` 的消失。远程访问尚未发布，没有已持久化的 token 受影响。

完整清单（`类别` 列：1、2 同上）：

| 资源                                         | 旧 scope         | 新要求          | 类别 |
| -------------------------------------------- | ---------------- | --------------- | ---- |
| `clash_api_get_configs`                      | `sensitive_read` | `settings:read` | 1    |
| `clash_api_get_providers_proxies`            | `read`           | `proxy:read`    | 2    |
| `clash_api_get_providers_rules`              | `read`           | `proxy:read`    | 2    |
| `clash_api_get_rules`                        | `read`           | `proxy:read`    | 2    |
| `clash_api_get_settings`                     | `read`           | `settings:read` | 2    |
| `clash_api_get_version`                      | `read`           | `system:read`   | 2    |
| `close_log_session`                          | `read`           | `logs:read`     | 2    |
| `fetch_latest_core_versions`                 | `read`           | `system:read`   | 2    |
| `get_all_storage_items`                      | `sensitive_read` | `settings:read` | 1    |
| `get_app_config`                             | `sensitive_read` | `settings:read` | 1    |
| `get_app_settings`                           | `read`           | `settings:read` | 2    |
| `get_app_update_changelog`                   | `read`           | `system:read`   | 2    |
| `get_app_update_state`                       | `sensitive_read` | `system:read`   | 1    |
| `get_app_update_summary`                     | `read`           | `system:read`   | 2    |
| `get_clash_config`                           | `sensitive_read` | `settings:read` | 1    |
| `get_clash_info`                             | `sensitive_read` | `settings:read` | 1    |
| `get_clash_settings`                         | `read`           | `settings:read` | 2    |
| `get_clash_ws_snapshot`                      | `read`           | `traffic:read`  | 2    |
| `get_config_features`                        | `read`           | `system:read`   | 2    |
| `get_configuration_status`                   | `sensitive_read` | `system:read`   | 1    |
| `get_configuration_status_summary`           | `read`           | `system:read`   | 2    |
| `get_core_agent_skills`                      | `sensitive_read` | `system:read`   | 1    |
| `get_core_agent_skills_summary`              | `read`           | `system:read`   | 2    |
| `get_core_dir`                               | `sensitive_read` | `system:read`   | 1    |
| `get_core_log`                               | `sensitive_read` | `logs:read`     | 1    |
| `get_core_log_status`                        | `sensitive_read` | `logs:read`     | 1    |
| `get_core_log_status_summary`                | `read`           | `logs:read`     | 2    |
| `get_core_status`                            | `sensitive_read` | `system:read`   | 1    |
| `get_core_status_summary`                    | `read`           | `system:read`   | 2    |
| `get_core_version`                           | `read`           | `system:read`   | 2    |
| `get_custom_app_dir`                         | `sensitive_read` | `system:read`   | 1    |
| `get_hotkey_functions`                       | `read`           | `system:read`   | 2    |
| `get_hotkeys`                                | `read`           | `system:read`   | 2    |
| `get_http_server_status`                     | `read`           | `system:read`   | 2    |
| `get_ip_lookup_providers`                    | `read`           | `system:read`   | 2    |
| `get_ipsb_asn`                               | `read`           | `system:read`   | 2    |
| `get_network_automation_status`              | `sensitive_read` | `system:read`   | 1    |
| `get_postprocessing_output`                  | `sensitive_read` | `profile:read`  | 1    |
| `get_profile_sync_logs`                      | `sensitive_read` | `profile:read`  | 1    |
| `get_profile_sync_runs`                      | `sensitive_read` | `profile:read`  | 1    |
| `get_profile_sync_runs_summary`              | `read`           | `profile:read`  | 2    |
| `get_profile_sync_status`                    | `sensitive_read` | `profile:read`  | 1    |
| `get_profile_sync_status_summary`            | `read`           | `profile:read`  | 2    |
| `get_profiles`                               | `sensitive_read` | `profile:read`  | 1    |
| `get_profiles_listing`                       | `read`           | `profile:read`  | 2    |
| `get_proxies`                                | `read`           | `proxy:read`    | 2    |
| `get_proxy_group_icons`                      | `sensitive_read` | `proxy:read`    | 1    |
| `get_release_channel`                        | `read`           | `settings:read` | 2    |
| `get_runtime_config`                         | `sensitive_read` | `profile:read`  | 1    |
| `get_runtime_exists`                         | `read`           | `profile:read`  | 2    |
| `get_runtime_yaml`                           | `sensitive_read` | `profile:read`  | 1    |
| `get_service_install_prompt`                 | `sensitive_read` | `system:read`   | 1    |
| `get_service_status_summary`                 | `read`           | `system:read`   | 2    |
| `get_storage_item`                           | `sensitive_read` | `settings:read` | 1    |
| `get_sync_provider_kinds`                    | `read`           | `settings:read` | 2    |
| `get_sync_settings`                          | `sensitive_read` | `settings:read` | 1    |
| `get_sync_status`                            | `read`           | `settings:read` | 2    |
| `get_sys_proxy`                              | `read`           | `system:read`   | 2    |
| `get_system_accent_color`                    | `read`           | `system:read`   | 2    |
| `get_traffic_summary`                        | `read`           | `traffic:read`  | 2    |
| `get_transparent_proxy_status`               | `sensitive_read` | `system:read`   | 1    |
| `get_transparent_proxy_status_summary`       | `read`           | `system:read`   | 2    |
| `inspect_applied_runtime`                    | `sensitive_read` | `profile:read`  | 1    |
| `inspect_runtime`                            | `sensitive_read` | `profile:read`  | 1    |
| `inspect_runtime_node`                       | `sensitive_read` | `profile:read`  | 1    |
| `inspect_updater`                            | `sensitive_read` | `system:read`   | 1    |
| `inspect_updater_summary`                    | `read`           | `system:read`   | 2    |
| `is_appimage`                                | `read`           | `system:read`   | 2    |
| `is_portable`                                | `read`           | `system:read`   | 2    |
| `is_tray_icon_set`                           | `read`           | `system:read`   | 2    |
| `is_widget_icon_set`                         | `read`           | `system:read`   | 2    |
| `list_log_files`                             | `read`           | `logs:read`     | 2    |
| `list_sync_conflicts`                        | `sensitive_read` | `settings:read` | 1    |
| `list_sync_snapshots`                        | `read`           | `settings:read` | 2    |
| `open_log_session`                           | `read`           | `logs:read`     | 2    |
| `probe_profile_url`                          | `sensitive_read` | `profile:read`  | 1    |
| `query_core_logs`                            | `sensitive_read` | `logs:read`     | 1    |
| `query_core_logs_summary`                    | `read`           | `logs:read`     | 2    |
| `query_logs`                                 | `sensitive_read` | `logs:read`     | 1    |
| `query_logs_summary`                         | `read`           | `logs:read`     | 2    |
| `query_traffic_active_connection_ids`        | `read`           | `traffic:read`  | 2    |
| `query_traffic_closed_connections`           | `read`           | `traffic:read`  | 2    |
| `query_traffic_report`                       | `read`           | `traffic:read`  | 2    |
| `query_traffic_usage`                        | `read`           | `traffic:read`  | 2    |
| `query_traffic_usage_by_keys`                | `read`           | `traffic:read`  | 2    |
| `read_clipboard_text`                        | `sensitive_read` | `system:read`   | 1    |
| `read_profile_file`                          | `sensitive_read` | `profile:read`  | 1    |
| `status_service`                             | `sensitive_read` | `system:read`   | 1    |
| `subscribe_clash_connection_details`         | `read`           | `traffic:read`  | 2    |
| `unsubscribe_clash_connection_details`       | `read`           | `traffic:read`  | 2    |
| `event:app-update-state-changed`             | `sensitive_read` | `system:read`   | 1    |
| `event:app-update-summary-changed`           | `read`           | `system:read`   | 2    |
| `event:clash-ws-event`                       | `read`           | `traffic:read`  | 2    |
| `event:configuration-status-changed`         | `sensitive_read` | `system:read`   | 1    |
| `event:configuration-status-summary-changed` | `read`           | `system:read`   | 2    |
| `event:core-logs-changed`                    | `sensitive_read` | `logs:read`     | 1    |
| `event:core-logs-summary-changed`            | `read`           | `logs:read`     | 2    |
| `event:core-status-changed-event`            | `sensitive_read` | `system:read`   | 1    |
| `event:core-status-summary-changed-event`    | `read`           | `system:read`   | 2    |
| `event:scheme-request-received-event`        | `read`           | `system:read`   | 2    |
| `event:service-status-changed-event`         | `sensitive_read` | `system:read`   | 1    |
| `event:service-status-summary-changed-event` | `read`           | `system:read`   | 2    |
| `event:state-changed-event`                  | `read`           | `settings:read` | 2    |
| `event:storage-resync-event`                 | `read`           | `settings:read` | 2    |
| `event:storage-value-changed-event`          | `sensitive_read` | `settings:read` | 1    |
| `event:sync-status-changed-event`            | `sensitive_read` | `settings:read` | 1    |
| `event:window-message-event`                 | `sensitive_read` | `system:read`   | 1    |
| `route:icon_fetch`                           | `read`           | `system:read`   | 2    |
| `route:logs_archive`                         | `sensitive_read` | `logs:read`     | 1    |
| `stream:connection_details`                  | `read`           | `traffic:read`  | 2    |

## 搬迁卡的顺序

与 tasks.md 的顺序相同，测试 `registered_top_modules_hold_no_legacy_declarations` 按此核对：

| 卡   | 登记的顶层模块       | 对象数 | 其中 native | 注意                                                                                                                                                                       |
| ---- | -------------------- | -----: | ----------: | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P4-1 | `meta`、`remote`     |     10 |           0 | `create_remote_access_token` 等在 `remote_access.rs`，`set_remote_access`、`reset_remote_access_tls` 在 `host.rs`，`get_local_socket_status` 在 `status.rs`                |
| P4-2 | `proxy`              |     14 |           1 | 规则读取与提供者的健康检查归此处；更新提供者归 `profile`                                                                                                                   |
| P4-3 | `profile`            |     36 |           1 | 运行时配置的读取与检视来自 `runtime.rs`                                                                                                                                    |
| P4-4 | `settings`、`system` |    137 |          55 | 最大的一张卡，可按 `settings` 与 `system` 拆成两个 PR；`ipc.rs` 里的 native 命令几乎都在这里                                                                               |
| P4-5 | `logs`、`traffic`    |     28 |           2 | 现有的 `commands/logs.rs`、`commands/traffic.rs` 与新顶层模块同名；`traffic.rs` 里的内核日志命令（`get_core_log`、`query_core_logs` 等）须与 `traffic` 同一张卡移入 `logs` |

“native”指声明在 `tauri/src/ipc.rs` 的命令，它们随所属资源搬迁，位于只对桌面暴露的子模块。按卡计数的总和是 225，其中 native 59 条（tasks.md 写 58，同样是旧数字）。

## 与设计稿的出入

- 设计 §2.3、§7 与 tasks.md 写“34 条 `sensitive_read` 命令”“189 条命令、208 个资源”，那是设计稿成稿时的数字。现在的基线是 37 条、205 条、225 个（rebase 到含配置同步的 main 之后）（[baseline.md](baseline.md)）；本文与测试都用现在的数字。
- 任务卡要求“现有 34 条 `sensitive_read` 命令……都带 `sensitive` 标记”。本文把敏感集合定为**恰好**等于现有的 `sensitive_read` 集合，没有借机扩大：例如 `get_profiles_listing` 等安全视图仍不带标记。之后增加标记属于评审过的变更。
- 设计 §3.2 的示例把 `read_profile_file` 放在 `profile.content` 并声明整个子树 `sensitive`。本文只记录每个对象是否带标记；是写在命名空间上还是命令上，由 P4 各卡按子树的组成决定（例如 `profile.sync` 里的两个摘要不带标记，所以那里写在命令上）。

## 已确认

以下几项于 2026-11-18 按本稿确认（设计 §10），P3-2 据此写会持久化的类型与名字。

| 编号 | 问题                               | 本稿的取舍                                                                                      |
| ---- | ---------------------------------- | ----------------------------------------------------------------------------------------------- |
| C-1  | `anonymous` 的权限怎么配置         | 角色只有能力发现；`anonymous_permissions` 映射往上加，上限是 `operator` 的展开，不能含 `remote` |
| C-2  | 切换时脱敏的过渡规则               | 持有被调用资源的 `write` 才得到未脱敏的版本                                                     |
| C-3  | launch code 建立的会话是否 `admin` | 是                                                                                              |
| M-1  | 本文的资源归属                     | 见“资源表”一节的取舍与下文的映射表；资源名、角色名、序列化名照设计稿                            |
