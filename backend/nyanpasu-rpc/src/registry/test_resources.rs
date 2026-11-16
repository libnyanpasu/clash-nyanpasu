//! Resource rows of the test-only `rpc` declarations.
//!
//! The generator reads only the production declaration roots, so the commands that tests declare
//! to exercise dispatch, auditing and privacy have their rows written here. Each row repeats the
//! declaration it belongs to; the registry refuses a linked handler that has no row.

use nyanpasu_rpc_protocol::{
    CommandEffect, CommandKind, RpcTarget, Scope, ShellCapability,
    permission::{PermissionSource, ResourceKind, ResourceMetadata},
};

const HTTP: &[RpcTarget] = &[RpcTarget::Desktop, RpcTarget::Http];

const fn mutation(
    name: &'static str,
    kind: CommandKind,
    scope: Scope,
    targets: &'static [RpcTarget],
    capabilities: &'static [ShellCapability],
    optional_capabilities: &'static [ShellCapability],
) -> ResourceMetadata {
    ResourceMetadata {
        name,
        namespace: "test",
        kind: ResourceKind::Command(kind),
        effect: CommandEffect::Mutation,
        scope,
        source: PermissionSource::Legacy,
        targets,
        capabilities,
        optional_capabilities,
    }
}

const fn shared_http(name: &'static str, scope: Scope) -> ResourceMetadata {
    mutation(name, CommandKind::Shared, scope, HTTP, &[], &[])
}

pub(crate) const TEST_RESOURCES: &[ResourceMetadata] = &[
    // `test_support`
    shared_http("audit_echo", Scope::Settings),
    // `registry/audit_tests.rs`
    shared_http("audit_panic", Scope::Settings),
    shared_http("audit_mutation", Scope::Settings),
    shared_http("audit_mutation_alias", Scope::Settings),
    mutation(
        "local_optional_effect",
        CommandKind::Shared,
        Scope::Settings,
        &[RpcTarget::Desktop, RpcTarget::Local],
        &[],
        &[ShellCapability::Elevation],
    ),
    mutation(
        "requires_clipboard",
        CommandKind::Shared,
        Scope::Settings,
        HTTP,
        &[ShellCapability::Clipboard],
        &[],
    ),
    // `privacy_tests.rs`
    shared_http("privacy_canary_read_mutation", Scope::Read),
    shared_http("privacy_canary_proxy_mutation", Scope::Proxy),
    shared_http("privacy_canary_profile_mutation", Scope::Profile),
    shared_http("privacy_canary_settings_mutation", Scope::Settings),
    shared_http("privacy_canary_system_mutation", Scope::System),
    shared_http("privacy_canary_remote_admin_mutation", Scope::RemoteAdmin),
    shared_http("privacy_canary_app_update_mutation", Scope::System),
    shared_http("privacy_canary_network_status", Scope::System),
    // The server's `panic-injection` binary and its tests, which submit entries by hand.
    mutation(
        "e4b_test_panic",
        CommandKind::Shared,
        Scope::Settings,
        &[RpcTarget::Http, RpcTarget::Local],
        &[],
        &[],
    ),
    mutation(
        "e4b_test_owner_work",
        CommandKind::Shared,
        Scope::Settings,
        &[RpcTarget::Local],
        &[],
        &[],
    ),
    // The desktop shell's `unified_rpc/audit_tests.rs`.
    mutation(
        "audit_native_echo",
        CommandKind::Native,
        Scope::Settings,
        &[RpcTarget::Desktop],
        &[ShellCapability::Presentation],
        &[],
    ),
];
