//! The permission model in isolation: every requirement in the resource table, checked against
//! grants whose expected answers are written independently.

use nyanpasu_config::application::{Access, Permission, Permissions, Resource, Role};

use super::{RESOURCES, ResourceMetadata};

const DOMAINS: [Resource; 8] = [
    Resource::Meta,
    Resource::Proxy,
    Resource::Profile,
    Resource::Settings,
    Resource::System,
    Resource::Logs,
    Resource::Traffic,
    Resource::Remote,
];

/// Each role's expansion, written out by hand.
fn role_table(role: Role) -> &'static [(Resource, Access)] {
    use Access::{Read as R, Write as W};
    use Resource::*;
    match role {
        Role::Anonymous => &[],
        Role::Viewer => &[
            (Proxy, R),
            (Profile, R),
            (Settings, R),
            (System, R),
            (Logs, R),
            (Traffic, R),
        ],
        Role::Operator => &[
            (Proxy, W),
            (Profile, W),
            (Settings, W),
            (System, R),
            (Logs, R),
            (Traffic, W),
        ],
        Role::Admin => &[
            (Proxy, W),
            (Profile, W),
            (Settings, W),
            (System, W),
            (Logs, W),
            (Traffic, W),
        ],
        Role::Owner => &[
            (Proxy, W),
            (Profile, W),
            (Settings, W),
            (System, W),
            (Logs, W),
            (Traffic, W),
            (Remote, W),
        ],
    }
}

/// The expected answer, by the rules alone: `meta:read` always; otherwise the resource is held
/// at the required level or above (`write` includes `read`).
fn expected(held: &[(Resource, Access)], required: Permission) -> bool {
    required == Permission::META_READ
        || held.iter().any(|(resource, access)| {
            *resource == required.resource
                && (*access == required.access || *access == Access::Write)
        })
}

#[test]
fn every_requirement_is_decided_for_single_permissions() {
    let mut grants = vec![(Permissions::NONE, Vec::new())];
    for resource in DOMAINS {
        for access in [Access::Read, Access::Write] {
            grants.push((
                Permissions::NONE.with(resource, access),
                vec![(resource, access)],
            ));
        }
    }
    for ResourceMetadata {
        name, permission, ..
    } in RESOURCES
    {
        for (grant, held) in &grants {
            assert_eq!(
                grant.permits(*permission),
                expected(held, *permission),
                "{name} requires {permission}, held {held:?}"
            );
        }
    }
}

#[test]
fn every_requirement_is_decided_for_every_role() {
    for role in [
        Role::Anonymous,
        Role::Viewer,
        Role::Operator,
        Role::Admin,
        Role::Owner,
    ] {
        let permissions = role.permissions();
        for ResourceMetadata {
            name, permission, ..
        } in RESOURCES
        {
            assert_eq!(
                permissions.permits(*permission),
                expected(role_table(role), *permission),
                "{name} requires {permission}, role {role}"
            );
        }
    }
}

/// Only `rpc_describe` is admitted without any permission; the owner is admitted everywhere.
#[test]
fn the_extremes_of_the_role_table() {
    let open: Vec<_> = RESOURCES
        .iter()
        .filter(|resource| Permissions::NONE.permits(resource.permission))
        .map(|resource| resource.name)
        .collect();
    assert_eq!(open, ["rpc_describe"]);
    assert!(
        RESOURCES
            .iter()
            .all(|resource| Role::Owner.permissions().permits(resource.permission))
    );
}
