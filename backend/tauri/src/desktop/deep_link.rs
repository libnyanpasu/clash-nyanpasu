//! Custom-scheme deep links: which launches carry one, how the OS learns to
//! start this app for them, and how a link reaches the frontend.
//!
//! Windows and Linux start a new process with the URL as its only argument; a
//! later process hands its arguments to the primary instance, which filters
//! them here. macOS delivers URLs to the running app as `RunEvent::Opened`, and
//! takes the schemes from the bundle's `Info.plist`.

use std::{ffi::OsStr, io, path::Path};

use snafu::Snafu;
use tauri::{AppHandle, Manager as _};
use url::Url;

use crate::window;

/// The schemes this app handles. The installers register the same list.
pub const SCHEMES: [&str; 2] = ["clash-nyanpasu", "clash"];

/// The deep link a launch carries: exactly one argument after the executable,
/// a URL with one of [`SCHEMES`]. Anything else is an ordinary launch.
pub fn from_args<I, S>(args: I) -> Option<Url>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter().skip(1);
    let candidate = args.next()?;
    if args.next().is_some() {
        return None;
    }
    accepted(Url::parse(candidate.as_ref().to_str()?).ok()?)
}

/// `url` when its scheme is one of [`SCHEMES`].
pub fn accepted(url: Url) -> Option<Url> {
    SCHEMES.contains(&url.scheme()).then_some(url)
}

/// Queues `url` for the frontend, shows the main window and pokes any listening
/// frontend to take the queue. With no frontend listening yet the poke is lost
/// but the link is not: a frontend drains the queue once it has registered its
/// listener.
///
/// The queue is managed before the app runs. The event bus and the window
/// manager are not until setup, which a macOS `Opened` event can precede; the
/// queued link then waits for the next frontend to mount.
pub fn deliver(app_handle: &AppHandle, url: &Url) {
    // The URL itself can carry a subscription token, so only its scheme is logged.
    tracing::info!(scheme = url.scheme(), "deep link received");
    app_handle
        .state::<crate::ipc::PendingDeepLinks>()
        .push(url.to_string());
    if let Some(events) = app_handle.try_state::<nyanpasu_rpc::events::EventBus>()
        && let Err(error) = events.publish_serialized(
            nyanpasu_rpc_protocol::EventName::SchemeRequestReceived,
            &crate::ipc::SchemeRequestReceivedEvent,
        )
    {
        tracing::error!(%error, "failed to emit scheme-request-received event");
    }
    show_main_window(app_handle);
}

/// Shows and focuses the main window, building it when it is closed. Before
/// setup has managed the window manager there is no window to show yet.
pub fn show_main_window(app_handle: &AppHandle) {
    let Some(windows) = app_handle.try_state::<window::WindowManager>() else {
        return;
    };
    if let Err(error) = windows.open(&window::kinds::MainWindow, None) {
        tracing::error!(%error, "failed to show the main window");
    }
}

/// Failure to register this executable as the handler of [`SCHEMES`].
#[derive(Debug, Snafu)]
pub enum RegisterError {
    #[cfg(windows)]
    #[snafu(display("failed to register the {scheme} scheme in the registry"))]
    Registry {
        scheme: &'static str,
        source: io::Error,
    },
    #[cfg(target_os = "linux")]
    #[snafu(display("the XDG data directory is unknown"))]
    DataDirectory,
    #[cfg(target_os = "linux")]
    #[snafu(display("failed to write the desktop entry {}", path.display()))]
    DesktopEntry {
        path: std::path::PathBuf,
        source: io::Error,
    },
    #[cfg(target_os = "linux")]
    #[snafu(display("failed to run {tool}"))]
    RunTool {
        tool: &'static str,
        source: io::Error,
    },
    #[cfg(target_os = "linux")]
    #[snafu(display("{tool} exited with {status}"))]
    ToolFailed {
        tool: &'static str,
        status: std::process::ExitStatus,
    },
}

/// Registers `executable` for the current user as the handler of [`SCHEMES`],
/// replacing what an earlier location registered.
///
/// On Windows this writes `HKCU\Software\Classes\<scheme>`, as the installer
/// does for all users; `identifier` names the protocol. On Linux it writes a
/// hidden desktop entry, named after `product_name`, and makes it the default
/// handler. macOS registers the schemes from `Info.plist`, so this does nothing
/// there.
///
/// # Errors
/// Fails when the registry, the desktop entry or the XDG tools fail.
pub fn register(
    executable: &Path,
    identifier: &str,
    product_name: &str,
) -> Result<(), RegisterError> {
    platform::register(executable, identifier, product_name)
}

#[cfg(windows)]
mod platform {
    use std::{io, path::Path};

    use snafu::ResultExt as _;

    use super::{RegisterError, RegistrySnafu, SCHEMES};

    pub(super) fn register(
        executable: &Path,
        identifier: &str,
        _product_name: &str,
    ) -> Result<(), RegisterError> {
        let executable = dunce::simplified(executable).display().to_string();
        for scheme in SCHEMES {
            write_scheme(scheme, &executable, identifier)
                .map_err(io::Error::from)
                .context(RegistrySnafu { scheme })?;
        }
        Ok(())
    }

