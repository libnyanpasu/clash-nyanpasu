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

    /// Return the next monotonic version.
    ///
    /// Panics if the counter overflows, because wrapping would break CAS
    /// monotonicity and may make different state changes compare as the same
    /// version.
    pub fn next(&self) -> Self {
        Self(self.0.checked_add(1).expect("version overflow"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "version overflow")]
    fn version_next_panics_on_overflow() {
        Version::new(u64::MAX).next();
    }

    #[test]
    #[should_panic(expected = "version overflow")]
    fn state_change_id_next_panics_on_overflow() {
        StateChangeId::new(u64::MAX).next();
    }
}
