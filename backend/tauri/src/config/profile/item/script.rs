use super::{
    ambassador_impl_ProfileFileIo, ambassador_impl_ProfileSharedGetter,
    ambassador_impl_ProfileSharedSetter, ProfileCleanup, ProfileFileIo, ProfileHelper,
    ProfileShared, ProfileSharedBuilder, ProfileSharedGetter, ProfileSharedSetter,
};
use crate::{config::profile::item_type::ProfileItemType, enhance::ScriptType};
use ambassador::Delegate;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileSharedSetter, target = "shared")]
#[delegate(ProfileSharedGetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct ScriptProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(|e| ScriptProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
}

impl ScriptProfile {
    pub fn builder(script_type: &ScriptType) -> ScriptProfileBuilder {
        let mut builder = ScriptProfileBuilder::default();
        let mut shared = ProfileSharedBuilder::default();
        shared.r#type(ProfileItemType::Script(script_type.clone()));
        builder.shared(shared);
        builder
    }
}

impl ProfileHelper for ScriptProfile {}
impl ProfileCleanup for ScriptProfile {}
