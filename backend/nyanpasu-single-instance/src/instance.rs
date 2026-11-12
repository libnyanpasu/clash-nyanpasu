//! One process per user, namespace and directory, and a way for later
//! launches to hand their arguments to it.
//!
//! [`claim`] binds the instance endpoint of a directory. The first process gets
//! a [`Primary`]; every later one gets a [`Peer`], [notifies](Peer::notify) the
//! primary of its own launch and exits. The primary receives those launches
//! through [`Launches`], acknowledging each one as soon as it arrives, so a
//! peer never waits for the primary to finish starting.

use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::{ResultExt as _, Snafu};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

use crate::endpoint::{Endpoint, EndpointError, EndpointListener, Namespace};

#[cfg(test)]
#[path = "instance/tests.rs"]
mod tests;

/// The only wire version; a peer of another version is refused, not misread.
const VERSION: u8 = 1;
/// Far above a real command line; bounds what one connection can make us buffer.
const MAX_FRAME_LEN: u32 = 64 * 1024;
/// The IPC deadline of one hand-off, on both ends.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(5);

/// Failure to find or bind the instance endpoint for reasons other than a live owner.
#[derive(Debug, Snafu)]
pub enum ClaimError {
    #[snafu(display("failed to locate the instance endpoint for {}", root.display()))]
    Locate {
        root: PathBuf,
        source: EndpointError,
    },
    #[snafu(display("failed to bind the instance endpoint for {}", root.display()))]
    Bind {
        root: PathBuf,
        source: EndpointError,
    },
}

/// Failure to hand a launch to the primary instance.
#[derive(Debug, Snafu)]
pub enum NotifyError {
    #[snafu(display("failed to connect to the primary instance"))]
    Connect { source: EndpointError },
    #[snafu(display("failed to send the launch to the primary instance"))]
    Send { source: FrameError },
    #[snafu(display("failed to read the primary instance's acknowledgement"))]
    Receive { source: FrameError },
    #[snafu(display("the primary instance did not acknowledge within {timeout:?}"))]
    Timeout { timeout: Duration },
}

/// A malformed or failed frame exchange.
#[derive(Debug, Snafu)]
pub enum FrameError {
    #[snafu(display("failed to transfer a frame"))]
    Io { source: io::Error },
    #[snafu(display("unsupported frame version {found}"))]
    Version { found: u8 },
    #[snafu(display("frame of {len} bytes exceeds the {MAX_FRAME_LEN}-byte limit"))]
    TooLarge { len: usize },
    #[snafu(display("failed to encode a frame"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to decode a frame"))]
    Decode { source: serde_json::Error },
}

/// What a later launch hands to the primary instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Launch {
    /// Every argument, the executable first; arguments that are not Unicode are
    /// converted lossily.
    pub args: Vec<String>,
    /// The working directory of the launching process, converted lossily.
    pub cwd: Option<String>,
}

impl Launch {
    /// The arguments and working directory of this process.
    pub fn current() -> Self {
        Self {
            args: std::env::args_os()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            cwd: std::env::current_dir()
                .ok()
                .map(|cwd| cwd.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Ack {
    pid: u32,
}

/// The outcome of a [`claim`].
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "made once per process; only the Unix claim metadata is large, so expect would fail on Windows"
)]
pub enum Claim {
    /// This process owns the instance endpoint until the value is dropped.
    Primary(Primary),
    /// A live process owns the instance endpoint.
    AlreadyRunning(Peer),
}

/// Binds the instance endpoint of `root` in `namespace`, owned by the first
/// process of this user that binds it. `root` must exist; its canonical path
/// identifies the instance.
///
/// The claim neither waits nor retries: a peer that cannot reach the primary
/// decides whether to claim again.
///
/// # Errors
/// Fails when the endpoint cannot be located or bound for any reason other
/// than a live owner, such as an insecure directory.
pub async fn claim(namespace: Namespace, root: &Path) -> Result<Claim, ClaimError> {
    let owned = root.to_owned();
    let endpoint = crate::endpoint::join_blocking(
        tokio::task::spawn_blocking(move || Endpoint::discover(namespace, &owned, None)).await,
    )
    .context(LocateSnafu { root })?;
    match EndpointListener::bind(&endpoint).await {
        Ok(listener) => Ok(Claim::Primary(Primary { listener })),
        Err(EndpointError::AlreadyServing { .. }) => Ok(Claim::AlreadyRunning(Peer { endpoint })),
        Err(source) => Err(ClaimError::Bind {
            root: root.to_owned(),
            source,
        }),
    }
}

/// The owner of an instance endpoint. Dropping it releases the endpoint.
#[derive(Debug)]
pub struct Primary {
    listener: EndpointListener,
}

impl Primary {
    /// Starts accepting later launches on the current Tokio runtime.
    ///
    /// Each launch is acknowledged as soon as it is read, then queued until it is
    /// [received](Launches::recv). Dropping the returned value stops accepting;
    /// the endpoint is released once the runtime drops the aborted task, not
    /// within the drop itself. Drop the [`Primary`] instead where a later
    /// claim must succeed immediately.
    ///
    /// # Panics
    /// Panics when called outside a Tokio runtime.
    pub fn into_launches(self) -> Launches {
        let (sender, receiver) = mpsc::unbounded_channel();
        let task = tokio::spawn(accept_launches(self.listener, sender));
        Launches { receiver, task }
    }
}

/// Launches handed over by later processes, oldest first.
#[derive(Debug)]
pub struct Launches {
    receiver: mpsc::UnboundedReceiver<Launch>,
    task: JoinHandle<()>,
}

impl Launches {
    /// The next launch; `None` once accepting has stopped and the queue is empty.
    pub async fn recv(&mut self) -> Option<Launch> {
        self.receiver.recv().await
    }
}

impl Drop for Launches {
    fn drop(&mut self) {
        // The task owns the listener and every open connection.
        self.task.abort();
    }
}

async fn accept_launches(listener: EndpointListener, sender: mpsc::UnboundedSender<Launch>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    let sender = sender.clone();
                    connections.spawn(async move {
                        let result =
                            tokio::time::timeout(EXCHANGE_TIMEOUT, receive_launch(stream, &sender))
                                .await;
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::warn!(%error, "ignored a malformed instance launch");
                            }
                            Err(_) => tracing::warn!("an instance launch timed out"),
                        }
                    });
                }
                Err(error) => {
                    // The endpoint is unusable; later launches fail to connect and decide themselves.
                    tracing::error!(%error, "stopped accepting instance launches");
                    break;
                }
            },
            Some(joined) = connections.join_next() => {
                if let Err(error) = joined
                    && let Ok(panic) = error.try_into_panic()
                {
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
}

async fn receive_launch<S>(
    mut stream: S,
    sender: &mpsc::UnboundedSender<Launch>,
) -> Result<(), FrameError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let launch: Launch = read_frame(&mut stream).await?;
    // A closed receiver means the primary no longer takes launches; the peer is
    // still acknowledged, as the primary is alive and owns the endpoint.
    let _ = sender.send(launch);
    write_frame(
        &mut stream,
        &Ack {
            pid: std::process::id(),
        },
    )
    .await
}

