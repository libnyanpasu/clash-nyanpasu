# Legacy IVerge Call Network

This document is the Phase 0 baseline for the three-StateActor migration. It records the legacy `Config::*` call network and the field ownership map required before persistence ownership moves from mixed `IVerge` state to typed `nyanpasu-config` state.

Phase 0 is intentionally metadata-only:

- no new actors or clients;
- no production bridge conversions;
- no legacy call-site rewrites;
- no new global service accessors.

## Legacy accessor summary

| Accessor             | Current role                                                        | Migration status                                                                       |
| -------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `Config::verge()`    | Mixed application, session/window, and partial Clash settings owner | Split across Application, Session, and Clash typed actors in later phases              |
| `Config::clash()`    | Raw Clash override mapping owner                                    | Mapped to persistent `ClashConfig` in later phases; runtime Clash API remains separate |
| `Config::profiles()` | Profile index and profile file owner                                | Out of scope for the three-StateActor migration                                        |
| `Config::runtime()`  | Generated/runtime config bookkeeping owner                          | Out of scope; must not be conflated with persistent `ClashConfig`                      |

## Call-network hotspots

### `Config::verge()`

Representative groups:

- Startup and initialization: `backend/tauri/src/lib.rs`, `backend/tauri/src/utils/init/mod.rs`, `backend/tauri/src/utils/init/logging.rs`.
- Window/session state: `backend/tauri/src/window.rs`, `backend/tauri/src/utils/resolve.rs`, `backend/tauri/src/ipc.rs`.
- Core/runtime behavior: `backend/tauri/src/core/clash/core.rs`, `backend/tauri/src/core/service/mod.rs`, `backend/tauri/src/core/service/ipc.rs`.
- System integration: `backend/tauri/src/core/sysopt.rs`, `backend/tauri/src/core/hotkey.rs`, `backend/tauri/src/core/tray/mod.rs`, `backend/tauri/src/core/tray/proxies.rs`.
- Command/API writes: `backend/tauri/src/feat.rs`, `backend/tauri/src/ipc.rs`.
- Config generation/enhancement: `backend/tauri/src/enhance/mod.rs`, `backend/tauri/src/enhance/tun.rs`, `backend/tauri/src/enhance/advice.rs`, `backend/tauri/src/config/clash/mod.rs`.

Highest-risk writes:

- `backend/tauri/src/feat.rs` patches `Config::verge().draft()` and calls `save_file()`.
- `backend/tauri/src/window.rs` persists window state through `Config::verge().data().save_file()`.
- `backend/tauri/src/utils/resolve.rs` patches random port state and saves legacy Verge/Clash files.
- `backend/tauri/src/core/clash/core.rs` mutates `clash_core`, applies/discards Verge and Runtime drafts, and saves Verge.
- `backend/tauri/src/ipc.rs` already contains reseed comments for legacy core/window writes.

### `Config::clash()`

Representative groups:

- Clash patch commands: `backend/tauri/src/feat.rs`.
- Config enhancement: `backend/tauri/src/enhance/mod.rs`, `backend/tauri/src/config/core.rs`.
- Port/controller resolution: `backend/tauri/src/utils/resolve.rs`, `backend/tauri/src/utils/config.rs`, `backend/tauri/src/config/profile/item/remote.rs`.
- Runtime core reload: `backend/tauri/src/core/clash/core.rs`.
- Live API info reads: `backend/tauri/src/core/clash/api.rs`, `backend/tauri/src/core/clash/ws.rs`, `backend/tauri/src/ipc.rs`.

Important constraint: `clash_api_get_configs` must continue to represent live Clash runtime API state. `patch_clash_config` must continue to accept `PatchRuntimeConfig` and patch the live Clash runtime API before any persistent mirror/update path. Persistent `nyanpasu_config::clash::config::ClashConfig` is a saved configuration domain, not a replacement for runtime API responses or runtime patch commands.

Frontend/API compatibility constraint: `get_verge_config`, `patch_verge_config`, `clash_api_get_configs`, and `patch_clash_config` are stable frontend-facing commands during this migration. Their generated binding names (`commands.getVergeConfig`, `commands.patchVergeConfig`, `commands.clashApiGetConfigs`, `commands.patchClashConfig`) must not change in Phase 0-3.

