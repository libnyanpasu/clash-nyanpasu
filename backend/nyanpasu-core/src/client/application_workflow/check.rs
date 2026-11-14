//! Independent candidate validation; only successful checks and commits reach the workflow.
use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_runtime_values::{
    mutation::ConfigDomain,
    runtime_error::{
        CheckUnavailableSnafu, CoreRejectedConfigSnafu, ResolveCoreBinarySnafu, RuntimeError,
    },
};
use nyanpasu_state_engine::{StateSnapshot, Version};
use snafu::IntoError as _;

use super::{
    ApplicationWorkflowClient,
    ports::{RuntimeBuildPort, RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeValidatorPort},
    preparation::RuntimePreparation,
};
use crate::{
    client::{
        core_lifecycle::ports::RuntimePreparationPort, ports::SessionPortResolver,
        runtime::RuntimeRevisionAllocator,
    },
    state::mutation::{
        DomainChange,
        ports::{RuntimeCandidate, RuntimeGate},
    },
};

pub(in crate::client) struct CandidateCheck {
    application: StateSnapshot<NyanpasuAppConfig>,
    clash: StateSnapshot<ClashConfig>,
    preparation: RuntimePreparation,
    validator: Arc<dyn RuntimeValidatorPort>,
    workflow: ApplicationWorkflowClient,
}

pub(in crate::client) struct Args {
    pub application: StateSnapshot<NyanpasuAppConfig>,
    pub clash: StateSnapshot<ClashConfig>,
    pub profiles: StateSnapshot<Profiles>,
    pub content_lock: crate::state::mutation::ProfileContentLock,
    pub builder: Arc<dyn RuntimeBuildPort>,
    pub ports: Arc<SessionPortResolver>,
    pub revisions: RuntimeRevisionAllocator,
    pub validator: Arc<dyn RuntimeValidatorPort>,
    pub workflow: ApplicationWorkflowClient,
}

impl CandidateCheck {
    pub(in crate::client) fn new(args: Args) -> Self {
        let Args {
            application,
            clash,
            profiles,
            content_lock,
            builder,
            ports,
            revisions,
            validator,
            workflow,
        } = args;
        let preparation = RuntimePreparation::new(
            application.clone(),
            clash.clone(),
            profiles.clone(),
            content_lock,
            builder,
            ports,
        )
        .with_revisions(revisions);
        Self {
            application,
            clash,
            preparation,
            validator,
            workflow,
        }
    }
}

#[async_trait::async_trait]
impl RuntimeGate for CandidateCheck {
    async fn check(&self, candidate: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
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
        let prepared = self
            .preparation
            .prepare_candidate_inputs(inputs)
            .await
            .map_err(Arc::new)?;
        let core_spec = self
            .preparation
            .core_spec(&prepared.snapshot.target_core)
            .map_err(|error| Arc::new(ResolveCoreBinarySnafu.into_error(error)))?;
        match self
            .validator
            .check(RuntimeCheckRequest {
                core_spec,
                intent: &prepared.intent,
            })
            .await
        {
            RuntimeCheckOutcome::Passed => self.workflow.checked(prepared),
            RuntimeCheckOutcome::Rejected { kind, message } => {
                return Err(Arc::new(
                    CoreRejectedConfigSnafu {
                        core_kind: kind,
                        message,
                    }
                    .build(),
                ));
            }
            RuntimeCheckOutcome::Unavailable(reason) => {
                return Err(Arc::new(CheckUnavailableSnafu { reason }.build()));
            }
        }
        Ok(())
    }

    fn committed(&self, domain: ConfigDomain, version: Version) {
        self.workflow.source_committed(domain, version);
    }
}
