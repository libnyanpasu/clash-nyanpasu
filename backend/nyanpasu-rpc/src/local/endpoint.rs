//! Discoverable, user-private endpoints shared by desktop, server and CLI.

use std::{
    io,
    path::{Path, PathBuf},
};

use interprocess::local_socket::{
    tokio::{Listener, Stream},
    traits::tokio::{Listener as _, Stream as _},
};
use nyanpasu_owner_support::blocking;
use sha2::{Digest as _, Sha256};
use snafu::{ResultExt as _, Snafu};

#[cfg(any(unix, test))]
#[path = "endpoint/directory_chain.rs"]
mod directory_chain;
#[cfg(unix)]
#[path = "endpoint/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "endpoint/windows.rs"]
mod platform;
#[cfg(test)]
#[path = "endpoint/tests.rs"]
mod tests;

/// Failure to discover, claim or use a secure local endpoint.
#[derive(Debug, Snafu)]
pub enum EndpointError {
    #[snafu(display("failed to normalize app data directory {}", path.display()))]
    Normalize { path: PathBuf, source: io::Error },
    #[snafu(display("failed to identify the current OS user"))]
    CurrentUser { source: io::Error },
    #[snafu(display("invalid local endpoint {}", path.display()))]
    InvalidLocation { path: PathBuf },
    #[snafu(display("insecure local endpoint path {}", path.display()))]
    InsecurePath { path: PathBuf },
    #[snafu(display("failed to inspect local endpoint {}", path.display()))]
    Inspect { path: PathBuf, source: io::Error },
    #[snafu(display("another listener owns local endpoint {}", path.display()))]
    AlreadyServing { path: PathBuf },
    #[snafu(display("failed to claim local endpoint {}", path.display()))]
    Claim { path: PathBuf, source: io::Error },
    #[snafu(display("failed to reclaim local endpoint {}", path.display()))]
    Reclaim { path: PathBuf, source: io::Error },
    #[snafu(display("failed to restrict local endpoint access"))]
    AccessControl { source: io::Error },
    #[snafu(display("failed to bind local endpoint {}", path.display()))]
    Bind { path: PathBuf, source: io::Error },
    #[snafu(display("failed to accept a local connection"))]
    Accept { source: io::Error },
    #[snafu(display("failed to connect to local endpoint {}", path.display()))]
    Connect { path: PathBuf, source: io::Error },
}

/// A validated location, without a listener or connection owned by its discoverer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    path: PathBuf,
}

impl Endpoint {
    /// Resolves one location per OS user and canonical, existing app data directory.
    ///
    /// An override changes only the location. Unix paths must be absolute and in a
    /// private directory; Windows paths must be in this user's named-pipe namespace.
    /// Performs filesystem inspection; call on a blocking thread in async hosts.
    ///
    /// # Errors
    /// Refuses unavailable data directories and insecure or invalid locations.
    pub fn discover(data_directory: &Path, location: Option<&Path>) -> Result<Self, EndpointError> {
        let directory = data_directory.canonicalize().context(NormalizeSnafu {
            path: data_directory,
        })?;
        let user = platform::current_user()?;
        let mut digest = Sha256::new();
        digest.update(user.as_bytes());
        digest.update([0]);
        digest.update(directory.as_os_str().as_encoded_bytes());
        // 128 bits keep the Unix path short, including under macOS's /private/tmp.
        let identity: String = digest.finalize()[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let path = match location {
            Some(path) => path.to_owned(),
            None => platform::default_path(&user, &identity)?,
        };
        platform::validate_location(&path)?;
        Ok(Self { path })
    }

    /// The complete Unix socket path or Windows pipe path, suitable for `--endpoint`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens a byte stream at the same location used by the listener.
    ///
    /// # Errors
    /// Refuses invalid locations, insecure Unix paths and failed OS connections.
    pub async fn connect(&self) -> Result<Stream, EndpointError> {
        let path = self.path.clone();
        let name = blocking::join(
            tokio::task::spawn_blocking(move || {
                platform::validate_connect(&path)?;
                platform::socket_name(&path).context(ConnectSnafu { path })
            })
            .await,
        )?;
        Stream::connect(name)
            .await
            .context(ConnectSnafu { path: &self.path })
    }
}

/// A persistent acceptor; dropping it releases only its own endpoint claim.
#[derive(Debug)]
pub struct EndpointListener {
    // Close the listener before releasing the claim (fields drop in declaration order).
    listener: Listener,
    #[cfg(unix)]
    _claim: platform::Claim,
}

impl EndpointListener {
    /// Claims a location exclusively; this does not start a dispatcher or background server.
    ///
    /// # Errors
    /// A live owner, uncertain stale state or invalid permissions prevents binding.
    pub async fn bind(endpoint: &Endpoint) -> Result<Self, EndpointError> {
        let path = endpoint.path.clone();
        blocking::join(tokio::task::spawn_blocking(move || platform::bind(&path)).await)
    }

    /// Accepts the next stream, retaining the listener for further connections.
    ///
    /// # Errors
    /// Reports the OS accept error without treating the endpoint as stale.
    pub async fn accept(&self) -> Result<Stream, EndpointError> {
        self.listener.accept().await.context(AcceptSnafu)
    }
}
