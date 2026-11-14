//! Verifies whether an observed running instance matches a confirmed application.
use nyanpasu_ipc::api::status::CoreStateDetail;

use crate::{
    actor_v2::{CoreStatusProjection, facade::AppliedConfigBinding},
    state::mutation::runtime_apply::RuntimeApplyReceipt,
};
use nyanpasu_runtime_values::execution_host::ExecutionHost;

/// What the app can observe about the runtime right now. Built from a status
/// read, or from the report of the operation that claimed to restore it.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::client) struct ObservedRuntime {
    pub host: ExecutionHost,
    pub instance_id: Option<String>,
    pub binding: Option<AppliedConfigBinding>,
    /// `None` means the host published no state this app can trust. It is not
    /// `Stopped` and it is not `Running`; it is an absent fact.
    pub state: Option<CoreStateDetail>,
    /// The source hash of the revision the host reports as applied. This is
    /// the document identity that survives a restart; the effective hash of
    /// the same document does not, because it covers the epoch-specific
    /// controller endpoint the runtime writes into it.
    pub source_hash: Option<String>,
}

impl ObservedRuntime {
    pub fn from_status(status: &CoreStatusProjection) -> Self {
        let snapshot = status.snapshot.as_ref();
        Self {
            host: status.host,
            instance_id: snapshot.and_then(|s| s.instance_id.clone()),
            binding: snapshot
                .and_then(|snapshot| snapshot.revision.clone())
                .map(|revision| AppliedConfigBinding {
                    revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                        epoch: revision.epoch,
                        generation: revision.generation,
                        effective_hash: revision.effective_hash,
                        source_hash: snapshot
                            .and_then(|s| s.source_hash.clone())
                            .unwrap_or_default(),
                    },
                    host: status.host,
                    generation: status.generation,
                }),
            state: snapshot.and_then(|snapshot| snapshot.state.clone()),
            source_hash: snapshot.and_then(|snapshot| snapshot.source_hash.clone()),
        }
    }
}

/// Whether the running instance still runs the confirmed application.
/// A different connection must prove that the very same process survived.
pub(in crate::client) fn matches_confirmed_apply(
    receipt: &RuntimeApplyReceipt,
    observed: &ObservedRuntime,
) -> bool {
    observed.host == receipt.host
        && observed.binding.as_ref().is_some_and(|binding| {
            binding.revision == receipt.binding.revision
                && (binding.generation == receipt.binding.generation
                    || receipt
                        .instance_id
                        .as_ref()
                        .is_some_and(|id| observed.instance_id.as_ref() == Some(id)))
        })
        && observed.source_hash.as_deref() == Some(receipt.binding.revision.source_hash.as_str())
        && matches!(observed.state, Some(CoreStateDetail::Running { epoch, .. })
            if epoch == receipt.binding.revision.epoch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::application::ClashCore;
    pub(super) fn receipt() -> RuntimeApplyReceipt {
        RuntimeApplyReceipt {
            instance_id: None,
            revision: crate::client::runtime::tests::test_revision(),
            target_core: ClashCore::Mihomo,
            host: ExecutionHost::Local,
            binding: crate::actor_v2::facade::AppliedConfigBinding {
                revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                    epoch: 4,
                    generation: 11,
                    source_hash: "source-a".into(),
                    effective_hash: "effective-a".into(),
                },
                host: ExecutionHost::Local,
                generation: 3,
            },
            ports: crate::client::ports::SessionPortResolver::default()
                .resolve_candidate(&crate::client::test_support::test_clash_config())
                .expect("default port strategies resolve"),
            target: None,
        }
    }
}
#[cfg(test)]
mod confirmed_apply_tests {
    use super::*;

    #[test]
    fn process_identity_is_required_only_across_connections() {
        for (generation, recorded, observed_id, expected) in [
            (3, Some("a"), Some("b"), true),
            (4, Some("a"), Some("a"), true),
            (4, Some("a"), Some("b"), false),
            (4, None, Some("a"), false),
        ] {
            let mut receipt = super::tests::receipt();
            receipt.instance_id = recorded.map(str::to_owned);
            let mut binding = receipt.binding.clone();
            binding.generation = generation;
            let observed = ObservedRuntime {
                host: receipt.host,
                instance_id: observed_id.map(str::to_owned),
                binding: Some(binding),
                state: Some(CoreStateDetail::Running {
                    epoch: receipt.binding.revision.epoch,
                    pid: 42,
                }),
                source_hash: Some(receipt.binding.revision.source_hash.clone()),
            };
            assert_eq!(
                matches_confirmed_apply(&receipt, &observed),
                expected,
                "{generation}/{recorded:?}/{observed_id:?}"
            );
        }
    }

    // Guards revision and epoch matching across reconnection to the same process.
    #[test]
    fn confirmed_apply_matches_only_the_same_running_revision() {
        let mut receipt = super::tests::receipt();
        receipt.instance_id = Some("same-process".into());
        let mut observed = ObservedRuntime {
            host: receipt.host,
            instance_id: Some("same-process".into()),
            binding: Some(receipt.binding.clone()),
            state: Some(CoreStateDetail::Running {
                epoch: receipt.binding.revision.epoch,
                pid: 42,
            }),
            source_hash: Some(receipt.binding.revision.source_hash.clone()),
        };
        observed.binding.as_mut().unwrap().generation += 1;
        assert!(matches_confirmed_apply(&receipt, &observed));
        for field in [
            "host",
            "epoch",
            "generation",
            "effective_hash",
            "source_hash",
            "running_epoch",
            "stopped",
            "no_binding",
            "no_source",
        ] {
            let mut mismatch = observed.clone();
            match field {
                "host" => mismatch.host = ExecutionHost::Service,
                "epoch" => mismatch.binding.as_mut().unwrap().revision.epoch += 1,
                "generation" => mismatch.binding.as_mut().unwrap().revision.generation += 1,
                "effective_hash" => mismatch
                    .binding
                    .as_mut()
                    .unwrap()
                    .revision
                    .effective_hash
                    .push('x'),
                "source_hash" => mismatch.source_hash = Some("different".into()),
                "running_epoch" => {
                    mismatch.state = Some(CoreStateDetail::Running { epoch: 99, pid: 42 })
                }
                "stopped" => mismatch.state = Some(CoreStateDetail::Stopped { reason: None }),
                "no_binding" => mismatch.binding = None,
                "no_source" => mismatch.source_hash = None,
                _ => unreachable!(),
            }
            assert!(!matches_confirmed_apply(&receipt, &mismatch), "{field}");
        }
    }
}
