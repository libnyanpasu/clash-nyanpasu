//! Result protocol for applied effects: per-effect health plus the revision
//! bookkeeping that lets an owner drop a superseded reconcile.

use super::plan::EffectKind;

/// Monotonic counter over committed desired configurations, allocated by the
/// facade after a commit.
///
/// Distinct from a typed actor's per-domain `version`: one patch can bump two
/// unrelated domain versions, which cannot be totally ordered. Not persisted —
/// a restart begins at zero and is reconciled by a full plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct EffectRevision(u64);

impl EffectRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    // Read by the effect owners that compare it against their applied revision.
    #[allow(dead_code)]
    pub const fn get(self) -> u64 {
        self.0
    }
}

// Every variant but `Healthy` is constructed by the effect owners, which are
// added one task at a time.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectHealth {
    Pending,
    Healthy,
    Degraded {
        /// Stable snake_case code, not a free-form phrase.
        code: &'static str,
        message: String,
        retryable: bool,
    },
    /// A newer reconcile already applied a higher revision.
    Superseded,
    /// The platform or a dependency cannot provide the effect at all.
    Unsupported {
        code: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectStatus {
    pub kind: EffectKind,
    pub desired_revision: EffectRevision,
    /// Last successfully installed desired revision. Failed attempts never advance it.
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_is_monotonically_comparable() {
        assert!(EffectRevision::new(1) < EffectRevision::new(2));
        assert_eq!(EffectRevision::default(), EffectRevision::new(0));
        assert_eq!(EffectRevision::new(7).get(), 7);
    }
}
