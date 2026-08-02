use std::sync::Arc;

use nyanpasu_config::application::ClashCore;
use serde_yaml::Mapping;
use sha2::{Digest, Sha256};

use crate::enhance::PostProcessingOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RuntimeRevision(pub(crate) u64);

impl RuntimeRevision {
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshotData {
    pub(crate) config: Mapping,
    pub(crate) exists_keys: Vec<String>,
    pub(crate) postprocessing_output: PostProcessingOutput,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshot {
    pub(crate) revision: RuntimeRevision,
    pub(crate) target_core: ClashCore,
    pub(crate) product_sha256: [u8; 32],
    product_bytes: Arc<[u8]>,
    pub(crate) config: Mapping,
    pub(crate) exists_keys: Vec<String>,
    pub(crate) postprocessing_output: PostProcessingOutput,
}

impl RuntimeSnapshot {
    pub(crate) fn from_data(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Arc<[u8]>,
        data: RuntimeSnapshotData,
    ) -> Self {
        let product_sha256 = Sha256::digest(&product_bytes).into();
        Self {
            revision,
            target_core,
            product_sha256,
            product_bytes,
            config: data.config,
            exists_keys: data.exists_keys,
            postprocessing_output: data.postprocessing_output,
        }
    }

    pub(crate) fn product_bytes(&self) -> &[u8] {
        &self.product_bytes
    }

    pub(crate) fn identity_eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.target_core == other.target_core
            && self.product_sha256 == other.product_sha256
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeLifecycleState {
    pub(crate) promoted: Option<Arc<RuntimeSnapshot>>,
    pub(crate) applied: Option<Arc<RuntimeSnapshot>>,
}
