//! The redaction layer: how much of an admitted call's results and errors the caller sees.
//!
//! It sits inside admission in the request pipeline (design section 4.6 of the permission
//! namespace spec) and decides a [`ResponseVisibility`] from the admitted resource and the
//! caller's permissions. The typed projection itself still runs in the `rpc` wrapper, which
//! encodes the result for that visibility before the JSON is erased.
// TODO(E-5): give this layer its own rules. Until then it applies the transitional rule the spec
// confirmed (C-2): a caller sees the unredacted response only when it holds `write` on the
// resource it called.

use nyanpasu_contracts::auth::{Access, Permissions};
use nyanpasu_rpc_protocol::{AuthContext, ResponseVisibility, permission::ResourceMetadata};

/// The visibility of a response from `resource` to a caller holding `permissions`.
pub fn visibility(resource: &ResourceMetadata, permissions: &Permissions) -> ResponseVisibility {
    if permissions.get(resource.permission.resource) == Some(Access::Write) {
        ResponseVisibility::Full
    } else {
        ResponseVisibility::Public
    }
}

/// The visibility of a response from `resource` to `caller`; an unresolved caller is redacted.
pub fn for_caller(resource: &ResourceMetadata, caller: Option<&AuthContext>) -> ResponseVisibility {
    caller.map_or(ResponseVisibility::Public, |caller| {
        visibility(resource, &caller.permissions)
    })
}

/// The visibility of a response that no admitted resource produced: a malformed request, an
/// unknown method or event, or a subscription to every event. No `write` can lift it.
pub const UNADMITTED: ResponseVisibility = ResponseVisibility::Public;

#[cfg(test)]
mod tests {
    use nyanpasu_contracts::auth::{Resource, Role};
    use nyanpasu_rpc_protocol::permission::RESOURCES;

    use super::*;

    fn resource(name: &str) -> &'static ResourceMetadata {
        RESOURCES
            .iter()
            .find(|resource| resource.name == name)
            .unwrap()
    }

    #[test]
    fn full_visibility_needs_write_on_the_called_resource() {
        // `get_profiles` requires `profile:read`.
        let called = resource("get_profiles");
        assert_eq!(called.permission.resource, Resource::Profile);
        for (permissions, expected) in [
            (Permissions::NONE, ResponseVisibility::Public),
            (
                Permissions::NONE.with(Resource::Profile, Access::Read),
                ResponseVisibility::Public,
            ),
            (
                Permissions::NONE.with(Resource::Proxy, Access::Write),
                ResponseVisibility::Public,
            ),
            (
                Permissions::NONE.with(Resource::Profile, Access::Write),
                ResponseVisibility::Full,
            ),
            (Role::Viewer.permissions(), ResponseVisibility::Public),
            (Role::Operator.permissions(), ResponseVisibility::Full),
            (Role::Owner.permissions(), ResponseVisibility::Full),
        ] {
            assert_eq!(
                visibility(called, &permissions),
                expected,
                "{permissions:?}"
            );
        }
    }

    /// `system` and `logs` are written only by `admin` and above, so an operator reads their
    /// sensitive queries with public visibility.
    #[test]
    fn the_operator_gets_public_visibility_for_system_and_logs() {
        for name in ["get_core_status", "query_logs"] {
            assert_eq!(
                visibility(resource(name), &Role::Operator.permissions()),
                ResponseVisibility::Public,
                "{name}"
            );
            assert_eq!(
                visibility(resource(name), &Role::Admin.permissions()),
                ResponseVisibility::Full,
                "{name}"
            );
        }
    }

    #[test]
    fn anonymous_callers_get_public_visibility_by_default() {
        assert!(
            RESOURCES
                .iter()
                .all(|resource| visibility(resource, &Permissions::NONE)
                    == ResponseVisibility::Public)
        );
    }
}
