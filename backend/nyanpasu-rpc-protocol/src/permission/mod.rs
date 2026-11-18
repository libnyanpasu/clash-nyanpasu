//! Authorization metadata of every protected resource, generated from the declarations.
//!
//! A resource is anything a caller is admitted to: a command, an event, an HTTP route or a
//! stream. The table in [`RESOURCES`] is the only source of a resource's requirement, exposure
//! and capabilities: the `rpc` attribute registers implementations and carries none of it.

use crate::{CommandEffect, CommandKind, RpcTarget, Scope, ShellCapability};

mod generated;

pub use generated::RESOURCES;

/// What kind of object a resource is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// An RPC command, dispatched natively on the desktop or through a shared handler.
    Command(CommandKind),
    /// A shared event, admitted when subscribed and filtered on every delivery.
    Event,
    /// A direct HTTP route outside `/bridge/rpc`.
    Route,
    /// A long-lived stream outside the event bus.
    Stream,
}

/// Where a resource's requirement comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionSource {
    /// Written on the declaration as `scope = ...`, before the namespace tree derives it.
    Legacy,
}

/// The authorization facts of one resource, known whether or not its handler is linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceMetadata {
    /// Stable wire name.
    pub name: &'static str,
    /// The logical module path of the declaration below its declaration root, dot-separated.
    pub namespace: &'static str,
    pub kind: ResourceKind,
    /// Whether the resource only reads.
    pub effect: CommandEffect,
    /// The one scope a principal must hold.
    pub scope: Scope,
    pub source: PermissionSource,
    /// Explicit transport exposure.
    pub targets: &'static [RpcTarget],
    /// Facilities required to execute the resource.
    pub capabilities: &'static [ShellCapability],
    /// Reachable conditional effects, never a blanket admission requirement.
    pub optional_capabilities: &'static [ShellCapability],
}

/// The resource of `kind` named `name`.
pub fn find(kind: ResourceKind, name: &str) -> Option<&'static ResourceMetadata> {
    RESOURCES
        .iter()
        .find(|resource| resource.kind == kind && resource.name == name)
}

impl ResourceMetadata {
    /// Whether this resource is a command of either kind.
    pub fn is_command(&self) -> bool {
        matches!(self.kind, ResourceKind::Command(_))
    }

    /// Whether this is a command that needs a linked native wrapper on the desktop.
    pub fn is_native_command(&self) -> bool {
        self.kind == ResourceKind::Command(CommandKind::Native)
    }
}

#[cfg(test)]
mod tests {
    use super::{PermissionSource, RESOURCES, ResourceMetadata};
    use crate::{CommandEffect, RpcTarget, Scope, ShellCapability};

    /// Every committed requirement is a reviewed legacy declaration until the commands move into
    /// the namespace tree; this budget only goes down, to zero.
    const MAX_LEGACY: usize = 225;

    #[test]
    fn legacy_declarations_stay_within_their_budget() {
        let legacy = RESOURCES
            .iter()
            .filter(|resource| resource.source == PermissionSource::Legacy)
            .count();
        assert_eq!(
            legacy, MAX_LEGACY,
            "lower MAX_LEGACY when declarations move"
        );
    }

    fn names(filter: impl Fn(&ResourceMetadata) -> bool) -> Vec<&'static str> {
        let mut names: Vec<_> = RESOURCES
            .iter()
            .filter(|command| command.is_command() && filter(command))
            .map(|command| command.name)
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    #[test]
    fn latency_measurements_require_proxy_authority() {
        for name in [
            "clash_api_get_proxy_delay",
            "clash_api_get_group_delay",
            "url_delay_test",
        ] {
            let command = RESOURCES.iter().find(|item| item.name == name).unwrap();
            assert_eq!(command.effect, CommandEffect::Query, "{name}");
            assert_eq!(command.scope, Scope::Proxy, "{name}");
        }
    }

    /// Adding a secret-bearing query, or demoting one, is a reviewed change to this list.
    #[test]
    fn sensitive_reads_are_pinned() {
        assert_eq!(
            names(|command| command.scope == Scope::SensitiveRead),
            [
                "clash_api_get_configs",
                "get_all_storage_items",
                "get_app_config",
                "get_app_update_state",
                "get_clash_config",
                "get_clash_info",
                "get_configuration_status",
                "get_core_agent_skills",
                "get_core_dir",
                "get_core_log",
                "get_core_log_status",
                "get_core_status",
                "get_custom_app_dir",
                "get_network_automation_status",
                "get_postprocessing_output",
                "get_profile_sync_logs",
                "get_profile_sync_runs",
                "get_profile_sync_status",
                "get_profiles",
                "get_proxy_group_icons",
                "get_runtime_config",
                "get_runtime_yaml",
                "get_service_install_prompt",
                "get_storage_item",
                "get_sync_settings",
                "get_transparent_proxy_status",
                "inspect_applied_runtime",
                "inspect_runtime",
                "inspect_runtime_node",
                "inspect_updater",
                "list_sync_conflicts",
                "probe_profile_url",
                "query_core_logs",
                "query_logs",
                "read_clipboard_text",
                "read_profile_file",
                "status_service",
            ]
        );
    }

