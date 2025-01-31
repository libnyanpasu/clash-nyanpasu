use super::{
    ambassador_impl_ProfileFileIo, ambassador_impl_ProfileSharedGetter,
    ambassador_impl_ProfileSharedSetter, ProfileCleanup, ProfileFileIo, ProfileHelper,
    ProfileShared, ProfileSharedBuilder, ProfileSharedGetter, ProfileSharedSetter,
};
use crate::config::profile::item_type::ProfileUid;
use ambassador::Delegate;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileSharedGetter, target = "shared")]
#[delegate(ProfileSharedSetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct LocalProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(|e| LocalProfileBuilderError::from(e.to_string()))?"
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

impl ProfileHelper for LocalProfile {}
impl ProfileCleanup for LocalProfile {}
