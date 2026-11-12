//! What a request to the effects owner fails with.

use serde::{Deserialize, Serialize};
use snafu::Snafu;

#[derive(Debug, Snafu, Serialize, Deserialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectsError {
    /// The owner's mailbox is closed: the app is exiting, or the owner died.
    #[snafu(display("the effects owner is not running"))]
    EffectsStopped,
}
