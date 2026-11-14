//! Candidate documents shared by source owners and runtime consumers.

use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};

/// One source's proposed document; the other domains are sampled independently.
#[derive(Debug, Clone)]
pub enum DomainChange {
    Application { candidate: Arc<NyanpasuAppConfig> },
    Clash { candidate: Arc<ClashConfig> },
    Profiles { candidate: Arc<Profiles> },
}
