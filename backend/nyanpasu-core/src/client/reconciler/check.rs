//! Independent candidate validation; only successful checks and commits reach the workflow.
use std::sync::Arc;

use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};
use nyanpasu_runtime_values::{mutation::ConfigDomain, runtime_error::RuntimeError};
use nyanpasu_state_engine::{StateSnapshot, Version};

use super::{
    RuntimeReconcilerClient,
    ports::{RuntimeBuildPort, RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeValidatorPort},
    preparation::{PrepareFailure, RuntimePreparation},
};
use crate::{
    client::{
        candidate_inspection::{CandidateEvidence, CandidateSource, CandidateStore},
        core_lifecycle::ports::RuntimePreparationPort,
        ports::SessionPortResolver,
        runtime::RuntimeRevisionAllocator,
        runtime_inspection::CandidateStage,
    },
    state::mutation::{
        DomainChange,
        ports::{CheckPass, KeepChecked, RuntimeCandidate, RuntimeGate},
    },
};

pub(in crate::client) struct CandidateCheck {
    application: StateSnapshot<NyanpasuAppConfig>,
    clash: StateSnapshot<ClashConfig>,
    preparation: RuntimePreparation,
    validator: Arc<dyn RuntimeValidatorPort>,
    workflow: RuntimeReconcilerClient,
    candidates: Arc<CandidateStore>,
}

pub(in crate::client) struct Args {
    pub application: StateSnapshot<NyanpasuAppConfig>,
    pub clash: StateSnapshot<ClashConfig>,
    pub profile_content: crate::state::mutation::ProfileContentReader,
    pub builder: Arc<dyn RuntimeBuildPort>,
    pub ports: Arc<SessionPortResolver>,
    pub revisions: RuntimeRevisionAllocator,
    pub validator: Arc<dyn RuntimeValidatorPort>,
    pub workflow: RuntimeReconcilerClient,
    pub candidates: Arc<CandidateStore>,
}

impl CandidateCheck {
    pub(in crate::client) fn new(args: Args) -> Self {
        let Args {
            application,
            clash,
            profile_content,
            builder,
            ports,
            revisions,
            validator,
            workflow,
            candidates,
        } = args;
        let preparation =
            RuntimePreparation::new(profile_content, builder, ports).with_revisions(revisions);
        Self {
            application,
            clash,
            preparation,
            validator,
            workflow,
            candidates,
        }
    }

    /// Keeps the candidate the failure refused, naming it in the error.
    async fn keep(
        &self,
        error: RuntimeError,
        candidate: Option<Box<CandidateEvidence>>,
        source: Option<CandidateSource>,
    ) -> RuntimeError {
        let Some(candidate) = candidate else {
            return error;
        };
        match self.candidates.save((*candidate).from_source(source)).await {
            Some(id) => error.with_candidate_snapshot_id(id),
            None => error,
        }
    }
}

/// A candidate the core accepted, kept only if its source then fails to
/// commit it.
struct Prechecked {
    candidates: Arc<CandidateStore>,
    candidate: CandidateEvidence,
}

#[async_trait::async_trait]
impl KeepChecked for Prechecked {
    async fn keep(
        self: Box<Self>,
    ) -> Option<nyanpasu_runtime_values::candidate::CandidateSnapshotId> {
        self.candidates.save(self.candidate).await
    }
}

#[async_trait::async_trait]
impl RuntimeGate for CandidateCheck {
    async fn check(&self, candidate: RuntimeCandidate) -> Result<CheckPass, Arc<RuntimeError>> {
        let source = candidate.background.as_ref().map(CandidateSource::profile);
        let app = match &candidate.change {
            DomainChange::Application { candidate, .. } => candidate.as_ref().clone(),
            _ => self.application.load().state.clone(),
        };
        let clash = match &candidate.change {
            DomainChange::Clash { candidate } => candidate.as_ref().clone(),
            _ => self.clash.load().state.clone(),
        };
        let mut inputs = match &candidate.change {
            DomainChange::Profiles { candidate, .. } => {
                self.preparation
                    .capture_inputs(app, clash, candidate.clone())
                    .await
            }
            _ => {
                self.preparation
                    .capture_committed_inputs(app, clash)
                    .await
                    .1
            }
        };
        for (path, content) in candidate.staged {
            inputs.content.0.insert(path, Ok(content));
        }
        let prepared = match self.preparation.prepare_candidate_inputs(inputs).await {
            Ok(prepared) => prepared,
            Err(PrepareFailure { error, candidate }) => {
                return Err(Arc::new(self.keep(error, candidate, source).await));
            }
        };
        let failure = match self.preparation.core_spec(&prepared.snapshot.target_core) {
            Err(source) => RuntimeError::ResolveCoreBinary {
                source,
                candidate_snapshot_id: None,
            },
            Ok(core_spec) => match self
                .validator
                .check(RuntimeCheckRequest {
                    core_spec,
                    intent: &prepared.intent,
                })
                .await
            {
                RuntimeCheckOutcome::Passed => {
                    let candidate = CandidateEvidence::of_snapshot(&prepared.snapshot)
                        .at(CandidateStage::Prechecked)
                        .from_source(source);
                    self.workflow.checked(prepared);
                    return Ok(CheckPass::new(Prechecked {
                        candidates: self.candidates.clone(),
                        candidate,
                    }));
                }
                RuntimeCheckOutcome::Rejected { kind, message } => {
                    RuntimeError::CoreRejectedConfig {
                        core_kind: kind,
                        message,
                        candidate_snapshot_id: None,
                    }
                }
                RuntimeCheckOutcome::Unavailable(reason) => RuntimeError::CheckUnavailable {
                    reason,
                    candidate_snapshot_id: None,
                },
            },
        };
        // Every refusal of a built candidate leaves through here.
        let candidate = CandidateEvidence::of_snapshot(&prepared.snapshot);
        Err(Arc::new(
            self.keep(failure, Some(Box::new(candidate)), source).await,
        ))
    }

    fn committed(&self, domain: ConfigDomain, version: Version) {
        self.workflow.source_committed(domain, version);
    }
}
