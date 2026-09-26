//! Verifying that a recovery actually landed on its target (v2 C4/D10).
//!
//! The lower control plane answers a restore request with its own transaction
//! result, and `RolledBack` there means "this request failed and I put myself
//! back where I was". After a B → A restore that usually leaves the core on B.
//! So the enum name is never the proof: the proof is that what is running now
//! matches the receipt of the apply we are restoring — its document identity,
//! its core, its execution host and its run/stop intent.
//!
//! A recovery legitimately produces a *new* instance generation. Requiring the
//! old pid or the old generation back would reject every successful restart,
//! so the old generation and effective hash are not compared with the restored
//! instance. The fresh observation must instead match the restore receipt
//! in full, including its new generation and effective hash: the runtime stamps an epoch-specific
//! controller endpoint (`core-{epoch}.sock`) into every effective document, so
//! restoring one unchanged configuration into a new epoch produces a different
//! effective hash by construction. What identifies the document across a
//! restart is its source hash, and that is what this compares.
//!
//! The source hash does not cover everything a revision carries, though. The
//! local-IPC settings travel with the submission rather than inside the YAML,
//! so two revisions can share a source hash, a core, a host and a run intent
//! while listening on different control channels — and no host publishes them
//! back. The observation therefore cannot settle that part of the target on
//! its own, and the only evidence that can is the host's own confirmation of
//! the submission that carried them. That is why this takes the restore's
//! confirmed binding as a third input: `RolledBack`, an unobserved outcome and
//! an error are all "nothing confirmed those settings".

use nyanpasu_core_manager::CoreKind;
use nyanpasu_ipc::api::status::CoreStateDetail;

use super::{application_workflow::policy::CoreRunIntent, runtime::RuntimeApplyReceipt};
use crate::core::actor_v2::{
    CoreStatusProjection,
    endpoint::{ExecutionHost, wire_core_type_to_kind},
    facade::AppliedConfigBinding,
};

/// What the app can observe about the runtime right now. Built from a status
/// read, or from the report of the operation that claimed to restore it.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::client) struct ObservedRuntime {
    pub host: ExecutionHost,
    pub binding: Option<AppliedConfigBinding>,
    /// `None` means the host published no state this app can trust. It is not
    /// `Stopped` and it is not `Running`; it is an absent fact.
    pub state: Option<CoreStateDetail>,
    /// The source hash of the revision the host reports as applied. This is
    /// the document identity that survives a restart; the effective hash of
    /// the same document does not, because it covers the epoch-specific
    /// controller endpoint the runtime writes into it.
    pub source_hash: Option<String>,
    /// The kind of core the host has actually applied.
    pub applied_kind: Option<CoreKind>,
}

impl ObservedRuntime {
    pub fn from_status(status: &CoreStatusProjection) -> Self {
        let snapshot = status.snapshot.as_ref();
        Self {
            host: status.host,
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
            applied_kind: snapshot.and_then(|snapshot| snapshot.applied_kind),
        }
    }
}

/// One way the observed runtime fails to be the recovery target.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::client) enum RecoveryMismatch {
    /// The runtime is owned by a different execution host than the receipt.
    Host {
        expected: ExecutionHost,
        observed: ExecutionHost,
    },
    /// The core is not in the run/stop state the receipt describes, or the
    /// host published nothing that could settle the question.
    RunIntent {
        expected: CoreRunIntent,
        observed: Option<CoreStateDetail>,
    },
    /// A different source document is applied, or none the app can identify.
    Content {
        expected: String,
        observed: Option<String>,
    },
    /// A different core is applied, or the host does not say which.
    Core {
        expected: Option<CoreKind>,
        observed: Option<CoreKind>,
    },
    /// Nothing confirmed that the receipt's own submission took effect, so the
    /// part of the target no host publishes — its local-IPC settings — is
    /// unproven. A revision that differs from the receipt only in its control
    /// channel passes every observable comparison, so this is the one check
    /// that keeps such a revision from being read as the target.
    Submission {
        /// What the submission confirmed instead, when it confirmed anything.
        confirmed: Option<AppliedConfigBinding>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::client) enum RecoveryVerification {
    /// Everything the receipt describes is what is running.
    Verified,
    /// Some part of the target is not confirmed. This includes "we cannot
    /// tell": an unverified recovery is a failed one.
    Mismatch { reasons: Vec<RecoveryMismatch> },
}

