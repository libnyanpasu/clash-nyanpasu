use std::io::{ErrorKind, Result};

use once_cell::sync::OnceCell;

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform_impl;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform_impl;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform_impl;

static ID: OnceCell<String> = OnceCell::new();

/// This function is meant for use-cases where the default [`prepare()`] function can't be used.
///
/// # Errors
/// If ID was already set this functions returns an AlreadyExists error.
pub fn set_identifier(identifier: &str) -> Result<()> {
    ID.set(identifier.to_string())
        .map_err(|_| ErrorKind::AlreadyExists.into())
}

// Consider adding a function to register without starting the listener.

/// Registers a handler for the given scheme.
///
/// ## Platform-specific:
///
/// - **macOS**: On macOS schemes must be defined in an Info.plist file, therefore this function only calls [`listen()`] without registering the scheme. This function can only be called once on macOS.
pub fn register<F: FnMut(String) + Send + 'static>(scheme: &[&str], handler: F) -> Result<()> {
    platform_impl::register(scheme, handler)
}

/// Starts the event listener without registering any schemes.
///
/// ## Platform-specific:
///
/// - **macOS**: This function can only be called once on macOS.
pub fn listen<F: FnMut(String) + Send + 'static>(handler: F) -> Result<()> {
    platform_impl::listen(handler)
}

/// Unregister a previously registered scheme.
///
/// ## Platform-specific:
///
/// - **macOS**: This function has no effect on macOS.
pub fn unregister(scheme: &[&str]) -> Result<()> {
    platform_impl::unregister(scheme)
}

/// Checks if current instance is the primary instance.
/// Also sends the URL event data to the primary instance and stops the process afterwards.
///
/// ## Platform-specific:
///
/// - **macOS**: Only registers the identifier (only relevant in debug mode). It does not interact with the primary instance and does not exit the app.
pub fn prepare(identifier: &str) {
    platform_impl::prepare(identifier)
}
