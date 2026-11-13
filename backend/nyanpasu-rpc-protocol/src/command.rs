//! Declaration metadata, independent of linked command implementations.

use nyanpasu_config::application::Scope;

/// A command's explicitly enabled transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcTarget {
    /// The complete desktop command surface.
    Desktop,
    /// Browser access, explicitly opted into by the declaration.
    Http,
    /// Local socket access, independently opted into by the declaration.
    Local,
}

/// What a command does, independent of how a transport dispatches it.
///
/// Queries observe application or network state; refreshing a read model or measuring latency
/// does not change application configuration. Mutations perform application operations, including
/// allocating caller-owned resources. Scope independently controls who may request either effect.
/// The declaration is the source of truth; the generated frontend's split is checked against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    /// Returns data and changes nothing.
    Query,
    /// Everything else.
    Mutation,
}

/// Whether desktop dispatch needs a native wrapper or a shared handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Requires a linked shell wrapper for desktop dispatch.
    Native,
    /// Uses explicit dependencies and a transport-supplied caller identity.
    Shared,
}

/// Reachable shell facilities; these declarations are not per-call authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCapability {
    /// Clipboard reads and writes.
    Clipboard,
    /// Native notifications.
    Notification,
    /// Native message and file dialogs.
    Dialog,
    /// Native caller windows, webviews and bound channels.
    CallerWebview,
    /// Window creation and state.
    Window,
    /// Shell process restart, quit and crash operations.
    Lifecycle,
    /// Opening paths, URLs, editors and reporters.
    Opener,
    /// Tray and widget presentation.
    Presentation,
    /// The shell's deep-link queue.
    DeepLink,
    /// Application package updates.
    AppUpdate,
    /// Global shortcuts.
    Hotkey,
    /// Unprivileged system proxy and autostart integration.
    OsIntegration,
    /// Reachable elevation effects, governed by injected execution policy.
    Elevation,
    /// Host accent color queries.
    AccentColor,
    /// Host HTTP server control.
    HttpServer,
}

/// Static facts generated from a command declaration, even without its handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandMetadata {
    /// Stable wire command name.
    pub name: &'static str,
    /// Desktop dispatch category. This is not the query/mutation effect.
    pub kind: CommandKind,
    /// Whether the command only reads.
    pub effect: CommandEffect,
    /// The one scope a principal must hold to call the command. Queries default to
    /// [`Scope::Read`]; every mutation names its scope explicitly.
    pub scope: Scope,
    /// Explicit transport exposure.
    pub targets: &'static [RpcTarget],
    /// Facilities required to execute the command.
    pub capabilities: &'static [ShellCapability],
    /// Reachable conditional effects, never a blanket admission requirement.
    pub optional_capabilities: &'static [ShellCapability],
}

#[cfg(test)]
mod tests {
    use super::{CommandEffect, RpcTarget, ShellCapability};
    use crate::{COMMAND_CATALOG, Scope};

    fn names(filter: impl Fn(&super::CommandMetadata) -> bool) -> Vec<&'static str> {
        let mut names: Vec<_> = COMMAND_CATALOG
            .iter()
            .filter(|command| filter(command))
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
            let command = COMMAND_CATALOG
                .iter()
                .find(|item| item.name == name)
                .unwrap();
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
                "get_clash_config",
                "get_clash_info",
                "get_configuration_status",
                "get_core_log",
                "get_core_log_status",
                "get_core_status",
                "get_postprocessing_output",
                "get_profile_sync_logs",
                "get_profile_sync_runs",
                "get_profile_sync_status",
                "get_profiles",
                "get_proxy_group_icons",
                "get_runtime_config",
                "get_runtime_yaml",
                "get_storage_item",
                "inspect_applied_runtime",
                "inspect_runtime",
                "inspect_runtime_node",
                "probe_profile_url",
                "query_core_logs",
                "query_logs",
                "read_clipboard_text",
                "read_profile_file",
                "status_service",
            ]
        );
    }

    #[test]
    fn remote_administration_is_never_a_remote_command() {
        for command in COMMAND_CATALOG {
            if command.scope == Scope::RemoteAdmin {
                assert_eq!(command.targets, &[RpcTarget::Desktop], "{}", command.name);
            }
        }
        assert_eq!(
            names(|command| command.scope == Scope::RemoteAdmin),
            ["create_http_launch_url", "set_remote_access"]
        );
    }

    /// A shell or OS facility is never reachable by a scope below `system`.
    #[test]
    fn shell_capabilities_need_the_system_scope() {
        for command in COMMAND_CATALOG {
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
        for command in COMMAND_CATALOG {
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
            let command = COMMAND_CATALOG
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
            let command = COMMAND_CATALOG
                .iter()
                .find(|command| command.name == name)
                .unwrap();
            assert!(command.targets.contains(&RpcTarget::Http), "{name}");
        }
        assert_eq!(
            COMMAND_CATALOG
                .iter()
                .filter(|command| command.targets.contains(&RpcTarget::Local))
                .count(),
            13
        );
        let patch = COMMAND_CATALOG
            .iter()
            .find(|command| command.name == "patch_app_config")
            .unwrap();
        assert!(!patch.targets.contains(&RpcTarget::Local));
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
