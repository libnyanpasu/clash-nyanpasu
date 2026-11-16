//! What the network automation actor consumes besides the network observer.

use nyanpasu_config::application::MechanismActions;
use nyanpasu_runtime_values::network_automation::MechanismOutcomes;

/// The capture switches: system proxy, TUN and transparent proxy.
///
/// The actor decides when a rule applies; this port writes the switches
/// through the same committed mutations a settings change uses, and answers
/// what happened to each.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub(crate) trait CaptureSwitches: Send + Sync + 'static {
    async fn apply(&self, actions: MechanismActions) -> MechanismOutcomes;
}
