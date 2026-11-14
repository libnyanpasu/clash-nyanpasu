use async_trait::async_trait;

use crate::actor_v2::{CoreClient, endpoint::EndpointHandle};

/// Checks inline documents through the session's local control plane.
///
/// The local endpoint stays alive while the service owns the running core.
/// Its read-only check runs outside the mutating queue and is awaited to completion.
pub(in crate::client) struct CoreCheckValidator {
    endpoint: EndpointHandle,
}

/// The phrase the runtime puts in the message when its own check bound
/// elapses. The kind is `ConfigCheckFailed` either way, so the message is the
/// only thing that tells a timeout apart from a verdict — and the runtime
/// writes it deliberately, "so a slow core is diagnosable".
const RUNTIME_CHECK_TIMEOUT: &str = "config check timed out after ";

impl CoreCheckValidator {
    pub(in crate::client) fn new(core: &CoreClient) -> Self {
        Self {
            endpoint: core.initial_endpoint(),
        }
    }
}

#[async_trait]
impl super::ports::RuntimeValidatorPort for CoreCheckValidator {
    async fn check(
        &self,
        request: super::ports::RuntimeCheckRequest<'_>,
    ) -> super::ports::RuntimeCheckOutcome {
        use super::ports::RuntimeCheckOutcome;
        use crate::actor_v2::endpoint::{CheckSubmission, CheckSupport};
        use nyanpasu_runtime_values::check::RuntimeCheckUnavailable;

        let support = self
            .endpoint
            .check_config(CheckSubmission {
                core_spec: request.core_spec,
                core_type: request.intent.core_type.clone(),
                config_bytes: request.intent.config_text.as_bytes().to_vec(),
                digest: request.intent.digest.clone(),
                staged_config: None,
            })
            .await;
        match support {
            CheckSupport::Ran(Ok(())) => RuntimeCheckOutcome::Passed,
            CheckSupport::Ran(Err(error)) => {
                // Only the core's own verdict on the document is a rejection,
                // and its own check timeout is not one: it arrives under the
                // same kind but says the binary never answered, which is
                // retryable and must keep the deferral the failure matrix
                // allows for an unreachable check.
                if error.message.contains(RUNTIME_CHECK_TIMEOUT) {
                    RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                        kind: error.kind,
                        message: error.message,
                        retryable: true,
                    })
                } else if matches!(
                    error.kind,
                    Some(
                        nyanpasu_core_manager::CoreErrorKind::ConfigCheckFailed
                            | nyanpasu_core_manager::CoreErrorKind::InvalidConfig
                    )
                ) {
                    RuntimeCheckOutcome::Rejected {
                        kind: error.kind,
                        message: error.message,
                    }
                } else {
                    RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                        kind: error.kind,
                        message: error.message,
                        retryable: error.retryable,
                    })
                }
            }
            CheckSupport::Unsupported { reason } => {
                RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                    kind: None,
                    message: reason,
                    retryable: false,
                })
            }
        }
    }
}
