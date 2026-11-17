use nyanpasu_rpc_protocol::{
    CommandEffect, CommandKind, EventName, RpcTarget, Scope, ScopeSet, ShellCapability,
    permission::{PermissionSource, ResourceKind, ResourceMetadata},
};

use super::{Authority, CONNECTION_DETAILS, ICON_FETCH, LOGS_ARCHIVE, declared};

const PROXY_COMMAND: ResourceMetadata = ResourceMetadata {
    name: "select_proxy",
    namespace: "clash",
    kind: ResourceKind::Command(CommandKind::Shared),
    effect: CommandEffect::Mutation,
    scope: Scope::Proxy,
    source: PermissionSource::Legacy,
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
    let none = ScopeSet::default();
    let proxy: ScopeSet = [Scope::Proxy].into_iter().collect();
    let with_window = Authority::new(&[ShellCapability::Window]);
    let without_window = Authority::new(&[]);

    // Unresolved: 401 even where the resource is not offered and the scope is missing.
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
    // Offered: the requirement decides.
    assert_eq!(
        kind(with_window.admit(&PROXY_COMMAND, RpcTarget::Http, Some(&none))),
        "forbidden"
    );
    assert_eq!(
        kind(with_window.admit(&PROXY_COMMAND, RpcTarget::Http, Some(&proxy))),
        "ok"
    );
}

#[test]
fn visibility_is_the_admission_predicate() {
    let proxy: ScopeSet = [Scope::Proxy].into_iter().collect();
    let authority = Authority::new(&[ShellCapability::Window]);
    for target in [RpcTarget::Desktop, RpcTarget::Http, RpcTarget::Local] {
        for scopes in [ScopeSet::default(), proxy.clone()] {
            assert_eq!(
                authority.visible(&PROXY_COMMAND, target, &scopes),
                authority
                    .admit(&PROXY_COMMAND, target, Some(&scopes))
                    .is_ok(),
                "{target:?} {scopes:?}"
            );
        }
    }
}

#[test]
fn any_resolved_caller_may_describe_itself() {
    let describe = ResourceMetadata {
        name: super::RPC_DESCRIBE_METHOD,
        scope: Scope::Read,
        capabilities: &[],
        ..PROXY_COMMAND
    };
    let authority = Authority::new(&[]);
    assert!(
        authority
            .admit(&describe, RpcTarget::Http, Some(&ScopeSet::default()))
            .is_ok()
    );
    // The exception is the command's, not the name's.
    let event = ResourceMetadata {
        kind: ResourceKind::Event,
        ..describe
    };
    assert!(
        authority
            .admit(&event, RpcTarget::Http, Some(&ScopeSet::default()))
            .is_err()
    );
}

#[test]
fn transports_admit_declared_routes_and_streams() {
    assert_eq!(declared(ICON_FETCH).scope, Scope::Read);
    assert_eq!(declared(ICON_FETCH).targets, &[RpcTarget::Http]);
    assert_eq!(declared(LOGS_ARCHIVE).scope, Scope::SensitiveRead);
    assert_eq!(
        declared(CONNECTION_DETAILS).targets,
        &[RpcTarget::Http, RpcTarget::Local]
    );
    for event in EventName::ALL {
        assert_eq!(event.resource().kind, ResourceKind::Event);
    }
}
