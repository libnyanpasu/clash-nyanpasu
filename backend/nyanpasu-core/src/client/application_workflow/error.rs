use crate::runtime::config::RuntimeBuildError;
use serde::Serialize;
use snafu::Snafu;

#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub(crate)))]
pub enum RuntimePreparationError {
    #[snafu(display("could not build the runtime artifact: {source}"))]
    BuildArtifact { source: RuntimeBuildError },

    #[snafu(display("could not start the script runner"))]
    StartScriptRunner {
        #[serde(skip)]
        source: std::io::Error,
    },

    #[snafu(display("could not serialize the final config"))]
    SerializeFinalConfig {
        #[serde(skip)]
        source: serde_yaml_ng::Error,
    },

    #[snafu(display("the final config is not a mapping"))]
    ConfigNotMapping,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::runtime_error::RuntimeError, runtime::config::RuntimeBuildLog};
    use nyanpasu_config::{
        profile::ProfileId,
        runtime::{
            executor::{RuntimePipelineError, StepLogEntry, TransformFailure},
            snapshot::OperatorTag,
        },
    };

    #[test]
    fn preparation_failures_preserve_builder_context_on_the_wire() {
        let pipeline = RuntimeError::BuildRuntime {
            source: RuntimePreparationError::BuildArtifact {
                source: RuntimeBuildError::RunPipeline {
                    source: RuntimePipelineError::SelectedProfileNotFound {
                        profile: ProfileId("ghost".into()),
                    },
                },
            },
        };
        assert_eq!(
            serde_json::to_value(pipeline).unwrap(),
            serde_json::json!({
                "kind": "build_runtime",
                "source": {
                    "kind": "build_artifact",
                    "source": {
                        "kind": "run_pipeline",
                        "source": { "kind": "selected_profile_not_found", "profile": "ghost" },
                    },
                },
            })
        );

        let transforms = RuntimeError::BuildRuntime {
            source: RuntimePreparationError::BuildArtifact {
                source: RuntimeBuildError::TransformsFailed {
                    failures: vec![TransformFailure::Profile { id: "t1".into() }],
                    logs: vec![RuntimeBuildLog {
                        tag: OperatorTag::BareRoot,
                        entries: vec![StepLogEntry::error("transform rejected")],
                    }],
                },
            },
        };
        assert_eq!(
            serde_json::to_value(transforms).unwrap(),
            serde_json::json!({
                "kind": "build_runtime",
                "source": {
                    "kind": "build_artifact",
                    "source": {
                        "kind": "transforms_failed",
                        "failures": [{ "kind": "profile", "id": "t1" }],
                        "logs": [{
                            "tag": { "kind": "bare_root" },
                            "entries": [{ "level": "error", "message": "transform rejected" }],
                        }],
                    },
                },
            })
        );
    }
}
