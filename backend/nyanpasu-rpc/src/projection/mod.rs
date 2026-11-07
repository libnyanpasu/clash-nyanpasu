//! Value conversions at the application/transport boundary, without operational behavior.

mod app_update;
mod clash;
mod configuration;
mod core;
mod events;
mod logs;
mod profiles;
mod runtime;
mod storage;
pub mod streams;
mod updater;

/// Consume an application snapshot into its transport-owned representation.
pub trait IntoWire {
    /// The protocol value with the same serialized shape.
    type Wire;

    /// Preserve the snapshot's values without performing application work.
    fn into_wire(self) -> Self::Wire;
}

/// Convert a decoded transport request into the application's existing input.
pub trait IntoApplication {
    /// The input owned by the application capability.
    type Application;

    /// Preserve request values; admission and validation remain with the application.
    fn into_application(self) -> Self::Application;
}

#[cfg(test)]
mod tests;
