use super::{
    ambassador_impl_ProfileFileIo, ambassador_impl_ProfileSharedGetter,
    ambassador_impl_ProfileSharedSetter, ProfileCleanup, ProfileFileIo, ProfileHelper,
    ProfileShared, ProfileSharedBuilder, ProfileSharedGetter, ProfileSharedSetter,
};
use ambassador::Delegate;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileSharedGetter, target = "shared")]
#[delegate(ProfileSharedSetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct MergeProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(|e| MergeProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
}

impl ProfileCleanup for MergeProfile {}
impl ProfileHelper for MergeProfile {}
