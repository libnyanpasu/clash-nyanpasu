//! Deterministic backend shared by owner and application workflow tests.

use std::{sync::Arc, time::Instant};

use anyhow::Result;
use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use tokio_util::sync::CancellationToken;

use super::{
    ManifestVersion, UpdaterState,
    artifact::PreparedCoreBinary,
    ports::{UpdaterBackend, UpdaterProgress},
    shared::{CoreTypeMeta, get_arch},
};

/// Prepares a local test artifact without network access.
pub struct ReadyBackend;

#[async_trait]
impl UpdaterBackend for ReadyBackend {
    async fn fetch_manifest(
        &self,
        _mirror: Option<(String, Instant)>,
    ) -> Result<(ManifestVersion, (String, Instant))> {
        let mut manifest = ManifestVersion::default();
        manifest.latest.mihomo = "test-version".into();
        manifest
            .arch_template
            .mihomo
            .insert(get_arch().unwrap().into(), "mihomo-{}.gz".into());
        Ok((manifest, ("https://github.com".into(), Instant::now())))
    }

    async fn prepare(
        &self,
        core: ClashCore,
        _mirror: String,
        _artifact: String,
        _tag: CoreTypeMeta,
        progress: UpdaterProgress,
        _shutdown: &CancellationToken,
    ) -> Result<PreparedCoreBinary> {
        let staging = Arc::new(tempfile::TempDir::new()?);
        let source = staging.path().join("prepared");
        std::fs::write(&source, b"core binary")?;
        progress.report(UpdaterState::Replacing, None);
        Ok(PreparedCoreBinary {
            target: core,
            source,
            destination: staging.path().join("installed"),
            staging,
            progress: Arc::new(progress),
            companion: None,
        })
    }
}
