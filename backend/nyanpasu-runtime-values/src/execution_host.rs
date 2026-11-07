//! The execution host identity shared by public results.

/// Which controller owns the runtime. The app perceives the difference in
/// exactly two places: this tag on the endpoint slot, and the handoff
/// protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHost {
    Local,
    Service,
}
