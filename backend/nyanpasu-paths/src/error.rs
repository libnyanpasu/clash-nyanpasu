use camino::Utf8PathBuf;
use snafu::Snafu;
#[cfg(windows)]
use std::ffi::OsString;
use std::{fmt, io, path::PathBuf, sync::Arc};

/// Which of the two base directories an error is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Config,
    Data,
}

impl fmt::Display for Root {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Config => "config",
            Self::Data => "data",
        })
    }
}

/// The install dir could not be determined. Clonable because the resolver stores the
/// outcome and hands it out on every query.
#[derive(Debug, Clone, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum InstallDirError {
    #[snafu(display("the application install dir is unknown"))]
    Unknown,
    #[snafu(display("failed to locate the current executable"))]
    Executable { source: Arc<io::Error> },
    #[snafu(display("failed to canonicalize the executable path"))]
    Canonicalize { source: Arc<io::Error> },
    #[snafu(display("the executable has no parent directory"))]
    NoParent,
    #[snafu(display("the application install dir is not valid UTF-8: {}", path.display()))]
    NotUtf8 { path: PathBuf },
}

#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum DiscoverError {
    #[snafu(display("the portable layout needs the application install dir"))]
    PortableInstallDir { source: InstallDirError },
    #[snafu(display("the platform has no default {root} dir"))]
    NoPlatformDefault { root: Root },
    #[snafu(display("the {root} dir is not valid UTF-8: {}", path.display()))]
    NotUtf8 { root: Root, path: PathBuf },
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to create dir {path}"))]
pub struct CreateDirError {
    pub path: Utf8PathBuf,
    pub source: io::Error,
}

#[cfg(windows)]
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum RegistryError {
    #[snafu(display("failed to open the registry key"))]
    Open { source: io::Error },
    #[snafu(display("failed to read the registry value"))]
    Read { source: io::Error },
    #[snafu(display("failed to write the registry value"))]
    Write { source: io::Error },
    #[snafu(display("the registry value is not valid UTF-8: {}", value.to_string_lossy()))]
    NotUtf8 { value: OsString },
}
