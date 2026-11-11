//! Public evidence gaps and advisory check failures.

/// Evidence a mutation needed and did not have, before anything was tried.
///
/// Not a Try outcome. Nothing was submitted, so there is no result to classify
/// and this attempt made nothing less certain than it already was — which is
/// why it is a plain refusal the caller may retry (v2 §2.4, first row) and
/// never the isolated state §11.4 reserves for "结果可能已经执行".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGap {
    /// The core is mid-transition — starting, restarting, switching or
    /// stopping. That is an answer, but not one that says what a candidate
    /// would be applied on top of.
    CoreTransitioning,
    /// The host published no runtime state, or the read failed, and no stop
    /// this workflow recorded settles the question either.
    BaselineUnconfirmed,
    /// A core is running that this session never applied to, so no receipt
    /// describes what it is running. The Try could be submitted, but its Cancel
    /// would have nothing to put back — and a mutation that cannot be undone
    /// must not be attempted (R10). The gap is known before anything is
    /// submitted, which is what keeps it a refusal rather than the isolated
    /// state a post-submission unknown earns.
    NoRestorableBaseline,
}

/// Why no check ran. Every variant is a reason, never a verdict: an absent
/// check must not be reported as a passing one (v2 §2.4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "cause", rename_all = "snake_case")]
pub enum RuntimeCheckUnavailable {
    /// No host owns the runtime, so there is nothing to check against.
    NoEndpoint { reason: String },
    /// The host owning the runtime exposes no check for this request.
    HostUnsupported {
        host: super::execution_host::ExecutionHost,
        reason: String,
    },
    /// The host reads the candidate from disk and it could not be staged.
    CandidateUnavailable { reason: String },
    /// The host has the capability but could not serve it.
    Backend {
        kind: Option<nyanpasu_core_metadata::CoreErrorKind>,
        message: String,
        retryable: bool,
    },
}