Patch compatibility constraint: `patch_verge_config` currently accepts partial `IVerge` payloads from the frontend. Later typed bridges must preserve absent-field/no-op behavior and must not require frontend callers to send a complete typed app/session/clash snapshot.

### `Config::profiles()`

Representative groups:

- Profile CRUD and file writes: `backend/tauri/src/ipc.rs`.
- Profile jobs: `backend/tauri/src/core/tasks/jobs/profiles.rs`.
- Enhancement reads: `backend/tauri/src/enhance/mod.rs`.
- Command helpers: `backend/tauri/src/feat.rs`.

Migration note: profile persistence remains unchanged in this migration.

### `Config::runtime()`

Representative groups:

- Runtime patch command paths: `backend/tauri/src/feat.rs`.
- Runtime query commands: `backend/tauri/src/ipc.rs`.
- Core config state: `backend/tauri/src/config/core.rs`.
- Clash core lifecycle rollback/apply paths: `backend/tauri/src/core/clash/core.rs`.

Migration note: runtime state remains unchanged and must stay separate from persistent Clash config.

## IVerge field mapping

| Legacy field                | Owner       | Target                                             | Note                                                              |
| --------------------------- | ----------- | -------------------------------------------------- | ----------------------------------------------------------------- |
| `app_singleton_port`        | Application | `NyanpasuAppConfig.app_singleton_port`             | Preserve                                                          |
| `app_log_level`             | Application | `NyanpasuAppConfig.app_log_level`                  | Preserve                                                          |
| `language`                  | Application | `NyanpasuAppConfig.language`                       | Convert legacy string to typed language                           |
| `theme_mode`                | Application | `NyanpasuAppConfig.theme_mode`                     | Convert legacy string to typed theme mode                         |
| `traffic_graph`             | Application | `NyanpasuAppConfig.traffic_graph`                  | Preserve                                                          |
| `enable_memory_usage`       | Application | `NyanpasuAppConfig.enable_memory_usage`            | Preserve                                                          |
| `lighten_animation_effects` | Application | `NyanpasuAppConfig.lighten_animation_effects`      | Preserve                                                          |
| `enable_tun_mode`           | Clash       | `ClashConfig.enable_tun_mode`                      | Move to Clash domain                                              |
| `enable_service_mode`       | Application | `NyanpasuAppConfig.enable_service_mode`            | Preserve                                                          |
| `enable_auto_launch`        | Application | `NyanpasuAppConfig.enable_auto_launch`             | Preserve                                                          |
| `enable_silent_start`       | Application | `NyanpasuAppConfig.enable_silent_start`            | Preserve                                                          |
| `enable_system_proxy`       | Application | `NyanpasuAppConfig.enable_system_proxy`            | Preserve                                                          |
| `enable_proxy_guard`        | Application | `NyanpasuAppConfig.enable_proxy_guard`             | Preserve                                                          |
| `system_proxy_bypass`       | Application | `NyanpasuAppConfig.system_proxy_bypass`            | Preserve                                                          |
| `proxy_guard_interval`      | Application | `NyanpasuAppConfig.proxy_guard_interval`           | Preserve alias behavior                                           |
| `theme_color`               | Application | `NyanpasuAppConfig.theme_color`                    | Preserve validation behavior                                      |
| `web_ui_list`               | Clash       | `ClashConfig.web_ui_list`                          | Move to Clash domain                                              |
| `clash_core`                | Application | `NyanpasuAppConfig.core`                           | Rename target field                                               |
| `hotkeys`                   | Application | `NyanpasuAppConfig.hotkeys`                        | Preserve until hotkey actor consumes typed config                 |
| `auto_close_connection`     | Clash       | `ClashConfig.break_connection`                     | Deprecated; backfills break strategy when newer fields are absent |
| `break_when_proxy_change`   | Clash       | `ClashConfig.break_connection`                     | Fold into typed break strategy                                    |
| `break_when_profile_change` | Clash       | `ClashConfig.break_connection`                     | Fold into typed break strategy                                    |
| `break_when_mode_change`    | Clash       | `ClashConfig.break_connection`                     | Fold into typed break strategy                                    |
| `default_latency_test`      | Application | `NyanpasuAppConfig.default_latency_test`           | Preserve                                                          |
| `enable_clash_fields`       | Clash       | `ClashConfig.enable_clash_fields`                  | Move to Clash domain                                              |
| `enable_builtin_enhanced`   | Application | `NyanpasuAppConfig.enable_builtin_enhanced`        | Preserve                                                          |
| `proxy_layout_column`       | Application | `NyanpasuAppConfig.proxy_layout_column`            | Preserve                                                          |
| `auto_log_clean`            | Discard     | none                                               | Deprecated; superseded by `max_log_files`                         |
| `max_log_files`             | Application | `NyanpasuAppConfig.max_log_files`                  | Preserve                                                          |
| `window_size_position`      | Session     | `PersistentState.window_state`                     | Deprecated fallback for main-window geometry                      |
| `window_size_state`         | Session     | `PersistentState.window_state`                     | Move to Session domain                                            |
| `enable_random_port`        | Clash       | `ClashConfig.mixed_port`                           | Convert to port strategy semantics                                |
| `verge_mixed_port`          | Clash       | `ClashConfig.mixed_port`                           | Convert to fixed mixed-port strategy                              |
| `enable_auto_check_update`  | Application | `NyanpasuAppConfig.enable_auto_check_update`       | Preserve                                                          |
| `clash_strategy`            | Clash       | `ClashConfig.external_controller`, port strategies | Split into typed Clash strategies                                 |
| `clash_tray_selector`       | Application | `NyanpasuAppConfig.tray_selector_mode`             | Rename target field                                               |
| `always_on_top`             | Application | `NyanpasuAppConfig.always_on_top`                  | Preserve                                                          |
| `tun_stack`                 | Clash       | `ClashConfig.tun_stack`                            | Move to Clash domain                                              |
| `network_statistic_widget`  | Application | `NyanpasuAppConfig.network_statistic_widget`       | Preserve                                                          |
| `pac_url`                   | Application | `NyanpasuAppConfig.pac_url`                        | Parse string URL explicitly                                       |
| `enable_tray_text`          | Application | `NyanpasuAppConfig.enable_tray_text`               | Preserve                                                          |
| `window_type`               | Application | `NyanpasuAppConfig.use_legacy_ui`                  | Convert UI selection semantics                                    |
| `tray_menu_mode`            | Application | `NyanpasuAppConfig.tray_menu_mode`                 | Preserve                                                          |
| `tray_menu_close_behavior`  | Application | `NyanpasuAppConfig.tray_menu_close_behavior`       | Preserve                                                          |

