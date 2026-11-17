//! Resource rows of the test-only `rpc` declarations.
//!
//! The generator reads only the production declaration roots, so the commands that tests declare
//! to exercise dispatch, auditing and privacy have their rows written here. Each row repeats the
//! declaration it belongs to; the registry refuses a linked handler that has no row.

use nyanpasu_contracts::auth::{Access, Permission, Resource};
use nyanpasu_rpc_protocol::{
    CommandEffect, CommandKind, RpcTarget, ShellCapability,
    permission::{PermissionSource, ResourceKind, ResourceMetadata},
};

const HTTP: &[RpcTarget] = &[RpcTarget::Desktop, RpcTarget::Http];

const fn mutation(
    name: &'static str,
    kind: CommandKind,
    permission: Permission,
    targets: &'static [RpcTarget],
    capabilities: &'static [ShellCapability],
    optional_capabilities: &'static [ShellCapability],
) -> ResourceMetadata {
    ResourceMetadata {
        name,
        namespace: "test",
        kind: ResourceKind::Command(kind),
        effect: CommandEffect::Mutation,
        permission,
        source: PermissionSource::Derived,
        sensitive: false,
        targets,
        capabilities,
        optional_capabilities,
    }
}

const fn shared_http(name: &'static str, permission: Permission) -> ResourceMetadata {
    mutation(name, CommandKind::Shared, permission, HTTP, &[], &[])
}

const fn write(resource: Resource) -> Permission {
    Permission::new(resource, Access::Write)
}

/// The privacy canaries only read their resource, so a reader reaches the redacted projection
/// and a writer the full response.
const fn read(resource: Resource) -> Permission {
    Permission::new(resource, Access::Read)
}

pub(crate) const TEST_RESOURCES: &[ResourceMetadata] = &[
    // `test_support`
    shared_http("audit_echo", write(Resource::Settings)),
    // `registry/audit_tests.rs`
    shared_http("audit_panic", write(Resource::Settings)),
    shared_http("audit_mutation", read(Resource::Settings)),
    shared_http("audit_mutation_alias", read(Resource::Settings)),
    mutation(
        "local_optional_effect",
        CommandKind::Shared,
        write(Resource::Settings),
        &[RpcTarget::Desktop, RpcTarget::Local],
        &[],
        &[ShellCapability::Elevation],
    ),
    mutation(
        "requires_clipboard",
        CommandKind::Shared,
        write(Resource::Settings),
        HTTP,
        &[ShellCapability::Clipboard],
        &[],
    ),
    // `privacy_tests.rs`
    shared_http(
        "privacy_canary_read_mutation",
        Permission::new(Resource::Proxy, Access::Read),
    ),
    shared_http("privacy_canary_proxy_mutation", read(Resource::Proxy)),
    shared_http("privacy_canary_profile_mutation", read(Resource::Profile)),
    shared_http("privacy_canary_settings_mutation", read(Resource::Settings)),
    shared_http("privacy_canary_system_mutation", read(Resource::System)),
    shared_http(
        "privacy_canary_remote_admin_mutation",
        read(Resource::Remote),
    ),
    shared_http("privacy_canary_app_update_mutation", read(Resource::System)),
    shared_http("privacy_canary_network_status", read(Resource::System)),
    // The server's `panic-injection` binary and its tests, which submit entries by hand.
    mutation(
        "e4b_test_panic",
        CommandKind::Shared,
        write(Resource::Settings),
        &[RpcTarget::Http, RpcTarget::Local],
        &[],
        &[],
    ),
    mutation(
        "e4b_test_owner_work",
        CommandKind::Shared,
        write(Resource::Settings),
        &[RpcTarget::Local],
        &[],
        &[],
    ),
    // The desktop shell's `unified_rpc/audit_tests.rs`.
    mutation(
        "audit_native_echo",
        CommandKind::Native,
        write(Resource::Settings),
        &[RpcTarget::Desktop],
        &[ShellCapability::Presentation],
        &[],
    ),
];
