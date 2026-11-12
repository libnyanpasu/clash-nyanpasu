//! Discoverable, user-private endpoints shared by desktop, server and CLI.

use std::{
    io,
    path::{Path, PathBuf},
};

use interprocess::local_socket::{
    tokio::{Listener, Stream},
    traits::tokio::{Listener as _, Stream as _},
};
use sha2::{Digest as _, Sha256};
use snafu::{ResultExt as _, Snafu};
use tokio::task::JoinError;

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

/// One family of endpoints, such as the RPC listener. Endpoints of different
/// namespaces never share a location, so their owners never compete.
///
/// The name starts every Windows pipe name and every Unix socket directory of
/// the family, so changing it moves every endpoint of that family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Namespace(&'static str);

impl Namespace {
    /// The longest name that still leaves room for the user and directory identity.
    pub const MAX_LEN: usize = 32;

    /// # Panics
    /// Panics, at compile time in a `const`, unless the name has 1 to
    /// [`Self::MAX_LEN`] bytes of lowercase ASCII letters, digits and `-`.
    pub const fn new(name: &'static str) -> Self {
        let bytes = name.as_bytes();
        assert!(
            !bytes.is_empty() && bytes.len() <= Self::MAX_LEN,
            "namespace length"
        );
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            assert!(
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-',
                "namespace characters"
            );
            index += 1;
        }
        Self(name)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

/// A validated location, without a listener or connection owned by its discoverer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    namespace: Namespace,
    path: PathBuf,
}

impl Endpoint {
    /// Resolves one location per namespace, OS user and canonical, existing directory.
    ///
    /// An override changes only the location. Unix paths must be absolute and in a
    /// private directory; Windows paths must be in this user's named-pipe namespace.
    /// Performs filesystem inspection; call on a blocking thread in async hosts.
    ///
    /// # Errors
    /// Refuses unavailable directories and insecure or invalid locations.
    pub fn discover(
        namespace: Namespace,
        directory: &Path,
        location: Option<&Path>,
    ) -> Result<Self, EndpointError> {
        let directory = directory
            .canonicalize()
            .context(NormalizeSnafu { path: directory })?;
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
            None => platform::default_path(namespace, &user, &identity)?,
        };
        platform::validate(namespace, &path)?;
        Ok(Self { namespace, path })
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
        let namespace = self.namespace;
        let path = self.path.clone();
        let name = join_blocking(
            tokio::task::spawn_blocking(move || {
                platform::validate_connect(namespace, &path)?;
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
        let namespace = endpoint.namespace;
        let path = endpoint.path.clone();
        join_blocking(tokio::task::spawn_blocking(move || platform::bind(namespace, &path)).await)
    }

    /// Accepts the next stream, retaining the listener for further connections.
    ///
    /// # Errors
    /// Reports the OS accept error without treating the endpoint as stale.
    pub async fn accept(&self) -> Result<Stream, EndpointError> {
        self.listener.accept().await.context(AcceptSnafu)
    }
}

/// The result of a `spawn_blocking` task. A panic in it is a bug that must
/// interrupt the caller, so it is resumed instead of becoming an error.
pub(crate) fn join_blocking<T>(joined: Result<T, JoinError>) -> T {
    match joined {
        Ok(value) => value,
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            // Nothing aborts a blocking task, so only a dropped runtime cancels one.
            Err(error) => panic!("a blocking task was cancelled: {error}"),
        },
    }
}
