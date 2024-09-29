use crate::config::profile::item_type::ProfileUid;

use super::{ProfileShared, ProfileSharedBuilder};
use derive_builder::Builder;
use indexmap::IndexMap;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate)]
#[builder(derive(Serialize, Deserialize))]
#[builder_update(patch_fn = "apply")]
pub struct LocalProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(Into::into)?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
    /// file symlinks
    pub symlinks: IndexMap<String, PathBuf>,
    /// process chains
    #[serde(default)]
    pub chains: Vec<ProfileUid>,
}
