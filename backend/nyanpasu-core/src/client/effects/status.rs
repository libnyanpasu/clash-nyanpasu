//! Result protocol for applied effects: per-effect health plus the revision
//! bookkeeping that lets an owner drop a superseded reconcile.

use nyanpasu_runtime_values::configuration::{EffectFailureCode, EffectKind};

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

/// The diagnostic text of a failure: its message, then each cause down the
/// chain. Unlike `Display` it keeps what the library underneath reported.
pub fn failure_text(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
    }
    text
}

// Every variant but `Healthy` is constructed by the effect owners, which are
// added one task at a time.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectHealth {
    Pending,
    Healthy,
    Degraded {
        code: EffectFailureCode,
        message: String,
        retryable: bool,
    },
    /// A newer reconcile already applied a higher revision.
    Superseded,
    /// The platform or a dependency cannot provide the effect at all.
    Unsupported {
        code: EffectFailureCode,
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

/// Reports that the requested effect revision was applied successfully.
pub fn healthy(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: revision,
        health: EffectHealth::Healthy,
    }
}

/// Reports an effect failure without advancing the applied revision.
pub fn degraded(
    kind: EffectKind,
    revision: EffectRevision,
    code: EffectFailureCode,
    message: String,
    retryable: bool,
) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: EffectHealth::Degraded {
            code,
            message,
            retryable,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_text_joins_the_whole_chain() {
        #[derive(Debug, snafu::Snafu)]
        #[snafu(display("outer"))]
        struct Outer {
            source: std::io::Error,
        }
        let error = Outer {
            source: std::io::Error::other("inner"),
        };

        assert_eq!(failure_text(&error), "outer: inner");
    }

    #[test]
    fn revision_is_monotonically_comparable() {
        assert!(EffectRevision::new(1) < EffectRevision::new(2));
        assert_eq!(EffectRevision::default(), EffectRevision::new(0));
        assert_eq!(EffectRevision::new(7).get(), 7);
    }
}
