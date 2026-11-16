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
    user: String,
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

    /// Validated host user identity enforcing same-user access on this listener (UID decimal on
    /// Unix, SID on Windows).
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Accepts the next verified same-user stream, retaining the listener for further connections.
    ///
    /// Unauthorized or unverifiable peers are logged and skipped without failing the listener.
    ///
    /// # Errors
    /// Reports the OS accept error without treating the endpoint as stale.
    pub async fn accept(&self) -> Result<Stream, EndpointError> {
        accept_verified(&self.listener, platform::verify_peer).await
    }
}

async fn accept_verified(
    listener: &Listener,
    mut verify: impl FnMut(&Stream) -> Result<(), PeerRejection>,
) -> Result<Stream, EndpointError> {
    loop {
        let stream = listener.accept().await.context(AcceptSnafu)?;
        match verify(&stream) {
            Ok(()) => return Ok(stream),
            Err(rejection) => {
                tracing::warn!(%rejection, "rejected unauthorized local endpoint peer");
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PeerMismatch {
    MissingIdentity,
    #[cfg(any(unix, test))]
    Uid {
        peer: u32,
        expected: u32,
    },
    #[cfg(any(windows, test))]
    Sid {
        peer: String,
        expected: String,
    },
}

impl std::fmt::Display for PeerMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingIdentity => {
                formatter.write_str("OS peer credentials did not include a peer identity")
            }
            #[cfg(any(unix, test))]
            Self::Uid { peer, expected } => {
                write!(
                    formatter,
                    "peer UID {peer} does not match expected UID {expected}"
                )
            }
            #[cfg(any(windows, test))]
            Self::Sid { peer, expected } => {
                write!(
                    formatter,
                    "peer SID {peer} does not match expected SID {expected}"
                )
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum PeerRejection {
    Credentials(io::Error),
    #[cfg(windows)]
    Process(io::Error),
    #[cfg(windows)]
    CurrentUser(io::Error),
    Mismatch(PeerMismatch),
}

impl std::fmt::Display for PeerRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Credentials(error) => {
                write!(formatter, "failed to read peer credentials: {error}")
            }
            #[cfg(windows)]
            Self::Process(error) => {
                write!(formatter, "failed to inspect peer process token: {error}")
            }
            #[cfg(windows)]
            Self::CurrentUser(error) => {
                write!(formatter, "failed to read current user SID: {error}")
            }
            Self::Mismatch(mismatch) => std::fmt::Display::fmt(mismatch, formatter),
        }
    }
}

#[cfg(any(unix, test))]
pub(super) fn verify_uid(peer: Option<u32>, expected: u32) -> Result<(), PeerMismatch> {
    match peer {
        Some(uid) if uid == expected => Ok(()),
        Some(uid) => Err(PeerMismatch::Uid {
            peer: uid,
            expected,
        }),
        None => Err(PeerMismatch::MissingIdentity),
    }
}

#[cfg(any(windows, test))]
pub(super) fn verify_sid(peer: Option<&str>, expected: &str) -> Result<(), PeerMismatch> {
    match peer {
        Some(sid) if !expected.is_empty() && sid == expected => Ok(()),
        Some(sid) => Err(PeerMismatch::Sid {
            peer: sid.to_owned(),
            expected: expected.to_owned(),
        }),
        None => Err(PeerMismatch::MissingIdentity),
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