impl RecoveryVerification {
    /// For a caller that only needs the verdict; the workflow keeps the reasons
    /// because an unverified restore has to say what did not match.
    #[allow(dead_code)]
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Verified)
    }
}

/// Checks the runtime against the receipt of the apply a recovery was meant to
/// restore.
///
/// `confirmed` is the binding the operation that put the receipt back reported
/// as applied, and `None` for anything else it may have reported: the whole
/// point of this module is that a request's own transaction result is not a
/// statement about what is running, so only a confirmed apply counts.
///
/// Fail-closed throughout: every comparison needs a positive answer, and an
/// absent fact is a mismatch rather than a benefit of the doubt. A recovery
/// whose result cannot be confirmed must stay `RecoveryRequired`, not become
/// a silent success.
pub(in crate::client) fn verify_recovery_target(
    receipt: &RuntimeApplyReceipt,
    observed: &ObservedRuntime,
    confirmed: Option<&AppliedConfigBinding>,
) -> RecoveryVerification {
    let mut reasons = Vec::new();

    if observed.host != receipt.host {
        reasons.push(RecoveryMismatch::Host {
            expected: receipt.host,
            observed: observed.host,
        });
    }

    let running = matches!(observed.state, Some(CoreStateDetail::Running { .. }));
    let stopped = matches!(observed.state, Some(CoreStateDetail::Stopped { .. }));
    let intent_holds = match receipt.run_intent {
        CoreRunIntent::Running => running,
        CoreRunIntent::StoppedByUser => stopped,
    };
    if !intent_holds {
        reasons.push(RecoveryMismatch::RunIntent {
            expected: receipt.run_intent,
            observed: observed.state.clone(),
        });
    }

    // A core the user stopped has no applied document to compare, and asking
    // for one would make a correct restore look like a failure.
    if receipt.run_intent == CoreRunIntent::Running {
        // The source identity of the apply, not its effective one: everything
        // the runtime adds on top of the submitted document is a function of
        // the epoch it lands in, while the source hash is not. It identifies
        // the YAML and only the YAML, which is why the settings check below
        // exists rather than being assumed away by this comparison.
        let expected_hash = receipt.binding.revision.source_hash.as_str();
        if observed.source_hash.as_deref() != Some(expected_hash) {
            reasons.push(RecoveryMismatch::Content {
                expected: expected_hash.to_owned(),
                observed: observed.source_hash.clone(),
            });
        }

        // The document hash says nothing about which binary is running it, and
        // a host that does not report an applied kind has not proven anything.
        let expected_kind = wire_core_type_to_kind(&(&receipt.target_core).into());
        if observed.applied_kind.is_none() || observed.applied_kind != expected_kind {
            reasons.push(RecoveryMismatch::Core {
                expected: expected_kind,
                observed: observed.applied_kind,
            });
        }

        // The local-IPC settings are submitted with the document and published
        // by neither host, so a revision that differs from the receipt only in
        // its control channel matches everything above. The host's own
        // confirmation of the submission that carried them is the only thing
        // that separates the two, and it has to name this document on this
        // host to count.
        let confirms_settings = confirmed.is_some_and(|binding| {
            binding.revision.source_hash == expected_hash
                && binding.host == receipt.host
                && observed.binding.as_ref() == Some(binding)
                && matches!(observed.state, Some(CoreStateDetail::Running { epoch, .. }) if epoch == binding.revision.epoch)
        });
        if !confirms_settings {
            reasons.push(RecoveryMismatch::Submission {
                confirmed: confirmed.cloned(),
            });
        }
    }

    if reasons.is_empty() {
        RecoveryVerification::Verified
    } else {
        RecoveryVerification::Mismatch { reasons }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::application::ClashCore;

    pub(super) fn receipt(intent: CoreRunIntent) -> RuntimeApplyReceipt {
        RuntimeApplyReceipt {
            revision: crate::client::runtime::tests::test_revision(),
            config_text: std::sync::Arc::from("mode: rule\n"),
            config_digest: "digest".into(),
            target_core: ClashCore::Mihomo,
            core_spec: nyanpasu_core_manager::CoreSpec {
                kind: CoreKind::Mihomo,
                binary_path: camino::Utf8PathBuf::from("fake-core"),
                version: None,
                features: Vec::new(),
            },
            host: ExecutionHost::Local,
            run_intent: intent,
            local_ipc: nyanpasu_core_manager::LocalIpcSettings {
                policy: nyanpasu_core_manager::LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
            binding: crate::core::actor_v2::facade::AppliedConfigBinding {
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
                .resolve_candidate(&nyanpasu_config::clash::config::ClashConfig::default())
                .expect("default port strategies resolve"),
        }
    }

    pub(super) fn running(source_hash: &str) -> ObservedRuntime {
        ObservedRuntime {
            host: ExecutionHost::Local,
            binding: Some(confirmed(source_hash)),
            state: Some(CoreStateDetail::Running { epoch: 9, pid: 42 }),
            source_hash: Some(source_hash.into()),
            applied_kind: Some(CoreKind::Mihomo),
        }
    }

    /// The binding a host reports when it confirms the submission that put
    /// `source_hash` back. A restore that reported anything else confirmed
    /// nothing, and the tests pass `None` for those.
    pub(super) fn confirmed(source_hash: &str) -> AppliedConfigBinding {
        AppliedConfigBinding {
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 9,
                generation: 1,
                source_hash: source_hash.into(),
                effective_hash: "effective-a-epoch-9".into(),
            },
            host: ExecutionHost::Local,
            generation: 4,
        }
    }

    /// A recovery restarts the core, so it comes back with a different epoch,
    /// pid and controller generation. Only the target's identity has to match.
    ///
    /// The receipt's own binding is from epoch 4 and the core is now on epoch
    /// 9, which is what the runtime does to an unchanged configuration: its
    /// `ConfigSnapshot::prepare` rewrites the managed controller endpoint to
    /// `core-{epoch}.sock` before hashing, so the effective hash moves with the
    /// epoch while the source hash does not. Comparing effective hashes here
    /// would fail every successful restore of a local-IPC core.
    #[test]
    fn a_new_instance_generation_running_the_target_is_verified() {
        let verification = verify_recovery_target(
            &receipt(CoreRunIntent::Running),
            &running("source-a"),
            Some(&confirmed("source-a")),
        );
        assert_eq!(verification, RecoveryVerification::Verified);
    }

    /// V04: the restore request answered `RolledBack` and the core stayed on
    /// the configuration we were trying to get away from. That is a failed
    /// recovery, whatever the outcome was called.
    #[test]
    fn a_rolled_back_restore_that_left_the_core_on_b_is_not_a_recovery() {
        let verification =
            verify_recovery_target(&receipt(CoreRunIntent::Running), &running("source-b"), None);

        let RecoveryVerification::Mismatch { reasons } = verification else {
            panic!("a core still running the other document is not recovered");
        };
        assert_eq!(
            reasons,
            vec![
                RecoveryMismatch::Content {
                    expected: "source-a".into(),
                    observed: Some("source-b".into()),
                },
                RecoveryMismatch::Submission { confirmed: None },
            ]
        );
    }

    /// The audited IPC case. `A` and `B` are the same YAML submitted with
    /// different local-IPC settings, so they share a source hash and every
    /// other observable fact. The restore to `A` rolled back and left `B`
    /// listening on `B`'s control channel; the observation cannot see that, and
    /// the missing confirmation is what refuses it.
    #[test]
    fn a_rolled_back_restore_of_an_ipc_only_change_is_not_a_recovery() {
        let RecoveryVerification::Mismatch { reasons } =
            verify_recovery_target(&receipt(CoreRunIntent::Running), &running("source-a"), None)
        else {
            panic!("an unconfirmed restore never proves the settings it carried");
        };
        assert_eq!(
            reasons,
            vec![RecoveryMismatch::Submission { confirmed: None }]
        );
    }

    /// A confirmation of some *other* document is not a confirmation of this
    /// one, however healthy the runtime looks afterwards.
    #[test]
    fn a_submission_that_confirmed_another_document_proves_nothing() {
        let other = confirmed("source-b");
        let RecoveryVerification::Mismatch { reasons } = verify_recovery_target(
            &receipt(CoreRunIntent::Running),
            &running("source-a"),
            Some(&other),
        ) else {
            panic!("the submission confirmed a different document");
        };
        assert_eq!(
            reasons,
            vec![RecoveryMismatch::Submission {
                confirmed: Some(other)
            }]
        );
    }

    #[test]
    fn an_unpublished_state_or_revision_cannot_verify_anything() {
        let observed = ObservedRuntime {
            host: ExecutionHost::Local,
            binding: None,
            state: None,
            source_hash: None,
            applied_kind: None,
        };
        let RecoveryVerification::Mismatch { reasons } =
            verify_recovery_target(&receipt(CoreRunIntent::Running), &observed, None)
        else {
            panic!("an unobservable runtime is never verified");
        };
        assert_eq!(
            reasons.len(),
            4,
            "state, content, core and the submission all stay unproven"
        );
    }

    #[test]
    fn a_core_recovered_onto_another_host_is_not_the_target() {
        let mut observed = running("source-a");
        observed.host = ExecutionHost::Service;
        let RecoveryVerification::Mismatch { reasons } = verify_recovery_target(
            &receipt(CoreRunIntent::Running),
            &observed,
            Some(&confirmed("source-a")),
        ) else {
            panic!("a different execution host is a different target");
        };
        assert!(reasons.contains(&RecoveryMismatch::Host {
            expected: ExecutionHost::Local,
            observed: ExecutionHost::Service,
        }));
    }

    /// The right document under the wrong binary is not the target, and a host
    /// that will not name the applied core has not proven it is the right one.
    #[test]
    fn the_applied_core_must_be_named_and_match() {
        let proof = confirmed("source-a");
        let mut wrong = running("source-a");
        wrong.applied_kind = Some(CoreKind::ClashRust);
        assert!(
            !verify_recovery_target(&receipt(CoreRunIntent::Running), &wrong, Some(&proof))
                .is_verified()
        );

        let mut unnamed = running("source-a");
        unnamed.applied_kind = None;
        assert!(
            !verify_recovery_target(&receipt(CoreRunIntent::Running), &unnamed, Some(&proof))
                .is_verified()
        );
    }

    /// A core the user stopped is a target too. Restoring it means proving it
    /// is stopped, and never demanding an applied document it cannot have.
    #[test]
    fn a_stopped_target_is_verified_by_an_authoritative_stop() {
        let observed = ObservedRuntime {
            host: ExecutionHost::Local,
            binding: None,
            state: Some(CoreStateDetail::Stopped { reason: None }),
            source_hash: None,
            applied_kind: None,
        };
        assert_eq!(
            verify_recovery_target(&receipt(CoreRunIntent::StoppedByUser), &observed, None),
            RecoveryVerification::Verified
        );

        assert!(
            !verify_recovery_target(
                &receipt(CoreRunIntent::StoppedByUser),
                &running("source-a"),
                None
            )
            .is_verified(),
            "a core that is running is not a recovered stop"
        );
    }
}

#[cfg(test)]
mod binding_regression {
    use super::*;

    #[test]
    fn a_matching_source_hash_does_not_join_different_restore_instances() {
        let receipt = super::tests::receipt(CoreRunIntent::Running);
        let confirmed = super::tests::confirmed("source-a");
        let mut observed = super::tests::running("source-a");
        observed.binding.as_mut().unwrap().revision.generation += 1;
        assert!(!verify_recovery_target(&receipt, &observed, Some(&confirmed)).is_verified());
        observed.binding = Some(confirmed.clone());
        observed.binding.as_mut().unwrap().generation += 1;
        assert!(!verify_recovery_target(&receipt, &observed, Some(&confirmed)).is_verified());
    }
}
