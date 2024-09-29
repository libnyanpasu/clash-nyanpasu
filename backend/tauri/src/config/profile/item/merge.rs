use super::{ProfileShared, ProfileSharedBuilder};
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate)]
#[builder(derive(Serialize, Deserialize))]
#[builder_update(patch_fn = "apply")]
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