    /// `remote_admin` belongs to the desktop shell and the same-user local socket. HTTP never
    /// offers it, because no bearer token or browser session can hold the scope.
    #[test]
    fn remote_administration_is_never_an_http_command() {
        for command in RESOURCES.iter().filter(|resource| resource.is_command()) {
            if command.scope == Scope::RemoteAdmin {
                assert!(
                    command.targets.contains(&RpcTarget::Desktop),
                    "{}",
                    command.name
                );
                assert!(
                    !command.targets.contains(&RpcTarget::Http),
                    "{} must not expose HTTP",
                    command.name
                );
            }
        }
        assert_eq!(
            names(|command| command.scope == Scope::RemoteAdmin),
            [
                "create_http_launch_url",
                "create_remote_access_token",
                "get_local_socket_status",
                "list_remote_access_tokens",
                "list_remote_browser_sessions",
                "reset_remote_access_tls",
                "revoke_remote_access_token",
                "revoke_remote_browser_session",
                "set_remote_access",
            ]
        );
        // The launch URL is a credential for a browser, so it stays with the desktop that opens it.
        assert_eq!(
            names(|command| command.scope == Scope::RemoteAdmin
                && command.targets.contains(&RpcTarget::Local)),
            [
                "create_remote_access_token",
                "get_local_socket_status",
                "list_remote_access_tokens",
                "list_remote_browser_sessions",
                "reset_remote_access_tls",
                "revoke_remote_access_token",
                "revoke_remote_browser_session",
                "set_remote_access",
            ]
        );
    }

    /// A shell or OS facility is never reachable by a scope below `system`.
    #[test]
    fn shell_capabilities_need_the_system_scope() {
        for command in RESOURCES.iter().filter(|resource| resource.is_command()) {
            let shell = command.capabilities.iter().any(|capability| {
                !matches!(
                    capability,
                    ShellCapability::CallerWebview | ShellCapability::HttpServer
                )
            });
            if shell && command.effect == CommandEffect::Mutation {
                assert!(
                    matches!(command.scope, Scope::System | Scope::RemoteAdmin),
                    "{} needs a shell facility but holds {}",
                    command.name,
                    command.scope
                );
            }
        }
    }

    /// Scope is independent of target: a `system` command is HTTP-reachable only where it
    /// already was. This pins that set, so a new HTTP system command is a reviewed change.
    #[test]
    fn http_exposed_system_commands_are_pinned() {
        assert_eq!(
            names(|command| command.scope == Scope::System
                && command.targets.contains(&RpcTarget::Http)),
            [
                "change_clash_core",
                "patch_clash_system_config",
                "patch_runtime_access",
                "patch_system_settings",
                "restart_sidecar",
                "retry_configuration_effect",
                "retry_configuration_runtime",
                "set_tray_icon_from_bytes",
                "set_widget_icon_from_bytes",
            ]
        );
    }

    #[test]
    fn required_privileged_commands_are_not_http_targets() {
        for command in RESOURCES.iter().filter(|resource| resource.is_command()) {
            if command.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    ShellCapability::Elevation
                        | ShellCapability::Lifecycle
                        | ShellCapability::AppUpdate
                )
            }) {
                assert!(
                    !command.targets.contains(&RpcTarget::Http),
                    "{} must not expose HTTP",
                    command.name
                );
            }
        }
    }

    #[test]
    fn dedicated_settings_commands_are_desktop_only() {
        for name in ["set_hotkeys", "set_service_mode"] {
            let command = RESOURCES
                .iter()
                .find(|command| command.name == name)
                .unwrap();
            assert_eq!(command.targets, &[RpcTarget::Desktop]);
        }
        for name in [
            "retry_configuration_effect",
            "create_profile",
            "update_profile",
            "delete_profile",
        ] {
            let command = RESOURCES
                .iter()
                .find(|command| command.name == name)
                .unwrap();
            assert!(command.targets.contains(&RpcTarget::Http), "{name}");
        }
        let mut local: Vec<_> = RESOURCES
            .iter()
            .filter(|command| command.is_command() && command.targets.contains(&RpcTarget::Local))
            .map(|command| command.name)
            .collect();
        local.sort_unstable();
        assert_eq!(local.len(), 47);
        assert_eq!(
            local,
            [
                "activate_profile",
                "change_clash_core",
                "clash_api_delete_connections",
                "clash_api_get_group_delay",
                "clash_api_get_proxy_delay",
                "clash_api_get_version",
                "close_log_session",
                "create_profile",
                "create_remote_access_token",
                "delete_profile",
                "get_app_settings",
                "get_clash_settings",
                "get_configuration_status",
                "get_configuration_status_summary",
                "get_core_status",
                "get_core_status_summary",
                "get_http_server_status",
                "get_local_socket_status",
                "get_profiles",
                "get_profiles_listing",
                "get_proxies",
                "get_storage_item",
                "get_sys_proxy",
                "get_traffic_summary",
                "import_profile",
                "list_log_files",
                "list_remote_access_tokens",
                "list_remote_browser_sessions",
                "open_log_session",
                "patch_app_config",
                "patch_clash_system_config",
                "patch_system_settings",
                "query_core_logs",
                "query_logs",
                "query_traffic_active_connection_ids",
                "read_profile_file",
                "remove_storage_item",
                "reset_remote_access_tls",
                "restart_sidecar",
                "revoke_remote_access_token",
                "revoke_remote_browser_session",
                "rpc_describe",
                "select_proxy",
                "set_proxy_mode",
                "set_remote_access",
                "set_storage_item",
                "update_profile",
            ]
        );
        let patch = RESOURCES
            .iter()
            .find(|command| command.name == "patch_app_config")
            .unwrap();
        assert!(patch.targets.contains(&RpcTarget::Local));
        assert!(
            !patch
                .optional_capabilities
                .contains(&ShellCapability::Hotkey)
        );
        assert!(
            patch
                .optional_capabilities
                .contains(&ShellCapability::Elevation)
        );
    }
}
