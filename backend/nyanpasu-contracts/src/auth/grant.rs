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

    /// Checks a grant before it is issued. Only `viewer`, `operator` and `admin` can be
    /// assigned: `anonymous` is the absence of a credential and `owner` is structural. The power
    /// to grant stays with the owner, so a custom map never holds `remote`.
    pub fn validate(&self) -> Result<(), GrantError> {
        match self {
            Self::Role(Role::Viewer | Role::Operator | Role::Admin) => Ok(()),
            Self::Role(role) => Err(GrantError::UnassignableRole { role: *role }),
            Self::Custom(permissions) if permissions.get(Resource::Remote).is_some() => {
                Err(GrantError::RemoteInCustomPermissions)
            }
            Self::Custom(_) => Ok(()),
        }
    }
}

/// A grant that cannot be issued.
#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
pub enum GrantError {
    #[snafu(display("the {role} role cannot be granted"))]
    UnassignableRole { role: Role },
    #[snafu(display("custom permissions cannot include remote"))]
    RemoteInCustomPermissions,
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
        for access in [Access::Read, Access::Write] {
            assert_eq!(
                Grant::Custom(Permissions::NONE.with(Resource::Remote, access)).validate(),
                Err(GrantError::RemoteInCustomPermissions)
            );
        }
    }
}
