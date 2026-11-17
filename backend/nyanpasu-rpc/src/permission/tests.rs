use nyanpasu_contracts::auth::{Access, Permission, Permissions, Resource};
use nyanpasu_rpc_protocol::{
    CommandEffect, CommandKind, EventName, RpcTarget, ShellCapability,
    permission::{PermissionSource, RESOURCES, ResourceKind, ResourceMetadata},
};

use super::{Authority, CONNECTION_DETAILS, ICON_FETCH, LOGS_ARCHIVE, declared};

const PROXY_COMMAND: ResourceMetadata = ResourceMetadata {
    name: "select_proxy",
    namespace: "clash",
    kind: ResourceKind::Command(CommandKind::Shared),
    effect: CommandEffect::Mutation,
    permission: Permission::new(Resource::Proxy, Access::Write),
    source: PermissionSource::Legacy,
    sensitive: false,
    targets: &[RpcTarget::Desktop, RpcTarget::Http],
    capabilities: &[ShellCapability::Window],
    optional_capabilities: &[],
};

fn kind(result: Result<(), nyanpasu_rpc_protocol::RpcError>) -> &'static str {
    result.err().map_or("ok", |error| error.kind)
}

/// `401` → `unsupported` → `403`: each refusal wins over every later one. `unknown_method` sits
/// between `401` and `unsupported` and is the registry's, which resolves the name first.
#[test]
fn refusals_follow_the_admission_order() {
    let none = Permissions::NONE;
    let proxy = Permissions::NONE.with(Resource::Proxy, Access::Write);
    let with_window = Authority::new(&[ShellCapability::Window]);
    let without_window = Authority::new(&[]);

    // Unresolved: 401 even where the resource is not offered and the permission is missing.
    assert_eq!(
        kind(without_window.admit(&PROXY_COMMAND, RpcTarget::Local, None)),
        "unauthorized"
    );
    // Not offered on the transport, or lacking a capability: unsupported before 403.
    assert_eq!(
        kind(with_window.admit(&PROXY_COMMAND, RpcTarget::Local, Some(&none))),
        "unsupported"
    );
    assert_eq!(
        kind(without_window.admit(&PROXY_COMMAND, RpcTarget::Http, Some(&none))),
        "unsupported"
    );
    // Offered: the requirement decides, and the refusal names the missing permission.
    let refused = with_window
        .admit(&PROXY_COMMAND, RpcTarget::Http, Some(&none))
        .unwrap_err();
    assert_eq!(refused.kind, "forbidden");
    assert_eq!(refused.message, "missing permission proxy:write");
    assert_eq!(
        kind(with_window.admit(&PROXY_COMMAND, RpcTarget::Http, Some(&proxy))),
        "ok"
    );
}

#[test]
fn visibility_is_the_admission_predicate() {
    let authority = Authority::new(&[ShellCapability::Window]);
    for target in [RpcTarget::Desktop, RpcTarget::Http, RpcTarget::Local] {
        for permissions in [
            Permissions::NONE,
            Permissions::NONE.with(Resource::Proxy, Access::Read),
            Permissions::NONE.with(Resource::Proxy, Access::Write),
        ] {
            assert_eq!(
                authority.visible(&PROXY_COMMAND, target, &permissions),
                authority
                    .admit(&PROXY_COMMAND, target, Some(&permissions))
                    .is_ok(),
                "{target:?} {permissions:?}"
            );
        }
    }
}

/// `rpc_describe` is open to every resolved caller because it requires `meta:read`, which every
/// resolved caller holds, not because of its name.
#[test]
fn any_resolved_caller_may_describe_itself() {
    let describe = RESOURCES
        .iter()
        .find(|resource| resource.name == "rpc_describe")
        .unwrap();
    assert_eq!(describe.permission, Permission::META_READ);
    let authority = Authority::new(&[]);
    assert!(
        authority
            .admit(describe, RpcTarget::Http, Some(&Permissions::NONE))
            .is_ok()
    );
    // The same name with another requirement is refused.
    let renamed = ResourceMetadata {
        permission: Permission::new(Resource::Proxy, Access::Read),
        ..*describe
    };
    assert!(
        authority
            .admit(&renamed, RpcTarget::Http, Some(&Permissions::NONE))
            .is_err()
    );
}

#[test]
fn transports_admit_declared_routes_and_streams() {
    assert_eq!(declared(ICON_FETCH).permission.access, Access::Read);
    assert_eq!(declared(ICON_FETCH).targets, &[RpcTarget::Http]);
    assert_eq!(
        declared(LOGS_ARCHIVE).permission,
        Permission::new(Resource::Logs, Access::Read)
    );
    assert!(declared(LOGS_ARCHIVE).sensitive);
    assert_eq!(
        declared(CONNECTION_DETAILS).targets,
        &[RpcTarget::Http, RpcTarget::Local]
    );
    for event in EventName::ALL {
        assert_eq!(event.resource().kind, ResourceKind::Event);
    }
}

/// Admission never reads the sensitive marking: two rows that differ only in it are admitted
/// alike for every caller.
#[test]
fn the_sensitive_marking_takes_no_part_in_admission() {
    let marked = ResourceMetadata {
        sensitive: true,
        ..PROXY_COMMAND
    };
    let authority = Authority::new(&[ShellCapability::Window]);
    for permissions in [
        Permissions::NONE,
        Permissions::NONE.with(Resource::Proxy, Access::Read),
        Permissions::NONE.with(Resource::Proxy, Access::Write),
    ] {
        for target in [RpcTarget::Desktop, RpcTarget::Http] {
            assert_eq!(
                kind(authority.admit(&marked, target, Some(&permissions))),
                kind(authority.admit(&PROXY_COMMAND, target, Some(&permissions)))
            );
        }
    }
}
