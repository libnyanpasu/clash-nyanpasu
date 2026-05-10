use std::ops::Deref;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Version(u64);

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl Deref for Version {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<u64> for Version {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl Version {
    pub fn new(version: u64) -> Self {
        Self(version)
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

/// Unique identifier for a state change, used for tracking and acknowledgment purposes.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StateChangeId(pub Version);

impl Deref for StateChangeId {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<u64> for StateChangeId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl StateChangeId {
    pub fn new(id: u64) -> Self {
        Self(Version::new(id))
    }

    pub fn next(&self) -> Self {
        Self(self.0.next())
    }
}
