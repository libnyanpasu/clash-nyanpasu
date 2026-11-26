//! Pure runtime assembly and builtin transforms over the config executor.

mod runtime_builder;

pub use runtime_builder::{
    BuildCandidate, BuildFailure, ConfigNotMappingSnafu, RuntimeBuildError, RuntimeBuildInput,
    RuntimeBuildLog, RuntimeBuilder, SerializeFinalConfigSnafu, StartScriptRunnerSnafu,
    builtin_transforms_for,
};

mod chain;
mod utils;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use chain::PostProcessingOutput;
pub use utils::{LogSpan, Logs};
