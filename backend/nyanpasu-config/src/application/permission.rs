//! Permissions: what a caller may do to each domain resource.
//!
//! A permission is a domain [`Resource`] and an [`Access`] level; a caller holds at most one level
//! per resource, and `write` includes `read`. Requirements, roles and grants are all expressed in
//! these terms; the permission layer reads only [`Permissions`].

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use specta::Type;
use strum::EnumCount;

// One variant per top-level `namespace!`; written by `export_rpc_resource_table`.
mod generated;

pub use generated::Resource;

/// An access level. The derived order is the implication: `Write >= Read`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Type,
    strum::Display,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

/// One requirement, written `proxy:write`; on the wire `{ "resource": "proxy", "access": "write" }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct Permission {
    pub resource: Resource,
    pub access: Access,
}

impl Permission {
    /// Capability discovery, which every resolved caller holds.
    pub const META_READ: Self = Self::new(Resource::Meta, Access::Read);

    pub const fn new(resource: Resource, access: Access) -> Self {
        Self { resource, access }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.resource, self.access)
    }
}

/// Every permission a caller holds. On the wire and on disk it is a map from resource to level,
/// `{ "proxy": "write", "logs": "read" }`; a resource that is not listed is not held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<Resource, Access>",
    into = "BTreeMap<Resource, Access>"
)]
pub struct Permissions([Option<Access>; Resource::COUNT]);

impl Permissions {
    /// No permission beyond `meta:read`.
    pub const NONE: Self = Self([None; Resource::COUNT]);

    /// These permissions with `access` to `resource`, replacing the level held before.
    #[must_use]
    pub const fn with(mut self, resource: Resource, access: Access) -> Self {
        self.0[resource as usize] = Some(access);
        self
    }

    /// The level held on `resource`, if any.
    pub const fn get(&self, resource: Resource) -> Option<Access> {
        self.0[resource as usize]
    }

    /// Whether these permissions satisfy `required`: `write` includes `read`, and `meta:read` holds
    /// for every caller that reached admission.
    pub fn permits(&self, required: Permission) -> bool {
        required == Permission::META_READ
            || self
                .get(required.resource)
                .is_some_and(|held| held >= required.access)
    }

    /// The held levels, in resource order.
    pub fn iter(&self) -> impl Iterator<Item = (Resource, Access)> + '_ {
        <Resource as strum::IntoEnumIterator>::iter()
            .filter_map(|resource| self.get(resource).map(|access| (resource, access)))
    }
}

impl Type for Permissions {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <BTreeMap<Resource, Access> as Type>::definition(types)
    }
}

impl From<BTreeMap<Resource, Access>> for Permissions {
    fn from(map: BTreeMap<Resource, Access>) -> Self {
        map.into_iter()
            .fold(Self::NONE, |permissions, (resource, access)| {
                permissions.with(resource, access)
            })
    }
}

impl From<Permissions> for BTreeMap<Resource, Access> {
    fn from(permissions: Permissions) -> Self {
        permissions.iter().collect()
    }
}

/// A named set of permissions. The permission layer never sees a role: a role only appears
/// where a grant is assigned or expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Role {
    /// An HTTP request without a credential, under the `local_open` listening strategy.
    Anonymous,
    Viewer,
    Operator,
    Admin,
    /// The desktop window and the same-user local socket; never assignable.
    Owner,
}

impl Role {
    /// The permissions this role expands to. Each role adds to the one before it.
    pub const fn permissions(self) -> Permissions {
        use Access::{Read, Write};
        use Resource::{Logs, Profile, Proxy, Remote, Settings, System, Traffic};
        match self {
            Self::Anonymous => Permissions::NONE,
            Self::Viewer => Self::Anonymous
                .permissions()
                .with(Proxy, Read)
                .with(Profile, Read)
                .with(Settings, Read)
                .with(System, Read)
                .with(Logs, Read)
                .with(Traffic, Read),
            Self::Operator => Self::Viewer
                .permissions()
                .with(Proxy, Write)
                .with(Profile, Write)
                .with(Traffic, Write)
                .with(Settings, Write),
            Self::Admin => Self::Operator
                .permissions()
                .with(System, Write)
                .with(Logs, Write),
            Self::Owner => Self::Admin.permissions().with(Remote, Write),
        }
    }
}

/// A permission set that may not be configured.
#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
pub enum PermissionError {
    /// Anonymous callers are unidentified local processes, so they never exceed `operator`.
    #[snafu(display("anonymous permissions cannot exceed the operator role, but hold {held}"))]
    AnonymousBeyondOperator { held: Permission },
}

/// Checks the permissions configured for anonymous callers: at most the `operator` role, which
/// also keeps `remote` out of reach.
pub fn validate_anonymous_permissions(permissions: &Permissions) -> Result<(), PermissionError> {
    let bound = Role::Operator.permissions();
    match permissions
        .iter()
        .map(|(resource, access)| Permission::new(resource, access))
        .find(|held| !bound.permits(*held))
    {
        Some(held) => Err(PermissionError::AnonymousBeyondOperator { held }),
        None => Ok(()),
    }
}

/// The permissions an anonymous caller actually holds: `configured`, each level capped at what the
/// `operator` role holds, so a hand-edited configuration cannot widen them past
/// [`validate_anonymous_permissions`].
pub fn cap_anonymous_permissions(configured: &Permissions) -> Permissions {
    let bound = Role::Operator.permissions();
    configured
        .iter()
        .fold(Permissions::NONE, |held, (resource, access)| {
            match bound.get(resource) {
                Some(limit) => held.with(resource, access.min(limit)),
                None => held,
            }
        })
}

#[cfg(test)]
mod tests;
