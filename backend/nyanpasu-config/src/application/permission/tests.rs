use std::collections::BTreeMap;

use strum::IntoEnumIterator as _;

use super::*;
use Access::{Read as R, Write as W};

fn map(pairs: &[(Resource, Access)]) -> Permissions {
    pairs.iter().copied().collect::<BTreeMap<_, _>>().into()
}

#[test]
fn write_includes_read_and_unlisted_resources_are_refused() {
    use Resource::{Logs, Proxy};
    // (held, required, permitted)
    let cases = [
        (None, R, false),
        (None, W, false),
        (Some(R), R, true),
        (Some(R), W, false),
        (Some(W), R, true),
        (Some(W), W, true),
    ];
    for (held, access, permitted) in cases {
        let held = held.map_or(Permissions::NONE, |held| {
            Permissions::NONE.with(Proxy, held)
        });
        assert_eq!(
            held.permits(Permission::new(Proxy, access)),
            permitted,
            "{held:?} {access}"
        );
        // Holding one resource never grants another.
        assert!(!held.permits(Permission::new(Logs, R)));
    }
}

#[test]
fn every_caller_holds_meta_read() {
    assert!(Permissions::NONE.permits(Permission::META_READ));
    assert!(!Permissions::NONE.permits(Permission::new(Resource::Meta, W)));
}

#[test]
fn permissions_serialize_as_a_map_of_held_levels() {
    let permissions = map(&[(Resource::Proxy, W), (Resource::Logs, R)]);
    let json = serde_json::to_value(permissions).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "proxy": "write", "logs": "read" })
    );
    assert_eq!(
        serde_json::from_value::<Permissions>(json).unwrap(),
        permissions
    );
    assert_eq!(
        serde_json::to_value(Permissions::NONE).unwrap(),
        serde_json::json!({})
    );
    for role in [
        Role::Anonymous,
        Role::Viewer,
        Role::Operator,
        Role::Admin,
        Role::Owner,
    ] {
        let permissions = role.permissions();
        let round_trip: Permissions =
            serde_json::from_value(serde_json::to_value(permissions).unwrap()).unwrap();
        assert_eq!(round_trip, permissions, "{role}");
    }
    assert!(
        serde_json::from_value::<Permissions>(serde_json::json!({ "secrets": "read" })).is_err()
    );
    assert!(
        serde_json::from_value::<Permissions>(serde_json::json!({ "proxy": "admin" })).is_err()
    );
}

#[test]
fn names_are_the_persisted_snake_case_names() {
    let names: Vec<_> = Resource::iter()
        .map(|resource| resource.to_string())
        .collect();
    assert_eq!(
        names,
        [
            "meta", "proxy", "profile", "settings", "system", "logs", "traffic", "remote"
        ]
    );
    for resource in Resource::iter() {
        assert_eq!(
            serde_json::to_value(resource).unwrap(),
            serde_json::json!(resource.to_string())
        );
    }
    assert_eq!(
        Permission::new(Resource::Proxy, W).to_string(),
        "proxy:write"
    );
    assert_eq!(
        serde_json::to_value(Role::Operator).unwrap(),
        serde_json::json!("operator")
    );
}

/// Each role's expansion, written out by hand rather than derived from the role table.
#[test]
fn roles_expand_to_their_reviewed_permissions() {
    use Resource::*;
    assert_eq!(Role::Anonymous.permissions(), Permissions::NONE);
    assert_eq!(
        Role::Viewer.permissions(),
        map(&[
            (Proxy, R),
            (Profile, R),
            (Settings, R),
            (System, R),
            (Logs, R),
            (Traffic, R),
        ])
    );
    assert_eq!(
        Role::Operator.permissions(),
        map(&[
            (Proxy, W),
            (Profile, W),
            (Settings, W),
            (System, R),
            (Logs, R),
            (Traffic, W),
        ])
    );
    assert_eq!(
        Role::Admin.permissions(),
        map(&[
            (Proxy, W),
            (Profile, W),
            (Settings, W),
            (System, W),
            (Logs, W),
            (Traffic, W),
        ])
    );
    assert_eq!(
        Role::Owner.permissions(),
        map(&[
            (Proxy, W),
            (Profile, W),
            (Settings, W),
            (System, W),
            (Logs, W),
            (Traffic, W),
            (Remote, W),
        ])
    );
}

/// A resource added later must be reachable from the desktop at least.
#[test]
fn the_owner_role_writes_every_resource_but_meta() {
    for resource in Resource::iter().filter(|resource| *resource != Resource::Meta) {
        assert_eq!(
            Role::Owner.permissions().get(resource),
            Some(W),
            "{resource}"
        );
    }
}

#[test]
fn anonymous_permissions_stay_within_the_operator_role() {
    assert_eq!(validate_anonymous_permissions(&Permissions::NONE), Ok(()));
    assert_eq!(
        validate_anonymous_permissions(&Role::Operator.permissions()),
        Ok(())
    );
    assert_eq!(
        validate_anonymous_permissions(&map(&[(Resource::Logs, R)])),
        Ok(())
    );
    for held in [
        Permission::new(Resource::System, W),
        Permission::new(Resource::Logs, W),
        Permission::new(Resource::Remote, R),
        Permission::new(Resource::Remote, W),
    ] {
        assert_eq!(
            validate_anonymous_permissions(&map(&[(held.resource, held.access)])),
            Err(PermissionError::AnonymousBeyondOperator { held }),
            "{held}"
        );
    }
}

#[test]
fn anonymous_permissions_are_capped_at_the_operator_role() {
    use Resource::*;
    assert_eq!(
        cap_anonymous_permissions(&Role::Owner.permissions()),
        Role::Operator.permissions()
    );
    assert_eq!(
        cap_anonymous_permissions(&map(&[(System, W), (Logs, R), (Remote, R)])),
        map(&[(System, R), (Logs, R)])
    );
    assert_eq!(
        cap_anonymous_permissions(&Permissions::NONE),
        Permissions::NONE
    );
}
