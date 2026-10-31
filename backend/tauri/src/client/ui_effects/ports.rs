//! Widget runtime capabilities consumed by the UI-side controller.

use std::time::Duration;

use nyanpasu_widget_protocol::{Logo, StatisticWidgetVariant};
use tokio::time::Instant;

use crate::client::effects::ports::WidgetError;

/// How long a widget stop waits for the widget to leave before it reports the
/// widget still owned (T10 §5.5).
pub const WIDGET_STOP_BOUND: Duration = Duration::from_secs(3);

/// The widget process itself, as the controller uses it.
///
/// Separate from [`WidgetController`](crate::client::effects::ports::WidgetController): the controller owns the lifecycle rules
/// (when a start is redundant, when a stop is a no-op) and this is the thing
/// those rules drive, so the rules are testable without spawning a process.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetRuntime: Send + Sync + 'static {
    async fn start(&self, variant: StatisticWidgetVariant) -> Result<(), WidgetError>;
    async fn stop(&self, deadline: Instant) -> Result<(), WidgetError>;
    async fn is_running(&self) -> bool;
    /// Remembers `logo` for every later start and sends it to the running
    /// widget. A send that fails is only logged, as a traffic sample is.
    async fn set_logo(&self, logo: Logo);
}