    fn write_scheme(
        scheme: &str,
        executable: &str,
        identifier: &str,
    ) -> windows_registry::Result<()> {
        let base = format!(r"Software\Classes\{scheme}");
        let key = windows_registry::CURRENT_USER.create(&base)?;
        key.set_string("", format!("URL:{identifier} protocol"))?;
        key.set_string("URL Protocol", "")?;
        windows_registry::CURRENT_USER
            .create(format!(r"{base}\DefaultIcon"))?
            .set_string("", format!("\"{executable}\",0"))?;
        windows_registry::CURRENT_USER
            .create(format!(r"{base}\shell\open\command"))?
            .set_string("", format!("\"{executable}\" \"%1\""))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{io, path::Path, process::Command};

    use snafu::{OptionExt as _, ResultExt as _};

    use super::{
        DataDirectorySnafu, DesktopEntrySnafu, RegisterError, RunToolSnafu, SCHEMES,
        ToolFailedSnafu,
    };

    pub(super) fn register(
        executable: &Path,
        _identifier: &str,
        product_name: &str,
    ) -> Result<(), RegisterError> {
        let directory = dirs::data_dir()
            .context(DataDirectorySnafu)?
            .join("applications");
        let file_name = format!(
            "{}-handler.desktop",
            executable
                .file_name()
                .unwrap_or(executable.as_os_str())
                .to_string_lossy()
        );
        let path = directory.join(&file_name);
        // An AppImage runs from a temporary mount; the OS must start the image.
        let exec = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| executable.to_owned());
        let entry = desktop_entry(product_name, &exec.to_string_lossy());
        if std::fs::read_to_string(&path).ok().as_deref() != Some(entry.as_str()) {
            std::fs::create_dir_all(&directory).context(DesktopEntrySnafu { path: &path })?;
            std::fs::write(&path, entry).context(DesktopEntrySnafu { path: &path })?;
        }
        match Command::new("update-desktop-database")
            .arg(&directory)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => {
                return ToolFailedSnafu {
                    tool: "update-desktop-database",
                    status,
                }
                .fail();
            }
            // xdg-mime still registers the handler; only the MIME cache stays stale.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::warn!("update-desktop-database is not installed");
            }
            Err(source) => {
                return Err(RegisterError::RunTool {
                    tool: "update-desktop-database",
                    source,
                });
            }
        }
        for scheme in SCHEMES {
            let status = Command::new("xdg-mime")
                .args(["default", &file_name, &format!("x-scheme-handler/{scheme}")])
                .status()
                .context(RunToolSnafu { tool: "xdg-mime" })?;
            snafu::ensure!(
                status.success(),
                ToolFailedSnafu {
                    tool: "xdg-mime",
                    status,
                }
            );
        }
        Ok(())
    }

    /// A hidden entry that only handles the schemes.
    pub(super) fn desktop_entry(name: &str, exec: &str) -> String {
        let mime_types: String = SCHEMES
            .iter()
            .map(|scheme| format!("x-scheme-handler/{scheme};"))
            .collect();
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec={} %u\nTerminal=false\nMimeType={mime_types}\nNoDisplay=true\n",
            quote_exec(exec)
        )
    }

    /// Quotes one `Exec` argument as the Desktop Entry specification requires.
    pub(super) fn quote_exec(argument: &str) -> String {
        let mut quoted = String::with_capacity(argument.len() + 2);
        quoted.push('"');
        for character in argument.chars() {
            if matches!(character, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        quoted
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use std::path::Path;

    use super::RegisterError;

    pub(super) fn register(
        _executable: &Path,
        _identifier: &str,
        _product_name: &str,
    ) -> Result<(), RegisterError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{accepted, from_args};

    #[test]
    fn only_a_single_url_with_our_scheme_is_a_deep_link() {
        let link = "clash://install-config?url=https%3A%2F%2Fexample.com";
        assert_eq!(
            from_args(["app", link]).map(String::from),
            Some(link.to_owned())
        );
        assert!(from_args(["app", "clash-nyanpasu://install-config"]).is_some());
        for args in [
            &["app"][..],
            &["app", link, "--extra"],
            &["app", "https://example.com"],
            &["app", r"C:\Users\profile.yaml"],
            &["app", "migrate"],
            &["app", "not a url"],
        ] {
            assert_eq!(from_args(args), None, "{args:?}");
        }
    }

    #[test]
    fn other_schemes_are_not_accepted() {
        assert!(accepted("clash://x".parse().unwrap()).is_some());
        assert!(accepted("file:///etc/passwd".parse().unwrap()).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_desktop_entry_quotes_its_executable() {
        assert_eq!(
            super::platform::quote_exec(r#"/opt/a b/"$x`\app"#),
            r#""/opt/a b/\"\$x\`\\app""#
        );
        let entry = super::platform::desktop_entry("Clash Nyanpasu", "/usr/bin/app");
        assert!(entry.contains("Exec=\"/usr/bin/app\" %u\n"), "{entry}");
        assert!(
            entry.contains("MimeType=x-scheme-handler/clash-nyanpasu;x-scheme-handler/clash;\n"),
            "{entry}"
        );
        assert!(entry.contains("NoDisplay=true\n"), "{entry}");
    }
}
