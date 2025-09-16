use super::{
    ProfileCleanup, ProfileFileIo, ProfileHelper, ProfileMetaGetter, ProfileMetaSetter,
    ProfileShared, ProfileSharedBuilder, ambassador_impl_ProfileFileIo,
    ambassador_impl_ProfileMetaGetter, ambassador_impl_ProfileMetaSetter,
};
use crate::{
    config::{ProfileKindGetter, profile::item_type::ProfileItemType},
    enhance::ScriptType,
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
#[delegate(ProfileMetaSetter, target = "shared")]
#[delegate(ProfileMetaGetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct ScriptProfile {
    #[serde(flatten)]
    #[builder(field(ty = "ProfileSharedBuilder", build = "self.build_shared()?"))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
    pub script_type: ScriptType,
}

impl ScriptProfileBuilder {
    fn build_shared(&self) -> Result<ProfileShared, ScriptProfileBuilderError> {
        self.script_type
            .ok_or(ScriptProfileBuilderError::UninitializedField(
                "`script_type` is missing",
            ))
            .and_then(|script_type| {
                self.shared
                    .build(&ProfileItemType::Script(script_type))
                    .map_err(|e| ScriptProfileBuilderError::from(e.to_string()))
            })
    }
}

impl ProfileKindGetter for ScriptProfile {
    fn kind(&self) -> ProfileItemType {
        ProfileItemType::Script(self.script_type)
    }
}

impl ScriptProfile {
    pub fn builder(script_type: &ScriptType) -> ScriptProfileBuilder {
        let mut builder = ScriptProfileBuilder::default();
        let shared = ProfileShared::get_default_builder(&ProfileItemType::Script(*script_type));
        builder.script_type(*script_type);
        builder.shared(shared);
        builder
    }
}

impl ProfileHelper for ScriptProfile {}
impl ProfileCleanup for ScriptProfile {}
