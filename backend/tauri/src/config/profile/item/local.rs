use super::{
    ProfileCleanup, ProfileFileIo, ProfileHelper, ProfileMetaGetter, ProfileMetaSetter,
    ProfileShared, ProfileSharedBuilder, ambassador_impl_ProfileFileIo,
    ambassador_impl_ProfileMetaGetter, ambassador_impl_ProfileMetaSetter,
};
use crate::config::{
    ProfileKindGetter,
    profile::item_type::{ProfileItemType, ProfileUid},
};
use ambassador::Delegate;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const PROFILE_TYPE: ProfileItemType = ProfileItemType::Local;

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
#[delegate(ProfileMetaSetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct LocalProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build(&PROFILE_TYPE).map_err(|e| LocalProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option), default)]
    /// file symlinks
    pub symlinks: Option<PathBuf>,
    /// process chain
    #[builder(default)]
    #[serde(alias = "chains", default)]
    #[builder_field_attr(serde(alias = "chains", default))]
    pub chain: Vec<ProfileUid>,
}

impl LocalProfile {
    pub fn builder() -> LocalProfileBuilder {
        let mut builder = LocalProfileBuilder::default();
        let shared = ProfileShared::get_default_builder(&PROFILE_TYPE);
        builder.shared(shared);
        builder
    }
}

impl ProfileKindGetter for LocalProfile {
    fn kind(&self) -> ProfileItemType {
        PROFILE_TYPE
    }
}

impl ProfileHelper for LocalProfile {}
impl ProfileCleanup for LocalProfile {}
