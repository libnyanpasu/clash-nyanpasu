use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;

/// One named profile item.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProfileItem {
    pub uid: ProfileId,

    #[serde(flatten)]
    pub metadata: ProfileMetadata,

    #[serde(flatten)]
    pub definition: ProfileDefinition,
}