## Phased migration batches

1. **Baseline metadata**: add this document and mapping coverage tests. No runtime behavior changes.
2. **Typed infrastructure without takeover**: add bridge traits, typed actors, typed clients, and typed facade APIs, but keep legacy API routes unchanged.
3. **Production bridges and migration V2**: implement conversion bridges and run one-time file migration before actor spawn.
4. **Legacy read API compatibility**: route `get_verge_config` through typed snapshots while preserving frontend DTOs, command names, generated bindings, query keys, and mutation payloads.
5. **Hot legacy write reseed**: wrap direct writes in `feat.rs`, `window.rs`, `utils/resolve.rs`, `core/clash/core.rs`, and related IPC paths.
6. **Cleanup**: remove old `StateActor` / `StateClient` only after typed facade and compatibility tests pass.

## Verification

```bash
cargo test --manifest-path backend/Cargo.toml iverge_mapping
rg -n "get_verge_config|patch_verge_config|clash_api_get_configs|patch_clash_config" backend/tauri/src/ipc.rs
rg -n "getVergeConfig|patchVergeConfig|clashApiGetConfigs|patchClashConfig" frontend/interface/src/ipc/bindings.ts
rg -n "NYANPASU_SETTING_QUERY_KEY|CLASH_CONFIG_QUERY_KEY|nyanpasu://mutation|nyanpasu_config|clash_config|profiles|proxies" frontend/interface/src backend/tauri/src
rg -n "Config::(verge|clash|profiles|runtime)\(" backend/tauri/src
rg -n "save_file\(|save_config\(" backend/tauri/src
```

Generated binding safety gate:

- `get_verge_config` must continue returning frontend-compatible `IVerge`.
- `patch_verge_config` must continue accepting partial `IVerge` payloads.
- `clash_api_get_configs` must continue returning runtime API `clash::api::ClashConfig`.
- `patch_clash_config` must continue accepting runtime API `PatchRuntimeConfig`.
- The `nyanpasu://mutation` event payloads must remain `nyanpasu_config`, `clash_config`, `profiles`, and `proxies`.
