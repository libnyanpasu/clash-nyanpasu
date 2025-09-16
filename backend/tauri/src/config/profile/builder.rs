use crate::config::*;

use super::item::{
    LocalProfileBuilder, MergeProfileBuilder, RemoteProfileBuilder, ScriptProfileBuilder,
};

#[derive(Debug, serde:: Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileBuilder {
    Remote(RemoteProfileBuilder),
    Local(LocalProfileBuilder),
    Merge(MergeProfileBuilder),
    Script(ScriptProfileBuilder),
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileBuilderError {
    #[error(transparent)]
    Remote(#[from] RemoteProfileBuilderError),
    #[error(transparent)]
    Local(#[from] LocalProfileBuilderError),
    #[error(transparent)]
    Merge(#[from] MergeProfileBuilderError),
    #[error(transparent)]
    Script(#[from] ScriptProfileBuilderError),
}

impl ProfileBuilder {
    pub fn build(self) -> Result<Profile, ProfileBuilderError> {
        let profile = match self {
            ProfileBuilder::Remote(mut builder) => builder.build()?.into(),
            ProfileBuilder::Local(builder) => builder.build()?.into(),
            ProfileBuilder::Merge(builder) => builder.build()?.into(),
            ProfileBuilder::Script(builder) => builder.build()?.into(),
        };

        Ok(profile)
    }
}