/// A process that found the instance endpoint owned by another.
#[derive(Debug)]
pub struct Peer {
    endpoint: Endpoint,
}

/// The primary instance's reply to a launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Acknowledged {
    /// The process ID of the primary instance.
    pub primary_pid: u32,
}

impl Peer {
    /// Hands `launch` to the primary instance and waits up to five seconds for
    /// its acknowledgement.
    ///
    /// On Windows, the primary is then allowed to bring its window to the
    /// foreground, which Windows otherwise reserves for the launching process.
    ///
    /// # Errors
    /// Fails when the primary cannot be reached, misbehaves or does not answer
    /// in time; whether the launch reached the primary is then unknown.
    pub async fn notify(&self, launch: &Launch) -> Result<Acknowledged, NotifyError> {
        let acknowledged = notify_within(&self.endpoint, launch, EXCHANGE_TIMEOUT).await?;
        #[cfg(windows)]
        allow_foreground(acknowledged.primary_pid);
        Ok(acknowledged)
    }
}

async fn notify_within(
    endpoint: &Endpoint,
    launch: &Launch,
    timeout: Duration,
) -> Result<Acknowledged, NotifyError> {
    tokio::time::timeout(timeout, async {
        let mut stream = endpoint.connect().await.context(ConnectSnafu)?;
        write_frame(&mut stream, launch).await.context(SendSnafu)?;
        let ack: Ack = read_frame(&mut stream).await.context(ReceiveSnafu)?;
        Ok(Acknowledged {
            primary_pid: ack.pid,
        })
    })
    .await
    .map_err(|_| NotifyError::Timeout { timeout })?
}

#[cfg(windows)]
fn allow_foreground(pid: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    // SAFETY: AllowSetForegroundWindow takes a plain process ID and has no
    // memory preconditions; failure only leaves foreground rights unchanged.
    if unsafe { AllowSetForegroundWindow(pid) } == 0 {
        tracing::debug!(
            error = %io::Error::last_os_error(),
            pid,
            "could not let the primary instance take the foreground"
        );
    }
}

async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = serde_json::to_vec(value).context(EncodeSnafu)?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|len| *len <= MAX_FRAME_LEN)
        .ok_or(FrameError::TooLarge { len: body.len() })?;
    let mut frame = Vec::with_capacity(5 + body.len());
    frame.push(VERSION);
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&body);
    writer.write_all(&frame).await.context(IoSnafu)?;
    writer.flush().await.context(IoSnafu)
}

async fn read_frame<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let version = reader.read_u8().await.context(IoSnafu)?;
    snafu::ensure!(version == VERSION, VersionSnafu { found: version });
    let len = reader.read_u32().await.context(IoSnafu)?;
    snafu::ensure!(len <= MAX_FRAME_LEN, TooLargeSnafu { len: len as usize });
    let mut body = vec![0; len as usize];
    reader.read_exact(&mut body).await.context(IoSnafu)?;
    serde_json::from_slice(&body).context(DecodeSnafu)
}
