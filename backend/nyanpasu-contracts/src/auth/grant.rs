//! What a token record or browser session is granted: a role, or a custom permission map.
//!
//! A grant is expanded into [`Permissions`] by the authentication component; everything after
//! that reads only the permissions.

use nyanpasu_config::application::{Permissions, Resource, Role};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A persisted grant: `{ "role": "viewer" }` or `{ "permissions": { "proxy": "write" } }`.
///
/// A role follows later changes to its definition, so commands added by an upgrade become
/// available with it; a custom map stays exactly what was granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    Role(Role),
    #[serde(rename = "permissions")]
    Custom(Permissions),
}

impl Grant {
    /// The permissions this grant expands to.
    pub const fn permissions(&self) -> Permissions {
        match self {
            Self::Role(role) => role.permissions(),
            Self::Custom(permissions) => *permissions,
        }
    }

    /// The role this grant names; `None` for a custom map.
    pub const fn role(&self) -> Option<Role> {
        match self {
            Self::Role(role) => Some(*role),
            Self::Custom(_) => None,
        }
    }

    /// The roles a grant can name, from the least to the most: `anonymous` is the absence of a
    /// credential and `owner` is structural.
    pub const ASSIGNABLE_ROLES: [Role; 3] = [Role::Viewer, Role::Operator, Role::Admin];

    /// Whether a custom map can hold `resource`. Every caller already holds `meta:read`, and the
    /// power to grant stays with the owner, so never `meta` or `remote`.
    pub const fn is_grantable(resource: Resource) -> bool {
        !matches!(resource, Resource::Meta | Resource::Remote)
    }

    /// The resources a custom map can hold, in resource order.
    pub fn grantable_resources() -> impl Iterator<Item = Resource> {
        <Resource as strum::IntoEnumIterator>::iter()
            .filter(|resource| Self::is_grantable(*resource))
    }

    /// Checks a grant before it is issued: a role from [`Self::ASSIGNABLE_ROLES`], or a custom
    /// map of [`Self::grantable_resources`].
    pub fn validate(&self) -> Result<(), GrantError> {
        match self {
            Self::Role(role) if Self::ASSIGNABLE_ROLES.contains(role) => Ok(()),
            Self::Role(role) => Err(GrantError::UnassignableRole { role: *role }),
            Self::Custom(permissions) => match permissions
                .iter()
                .find(|(resource, _)| !Self::is_grantable(*resource))
            {
                Some((resource, _)) => Err(GrantError::UngrantableResource { resource }),
                None => Ok(()),
            },
        }
    }
}

/// A grant that cannot be issued.
#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
pub enum GrantError {
    #[snafu(display("the {role} role cannot be granted"))]
    UnassignableRole { role: Role },
    #[snafu(display("custom permissions cannot include {resource}"))]
    UngrantableResource { resource: Resource },
}

#[cfg(test)]
mod tests {
    use nyanpasu_config::application::Access;

    use super::*;

    #[test]
    fn grants_persist_as_a_role_or_a_permission_map() {
        let role = Grant::Role(Role::Viewer);
        assert_eq!(
            serde_json::to_value(role).unwrap(),
            serde_json::json!({ "role": "viewer" })
        );
        let custom = Grant::Custom(
            Permissions::NONE
                .with(Resource::Proxy, Access::Write)
                .with(Resource::Logs, Access::Read),
        );
        let json = serde_json::json!({ "permissions": { "proxy": "write", "logs": "read" } });
        assert_eq!(serde_json::to_value(custom).unwrap(), json);
        assert_eq!(serde_json::from_value::<Grant>(json).unwrap(), custom);
        assert_eq!(
            serde_json::from_value::<Grant>(serde_json::json!({ "role": "admin" })).unwrap(),
            Grant::Role(Role::Admin)
        );
    }

    #[test]
    fn grants_expand_to_their_permissions() {
        assert_eq!(
            Grant::Role(Role::Operator).permissions(),
            Role::Operator.permissions()
        );
        let custom = Permissions::NONE.with(Resource::Traffic, Access::Read);
        assert_eq!(Grant::Custom(custom).permissions(), custom);
    }

    #[test]
    fn only_delegable_grants_are_issued() {
        for role in [Role::Viewer, Role::Operator, Role::Admin] {
            assert_eq!(Grant::Role(role).validate(), Ok(()));
        }
        for role in [Role::Anonymous, Role::Owner] {
            assert_eq!(
                Grant::Role(role).validate(),
                Err(GrantError::UnassignableRole { role })
            );
        }
        assert_eq!(Grant::Custom(Role::Admin.permissions()).validate(), Ok(()));
        for resource in [Resource::Meta, Resource::Remote] {
            for access in [Access::Read, Access::Write] {
                assert_eq!(
                    Grant::Custom(Permissions::NONE.with(resource, access)).validate(),
                    Err(GrantError::UngrantableResource { resource })
                );
            }
        }
    }

    #[test]
    fn grantable_resources_are_every_resource_but_meta_and_remote() {
        let names: Vec<_> = Grant::grantable_resources()
            .map(|resource| resource.to_string())
            .collect();
        assert_eq!(
            names,
            ["logs", "profile", "proxy", "settings", "system", "traffic"]
        );
    }
}
