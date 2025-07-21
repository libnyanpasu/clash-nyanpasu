use crate::config::{ProfileKindGetter, profile::item_type::ProfileItemType};

use super::{
    ProfileCleanup, ProfileFileIo, ProfileHelper, ProfileMetaGetter, ProfileMetaSetter,
    ProfileShared, ProfileSharedBuilder, ambassador_impl_ProfileFileIo,
    ambassador_impl_ProfileMetaGetter, ambassador_impl_ProfileMetaSetter,
};
use ambassador::Delegate;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};

const PROFILE_TYPE: ProfileItemType = ProfileItemType::Merge;

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
#[delegate(ProfileMetaSetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct MergeProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build(&PROFILE_TYPE).map_err(|e| MergeProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
}

impl MergeProfile {
    pub fn builder() -> MergeProfileBuilder {
        let mut builder = MergeProfileBuilder::default();
        let shared = ProfileShared::get_default_builder(&PROFILE_TYPE);
        builder.shared(shared);
        builder
    }
}

impl ProfileKindGetter for MergeProfile {
    fn kind(&self) -> ProfileItemType {
        PROFILE_TYPE
    }
}

impl ProfileCleanup for MergeProfile {}
impl ProfileHelper for MergeProfile {}
